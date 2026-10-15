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
    /// 列表分隔符（设备推断依据那类并列短语用）。
    ///
    /// 中文用顿号「、」，英文用 `"; "`——和 [`Key::RegistrySeparator`] 同一个理由：
    /// 把中文标点留在英文句子里，读起来是「一眼假」的机器翻译味。
    ListSeparator,

    /// 参数：`新上线数`、`已离线数`、`延迟变化数`、`状态变化数`、`仅旧数`、`仅新数`
    DiffSummary,

    // —— 自由文本（M18 第二步）——
    //
    // 第一步只把「枚举标签」本地化了；这些是**句子**——带参数、带语序的
    // 完整陈述。它们以前是散在各模块里的 `format!`，英文界面下原样露出中文。
    //
    // 划界原则：**只有散文进字典，标点不进**。`{} → {} ms` 这种纯符号模板
    // 留在代码里（`→` 在两种语言里都是 `→`），否则字典会被一堆
    // 「中英一字不差」的条目撑爆，真正需要复核的条目反而被淹掉。
    /// 参数：无
    SpecNoTargets,
    /// 参数：`网段数`、`目标数`
    SpecSegmentsAndTargets,
    /// 参数：`标签`、`个数`
    SegmentDescribe,
    ErrTcpPortZero,

    // TCP 模式的状态名。**刻意与 ICMP 的状态名不同一套词**：
    // `TCP-OPEN` 绝不能被读成「ICMP 可达」，所以这里不能直接复用
    // `LabelStatusOnline`。
    LabelModeTcpPending,
    LabelModeTcpOpen,
    LabelModeTcpTimeout,
    LabelModeTcpClosed,

    /// 参数：`地址`、`变化类型`、`细节`
    ChangeSummary,

    /// 参数：`片段`
    ErrPortRangeStartNotNumber,
    /// 参数：`片段`
    ErrPortRangeEndNotNumber,
    /// 参数：`片段`
    ErrPortRangeInvalid,
    /// 参数：`片段`、`上限`
    ErrPortRangeTooWide,
    /// 参数：`片段`
    ErrPortNotNumber,
    /// 参数：`片段`
    ErrPortOutOfRange,
    ErrPortListEmpty,

    // —— 设备类型（推断结论的名字）——
    LabelDeviceWindowsHost,
    LabelDeviceNetworkGear,
    LabelDeviceSamba,
    LabelDeviceUnixHost,
    LabelDevicePrinter,
    LabelDeviceRemoteDesktop,
    LabelDeviceDatabase,
    LabelDeviceWeb,

    /// 参数：`设备名`、`置信`、`依据`
    DeviceGuessSummary,
    /// 参数：`端口`、`服务名`
    BasisPortOpen,
    /// 参数：`端口`
    BasisPortOpenNoService,
    /// 参数：`厂商`
    BasisVendor,

    CapsNeedsPrivilege,
    CapsIpv4And6,
    CapsIpv4Only,
    CapsIpv6Only,
    CapsNotePlatformIcmpMissing,
    CapsNoteIcmp6Missing,
    CapsNoteTestOnly,
    CapsNoteUnknownBackend,

    /// 参数：`个数`
    ProfileTargetCount,
    ProfileSpecUnparsable,
    /// 参数：`并发数`
    ProfileConcurrency,

    HintIcmpWindows,
    HintIcmpAndroid,
    HintIcmpLinux,
    HintIcmpUnknown,

    NotifyConsoleAvailable,
    NotifySystemWindows,
    NotifySystemLinuxFound,
    NotifySystemLinuxMissing,
    NotifySystemNone,
    NotifyWebhookAvailable,
    NotifyCommandAvailable,
    NotifySmtpAvailable,

    /// 参数：`实际头`
    TsBadSignature,
    /// 参数：`文件版本`、`当前版本`
    TsBadVersion,
    TsNoVersion,
    TsEmpty,

    /// 参数：`实际头`
    AlertStateBadSignature,
    /// 参数：`文件版本`、`当前版本`
    AlertStateBadVersion,
    /// 参数：`路径`、`原因`
    AlertStateWriteFailed,
    /// 参数：`路径`、`原因`
    AlertStateReadFailed,
    AlertStateNoVersion,
    AlertStateEmpty,

    /// 参数：`地址`
    AlertAppeared,
    /// 参数：`地址`、`状态`
    AlertDisappeared,
    /// 参数：`地址`、`状态`、`细节`
    AlertDisappearedWithNote,
    /// 参数：`地址`、`实测`、`阈值`
    AlertLatencyAbove,
    /// 参数：`地址`、`实测`、`阈值`
    AlertUptimeBelow,
}

/// 中英**本来就该一模一样**的技术记号。
///
/// `TCP-OPEN`、`IPv4+IPv6` 是记号不是散文，两种语言下相同才是对的。
/// 但 [`tests::chinese_entries_are_not_empty_and_not_ascii_only`] 会把这些
/// 当成「中文栏被误填成英文」而报错。给它们一个显式白名单，
/// 比放宽那条测试要好：**放宽之后就再也抓不到真正的误填了**。
///
/// 这个列表越长，那条检查就越没用——加条目必须能说出理由。
/// [`tests::ascii_token_allowlist_is_exact`] 保证它不会腐烂：
/// 白名单多一条或少一条都会失败。
pub const ASCII_TOKEN_KEYS: &[Key] = &[Key::LabelModeTcpOpen, Key::CapsIpv4And6];

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
        Key::ListSeparator => 149,
        Key::DiffSummary => 150,
        Key::SpecNoTargets => 83,
        Key::SpecSegmentsAndTargets => 84,
        Key::SegmentDescribe => 85,
        Key::ErrTcpPortZero => 86,
        Key::LabelModeTcpPending => 87,
        Key::LabelModeTcpOpen => 88,
        Key::LabelModeTcpTimeout => 89,
        Key::LabelModeTcpClosed => 90,
        Key::ChangeSummary => 91,
        Key::ErrPortRangeStartNotNumber => 92,
        Key::ErrPortRangeEndNotNumber => 93,
        Key::ErrPortRangeInvalid => 94,
        Key::ErrPortRangeTooWide => 95,
        Key::ErrPortNotNumber => 96,
        Key::ErrPortOutOfRange => 97,
        Key::ErrPortListEmpty => 98,
        Key::LabelDeviceWindowsHost => 99,
        Key::LabelDeviceNetworkGear => 100,
        Key::LabelDeviceSamba => 101,
        Key::LabelDeviceUnixHost => 102,
        Key::LabelDevicePrinter => 103,
        Key::LabelDeviceRemoteDesktop => 104,
        Key::LabelDeviceDatabase => 105,
        Key::LabelDeviceWeb => 106,
        Key::DeviceGuessSummary => 107,
        Key::BasisPortOpen => 108,
        Key::BasisPortOpenNoService => 109,
        Key::BasisVendor => 110,
        Key::CapsNeedsPrivilege => 111,
        Key::CapsIpv4And6 => 112,
        Key::CapsIpv4Only => 113,
        Key::CapsIpv6Only => 114,
        Key::CapsNotePlatformIcmpMissing => 115,
        Key::CapsNoteIcmp6Missing => 116,
        Key::CapsNoteTestOnly => 117,
        Key::CapsNoteUnknownBackend => 118,
        Key::ProfileTargetCount => 119,
        Key::ProfileSpecUnparsable => 120,
        Key::ProfileConcurrency => 121,
        Key::HintIcmpWindows => 122,
        Key::HintIcmpAndroid => 123,
        Key::HintIcmpLinux => 124,
        Key::HintIcmpUnknown => 125,
        Key::NotifyConsoleAvailable => 126,
        Key::NotifySystemWindows => 127,
        Key::NotifySystemLinuxFound => 128,
        Key::NotifySystemLinuxMissing => 129,
        Key::NotifySystemNone => 130,
        Key::NotifyWebhookAvailable => 131,
        Key::NotifyCommandAvailable => 132,
        Key::NotifySmtpAvailable => 133,
        Key::TsBadSignature => 134,
        Key::TsBadVersion => 135,
        Key::TsNoVersion => 136,
        Key::TsEmpty => 137,
        Key::AlertStateBadSignature => 138,
        Key::AlertStateBadVersion => 139,
        Key::AlertStateWriteFailed => 140,
        Key::AlertStateReadFailed => 141,
        Key::AlertStateNoVersion => 142,
        Key::AlertStateEmpty => 143,
        Key::AlertAppeared => 144,
        Key::AlertDisappeared => 145,
        Key::AlertDisappearedWithNote => 146,
        Key::AlertLatencyAbove => 147,
        Key::AlertUptimeBelow => 148,
    }
}

/// 字典里的键总数。
pub const KEY_COUNT: usize = 151;

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
    // —— 自由文本（M18 第二步）——
    Entry {
        key: Key::SpecNoTargets,
        zh: "尚未设置目标",
        en: "no targets set yet",
    },
    Entry {
        key: Key::SpecSegmentsAndTargets,
        zh: "{} 个网段 / {} 个目标",
        en: "{} segments / {} targets",
    },
    Entry {
        key: Key::SegmentDescribe,
        zh: "{}（{} 个）",
        en: "{} ({} targets)",
    },
    Entry {
        key: Key::ErrTcpPortZero,
        zh: "TCP 端口不能为 0",
        en: "TCP port must not be zero",
    },
    Entry {
        key: Key::LabelModeTcpPending,
        zh: "等待",
        en: "pending",
    },
    Entry {
        // 技术记号：两种语言下都必须是 `TCP-OPEN`。
        key: Key::LabelModeTcpOpen,
        zh: "TCP-OPEN",
        en: "TCP-OPEN",
    },
    Entry {
        key: Key::LabelModeTcpTimeout,
        zh: "TCP-超时",
        en: "TCP-timeout",
    },
    Entry {
        key: Key::LabelModeTcpClosed,
        zh: "TCP-关闭",
        en: "TCP-closed",
    },
    Entry {
        key: Key::ChangeSummary,
        zh: "{} {}（{}）",
        en: "{} {} ({})",
    },
    Entry {
        key: Key::ErrPortRangeStartNotNumber,
        zh: "端口区间起点 {} 不是数字",
        en: "port range start {} is not a number",
    },
    Entry {
        key: Key::ErrPortRangeEndNotNumber,
        zh: "端口区间终点 {} 不是数字",
        en: "port range end {} is not a number",
    },
    Entry {
        key: Key::ErrPortRangeInvalid,
        zh: "端口区间 {} 无效（需 1-65535 且起点 ≤ 终点）",
        en: "invalid port range {} (must be 1-65535 with start <= end)",
    },
    Entry {
        key: Key::ErrPortRangeTooWide,
        zh: "端口区间 {} 跨度超过上限 {}",
        en: "port range {} spans more than the limit of {}",
    },
    Entry {
        key: Key::ErrPortNotNumber,
        zh: "端口 {} 不是数字",
        en: "port {} is not a number",
    },
    Entry {
        key: Key::ErrPortOutOfRange,
        zh: "端口 {} 超出 1-65535",
        en: "port {} is out of range 1-65535",
    },
    Entry {
        key: Key::ErrPortListEmpty,
        zh: "端口列表为空",
        en: "port list is empty",
    },
    Entry {
        key: Key::LabelDeviceWindowsHost,
        zh: "Windows 主机",
        en: "Windows host",
    },
    Entry {
        key: Key::LabelDeviceNetworkGear,
        zh: "网络设备（路由器/交换机）",
        en: "network device (router/switch)",
    },
    Entry {
        key: Key::LabelDeviceSamba,
        zh: "Windows/Samba 文件服务",
        en: "Windows/Samba file service",
    },
    Entry {
        key: Key::LabelDeviceUnixHost,
        zh: "Linux/Unix 主机",
        en: "Linux/Unix host",
    },
    Entry {
        key: Key::LabelDevicePrinter,
        zh: "打印机",
        en: "printer",
    },
    Entry {
        key: Key::LabelDeviceRemoteDesktop,
        zh: "远程桌面/VNC 端点",
        en: "remote desktop / VNC endpoint",
    },
    Entry {
        key: Key::LabelDeviceDatabase,
        zh: "数据库服务器",
        en: "database server",
    },
    Entry {
        key: Key::LabelDeviceWeb,
        zh: "Web 服务",
        en: "web service",
    },
    Entry {
        key: Key::DeviceGuessSummary,
        zh: "{}（推断，置信{}；依据：{}）",
        en: "{} (inferred, confidence: {}; basis: {})",
    },
    Entry {
        key: Key::BasisPortOpen,
        zh: "TCP {}({}) 开放",
        en: "TCP {}({}) open",
    },
    Entry {
        key: Key::BasisPortOpenNoService,
        zh: "TCP {} 开放",
        en: "TCP {} open",
    },
    Entry {
        key: Key::BasisVendor,
        zh: "厂商 {}",
        en: "vendor {}",
    },
    Entry {
        key: Key::CapsNeedsPrivilege,
        zh: "需要权限",
        en: "requires privileges",
    },
    Entry {
        // 技术记号：`IPv4+IPv6` 不是散文，两种语言下都是它。
        key: Key::CapsIpv4And6,
        zh: "IPv4+IPv6",
        en: "IPv4+IPv6",
    },
    Entry {
        key: Key::CapsIpv4Only,
        zh: "仅 IPv4",
        en: "IPv4 only",
    },
    Entry {
        key: Key::CapsIpv6Only,
        zh: "仅 IPv6",
        en: "IPv6 only",
    },
    Entry {
        key: Key::CapsNotePlatformIcmpMissing,
        zh: "该平台尚未实现原生 ICMP",
        en: "native ICMP is not implemented on this platform",
    },
    Entry {
        key: Key::CapsNoteIcmp6Missing,
        zh: "ICMPv6 尚未实装，请用 TCP 方式探测 IPv6 目标",
        en: "ICMPv6 is not implemented yet; probe IPv6 targets over TCP instead",
    },
    Entry {
        key: Key::CapsNoteTestOnly,
        zh: "仅测试用",
        en: "for testing only",
    },
    Entry {
        key: Key::CapsNoteUnknownBackend,
        zh: "未知后端",
        en: "unknown backend",
    },
    Entry {
        key: Key::ProfileTargetCount,
        zh: "{} 个目标",
        en: "{} targets",
    },
    Entry {
        key: Key::ProfileSpecUnparsable,
        zh: "目标规格无法解析",
        en: "target spec cannot be parsed",
    },
    Entry {
        key: Key::ProfileConcurrency,
        zh: "并发 {}",
        en: "concurrency {}",
    },
    Entry {
        key: Key::HintIcmpWindows,
        zh: "Windows：优先 SOCK_DGRAM+IPPROTO_ICMP（普通用户），失败回退 SOCK_RAW（需管理员）",
        en: "Windows: prefers SOCK_DGRAM+IPPROTO_ICMP (unprivileged), falls back to SOCK_RAW (needs administrator)",
    },
    Entry {
        key: Key::HintIcmpAndroid,
        zh: "Android：普通应用通常没有 CAP_NET_RAW。若两种套接字都返回 EPERM/EACCES，本工具的 ICMP 模式不可用，请改用独立的 TCP 连通性模式。",
        en: "Android: ordinary apps usually lack CAP_NET_RAW. If both socket types return EPERM/EACCES, this tool's ICMP mode is unavailable; use the separate TCP connectivity mode instead.",
    },
    Entry {
        key: Key::HintIcmpLinux,
        zh: "Linux：需要 CAP_NET_RAW（root 或 setcap cap_net_raw+ep <exe>）",
        en: "Linux: needs CAP_NET_RAW (root, or setcap cap_net_raw+ep <exe>)",
    },
    Entry {
        key: Key::HintIcmpUnknown,
        zh: "未知平台",
        en: "unknown platform",
    },
    Entry {
        key: Key::NotifyConsoleAvailable,
        zh: "打印到标准输出，任何平台都可用",
        en: "prints to stdout; available on every platform",
    },
    Entry {
        key: Key::NotifySystemWindows,
        zh: "通过 PowerShell 发 Windows Toast（需要 Windows 10 1607+）",
        en: "sends a Windows toast through PowerShell (needs Windows 10 1607+)",
    },
    Entry {
        key: Key::NotifySystemLinuxFound,
        zh: "调用 notify-send",
        en: "calls notify-send",
    },
    Entry {
        key: Key::NotifySystemLinuxMissing,
        zh: "未找到 notify-send（gnome/libnotify 未安装）",
        en: "notify-send not found (gnome/libnotify not installed)",
    },
    Entry {
        key: Key::NotifySystemNone,
        zh: "本平台没有内置的系统通知通道（Android/iOS 请用界面内提示或 command 通道）",
        en: "no built-in system notification channel on this platform (on Android/iOS use in-app alerts or the command channel)",
    },
    Entry {
        key: Key::NotifyWebhookAvailable,
        zh: "HTTP POST JSON；**仅 http://**，核心不含 TLS，https 会被明确拒绝",
        en: "HTTP POST JSON; **http:// only** — the core has no TLS, https is rejected explicitly",
    },
    Entry {
        key: Key::NotifyCommandAvailable,
        zh: "执行外部命令并把事件 JSON 从 stdin 喂入——接 curl / msmtp 等自带 TLS 的工具",
        en: "runs an external command and feeds the event JSON on stdin — pair it with curl / msmtp and similar tools that bring their own TLS",
    },
    Entry {
        key: Key::NotifySmtpAvailable,
        zh: "**明文** SMTP，不支持 TLS 与认证；仅适用于内网中继",
        en: "**plaintext** SMTP; no TLS and no auth — suitable only for an internal relay",
    },
    Entry {
        key: Key::TsBadSignature,
        zh: "不是 CPing 时序文件（头 {}）",
        en: "not a CPing time-series file (header {})",
    },
    Entry {
        key: Key::TsBadVersion,
        zh: "时序文件版本 {} 与当前 {} 不符，拒绝读取",
        en: "time-series file version {} does not match the current {}; refusing to read",
    },
    Entry {
        key: Key::TsNoVersion,
        zh: "时序文件缺少版本号",
        en: "time-series file is missing its version number",
    },
    Entry {
        key: Key::TsEmpty,
        zh: "时序文件为空",
        en: "time-series file is empty",
    },
    Entry {
        key: Key::AlertStateBadSignature,
        zh: "不是 CPing 告警状态文件（头 {}）",
        en: "not a CPing alert-state file (header {})",
    },
    Entry {
        key: Key::AlertStateBadVersion,
        zh: "告警状态文件版本 {} 与当前 {} 不符，拒绝读取",
        en: "alert-state file version {} does not match the current {}; refusing to read",
    },
    Entry {
        key: Key::AlertStateWriteFailed,
        zh: "写入 {} 失败：{}",
        en: "failed to write {}: {}",
    },
    Entry {
        key: Key::AlertStateReadFailed,
        zh: "读取 {} 失败：{}",
        en: "failed to read {}: {}",
    },
    Entry {
        key: Key::AlertStateNoVersion,
        zh: "告警状态文件缺少版本号",
        en: "alert-state file is missing its version number",
    },
    Entry {
        key: Key::AlertStateEmpty,
        zh: "告警状态文件为空",
        en: "alert-state file is empty",
    },
    Entry {
        key: Key::AlertAppeared,
        zh: "新增主机 {} 上线",
        en: "new host {} came online",
    },
    Entry {
        key: Key::AlertDisappeared,
        zh: "主机 {} 变为离线（{}）",
        en: "host {} went offline ({})",
    },
    Entry {
        key: Key::AlertDisappearedWithNote,
        zh: "主机 {} 变为离线（{}：{}）",
        en: "host {} went offline ({}: {})",
    },
    Entry {
        key: Key::AlertLatencyAbove,
        zh: "主机 {} 延迟 {} ms，超过阈值 {} ms",
        en: "host {} latency {} ms exceeds the threshold {} ms",
    },
    Entry {
        key: Key::AlertUptimeBelow,
        zh: "主机 {} 可用率 {}%，低于阈值 {}%",
        en: "host {} uptime {}% is below the threshold {}%",
    },
    // 注意：这一条必须留在**最后**——`TABLE` 是按下标取值的
    // （`text()` 做 `TABLE[ordinal(key)]`），所以 **TABLE 的顺序必须与
    // `ordinal` 完全一致**。往中间插条目会让后面所有键整体错位，
    // 而错位不会崩——只会静默显示成邻居那条消息。这也是
    // `table_covers_every_key` 之外我另外加一条测试的原因（见下）。
    Entry {
        key: Key::ListSeparator,
        zh: "、",
        en: "; ",
    },
    Entry {
        key: Key::DiffSummary,
        zh: "新上线 {} · 已离线 {} · 延迟变化 {} · 状态变化 {}（另：仅旧 {} / 仅新 {}）",
        en: "appeared {} · disappeared {} · latency changed {} · status changed {} (plus: base only {} / next only {})",
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
    fn table_order_matches_ordinal() {
        // `text()` 是 `TABLE[ordinal(key)]`——按下标取值，不是按 key 查找。
        // 所以 TABLE 的**顺序必须与 ordinal 一致**，而这一点
        // `table_covers_every_key` 是查不出来的：它只看序号集合是不是
        // `0..KEY_COUNT` 的一个排列，把任意两条互换位置它照样通过。
        //
        // 这个坑真的踩过：给 `Key` 追加变体时顺手把新条目插在了表格中间，
        // 结果后面所有键静默错位——`ICMP 需要权限`显示成了`厂商 {}`。
        // 不崩、不报错，只是所有消息都变成了邻居那一条。
        for (i, e) in TABLE.iter().enumerate() {
            assert_eq!(
                ordinal(e.key),
                i,
                "TABLE 第 {i} 条是 {:?}（序号 {}）——TABLE 必须严格按 ordinal 升序排列",
                e.key,
                ordinal(e.key)
            );
        }
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
        //
        // [`ASCII_TOKEN_KEYS`] 里的技术记号豁免（`TCP-OPEN` 本来就该长这样）。
        for e in TABLE {
            assert!(!e.zh.trim().is_empty(), "{:?} 的中文条目为空", e.key);
            assert!(!e.en.trim().is_empty(), "{:?} 的英文条目为空", e.key);
            if ASCII_TOKEN_KEYS.contains(&e.key) {
                continue;
            }
            assert!(
                has_cjk(e.zh).is_some(),
                "{:?} 的中文条目里没有任何中日韩字符，疑似误填成英文：{:?}",
                e.key,
                e.zh
            );
        }
    }

    #[test]
    fn ascii_token_allowlist_is_exact() {
        // 白名单是**精确匹配**的：多一条、少一条都失败。
        //
        // 少了：新写的技术记号会被 `chinese_entries_are_not_empty_and_not_ascii_only`
        // 报错，逼作者回来看这里；多了：说明某个条目已经从记号变成了散文
        // （或者条目被删了），白名单得跟着瘦下来。
        // 没有这条，白名单会慢慢变成一张「哪些错可以不报」的清单。
        let offenders: Vec<Key> = TABLE
            .iter()
            .filter(|e| has_cjk(e.zh).is_none())
            .map(|e| e.key)
            .collect();
        for k in &offenders {
            assert!(
                ASCII_TOKEN_KEYS.contains(k),
                "{k:?} 的中文条目不含中日韩字符，但它不在 ASCII_TOKEN_KEYS 里"
            );
        }
        for k in ASCII_TOKEN_KEYS {
            assert!(
                offenders.contains(k),
                "{k:?} 在 ASCII_TOKEN_KEYS 里，但它的中文条目已经有中日韩字符了，请从白名单删掉"
            );
            // 记号之所以是记号，就是因为两种语言下**完全一致**。
            // 若某条 zh != en，说明这个键其实是散文，被错误地标成了记号。
            let e = TABLE
                .iter()
                .find(|e| e.key == *k)
                .expect("白名单键必在表里");
            assert_eq!(e.zh, e.en, "{k:?} 标成了技术记号，但中英不一致");
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

    /// 自由文本（句子，不是标签）的双语总闸。
    ///
    /// 与 [`tests::enum_labels_are_localized_both_ways`] 同一套思路：
    /// 从**真实的输入**出发，一路走到最终字符串，断言英文里没有中日韩字符。
    /// 只查字典表是不够的——`TargetSpec::summary_in` 忘了把 `lang` 传下去，
    /// 表还是干净的，界面照样露中文。
    #[test]
    fn free_text_is_localized_both_ways() {
        use crate::alerts::{
            match_rule_in, AlertContext, AlertEngine, AlertKind, AlertRule, Severity,
        };
        use crate::fingerprint::{infer_device, DeviceKind, PortScanConfig};
        use crate::monitor::{Change, ChangeKind};
        use crate::profile::ScanProfile;
        use crate::registry::BackendRegistry;
        use crate::result::{ProbeResult, ProbeStatus};
        use crate::spec::{parse_spec, ProbeMode};
        use crate::target::TargetRange;
        use crate::timeseries::TimeSeries;

        let mut samples: Vec<(&'static str, String, String)> = Vec::new();

        // —— 目标规格摘要 ——
        let one = parse_spec("192.168.1", TargetRange::Standard).expect("合法前缀");
        samples.push((
            "TargetSpec::summary",
            one.summary_in(Lang::Zh),
            one.summary_in(Lang::En),
        ));
        samples.push((
            "Segment::describe",
            one.segments[0].describe_in(Lang::Zh),
            one.segments[0].describe_in(Lang::En),
        ));
        let multi = parse_spec("10.0.0.1, 10.0.0.2", TargetRange::Standard).expect("多段");
        samples.push((
            "TargetSpec::summary(multi)",
            multi.summary_in(Lang::Zh),
            multi.summary_in(Lang::En),
        ));
        samples.push((
            "TargetSpec::summary(empty)",
            crate::spec::TargetSpec::default().summary_in(Lang::Zh),
            crate::spec::TargetSpec::default().summary_in(Lang::En),
        ));

        // —— 变化事件摘要 ——
        for kind in [ChangeKind::Appeared, ChangeKind::Disappeared] {
            let c = Change {
                round: 2,
                index: 0,
                ip: "10.0.0.7".parse().unwrap(),
                kind,
                from: ProbeStatus::Timeout,
                to: ProbeStatus::Online { rtt_ms: 3 },
            };
            samples.push((
                "Change::summary",
                c.summary_in(Lang::Zh),
                c.summary_in(Lang::En),
            ));
        }

        // —— 探测方式的状态名（TCP 一套独立的词）——
        let statuses = [
            ProbeStatus::Pending,
            ProbeStatus::Online { rtt_ms: 3 },
            ProbeStatus::Timeout,
            ProbeStatus::Unreachable,
            ProbeStatus::PermissionDenied,
            ProbeStatus::Unsupported,
            ProbeStatus::Error("boom".into()),
        ];
        for mode in [ProbeMode::Icmp, ProbeMode::Tcp { port: 445 }] {
            for st in &statuses {
                samples.push((
                    "ProbeMode::status_label",
                    mode.status_label_in(st, Lang::Zh),
                    mode.status_label_in(st, Lang::En),
                ));
            }
        }

        // —— 设备推断（名字与依据都是散文）——
        for k in DeviceKind::ALL {
            samples.push((
                "DeviceKind::label",
                k.label_in(Lang::Zh).to_string(),
                k.label_in(Lang::En).to_string(),
            ));
        }
        for g in [
            infer_device(Some("TP-Link Technologies"), &[80, 443]),
            infer_device(None, &[3389, 445, 135]),
            infer_device(None, &[9100, 631]),
        ] {
            let g = g.expect("这些组合必然有结论");
            samples.push((
                "DeviceGuess::summary",
                g.summary_in(Lang::Zh),
                g.summary_in(Lang::En),
            ));
        }

        // —— 后端能力（含 `note`）——
        let reg = BackendRegistry::new();
        for caps in reg.caps() {
            samples.push((
                "BackendCaps::describe",
                caps.describe_in(Lang::Zh),
                caps.describe_in(Lang::En),
            ));
            let (z, e) = (caps.note.label_in(Lang::Zh), caps.note.label_in(Lang::En));
            if !z.is_empty() {
                samples.push(("CapsNote::label", z.to_string(), e.to_string()));
            }
        }
        samples.push((
            "BackendRegistry::summary",
            reg.summary_in(Lang::Zh),
            reg.summary_in(Lang::En),
        ));

        // —— 模板摘要 ——
        //
        // 注意 `note` 用的是 ASCII：它是**用户自己写的数据**，不是界面文案，
        // 不该被翻译。让它保持 ASCII，这条测试才是在检查「我们写的词」
        // 有没有翻，而不是在检查「用户写的中文」——用户写什么语言是他的自由。
        let profile = ScanProfile {
            name: "prod".into(),
            spec: "10.0.0.0/24".into(),
            range: TargetRange::HostsOnly,
            timeout_ms: 800,
            concurrency: 64,
            mode: ProbeMode::Tcp { port: 443 },
            note: "production subnet".into(),
            extra: Vec::new(),
        };
        samples.push((
            "ScanProfile::describe",
            profile.describe_in(Lang::Zh),
            profile.describe_in(Lang::En),
        ));

        // —— 平台提示与通知通道说明 ——
        samples.push((
            "native_icmp_hint",
            crate::native_icmp_hint_in(Lang::Zh).to_string(),
            crate::native_icmp_hint_in(Lang::En).to_string(),
        ));
        let zh_notify = crate::notifications_available_in(Lang::Zh);
        let en_notify = crate::notifications_available_in(Lang::En);
        assert_eq!(zh_notify.len(), en_notify.len(), "通道条数不该跟语言变");
        for (z, e) in zh_notify.iter().zip(en_notify.iter()) {
            assert_eq!(z.0, e.0, "通道键是机器字段，不该跟语言变");
            assert_eq!(z.1, e.1, "通道可用性是事实，不该跟语言变");
            samples.push(("notifications_available", z.2.clone(), e.2.clone()));
        }

        // —— 解析错误（`Result<_, String>` 那一类）——
        for bad in ["80x", "443-445x", "0-10", "1-99999", "70000", ""] {
            let zh = PortScanConfig::parse_ports_in(bad, Lang::Zh).unwrap_err();
            let en = PortScanConfig::parse_ports_in(bad, Lang::En).unwrap_err();
            samples.push(("parse_ports", zh, en));
        }

        // —— 落盘格式的错误 ——
        for (what, text) in [
            ("TimeSeries::from_text(sig)", "WRONG 1\n"),
            ("TimeSeries::from_text(version)", "CPING-SERIES 99\n"),
            ("AlertEngine::from_text(version)", "CPING-ALERTS 99\n"),
        ] {
            let zh = if text.starts_with("CPING-SERIES") {
                TimeSeries::from_text_in(text, Lang::Zh).unwrap_err()
            } else if text.starts_with("CPING-ALERTS") {
                AlertEngine::from_text_in(text, Lang::Zh).unwrap_err()
            } else {
                TimeSeries::from_text_in(text, Lang::Zh).unwrap_err()
            };
            let en = if text.starts_with("CPING-SERIES") {
                TimeSeries::from_text_in(text, Lang::En).unwrap_err()
            } else if text.starts_with("CPING-ALERTS") {
                AlertEngine::from_text_in(text, Lang::En).unwrap_err()
            } else {
                TimeSeries::from_text_in(text, Lang::En).unwrap_err()
            };
            samples.push((what, zh, en));
        }

        // —— 告警正文（判定逻辑不变，只有措辞跟着语言走）——
        let rule = |kind: AlertKind| AlertRule {
            kind,
            enabled: true,
            threshold: 100,
            cooldown_secs: 300,
        };
        let mk_result = |st: ProbeStatus| ProbeResult {
            index: 3,
            ip: "10.0.0.9".parse().unwrap(),
            mac: None,
            vendor: None,
            hostname: None,
            status: st,
        };
        // 每条规则各自构造命中它的上下文——「命不命中」是规则的事，
        // 这里只想拿到它命中的那句话。
        let cases: Vec<(AlertKind, ProbeResult, Option<bool>, Option<f64>)> = vec![
            (
                AlertKind::Appeared,
                mk_result(ProbeStatus::Online { rtt_ms: 5 }),
                None, // 从没见过 → 首次上线
                Some(1.0),
            ),
            (
                AlertKind::Disappeared,
                mk_result(ProbeStatus::Timeout),
                Some(true),
                Some(0.5),
            ),
            (
                AlertKind::LatencyAbove,
                mk_result(ProbeStatus::Online { rtt_ms: 500 }),
                Some(true),
                Some(1.0),
            ),
            (
                AlertKind::UptimeBelow,
                mk_result(ProbeStatus::Online { rtt_ms: 5 }),
                Some(true),
                Some(0.5),
            ),
        ];
        for (kind, current, was_online, uptime_ratio) in &cases {
            let r = rule(*kind);
            let ctx = AlertContext {
                ip: current.ip,
                index: current.index,
                current,
                was_online: *was_online,
                uptime_ratio: *uptime_ratio,
                at_ms: 0,
                round: 2,
            };
            let zh = match_rule_in(&r, &ctx, Lang::Zh);
            let en = match_rule_in(&r, &ctx, Lang::En);
            assert_eq!(
                zh.is_some(),
                en.is_some(),
                "{kind:?} 的命中与否不该跟语言有关"
            );
            let (z, e) = (zh.expect("样本必须命中"), en.expect("样本必须命中"));
            assert_eq!(z.severity, e.severity, "严重级不该跟语言变");
            assert_eq!(z.value, e.value, "实测值不该跟语言变");
            samples.push(("AlertEvent::message", z.message, e.message));
        }
        // 顺带确认 `AlertEngine` 与 `Severity` 都在作用域里可用
        let _ = Severity::Info;
        let _ = AlertEngine::default();

        // —— 差异摘要 ——
        let mk = |idx: u16, ip: &str, st: ProbeStatus| ProbeResult {
            index: idx,
            ip: ip.parse().unwrap(),
            mac: None,
            vendor: None,
            hostname: None,
            status: st,
        };
        let base = vec![
            mk(0, "10.0.0.1", ProbeStatus::Online { rtt_ms: 3 }),
            mk(1, "10.0.0.2", ProbeStatus::Online { rtt_ms: 3 }),
        ];
        let next = vec![
            mk(0, "10.0.0.1", ProbeStatus::Online { rtt_ms: 900 }),
            mk(2, "10.0.0.3", ProbeStatus::Timeout),
        ];
        let diff = crate::diff::diff_reports(&base, &next, &crate::classify::Thresholds::default());
        samples.push((
            "ScanDiff::summary",
            diff.summary_in(Lang::Zh),
            diff.summary_in(Lang::En),
        ));

        assert!(
            samples.len() >= 45,
            "样本太少（{}）：新增的自由文本要补进这里",
            samples.len()
        );
        for (what, zh, en) in samples {
            assert!(
                has_cjk(&en).is_none(),
                "{what}：英文渲染里仍有中日韩字符，说明漏接了本地化：{en:?}"
            );
            if zh == en {
                // 中英一字不差只可能是技术记号（`TCP-OPEN`）。允许，
                // 但要求它**纯 ASCII**——否则就是把中文粘到了英文栏。
                assert!(
                    zh.is_ascii(),
                    "{what}：中英渲染完全相同且含非 ASCII 字符，疑似漏译：{zh:?}"
                );
            } else {
                assert!(
                    has_cjk(&zh).is_some(),
                    "{what}：中文渲染里没有中日韩字符，疑似漏传 Lang：{zh:?}"
                );
            }
        }
    }
}
