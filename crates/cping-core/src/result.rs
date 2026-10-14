//! 探测结果模型、任务统计与事件。
//!
//! 这是 GUI 与 TUI **唯一**共享的结果模型：两侧都不得自行定义状态或统计口径。

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
    pub fn label_zh(&self) -> &'static str {
        match self {
            ProbeStatus::Pending => "等待",
            ProbeStatus::Online { .. } => "在线",
            ProbeStatus::Timeout => "超时",
            ProbeStatus::Unreachable => "不可达",
            ProbeStatus::PermissionDenied => "权限不足",
            ProbeStatus::Unsupported => "不支持",
            ProbeStatus::Error(_) => "错误",
        }
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
