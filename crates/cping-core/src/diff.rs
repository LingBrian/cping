//! 两次扫描的对比（M10）。
//!
//! 与 [`crate::monitor::diff_scans`] 的分工：
//!
//! | | `monitor::diff_scans` | `diff::diff_reports` |
//! | --- | --- | --- |
//! | 用途 | 实时监控的**事件流**（高亮、通知） | 两份结果的**对比报告**（归档、导出） |
//! | 关注 | 「变了什么」 | 「从什么变成了什么」，含 RTT 前后值与差值 |
//! | 目标集合变化 | 忽略 | 显式区分「只在旧结果里」「只在新结果里」 |
//!
//! 两者都是纯函数：给定同一对输入必然得到同一输出，不依赖时钟与全局状态。

use crate::classify::{classify_rtt, LatencyClass, Thresholds};
use crate::error::CoreResult;
use crate::export::csv_escape;
use crate::i18n::{self, Key, Lang};
use crate::result::{ProbeResult, ProbeStatus};
use std::collections::HashMap;
use std::net::IpAddr;

/// 单条差异的种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiffKind {
    /// 旧结果没有、新结果在线。
    Appeared,
    /// 旧结果在线、新结果不在线。
    Disappeared,
    /// 前后都在线，RTT 档位变化。
    LatencyChanged,
    /// 前后都不在线，非在线状态不同。
    StatusChanged,
    /// 只出现在旧结果里（本次没扫它）。
    OnlyInBase,
    /// 只出现在新结果里（上次没扫它）。
    OnlyInNext,
}

impl DiffKind {
    pub const ALL: [DiffKind; 6] = [
        DiffKind::Appeared,
        DiffKind::Disappeared,
        DiffKind::LatencyChanged,
        DiffKind::StatusChanged,
        DiffKind::OnlyInBase,
        DiffKind::OnlyInNext,
    ];

    pub const fn key(self) -> &'static str {
        match self {
            DiffKind::Appeared => "appeared",
            DiffKind::Disappeared => "disappeared",
            DiffKind::LatencyChanged => "latency_changed",
            DiffKind::StatusChanged => "status_changed",
            DiffKind::OnlyInBase => "only_in_base",
            DiffKind::OnlyInNext => "only_in_next",
        }
    }

    /// 中文标签（等价于 `label_in(Lang::Zh)`）。
    ///
    /// 核心层内部的报表与导出仍在用它——**产物内容是数据契约**：
    /// 同一份 CSV / HTML 在中英文机器上必须一致；只有界面跟着语言变。
    pub fn label_zh(self) -> &'static str {
        self.label_in(Lang::Zh)
    }

    /// 按语言渲染的标签。
    pub fn label_in(self, lang: Lang) -> &'static str {
        i18n::text(
            lang,
            match self {
                DiffKind::Appeared => Key::LabelDiffAppeared,
                DiffKind::Disappeared => Key::LabelDiffDisappeared,
                DiffKind::LatencyChanged => Key::LabelDiffLatencyChanged,
                DiffKind::StatusChanged => Key::LabelDiffStatusChanged,
                DiffKind::OnlyInBase => Key::LabelDiffOnlyInBase,
                DiffKind::OnlyInNext => Key::LabelDiffOnlyInNext,
            },
        )
    }

    /// 是否是「值得关注」的变化（用于统计与高亮）。
    pub const fn is_change(self) -> bool {
        matches!(
            self,
            DiffKind::Appeared | DiffKind::Disappeared | DiffKind::LatencyChanged
        )
    }
}

/// 一条差异记录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffEntry {
    pub ip: IpAddr,
    pub kind: DiffKind,
    pub base: Option<ProbeStatus>,
    pub next: Option<ProbeStatus>,
}

impl DiffEntry {
    pub fn base_rtt(&self) -> Option<u32> {
        self.base.as_ref().and_then(|s| s.rtt_ms())
    }

    pub fn next_rtt(&self) -> Option<u32> {
        self.next.as_ref().and_then(|s| s.rtt_ms())
    }

    /// RTT 差值（新 − 旧）。两侧都有 RTT 时才有效。
    pub fn delta_ms(&self) -> Option<i64> {
        Some(self.next_rtt()? as i64 - self.base_rtt()? as i64)
    }

    /// 带符号的差值文本，例如 `+12 ms`；无意义时返回 `-`。
    pub fn delta_text(&self) -> String {
        match self.delta_ms() {
            Some(d) if d > 0 => format!("+{d} ms"),
            Some(d) => format!("{d} ms"),
            None => "-".to_string(),
        }
    }

    fn base_label(&self) -> String {
        self.base
            .as_ref()
            .map(|s| s.label_zh().to_string())
            .unwrap_or_else(|| "-".into())
    }

    fn next_label(&self) -> String {
        self.next
            .as_ref()
            .map(|s| s.label_zh().to_string())
            .unwrap_or_else(|| "-".into())
    }
}

/// 两份扫描结果的完整对比。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScanDiff {
    pub base_total: usize,
    pub next_total: usize,
    /// 按 IP 数值升序。
    pub entries: Vec<DiffEntry>,
    /// 前后都在线且 RTT 档位未变的目标数。
    pub unchanged: usize,
}

impl ScanDiff {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn count_of(&self, kind: DiffKind) -> usize {
        self.entries.iter().filter(|e| e.kind == kind).count()
    }

    /// 全部变化条目数（不含「仅旧/仅新」）。
    pub fn change_count(&self) -> usize {
        self.entries.iter().filter(|e| e.kind.is_change()).count()
    }

    /// 一行摘要，供界面标题与 CLI 输出。
    pub fn summary(&self) -> String {
        format!(
            "新上线 {} · 已离线 {} · 延迟变化 {} · 状态变化 {}（另：仅旧 {} / 仅新 {}）",
            self.count_of(DiffKind::Appeared),
            self.count_of(DiffKind::Disappeared),
            self.count_of(DiffKind::LatencyChanged),
            self.count_of(DiffKind::StatusChanged),
            self.count_of(DiffKind::OnlyInBase),
            self.count_of(DiffKind::OnlyInNext),
        )
    }

    /// 差异报告 CSV（UTF-8 BOM + CRLF，与主导出格式一致）。
    pub fn to_csv(&self) -> CoreResult<Vec<u8>> {
        let mut text = String::new();
        text.push_str("IP地址,差异,旧状态,新状态,旧RTT(ms),新RTT(ms),RTT差值");
        text.push_str("\r\n");
        for e in &self.entries {
            let fields = [
                e.ip.to_string(),
                e.kind.label_zh().to_string(),
                e.base_label(),
                e.next_label(),
                e.base_rtt().map(|v| v.to_string()).unwrap_or_default(),
                e.next_rtt().map(|v| v.to_string()).unwrap_or_default(),
                e.delta_ms().map(|d| d.to_string()).unwrap_or_default(),
            ];
            let line: Vec<String> = fields.iter().map(|f| csv_escape(f)).collect();
            text.push_str(&line.join(","));
            text.push_str("\r\n");
        }
        let mut bytes = Vec::with_capacity(3 + text.len());
        bytes.extend_from_slice(&crate::export::CSV_BOM);
        bytes.extend_from_slice(text.as_bytes());
        Ok(bytes)
    }
}

/// 比较两份结果集。
///
/// 顺序规则：先按 IP 数值升序；同一 IP 只出现一条（不会既「上线」又「离线」）。
pub fn diff_reports(
    base: &[ProbeResult],
    next: &[ProbeResult],
    thresholds: &Thresholds,
) -> ScanDiff {
    let mut base_map: HashMap<IpAddr, &ProbeStatus> = HashMap::with_capacity(base.len());
    for r in base {
        base_map.insert(r.ip, &r.status);
    }
    let mut next_map: HashMap<IpAddr, &ProbeStatus> = HashMap::with_capacity(next.len());
    for r in next {
        next_map.insert(r.ip, &r.status);
    }

    let mut ips: Vec<IpAddr> = base_map.keys().copied().collect();
    for ip in next_map.keys() {
        if !base_map.contains_key(ip) {
            ips.push(*ip);
        }
    }
    ips.sort_by_key(|ip| addr_sort_key(*ip));

    let mut entries = Vec::new();
    let mut unchanged = 0usize;
    for ip in ips {
        let b = base_map.get(&ip).copied();
        let n = next_map.get(&ip).copied();
        match (b, n) {
            (None, Some(next_status)) => entries.push(DiffEntry {
                ip,
                kind: DiffKind::OnlyInNext,
                base: None,
                next: Some(next_status.clone()),
            }),
            (Some(base_status), None) => entries.push(DiffEntry {
                ip,
                kind: DiffKind::OnlyInBase,
                base: Some(base_status.clone()),
                next: None,
            }),
            (Some(base_status), Some(next_status)) => {
                let kind = match (base_status.is_online(), next_status.is_online()) {
                    (false, true) => Some(DiffKind::Appeared),
                    (true, false) => Some(DiffKind::Disappeared),
                    (true, true) => {
                        let a = classify_rtt(base_status.rtt_ms().unwrap_or(0), thresholds);
                        let c = classify_rtt(next_status.rtt_ms().unwrap_or(0), thresholds);
                        if a != c {
                            Some(DiffKind::LatencyChanged)
                        } else {
                            unchanged += 1;
                            None
                        }
                    }
                    (false, false) => {
                        if std::mem::discriminant(base_status)
                            != std::mem::discriminant(next_status)
                        {
                            Some(DiffKind::StatusChanged)
                        } else {
                            unchanged += 1;
                            None
                        }
                    }
                };
                if let Some(kind) = kind {
                    entries.push(DiffEntry {
                        ip,
                        kind,
                        base: Some(base_status.clone()),
                        next: Some(next_status.clone()),
                    });
                }
            }
            (None, None) => unreachable!("IP 至少来自其中一侧"),
        }
    }

    ScanDiff {
        base_total: base.len(),
        next_total: next.len(),
        entries,
        unchanged,
    }
}

/// 把对比结果转成一行行文本（CLI 与 TUI 共用）。
pub fn diff_lines(diff: &ScanDiff) -> Vec<String> {
    diff.entries
        .iter()
        .map(|e| {
            format!(
                "{:<15} {:<8} {} -> {}  {}",
                e.ip,
                e.kind.label_zh(),
                e.base_label(),
                e.next_label(),
                e.delta_text()
            )
        })
        .collect()
}

/// 延迟档位的排序权重（报表按「变化幅度」排序时用）。
pub fn latency_rank(class: LatencyClass) -> u8 {
    match class {
        LatencyClass::Low => 0,
        LatencyClass::Mid => 1,
        LatencyClass::High => 2,
    }
}

// 排序口径只在 `crate::spec::addr_sort_key` 定义一次（v4 在前、同族按数值）。
use crate::spec::addr_sort_key;

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    /// 测试用：取 IPv4 的第 4 段（测试数据都是 v4）。
    fn last4(ip: IpAddr) -> u8 {
        match ip {
            IpAddr::V4(v4) => v4.octets()[3],
            IpAddr::V6(v6) => v6.octets()[15],
        }
    }

    fn res(host: u8, status: ProbeStatus) -> ProbeResult {
        ProbeResult::new(
            host as u16,
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, host)),
            status,
        )
    }

    fn online(ms: u32) -> ProbeStatus {
        ProbeStatus::Online { rtt_ms: ms }
    }

    #[test]
    fn detects_all_three_change_kinds() {
        let base = vec![
            res(1, ProbeStatus::Timeout),
            res(2, online(10)),
            res(3, online(10)),
        ];
        let next = vec![
            res(1, online(5)),
            res(2, ProbeStatus::Timeout),
            res(3, online(200)),
        ];
        let d = diff_reports(&base, &next, &Thresholds::default());
        assert_eq!(d.entries.len(), 3);
        assert_eq!(d.count_of(DiffKind::Appeared), 1);
        assert_eq!(d.count_of(DiffKind::Disappeared), 1);
        assert_eq!(d.count_of(DiffKind::LatencyChanged), 1);
        assert_eq!(d.change_count(), 3);
    }

    #[test]
    fn identical_scans_produce_no_entries() {
        let base = vec![res(1, online(5)), res(2, ProbeStatus::Timeout)];
        let d = diff_reports(&base, &base, &Thresholds::default());
        assert!(d.is_empty());
        assert_eq!(d.unchanged, 2);
        assert_eq!(d.change_count(), 0);
    }

    #[test]
    fn jitter_within_the_same_class_is_not_a_change() {
        let base = vec![res(1, online(20))];
        let next = vec![res(1, online(30))];
        let d = diff_reports(&base, &next, &Thresholds::default());
        assert!(d.is_empty());
        assert_eq!(d.unchanged, 1);
    }

    #[test]
    fn target_set_differences_are_reported_separately() {
        let base = vec![res(1, online(5)), res(9, online(5))];
        let next = vec![res(1, online(5)), res(7, online(5))];
        let d = diff_reports(&base, &next, &Thresholds::default());
        assert_eq!(d.count_of(DiffKind::OnlyInBase), 1);
        assert_eq!(d.count_of(DiffKind::OnlyInNext), 1);
        assert_eq!(d.change_count(), 0, "集合差异不算变化");
    }

    #[test]
    fn status_change_among_offline_states_is_detected() {
        let base = vec![res(1, ProbeStatus::Timeout)];
        let next = vec![res(1, ProbeStatus::PermissionDenied)];
        let d = diff_reports(&base, &next, &Thresholds::default());
        assert_eq!(d.entries.len(), 1);
        assert_eq!(d.entries[0].kind, DiffKind::StatusChanged);
    }

    #[test]
    fn entries_are_sorted_by_numeric_ip() {
        let base = vec![res(200, ProbeStatus::Timeout), res(3, ProbeStatus::Timeout)];
        let next = vec![res(200, online(1)), res(3, online(1))];
        let d = diff_reports(&base, &next, &Thresholds::default());
        let order: Vec<u8> = d.entries.iter().map(|e| last4(e.ip)).collect();
        assert_eq!(order, vec![3, 200]);
    }

    #[test]
    fn delta_is_signed_and_only_valid_when_both_sides_have_rtt() {
        let base = vec![res(1, online(50)), res(2, ProbeStatus::Timeout)];
        let next = vec![res(1, online(20)), res(2, online(30))];
        let d = diff_reports(&base, &next, &Thresholds::default());
        let faster = &d.entries[0];
        assert_eq!(faster.delta_ms(), Some(-30));
        assert_eq!(faster.delta_text(), "-30 ms");
        let appeared = &d.entries[1];
        assert_eq!(appeared.delta_ms(), None);
        assert_eq!(appeared.delta_text(), "-");
    }

    #[test]
    fn csv_starts_with_bom_and_has_one_row_per_entry() {
        let base = vec![res(1, ProbeStatus::Timeout)];
        let next = vec![res(1, online(4))];
        let d = diff_reports(&base, &next, &Thresholds::default());
        let bytes = d.to_csv().unwrap();
        assert_eq!(&bytes[..3], &crate::export::CSV_BOM);
        let text = String::from_utf8(bytes[3..].to_vec()).unwrap();
        assert!(text.starts_with("IP地址,差异,"));
        assert_eq!(text.matches("\r\n").count(), 2, "表头 + 1 行");
    }

    #[test]
    fn summary_mentions_every_kind() {
        let base = vec![res(1, ProbeStatus::Timeout), res(9, online(1))];
        let next = vec![res(1, online(3)), res(7, online(3))];
        let text = diff_reports(&base, &next, &Thresholds::default()).summary();
        assert!(text.contains("新上线 1"), "{text}");
        assert!(text.contains("仅旧 1"), "{text}");
        assert!(text.contains("仅新 1"), "{text}");
    }

    #[test]
    fn diff_lines_are_human_readable_and_aligned() {
        let base = vec![res(1, ProbeStatus::Timeout)];
        let next = vec![res(1, online(4))];
        let lines = diff_lines(&diff_reports(&base, &next, &Thresholds::default()));
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("10.0.0.1"), "{}", lines[0]);
        assert!(lines[0].contains("新上线"), "{}", lines[0]);
    }

    #[test]
    fn empty_inputs_are_handled() {
        let d = diff_reports(&[], &[], &Thresholds::default());
        assert!(d.is_empty());
        assert_eq!(d.base_total, 0);
        assert_eq!(d.next_total, 0);
        let d2 = diff_reports(&[], &[res(1, online(1))], &Thresholds::default());
        assert_eq!(d2.count_of(DiffKind::OnlyInNext), 1);
    }

    #[test]
    fn latency_rank_orders_low_to_high() {
        assert!(latency_rank(LatencyClass::Low) < latency_rank(LatencyClass::Mid));
        assert!(latency_rank(LatencyClass::Mid) < latency_rank(LatencyClass::High));
    }

    #[test]
    fn diff_kind_labels_and_keys_are_complete() {
        for k in DiffKind::ALL {
            assert!(!k.key().is_empty());
            assert!(!k.label_zh().is_empty());
        }
        assert!(DiffKind::Appeared.is_change());
        assert!(!DiffKind::OnlyInBase.is_change());
    }
}
