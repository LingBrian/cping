//! 探测结果模型、任务统计与事件。
//!
//! 这是 GUI 与 TUI **唯一**共享的结果模型：两侧都不得自行定义状态或统计口径。

use crate::i18n::{self, Key, Lang};
use std::net::IpAddr;

/// 任务标识：每个探测任务一个唯一 `task_id`。
pub type TaskId = String;

/// 单个目标的终态 / 中间态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeStatus {
    /// 尚未得到结果。
    Pending,
    /// 收到应答。
    Online { rtt_ms: u32 },
    /// 超时无应答。
    Timeout,
    /// 明确不可达。
    Unreachable,
    /// 权限不足（无法执行 ICMP）。
    PermissionDenied,
    /// 平台不支持该能力。
    Unsupported,
    /// 其它错误（仅用于无法归入以上分类的情况）。
    Error(String),
}

impl ProbeStatus {
    pub fn is_online(&self) -> bool {
        matches!(self, ProbeStatus::Online { .. })
    }

    /// 是否已经是终态（不会再变化）。
    pub fn is_final(&self) -> bool {
        !matches!(self, ProbeStatus::Pending)
    }

    pub fn rtt_ms(&self) -> Option<u32> {
        match self {
            ProbeStatus::Online { rtt_ms } => Some(*rtt_ms),
            _ => None,
        }
    }

    /// 稳定的机器可读标识。
    pub fn key(&self) -> &'static str {
        match self {
            ProbeStatus::Pending => "pending",
            ProbeStatus::Online { .. } => "online",
            ProbeStatus::Timeout => "timeout",
            ProbeStatus::Unreachable => "unreachable",
            ProbeStatus::PermissionDenied => "permission_denied",
            ProbeStatus::Unsupported => "unsupported",
            ProbeStatus::Error(_) => "error",
        }
    }

    /// 界面显示用中文标签。
    /// 中文标签（等价于 `label_in(Lang::Zh)`）。
    ///
    /// 核心层内部的报表与导出仍在用它——**产物内容是数据契约**：
    /// 同一份 CSV / HTML 在中英文机器上必须一致；只有界面跟着语言变。
    pub fn label_zh(&self) -> &'static str {
        self.label_in(Lang::Zh)
    }

    /// 按语言渲染的标签。
    pub fn label_in(&self, lang: Lang) -> &'static str {
        i18n::text(
            lang,
            match self {
                ProbeStatus::Pending => Key::LabelStatusPending,
                ProbeStatus::Online { .. } => Key::LabelStatusOnline,
                ProbeStatus::Timeout => Key::LabelStatusTimeout,
                ProbeStatus::Unreachable => Key::LabelStatusUnreachable,
                ProbeStatus::PermissionDenied => Key::LabelStatusPermissionDenied,
                ProbeStatus::Unsupported => Key::LabelStatusUnsupported,
                ProbeStatus::Error(_) => Key::LabelStatusError,
            },
        )
    }

    /// 备注字段：仅在需要解释失败原因时返回内容。
    pub fn note(&self) -> Option<&str> {
        match self {
            ProbeStatus::Error(msg) => Some(msg.as_str()),
            ProbeStatus::Pending
            | ProbeStatus::Online { .. }
            | ProbeStatus::Timeout
            | ProbeStatus::Unreachable
            | ProbeStatus::PermissionDenied
            | ProbeStatus::Unsupported => None,
        }
    }
}

/// 单个目标的探测结果。
///
/// M7 起携带三项**目标信息**（全部可选）：`mac` / `vendor` / `hostname`。
/// 三者的取值规则统一为「拿不到就 `None`」——**任何一项都不得用占位文本顶替**，
/// 因为界面无法区分「真的叫 unknown」和「没查到」。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeResult {
    /// 目标在任务内的下标（0 基）。
    pub index: u16,
    /// 目标地址。M12 起同时容纳 IPv4 与 IPv6（[`IpAddr`]）。
    pub ip: IpAddr,
    /// MAC 地址；取不到时为 `None` —— **不得伪造**。
    ///
    /// 注意：ARP/邻居表只覆盖 IPv4，因此 IPv6 目标的 `mac` 恒为 `None`。
    pub mac: Option<String>,
    /// 由 MAC 前缀查到的厂商名（IEEE 注册表）；本地管理地址或未注册前缀为 `None`。
    pub vendor: Option<String>,
    /// DNS 反查得到的主机名；只在用户按需触发时才有值。
    pub hostname: Option<String>,
    pub status: ProbeStatus,
}

impl ProbeResult {
    pub fn new(index: u16, ip: IpAddr, status: ProbeStatus) -> Self {
        Self {
            index,
            ip,
            mac: None,
            vendor: None,
            hostname: None,
            status,
        }
    }

    /// 一次性设置三项目标信息（避免调用方逐个改字段时漏项）。
    pub fn with_target_info(
        mut self,
        mac: Option<String>,
        vendor: Option<String>,
        hostname: Option<String>,
    ) -> Self {
        self.mac = mac;
        self.vendor = vendor;
        self.hostname = hostname;
        self
    }

    /// 展示用 MAC（空串表示没有，而不是 `None` 字面量）。
    pub fn mac_or_empty(&self) -> &str {
        self.mac.as_deref().unwrap_or("")
    }

    /// 展示用厂商名。
    pub fn vendor_or_empty(&self) -> &str {
        self.vendor.as_deref().unwrap_or("")
    }

    /// 展示用主机名。
    pub fn hostname_or_empty(&self) -> &str {
        self.hostname.as_deref().unwrap_or("")
    }
}

/// 统计口径：**只在这里计算**，GUI/TUI 不得自行累加。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanStats {
    pub total: usize,
    pub completed: usize,
    pub pending: usize,
    pub online: usize,
    pub timeout: usize,
    pub unreachable: usize,
    pub permission_denied: usize,
    pub unsupported: usize,
    pub error: usize,
}

impl ScanStats {
    pub fn from_statuses<'a, I>(statuses: I) -> Self
    where
        I: IntoIterator<Item = &'a ProbeStatus>,
    {
        let mut stats = ScanStats::default();
        for status in statuses {
            stats.total += 1;
            match status {
                ProbeStatus::Pending => stats.pending += 1,
                ProbeStatus::Online { .. } => {
                    stats.online += 1;
                    stats.completed += 1;
                }
                ProbeStatus::Timeout => {
                    stats.timeout += 1;
                    stats.completed += 1;
                }
                ProbeStatus::Unreachable => {
                    stats.unreachable += 1;
                    stats.completed += 1;
                }
                ProbeStatus::PermissionDenied => {
                    stats.permission_denied += 1;
                    stats.completed += 1;
                }
                ProbeStatus::Unsupported => {
                    stats.unsupported += 1;
                    stats.completed += 1;
                }
                ProbeStatus::Error(_) => {
                    stats.error += 1;
                    stats.completed += 1;
                }
            }
        }
        stats
    }

    pub fn from_results(results: &[ProbeResult]) -> Self {
        Self::from_statuses(results.iter().map(|r| &r.status))
    }

    /// 已知目标总数时使用：未出现在 `results` 中的目标计为 `pending`。
    /// 取消任务后「已完成结果保留、未开始的目标仍计入总数」正是靠这里保持一致。
    pub fn from_results_with_total(results: &[ProbeResult], total: usize) -> Self {
        let mut stats = Self::from_results(results);
        stats.total = total;
        stats.pending = total.saturating_sub(stats.completed);
        stats
    }

    // -----------------------------------------------------------------
    // M19：增量口径
    // -----------------------------------------------------------------

    /// 设定目标总数（增量模式的起点）。
    ///
    /// 增量模式下 `total` 由调用方一次设定，之后 [`Self::push_status`] 只改分项与
    /// `completed`，**不再动 `total`**——否则「先设定总数再逐条累加」会把总数翻倍。
    pub fn set_total(&mut self, total: usize) {
        self.total = total;
        self.pending = total.saturating_sub(self.completed);
    }

    /// **增量**累加一个状态（M19）。
    ///
    /// 为什么需要它：三端此前都是「每收到一条结果就把整表拍平成 `Vec` 再全量统计」，
    /// 那是 O(n²)——65536 个目标要拍平 65536 次、累计克隆约 21 亿条结果，
    /// 万级目标下界面会被统计本身拖死。增量累加是 O(1)。
    ///
    /// 口径必须与 [`Self::from_statuses`] **逐字段相同**，
    /// 由 `incremental_stats_match_the_batch_ones` 守着：任何分项加错都会红灯。
    ///
    /// 注意 `pending` 在批量口径里也全由 `total - completed` 推出（见
    /// [`Self::from_results_with_total`]），这里沿用同一条定义，保证两个口径
    /// 在任意调用序列下都一致——**不要**在这里按变体去 `pending += 1`。
    pub fn push_status(&mut self, status: &ProbeStatus) {
        match status {
            ProbeStatus::Pending => {}
            ProbeStatus::Online { .. } => {
                self.online += 1;
                self.completed += 1;
            }
            ProbeStatus::Timeout => {
                self.timeout += 1;
                self.completed += 1;
            }
            ProbeStatus::Unreachable => {
                self.unreachable += 1;
                self.completed += 1;
            }
            ProbeStatus::PermissionDenied => {
                self.permission_denied += 1;
                self.completed += 1;
            }
            ProbeStatus::Unsupported => {
                self.unsupported += 1;
                self.completed += 1;
            }
            ProbeStatus::Error(_) => {
                self.error += 1;
                self.completed += 1;
            }
        }
        self.pending = self.total.saturating_sub(self.completed);
    }

    /// 便捷入口：按目标总数建一个统计，再逐条 [`Self::push_status`]。
    pub fn for_total(total: usize) -> Self {
        let mut stats = Self::default();
        stats.set_total(total);
        stats
    }

    /// 完成比例，`0.0..=1.0`。
    pub fn progress(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.completed as f64 / self.total as f64
        }
    }
}

/// 调度器向 UI 推送的事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeEvent {
    Started {
        task_id: TaskId,
        total: usize,
    },
    Result {
        task_id: TaskId,
        result: ProbeResult,
    },
    Progress {
        task_id: TaskId,
        completed: usize,
        total: usize,
    },
    Finished {
        task_id: TaskId,
        stats: ScanStats,
        elapsed_ms: u64,
    },
    /// 用户取消：已完成结果仍然保留，只是不再调度新的目标。
    Cancelled {
        task_id: TaskId,
        stats: ScanStats,
        elapsed_ms: u64,
    },
}

impl ProbeEvent {
    pub fn task_id(&self) -> &str {
        match self {
            ProbeEvent::Started { task_id, .. }
            | ProbeEvent::Result { task_id, .. }
            | ProbeEvent::Progress { task_id, .. }
            | ProbeEvent::Finished { task_id, .. }
            | ProbeEvent::Cancelled { task_id, .. } => task_id,
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            ProbeEvent::Finished { .. } | ProbeEvent::Cancelled { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    fn sample_statuses() -> Vec<ProbeStatus> {
        vec![
            ProbeStatus::Online { rtt_ms: 3 },
            ProbeStatus::Timeout,
            ProbeStatus::Unreachable,
            ProbeStatus::PermissionDenied,
            ProbeStatus::Unsupported,
            ProbeStatus::Error("boom".into()),
            ProbeStatus::Pending,
            ProbeStatus::Online { rtt_ms: 42 },
        ]
    }

    fn sample_results() -> Vec<ProbeResult> {
        sample_statuses()
            .iter()
            .enumerate()
            .map(|(i, s)| {
                ProbeResult::new(
                    i as u16,
                    IpAddr::V4(Ipv4Addr::new(192, 168, 1, i as u8 + 1)),
                    s.clone(),
                )
            })
            .collect()
    }

    /// M19 的核心不变量：增量口径与批量口径必须逐字段相同。
    ///
    /// 三端在 M19 之后走的是增量路径（每收到一条结果加一次），而落盘/报表走的
    /// 仍是批量路径。两条路一旦对不上，「界面上显示完成 500」与「报表里 498」
    /// 会同时存在，而且没人知道哪个对。
    #[test]
    fn incremental_stats_match_the_batch_ones() {
        let results = sample_results();
        let batch = ScanStats::from_results_with_total(&results, results.len());

        let mut incremental = ScanStats::for_total(results.len());
        for r in &results {
            incremental.push_status(&r.status);
        }

        assert_eq!(incremental, batch, "增量与批量统计必须逐字段相同");
    }

    /// 取消的场景：「已完成的保留、未开始的目标仍计入总数」。
    ///
    /// 批量路径靠 `from_results_with_total(results, total)` 表达，
    /// 增量路径靠 `set_total(total)` + 只 push 已完成的那部分。
    #[test]
    fn incremental_stats_keep_cancelled_remainder_as_pending() {
        let all = sample_results();
        // 只探测了前 5 个就取消
        let done = &all[..5];
        let batch = ScanStats::from_results_with_total(done, all.len());

        let mut incremental = ScanStats::for_total(all.len());
        for r in done {
            incremental.push_status(&r.status);
        }

        assert_eq!(incremental.completed, 5);
        assert_eq!(incremental.pending, all.len() - 5);
        assert_eq!(incremental, batch);
    }

    /// `Pending` 是「尚未得到结果」，不得被算进 `completed`，
    /// 也不得让 `pending` 被加两次。
    #[test]
    fn pending_status_is_not_completed_and_is_not_double_counted() {
        let mut stats = ScanStats::for_total(3);
        stats.push_status(&ProbeStatus::Pending);
        stats.push_status(&ProbeStatus::Online { rtt_ms: 1 });
        stats.push_status(&ProbeStatus::Timeout);

        assert_eq!(stats.completed, 2);
        assert_eq!(stats.pending, 1);
        assert_eq!(stats.total, 3);
        assert_eq!(
            stats.online
                + stats.timeout
                + stats.unreachable
                + stats.permission_denied
                + stats.unsupported
                + stats.error,
            stats.completed,
            "所有终态分项之和必须等于 completed"
        );
    }

    /// `set_total` 先设定总数、再逐条累加，`total` 不得被翻倍。
    #[test]
    fn setting_total_first_does_not_double_it() {
        let mut stats = ScanStats::for_total(10);
        for _ in 0..10 {
            stats.push_status(&ProbeStatus::Timeout);
        }
        assert_eq!(stats.total, 10);
        assert_eq!(stats.completed, 10);
        assert_eq!(stats.pending, 0);
    }
}
