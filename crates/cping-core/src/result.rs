//! 探测结果模型、任务统计与事件。
//!
//! 这是 GUI 与 TUI **唯一**共享的结果模型：两侧都不得自行定义状态或统计口径。

use std::net::Ipv4Addr;

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeResult {
    /// 目标在任务内的下标（0 基）。
    pub index: u16,
    pub ip: Ipv4Addr,
    /// MAC 地址；取不到时为 `None` —— **不得伪造**。
    pub mac: Option<String>,
    pub status: ProbeStatus,
}

impl ProbeResult {
    pub fn new(index: u16, ip: Ipv4Addr, status: ProbeStatus) -> Self {
        Self {
            index,
            ip,
            mac: None,
            status,
        }
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
