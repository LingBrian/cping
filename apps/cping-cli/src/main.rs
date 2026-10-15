//! CPing 无头命令行入口。
//!
//! 与 TUI / GUI 共用同一个 `cping-core`：目标解析、调度、分类、统计、导出
//! 全部走核心，CLI 里**不含任何业务规则**。
//!
//! 退出码见 [`args`] 模块；这是给脚本与 CI 用的契约，改动属于破坏性变更。

mod args;

use args::{Args, EXIT_BACKEND, EXIT_OFFLINE, EXIT_OK, EXIT_USAGE};
use cping_core::classify::Thresholds;
use cping_core::config::ProbeConfig;
use cping_core::enrich::enrich_with_mac_and_vendor;
use cping_core::export::{ExportFormat, ExportMeta, ScanReport};
use cping_core::monitor::{diff_scans, Change, ChangeLog, DEFAULT_CHANGE_CAPACITY};
use cping_core::result::{ProbeResult, ScanStats};
use cping_core::scheduler::ProbeScheduler;
use cping_core::spec::{parse_spec, TargetSpec};
use cping_core::timeseries::TimeSeries;
use cping_core::{backend_for_run, config::ColorConfig};
use std::time::{Duration, Instant};

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(run(&argv));
}

fn run(argv: &[String]) -> i32 {
    let parsed = match Args::parse(argv) {
        Ok(a) => a,
        Err(msg) => {
            eprintln!("参数错误：{msg}");
            eprintln!("\n{}", args::HELP);
            return EXIT_USAGE;
        }
    };
    if parsed.help {
        print!("{}", args::HELP);
        return EXIT_OK;
    }
    if parsed.version {
        println!(
            "cping-cli {} · {} · 后端 {}",
            cping_core::VERSION,
            std::env::consts::OS,
            parsed.mode.backend_name()
        );
        return EXIT_OK;
    }
    // M12：打印后端能力后退出。放在参数校验之前——这个命令不需要目标。
    if parsed.list_backends {
        print_backends();
        return EXIT_OK;
    }
    if let Err(msg) = parsed.validate() {
        eprintln!("参数错误：{msg}");
        return EXIT_USAGE;
    }

    let spec = match parse_spec(&parsed.target_spec, parsed.range) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("目标规格错误：{e}");
            return EXIT_USAGE;
        }
    };

    let mut session = match Session::new(&parsed, &spec) {
        Ok(s) => s,
        Err(code) => return code,
    };
    session.execute()
}

/// 一次 CLI 会话（可能包含多轮 watch）。
struct Session {
    args: Args,
    spec: TargetSpec,
    scheduler: ProbeScheduler,
    previous: Vec<ProbeResult>,
    changes: ChangeLog,
    round: u64,
    last_stats: ScanStats,
    /// M14：跨会话的长期时序（`--history` 载入并回写，`--report` 据此出报表）。
    series: TimeSeries,
}

impl Session {
    fn new(args: &Args, spec: &TargetSpec) -> Result<Self, i32> {
        if args.watch_seconds.is_some() && args.rounds.is_none() {
            eprintln!(
                "提示：持续监控模式不会自动停止（每 {} 秒一轮），按 Ctrl+C 结束",
                args.watch_seconds.unwrap_or(0)
            );
        }
        if spec.len() > 1024 && args.watch_seconds.is_some() {
            eprintln!(
                "警告：{} 个目标进入持续监控，每轮都会重新扫描，请确认网络负载可接受",
                spec.len()
            );
        }
        // M14：长期时序。`--history` 指向的文件若已存在就**续写**（跨会话累积），
        // 这样「连续观察一周的可用率」才是真的连续，而不是每次开机重来。
        let mut series = match &args.history {
            Some(path) => match TimeSeries::load_from_path(path) {
                Ok((s, skipped)) => {
                    if skipped > 0 {
                        eprintln!(
                            "警告：历史文件 {path} 有 {skipped} 行无法解析，已跳过（其余数据继续使用）"
                        );
                    }
                    if !args.quiet && !s.is_empty() {
                        println!(
                            "已载入历史 {} 点 · {} 轮（{path}）",
                            s.len(),
                            s.total_rounds()
                        );
                    }
                    s
                }
                Err(e) => {
                    // 文件**不存在**是首次使用的正常情况；文件存在却读不动（版本不符、
                    // 不是 CPing 时序）就必须拒绝——否则下一轮写回时会把用户的历史覆盖掉。
                    if std::path::Path::new(path).exists() {
                        eprintln!("历史文件无法使用：{e}");
                        return Err(EXIT_USAGE);
                    }
                    TimeSeries::new()
                }
            },
            None => TimeSeries::new(),
        };
        series.retention_secs = args.keep_days * 86_400;
        Ok(Self {
            args: args.clone(),
            spec: spec.clone(),
            // M12：后端选择统一走核心注册表。CLI 面向自动化，**不提供** mock —
            // 无头命令行里混进模拟数据只会让脚本的退出码失去意义。
            scheduler: ProbeScheduler::new(backend_for_run(args.mode, false)),
            previous: Vec::new(),
            changes: ChangeLog::new(DEFAULT_CHANGE_CAPACITY),
            round: 0,
            last_stats: ScanStats::default(),
            series,
        })
    }

    fn execute(&mut self) -> i32 {
        let thresholds = Thresholds::default();
        if self.round == 1 && !self.args.quiet {
            println!(
                "CPing {} · {} · {}",
                cping_core::VERSION,
                std::env::consts::OS,
                self.args.describe()
            );
        }
        loop {
            self.round += 1;
            let outcome = match self.scan_once(&thresholds) {
                Ok(v) => v,
                Err(code) => return code,
            };
            self.last_stats = outcome.stats.clone();

            if !self.args.quiet {
                print_round(
                    self.round,
                    &self.spec,
                    &self.args,
                    &outcome,
                    &outcome.changes,
                );
            }

            if let Some((format, path)) = &self.args.out {
                if let Err(e) = self.write_report(*format, path, &outcome) {
                    eprintln!("导出失败：{e}");
                    return EXIT_USAGE;
                }
                if !self.args.quiet {
                    println!("  已写出 {path}");
                }
            }

            // M14：并入长期时序。每轮**立即落盘**——Ctrl+C 或掉电时已经跑过的轮次
            // 不该丢，这是「长期观察」的基本要求。
            if self.args.history.is_some() || self.args.report.is_some() {
                self.series.append_results(now_ms(), &outcome.results);
            }
            if let Some(path) = &self.args.history {
                if let Err(e) = self.series.save_to_path(path) {
                    eprintln!("历史写入失败：{e}");
                    return EXIT_USAGE;
                }
                if !self.args.quiet {
                    println!(
                        "  历史：{} 点 · {} 轮 · 保留 {} 天 → {path}",
                        self.series.len(),
                        self.series.total_rounds(),
                        self.series.retention_secs / 86_400
                    );
                }
            }

            self.previous = outcome.results;
            self.changes.extend(outcome.changes);

            if let Some(total) = self.args.rounds {
                if self.round >= total {
                    break;
                }
            }
            match self.args.watch_seconds {
                None => break,
                Some(secs) => std::thread::sleep(Duration::from_secs(secs)),
            }
        }

        // M14：报表在**所有轮次之后**生成——它描述的是整个过程，而不是某一轮。
        if let Some(path) = &self.args.report {
            if self.series.is_empty() {
                eprintln!("报表：没有可用的历史数据，已跳过 {path}");
            } else if let Err(e) = self.write_uptime_report(path) {
                eprintln!("报表写入失败：{e}");
                return EXIT_USAGE;
            } else if !self.args.quiet {
                println!("可用率报表（{} 台主机）→ {path}", self.series.hosts().len());
            }
        }

        // 退出码：后端整体不可用 > 有离线 > 全在线
        let stats = &self.last_stats;
        if stats.total > 0
            && stats.online == 0
            && stats.permission_denied + stats.unsupported == stats.total
        {
            eprintln!(
                "后端不可用：{} 个目标全部「权限不足/不支持」。{}",
                stats.total,
                cping_core::native_icmp_hint()
            );
            return EXIT_BACKEND;
        }
        if stats.online == stats.total && stats.total > 0 {
            EXIT_OK
        } else {
            EXIT_OFFLINE
        }
    }

    fn scan_once(&self, thresholds: &Thresholds) -> Result<RoundOutcome, i32> {
        let config = ProbeConfig {
            prefix: None,
            range: self.args.range,
            timeout_ms: self.args.timeout_ms,
            concurrency: self.args.concurrency,
            thresholds: *thresholds,
            mode: self.args.mode,
        };
        let started = Instant::now();
        let handle = match self.scheduler.start(&config, self.spec.targets.clone()) {
            Ok(h) => h,
            Err(e) => {
                eprintln!("无法启动扫描：{e}");
                return Err(EXIT_USAGE);
            }
        };
        let outcome = handle.run_to_completion();
        let mut results = outcome.results;

        if self.args.enrich {
            let report = enrich_with_mac_and_vendor(&mut results);
            if !self.args.quiet {
                println!(
                    "  补全：MAC {} 条 · 厂商 {} 条（邻居表 {} 条）",
                    report.mac_filled, report.vendor_filled, report.entries
                );
            }
        }

        // M13：端口指纹只针对**在线**主机，且与在线判定分列。
        // 端口全关不会把主机改成离线；这里也不改 results 里的任何字段。
        let fingerprints = if self.args.fingerprint {
            let online: Vec<(std::net::IpAddr, Option<String>)> = results
                .iter()
                .filter(|r| r.status.is_online())
                .map(|r| (r.ip, r.vendor.clone()))
                .collect();
            if online.is_empty() {
                if !self.args.quiet {
                    println!("  端口指纹：没有在线主机，跳过");
                }
                Vec::new()
            } else {
                let cfg = self.args.fingerprint_config();
                let fps = cping_core::fingerprint_hosts(&online, &cfg);
                if !self.args.quiet {
                    let open_total: usize = fps.iter().map(|f| f.report.findings.len()).sum();
                    println!(
                        "  端口指纹：{} 台在线主机 × {} 个端口 · 发现 {} 个开放端口",
                        fps.len(),
                        cfg.ports.len(),
                        open_total
                    );
                }
                fps
            }
        } else {
            Vec::new()
        };

        let changes = diff_scans(&self.previous, &results, thresholds, self.round);
        let stats = ScanStats::from_results_with_total(&results, self.spec.len());
        Ok(RoundOutcome {
            results,
            stats,
            elapsed_ms: outcome.elapsed_ms.max(started.elapsed().as_millis() as u64),
            cancelled: outcome.cancelled,
            changes,
            fingerprints,
        })
    }

    fn write_report(
        &self,
        format: ExportFormat,
        path: &str,
        outcome: &RoundOutcome,
    ) -> Result<(), String> {
        let meta = ExportMeta {
            scan_time: now_text(),
            local_ip: String::new(),
            elapsed_ms: outcome.elapsed_ms,
            mode: self.args.mode,
            target_desc: self.spec.summary(),
            round: self.round,
        };
        let report = ScanReport::new(
            &outcome.results,
            meta,
            Thresholds::default(),
            outcome.changes.clone(),
        )
        .with_fingerprints(outcome.fingerprints.clone());
        let bytes = report
            .render(format, &ColorConfig::default())
            .map_err(|e| e.to_string())?;
        std::fs::write(path, bytes).map_err(|e| e.to_string())
    }

    /// M14：写出可用率报表。格式由扩展名决定（参数解析阶段已限定 .csv/.html）。
    fn write_uptime_report(&self, path: &str) -> Result<(), String> {
        let report = self.series.report();
        let bytes = if path.to_ascii_lowercase().ends_with(".html") {
            cping_core::report_html(&self.series, &report).into_bytes()
        } else {
            cping_core::report_csv(&report)
        };
        std::fs::write(path, bytes).map_err(|e| format!("写入 {path} 失败：{e}"))
    }
}

struct RoundOutcome {
    results: Vec<ProbeResult>,
    stats: ScanStats,
    elapsed_ms: u64,
    cancelled: bool,
    changes: Vec<Change>,
    /// M13：端口指纹（独立于 results，不参与在线判定）。
    fingerprints: Vec<cping_core::HostFingerprint>,
}

fn print_round(
    round: u64,
    spec: &TargetSpec,
    args: &Args,
    outcome: &RoundOutcome,
    changes: &[Change],
) {
    let stats = &outcome.stats;
    println!(
        "第 {round} 轮 · {} · {} · 用时 {:.3} s{}",
        args.mode.describe(),
        spec.summary(),
        outcome.elapsed_ms as f64 / 1000.0,
        if outcome.cancelled {
            "（已取消）"
        } else {
            ""
        }
    );
    println!(
        "  目标 {} · 在线 {} · 超时 {} · 不可达 {} · 权限 {} · 不支持 {} · 错误 {}",
        stats.total,
        stats.online,
        stats.timeout,
        stats.unreachable,
        stats.permission_denied,
        stats.unsupported,
        stats.error
    );

    if !changes.is_empty() {
        println!("  变化 {} 项：", changes.len());
        for c in changes.iter().take(20) {
            println!("    {}", c.summary());
        }
        if changes.len() > 20 {
            println!("    … 另有 {} 项，见导出文件", changes.len() - 20);
        }
    }

    if stats.online > 0 {
        println!("  在线主机：");
        for r in outcome.results.iter().filter(|r| r.status.is_online()) {
            let rtt = r.status.rtt_ms().unwrap_or(0);
            let vendor = if r.vendor_or_empty().is_empty() {
                String::new()
            } else {
                format!("  {}", r.vendor_or_empty())
            };
            let host = if r.hostname_or_empty().is_empty() {
                String::new()
            } else {
                format!("  {}", r.hostname_or_empty())
            };
            println!(
                "    {:<15} {:>6} ms  {}{}{}",
                r.ip,
                rtt,
                args.mode.status_label(&r.status),
                vendor,
                host
            );
        }
    }

    // M13：端口指纹单独一段，标题写明与在线判定无关
    if !outcome.fingerprints.is_empty() {
        println!("  端口指纹（附加信息，与上面的在线判定相互独立）：");
        for fp in &outcome.fingerprints {
            if !fp.has_open_ports() {
                println!("    {:<15} 无开放端口", fp.ip());
                continue;
            }
            println!("    {:<15} {}", fp.ip(), fp.open_ports_summary());
            for f in &fp.report.findings {
                if let Some(banner) = &f.banner {
                    println!("    {:<15}   :{} {}", "", f.port, banner);
                }
            }
        }
        for fp in outcome.fingerprints.iter().filter(|f| f.device.is_some()) {
            println!("    {} → {}", fp.ip(), fp.device_summary());
        }
    }
}

/// M12：列出本平台的后端能力。
/// 刻意把**不可用**的后端也打出来（并写明原因）：「ICMPv6 未实装」是用户需要
/// 知道的事实，藏起来只会让人对着清一色的「不支持」猜原因。
fn print_backends() {
    let reg = cping_core::BackendRegistry::new();
    println!(
        "cping-cli {} · {} · {}",
        cping_core::VERSION,
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    println!("{}", reg.summary_zh());
    println!();
    println!(
        "{:<8} {:<22} {:<8} {:<14} 说明",
        "ID", "名称", "可用", "地址族"
    );
    for caps in reg.caps() {
        let family = match (caps.ipv4, caps.ipv6) {
            (true, true) => "IPv4+IPv6",
            (true, false) => "仅 IPv4",
            (false, true) => "仅 IPv6",
            (false, false) => "—",
        };
        println!(
            "{:<8} {:<22} {:<8} {:<14} {}",
            caps.id.key(),
            caps.id.label_zh(),
            if caps.available { "是" } else { "否" },
            family,
            if caps.note.is_empty() {
                if caps.needs_privilege {
                    "需要原始套接字权限"
                } else {
                    "—"
                }
            } else {
                caps.note
            }
        );
    }
    println!();
    println!("提示：IPv6 目标目前请用 --mode tcp（ICMPv6 尚未实装）。");
}

fn now_text() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_unix_local(secs)
}

/// 当前 Unix 毫秒（M14 时序点的时间戳）。取不到时退化为 0，
/// 调用端会把它当作「时间未知」——而不是伪造一个看起来合理的数字。
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 把 Unix 秒转成 `YYYY-MM-DD HH:MM:SS`（本地时区偏移由系统给出）。
fn format_unix_local(unix_secs: u64) -> String {
    // 计算本地偏移的简易做法：用 libc 之外的路径不可得，这里退化为 UTC，
    // 并额外打印偏移不可知这一事实——绝不假装是本地时间。
    let days = unix_secs / 86_400;
    let rem = unix_secs % 86_400;
    let (y, m, d) = civil_from_days(days as i64);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Howard Hinnant 的 `civil_from_days`（1970-01-01 起的天数 → 年月日）。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cping_core::result::ProbeStatus;
    use cping_core::spec::ProbeMode;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn civil_from_days_matches_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(1), (1970, 1, 2));
        assert_eq!(civil_from_days(364), (1970, 12, 31));
        assert_eq!(civil_from_days(365), (1971, 1, 1));
        // 2026-10-15 = 20741 天（用 python 独立核算，不靠心算）
        assert_eq!(civil_from_days(20_741), (2026, 10, 15));
        assert_eq!(civil_from_days(20_740), (2026, 10, 14));
        // 闰年边界
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(11_017), (2000, 3, 1));
    }

    #[test]
    fn timestamp_is_marked_as_utc_not_local() {
        let text = format_unix_local(0);
        assert_eq!(text, "1970-01-01 00:00:00 UTC");
        assert!(format_unix_local(1_700_000_000).ends_with(" UTC"));
    }

    #[test]
    fn print_round_reports_tcp_labels_for_tcp_mode() {
        let args = Args {
            mode: ProbeMode::Tcp { port: 80 },
            quiet: false,
            ..Args::default()
        };
        let spec = parse_spec("10.0.0.1", cping_core::TargetRange::Standard).unwrap();
        let outcome = RoundOutcome {
            results: vec![ProbeResult::new(
                0,
                IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
                ProbeStatus::Online { rtt_ms: 1 },
            )],
            stats: ScanStats::from_results(&[ProbeResult::new(
                0,
                IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
                ProbeStatus::Online { rtt_ms: 1 },
            )]),
            elapsed_ms: 12,
            cancelled: false,
            changes: Vec::new(),
            fingerprints: Vec::new(),
        };
        // 只要不 panic 即可；内容由手工冒烟确认（见 docs/testing.md §M10）
        print_round(1, &spec, &args, &outcome, &[]);
    }
}
