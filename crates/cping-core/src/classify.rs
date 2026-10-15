//! RTT 分类。
//!
//! 边界语义来自原版 DFM 的四条 GroupBox 文案（M0.5 复核确证）：
//! 「时延小于 50ms」「时延在 50ms~100ms 之间」「时延大于 100ms」「无回应」。
//! 即 `rtt < 50` → 低；`50 <= rtt <= 100` → 中；`rtt > 100` → 高。

use crate::error::{CoreError, CoreResult};
use crate::i18n::{self, Key, Lang};
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
                LatencyClass::Low => Key::LabelLatencyLow,
                LatencyClass::Mid => Key::LabelLatencyMid,
                LatencyClass::High => Key::LabelLatencyHigh,
            },
        )
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
                ResultClass::Low => Key::LabelResultLow,
                ResultClass::Mid => Key::LabelResultMid,
                ResultClass::High => Key::LabelResultHigh,
                ResultClass::Offline => Key::LabelResultOffline,
                ResultClass::Error => Key::LabelResultError,
            },
        )
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

    /// 该类别的**单字符符号**。
    ///
    /// 符号是「不依赖颜色」的那条信息通道，所以它必须由核心给出唯一一份：
    /// 此前 GUI 在 TS 里存了一份、TUI 在渲染代码里又写了一遍、
    /// 报表里还第三份——三份里任何一份被改，用户就会在三个界面看到
    /// 同一个状态对应三种符号。**颜色可以没有（`--no-color`），
    /// 符号不能没有**，这正是它必须只有一个来源的原因。
    ///
    /// 五个符号两两不同（有测试断言），且在等宽字体下都是单宽度字符。
    pub const fn symbol(self) -> &'static str {
        match self {
            ResultClass::Low => "#",
            ResultClass::Mid => "+",
            ResultClass::High => "*",
            ResultClass::Offline => "-",
            ResultClass::Error => "!",
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
