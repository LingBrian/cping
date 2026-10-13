//! RTT 分类。
//!
//! 边界语义来自原版 DFM 的四条 GroupBox 文案（M0.5 复核确证）：
//! 「时延小于 50ms」「时延在 50ms~100ms 之间」「时延大于 100ms」「无回应」。
//! 即 `rtt < 50` → 低；`50 <= rtt <= 100` → 中；`rtt > 100` → 高。

use crate::error::{CoreError, CoreResult};
use crate::result::ProbeStatus;

/// 分类阈值（毫秒）。默认对齐原版。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Thresholds {
    /// 小于该值为「低延迟」。
    pub low_max_ms: u32,
    /// 小于等于该值为「中延迟」，大于为「高延迟」。
    pub mid_max_ms: u32,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            low_max_ms: 50,
            mid_max_ms: 100,
        }
    }
}

impl Thresholds {
    pub const fn new(low_max_ms: u32, mid_max_ms: u32) -> Self {
        Self {
            low_max_ms,
            mid_max_ms,
        }
    }

    pub fn validate(&self) -> CoreResult<()> {
        if self.low_max_ms == 0 {
            return Err(CoreError::InvalidConfig("低延迟阈值必须大于 0".into()));
        }
        if self.mid_max_ms <= self.low_max_ms {
            return Err(CoreError::InvalidConfig(format!(
                "中延迟阈值 {} 必须大于低延迟阈值 {}",
                self.mid_max_ms, self.low_max_ms
            )));
        }
        Ok(())
    }
}

/// 有应答时的延迟档次。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LatencyClass {
    Low,
    Mid,
    High,
}

impl LatencyClass {
    pub const fn key(self) -> &'static str {
        match self {
            LatencyClass::Low => "low",
            LatencyClass::Mid => "mid",
            LatencyClass::High => "high",
        }
    }

    pub const fn label_zh(self) -> &'static str {
        match self {
            LatencyClass::Low => "低延迟",
            LatencyClass::Mid => "中延迟",
            LatencyClass::High => "高延迟",
        }
    }
}

/// 面向展示/导出的统一分类结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultClass {
    Low,
    Mid,
    High,
    /// 超时或不可达统一显示为「无回应」。
    Offline,
    /// 权限不足 / 不支持 / 内部错误 —— 与「无回应」**必须区分**。
    Error,
}

impl ResultClass {
    pub const fn key(self) -> &'static str {
        match self {
            ResultClass::Low => "low",
            ResultClass::Mid => "mid",
            ResultClass::High => "high",
            ResultClass::Offline => "offline",
            ResultClass::Error => "error",
        }
    }

    pub const fn label_zh(self) -> &'static str {
        match self {
            ResultClass::Low => "低延迟",
            ResultClass::Mid => "中延迟",
            ResultClass::High => "高延迟",
            ResultClass::Offline => "无回应",
            ResultClass::Error => "错误",
        }
    }

    /// 该类别对应的颜色配置键。
    pub const fn color_key(self) -> &'static str {
        match self {
            ResultClass::Low => "low",
            ResultClass::Mid => "mid",
            ResultClass::High => "high",
            ResultClass::Offline => "offline",
            ResultClass::Error => "error",
        }
    }
}

/// 纯 RTT 分类。
pub fn classify_rtt(rtt_ms: u32, thresholds: &Thresholds) -> LatencyClass {
    if rtt_ms < thresholds.low_max_ms {
        LatencyClass::Low
    } else if rtt_ms <= thresholds.mid_max_ms {
        LatencyClass::Mid
    } else {
        LatencyClass::High
    }
}

/// 把探测状态映射为展示类别。`Pending` 还没有类别，返回 `None`。
pub fn classify_status(status: &ProbeStatus, thresholds: &Thresholds) -> Option<ResultClass> {
    match status {
        ProbeStatus::Pending => None,
        ProbeStatus::Online { rtt_ms } => Some(match classify_rtt(*rtt_ms, thresholds) {
            LatencyClass::Low => ResultClass::Low,
            LatencyClass::Mid => ResultClass::Mid,
            LatencyClass::High => ResultClass::High,
        }),
        ProbeStatus::Timeout | ProbeStatus::Unreachable => Some(ResultClass::Offline),
        ProbeStatus::PermissionDenied | ProbeStatus::Unsupported | ProbeStatus::Error(_) => {
            Some(ResultClass::Error)
        }
    }
}
