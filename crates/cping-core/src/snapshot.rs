//! 扫描结果快照：落盘与恢复（M7）。
//!
//! ## 为什么需要它
//!
//! Android 上 Activity 被销毁会**带动整个进程退出**（`tao` 在
//! `platform_impl/android/mod.rs` 里直接 `process::exit`，详见
//! `docs/platform-compatibility.md` §3.5）。进程一退，内存里的结果就没了。
//! 因此需要在合适的时机把结果落成文件，重启后由用户决定是否恢复。
//!
//! ## 格式（刻意保持简单、可手查、零依赖）
//!
//! 纯文本、UTF-8、`\n` 换行、制表符分隔，首行是固定签名：
//!
//! ```text
//! CPING-SNAPSHOT<TAB>1
//! time<TAB>2026-10-14 21:40:00
//! prefix<TAB>192.168.1
//! local_ip<TAB>192.168.1.5
//! elapsed_ms<TAB>8270
//! thresholds<TAB>50<TAB>100
//! total<TAB>255
//! cancelled<TAB>0
//! rows<TAB>3
//! 1<TAB>192.168.1.1<TAB>permission_denied<TAB><TAB><TAB><TAB><TAB>
//! ...
//! ```
//!
//! 行记录固定 8 列：`index, ip, status, rtt_ms, note, mac, vendor, hostname`。
//! 缺失字段写空串，**不写占位文本**。
//!
//! ## 解析策略：宁可报错，不要猜
//!
//! - 签名或版本不认识 → 直接失败（不尝试兼容旧/未知格式）；
//! - 未知状态关键字、行内列数不符、行数少于 `rows` 声明 → 失败；
//! - 多余的空行允许；文件末尾无换行允许。
//!
//! 这样界面才能诚实地说「快照已损坏」而不是恢复出一份看似正常、实则错位的数据。

use crate::classify::Thresholds;
use crate::error::{CoreError, CoreResult};
use crate::result::{ProbeResult, ProbeStatus, ScanStats};
use std::net::Ipv4Addr;

/// 签名首字段。
pub const SIGNATURE: &str = "CPING-SNAPSHOT";
/// 当前格式版本。
pub const FORMAT_VERSION: u32 = 1;
/// 行记录列数。
const ROW_COLUMNS: usize = 8;

/// 一份可落盘的扫描结果快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// 扫描开始时间（调用方格式化，仅作文本保存与展示）。
    pub scan_time: String,
    /// 网段前缀，例如 `192.168.1`。
    pub prefix: String,
    pub local_ip: String,
    pub elapsed_ms: u64,
    pub thresholds: Thresholds,
    /// 本次扫描的**目标总数**（含未完成的目标）。
    pub total: usize,
    /// 是否由用户取消（恢复后界面状态要与当时一致）。
    pub cancelled: bool,
    /// 已完成的结果，顺序不限；未出现在这里的下标即「未完成」。
    pub results: Vec<ProbeResult>,
}

impl Snapshot {
    /// 统计口径与实时扫描完全一致（同一个 `from_results_with_total`）。
    pub fn stats(&self) -> ScanStats {
        ScanStats::from_results_with_total(&self.results, self.total)
    }

    /// 序列化为文本（不含文件系统操作）。
    pub fn serialize(&self) -> String {
        let mut out = String::with_capacity(128 + self.results.len() * 48);
        out.push_str(&format!("{SIGNATURE}\t{FORMAT_VERSION}\n"));
        out.push_str(&format!("time\t{}\n", sanitize(&self.scan_time)));
        out.push_str(&format!("prefix\t{}\n", sanitize(&self.prefix)));
        out.push_str(&format!("local_ip\t{}\n", sanitize(&self.local_ip)));
        out.push_str(&format!("elapsed_ms\t{}\n", self.elapsed_ms));
        out.push_str(&format!(
            "thresholds\t{}\t{}\n",
            self.thresholds.low_max_ms, self.thresholds.mid_max_ms
        ));
        out.push_str(&format!("total\t{}\n", self.total));
        out.push_str(&format!(
            "cancelled\t{}\n",
            if self.cancelled { 1 } else { 0 }
        ));
        out.push_str(&format!("rows\t{}\n", self.results.len()));
        for r in &self.results {
            let rtt = r.status.rtt_ms().map(|v| v.to_string()).unwrap_or_default();
            let note = r.status.note().unwrap_or("").to_string();
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                r.index,
                r.ip,
                r.status.key(),
                rtt,
                sanitize(&note),
                sanitize(r.mac.as_deref().unwrap_or("")),
                sanitize(r.vendor.as_deref().unwrap_or("")),
                sanitize(r.hostname.as_deref().unwrap_or("")),
            ));
        }
        out
    }

    /// 从文本解析；任何不一致都返回错误，不做猜测性修补。
    pub fn parse(text: &str) -> CoreResult<Snapshot> {
        let mut lines = text.lines();

        // 1) 签名与版本
        let sig = lines.next().unwrap_or_default();
        let mut sig_parts = sig.split('\t');
        if sig_parts.next() != Some(SIGNATURE) {
            return Err(CoreError::Snapshot(
                "文件签名不匹配，不是 CPing 快照".into(),
            ));
        }
        let version: u32 = sig_parts
            .next()
            .and_then(|v| v.trim().parse().ok())
            .ok_or_else(|| CoreError::Snapshot("缺少快照版本号".into()))?;
        if version != FORMAT_VERSION {
            return Err(CoreError::Snapshot(format!(
                "快照版本 {version} 不受支持（当前支持 {FORMAT_VERSION}）"
            )));
        }

        // 2) 头部键值，直到 rows
        let mut scan_time = String::new();
        let mut prefix = String::new();
        let mut local_ip = String::new();
        let mut elapsed_ms = 0u64;
        let mut thresholds = Thresholds::default();
        let mut total = 0usize;
        let mut cancelled = false;
        let mut rows = 0usize;

        for line in lines.by_ref() {
            if line.trim().is_empty() {
                continue;
            }
            let mut it = line.split('\t');
            let key = it.next().unwrap_or_default();
            match key {
                "time" => scan_time = it.next().unwrap_or_default().to_string(),
                "prefix" => prefix = it.next().unwrap_or_default().to_string(),
                "local_ip" => local_ip = it.next().unwrap_or_default().to_string(),
                "elapsed_ms" => {
                    elapsed_ms = it
                        .next()
                        .and_then(|v| v.trim().parse().ok())
                        .ok_or_else(|| CoreError::Snapshot("elapsed_ms 字段非法".into()))?;
                }
                "thresholds" => {
                    let low = it.next().and_then(|v| v.trim().parse().ok());
                    let mid = it.next().and_then(|v| v.trim().parse().ok());
                    match (low, mid) {
                        (Some(low), Some(mid)) => {
                            thresholds = Thresholds::new(low, mid);
                        }
                        _ => {
                            return Err(CoreError::Snapshot("thresholds 字段非法".into()));
                        }
                    }
                }
                "total" => {
                    total = it
                        .next()
                        .and_then(|v| v.trim().parse().ok())
                        .ok_or_else(|| CoreError::Snapshot("total 字段非法".into()))?;
                }
                "cancelled" => {
                    cancelled = match it.next().map(str::trim) {
                        Some("1") => true,
                        Some("0") | None => false,
                        Some(other) => {
                            return Err(CoreError::Snapshot(format!(
                                "cancelled 字段非法: {other:?}"
                            )));
                        }
                    };
                }
                "rows" => {
                    rows = it
                        .next()
                        .and_then(|v| v.trim().parse().ok())
                        .ok_or_else(|| CoreError::Snapshot("rows 字段非法".into()))?;
                    break;
                }
                other => {
                    return Err(CoreError::Snapshot(format!("未知头部字段 {other:?}")));
                }
            }
        }

        // 3) 行记录
        let mut results = Vec::with_capacity(rows);
        for line in lines {
            if line.trim().is_empty() {
                continue;
            }
            let cols: Vec<&str> = line.split('\t').collect();
            if cols.len() != ROW_COLUMNS {
                return Err(CoreError::Snapshot(format!(
                    "行记录应有 {ROW_COLUMNS} 列，实际 {} 列",
                    cols.len()
                )));
            }
            results.push(parse_row(&cols)?);
        }
        if results.len() != rows {
            return Err(CoreError::Snapshot(format!(
                "声明 {rows} 行，实际解析到 {} 行",
                results.len()
            )));
        }

        Ok(Snapshot {
            scan_time,
            prefix,
            local_ip,
            elapsed_ms,
            thresholds,
            total,
            cancelled,
            results,
        })
    }
}

fn parse_row(cols: &[&str]) -> CoreResult<ProbeResult> {
    let index: u16 = cols[0]
        .trim()
        .parse()
        .map_err(|_| CoreError::Snapshot(format!("下标非法: {:?}", cols[0])))?;
    let ip: Ipv4Addr = cols[1]
        .trim()
        .parse()
        .map_err(|_| CoreError::Snapshot(format!("IP 非法: {:?}", cols[1])))?;

    let rtt: Option<u32> = if cols[3].trim().is_empty() {
        None
    } else {
        Some(
            cols[3]
                .trim()
                .parse()
                .map_err(|_| CoreError::Snapshot(format!("RTT 非法: {:?}", cols[3])))?,
        )
    };

    let status = match cols[2].trim() {
        "pending" => ProbeStatus::Pending,
        "online" => ProbeStatus::Online {
            rtt_ms: rtt
                .ok_or_else(|| CoreError::Snapshot(format!("下标 {index} 状态为在线但没有 RTT")))?,
        },
        "timeout" => ProbeStatus::Timeout,
        "unreachable" => ProbeStatus::Unreachable,
        "permission_denied" => ProbeStatus::PermissionDenied,
        "unsupported" => ProbeStatus::Unsupported,
        "error" => ProbeStatus::Error(cols[4].to_string()),
        other => {
            return Err(CoreError::Snapshot(format!("未知状态 {other:?}")));
        }
    };

    Ok(ProbeResult {
        index,
        ip,
        status,
        mac: none_if_empty(cols[5]),
        vendor: none_if_empty(cols[6]),
        hostname: none_if_empty(cols[7]),
    })
}

fn none_if_empty(s: &str) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

/// 制表符与换行会破坏行结构，落盘前统一替换为空格（**有损但安全**）。
fn sanitize(s: &str) -> String {
    if s.contains(['\t', '\n', '\r']) {
        s.replace(['\t', '\n', '\r'], " ")
    } else {
        s.to_string()
    }
}

/// 写到文件（UTF-8）。父目录不存在时报错，由调用方负责创建。
pub fn save_to_path(snapshot: &Snapshot, path: &str) -> CoreResult<()> {
    std::fs::write(path, snapshot.serialize())
        .map_err(|e| CoreError::Snapshot(format!("写入 {path} 失败：{e}")))
}

/// 从文件读取并解析。
pub fn load_from_path(path: &str) -> CoreResult<Snapshot> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| CoreError::Snapshot(format!("读取 {path} 失败：{e}")))?;
    Snapshot::parse(&text)
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Snapshot {
        let results = vec![
            ProbeResult {
                index: 0,
                ip: Ipv4Addr::new(192, 168, 1, 1),
                status: ProbeStatus::Online { rtt_ms: 12 },
                mac: Some("AA:BB:CC:DD:EE:01".into()),
                vendor: Some("Example Vendor".into()),
                hostname: Some("router.local".into()),
            },
            ProbeResult::new(
                1,
                Ipv4Addr::new(192, 168, 1, 2),
                ProbeStatus::PermissionDenied,
            ),
            ProbeResult::new(
                7,
                Ipv4Addr::new(192, 168, 1, 8),
                ProbeStatus::Error("内部错误：socket 创建失败".into()),
            ),
        ];
        Snapshot {
            scan_time: "2026-10-14 21:40:00".into(),
            prefix: "192.168.1".into(),
            local_ip: "192.168.1.5".into(),
            elapsed_ms: 8270,
            thresholds: Thresholds::new(50, 100),
            total: 255,
            cancelled: true,
            results,
        }
    }

    #[test]
    fn roundtrip_preserves_everything() {
        let original = sample();
        let text = original.serialize();
        let restored = Snapshot::parse(&text).expect("应能解析自己写出的快照");
        assert_eq!(restored, original, "往返后所有字段必须完全一致");
        // 统计口径必须一致（验收要求：total/online/elapsed 与快照时刻完全相同）
        assert_eq!(restored.stats(), original.stats());
        assert_eq!(restored.stats().total, 255);
        assert_eq!(restored.stats().online, 1);
        assert_eq!(restored.stats().pending, 252);
        assert_eq!(restored.elapsed_ms, original.elapsed_ms);
        // 再序列化一次应字节相同（稳定格式）
        assert_eq!(restored.serialize(), text);
    }

    #[test]
    fn empty_scan_roundtrip() {
        let s = Snapshot {
            scan_time: "2026-10-14 21:00:00".into(),
            prefix: "10.0.0".into(),
            local_ip: String::new(),
            elapsed_ms: 0,
            thresholds: Thresholds::default(),
            total: 255,
            cancelled: false,
            results: Vec::new(),
        };
        let restored = Snapshot::parse(&s.serialize()).unwrap();
        assert_eq!(restored, s);
        assert_eq!(restored.stats().pending, 255);
        assert_eq!(restored.stats().completed, 0);
    }

    #[test]
    fn rejects_wrong_signature_and_unknown_version() {
        assert!(Snapshot::parse("").is_err(), "空文件必须失败");
        assert!(Snapshot::parse("HELLO\t1\n").is_err());
        assert!(Snapshot::parse("CPING-SNAPSHOT\t2\n").is_err());
        assert!(Snapshot::parse("CPING-SNAPSHOT\n").is_err(), "缺版本号");
        let err = Snapshot::parse("CPING-SNAPSHOT\t99\n").unwrap_err();
        assert!(format!("{err}").contains("99"), "错误信息应说明版本");
    }

    #[test]
    fn rejects_unknown_status_key() {
        let text = "\
CPING-SNAPSHOT\t1
total\t1
rows\t1
0\t192.168.1.1\tsuper_online\t5\t\t\t\t
";
        let err = Snapshot::parse(text).unwrap_err();
        assert!(format!("{err}").contains("super_online"));
    }

    #[test]
    fn rejects_row_column_mismatch() {
        let text = "\
CPING-SNAPSHOT\t1
total\t1
rows\t1
0\t192.168.1.1\ttimeout
";
        assert!(Snapshot::parse(text).is_err(), "列数不足必须失败");
    }

    #[test]
    fn rejects_row_count_mismatch() {
        // 声明 2 行却只给 1 行：宁可报损坏，也不给出一份缺数据的结果
        let text = "\
CPING-SNAPSHOT\t1
total\t2
rows\t2
0\t192.168.1.1\ttimeout\t\t\t\t\t
";
        let err = Snapshot::parse(text).unwrap_err();
        assert!(format!("{err}").contains("实际解析到 1 行"));
    }

    #[test]
    fn rejects_online_without_rtt() {
        let text = "\
CPING-SNAPSHOT\t1
total\t1
rows\t1
0\t192.168.1.1\tonline\t\t\t\t\t
";
        assert!(Snapshot::parse(text).is_err(), "在线却没有 RTT 是坏数据");
    }

    #[test]
    fn rejects_bad_numbers_and_ip() {
        for bad in [
            "CPING-SNAPSHOT\t1\nelapsed_ms\tabc\n",
            "CPING-SNAPSHOT\t1\nthresholds\t50\n",
            "CPING-SNAPSHOT\t1\ncancelled\tyes\n",
            "CPING-SNAPSHOT\t1\nunknown_key\t1\n",
            "CPING-SNAPSHOT\t1\nrows\t1\n0\tnot-an-ip\ttimeout\t\t\t\t\t\n",
            "CPING-SNAPSHOT\t1\nrows\t1\nx\t192.168.1.1\ttimeout\t\t\t\t\t\n",
        ] {
            assert!(Snapshot::parse(bad).is_err(), "应拒绝: {bad:?}");
        }
    }

    #[test]
    fn tolerates_blank_lines_and_missing_final_newline() {
        let text = "\
CPING-SNAPSHOT\t1
total\t1

rows\t1

0\t192.168.1.9\ttimeout\t\t\t\t\t";
        let s = Snapshot::parse(text).expect("空行与末尾无换行都应容忍");
        assert_eq!(s.results.len(), 1);
        assert_eq!(s.total, 1);
    }

    #[test]
    fn tab_and_newline_in_note_do_not_break_format() {
        let mut s = sample();
        s.results[2].status = ProbeStatus::Error("带\t制表符\n和换行".into());
        let text = s.serialize();
        let restored = Snapshot::parse(&text).unwrap();
        // 换行/制表符被替换为空格，语义保留、结构不破
        assert_eq!(restored.results[2].status.note(), Some("带 制表符 和换行"));
        assert_eq!(restored.results.len(), s.results.len());
    }

    #[test]
    fn optional_fields_stay_none_not_empty_string() {
        let s = sample();
        let restored = Snapshot::parse(&s.serialize()).unwrap();
        // 无 MAC 的行必须仍是 None（不能被恢复成 Some("")）
        assert_eq!(restored.results[1].mac, None);
        assert_eq!(restored.results[1].vendor, None);
        assert_eq!(restored.results[1].hostname, None);
    }

    #[test]
    fn file_roundtrip_on_disk() {
        let dir = std::env::temp_dir().join(format!("cping-snap-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("snap.txt");
        let path_s = path.to_string_lossy().to_string();

        let s = sample();
        save_to_path(&s, &path_s).unwrap();
        let loaded = load_from_path(&path_s).unwrap();
        assert_eq!(loaded, s);

        // 损坏文件必须报错而不是给出半份数据
        std::fs::write(&path, "garbage").unwrap();
        assert!(load_from_path(&path_s).is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
