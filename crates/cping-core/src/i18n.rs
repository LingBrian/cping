//! 国际化（i18n）：中 / 英双语。
//!
//! # 为什么核心层也要有字典
//!
//! 三端（CLI / TUI / GUI）都要能把同一句「目标落在保留网段」翻成各自的语言。
//! 如果核心层只吐中文，界面层就只能**复刻**一遍判定逻辑才知道该显示什么，
//! 从而得到三份会各自漂移的实现。所以核心层产出「键 + 参数」，界面负责渲染。
//!
//! # 三条设计约束
//!
//! 1. **核心层产出键，不产出句子**。界面拿到 [`Key`] 再调 [`render`]。
//! 2. **键是枚举，不是字符串**。`t("typo")` 在运行期会静默显示成键名本身，
//!    而 `Key::Typo` 直接编译不过。
//! 3. **「翻译得好不好」没法自动验证，「漏译」能**：
//!    [`tests::english_has_no_cjk`] 断言英文条目里不出现中日韩字符，
//!    [`tests::placeholders_match`] 断言中英条目的 `{}` 个数相同。
//!    这两条把最常犯的错变成 CI 红灯，而不是等用户截图来报。
//!
//! # 刻意不做的事
//!
//! - **CSV / JSON 的列名与字段名不本地化**。它们是**数据契约**：
//!   同一份报表在中文与英文机器上被下游脚本消费时，列名必须一致。
//!   HTML 报表的标题与说明可以本地化（那部分是给人读的）。
//! - **底层错误细节原样透传**。`Export("No such file (os error 2)")` 里的
//!   `detail` 来自操作系统，翻不了也不该翻；只本地化外面的那句话。

use std::fmt;

// ---------------------------------------------------------------------------
// 语言
// ---------------------------------------------------------------------------

/// 支持的语言。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lang {
    /// 简体中文（默认）。
    Zh,
    /// 英文。
    En,
}

impl Lang {
    /// 全部语言，顺序即语言选择器里的显示顺序。
    pub const ALL: [Lang; 2] = [Lang::Zh, Lang::En];

    /// 稳定的机器可读标识（配置文件、命令行、审计记录里用它）。
    pub const fn code(self) -> &'static str {
        match self {
            Lang::Zh => "zh",
            Lang::En => "en",
        }
    }

    /// 用**该语言自称**写出来的名字，用于语言选择器。
    ///
    /// 「中文 / English」而不是「中文 / Chinese」：用户看不出当前界面在说什么时，
    /// 唯一能认出的是自己那门语言的自称。
    pub const fn label(self) -> &'static str {
        match self {
            Lang::Zh => "中文",
            Lang::En => "English",
        }
    }

    /// 解析命令行 / 配置文件里的语言标识。
    ///
    /// 接受 `zh` / `zh-CN` / `zh_CN.UTF-8` / `en` / `en_US` 等形式。
    /// **`C` 与 `POSIX` 返回 `None`**——那是「没有本地化偏好」，
    /// 不是「请用英文」；把它当成英文会让服务器的默认 locale 决定用户看到什么。
    pub fn parse(raw: &str) -> Option<Lang> {
        let s = raw.trim();
        if s.is_empty() {
            return None;
        }
        // `zh_CN.UTF-8@euro` → 取 `zh_CN` → 取 `zh`
        let head = s.split(['.', '@']).next().unwrap_or(s);
        let head = head.split(['_', '-']).next().unwrap_or(head);
        match head.to_ascii_lowercase().as_str() {
            "zh" | "cn" | "chs" | "cht" | "chinese" => Some(Lang::Zh),
            "en" | "english" => Some(Lang::En),
            _ => None,
        }
    }

    /// 从环境变量推断语言。
    ///
    /// 优先级：`CPING_LANG` → `LC_ALL` → `LC_MESSAGES` → `LANG`。
    /// `CPING_LANG` 放最前，因为**显式覆盖比环境推断可信**；
    /// 后面三个是 POSIX 规定的顺序（`LC_ALL` 覆盖一切，`LANG` 兜底）。
    pub fn from_env() -> Lang {
        let cp = std::env::var("CPING_LANG").ok();
        let all = std::env::var("LC_ALL").ok();
        let messages = std::env::var("LC_MESSAGES").ok();
        let lang = std::env::var("LANG").ok();
        detect_from_values(
            cp.as_deref(),
            all.as_deref(),
            messages.as_deref(),
            lang.as_deref(),
            Lang::default(),
        )
    }

    /// 一行摘要，例如 `zh（中文）`，供 `--lang` 的帮助文本与语言选择器使用。
    pub fn describe(self) -> String {
        format!("{}（{}）", self.code(), self.label())
    }
}

impl Default for Lang {
    /// 默认中文：本项目的原始用户群与原始软件的语种都是中文。
    fn default() -> Self {
        Lang::Zh
    }
}

impl fmt::Display for Lang {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// [`Lang::from_env`] 的**纯函数内核**。
///
/// 单独拆出来是为了能测：直接读写进程环境变量的测试会互相污染
/// （cargo test 默认多线程），而「优先级顺序」恰恰是最容易写错的逻辑。
fn detect_from_values(
    cping_lang: Option<&str>,
    lc_all: Option<&str>,
    lc_messages: Option<&str>,
    lang: Option<&str>,
    fallback: Lang,
) -> Lang {
    for v in [cping_lang, lc_all, lc_messages, lang]
        .into_iter()
        .flatten()
    {
        // 空串（`LC_ALL=` 是很常见的写法）与 `C` / `POSIX` 都表示
        // 「未指定偏好」，继续往下找，而不是当成某种语言。
        if let Some(l) = Lang::parse(v) {
            return l;
        }
    }
    fallback
}

// ---------------------------------------------------------------------------
// 键
// ---------------------------------------------------------------------------

/// 核心层面向用户的消息键。
///
/// 新增变体后必须同时做两件事，否则 [`tests::table_covers_every_key`] 会失败：
/// 在 [`ordinal`] 里登记序号，并在 [`TABLE`] 里给出中英条目。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    // —— CoreError 外框 ——
    /// 参数：`前缀/原因`
    ErrInvalidPrefix,
    /// 参数：`实际数量`、`上限`
    ErrTooManyTargets,
    /// 参数：`地址`、`原因`
    ErrReservedTarget,
    /// 参数：`细节`
    ErrInvalidConfig,
    /// 参数：`细节`
    ErrExport,
    /// 参数：`细节`
    ErrSnapshot,
    /// 参数：`细节`
    ErrUnsupported,

    // —— ProbeError 外框 ——
    /// 参数：`毫秒`
    ProbeTimeout,
    /// 参数：`细节`
    ProbeUnreachable,
    /// 参数：`细节`
    ProbePermission,
    /// 参数：`细节`
    ProbeUnsupported,
    /// 参数：`细节`
    ProbeInternal,

    // —— 保留网段的原因 ——
    RsrvMulticastV4,
    RsrvReservedRangeV4,
    RsrvThisNetwork,
    RsrvTestNet1,
    RsrvTestNet2,
    RsrvTestNet3,
    RsrvBenchmarkNet,
    RsrvUnspecifiedV6,
    RsrvMulticastV6,
    RsrvDocNetV6,

    // —— 扫描策略 ——
    /// 参数：`设定值`、`允许的最大值`
    PolicyPpsTooHigh,
    PolicyTargetsZero,
    /// 参数：`设定值`、`结构上限`
    PolicyTargetsTooHigh,
    PolicyPpsUnlimited,
    /// 参数：`每秒包数`
    PolicyPpsValue,
    /// 参数：`限速描述`、`目标上限`
    PolicyDescribe,
    PolicyDescribeReserved,

    // —— 首次授权告知 ——
    ConsentLine1,
    ConsentLine2,
    ConsentLine3,
    ConsentLine4,

    // —— 枚举标签 ——
    // 这些是三端「状态列 / 分类 / 类别名」直接显示给人的字。
    // 以前它们只有一份 `label_zh()`，英文界面下就露出中文。
    /// 告警类别：新增主机
    LabelAlertAppeared,
    /// 告警类别：主机离线
    LabelAlertDisappeared,
    /// 告警类别：延迟超阈
    LabelAlertLatencyAbove,
    /// 告警类别：可用率偏低
    LabelAlertUptimeBelow,
    /// 严重级：提示
    LabelSeverityInfo,
    /// 严重级：警告
    LabelSeverityWarning,
    /// 严重级：严重
    LabelSeverityCritical,
    /// 延迟档：低
    LabelLatencyLow,
    /// 延迟档：中
    LabelLatencyMid,
    /// 延迟档：高
    LabelLatencyHigh,
    /// 结果类别：低延迟
    LabelResultLow,
    /// 结果类别：中延迟
    LabelResultMid,
    /// 结果类别：高延迟
    LabelResultHigh,
    /// 结果类别：无回应
    LabelResultOffline,
    /// 结果类别：错误
    LabelResultError,
    /// 两次扫描差异：新上线
    LabelDiffAppeared,
    /// 两次扫描差异：已离线
    LabelDiffDisappeared,
    /// 两次扫描差异：延迟变化
    LabelDiffLatencyChanged,
    /// 两次扫描差异：状态变化
    LabelDiffStatusChanged,
    /// 两次扫描差异：仅旧结果
    LabelDiffOnlyInBase,
    /// 两次扫描差异：仅新结果
    LabelDiffOnlyInNext,
    /// 设备类型推断的置信度：低
    LabelConfidenceLow,
    /// 设备类型推断的置信度：中
    LabelConfidenceMedium,
    /// 监控变化：上线
    LabelChangeAppeared,
    /// 监控变化：离线
    LabelChangeDisappeared,
    /// 监控变化：延迟档变化
    LabelChangeRttClassChanged,
    /// 监控变化：状态变化
    LabelChangeStatusChanged,
    /// 审计事件：会话开始
    LabelAuditSessionStart,
    /// 审计事件：轮次
    LabelAuditRound,
    /// 审计事件：会话结束
    LabelAuditSessionEnd,
    /// 后端：原生 ICMP（IPv4）
    LabelBackendIcmp4,
    /// 后端：原生 ICMP（IPv6）
    LabelBackendIcmp6,
    /// 后端：TCP 连接
    LabelBackendTcp,
    /// 后端：模拟
    LabelBackendMock,
    /// 探测状态：等待
    LabelStatusPending,
    /// 探测状态：在线
    LabelStatusOnline,
    /// 探测状态：超时
    LabelStatusTimeout,
    /// 探测状态：不可达
    LabelStatusUnreachable,
    /// 探测状态：权限不足
    LabelStatusPermissionDenied,
    /// 探测状态：不支持
    LabelStatusUnsupported,
    /// 探测状态：错误
    LabelStatusError,
    /// 目标范围：标准
    LabelRangeStandard,
    /// 目标范围：全段
    LabelRangeFull,
    /// 目标范围：仅主机
    LabelRangeHostsOnly,

    // —— 后端能力摘要（`BackendRegistry::summary_in` 的碎片）——
    /// 参数：无
    RegistryIcmpAvailablePrivileged,
    RegistryIcmpAvailable,
    RegistryIcmpUnavailable,
    RegistryTcpAvailable,
    RegistryIcmp6Missing,
    /// 摘要里的连接符
    RegistrySeparator,
}

/// 字典条目。
#[derive(Debug, Clone, Copy)]
pub struct Entry {
    pub key: Key,
    pub zh: &'static str,
    pub en: &'static str,
}

/// 键 → 稳定序号。
///
/// 这个 `match` 是**穷尽**的：新增了 [`Key`] 变体却忘了在这里登记，
/// 编译就会失败。登记了却忘了加进 [`TABLE`]，
/// [`tests::table_covers_every_key`] 会因为序号不连续而失败。
/// 两道关卡合起来保证「字典不会漏键」。
const fn ordinal(key: Key) -> usize {
    match key {
        Key::ErrInvalidPrefix => 0,
        Key::ErrTooManyTargets => 1,
        Key::ErrReservedTarget => 2,
        Key::ErrInvalidConfig => 3,
        Key::ErrExport => 4,
        Key::ErrSnapshot => 5,
        Key::ErrUnsupported => 6,
        Key::ProbeTimeout => 7,
        Key::ProbeUnreachable => 8,
        Key::ProbePermission => 9,
        Key::ProbeUnsupported => 10,
        Key::ProbeInternal => 11,
        Key::RsrvMulticastV4 => 12,
        Key::RsrvReservedRangeV4 => 13,
        Key::RsrvThisNetwork => 14,
        Key::RsrvTestNet1 => 15,
        Key::RsrvTestNet2 => 16,
        Key::RsrvTestNet3 => 17,
        Key::RsrvBenchmarkNet => 18,
        Key::RsrvUnspecifiedV6 => 19,
        Key::RsrvMulticastV6 => 20,
        Key::RsrvDocNetV6 => 21,
        Key::PolicyPpsTooHigh => 22,
        Key::PolicyTargetsZero => 23,
        Key::PolicyTargetsTooHigh => 24,
        Key::PolicyPpsUnlimited => 25,
        Key::PolicyPpsValue => 26,
        Key::PolicyDescribe => 27,
        Key::PolicyDescribeReserved => 28,
        Key::ConsentLine1 => 29,
        Key::ConsentLine2 => 30,
        Key::ConsentLine3 => 31,
        Key::ConsentLine4 => 32,
        Key::LabelAlertAppeared => 33,
        Key::LabelAlertDisappeared => 34,
        Key::LabelAlertLatencyAbove => 35,
        Key::LabelAlertUptimeBelow => 36,
        Key::LabelSeverityInfo => 37,
        Key::LabelSeverityWarning => 38,
        Key::LabelSeverityCritical => 39,
        Key::LabelLatencyLow => 40,
        Key::LabelLatencyMid => 41,
        Key::LabelLatencyHigh => 42,
        Key::LabelResultLow => 43,
        Key::LabelResultMid => 44,
        Key::LabelResultHigh => 45,
        Key::LabelResultOffline => 46,
        Key::LabelResultError => 47,
        Key::LabelDiffAppeared => 48,
        Key::LabelDiffDisappeared => 49,
        Key::LabelDiffLatencyChanged => 50,
        Key::LabelDiffStatusChanged => 51,
        Key::LabelDiffOnlyInBase => 52,
        Key::LabelDiffOnlyInNext => 53,
        Key::LabelConfidenceLow => 54,
        Key::LabelConfidenceMedium => 55,
        Key::LabelChangeAppeared => 56,
        Key::LabelChangeDisappeared => 57,
        Key::LabelChangeRttClassChanged => 58,
        Key::LabelChangeStatusChanged => 59,
        Key::LabelAuditSessionStart => 60,
        Key::LabelAuditRound => 61,
        Key::LabelAuditSessionEnd => 62,
        Key::LabelBackendIcmp4 => 63,
        Key::LabelBackendIcmp6 => 64,
        Key::LabelBackendTcp => 65,
        Key::LabelBackendMock => 66,
        Key::LabelStatusPending => 67,
        Key::LabelStatusOnline => 68,
        Key::LabelStatusTimeout => 69,
        Key::LabelStatusUnreachable => 70,
        Key::LabelStatusPermissionDenied => 71,
        Key::LabelStatusUnsupported => 72,
        Key::LabelStatusError => 73,
        Key::LabelRangeStandard => 74,
        Key::LabelRangeFull => 75,
        Key::LabelRangeHostsOnly => 76,
        Key::RegistryIcmpAvailablePrivileged => 77,
        Key::RegistryIcmpAvailable => 78,
        Key::RegistryIcmpUnavailable => 79,
        Key::RegistryTcpAvailable => 80,
        Key::RegistryIcmp6Missing => 81,
        Key::RegistrySeparator => 82,
    }
}

/// 字典里的键总数。
pub const KEY_COUNT: usize = 83;

/// 全部条目。
///
/// 条目里的 `{}` 是顺序占位符，个数与顺序必须中英一致（有测试守着）。
/// 不引入具名占位符是为了让两种语言的语序可以完全不同——
/// 英文 `{} targets exceed the limit {}` 与中文 `目标数 {} 超过上限 {}`
/// 的参数顺序恰好相同只是巧合，不能依赖。
pub static TABLE: &[Entry] = &[
    // —— CoreError 外框 ——
    Entry {
        key: Key::ErrInvalidPrefix,
        zh: "网段前缀无效：{}",
        en: "invalid network prefix: {}",
    },
    Entry {
        key: Key::ErrTooManyTargets,
        zh: "目标数量 {} 超出上限 {}",
        en: "target count {} exceeds the limit {}",
    },
    Entry {
        key: Key::ErrReservedTarget,
        zh: "目标 {} 属于保留网段：{}",
        en: "target {} is in a reserved range: {}",
    },
    Entry {
        key: Key::ErrInvalidConfig,
        zh: "配置无效：{}",
        en: "invalid configuration: {}",
    },
    Entry {
        key: Key::ErrExport,
        zh: "导出失败：{}",
        en: "export failed: {}",
    },
    Entry {
        key: Key::ErrSnapshot,
        zh: "快照不可用：{}",
        en: "snapshot unavailable: {}",
    },
    Entry {
        key: Key::ErrUnsupported,
        zh: "不支持的操作：{}",
        en: "unsupported operation: {}",
    },
    // —— ProbeError 外框 ——
    Entry {
        key: Key::ProbeTimeout,
        zh: "超时（{} ms 内无应答）",
        en: "timed out (no reply within {} ms)",
    },
    Entry {
        key: Key::ProbeUnreachable,
        zh: "目标不可达：{}",
        en: "target unreachable: {}",
    },
    Entry {
        key: Key::ProbePermission,
        zh: "权限不足：{}",
        en: "permission denied: {}",
    },
    Entry {
        key: Key::ProbeUnsupported,
        zh: "当前平台不支持：{}",
        en: "not supported on this platform: {}",
    },
    Entry {
        key: Key::ProbeInternal,
        zh: "内部错误：{}",
        en: "internal error: {}",
    },
    // —— 保留网段的原因 ——
    Entry {
        key: Key::RsrvMulticastV4,
        zh: "组播地址：一个探测会打扰整组主机",
        en: "multicast address: one probe disturbs the entire group",
    },
    Entry {
        key: Key::RsrvReservedRangeV4,
        zh: "保留段（含 255.255.255.255 广播），不属于任何主机",
        en: "reserved range (includes the 255.255.255.255 broadcast); no host lives here",
    },
    Entry {
        key: Key::RsrvThisNetwork,
        zh: "「本网络」（RFC 1122 §3.2.1.3），不是可探测的目标",
        en: "\"this network\" (RFC 1122 3.2.1.3); not a probeable target",
    },
    Entry {
        key: Key::RsrvTestNet1,
        zh: "文档示例网段 TEST-NET-1（RFC 5737），公网不会路由",
        en: "documentation range TEST-NET-1 (RFC 5737); never routed on the public Internet",
    },
    Entry {
        key: Key::RsrvTestNet2,
        zh: "文档示例网段 TEST-NET-2（RFC 5737），公网不会路由",
        en: "documentation range TEST-NET-2 (RFC 5737); never routed on the public Internet",
    },
    Entry {
        key: Key::RsrvTestNet3,
        zh: "文档示例网段 TEST-NET-3（RFC 5737），公网不会路由",
        en: "documentation range TEST-NET-3 (RFC 5737); never routed on the public Internet",
    },
    Entry {
        key: Key::RsrvBenchmarkNet,
        zh: "基准测试网段（RFC 2544），公网不会路由",
        en: "benchmarking range (RFC 2544); never routed on the public Internet",
    },
    Entry {
        key: Key::RsrvUnspecifiedV6,
        zh: "未指定地址 `::`，不是可探测的目标",
        en: "unspecified address `::`; not a probeable target",
    },
    Entry {
        key: Key::RsrvMulticastV6,
        zh: "IPv6 组播地址：一个探测会打扰整组主机",
        en: "IPv6 multicast address: one probe disturbs the entire group",
    },
    Entry {
        key: Key::RsrvDocNetV6,
        zh: "文档示例网段 2001:db8::/32（RFC 3849），公网不会路由",
        en: "documentation range 2001:db8::/32 (RFC 3849); never routed on the public Internet",
    },
    // —— 扫描策略 ——
    Entry {
        key: Key::PolicyPpsTooHigh,
        zh: "速率上限 {} 包/秒 超过允许的最大值 {}（要关闭限速请写 0）",
        en: "rate limit {} pps exceeds the maximum {} (write 0 to disable the rate limit)",
    },
    Entry {
        key: Key::PolicyTargetsZero,
        zh: "目标数上限必须大于 0",
        en: "the target limit must be greater than 0",
    },
    Entry {
        key: Key::PolicyTargetsTooHigh,
        zh: "目标数上限 {} 超过本程序的结构上限 {}（只能收紧，不能放宽）",
        en: "target limit {} exceeds this program's structural maximum {} (it can only be tightened, never relaxed)",
    },
    Entry {
        key: Key::PolicyPpsUnlimited,
        zh: "不限速",
        en: "unlimited",
    },
    Entry {
        key: Key::PolicyPpsValue,
        zh: "{} 包/秒",
        en: "{} pps",
    },
    Entry {
        key: Key::PolicyDescribe,
        zh: "限速 {} · 目标上限 {}",
        en: "rate limit {} · target limit {}",
    },
    Entry {
        key: Key::PolicyDescribeReserved,
        zh: " · 允许保留网段",
        en: " · reserved ranges allowed",
    },
    // —— 首次授权告知 ——
    Entry {
        key: Key::ConsentLine1,
        zh: "CPing 只会向你拥有或获授权管理的网络发起探测。",
        en: "CPing only probes networks you own or are authorised to manage.",
    },
    Entry {
        key: Key::ConsentLine2,
        zh: "未经授权的端口扫描与主机探测在多数司法辖区是违法的。",
        en: "Unauthorised port scanning and host probing is illegal in most jurisdictions.",
    },
    Entry {
        key: Key::ConsentLine3,
        zh: "默认已开启速率上限，并对组播 / 保留段做了拒绝；请勿为了「快一点」而关掉它们。",
        en: "A rate limit is on by default and multicast / reserved ranges are refused; do not turn these off just to go faster.",
    },
    Entry {
        key: Key::ConsentLine4,
        zh: "继续使用即表示你确认自己对所扫描的网络拥有授权。",
        en: "By continuing you confirm that you are authorised to scan the networks in question.",
    },
    // —— 枚举标签 ——
    Entry {
        key: Key::LabelAlertAppeared,
        zh: "新增主机",
        en: "new host",
    },
    Entry {
        key: Key::LabelAlertDisappeared,
        zh: "主机离线",
        en: "host offline",
    },
    Entry {
        key: Key::LabelAlertLatencyAbove,
        zh: "延迟超阈",
        en: "latency above threshold",
    },
    Entry {
        key: Key::LabelAlertUptimeBelow,
        zh: "可用率偏低",
        en: "uptime below threshold",
    },
    Entry {
        key: Key::LabelSeverityInfo,
        zh: "提示",
        en: "info",
    },
    Entry {
        key: Key::LabelSeverityWarning,
        zh: "警告",
        en: "warning",
    },
    Entry {
        key: Key::LabelSeverityCritical,
        zh: "严重",
        en: "critical",
    },
    Entry {
        key: Key::LabelLatencyLow,
        zh: "低延迟",
        en: "low latency",
    },
    Entry {
        key: Key::LabelLatencyMid,
        zh: "中延迟",
        en: "medium latency",
    },
    Entry {
        key: Key::LabelLatencyHigh,
        zh: "高延迟",
        en: "high latency",
    },
    Entry {
        key: Key::LabelResultLow,
        zh: "低延迟",
        en: "low latency",
    },
    Entry {
        key: Key::LabelResultMid,
        zh: "中延迟",
        en: "medium latency",
    },
    Entry {
        key: Key::LabelResultHigh,
        zh: "高延迟",
        en: "high latency",
    },
    Entry {
        key: Key::LabelResultOffline,
        zh: "无回应",
        en: "no reply",
    },
    Entry {
        key: Key::LabelResultError,
        zh: "错误",
        en: "error",
    },
    Entry {
        key: Key::LabelDiffAppeared,
        zh: "新上线",
        en: "appeared",
    },
    Entry {
        key: Key::LabelDiffDisappeared,
        zh: "已离线",
        en: "disappeared",
    },
    Entry {
        key: Key::LabelDiffLatencyChanged,
        zh: "延迟变化",
        en: "latency changed",
    },
    Entry {
        key: Key::LabelDiffStatusChanged,
        zh: "状态变化",
        en: "status changed",
    },
    Entry {
        key: Key::LabelDiffOnlyInBase,
        zh: "仅旧结果",
        en: "only in previous",
    },
    Entry {
        key: Key::LabelDiffOnlyInNext,
        zh: "仅新结果",
        en: "only in next",
    },
    Entry {
        key: Key::LabelConfidenceLow,
        zh: "低",
        en: "low",
    },
    Entry {
        key: Key::LabelConfidenceMedium,
        zh: "中",
        en: "medium",
    },
    Entry {
        key: Key::LabelChangeAppeared,
        zh: "上线",
        en: "came online",
    },
    Entry {
        key: Key::LabelChangeDisappeared,
        zh: "离线",
        en: "went offline",
    },
    Entry {
        key: Key::LabelChangeRttClassChanged,
        zh: "延迟档变化",
        en: "latency class changed",
    },
    Entry {
        key: Key::LabelChangeStatusChanged,
        zh: "状态变化",
        en: "status changed",
    },
    Entry {
        key: Key::LabelAuditSessionStart,
        zh: "会话开始",
        en: "session start",
    },
    Entry {
        key: Key::LabelAuditRound,
        zh: "轮次结束",
        en: "round",
    },
    Entry {
        key: Key::LabelAuditSessionEnd,
        zh: "会话结束",
        en: "session end",
    },
    Entry {
        key: Key::LabelBackendIcmp4,
        zh: "原生 ICMP（IPv4）",
        en: "native ICMP (IPv4)",
    },
    Entry {
        key: Key::LabelBackendIcmp6,
        zh: "原生 ICMP（IPv6）",
        en: "native ICMP (IPv6)",
    },
    Entry {
        key: Key::LabelBackendTcp,
        zh: "TCP 连接",
        en: "TCP connect",
    },
    Entry {
        key: Key::LabelBackendMock,
        zh: "模拟（测试）",
        en: "mock (test)",
    },
    Entry {
        key: Key::LabelStatusPending,
        zh: "等待",
        en: "pending",
    },
    Entry {
        key: Key::LabelStatusOnline,
        zh: "在线",
        en: "online",
    },
    Entry {
        key: Key::LabelStatusTimeout,
        zh: "超时",
        en: "timed out",
    },
    Entry {
        key: Key::LabelStatusUnreachable,
        zh: "不可达",
        en: "unreachable",
    },
    Entry {
        key: Key::LabelStatusPermissionDenied,
        zh: "权限不足",
        en: "permission denied",
    },
    Entry {
        key: Key::LabelStatusUnsupported,
        zh: "不支持",
        en: "unsupported",
    },
    Entry {
        key: Key::LabelStatusError,
        zh: "错误",
        en: "error",
    },
    Entry {
        key: Key::LabelRangeStandard,
        zh: "标准（.1–.255，255 个）",
        en: "standard (.1-.255, 255 hosts)",
    },
    Entry {
        key: Key::LabelRangeFull,
        zh: "全段（.0–.255，256 个）",
        en: "full (.0-.255, 256 hosts)",
    },
    Entry {
        key: Key::LabelRangeHostsOnly,
        zh: "仅主机（.1–.254，254 个）",
        en: "hosts only (.1-.254, 254 hosts)",
    },
    // —— 后端能力摘要 ——
    Entry {
        key: Key::RegistryIcmpAvailablePrivileged,
        zh: "ICMP 可用（需要原始套接字权限，仅 IPv4）",
        en: "ICMP available (needs raw-socket privilege, IPv4 only)",
    },
    Entry {
        key: Key::RegistryIcmpAvailable,
        zh: "ICMP 可用（仅 IPv4）",
        en: "ICMP available (IPv4 only)",
    },
    Entry {
        key: Key::RegistryIcmpUnavailable,
        zh: "ICMP 不可用",
        en: "ICMP unavailable",
    },
    Entry {
        key: Key::RegistryTcpAvailable,
        zh: "TCP 可用（IPv4 + IPv6）",
        en: "TCP available (IPv4 + IPv6)",
    },
    Entry {
        key: Key::RegistryIcmp6Missing,
        zh: "ICMPv6 未实装，IPv6 目标请用 TCP",
        en: "ICMPv6 not implemented; use TCP for IPv6 targets",
    },
    Entry {
        key: Key::RegistrySeparator,
        zh: "；",
        en: "; ",
    },
];

/// 取某个键在指定语言下的模板。
pub fn text(lang: Lang, key: Key) -> &'static str {
    let idx = ordinal(key);
    // `TABLE` 与 `ordinal` 的一致性由 `table_covers_every_key` 守着；
    // 真跑到这里说明有人绕过了那道测试，此时**返回中文兜底**也好过 panic——
    // 界面显示一句中文，比整个程序崩掉好。
    if let Some(e) = TABLE.get(idx) {
        return match lang {
            Lang::Zh => e.zh,
            Lang::En => e.en,
        };
    }
    match lang {
        Lang::Zh => "（未登记的消息键）",
        Lang::En => "(unregistered message key)",
    }
}

/// 渲染带参数的模板。
///
/// 参数不足时**原样保留 `{}`**，而不是悄悄吞掉：界面上看见一个 `{}`
/// 比看见一句读起来通顺、但少了关键数字的话更容易定位问题。
/// 参数多余时忽略多余的（多传一个参数不该炸）。
pub fn render_template(tpl: &str, args: &[&str]) -> String {
    let mut out = String::with_capacity(tpl.len() + 32);
    let mut rest = tpl;
    let mut i = 0usize;
    while let Some(pos) = rest.find("{}") {
        out.push_str(&rest[..pos]);
        match args.get(i) {
            Some(a) => out.push_str(a),
            None => out.push_str("{}"),
        }
        rest = &rest[pos + 2..];
        i += 1;
    }
    out.push_str(rest);
    out
}

/// 按核心层的键渲染。
///
/// CLI / TUI / GUI 各有自己的键空间（它们要说的话不一样：核心层描述
/// 「逻辑发生了什么」，界面描述「这个界面在说什么」），所以三端用
/// [`render_template`] 自己实现同名函数，只共用这里的算法。
pub fn render(lang: Lang, key: Key, args: &[&str]) -> String {
    render_template(text(lang, key), args)
}

/// 便捷形式：`t(Lang::En, Key::ErrExport)`。
pub fn t(lang: Lang, key: Key) -> &'static str {
    text(lang, key)
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// 中日韩字符与全角标点的范围。英文条目里出现这些，说明是漏译或
    /// 「先复制中文再改」时漏改的残留。
    fn has_cjk(s: &str) -> Option<char> {
        s.chars().find(|c| {
            let u = *c as u32;
            (0x3000..=0x303F).contains(&u)      // CJK 标点
                || (0x3040..=0x30FF).contains(&u) // 假名
                || (0x3400..=0x4DBF).contains(&u) // 扩展 A
                || (0x4E00..=0x9FFF).contains(&u) // 基本区
                || (0xF900..=0xFAFF).contains(&u) // 兼容表意
                || (0xFF00..=0xFFEF).contains(&u) // 全角
        })
    }

    fn placeholder_count(s: &str) -> usize {
        s.matches("{}").count()
    }

    #[test]
    fn table_covers_every_key() {
        // 双射：TABLE 的条目数 == 键总数，且每个序号恰好出现一次。
        // 新增变体只改了 `Key` 与 `ordinal` 而忘了加 TABLE，
        // 或者加 TABLE 时复制粘贴导致某个序号重复，都会在这里失败。
        assert_eq!(
            TABLE.len(),
            KEY_COUNT,
            "TABLE 条目数（{}）与 KEY_COUNT（{}）不一致",
            TABLE.len(),
            KEY_COUNT
        );
        let mut seen = HashSet::new();
        for e in TABLE {
            let o = ordinal(e.key);
            assert!(o < KEY_COUNT, "{:?} 的序号 {o} 越界", e.key);
            assert!(seen.insert(o), "{:?} 的序号 {o} 与其它键重复", e.key);
        }
        assert_eq!(seen.len(), KEY_COUNT, "有键没有出现在 TABLE 里");
    }

    #[test]
    fn placeholders_match() {
        // 中英条目的 `{}` 个数必须一致：不一致意味着某种语言下会显示出
        // 悬空的 `{}`，或者有个参数被静默丢掉。
        for e in TABLE {
            assert_eq!(
                placeholder_count(e.zh),
                placeholder_count(e.en),
                "{:?} 的占位符个数不一致：zh={:?} en={:?}",
                e.key,
                e.zh,
                e.en
            );
        }
    }

    #[test]
    fn english_has_no_cjk() {
        for e in TABLE {
            if let Some(c) = has_cjk(e.en) {
                panic!("{:?} 的英文条目里出现了中日韩字符 {c:?}：{:?}", e.key, e.en);
            }
        }
    }

    #[test]
    fn chinese_entries_are_not_empty_and_not_ascii_only() {
        // 反向检查：中文条目至少含一个中日韩字符。
        // 「英文条目漏译成了中文」由上一个测试抓，「中文条目被误填成英文」
        // 由这个抓——例如复制粘贴时把 en 的内容粘到了 zh 上。
        for e in TABLE {
            assert!(!e.zh.trim().is_empty(), "{:?} 的中文条目为空", e.key);
            assert!(!e.en.trim().is_empty(), "{:?} 的英文条目为空", e.key);
            assert!(
                has_cjk(e.zh).is_some(),
                "{:?} 的中文条目里没有任何中日韩字符，疑似误填成英文：{:?}",
                e.key,
                e.zh
            );
        }
    }

    #[test]
    fn render_substitutes_in_order() {
        let s = render(Lang::Zh, Key::ErrTooManyTargets, &["300", "255"]);
        assert_eq!(s, "目标数量 300 超出上限 255");
        let s = render(
            Lang::En,
            Key::ErrReservedTarget,
            &["224.0.0.1", "multicast"],
        );
        assert_eq!(s, "target 224.0.0.1 is in a reserved range: multicast");
    }

    #[test]
    fn render_without_args_uses_the_template_verbatim() {
        assert_eq!(
            render(Lang::Zh, Key::PolicyTargetsZero, &[]),
            "目标数上限必须大于 0"
        );
    }

    #[test]
    fn render_keeps_placeholder_when_args_run_short() {
        // 少给一个参数：`{}` 必须**留在原地**，而不是变成空串。
        let s = render(Lang::Zh, Key::ErrTooManyTargets, &["300"]);
        assert_eq!(s, "目标数量 300 超出上限 {}");
        // 多给参数则忽略，不 panic。
        let s = render(Lang::Zh, Key::PolicyTargetsZero, &["多余"]);
        assert_eq!(s, "目标数上限必须大于 0");
    }

    #[test]
    fn parse_accepts_locale_forms() {
        for raw in ["zh", "ZH", "zh-CN", "zh_CN.UTF-8", "cn", "Chinese"] {
            assert_eq!(Lang::parse(raw), Some(Lang::Zh), "解析 {raw:?}");
        }
        for raw in ["en", "EN", "en-US", "en_US.UTF-8", "English"] {
            assert_eq!(Lang::parse(raw), Some(Lang::En), "解析 {raw:?}");
        }
    }

    #[test]
    fn parse_rejects_unset_locales() {
        // 空串与 `C` / `POSIX` 都表示「没有偏好」，必须回退而不是猜成英文。
        for raw in ["", "   ", "C", "POSIX", "de", "ja_JP.UTF-8"] {
            assert_eq!(Lang::parse(raw), None, "{raw:?} 不该被解析成某种语言");
        }
    }

    #[test]
    fn env_priority_is_cping_then_lc_all_then_lc_messages_then_lang() {
        assert_eq!(
            detect_from_values(Some("en"), Some("zh_CN"), None, None, Lang::Zh),
            Lang::En,
            "CPING_LANG 必须能盖过 LC_ALL"
        );
        assert_eq!(
            detect_from_values(None, Some("en_US"), Some("zh_CN"), None, Lang::Zh),
            Lang::En,
            "LC_ALL 优先于 LC_MESSAGES"
        );
        assert_eq!(
            detect_from_values(None, None, Some("en_US"), Some("zh_CN"), Lang::Zh),
            Lang::En,
            "LC_MESSAGES 优先于 LANG"
        );
        assert_eq!(
            detect_from_values(None, None, None, Some("en_GB"), Lang::Zh),
            Lang::En
        );
    }

    #[test]
    fn empty_or_unset_env_falls_back() {
        assert_eq!(
            detect_from_values(Some(""), Some(""), Some("C"), Some("POSIX"), Lang::Zh),
            Lang::Zh
        );
        // 注意：`CPING_LANG=""` 时**继续往下找**，而不是直接回退——
        // 「显式设了但设成空」与「完全没设」在 shell 里难以区分，宽容处理。
        assert_eq!(
            detect_from_values(Some(""), None, None, Some("en_US"), Lang::Zh),
            Lang::En
        );
        assert_eq!(
            detect_from_values(None, None, None, None, Lang::En),
            Lang::En,
            "没有线索时必须用调用方给的兜底值"
        );
    }

    #[test]
    fn lang_metadata_is_stable() {
        assert_eq!(Lang::default(), Lang::Zh);
        assert_eq!(Lang::Zh.code(), "zh");
        assert_eq!(Lang::En.code(), "en");
        assert_eq!(Lang::En.label(), "English");
        assert_eq!(Lang::ALL.len(), 2);
    }

    #[test]
    fn unknown_key_falls_back_to_a_visible_placeholder() {
        // 直接构造一个未登记在 TABLE 里的键是不可能通过编译的（枚举是封闭的），
        // 所以这里只断言正常路径返回非空模板。
        for lang in Lang::ALL {
            for e in TABLE {
                assert!(!text(lang, e.key).is_empty());
            }
        }
    }

    /// 每个枚举标签的英文渲染都不能带中文，中文渲染也不能退化成英文。
    ///
    /// 光有 [`tests::english_has_no_cjk`]（它只查表）还不够：那条保证**条目**
    /// 是英文的，但保证不了某个枚举**映射到了正确的条目**——把
    /// `Severity::Info` 接到 `LabelSeverityCritical` 上，表还是干净的，
    /// 界面却显示错了。这条测试从枚举出发一路走到渲染结果，
    /// 才是「英文模式下不再露出中文」的总闸。
    #[test]
    fn enum_labels_are_localized_both_ways() {
        use crate::alerts::{AlertKind, Severity};
        use crate::classify::{LatencyClass, ResultClass};
        use crate::diff::DiffKind;
        use crate::fingerprint::Confidence;
        use crate::monitor::ChangeKind;
        use crate::policy::AuditEvent;
        use crate::registry::BackendId;
        use crate::result::ProbeStatus;
        use crate::target::TargetRange;

        let mut pairs: Vec<(String, String)> = Vec::new();

        for k in [
            AlertKind::Appeared,
            AlertKind::Disappeared,
            AlertKind::LatencyAbove,
            AlertKind::UptimeBelow,
        ] {
            pairs.push((k.label_in(Lang::Zh).into(), k.label_in(Lang::En).into()));
        }
        for s in [Severity::Info, Severity::Warning, Severity::Critical] {
            pairs.push((s.label_in(Lang::Zh).into(), s.label_in(Lang::En).into()));
        }
        for c in [LatencyClass::Low, LatencyClass::Mid, LatencyClass::High] {
            pairs.push((c.label_in(Lang::Zh).into(), c.label_in(Lang::En).into()));
        }
        for c in [
            ResultClass::Low,
            ResultClass::Mid,
            ResultClass::High,
            ResultClass::Offline,
            ResultClass::Error,
        ] {
            pairs.push((c.label_in(Lang::Zh).into(), c.label_in(Lang::En).into()));
        }
        for d in [
            DiffKind::Appeared,
            DiffKind::Disappeared,
            DiffKind::LatencyChanged,
            DiffKind::StatusChanged,
            DiffKind::OnlyInBase,
            DiffKind::OnlyInNext,
        ] {
            pairs.push((d.label_in(Lang::Zh).into(), d.label_in(Lang::En).into()));
        }
        for c in [Confidence::Low, Confidence::Medium] {
            pairs.push((c.label_in(Lang::Zh).into(), c.label_in(Lang::En).into()));
        }
        for c in [
            ChangeKind::Appeared,
            ChangeKind::Disappeared,
            ChangeKind::RttClassChanged,
            ChangeKind::StatusChanged,
        ] {
            pairs.push((c.label_in(Lang::Zh).into(), c.label_in(Lang::En).into()));
        }
        for e in [
            AuditEvent::SessionStart,
            AuditEvent::Round,
            AuditEvent::SessionEnd,
        ] {
            pairs.push((e.label_in(Lang::Zh).into(), e.label_in(Lang::En).into()));
        }
        for b in [
            BackendId::Icmp4,
            BackendId::Icmp6,
            BackendId::Tcp,
            BackendId::Mock,
        ] {
            pairs.push((b.label_in(Lang::Zh).into(), b.label_in(Lang::En).into()));
        }
        for st in [
            ProbeStatus::Pending,
            ProbeStatus::Online { rtt_ms: 3 },
            ProbeStatus::Timeout,
            ProbeStatus::Unreachable,
            ProbeStatus::PermissionDenied,
            ProbeStatus::Unsupported,
            ProbeStatus::Error("boom".into()),
        ] {
            pairs.push((st.label_in(Lang::Zh).into(), st.label_in(Lang::En).into()));
        }
        for r in [
            TargetRange::Standard,
            TargetRange::Full,
            TargetRange::HostsOnly,
        ] {
            pairs.push((r.label_in(Lang::Zh).into(), r.label_in(Lang::En).into()));
        }

        assert_eq!(pairs.len(), 44, "标签样本数变了：逐个核对过再改这个数字");
        for (zh, en) in pairs {
            assert!(
                has_cjk(&zh).is_some(),
                "中文标签里没有中日韩字符，疑似误填成英文：{zh:?}"
            );
            assert!(
                has_cjk(&en).is_none(),
                "英文标签里仍有中日韩字符，说明漏接了本地化：{en:?}"
            );
            assert_ne!(zh, en, "中英标签完全一样，疑似漏译：{zh:?}");
        }
    }
}
