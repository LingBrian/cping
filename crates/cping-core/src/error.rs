//! 统一错误类型。
//!
//! 设计要点：**权限不足、超时、不可达、内部错误必须互相区分**，
//! 不允许用同一个笼统的 `Error` 表示，也不允许把失败伪装成成功。

use std::fmt;

/// 探测层面的失败原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeError {
    /// 在超时时间内没有任何应答。
    Timeout { timeout_ms: u64 },
    /// 明确收到「目标不可达」类信号（ICMP unreachable / 无路由等）。
    Unreachable { detail: String },
    /// 权限不足（例如无法创建原始套接字、缺少原始 ICMP 能力）。
    PermissionDenied { detail: String },
    /// 当前平台/后端不支持所需能力。
    Unsupported { detail: String },
    /// 其它内部错误（不得用于表达超时或权限问题）。
    Internal { detail: String },
}

impl ProbeError {
    pub fn timeout(timeout_ms: u64) -> Self {
        ProbeError::Timeout { timeout_ms }
    }
    pub fn unreachable(detail: impl Into<String>) -> Self {
        ProbeError::Unreachable {
            detail: detail.into(),
        }
    }
    pub fn permission_denied(detail: impl Into<String>) -> Self {
        ProbeError::PermissionDenied {
            detail: detail.into(),
        }
    }
    pub fn unsupported(detail: impl Into<String>) -> Self {
        ProbeError::Unsupported {
            detail: detail.into(),
        }
    }
    pub fn internal(detail: impl Into<String>) -> Self {
        ProbeError::Internal {
            detail: detail.into(),
        }
    }

    /// 稳定的机器可读标识，供 GUI/TUI 与导出使用。
    pub fn kind(&self) -> &'static str {
        match self {
            ProbeError::Timeout { .. } => "timeout",
            ProbeError::Unreachable { .. } => "unreachable",
            ProbeError::PermissionDenied { .. } => "permission_denied",
            ProbeError::Unsupported { .. } => "unsupported",
            ProbeError::Internal { .. } => "internal",
        }
    }
}

impl fmt::Display for ProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProbeError::Timeout { timeout_ms } => write!(f, "超时（{timeout_ms} ms 内无应答）"),
            ProbeError::Unreachable { detail } => write!(f, "目标不可达：{detail}"),
            ProbeError::PermissionDenied { detail } => write!(f, "权限不足：{detail}"),
            ProbeError::Unsupported { detail } => write!(f, "当前平台不支持：{detail}"),
            ProbeError::Internal { detail } => write!(f, "内部错误：{detail}"),
        }
    }
}

impl std::error::Error for ProbeError {}

/// 核心逻辑（解析、配置、导出）层面的错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreError {
    /// 网段前缀不合法。
    InvalidPrefix(String),
    /// 目标数量超过安全上限。
    TooManyTargets { actual: usize, limit: usize },
    /// 配置不合法（并发、超时、阈值等）。
    InvalidConfig(String),
    /// 导出失败。
    Export(String),
    /// 能力缺失。
    Unsupported(String),
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoreError::InvalidPrefix(msg) => write!(f, "网段前缀无效：{msg}"),
            CoreError::TooManyTargets { actual, limit } => {
                write!(f, "目标数量 {actual} 超出上限 {limit}")
            }
            CoreError::InvalidConfig(msg) => write!(f, "配置无效：{msg}"),
            CoreError::Export(msg) => write!(f, "导出失败：{msg}"),
            CoreError::Unsupported(msg) => write!(f, "不支持的操作：{msg}"),
        }
    }
}

impl std::error::Error for CoreError {}

pub type CoreResult<T> = Result<T, CoreError>;
