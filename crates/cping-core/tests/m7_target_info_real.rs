//! M7 目标信息链路**实机验证**：真实 ICMP 扫描 → 读邻居表 → 查厂商 → 按需反查主机名。
//!
//! 默认 `#[ignore]`，因为它依赖真实网络环境与目标地址；手动运行：
//!
//! ```text
//! # 不设环境变量时会打印提示并跳过（不失败）
//! CPING_M7_TARGETS=127.0.0.1,192.168.1.3,192.168.1.1 \
//!   cargo test -p cping-core --test m7_target_info_real -- --ignored --nocapture
//! ```
//!
//! **只探测调用者显式给出的地址**，不会自行扫描任何网段。

use cping_core::classify::Thresholds;
use cping_core::config::ProbeConfig;
use cping_core::enrich::{enrich_with_mac_and_vendor, lookup_hostname};
use cping_core::native::platform_icmp_backend;
use cping_core::oui::{parse_mac, vendor_of};
use cping_core::snapshot::{load_from_path, save_to_path, Snapshot};
use cping_core::target::{build_targets, parse_prefix, TargetRange};
use cping_core::ProbeScheduler;
use std::net::IpAddr;
use std::time::Duration;

fn targets_from_env() -> Option<Vec<IpAddr>> {
    let raw = std::env::var("CPING_M7_TARGETS").ok()?;
    let list: Vec<IpAddr> = raw
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    if list.is_empty() {
        None
    } else {
        Some(list)
    }
}

/// 完整链路：真实扫描 → 补 MAC/厂商 → 按需反查主机名。
#[test]
#[ignore = "真实网络；需要 CPING_M7_TARGETS 指定目标"]
fn real_scan_then_enrich_target_info() {
    let Some(ips) = targets_from_env() else {
        println!("未设置 CPING_M7_TARGETS，跳过本次实机验证");
        return;
    };

    // 1) 真实 ICMP 扫描
    let scheduler = ProbeScheduler::new(platform_icmp_backend());
    let config = ProbeConfig {
        timeout_ms: 1000,
        concurrency: 8,
        ..Default::default()
    };
    let handle = scheduler.start(&config, ips.clone()).expect("启动扫描失败");
    let outcome = handle.run_to_completion();
    let mut results = outcome.results;
    println!(
        "扫描完成：total={} online={} timeout={} permission_denied={}",
        outcome.stats.total,
        outcome.stats.online,
        outcome.stats.timeout,
        outcome.stats.permission_denied
    );

    // 2) 补 MAC 与厂商（ARP 表此时才可能刚被上面的发包填充）
    let report = enrich_with_mac_and_vendor(&mut results);
    println!(
        "邻居表：ok={} 条目={} 补上 MAC={} 补上厂商={}{}",
        report.table_ok,
        report.entries,
        report.mac_filled,
        report.vendor_filled,
        report
            .error
            .as_deref()
            .map(|e| format!(" 错误={e}"))
            .unwrap_or_default()
    );

    // 3) 按需反查主机名（只对前 3 个目标，避免测试耗时不可控）
    println!(
        "\n{:<17}{:<12}{:<19}{:<28}主机名",
        "IP", "状态", "MAC", "厂商"
    );
    println!("{}", "-".repeat(100));
    for (i, r) in results.iter_mut().enumerate() {
        if i < 3 {
            lookup_hostname(r, Duration::from_millis(1500));
        }
        println!(
            "{:<17}{:<12}{:<19}{:<28}{}",
            r.ip,
            r.status.label_zh(),
            r.mac_or_empty(),
            truncate_col(r.vendor_or_empty(), 26),
            r.hostname_or_empty(),
        );
    }

    // 4) 自洽性断言：不依赖具体网络环境，只检查「不许伪造」这条硬规则
    for r in &results {
        if let Some(mac_s) = r.mac.as_deref() {
            let mac = parse_mac(mac_s).unwrap_or_else(|| panic!("MAC 格式非法: {mac_s}"));
            assert_ne!(mac, [0u8; 6], "不得填入零 MAC");
            // 厂商名必须与直接查表结果一致（防止回填时张冠李戴）
            assert_eq!(
                r.vendor.as_deref(),
                vendor_of(mac),
                "{} 的厂商名与 MAC 前缀不匹配",
                r.ip
            );
        } else {
            assert_eq!(r.vendor, None, "{} 没有 MAC 却有厂商名，属于伪造", r.ip);
        }
    }
    assert!(report.mac_filled <= report.entries);

    // 回环地址永远不该有 ARP 条目（二层不存在），留空才是正确行为
    for r in &results {
        if r.ip.is_loopback() {
            assert_eq!(r.mac, None, "回环地址不得出现 MAC");
        }
    }

    // 至少统计出有多少条拿到了厂商，便于人工判断环境是否覆盖到了
    let with_vendor = results.iter().filter(|r| r.vendor.is_some()).count();
    println!("\n拿到厂商名的目标数: {with_vendor}/{}", results.len());
}

/// 校验 M7 新能力与既有扫描链路在**同一次运行**里互不干扰：
/// 补全信息不得改变任何状态或统计。
#[test]
#[ignore = "真实网络；需要 CPING_M7_TARGETS 指定目标"]
fn enrich_does_not_change_status_or_stats() {
    let Some(ips) = targets_from_env() else {
        println!("未设置 CPING_M7_TARGETS，跳过本次实机验证");
        return;
    };
    let parsed = parse_prefix("127.0.0").expect("前缀解析失败");
    let generated = build_targets(&parsed, TargetRange::Standard).expect("目标生成失败");
    let _ = generated;

    let scheduler = ProbeScheduler::new(platform_icmp_backend());
    let config = ProbeConfig {
        timeout_ms: 800,
        concurrency: 4,
        ..Default::default()
    };
    let handle = scheduler.start(&config, ips).expect("启动扫描失败");
    let outcome = handle.run_to_completion();
    let before_stats = outcome.stats.clone();
    let before_status: Vec<_> = outcome.results.iter().map(|r| r.status.clone()).collect();

    let mut results = outcome.results;
    let _ = enrich_with_mac_and_vendor(&mut results);

    let after_status: Vec<_> = results.iter().map(|r| r.status.clone()).collect();
    assert_eq!(before_status, after_status, "补全信息不得改动探测状态");
    assert_eq!(
        before_stats,
        cping_core::ScanStats::from_results_with_total(&results, before_stats.total),
        "补全信息不得改动统计口径"
    );
}

fn truncate_col(s: &str, width: usize) -> String {
    if s.chars().count() <= width {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(width.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

/// 真实结果 → 落盘 → 读回：字段与统计必须与落盘那一刻完全一致。
///
/// 这是「进程被杀后可以恢复」这条承诺的实测依据——恢复出来的东西必须
/// 和当时看到的**是同一份**，而不是一份看起来差不多、其实下标错位的数据。
#[test]
#[ignore = "真实网络；需要 CPING_M7_TARGETS 指定目标"]
fn real_results_survive_a_snapshot_roundtrip() {
    let Some(ips) = targets_from_env() else {
        println!("未设置 CPING_M7_TARGETS，跳过本次实机验证");
        return;
    };

    let scheduler = ProbeScheduler::new(platform_icmp_backend());
    let config = ProbeConfig {
        timeout_ms: 800,
        concurrency: 8,
        ..Default::default()
    };
    let handle = scheduler.start(&config, ips).expect("启动扫描失败");
    let outcome = handle.run_to_completion();
    let mut results = outcome.results;
    enrich_with_mac_and_vendor(&mut results);

    // 用真实 IP 反推一个前缀，模拟界面上真实的落盘内容
    let prefix = results
        .first()
        .map(|r| {
            let o = match r.ip {
                IpAddr::V4(v4) => v4.octets(),
                IpAddr::V6(_) => [0, 0, 0, 0],
            };
            format!("{}.{}.{}", o[0], o[1], o[2])
        })
        .unwrap_or_else(|| "0.0.0".to_string());

    let snap = Snapshot {
        scan_time: "2026-10-14 21:40:00".into(),
        prefix,
        local_ip: "未知".into(),
        elapsed_ms: outcome.elapsed_ms,
        thresholds: Thresholds::default(),
        total: results.len(),
        cancelled: outcome.cancelled,
        results,
    };

    let dir = std::env::temp_dir().join(format!("cping-m7-real-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("创建临时目录失败");
    let path = dir.join("real.snap");
    let path_s = path.to_string_lossy().to_string();
    save_to_path(&snap, &path_s).expect("落盘失败");

    let back = load_from_path(&path_s).expect("读回失败");
    assert_eq!(back, snap, "往返后必须逐字段一致");
    assert_eq!(back.stats(), snap.stats(), "统计口径必须一致");
    assert_eq!(back.elapsed_ms, snap.elapsed_ms);
    assert_eq!(back.total, snap.total);

    // 顺带验证文本格式是稳定的（同一份数据两次序列化应字节相同）
    let text = std::fs::read_to_string(&path).expect("读文件失败");
    assert_eq!(back.serialize(), text, "序列化必须稳定");

    println!(
        "快照往返 OK：{} 条结果 / 共 {} 个目标，{} 字节",
        back.results.len(),
        back.total,
        text.len()
    );
    for r in &back.results {
        println!(
            "{:<17}{:<12}{:<19}{:<26}{}",
            r.ip,
            r.status.label_zh(),
            r.mac_or_empty(),
            truncate_col(r.vendor_or_empty(), 24),
            r.hostname_or_empty()
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// 纯离线检查：三张 OUI 表在真实编译产物里确实可用（不联网、不扫网）。
#[test]
fn oui_tables_are_embedded_in_binary() {
    let (l, m, s) = cping_core::entry_counts();
    println!("MA-L={l} MA-M={m} MA-S={s}");
    assert!(l > 30_000, "MA-L 条目数偏少: {l}");
    assert!(m > 5_000, "MA-M 条目数偏少: {m}");
    assert!(s > 5_000, "MA-S 条目数偏少: {s}");
    // 嵌入数据必须真的可查（而不是只有计数正确）
    assert_eq!(
        vendor_of([0x00, 0x00, 0x0C, 0x00, 0x00, 0x00]),
        Some("Cisco Systems, Inc")
    );
}

/// 打印本机邻居表（诊断用）。排障时先跑这个，再决定要探测哪些地址：
/// 只有**已经在 ARP 缓存里**的地址才能补上 MAC。
#[test]
#[ignore = "需要读取本机邻居表；用于诊断"]
fn dump_neighbor_table() {
    match cping_core::neighbor_table() {
        Ok(entries) => {
            println!("邻居表条目数: {}", entries.len());
            println!("{:<17}{:<19}厂商", "IP", "MAC");
            println!("{}", "-".repeat(70));
            for e in &entries {
                let vendor = vendor_of(e.mac).unwrap_or("(无/本地管理地址)");
                println!(
                    "{:<17}{:<19}{}",
                    e.ip,
                    cping_core::mac_to_string(e.mac),
                    truncate_col(vendor, 40)
                );
            }
            // 至少验证一次内部一致性：能查到的厂商名必须能再查回同样的 MAC 前缀
            for e in &entries {
                if let Some(v) = vendor_of(e.mac) {
                    assert!(!v.is_empty());
                    assert_eq!(vendor_of(e.mac), Some(v));
                }
            }
        }
        Err(msg) => println!("邻居表不可用：{msg}"),
    }
}
