//! CLI 自己的中／英双语字典。
//!
//! # 为什么不复用核心层的 `Key` / `TABLE`
//!
//! 核心层的键空间只覆盖「核心逻辑要说的那 33 句话」（错误外框、保留网段原因、
//! 扫描策略、首次授权告知）。CLI 要说的话完全是另一套：帮助文本、`--xxx` 的
//! 取值校验、状态摘要、列标签、提示语。把这些塞进核心层会让核心去描述
//! 「某个命令行界面长什么样」——那不是它的职责。
//!
//! 所以这里**照抄核心层的机制**（枚举键 + 中英双列 + 穷尽 `ordinal` +
//! 一致性强测试），但键空间独立。`render_template` 是共用算法，直接调核心层。
//!
//! # 不本地化的东西
//!
//! - CSV / JSON 的列名与字段名：**数据契约**，中英文机器上必须一致，
//!   下游脚本按列名解析，翻了就崩。
//! - 快照 / 时序 / 审计的格式与机器字段（`session_start`、`round`、`exitCode` 等）。
//! - 命令行参数名本身（`--max-pps`、`--audit`）。
//! - 来自操作系统的原始错误详情（`No such file (os error 2)` 之类）——
//!   原样透传，只翻外面那层框架。
//! - 源代码注释：注释不是 UI，保持中文。
//!
//! # 关于核心层那些只有中文的摘要
//!
//! `TargetSpec::summary`、`Change::summary`、`AlertEvent::message`、
//! `BackendRegistry::summary_zh` 等由核心层产出、且**没有** `_in(lang)` 变体，
//! 本阶段不（也无法）在 CLI 里改。CLI 只保证**自己写的那部分**是中英双语的，
//! 这一点在交付报告里如实列出。

use cping_core::i18n::Lang;

/// CLI 面向用户的消息键。
///
/// 新增变体后必须同时做两件事，否则 [`tests::table_covers_every_key`] 会失败：
/// 在 [`ordinal`] 里登记序号，并在 [`TABLE`] 里给出中英条目。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    // —— 帮助文本 ——
    /// 无参数：整篇 `--help`。
    Help,

    // —— 参数解析错误（args.rs）——
    /// 参数：`参数名`
    ErrMissingValue,
    /// 参数：`参数的 Debug 形式`
    ErrUnknownFlag,
    /// 参数：`用户的原始取值`
    ErrInvalidLang,
    /// 参数：`底层原因`
    ErrFpPorts,
    /// 参数：`上限毫秒`
    ErrFpTimeoutRange,
    /// 参数：`路径`、`底层原因`
    ErrTargetFileRead,
    /// 参数：`用户的原始取值`
    ErrRangeInvalid,
    /// 参数：`用户的原始取值`
    ErrModeInvalid,
    ErrPortZero,
    /// 参数：`上限毫秒`
    ErrTimeoutRange,
    /// 参数：`上限`
    ErrConcurrencyRange,
    ErrWatchZero,
    ErrRoundsZero,
    ErrReportFormat,
    ErrKeepDaysRange,
    /// 参数：`用户的原始取值`
    ErrAlertKindsUnknown,
    ErrAlertKindsEmpty,
    ErrAlertUptimeRange,
    /// 参数：`用户的原始取值`
    ErrNotifyUnknown,
    /// 参数：`路径`
    ErrOutExtension,
    ErrListProfilesNeedsConfig,
    ErrProfileNeedsConfig,
    /// 参数：`配置路径`、`模板名`
    ErrNoSuchProfile,
    ErrMissingTarget,
    ErrKeepDaysNeedsHistory,
    ErrNotifyWebhookNeedsUrl,
    ErrNotifySmtpNeedsHost,
    ErrNotifyCommandNeedsCmd,
    ErrWebhookNotSelected,
    ErrSmtpNotSelected,
    ErrCommandNotSelected,
    ErrSmtpNeedsMailTo,
    ErrAlertStateNeedsAlert,
    /// 参数：`参数名`、`用户的原始取值`
    ErrExpectInteger,
    /// 参数：`参数名`
    ErrOutOfRange16,
    /// 参数：`参数名`、`上限`
    ErrOutOfRange32,

    // —— 配置合并说明（args.rs 的 config_notes）——
    /// 参数：`配置路径`、`模板名`、`目标规格`
    NoteConfigUseProfile,
    /// 参数：`配置路径`
    NoteConfigNoProfile,
    /// 参数：`模板名`、`模板里的目标规格`
    NoteCliTargetOverridesProfile,
    /// 参数：`模板名`、`目标规格`
    NoteTargetFromProfile,
    /// 参数：`字段列表`
    NoteProbeFromProfile,
    /// 参数：`字段列表`
    NoteProbeFromApp,
    /// 参数：`参数列表`
    NoteCliOverridesConfig,
    NoteConfigHeader,

    // —— Args::describe() 状态摘要 ——
    /// 参数：`秒数`
    DescWatch,
    /// 参数：`端口数`
    DescFpCommonPorts,
    /// 参数：`端口数`
    DescFpListedPorts,
    /// 参数：`端口描述`、`横幅后缀`
    DescFp,
    DescFpBanner,
    /// 参数：`天数`
    DescHistory,
    DescReport,
    /// 参数：`规则列表`、`静音后缀`、`通知通道`
    DescAlerts,
    DescAlertMuted,
    /// 参数：`配置路径`、`模板名`
    DescConfigWithProfile,
    /// 参数：`配置路径`
    DescConfig,
    /// 参数：`探测方式`、`并发数`、`超时毫秒`
    DescSummary,
    AlertKindAll,
    AlertKindAppeared,
    AlertKindDisappeared,
    AlertKindLatency,
    AlertKindUptime,

    // —— main.rs：错误前缀与外框 ——
    /// 参数：`细节`
    ErrUsagePrefix,
    /// 参数：`细节`
    ErrDumpConfigPrefix,
    /// 参数：`细节`
    ErrTargetSpecPrefix,
    HintAllowReserved,
    /// 参数：`版本`、`操作系统`、`后端名`
    VersionLine,
    /// 参数：`状态文件路径`
    HintConsentUnwritable,
    HomeDirectory,
    /// 参数：`秒数`
    HintWatchForever,
    /// 参数：`目标数`
    WarnManyTargetsWatch,
    /// 参数：`路径`、`行数`
    WarnHistoryUnparsed,
    /// 参数：`点数`、`轮数`、`路径`
    InfoHistoryLoaded,
    /// 参数：`细节`
    ErrHistoryUnusable,
    /// 参数：`路径`、`行数`
    WarnAlertStateUnparsed,
    /// 参数：`主机数`、`路径`
    InfoAlertStateLoaded,
    /// 参数：`细节`
    ErrAlertStateUnusable,
    HintAlertsMuted,
    HintSingleRoundBaseline,
    /// 参数：`细节`
    ErrPolicyInvalid,
    /// 参数：`细节`
    ErrAuditUnavailable,
    /// 参数：`条数`、`路径`
    InfoAuditCount,
    /// 参数：`细节`
    ErrAuditWrite,
    /// 参数：`路径`
    InfoWrote,
    /// 参数：`细节`
    ErrExportFailed,
    /// 参数：`细节`
    ErrHistoryWrite,
    /// 参数：`点数`、`轮数`、`天数`、`路径`
    InfoHistoryLine,
    /// 参数：`路径`
    WarnReportNoData,
    /// 参数：`细节`
    ErrReportWrite,
    /// 参数：`主机数`、`路径`
    InfoUptimeReport,
    /// 参数：`目标数`、`平台提示`
    ErrBackendUnavailable,
    /// 参数：`细节`
    ErrScanStart,
    /// 参数：`MAC 条数`、`厂商条数`、`邻居表条数`
    InfoEnrich,
    InfoFpNoOnline,
    /// 参数：`主机数`、`端口数`、`开放端口数`
    InfoFpSummary,
    InfoAlertsNone,
    /// 参数：`条数`
    InfoAlertsCount,
    /// 参数：`冷却抑制数`、`静音抑制数`
    InfoAlertsSuppressed,
    /// 参数：`路径`、`主机数`
    InfoAlertStateSaved,
    /// 参数：`细节`
    ErrAlertStateWrite,
    /// 参数：`丢弃批数`
    WarnNotifyQueueFull,
    /// 参数：`已投递批数`
    InfoNotifyNoChannels,
    /// 参数：`已投递批数`
    InfoNotifyResults,
    /// 参数：`标记`、`通道名`、`细节`
    InfoNotifyLine,
    /// 参数：`通道名`、`细节`
    ErrNotifyChannelFailed,
    /// 参数：`失败通道数`
    HintNotifyUndelivered,
    /// 参数：`路径`、`细节`
    ErrWriteFile,

    // —— --notify-list ——
    /// 参数：`版本`、`操作系统`
    NotifyHeader,
    ColChannel,
    ColAvailable,
    ColNote,
    BoolYes,
    BoolNo,
    NotifyHintSilentFail,
    NotifyHintHttps,

    // —— --list-profiles ——
    /// 参数：`版本`、`操作系统`、`配置路径`
    ProfilesHeader,
    ProfilesEmpty,
    ProfilesEmptyAdd,
    ColProfile,
    ColTargetSpec,
    /// 参数：`配置路径`
    ProfilesUsage,
    ProfilesNoDefault,
    /// 参数：`模板名`
    ProfilesUseDefault,
    /// 参数：`键数`
    ProfilesUnknownKeys,

    // —— --dump-config ——
    /// 参数：`路径`
    DumpProbeOnly,
    DumpProbeOnlyHint,
    /// 参数：`路径`
    DumpWritten,
    /// 参数：`模板名`、`目标规格`、`目标数描述`
    DumpProfileLine,
    /// 参数：`目标数`
    DumpCountTargets,
    DumpCountUnparsable,
    /// 参数：`路径`
    DumpDefaultSet,
    /// 参数：`模板名`、`旧目标规格`
    DumpOverwrote,
    DumpNotWritten,

    // —— 每轮输出 ——
    /// 参数：`轮次`、`探测方式`、`目标规格`、`用时秒`、`取消后缀`
    RoundHeader,
    RoundCancelled,
    /// 参数：`总数`、`在线`、`超时`、`不可达`、`权限不足`、`不支持`、`错误`
    RoundStats,
    /// 参数：`变化条数`
    RoundChanges,
    /// 参数：`隐藏条数`
    RoundChangesMore,
    RoundOnlineHosts,
    RoundFpHeader,
    /// 参数：`IP`
    RoundFpNoPorts,

    // —— --list-backends ——
    ColName,
    ColFamily,
    FamilyV4Only,
    FamilyV6Only,
    BackendsNeedsPrivilege,
    BackendsHint,

    /// 参数：`配置路径`、`核心层给出的原因`
    ///
    /// 追加在**末尾**（而不是插到 `ErrProfileNeedsConfig` 后面）：序号是稳定标识，
    /// 插入会让后面所有键的序号平移。新增键一律追加，避免无意义的重新编号。
    ErrConfigPrefix,
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
/// 编译就会失败。登记了却忘了加进 [`TABLE`]，[`tests::table_covers_every_key`]
/// 会因为序号不连续或重复而失败。两道关卡合起来保证「字典不会漏键」。
pub const fn ordinal(key: Key) -> usize {
    match key {
        Key::Help => 0,
        Key::ErrMissingValue => 1,
        Key::ErrUnknownFlag => 2,
        Key::ErrInvalidLang => 3,
        Key::ErrFpPorts => 4,
        Key::ErrFpTimeoutRange => 5,
        Key::ErrTargetFileRead => 6,
        Key::ErrRangeInvalid => 7,
        Key::ErrModeInvalid => 8,
        Key::ErrPortZero => 9,
        Key::ErrTimeoutRange => 10,
        Key::ErrConcurrencyRange => 11,
        Key::ErrWatchZero => 12,
        Key::ErrRoundsZero => 13,
        Key::ErrReportFormat => 14,
        Key::ErrKeepDaysRange => 15,
        Key::ErrAlertKindsUnknown => 16,
        Key::ErrAlertKindsEmpty => 17,
        Key::ErrAlertUptimeRange => 18,
        Key::ErrNotifyUnknown => 19,
        Key::ErrOutExtension => 20,
        Key::ErrListProfilesNeedsConfig => 21,
        Key::ErrProfileNeedsConfig => 22,
        Key::ErrNoSuchProfile => 23,
        Key::ErrMissingTarget => 24,
        Key::ErrKeepDaysNeedsHistory => 25,
        Key::ErrNotifyWebhookNeedsUrl => 26,
        Key::ErrNotifySmtpNeedsHost => 27,
        Key::ErrNotifyCommandNeedsCmd => 28,
        Key::ErrWebhookNotSelected => 29,
        Key::ErrSmtpNotSelected => 30,
        Key::ErrCommandNotSelected => 31,
        Key::ErrSmtpNeedsMailTo => 32,
        Key::ErrAlertStateNeedsAlert => 33,
        Key::ErrExpectInteger => 34,
        Key::ErrOutOfRange16 => 35,
        Key::ErrOutOfRange32 => 36,
        Key::NoteConfigUseProfile => 37,
        Key::NoteConfigNoProfile => 38,
        Key::NoteCliTargetOverridesProfile => 39,
        Key::NoteTargetFromProfile => 40,
        Key::NoteProbeFromProfile => 41,
        Key::NoteProbeFromApp => 42,
        Key::NoteCliOverridesConfig => 43,
        Key::NoteConfigHeader => 44,
        Key::DescWatch => 45,
        Key::DescFpCommonPorts => 46,
        Key::DescFpListedPorts => 47,
        Key::DescFp => 48,
        Key::DescFpBanner => 49,
        Key::DescHistory => 50,
        Key::DescReport => 51,
        Key::DescAlerts => 52,
        Key::DescAlertMuted => 53,
        Key::DescConfigWithProfile => 54,
        Key::DescConfig => 55,
        Key::DescSummary => 56,
        Key::AlertKindAll => 57,
        Key::AlertKindAppeared => 58,
        Key::AlertKindDisappeared => 59,
        Key::AlertKindLatency => 60,
        Key::AlertKindUptime => 61,
        Key::ErrUsagePrefix => 62,
        Key::ErrDumpConfigPrefix => 63,
        Key::ErrTargetSpecPrefix => 64,
        Key::HintAllowReserved => 65,
        Key::VersionLine => 66,
        Key::HintConsentUnwritable => 67,
        Key::HomeDirectory => 68,
        Key::HintWatchForever => 69,
        Key::WarnManyTargetsWatch => 70,
        Key::WarnHistoryUnparsed => 71,
        Key::InfoHistoryLoaded => 72,
        Key::ErrHistoryUnusable => 73,
        Key::WarnAlertStateUnparsed => 74,
        Key::InfoAlertStateLoaded => 75,
        Key::ErrAlertStateUnusable => 76,
        Key::HintAlertsMuted => 77,
        Key::HintSingleRoundBaseline => 78,
        Key::ErrPolicyInvalid => 79,
        Key::ErrAuditUnavailable => 80,
        Key::InfoAuditCount => 81,
        Key::ErrAuditWrite => 82,
        Key::InfoWrote => 83,
        Key::ErrExportFailed => 84,
        Key::ErrHistoryWrite => 85,
        Key::InfoHistoryLine => 86,
        Key::WarnReportNoData => 87,
        Key::ErrReportWrite => 88,
        Key::InfoUptimeReport => 89,
        Key::ErrBackendUnavailable => 90,
        Key::ErrScanStart => 91,
        Key::InfoEnrich => 92,
        Key::InfoFpNoOnline => 93,
        Key::InfoFpSummary => 94,
        Key::InfoAlertsNone => 95,
        Key::InfoAlertsCount => 96,
        Key::InfoAlertsSuppressed => 97,
        Key::InfoAlertStateSaved => 98,
        Key::ErrAlertStateWrite => 99,
        Key::WarnNotifyQueueFull => 100,
        Key::InfoNotifyNoChannels => 101,
        Key::InfoNotifyResults => 102,
        Key::InfoNotifyLine => 103,
        Key::ErrNotifyChannelFailed => 104,
        Key::HintNotifyUndelivered => 105,
        Key::ErrWriteFile => 106,
        Key::NotifyHeader => 107,
        Key::ColChannel => 108,
        Key::ColAvailable => 109,
        Key::ColNote => 110,
        Key::BoolYes => 111,
        Key::BoolNo => 112,
        Key::NotifyHintSilentFail => 113,
        Key::NotifyHintHttps => 114,
        Key::ProfilesHeader => 115,
        Key::ProfilesEmpty => 116,
        Key::ProfilesEmptyAdd => 117,
        Key::ColProfile => 118,
        Key::ColTargetSpec => 119,
        Key::ProfilesUsage => 120,
        Key::ProfilesNoDefault => 121,
        Key::ProfilesUseDefault => 122,
        Key::ProfilesUnknownKeys => 123,
        Key::DumpProbeOnly => 124,
        Key::DumpProbeOnlyHint => 125,
        Key::DumpWritten => 126,
        Key::DumpProfileLine => 127,
        Key::DumpCountTargets => 128,
        Key::DumpCountUnparsable => 129,
        Key::DumpDefaultSet => 130,
        Key::DumpOverwrote => 131,
        Key::DumpNotWritten => 132,
        Key::RoundHeader => 133,
        Key::RoundCancelled => 134,
        Key::RoundStats => 135,
        Key::RoundChanges => 136,
        Key::RoundChangesMore => 137,
        Key::RoundOnlineHosts => 138,
        Key::RoundFpHeader => 139,
        Key::RoundFpNoPorts => 140,
        Key::ColName => 141,
        Key::ColFamily => 142,
        Key::FamilyV4Only => 143,
        Key::FamilyV6Only => 144,
        Key::BackendsNeedsPrivilege => 145,
        Key::BackendsHint => 146,
        Key::ErrConfigPrefix => 147,
    }
}

/// 字典里的键总数。
pub const KEY_COUNT: usize = 148;

/// 全部条目。
///
/// 条目里的 `{}` 是顺序占位符，个数必须中英一致（有测试守着）。不用具名占位符
/// 是为了让两种语言的语序可以完全不同——英文与中文的参数顺序恰好相同只是巧合。
///
/// 纯 ASCII 的模板（例如 `CPing {} · {} · {}`）**刻意不进字典**：它们没有可翻的内容，
/// 放进来的话中英两列会一模一样，而「中文条目必须含中日韩字符」那条测试会因此报错。
pub static TABLE: &[Entry] = &[
    Entry {
        key: Key::Help,
        zh: include_str!("help_zh.txt"),
        en: include_str!("help_en.txt"),
    },
    // —— 参数解析错误 ——
    Entry {
        key: Key::ErrMissingValue,
        zh: "{} 缺少取值",
        en: "missing value for {}",
    },
    Entry {
        key: Key::ErrUnknownFlag,
        zh: "未知参数 {}（用 --help 查看用法）",
        en: "unknown argument {} (use --help for usage)",
    },
    Entry {
        key: Key::ErrInvalidLang,
        zh: "--lang 取值 {} 无效，只能是 zh / en",
        en: "--lang value {} is invalid; expected zh or en",
    },
    Entry {
        key: Key::ErrFpPorts,
        zh: "--fp-ports 无效：{}",
        en: "--fp-ports is invalid: {}",
    },
    Entry {
        key: Key::ErrFpTimeoutRange,
        zh: "--fp-timeout 必须在 1..={} 之间",
        en: "--fp-timeout must be within 1..={}",
    },
    Entry {
        key: Key::ErrTargetFileRead,
        zh: "读取目标文件 {} 失败：{}",
        en: "failed to read target file {}: {}",
    },
    Entry {
        key: Key::ErrRangeInvalid,
        zh: "--range {} 无效，只能是 standard / full / hosts_only",
        en: "--range {} is invalid; expected standard / full / hosts_only",
    },
    Entry {
        key: Key::ErrModeInvalid,
        zh: "--mode {} 无效，只能是 icmp / tcp",
        en: "--mode {} is invalid; expected icmp / tcp",
    },
    Entry {
        key: Key::ErrPortZero,
        zh: "--port 不能为 0",
        en: "--port must not be 0",
    },
    Entry {
        key: Key::ErrTimeoutRange,
        zh: "--timeout 必须在 1..={} 之间",
        en: "--timeout must be within 1..={}",
    },
    Entry {
        key: Key::ErrConcurrencyRange,
        zh: "--concurrency 必须在 1..={} 之间",
        en: "--concurrency must be within 1..={}",
    },
    Entry {
        key: Key::ErrWatchZero,
        zh: "--watch 间隔必须大于 0 秒",
        en: "--watch interval must be greater than 0 seconds",
    },
    Entry {
        key: Key::ErrRoundsZero,
        zh: "--rounds 必须大于 0",
        en: "--rounds must be greater than 0",
    },
    Entry {
        key: Key::ErrReportFormat,
        zh: "--report 只支持 .csv 或 .html",
        en: "--report only supports .csv or .html",
    },
    Entry {
        key: Key::ErrKeepDaysRange,
        zh: "--keep-days 必须在 1..=3650 之间",
        en: "--keep-days must be within 1..=3650",
    },
    Entry {
        key: Key::ErrAlertKindsUnknown,
        zh: "--alert-kinds 不认识 {}（appeared/disappeared/latency/uptime）",
        en: "--alert-kinds does not recognise {} (appeared/disappeared/latency/uptime)",
    },
    Entry {
        key: Key::ErrAlertKindsEmpty,
        zh: "--alert-kinds 至少要给一类",
        en: "--alert-kinds needs at least one kind",
    },
    Entry {
        key: Key::ErrAlertUptimeRange,
        zh: "--alert-uptime 是百分数，必须在 0..=100 之间",
        en: "--alert-uptime is a percentage and must be within 0..=100",
    },
    Entry {
        key: Key::ErrNotifyUnknown,
        zh: "--notify 不认识通道 {}（console/system/webhook/command/smtp）",
        en: "--notify does not recognise channel {} (console/system/webhook/command/smtp)",
    },
    Entry {
        key: Key::ErrOutExtension,
        zh: "--out {} 的扩展名无法识别，请用 .csv / .txt / .json / .html",
        en: "--out {} has an unrecognised extension; use .csv / .txt / .json / .html",
    },
    Entry {
        key: Key::ErrListProfilesNeedsConfig,
        zh: "--list-profiles 需要同时给 --config",
        en: "--list-profiles requires --config",
    },
    Entry {
        key: Key::ErrProfileNeedsConfig,
        zh: "--profile 需要同时给 --config（模板来自配置文件）",
        en: "--profile requires --config (profiles come from the config file)",
    },
    Entry {
        key: Key::ErrNoSuchProfile,
        zh: "配置 {} 里没有模板 {}",
        en: "config {} has no profile {}",
    },
    Entry {
        key: Key::ErrMissingTarget,
        zh: "缺少目标：请用 --target 指定（例如 --target 192.168.1），或用 --config + --profile 选一份带目标的模板",
        en: "no target: pass one with --target (for example --target 192.168.1), or pick a profile that carries a target via --config + --profile",
    },
    Entry {
        key: Key::ErrKeepDaysNeedsHistory,
        zh: "--keep-days 仅在同时指定 --history 时有意义",
        en: "--keep-days is only meaningful together with --history",
    },
    Entry {
        key: Key::ErrNotifyWebhookNeedsUrl,
        zh: "--notify webhook 需要同时给 --notify-webhook <http://URL>",
        en: "--notify webhook requires --notify-webhook <http://URL>",
    },
    Entry {
        key: Key::ErrNotifySmtpNeedsHost,
        zh: "--notify smtp 需要同时给 --notify-smtp <host[:port]>",
        en: "--notify smtp requires --notify-smtp <host[:port]>",
    },
    Entry {
        key: Key::ErrNotifyCommandNeedsCmd,
        zh: "--notify command 需要同时给 --notify-cmd <模板>",
        en: "--notify command requires --notify-cmd <template>",
    },
    Entry {
        key: Key::ErrWebhookNotSelected,
        zh: "给了 --notify-webhook 却没在 --notify 里选上 webhook",
        en: "--notify-webhook was given but webhook is not selected in --notify",
    },
    Entry {
        key: Key::ErrSmtpNotSelected,
        zh: "给了 --notify-smtp 却没在 --notify 里选上 smtp",
        en: "--notify-smtp was given but smtp is not selected in --notify",
    },
    Entry {
        key: Key::ErrCommandNotSelected,
        zh: "给了 --notify-cmd 却没在 --notify 里选上 command",
        en: "--notify-cmd was given but command is not selected in --notify",
    },
    Entry {
        key: Key::ErrSmtpNeedsMailTo,
        zh: "--notify-smtp 需要同时给 --notify-mail-to（至少一个收件人）",
        en: "--notify-smtp requires --notify-mail-to (at least one recipient)",
    },
    Entry {
        key: Key::ErrAlertStateNeedsAlert,
        zh: "--alert-state 只在开启告警（--alert 或任一 --alert-*）时有意义",
        en: "--alert-state is only meaningful when alerting is on (--alert or any --alert-*)",
    },
    Entry {
        key: Key::ErrExpectInteger,
        zh: "{} 需要一个非负整数，得到 {}",
        en: "{} expects a non-negative integer, got {}",
    },
    Entry {
        key: Key::ErrOutOfRange16,
        zh: "{} 超出 0-65535",
        en: "{} is out of the 0-65535 range",
    },
    Entry {
        key: Key::ErrOutOfRange32,
        zh: "{} 超出 0-{}",
        en: "{} is out of the 0-{} range",
    },
    // —— 配置合并说明 ——
    Entry {
        key: Key::NoteConfigUseProfile,
        zh: "配置 {}：使用模板 {}（{}）",
        en: "config {}: using profile {} ({})",
    },
    Entry {
        key: Key::NoteConfigNoProfile,
        zh: "配置 {}：没有选中模板，只取 [probe] 的探测参数",
        en: "config {}: no profile selected; only [probe] parameters are used",
    },
    Entry {
        key: Key::NoteCliTargetOverridesProfile,
        zh: "命令行 --target 盖掉了模板 {} 的目标规格（模板里是 {}）",
        en: "command line --target overrides profile {}'s target spec (the profile has {})",
    },
    Entry {
        key: Key::NoteTargetFromProfile,
        zh: "目标规格取自模板 {}：{}",
        en: "target spec taken from profile {}: {}",
    },
    Entry {
        key: Key::NoteProbeFromProfile,
        zh: "模板提供：{}",
        en: "provided by the profile: {}",
    },
    Entry {
        key: Key::NoteProbeFromApp,
        zh: "配置 [probe] 提供：{}",
        en: "provided by config [probe]: {}",
    },
    Entry {
        key: Key::NoteCliOverridesConfig,
        zh: "命令行 {} 盖掉了配置里的对应值",
        en: "command line {} overrides the corresponding config value",
    },
    Entry {
        key: Key::NoteConfigHeader,
        zh: "配置来源：",
        en: "configuration sources:",
    },
    // —— 状态摘要 ——
    Entry {
        key: Key::DescWatch,
        zh: " · 每 {} 秒重扫",
        en: " · rescan every {} s",
    },
    Entry {
        key: Key::DescFpCommonPorts,
        zh: "{} 个常用端口",
        en: "{} common ports",
    },
    Entry {
        key: Key::DescFpListedPorts,
        zh: "{} 个指定端口",
        en: "{} listed ports",
    },
    Entry {
        key: Key::DescFp,
        zh: " · 端口指纹（{}{}）",
        en: " · port fingerprint ({}{})",
    },
    Entry {
        key: Key::DescFpBanner,
        zh: "，含横幅",
        en: ", with banners",
    },
    Entry {
        key: Key::DescHistory,
        zh: " · 长期时序（保留 {} 天）",
        en: " · long-term series ({} days kept)",
    },
    Entry {
        key: Key::DescReport,
        zh: " · 结束时出可用率报表",
        en: " · uptime report at the end",
    },
    Entry {
        key: Key::DescAlerts,
        zh: " · 告警（{}）{} · 通知 {}",
        en: " · alerts ({}){} · notify {}",
    },
    Entry {
        key: Key::DescAlertMuted,
        zh: "［已静音］",
        en: " [muted]",
    },
    Entry {
        key: Key::DescConfigWithProfile,
        zh: " · 配置 {}［模板 {}］",
        en: " · config {} [profile {}]",
    },
    Entry {
        key: Key::DescConfig,
        zh: " · 配置 {}",
        en: " · config {}",
    },
    Entry {
        key: Key::DescSummary,
        zh: "{} · {} 并发 · 超时 {} ms",
        en: "{} · {} concurrent · timeout {} ms",
    },
    Entry {
        key: Key::AlertKindAll,
        zh: "全部",
        en: "all",
    },
    Entry {
        key: Key::AlertKindAppeared,
        zh: "新增主机",
        en: "appeared",
    },
    Entry {
        key: Key::AlertKindDisappeared,
        zh: "主机离线",
        en: "offline",
    },
    Entry {
        key: Key::AlertKindLatency,
        zh: "延迟超阈",
        en: "latency above",
    },
    Entry {
        key: Key::AlertKindUptime,
        zh: "可用率偏低",
        en: "uptime below",
    },
    // —— main.rs ——
    Entry {
        key: Key::ErrUsagePrefix,
        zh: "参数错误：{}",
        en: "argument error: {}",
    },
    Entry {
        key: Key::ErrDumpConfigPrefix,
        zh: "配置写出失败：{}",
        en: "failed to write config: {}",
    },
    Entry {
        key: Key::ErrTargetSpecPrefix,
        zh: "目标规格错误：{}",
        en: "target spec error: {}",
    },
    Entry {
        key: Key::HintAllowReserved,
        zh: "提示：确认要扫就加 --allow-reserved；这类地址扫了要么没意义、要么会打扰别人。",
        en: "hint: add --allow-reserved if you really mean to scan this; such addresses are either meaningless or disturb others.",
    },
    Entry {
        key: Key::VersionLine,
        zh: "cping-cli {} · {} · 后端 {}",
        en: "cping-cli {} · {} · backend {}",
    },
    Entry {
        key: Key::HintConsentUnwritable,
        zh: "提示：无法记录授权确认状态（{} 不可写），下次启动会再提示一遍",
        en: "hint: cannot record the consent acknowledgement ({} is not writable); you will be prompted again next launch",
    },
    Entry {
        key: Key::HomeDirectory,
        zh: "家目录",
        en: "home directory",
    },
    Entry {
        key: Key::HintWatchForever,
        zh: "提示：持续监控模式不会自动停止（每 {} 秒一轮），按 Ctrl+C 结束",
        en: "hint: continuous monitoring never stops on its own (one round every {} s); press Ctrl+C to end",
    },
    Entry {
        key: Key::WarnManyTargetsWatch,
        zh: "警告：{} 个目标进入持续监控，每轮都会重新扫描，请确认网络负载可接受",
        en: "warning: {} targets are under continuous monitoring and will be rescanned every round; make sure the network load is acceptable",
    },
    Entry {
        key: Key::WarnHistoryUnparsed,
        zh: "警告：历史文件 {} 有 {} 行无法解析，已跳过（其余数据继续使用）",
        en: "warning: history file {} has {} unparsable line(s); they were skipped (the rest is still used)",
    },
    Entry {
        key: Key::InfoHistoryLoaded,
        zh: "已载入历史 {} 点 · {} 轮（{}）",
        en: "loaded history: {} points · {} rounds ({})",
    },
    Entry {
        key: Key::ErrHistoryUnusable,
        zh: "历史文件无法使用：{}",
        en: "history file unusable: {}",
    },
    Entry {
        key: Key::WarnAlertStateUnparsed,
        zh: "警告：告警状态 {} 有 {} 行无法解析，已跳过",
        en: "warning: alert state {} has {} unparsable line(s); they were skipped",
    },
    Entry {
        key: Key::InfoAlertStateLoaded,
        zh: "已载入告警状态：{} 台主机的冷却记录（{}）",
        en: "loaded alert state: cooldown records for {} host(s) ({})",
    },
    Entry {
        key: Key::ErrAlertStateUnusable,
        zh: "告警状态文件无法使用：{}",
        en: "alert state file unusable: {}",
    },
    Entry {
        key: Key::HintAlertsMuted,
        zh: "提示：告警已静音（--mute-alerts）：规则照常求值，但不会通知",
        en: "hint: alerts are muted (--mute-alerts); rules are still evaluated, but no notification is sent",
    },
    Entry {
        key: Key::HintSingleRoundBaseline,
        zh: "提示：只跑一轮时，「新增主机」规则只建立基线（不发事件）。要观察上线下线请加 --watch",
        en: "hint: with a single round the \"appeared\" rule only establishes a baseline (no events); add --watch to observe up/down transitions",
    },
    Entry {
        key: Key::ErrPolicyInvalid,
        zh: "安全策略无效：{}",
        en: "invalid security policy: {}",
    },
    Entry {
        key: Key::ErrAuditUnavailable,
        zh: "审计日志不可用：{}",
        en: "audit log unavailable: {}",
    },
    Entry {
        key: Key::InfoAuditCount,
        zh: "  审计：{} 条记录 → {}",
        en: "  audit: {} record(s) → {}",
    },
    Entry {
        key: Key::ErrAuditWrite,
        zh: "审计写入失败：{}",
        en: "failed to write audit log: {}",
    },
    Entry {
        key: Key::InfoWrote,
        zh: "  已写出 {}",
        en: "  wrote {}",
    },
    Entry {
        key: Key::ErrExportFailed,
        zh: "导出失败：{}",
        en: "export failed: {}",
    },
    Entry {
        key: Key::ErrHistoryWrite,
        zh: "历史写入失败：{}",
        en: "failed to write history: {}",
    },
    Entry {
        key: Key::InfoHistoryLine,
        zh: "  历史：{} 点 · {} 轮 · 保留 {} 天 → {}",
        en: "  history: {} points · {} rounds · {} days kept → {}",
    },
    Entry {
        key: Key::WarnReportNoData,
        zh: "报表：没有可用的历史数据，已跳过 {}",
        en: "report: no usable history data; skipped {}",
    },
    Entry {
        key: Key::ErrReportWrite,
        zh: "报表写入失败：{}",
        en: "failed to write report: {}",
    },
    Entry {
        key: Key::InfoUptimeReport,
        zh: "可用率报表（{} 台主机）→ {}",
        en: "uptime report ({} hosts) → {}",
    },
    Entry {
        key: Key::ErrBackendUnavailable,
        zh: "后端不可用：{} 个目标全部「权限不足/不支持」。{}",
        en: "backend unavailable: all {} targets were \"permission denied/unsupported\". {}",
    },
    Entry {
        key: Key::ErrScanStart,
        zh: "无法启动扫描：{}",
        en: "cannot start scan: {}",
    },
    Entry {
        key: Key::InfoEnrich,
        zh: "  补全：MAC {} 条 · 厂商 {} 条（邻居表 {} 条）",
        en: "  enrichment: {} MAC · {} vendor ({} neighbour entries)",
    },
    Entry {
        key: Key::InfoFpNoOnline,
        zh: "  端口指纹：没有在线主机，跳过",
        en: "  port fingerprint: no online host, skipped",
    },
    Entry {
        key: Key::InfoFpSummary,
        zh: "  端口指纹：{} 台在线主机 × {} 个端口 · 发现 {} 个开放端口",
        en: "  port fingerprint: {} online host(s) × {} port(s) · {} open port(s) found",
    },
    Entry {
        key: Key::InfoAlertsNone,
        zh: "  告警：无",
        en: "  alerts: none",
    },
    Entry {
        key: Key::InfoAlertsCount,
        zh: "  告警 {} 条：",
        en: "  {} alert(s):",
    },
    Entry {
        key: Key::InfoAlertsSuppressed,
        zh: "     （已按冷却/静音抑制：冷却 {} 条 · 静音 {} 条）",
        en: "     (suppressed by cooldown/mute: {} by cooldown · {} by mute)",
    },
    Entry {
        key: Key::InfoAlertStateSaved,
        zh: "告警状态 → {}（{} 台主机的冷却记录）",
        en: "alert state → {} (cooldown records for {} hosts)",
    },
    Entry {
        key: Key::ErrAlertStateWrite,
        zh: "告警状态写入失败：{}",
        en: "failed to write alert state: {}",
    },
    Entry {
        key: Key::WarnNotifyQueueFull,
        zh: "警告：通知队列满，有 {} 批未投递（投递速度跟不上告警速度）",
        en: "warning: notification queue full; {} batch(es) were dropped (delivery cannot keep up with the alerts)",
    },
    Entry {
        key: Key::InfoNotifyNoChannels,
        zh: "通知：{} 批已投递（未配置任何通道）",
        en: "notify: {} batch(es) posted (no channel configured)",
    },
    Entry {
        key: Key::InfoNotifyResults,
        zh: "通知：{} 批 · 通道结果：",
        en: "notify: {} batch(es) · channel results:",
    },
    Entry {
        key: Key::InfoNotifyLine,
        zh: "  {} {}：{}",
        en: "  {} {}: {}",
    },
    Entry {
        key: Key::ErrNotifyChannelFailed,
        zh: "通知通道 {} 失败：{}",
        en: "notification channel {} failed: {}",
    },
    Entry {
        key: Key::HintNotifyUndelivered,
        zh: "提示：有 {} 个通知通道未能送达（告警本身已记录，见 --notify-list 查能力）",
        en: "hint: {} notification channel(s) could not deliver (the alert itself is recorded; see --notify-list for capabilities)",
    },
    Entry {
        key: Key::ErrWriteFile,
        zh: "写入 {} 失败：{}",
        en: "failed to write {}: {}",
    },
    // —— --notify-list ——
    Entry {
        key: Key::NotifyHeader,
        zh: "cping-cli {} · {} · 通知通道",
        en: "cping-cli {} · {} · notification channels",
    },
    Entry {
        key: Key::ColChannel,
        zh: "通道",
        en: "channel",
    },
    Entry {
        key: Key::ColAvailable,
        zh: "可用",
        en: "available",
    },
    Entry {
        key: Key::ColNote,
        zh: "说明",
        en: "notes",
    },
    Entry {
        key: Key::BoolYes,
        zh: "是",
        en: "yes",
    },
    Entry {
        key: Key::BoolNo,
        zh: "否",
        en: "no",
    },
    Entry {
        key: Key::NotifyHintSilentFail,
        zh: "提示：通道失败一定会被打印出来——告警系统静默失败比不装还糟。",
        en: "hint: channel failures are always printed -- a silently failing alert system is worse than none.",
    },
    Entry {
        key: Key::NotifyHintHttps,
        zh: "      https 的 webhook 请走 --notify-cmd 调 curl 等自带 TLS 的工具。",
        en: "      for https webhooks, use --notify-cmd to call a TLS-capable tool such as curl.",
    },
    // —— --list-profiles ——
    Entry {
        key: Key::ProfilesHeader,
        zh: "cping-cli {} · {} · 模板（{}）",
        en: "cping-cli {} · {} · profiles ({})",
    },
    Entry {
        key: Key::ProfilesEmpty,
        zh: "这份配置里没有任何模板。",
        en: "this config file has no profiles.",
    },
    Entry {
        key: Key::ProfilesEmptyAdd,
        zh: "加一份（注意是**双方括号**，写成 [profiles] 会被明确拒绝）：",
        en: "add one (note the **double brackets**; writing [profiles] is explicitly rejected):",
    },
    Entry {
        key: Key::ColProfile,
        zh: "模板",
        en: "profile",
    },
    Entry {
        key: Key::ColTargetSpec,
        zh: "目标规格",
        en: "target spec",
    },
    Entry {
        key: Key::ProfilesUsage,
        zh: "用法：--config {} --profile <模板名>",
        en: "usage: --config {} --profile <profile-name>",
    },
    Entry {
        key: Key::ProfilesNoDefault,
        zh: "     配置里没写 default_profile，必须显式给 --profile",
        en: "     the config has no default_profile; --profile is required",
    },
    Entry {
        key: Key::ProfilesUseDefault,
        zh: "     不给 --profile 时用 default_profile = {} 那一份",
        en: "     without --profile the one named default_profile = {} is used",
    },
    Entry {
        key: Key::ProfilesUnknownKeys,
        zh: "     另有 {} 个本程序不认识的键，已原样保留（写回时不变）",
        en: "     {} unrecognised key(s) were preserved verbatim (unchanged when written back)",
    },
    // —— --dump-config ——
    Entry {
        key: Key::DumpProbeOnly,
        zh: "已写出配置（只含 [probe]，没有目标规格）→ {}",
        en: "wrote config ([probe] only, no target spec) → {}",
    },
    Entry {
        key: Key::DumpProbeOnlyHint,
        zh: "提示：目标规格不在 [probe] 里（装不下 CIDR/多网段），请每次用 --target 指定",
        en: "hint: the target spec does not belong in [probe] (it cannot hold CIDR/multi-segment specs); pass it with --target each time",
    },
    Entry {
        key: Key::DumpWritten,
        zh: "已写出配置 → {}",
        en: "wrote config → {}",
    },
    Entry {
        key: Key::DumpProfileLine,
        zh: "  模板 {}：{}（{}）",
        en: "  profile {}: {} ({})",
    },
    Entry {
        key: Key::DumpCountTargets,
        zh: "{} 个目标",
        en: "{} target(s)",
    },
    Entry {
        key: Key::DumpCountUnparsable,
        zh: "目标规格无法解析",
        en: "target spec could not be parsed",
    },
    Entry {
        key: Key::DumpDefaultSet,
        zh: "  已设为 default_profile：下次直接 cping-cli --config {}",
        en: "  set as default_profile: next time just run cping-cli --config {}",
    },
    Entry {
        key: Key::DumpOverwrote,
        zh: "  注意：模板 {} 原有的目标规格 {} 已被当前生效值覆盖",
        en: "  note: profile {}'s previous target spec {} was overwritten by the effective value",
    },
    Entry {
        key: Key::DumpNotWritten,
        zh: "  未写入（刻意不属模板）：--out / --json / --history / --report /",
        en: "  not written (deliberately not part of a profile): --out / --json / --history / --report /",
    },
    // —— 每轮输出 ——
    Entry {
        key: Key::RoundHeader,
        zh: "第 {} 轮 · {} · {} · 用时 {} s{}",
        en: "round {} · {} · {} · took {} s{}",
    },
    Entry {
        key: Key::RoundCancelled,
        zh: "（已取消）",
        en: " (cancelled)",
    },
    Entry {
        key: Key::RoundStats,
        zh: "  目标 {} · 在线 {} · 超时 {} · 不可达 {} · 权限 {} · 不支持 {} · 错误 {}",
        en: "  targets {} · online {} · timeout {} · unreachable {} · permission {} · unsupported {} · error {}",
    },
    Entry {
        key: Key::RoundChanges,
        zh: "  变化 {} 项：",
        en: "  {} change(s):",
    },
    Entry {
        key: Key::RoundChangesMore,
        zh: "    … 另有 {} 项，见导出文件",
        en: "    ... {} more, see the exported file",
    },
    Entry {
        key: Key::RoundOnlineHosts,
        zh: "  在线主机：",
        en: "  online hosts:",
    },
    Entry {
        key: Key::RoundFpHeader,
        zh: "  端口指纹（附加信息，与上面的在线判定相互独立）：",
        en: "  port fingerprint (extra information, independent of the online verdict above):",
    },
    Entry {
        key: Key::RoundFpNoPorts,
        zh: "    {} 无开放端口",
        en: "    {} no open ports",
    },
    // —— --list-backends ——
    Entry {
        key: Key::ColName,
        zh: "名称",
        en: "name",
    },
    Entry {
        key: Key::ColFamily,
        zh: "地址族",
        en: "address family",
    },
    Entry {
        key: Key::FamilyV4Only,
        zh: "仅 IPv4",
        en: "IPv4 only",
    },
    Entry {
        key: Key::FamilyV6Only,
        zh: "仅 IPv6",
        en: "IPv6 only",
    },
    Entry {
        key: Key::BackendsNeedsPrivilege,
        zh: "需要原始套接字权限",
        en: "requires raw socket privileges",
    },
    Entry {
        key: Key::BackendsHint,
        zh: "提示：IPv6 目标目前请用 --mode tcp（ICMPv6 尚未实装）。",
        en: "hint: for now use --mode tcp for IPv6 targets (ICMPv6 is not implemented yet).",
    },
    Entry {
        key: Key::ErrConfigPrefix,
        zh: "配置 {}：{}",
        en: "config {}: {}",
    },
];

/// 取某个键在指定语言下的模板。
pub fn text(lang: Lang, key: Key) -> &'static str {
    let idx = ordinal(key);
    // `TABLE` 与 `ordinal` 的一致性由 `table_covers_every_key` 守着；
    // 真跑到这里说明有人绕过了那道测试。此时**返回中文兜底**也好过 panic——
    // 界面显示一句中文，比整个程序崩掉好（与核心层同一取舍）。
    //
    // 额外核对条目确实是这个键的：万一有人手工挪动 `TABLE` 的顺序却忘了改
    // `ordinal`，宁可显示兜底句，也不能张冠李戴地把别的键的话显示出来。
    if idx < KEY_COUNT {
        if let Some(e) = TABLE.get(idx) {
            if e.key == key {
                return match lang {
                    Lang::Zh => e.zh,
                    Lang::En => e.en,
                };
            }
        }
    }
    match lang {
        Lang::Zh => "（未登记的消息键）",
        Lang::En => "(unregistered message key)",
    }
}

/// 按本字典的键渲染。
///
/// 算法直接复用核心层的 [`cping_core::i18n::render_template`]：占位符规则
/// （`{}` 顺序替换、参数不足原样保留、多余忽略）只需有一份实现。
pub fn render(lang: Lang, key: Key, args: &[&str]) -> String {
    cping_core::i18n::render_template(text(lang, key), args)
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
    fn render_keeps_placeholder_when_args_run_short() {
        // 少给一个参数：`{}` 必须**留在原地**，而不是变成空串。
        let s = render(Lang::Zh, Key::ErrOutOfRange32, &["--max-pps"]);
        assert_eq!(s, "--max-pps 超出 0-{}");
        // 参数给全（调用方按原样传的是 `Debug` 形式，所以带引号）。
        let s = render(Lang::Zh, Key::ErrOutExtension, &["\"x.xml\""]);
        assert_eq!(
            s,
            "--out \"x.xml\" 的扩展名无法识别，请用 .csv / .txt / .json / .html"
        );
        // 多给参数则忽略，不 panic。
        let s = render(Lang::En, Key::ErrWatchZero, &["多余"]);
        assert_eq!(s, "--watch interval must be greater than 0 seconds");
    }

    #[test]
    fn every_key_is_reachable_in_both_languages() {
        // 顺带守一句：两个语言下每条模板都非空，避免 `text` 走到兜底分支。
        for lang in Lang::ALL {
            for e in TABLE {
                assert!(!text(lang, e.key).is_empty(), "{:?} / {lang}", e.key);
            }
        }
    }
}
