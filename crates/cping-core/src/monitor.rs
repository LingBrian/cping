//! 持续监控（M8）：轮次语义、两轮之间的**纯函数**变化检测、变化事件环形缓冲。
//!
//! 设计原则：
//! - 变化检测是 `diff_scans(prev, next, thresholds, round)` 一个纯函数，
//!   不持有任何状态，也不碰时钟——重放同一对快照必须得到同一结果。
//! - **首轮不产生变化事件**：`prev` 为空时基线尚未建立，
//!   把所有在线主机报成「新出现」是噪音而不是信息。
//! - 事件日志有容量上限（[`ChangeLog`]），长跑（数小时轮询）不得无限增长内存。
//! - 「在线 / 离线」只看 [`ProbeStatus::is_online`]；延迟档位沿用
//!   [`crate::classify`] 的阈值，**不在这里另立一套分档**。

use crate::classify::{classify_rtt, LatencyClass, Thresholds};
use crate::i18n::{self, Key, Lang};
use crate::result::{ProbeResult, ProbeStatus};
use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;

/// 默认重扫间隔：30 秒。
pub const DEFAULT_INTERVAL_MS: u64 = 30_000;
/// 间隔下限：低于 5 秒会在多数网络上被当作端口扫描。
pub const MIN_INTERVAL_MS: u64 = 5_000;
/// 间隔上限：1 小时。
pub const MAX_INTERVAL_MS: u64 = 3_600_000;

/// 变化事件日志的默认容量。
pub const DEFAULT_CHANGE_CAPACITY: usize = 1000;

/// 持续监控配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitorConfig {
    pub enabled: bool,
    /// 两次重扫之间的间隔（毫秒）。
    pub interval_ms: u64,
    /// 主机上线时是否提示。
    pub notify_appeared: bool,
    /// 主机离线时是否提示。
    pub notify_disappeared: bool,
    /// 变化日志容量上限。
    pub capacity: usize,
}

impl Default for MonitorConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_ms: DEFAULT_INTERVAL_MS,
            notify_appeared: true,
            notify_disappeared: true,
            capacity: DEFAULT_CHANGE_CAPACITY,
        }
    }
}

impl MonitorConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.interval_ms < MIN_INTERVAL_MS {
            return Err(format!("重扫间隔不得小于 {MIN_INTERVAL_MS} ms"));
        }
        if self.interval_ms > MAX_INTERVAL_MS {
            return Err(format!("重扫间隔不得大于 {MAX_INTERVAL_MS} ms"));
        }
        if self.capacity == 0 {
            return Err("变化日志容量必须大于 0".into());
        }
        Ok(())
    }

    /// 是否需要为某个变化种类发通知。
    pub fn wants_notification(&self, kind: ChangeKind) -> bool {
        match kind {
            ChangeKind::Appeared => self.notify_appeared,
            ChangeKind::Disappeared => self.notify_disappeared,
            // 延迟档位与状态变化默认不打扰用户，只在事件流里留痕。
            ChangeKind::RttClassChanged | ChangeKind::StatusChanged => false,
        }
    }
}

/// 变化种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChangeKind {
    /// 不在线（或首次出现）→ 在线。
    Appeared,
    /// 在线 → 不在线。
    Disappeared,
    /// 前后都在线，但延迟档位跨越了阈值。
    RttClassChanged,
    /// 前后都不在线，但非在线状态本身变了（超时 ↔ 不可达 ↔ 权限不足 …）。
    StatusChanged,
}

impl ChangeKind {
    /// 稳定的机器可读标识。
    pub const fn key(self) -> &'static str {
        match self {
            ChangeKind::Appeared => "appeared",
            ChangeKind::Disappeared => "disappeared",
            ChangeKind::RttClassChanged => "rtt_class_changed",
            ChangeKind::StatusChanged => "status_changed",
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
                ChangeKind::Appeared => Key::LabelChangeAppeared,
                ChangeKind::Disappeared => Key::LabelChangeDisappeared,
                ChangeKind::RttClassChanged => Key::LabelChangeRttClassChanged,
                ChangeKind::StatusChanged => Key::LabelChangeStatusChanged,
            },
        )
    }

    pub const ALL: [ChangeKind; 4] = [
        ChangeKind::Appeared,
        ChangeKind::Disappeared,
        ChangeKind::RttClassChanged,
        ChangeKind::StatusChanged,
    ];
}

/// 一条变化记录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// 发现该变化的轮次（从 2 开始，首轮是基线）。
    pub round: u64,
    pub index: u16,
    pub ip: IpAddr,
    pub kind: ChangeKind,
    pub from: ProbeStatus,
    pub to: ProbeStatus,
}

impl Change {
    /// 人类可读摘要，例如 `192.168.1.7 上线（- → 3 ms）`。
    pub fn summary(&self) -> String {
        format!("{} {}（{}）", self.ip, self.kind.label_zh(), self.detail())
    }

    fn detail(&self) -> String {
        match (&self.from, &self.to) {
            (_, ProbeStatus::Online { rtt_ms }) if self.kind == ChangeKind::Appeared => {
                format!("{} → {rtt_ms} ms", self.from.label_zh())
            }
            (ProbeStatus::Online { rtt_ms: a }, ProbeStatus::Online { rtt_ms: b })
                if self.kind == ChangeKind::RttClassChanged =>
            {
                format!("{a} ms → {b} ms")
            }
            _ => format!("{} → {}", self.from.label_zh(), self.to.label_zh()),
        }
    }
}

/// 两轮扫描之间**非在线状态**是否算「变了」。
///
/// `Error(msg)` 之间只要类型相同就不算变化（否则每条不同的错误文本都会刷屏）。
fn status_variant_changed(from: &ProbeStatus, to: &ProbeStatus) -> bool {
    std::mem::discriminant(from) != std::mem::discriminant(to)
}

/// 比较两轮结果，产出变化列表（按 `index` 升序）。
///
/// - `prev` 为空（首轮）→ 恒返回空：基线尚未建立，不产生噪音事件。
/// - 只比较 `next` 中出现的目标；上一轮有、本轮没有的目标视为「本轮未探测」，不算离线。
/// - 阈值来自 [`Thresholds`]，与导出/界面完全一致。
pub fn diff_scans(
    prev: &[ProbeResult],
    next: &[ProbeResult],
    thresholds: &Thresholds,
    round: u64,
) -> Vec<Change> {
    if prev.is_empty() {
        return Vec::new();
    }
    let mut by_ip: HashMap<IpAddr, &ProbeResult> = HashMap::with_capacity(prev.len());
    for r in prev {
        by_ip.insert(r.ip, r);
    }

    let mut out = Vec::new();
    for cur in next {
        let Some(old) = by_ip.get(&cur.ip) else {
            // 本轮新增的目标：不是「上线」，是目标集合变了，不报变化。
            continue;
        };
        let kind = match (old.status.is_online(), cur.status.is_online()) {
            (false, true) => Some(ChangeKind::Appeared),
            (true, false) => Some(ChangeKind::Disappeared),
            (true, true) => {
                let a = classify_rtt(old.status.rtt_ms().unwrap_or(0), thresholds);
                let b = classify_rtt(cur.status.rtt_ms().unwrap_or(0), thresholds);
                (a != b).then_some(ChangeKind::RttClassChanged)
            }
            (false, false) => status_variant_changed(&old.status, &cur.status)
                .then_some(ChangeKind::StatusChanged),
        };
        if let Some(kind) = kind {
            out.push(Change {
                round,
                index: cur.index,
                ip: cur.ip,
                kind,
                from: old.status.clone(),
                to: cur.status.clone(),
            });
        }
    }
    out.sort_by_key(|c| c.index);
    out
}

/// 变化事件日志：固定容量的环形缓冲。
///
/// `total_seen` 记录历史累计条数（含已被挤出的），便于状态栏显示
/// 「本轮 +N 变化（累计 M）」。**容量上限保证长跑不增长内存。**
#[derive(Debug, Clone)]
pub struct ChangeLog {
    capacity: usize,
    entries: VecDeque<Change>,
    total_seen: u64,
}

impl Default for ChangeLog {
    fn default() -> Self {
        Self::new(DEFAULT_CHANGE_CAPACITY)
    }
}

impl ChangeLog {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: VecDeque::new(),
            total_seen: 0,
        }
    }

    pub fn with_capacity_from(config: &MonitorConfig) -> Self {
        Self::new(config.capacity)
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 历史累计条数（不受容量影响）。
    pub fn total_seen(&self) -> u64 {
        self.total_seen
    }

    pub fn push(&mut self, change: Change) {
        if self.entries.len() == self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back(change);
        self.total_seen += 1;
    }

    pub fn extend(&mut self, changes: impl IntoIterator<Item = Change>) {
        for c in changes {
            self.push(c);
        }
    }

    /// 最近 `n` 条（时间升序），不足则返回全部。
    pub fn recent(&self, n: usize) -> Vec<&Change> {
        let skip = self.entries.len().saturating_sub(n);
        self.entries.iter().skip(skip).collect()
    }

    /// 倒序迭代（最新在前），界面事件流按此顺序展示。
    pub fn iter_recent_first(&self) -> impl Iterator<Item = &Change> {
        self.entries.iter().rev()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Change> {
        self.entries.iter()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.total_seen = 0;
    }

    /// 按种类统计当前缓冲内的条数。
    pub fn count_of(&self, kind: ChangeKind) -> usize {
        self.entries.iter().filter(|c| c.kind == kind).count()
    }
}

/// 监控状态机：推进轮次、保存上一轮结果、产出变化事件。
///
/// **不持有线程与时钟**：调用方（GUI/TUI/CLI）自己决定什么时候重扫，
/// 这里只回答「这一轮相对上一轮变了什么」。
#[derive(Debug)]
pub struct MonitorState {
    config: MonitorConfig,
    round: u64,
    previous: Vec<ProbeResult>,
    log: ChangeLog,
    last_round_changes: Vec<Change>,
}

impl MonitorState {
    pub fn new(config: MonitorConfig) -> Result<Self, String> {
        config.validate()?;
        let log = ChangeLog::with_capacity_from(&config);
        Ok(Self {
            config,
            round: 0,
            previous: Vec::new(),
            log,
            last_round_changes: Vec::new(),
        })
    }

    pub fn config(&self) -> &MonitorConfig {
        &self.config
    }

    /// 修改配置（例如用户在界面改间隔）。校验失败时保持原配置不变。
    pub fn set_config(&mut self, config: MonitorConfig) -> Result<(), String> {
        config.validate()?;
        // 容量变了就重建日志，保留最近的内容
        if config.capacity != self.log.capacity() {
            let keep: Vec<Change> = self
                .log
                .recent(config.capacity.min(self.log.len()))
                .into_iter()
                .cloned()
                .collect();
            self.log = ChangeLog::new(config.capacity);
            self.log.extend(keep);
        }
        self.config = config;
        Ok(())
    }

    /// 已完成的轮次（0 表示还没跑过）。
    pub fn round(&self) -> u64 {
        self.round
    }

    pub fn log(&self) -> &ChangeLog {
        &self.log
    }

    pub fn log_mut(&mut self) -> &mut ChangeLog {
        &mut self.log
    }

    pub fn previous(&self) -> &[ProbeResult] {
        &self.previous
    }

    /// 最近一轮产生的变化。
    pub fn last_round_changes(&self) -> &[Change] {
        &self.last_round_changes
    }

    /// 提交一轮结果：推进轮次、计算变化、写入日志、保存为本轮基线。
    pub fn commit_round(
        &mut self,
        results: &[ProbeResult],
        thresholds: &Thresholds,
    ) -> Vec<Change> {
        self.round += 1;
        let changes = diff_scans(&self.previous, results, thresholds, self.round);
        self.log.extend(changes.iter().cloned());
        self.last_round_changes = changes.clone();
        self.previous = results.to_vec();
        changes
    }

    /// 清空基线：下次提交的将是「首轮」，不产生变化事件。
    pub fn reset_baseline(&mut self) {
        self.previous.clear();
        self.last_round_changes.clear();
    }

    /// 完全重置（换网段时使用）：轮次归零、日志清空。
    pub fn reset_all(&mut self) {
        self.round = 0;
        self.reset_baseline();
        self.log.clear();
    }
}

/// 某主机在最近若干轮中的停留状态，供界面做「新出现」高亮淡出。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HighlightState {
    /// 还需要高亮多少轮（0 表示不高亮）。
    pub remaining_rounds: u8,
}

impl HighlightState {
    /// 变化后默认高亮轮数：3 轮（约 1.5 分钟 @30s 间隔）。
    pub const DEFAULT_ROUNDS: u8 = 3;

    pub fn trigger(&mut self) {
        self.remaining_rounds = Self::DEFAULT_ROUNDS;
    }

    pub fn is_active(&self) -> bool {
        self.remaining_rounds > 0
    }

    /// 每轮结束时调用一次；返回本主机是否仍处高亮期。
    pub fn tick(&mut self) -> bool {
        self.remaining_rounds = self.remaining_rounds.saturating_sub(1);
        self.is_active()
    }
}

/// 把一轮变化转成「主机下标 → 高亮」表，方便界面直接查表。
pub fn highlight_map(changes: &[Change]) -> HashMap<u16, ChangeKind> {
    let mut map = HashMap::with_capacity(changes.len());
    for c in changes {
        map.insert(c.index, c.kind);
    }
    map
}

/// 延迟档位的可读名（供事件流与导出复用同一套措辞）。
pub fn latency_class_key(class: LatencyClass) -> &'static str {
    class.key()
}

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
    use crate::result::ProbeResult;

    fn res(index: u16, host: u8, status: ProbeStatus) -> ProbeResult {
        ProbeResult::new(index, IpAddr::V4(Ipv4Addr::new(192, 168, 1, host)), status)
    }

    fn online(ms: u32) -> ProbeStatus {
        ProbeStatus::Online { rtt_ms: ms }
    }

    fn sample(hosts: &[(u8, ProbeStatus)]) -> Vec<ProbeResult> {
        hosts
            .iter()
            .enumerate()
            .map(|(i, (h, s))| res(i as u16, *h, s.clone()))
            .collect()
    }

    #[test]
    fn first_round_is_a_silent_baseline() {
        let next = sample(&[(1, online(3)), (2, ProbeStatus::Timeout)]);
        let changes = diff_scans(&[], &next, &Thresholds::default(), 1);
        assert!(changes.is_empty(), "首轮不得产生变化事件");
    }

    #[test]
    fn appeared_and_disappeared_are_detected() {
        let prev = sample(&[(1, ProbeStatus::Timeout), (2, online(5))]);
        let next = sample(&[(1, online(9)), (2, ProbeStatus::Timeout)]);
        let changes = diff_scans(&prev, &next, &Thresholds::default(), 2);
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].kind, ChangeKind::Appeared);
        assert_eq!(last4(changes[0].ip), 1);
        assert_eq!(changes[1].kind, ChangeKind::Disappeared);
        assert_eq!(last4(changes[1].ip), 2);
    }

    #[test]
    fn latency_class_crossing_is_reported_but_plain_jitter_is_not() {
        let t = Thresholds::default(); // 低 <50，中 50..=100，高 >100
        let prev = sample(&[(1, online(20)), (2, online(20))]);
        let next = sample(&[(1, online(30)), (2, online(60))]);
        let changes = diff_scans(&prev, &next, &t, 2);
        assert_eq!(changes.len(), 1, "同一档内的抖动不该报变化");
        assert_eq!(changes[0].kind, ChangeKind::RttClassChanged);
        assert_eq!(last4(changes[0].ip), 2);
    }

    #[test]
    fn non_online_status_switch_counts_as_status_change() {
        let prev = sample(&[(1, ProbeStatus::Timeout)]);
        let next = sample(&[(1, ProbeStatus::Unreachable)]);
        let changes = diff_scans(&prev, &next, &Thresholds::default(), 2);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, ChangeKind::StatusChanged);
    }

    #[test]
    fn identical_error_messages_do_not_flap() {
        let prev = sample(&[(1, ProbeStatus::Error("boom".into()))]);
        let next = sample(&[(1, ProbeStatus::Error("another".into()))]);
        let changes = diff_scans(&prev, &next, &Thresholds::default(), 2);
        assert!(changes.is_empty(), "同为 Error 变体不得反复报变化");
    }

    #[test]
    fn targets_missing_from_previous_round_are_not_disappearances() {
        let prev = sample(&[(1, online(5))]);
        let next = sample(&[(1, online(5)), (2, ProbeStatus::Timeout)]);
        let changes = diff_scans(&prev, &next, &Thresholds::default(), 2);
        assert!(changes.is_empty());
    }

    #[test]
    fn change_log_is_bounded_and_tracks_total() {
        let mut log = ChangeLog::new(3);
        for i in 0..10u8 {
            log.push(Change {
                round: i as u64 + 2,
                index: i as u16,
                ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, i)),
                kind: ChangeKind::Appeared,
                from: ProbeStatus::Timeout,
                to: online(1),
            });
        }
        assert_eq!(log.len(), 3, "容量上限必须生效");
        assert_eq!(log.total_seen(), 10);
        // 保留的应是最新的三条
        let recent: Vec<u8> = log.iter().map(|c| last4(c.ip)).collect();
        assert_eq!(recent, vec![7, 8, 9]);
    }

    #[test]
    fn change_log_recent_returns_tail_in_order() {
        let mut log = ChangeLog::new(10);
        for i in 0..5u8 {
            log.push(Change {
                round: 2,
                index: i as u16,
                ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, i)),
                kind: ChangeKind::Disappeared,
                from: online(1),
                to: ProbeStatus::Timeout,
            });
        }
        let recent: Vec<u8> = log.recent(2).iter().map(|c| last4(c.ip)).collect();
        assert_eq!(recent, vec![3, 4]);
        let newest_first: Vec<u8> = log.iter_recent_first().map(|c| last4(c.ip)).collect();
        assert_eq!(newest_first, vec![4, 3, 2, 1, 0]);
    }

    #[test]
    fn monitor_state_advances_rounds_and_keeps_baseline() {
        let mut state = MonitorState::new(MonitorConfig::default()).unwrap();
        let t = Thresholds::default();
        let r1 = sample(&[(1, ProbeStatus::Timeout), (2, online(5))]);
        assert!(state.commit_round(&r1, &t).is_empty());
        assert_eq!(state.round(), 1);

        let r2 = sample(&[(1, online(7)), (2, online(5))]);
        let changes = state.commit_round(&r2, &t);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, ChangeKind::Appeared);
        assert_eq!(state.round(), 2);
        assert_eq!(state.log().len(), 1);

        // 第三轮与第二轮完全一致 -> 无变化
        assert!(state.commit_round(&r2, &t).is_empty());
    }

    #[test]
    fn reset_baseline_makes_the_next_round_silent() {
        let mut state = MonitorState::new(MonitorConfig::default()).unwrap();
        let t = Thresholds::default();
        state.commit_round(&sample(&[(1, ProbeStatus::Timeout)]), &t);
        state.reset_baseline();
        let changes = state.commit_round(&sample(&[(1, online(3))]), &t);
        assert!(changes.is_empty());
        assert_eq!(state.round(), 2, "轮次计数不受基线重置影响");
        state.reset_all();
        assert_eq!(state.round(), 0);
        assert_eq!(state.log().len(), 0);
    }

    #[test]
    fn config_validation_rejects_out_of_range_intervals() {
        let mut cfg = MonitorConfig {
            interval_ms: MIN_INTERVAL_MS - 1,
            ..MonitorConfig::default()
        };
        assert!(cfg.validate().is_err());
        cfg.interval_ms = MAX_INTERVAL_MS + 1;
        assert!(cfg.validate().is_err());
        cfg.interval_ms = DEFAULT_INTERVAL_MS;
        assert!(cfg.validate().is_ok());
        cfg.capacity = 0;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn shrinking_capacity_keeps_the_most_recent_changes() {
        let mut state = MonitorState::new(MonitorConfig::default()).unwrap();
        let t = Thresholds::default();
        state.commit_round(&sample(&[(1, ProbeStatus::Timeout)]), &t);
        state.commit_round(&sample(&[(1, online(1))]), &t);
        let mut cfg = state.config().clone();
        cfg.capacity = 1;
        state.set_config(cfg).unwrap();
        assert_eq!(state.log().capacity(), 1);
        assert_eq!(state.log().len(), 1);
    }

    #[test]
    fn highlight_fades_after_configured_rounds() {
        let mut h = HighlightState::default();
        assert!(!h.is_active());
        h.trigger();
        assert!(h.is_active());
        for _ in 0..HighlightState::DEFAULT_ROUNDS {
            h.tick();
        }
        assert!(!h.is_active(), "高亮必须在若干轮后自动淡出");
    }

    #[test]
    fn notification_preferences_only_cover_up_and_down() {
        let cfg = MonitorConfig::default();
        assert!(cfg.wants_notification(ChangeKind::Appeared));
        assert!(cfg.wants_notification(ChangeKind::Disappeared));
        assert!(!cfg.wants_notification(ChangeKind::RttClassChanged));
    }

    #[test]
    fn change_kinds_have_stable_keys_and_labels() {
        for k in ChangeKind::ALL {
            assert!(!k.key().is_empty());
            assert!(!k.label_zh().is_empty());
        }
        assert_eq!(ChangeKind::Appeared.key(), "appeared");
    }

    #[test]
    fn change_summary_mentions_ip_and_reason() {
        let prev = sample(&[(7, ProbeStatus::Timeout)]);
        let next = sample(&[(7, online(3))]);
        let changes = diff_scans(&prev, &next, &Thresholds::default(), 2);
        let text = changes[0].summary();
        assert!(text.contains("192.168.1.7"), "{text}");
        assert!(text.contains("上线"), "{text}");
        assert!(text.contains("3 ms"), "{text}");
    }

    #[test]
    fn highlight_map_indexes_by_target_index() {
        let prev = sample(&[(1, ProbeStatus::Timeout), (2, online(1))]);
        let next = sample(&[(1, online(2)), (2, ProbeStatus::Timeout)]);
        let changes = diff_scans(&prev, &next, &Thresholds::default(), 2);
        let map = highlight_map(&changes);
        assert_eq!(map.get(&0), Some(&ChangeKind::Appeared));
        assert_eq!(map.get(&1), Some(&ChangeKind::Disappeared));
    }
}
