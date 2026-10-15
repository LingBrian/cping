//! 统一错误类型。
//!
//! 设计要点：**权限不足、超时、不可达、内部错误必须互相区分**，
//! 不允许用同一个笼统的 `Error` 表示，也不允许把失败伪装成成功。
//!
//! M18 起每个错误都有两份表示：**稳定的错误码**（[`CoreError::code`] /
//! [`ProbeError::kind`]，给机器与本地化查表用）和**渲染后的句子**
//! （[`CoreError::localized`]，给人读）。`Display` 等于
//! `localized(Lang::Zh)`——**共用一处实现**，避免改了中文忘了改另一处。

use crate::i18n::{self, Key, Lang};
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

    /// 按语言渲染成人话。
    ///
    /// `detail` **原样透传**：它来自操作系统或后端，翻了反而更难查
    /// （用户拿这句去搜索时要搜的是原始英文）。本地化的只是外面那层框。
    pub fn localized(&self, lang: Lang) -> String {
        match self {
            ProbeError::Timeout { timeout_ms } => {
                i18n::render(lang, Key::ProbeTimeout, &[&timeout_ms.to_string()])
            }
            ProbeError::Unreachable { detail } => {
                i18n::render(lang, Key::ProbeUnreachable, &[detail])
            }
            ProbeError::PermissionDenied { detail } => {
                i18n::render(lang, Key::ProbePermission, &[detail])
            }
            ProbeError::Unsupported { detail } => {
                i18n::render(lang, Key::ProbeUnsupported, &[detail])
            }
            ProbeError::Internal { detail } => i18n::render(lang, Key::ProbeInternal, &[detail]),
        }
    }
}

impl fmt::Display for ProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Display 就是中文渲染，不另写一份 match。
        f.write_str(&self.localized(Lang::Zh))
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
    /// M17：目标落在保留 / 特殊用途网段，而策略未放行。
    ///
    /// 单独立一个变体（而不是塞进 [`CoreError::InvalidConfig`]）的理由：
    /// 三端都要能把「这条提示」与「配置写错了」区分开——前者要给出
    /// 「加 --allow-reserved 才能扫」这类**可执行的下一步**。
    ReservedTarget { ip: String, reason: String },
    /// 配置不合法（并发、超时、阈值等）。
    InvalidConfig(String),
    /// 导出失败。
    Export(String),
    /// 扫描快照读写/解析失败。
    Snapshot(String),
    /// 能力缺失。
    Unsupported(String),
}

impl CoreError {
    /// 稳定的机器可读错误码。界面用它做本地化查表，脚本用它做分支判断
    /// ——**不要靠匹配消息文本来判断错误类型**，那句话随时会因为改措辞而变化。
    pub fn code(&self) -> &'static str {
        match self {
            CoreError::InvalidPrefix(_) => "invalid_prefix",
            CoreError::TooManyTargets { .. } => "too_many_targets",
            CoreError::ReservedTarget { .. } => "reserved_target",
            CoreError::InvalidConfig(_) => "invalid_config",
            CoreError::Export(_) => "export_failed",
            CoreError::Snapshot(_) => "snapshot_unavailable",
            CoreError::Unsupported(_) => "unsupported",
        }
    }

    /// 按语言渲染成人话。
    ///
    /// 各变体括号里的 `detail` 是**自由文本**（来自解析器、文件系统、后端），
    /// 如实原样透传；本地化的是外面那层框。这样至少能保证「这是什么类别的
    /// 错误」在任何语言下都读得懂，而细节仍然可供搜索与报 bug。
    pub fn localized(&self, lang: Lang) -> String {
        match self {
            CoreError::InvalidPrefix(msg) => i18n::render(lang, Key::ErrInvalidPrefix, &[msg]),
            CoreError::TooManyTargets { actual, limit } => i18n::render(
                lang,
                Key::ErrTooManyTargets,
                &[&actual.to_string(), &limit.to_string()],
            ),
            CoreError::ReservedTarget { ip, reason } => {
                i18n::render(lang, Key::ErrReservedTarget, &[ip, reason])
            }
            CoreError::InvalidConfig(msg) => i18n::render(lang, Key::ErrInvalidConfig, &[msg]),
            CoreError::Export(msg) => i18n::render(lang, Key::ErrExport, &[msg]),
            CoreError::Snapshot(msg) => i18n::render(lang, Key::ErrSnapshot, &[msg]),
            CoreError::Unsupported(msg) => i18n::render(lang, Key::ErrUnsupported, &[msg]),
        }
    }
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.localized(Lang::Zh))
    }
}

impl std::error::Error for CoreError {}

pub type CoreResult<T> = Result<T, CoreError>;

#[cfg(test)]
mod tests {
    use super::*;

    fn all_core_errors() -> Vec<CoreError> {
        vec![
            CoreError::InvalidPrefix("192.168.1.999".into()),
            CoreError::TooManyTargets {
                actual: 300,
                limit: 255,
            },
            CoreError::ReservedTarget {
                ip: "224.0.0.1".into(),
                reason: "multicast address".into(),
            },
            CoreError::InvalidConfig("concurrency = 0".into()),
            CoreError::Export("No such file (os error 2)".into()),
            CoreError::Snapshot("bad magic".into()),
            CoreError::Unsupported("icmp6".into()),
        ]
    }

    fn all_probe_errors() -> Vec<ProbeError> {
        vec![
            ProbeError::Timeout { timeout_ms: 1500 },
            ProbeError::Unreachable {
                detail: "network unreachable".into(),
            },
            ProbeError::PermissionDenied {
                detail: "EPERM".into(),
            },
            ProbeError::Unsupported {
                detail: "raw socket".into(),
            },
            ProbeError::Internal {
                detail: "bad length".into(),
            },
        ]
    }

    /// 错误码是给脚本与本地化查表用的**契约**，不能重复、不能是给人看的中文。
    #[test]
    fn error_codes_are_unique_snake_case_ascii() {
        let mut seen = std::collections::HashSet::new();
        for e in all_core_errors() {
            let c = e.code();
            assert!(seen.insert(c), "错误码 {c:?} 重复");
            assert!(
                !c.is_empty()
                    && c.bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
                "错误码 {c:?} 不是 snake_case ASCII"
            );
        }
        assert_eq!(seen.len(), 7, "CoreError 变体数与错误码数量不一致");

        let mut kinds = std::collections::HashSet::new();
        for e in all_probe_errors() {
            let k = e.kind();
            assert!(kinds.insert(k), "ProbeError::kind {k:?} 重复");
        }
        assert_eq!(kinds.len(), 5);
    }

    /// `Display` 必须与「中文渲染」逐字相同——两者各写一份 match 的结果，
    /// 就是改了其中一处、另一处悄悄留在旧措辞上。
    #[test]
    fn display_is_exactly_the_chinese_rendering() {
        for e in all_core_errors() {
            assert_eq!(e.to_string(), e.localized(Lang::Zh), "CoreError::{e:?}");
        }
        for e in all_probe_errors() {
            assert_eq!(e.to_string(), e.localized(Lang::Zh), "ProbeError::{e:?}");
        }
    }

    /// 英文渲染里不该出现中日韩字符。这里故意让 `detail` 保持 ASCII，
    /// 以便断言的是**外框**被正确翻译了。
    #[test]
    fn english_rendering_is_actually_english() {
        let has_cjk = |s: &str| {
            s.chars().any(|c| {
                let u = c as u32;
                (0x3000..=0x303F).contains(&u)
                    || (0x3400..=0x4DBF).contains(&u)
                    || (0x4E00..=0x9FFF).contains(&u)
                    || (0xF900..=0xFAFF).contains(&u)
                    || (0xFF00..=0xFFEF).contains(&u)
            })
        };
        for e in all_core_errors() {
            let s = e.localized(Lang::En);
            assert!(!has_cjk(&s), "CoreError::{e:?} 的英文渲染仍有中文：{s}");
            assert!(
                !s.contains("{}"),
                "CoreError::{e:?} 的英文渲染有未替换的占位符：{s}"
            );
        }
        for e in all_probe_errors() {
            let s = e.localized(Lang::En);
            assert!(!has_cjk(&s), "ProbeError::{e:?} 的英文渲染仍有中文：{s}");
            assert!(
                !s.contains("{}"),
                "ProbeError::{e:?} 的英文渲染有未替换的占位符：{s}"
            );
        }
    }

    /// 中文渲染同样不能留下未替换的占位符。
    #[test]
    fn chinese_rendering_substitutes_every_placeholder() {
        for e in all_core_errors() {
            assert!(!e.localized(Lang::Zh).contains("{}"), "CoreError::{e:?}");
        }
        for e in all_probe_errors() {
            assert!(!e.localized(Lang::Zh).contains("{}"), "ProbeError::{e:?}");
        }
    }
}
