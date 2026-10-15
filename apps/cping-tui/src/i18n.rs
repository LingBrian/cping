//! cping-tui 自己的国际化字典（中 / 英）。
//!
//! # 为什么界面层也要有一份字典
//!
//! 核心层（`cping_core::i18n`）只覆盖**核心逻辑**要说的话：「目标落在保留网段」
//! 「探测超时」这类由 `cping-core` 判定产生的消息。而 TUI 要说的话完全是另一套：
//! 「按 i 编辑」「告警面板」「终端过小」，核心层根本不认识这些概念。
//! 所以两边各有一份键空间，只共用 `cping_core::i18n::render_template` 这一个算法
//! ——这与 CLI / GUI 的做法一致（见 `cping_core::i18n::render` 的文档）。
//!
//! # 机制与核心层同构
//!
//! - **键是枚举**：`text(lang, "typo")` 那种写法在运行期只会静默显示键名本身，
//!   而 `Key::Typo` 直接编译不过。
//! - **双射由测试守着**：`ordinal` 是穷尽 `match`（漏登记编译失败），
//!   而 `TABLE` 必须与 `KEY_COUNT` 一一对应（漏条目测试失败）。
//! - **漏译能自动抓**：`english_has_no_cjk` 与 `placeholders_match` 把最常见的
//!   两类错误（英文里混进中文、中英占位符不一致）变成 CI 红灯。
//!
//! # 刻意不做的事
//!
//! - **CSV / JSON 导出的列名与字段名不本地化**：它们是**数据契约**，同一份报表
//!   在中英文机器上被下游脚本消费时列名必须一致（见 `cping_core::export`）。
//! - **快照 / 时序 / 审计日志的字段名与事件名不本地化**：同上，都是落盘格式。
//! - **命令行参数名本身不本地化**：`--prefix` 永远叫 `--prefix`，
//!   翻译它只会让脚本与文档对不上。
//! - **TUI 的状态符号不本地化**：`#` `+` `*` `-` `!` `.` 是**形状编码**，
//!   它们是「无颜色终端下仍可读」这条承诺的实现方式，翻了就失去意义。
//! - **来自操作系统 / 核心层 `detail` 的原始错误详情原样透传**：
//!   用户拿着它去搜索时要搜的是原始文本，只本地化外面那层框。

use cping_core::i18n::Lang;

/// TUI 面向用户的消息键。
///
/// 新增变体后必须同时做两件事，否则测试会失败：
/// 在 [`ordinal`] 里登记序号，并在 [`TABLE`] 里给出中英条目。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    // —— 标题与区块 ——
    AppTitle,
    InputLabelTarget,
    InputHintEditing,
    InputHintIdle,
    MatrixTitle,
    TerminalTooSmall,
    TableTitle,
    TableFilteredSuffix,
    TableEmpty,
    RestoreTitle,
    ChangesTitle,
    TrendTitle,
    FingerprintTitle,
    AlertsTitle,
    HelpTitle,
    // —— 状态栏 ——
    StateScanning,
    StateMonitoring,
    StateIdle,
    MonitorWatch,
    StatusDetail,
    ShortcutBar,
    StatusReady,
    // —— 表格详情行 ——
    DetailVendor,
    DetailHostname,
    // —— 状态栏消息 ——
    StatusPolicyInvalid,
    StatusCantSwitchMode,
    StatusModeSwitched,
    StatusResized,
    StatusInputConfirmed,
    StatusQuitConfirm,
    StatusAlreadyRunning,
    StatusNoTask,
    StatusEditingPrefix,
    StatusFilterOn,
    StatusFilterOff,
    StatusTrendEmpty,
    StatusFingerprintBusy,
    StatusFingerprintNoHosts,
    StatusFingerprintRunning,
    StatusFingerprintDone,
    StatusFingerprintThreadFail,
    FingerprintLinesHeader,
    FingerprintLinesEmpty,
    FingerprintNoOpenPorts,
    TrendHeader,
    TrendEmpty1,
    TrendEmpty2,
    TrendColHost,
    TrendColUptime,
    TrendColOnlineTotal,
    TrendColAvgRtt,
    TrendColWorstRtt,
    TrendColJitter,
    TrendLegend,
    AlertLinesDisabled1,
    AlertLinesDisabled2,
    AlertStateMuted,
    AlertStateActive,
    AlertStatusLine,
    AlertColTime,
    AlertColRound,
    AlertColHost,
    AlertColKind,
    AlertLinesEmpty1,
    AlertLinesEmpty2,
    AlertLinesLatest,
    AlertNone,
    AlertNote,
    AlertNoteMutedSuffix,
    StatusMonitorOff,
    StatusMonitorOn,
    StatusInterval,
    StatusInvalidInput,
    StatusScanStarted,
    StatusStartFailed,
    StatusCancelRequested,
    StatusRoundDone,
    StatusCancelled,
    ChangeNote,
    StatusNoResultsEnrich,
    EnrichTableFailed,
    EnrichUnknownReason,
    EnrichNoFill,
    EnrichFilled,
    StatusHostnameBusy,
    StatusHostnameNoRow,
    StatusHostnameNoResult,
    StatusHostnameAlready,
    StatusHostnameLooking,
    StatusHostnameNotFound,
    StatusHostnameThreadFail,
    StatusNoResultsToSave,
    StatusSnapshotSaved,
    StatusSnapshotSaveFailed,
    StatusPendingFound,
    StatusPendingUnreadable,
    StatusRestoreUnreadable,
    StatusRestoreFailed,
    StatusRestoreBadPrefix,
    StatusRestoreSpecFailed,
    StatusRestoreMismatch,
    StatusRestoreIndexOob,
    StatusRestored,
    StatusNoSnapshot,
    StatusDismissed,
    // —— 恢复确认框 ——
    RestoreDetected,
    RestoreUnreadable,
    RestoreScanTime,
    RestorePrefix,
    RestoreRows,
    RestoreElapsed,
    RestoreFile,
    RestoreSayError,
    RestoreSayOk,
    // —— 变化事件流 ——
    ChangesEmpty,
    ChangeRow,
    // —— 帮助面板 ——
    HelpKeysHeading,
    HelpLineEnter,
    HelpLineI,
    HelpLineEsc,
    HelpLineGt,
    HelpLineF,
    HelpLineSS,
    HelpLineJH,
    HelpLineScroll,
    HeadingM13,
    HelpM13F,
    HelpM13NoteA,
    HelpM13NoteB,
    HeadingM14,
    HelpM14U,
    HelpM14NoteA,
    HelpM14NoteB,
    HelpM14NoteC,
    HeadingM15,
    HelpM15A,
    HelpM15B,
    HelpM15Mute,
    HelpM15MuteB,
    HelpM15Note,
    HelpM15NoteB,
    HeadingM9,
    HelpM9Input,
    HelpM9InputB,
    HelpM9T,
    HelpM9P,
    HeadingM12,
    HelpM12Current,
    HelpM12Options,
    HelpM12V6,
    HelpM12Icmp6,
    HelpM12Local,
    HeadingM8,
    HelpM8M,
    HelpM8Bracket,
    HelpM8E,
    HeadingM7,
    HelpM7M,
    HelpM7N,
    HelpM7Wl,
    HelpM7Note,
    HelpM7NoteB,
    HelpLineHelpHint,
    HelpLineQuit,
    HelpLineCtrlC,
    HelpSymbols,
    // —— `--help` 文本 ——
    CliHelpTop,
    CliHelpOptions,
    CliHelpBackend,
    CliHelpTargets,
    CliHelpSnapshot,
    CliHelpWarning,
    CliHelpKeys,
    // —— 命令行参数告警（参数名本身不翻译，作为 `{}` 传进来） ——
    ArgIntExpected,
    ArgMissingValue,
    ArgLangInvalid,
    ArgWarnSeparator,
    // —— 告警面板的状态栏提示 ——
    StatusAlertsEnabled,
    StatusAlertsEmpty,
    StatusAlertsPanel,
    StatusAlertsMuted,
    StatusAlertsUnmuted,
    // —— 导出与「本机地址」占位文本 ——
    StatusNothingToExport,
    StatusNoDoneResults,
    StatusExported,
    StatusWriteFailed,
    StatusExportFailed,
    LocalIpUnknown,
    LocalIpUnknownM5,
    HelpCliFlags,
    // —— M16 模板选择面板（TUI 自己的界面文案） ——
    ProfilesTitle,
    ProfilesHint,
    ProfilesEmpty,
    ProfilesNoConfig,
    ProfilesFilePath,
    ProfilesCurrentMark,
    ProfilesCliNote,
    ProfileNone,
    ProfileInUse,
    StatusProfileApplied,
    StatusProfileBlocked,
    StatusConfigUnreadable,
    StatusConfigNotes,
    // —— M17 安全策略面板（TUI 自己的界面文案） ——
    PolicyTitle,
    PolicyHint,
    PolicyLinesCurrent,
    PolicyLinesDefaults,
    PolicyFieldPps,
    PolicyFieldTargets,
    PolicyFieldReserved,
    PolicyFieldLine,
    PolicyOn,
    PolicyOff,
    PolicyWarnReserved,
    StatusPolicyApplied,
    // —— 新增面板的帮助行 ——
    HelpM16P,
    HelpM17G,
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
/// 这个 `match` 是**穷尽**的：新增 [`Key`] 变体却忘了登记，编译就会失败；
/// 登记了却忘了加进 [`TABLE`]，`table_covers_every_key` 会因为序号不连续而失败。
/// 两道关卡合起来保证「字典不会漏键」。
pub const fn ordinal(key: Key) -> usize {
    match key {
        Key::AppTitle => 0,
        Key::InputLabelTarget => 1,
        Key::InputHintEditing => 2,
        Key::InputHintIdle => 3,
        Key::MatrixTitle => 4,
        Key::TerminalTooSmall => 5,
        Key::TableTitle => 6,
        Key::TableFilteredSuffix => 7,
        Key::TableEmpty => 8,
        Key::RestoreTitle => 9,
        Key::ChangesTitle => 10,
        Key::TrendTitle => 11,
        Key::FingerprintTitle => 12,
        Key::AlertsTitle => 13,
        Key::HelpTitle => 14,
        Key::StateScanning => 15,
        Key::StateMonitoring => 16,
        Key::StateIdle => 17,
        Key::MonitorWatch => 18,
        Key::StatusDetail => 19,
        Key::ShortcutBar => 20,
        Key::StatusReady => 21,
        Key::DetailVendor => 22,
        Key::DetailHostname => 23,
        Key::StatusPolicyInvalid => 24,
        Key::StatusCantSwitchMode => 25,
        Key::StatusModeSwitched => 26,
        Key::StatusResized => 27,
        Key::StatusInputConfirmed => 28,
        Key::StatusQuitConfirm => 29,
        Key::StatusAlreadyRunning => 30,
        Key::StatusNoTask => 31,
        Key::StatusEditingPrefix => 32,
        Key::StatusFilterOn => 33,
        Key::StatusFilterOff => 34,
        Key::StatusTrendEmpty => 35,
        Key::StatusFingerprintBusy => 36,
        Key::StatusFingerprintNoHosts => 37,
        Key::StatusFingerprintRunning => 38,
        Key::StatusFingerprintDone => 39,
        Key::StatusFingerprintThreadFail => 40,
        Key::FingerprintLinesHeader => 41,
        Key::FingerprintLinesEmpty => 42,
        Key::FingerprintNoOpenPorts => 43,
        Key::TrendHeader => 44,
        Key::TrendEmpty1 => 45,
        Key::TrendEmpty2 => 46,
        Key::TrendColHost => 47,
        Key::TrendColUptime => 48,
        Key::TrendColOnlineTotal => 49,
        Key::TrendColAvgRtt => 50,
        Key::TrendColWorstRtt => 51,
        Key::TrendColJitter => 52,
        Key::TrendLegend => 53,
        Key::AlertLinesDisabled1 => 54,
        Key::AlertLinesDisabled2 => 55,
        Key::AlertStateMuted => 56,
        Key::AlertStateActive => 57,
        Key::AlertStatusLine => 58,
        Key::AlertColTime => 59,
        Key::AlertColRound => 60,
        Key::AlertColHost => 61,
        Key::AlertColKind => 62,
        Key::AlertLinesEmpty1 => 63,
        Key::AlertLinesEmpty2 => 64,
        Key::AlertLinesLatest => 65,
        Key::AlertNone => 66,
        Key::AlertNote => 67,
        Key::AlertNoteMutedSuffix => 68,
        Key::StatusMonitorOff => 69,
        Key::StatusMonitorOn => 70,
        Key::StatusInterval => 71,
        Key::StatusInvalidInput => 72,
        Key::StatusScanStarted => 73,
        Key::StatusStartFailed => 74,
        Key::StatusCancelRequested => 75,
        Key::StatusRoundDone => 76,
        Key::StatusCancelled => 77,
        Key::ChangeNote => 78,
        Key::StatusNoResultsEnrich => 79,
        Key::EnrichTableFailed => 80,
        Key::EnrichUnknownReason => 81,
        Key::EnrichNoFill => 82,
        Key::EnrichFilled => 83,
        Key::StatusHostnameBusy => 84,
        Key::StatusHostnameNoRow => 85,
        Key::StatusHostnameNoResult => 86,
        Key::StatusHostnameAlready => 87,
        Key::StatusHostnameLooking => 88,
        Key::StatusHostnameNotFound => 89,
        Key::StatusHostnameThreadFail => 90,
        Key::StatusNoResultsToSave => 91,
        Key::StatusSnapshotSaved => 92,
        Key::StatusSnapshotSaveFailed => 93,
        Key::StatusPendingFound => 94,
        Key::StatusPendingUnreadable => 95,
        Key::StatusRestoreUnreadable => 96,
        Key::StatusRestoreFailed => 97,
        Key::StatusRestoreBadPrefix => 98,
        Key::StatusRestoreSpecFailed => 99,
        Key::StatusRestoreMismatch => 100,
        Key::StatusRestoreIndexOob => 101,
        Key::StatusRestored => 102,
        Key::StatusNoSnapshot => 103,
        Key::StatusDismissed => 104,
        Key::RestoreDetected => 105,
        Key::RestoreUnreadable => 106,
        Key::RestoreScanTime => 107,
        Key::RestorePrefix => 108,
        Key::RestoreRows => 109,
        Key::RestoreElapsed => 110,
        Key::RestoreFile => 111,
        Key::RestoreSayError => 112,
        Key::RestoreSayOk => 113,
        Key::ChangesEmpty => 114,
        Key::ChangeRow => 115,
        Key::HelpKeysHeading => 116,
        Key::HelpLineEnter => 117,
        Key::HelpLineI => 118,
        Key::HelpLineEsc => 119,
        Key::HelpLineGt => 120,
        Key::HelpLineF => 121,
        Key::HelpLineSS => 122,
        Key::HelpLineJH => 123,
        Key::HelpLineScroll => 124,
        Key::HeadingM13 => 125,
        Key::HelpM13F => 126,
        Key::HelpM13NoteA => 127,
        Key::HelpM13NoteB => 128,
        Key::HeadingM14 => 129,
        Key::HelpM14U => 130,
        Key::HelpM14NoteA => 131,
        Key::HelpM14NoteB => 132,
        Key::HelpM14NoteC => 133,
        Key::HeadingM15 => 134,
        Key::HelpM15A => 135,
        Key::HelpM15B => 136,
        Key::HelpM15Mute => 137,
        Key::HelpM15MuteB => 138,
        Key::HelpM15Note => 139,
        Key::HelpM15NoteB => 140,
        Key::HeadingM9 => 141,
        Key::HelpM9Input => 142,
        Key::HelpM9InputB => 143,
        Key::HelpM9T => 144,
        Key::HelpM9P => 145,
        Key::HeadingM12 => 146,
        Key::HelpM12Current => 147,
        Key::HelpM12Options => 148,
        Key::HelpM12V6 => 149,
        Key::HelpM12Icmp6 => 150,
        Key::HelpM12Local => 151,
        Key::HeadingM8 => 152,
        Key::HelpM8M => 153,
        Key::HelpM8Bracket => 154,
        Key::HelpM8E => 155,
        Key::HeadingM7 => 156,
        Key::HelpM7M => 157,
        Key::HelpM7N => 158,
        Key::HelpM7Wl => 159,
        Key::HelpM7Note => 160,
        Key::HelpM7NoteB => 161,
        Key::HelpLineHelpHint => 162,
        Key::HelpLineQuit => 163,
        Key::HelpLineCtrlC => 164,
        Key::HelpSymbols => 165,
        Key::CliHelpTop => 166,
        Key::CliHelpOptions => 167,
        Key::CliHelpBackend => 168,
        Key::CliHelpTargets => 169,
        Key::CliHelpSnapshot => 170,
        Key::CliHelpWarning => 171,
        Key::CliHelpKeys => 172,
        Key::ArgIntExpected => 173,
        Key::ArgMissingValue => 174,
        Key::ArgLangInvalid => 175,
        Key::ArgWarnSeparator => 176,
        Key::StatusAlertsEnabled => 177,
        Key::StatusAlertsEmpty => 178,
        Key::StatusAlertsPanel => 179,
        Key::StatusAlertsMuted => 180,
        Key::StatusAlertsUnmuted => 181,
        Key::StatusNothingToExport => 182,
        Key::StatusNoDoneResults => 183,
        Key::StatusExported => 184,
        Key::StatusWriteFailed => 185,
        Key::StatusExportFailed => 186,
        Key::LocalIpUnknown => 187,
        Key::LocalIpUnknownM5 => 188,
        Key::HelpCliFlags => 189,
        Key::ProfilesTitle => 190,
        Key::ProfilesHint => 191,
        Key::ProfilesEmpty => 192,
        Key::ProfilesNoConfig => 193,
        Key::ProfilesFilePath => 194,
        Key::ProfilesCurrentMark => 195,
        Key::ProfilesCliNote => 196,
        Key::ProfileNone => 197,
        Key::ProfileInUse => 198,
        Key::StatusProfileApplied => 199,
        Key::StatusProfileBlocked => 200,
        Key::StatusConfigUnreadable => 201,
        Key::StatusConfigNotes => 202,
        Key::PolicyTitle => 203,
        Key::PolicyHint => 204,
        Key::PolicyLinesCurrent => 205,
        Key::PolicyLinesDefaults => 206,
        Key::PolicyFieldPps => 207,
        Key::PolicyFieldTargets => 208,
        Key::PolicyFieldReserved => 209,
        Key::PolicyFieldLine => 210,
        Key::PolicyOn => 211,
        Key::PolicyOff => 212,
        Key::PolicyWarnReserved => 213,
        Key::StatusPolicyApplied => 214,
        Key::HelpM16P => 215,
        Key::HelpM17G => 216,
    }
}

/// 字典里的键总数。
pub const KEY_COUNT: usize = 217;

/// 全部条目。顺序必须与 [`ordinal`] 一致（有测试守着）。
///
/// 条目里的 `{}` 是顺序占位符，个数与顺序必须中英一致。不引入具名占位符
/// 是为了让两种语言的语序可以完全不同（与核心层同一理由）。
pub static TABLE: &[Entry] = &[
    Entry {
        key: Key::AppTitle,
        zh: " C网群PING工具 · 终端版 ",
        en: " CPing · Terminal ",
    },
    Entry {
        key: Key::InputLabelTarget,
        zh: " 目标: ",
        en: " Target: ",
    },
    Entry {
        key: Key::InputHintEditing,
        zh: "  ← 输入中：网段 / 单主机 / CIDR / 多网段，Enter 确认",
        en: "  <- editing: network / single host / CIDR / multiple, Enter to confirm",
    },
    Entry {
        key: Key::InputHintIdle,
        zh: "  （按 i 编辑）",
        en: "  (press i to edit)",
    },
    Entry {
        key: Key::MatrixTitle,
        zh: " 图形显示（16×16 = {} 格，格号 = IP 最后一段） ",
        en: " Matrix view (16×16 = {} cells; cell number = last IP octet) ",
    },
    Entry {
        key: Key::TerminalTooSmall,
        zh: "终端过小：矩阵需要至少 {} 列 × {} 行，当前 {} × {}。可切换表格视图（t）",
        en: "terminal too small: the matrix needs at least {} columns × {} rows, current {} × {}; switch to the table view (t)",
    },
    Entry {
        key: Key::TableTitle,
        zh: " 表单显示（{} 行{}） ",
        en: " Table view ({} rows{}) ",
    },
    Entry {
        key: Key::TableFilteredSuffix,
        zh: "，已过滤不在线",
        en: ", offline hidden",
    },
    Entry {
        key: Key::TableEmpty,
        zh: "没有可显示的行（可尝试关闭过滤：按 f）",
        en: "no rows to show (try turning the filter off: press f)",
    },
    Entry {
        key: Key::RestoreTitle,
        zh: " CPing · 发现上次的结果 ",
        en: " CPing · previous results found ",
    },
    Entry {
        key: Key::ChangesTitle,
        zh: " 变化事件流（累计 {} 条，缓冲上限 {}） ",
        en: " Change feed ({} total, {} max buffered) ",
    },
    Entry {
        key: Key::TrendTitle,
        zh: " 历史与趋势（{} 台主机 · {} 点 / {} 轮） ",
        en: " History & trend ({} hosts · {} points / {} rounds) ",
    },
    Entry {
        key: Key::FingerprintTitle,
        zh: " 端口指纹（{} 台主机 · 附加信息，与在线判定相互独立） ",
        en: " Port fingerprints ({} hosts · extra info, independent of the online verdict) ",
    },
    Entry {
        key: Key::AlertsTitle,
        zh: " 告警（累计 {} 条 · 任意键关闭 · ! 切静音） ",
        en: " Alerts ({} total · any key closes · ! toggles mute) ",
    },
    Entry {
        key: Key::HelpTitle,
        zh: " 帮助（任意键关闭） ",
        en: " Help (any key closes) ",
    },
    Entry {
        key: Key::StateScanning,
        zh: "探测中",
        en: "scanning",
    },
    Entry {
        key: Key::StateMonitoring,
        zh: "监控待扫",
        en: "monitoring",
    },
    Entry {
        key: Key::StateIdle,
        zh: "空闲",
        en: "idle",
    },
    Entry {
        key: Key::MonitorWatch,
        zh: "  监控 {}s（{}s 后）",
        en: "  monitor {}s (next in {}s)",
    },
    Entry {
        key: Key::StatusDetail,
        zh: " {} 第{}轮  {}  在线 {}  超时 {}  不可达 {}  错误 {}  变化 {}  用时 {}s  后端 {}  本机 {}  厂商 {}  主机名 {}{}{}",
        en: " {} round {}  {}  online {}  timeout {}  unreachable {}  error {}  changes {}  elapsed {}s  backend {}  local {}  vendor {}  hostname {}{}{}",
    },
    Entry {
        key: Key::ShortcutBar,
        zh: "Enter 开始 · i 编辑 · g/t 视图 · f 过滤 · s/S/j/H 导出 · m 补厂商 · n 查主机名 · F 指纹 · U 趋势 · A 告警 · ! 静音 · w/l 快照 · M 监控 · e 变化 · T/p 方式 · P 模板 · G 策略 · ? 帮助 · q 退出",
        en: "Enter start · i edit · g/t view · f filter · s/S/j/H export · m vendor · n hostname · F fingerprint · U trend · A alerts · ! mute · w/l snapshot · M monitor · e changes · T/p mode · P profiles · G policy · ? help · q quit",
    },
    Entry {
        key: Key::StatusReady,
        zh: "就绪。按 i 编辑网段，Enter 开始探测，M 开监控，? 查看帮助。",
        en: "Ready. Press i to edit the target, Enter to scan, M to monitor, ? for help.",
    },
    Entry {
        key: Key::DetailVendor,
        zh: "厂商 {}",
        en: "vendor {}",
    },
    Entry {
        key: Key::DetailHostname,
        zh: "主机 {}",
        en: "host {}",
    },
    Entry {
        key: Key::StatusPolicyInvalid,
        zh: "安全策略无效：{}",
        en: "invalid scan policy: {}",
    },
    Entry {
        key: Key::StatusCantSwitchMode,
        zh: "任务运行中不能切换探测方式，请先取消（Esc）",
        en: "cannot switch probe mode while a task is running; cancel it first (Esc)",
    },
    Entry {
        key: Key::StatusModeSwitched,
        zh: "探测方式已切换为 {}",
        en: "probe mode switched to {}",
    },
    Entry {
        key: Key::StatusResized,
        zh: "已适配终端尺寸",
        en: "terminal resized",
    },
    Entry {
        key: Key::StatusInputConfirmed,
        zh: "输入已确认",
        en: "input confirmed",
    },
    Entry {
        key: Key::StatusQuitConfirm,
        zh: "探测仍在进行：再按一次 q 退出（已完成的按 Esc 取消）",
        en: "scan still running: press q again to quit (Esc cancels and keeps finished results)",
    },
    Entry {
        key: Key::StatusAlreadyRunning,
        zh: "已有任务在运行",
        en: "a task is already running",
    },
    Entry {
        key: Key::StatusNoTask,
        zh: "没有正在运行的任务",
        en: "no task is running",
    },
    Entry {
        key: Key::StatusEditingPrefix,
        zh: "编辑网段前缀（只接受数字与点），Enter 确认",
        en: "editing the target prefix (digits and dots), Enter to confirm",
    },
    Entry {
        key: Key::StatusFilterOn,
        zh: "已开启「过滤不在线」",
        en: "offline filter on",
    },
    Entry {
        key: Key::StatusFilterOff,
        zh: "已关闭「过滤不在线」",
        en: "offline filter off",
    },
    Entry {
        key: Key::StatusTrendEmpty,
        zh: "趋势面板：还没有历史数据，先扫描一轮（M 开监控可持续累积）",
        en: "trend panel: no history yet; run one scan first (M keeps accumulating)",
    },
    Entry {
        key: Key::StatusFingerprintBusy,
        zh: "已有一次指纹探测在进行中，稍候…",
        en: "a fingerprint scan is already running, please wait...",
    },
    Entry {
        key: Key::StatusFingerprintNoHosts,
        zh: "没有在线主机可做端口指纹（先扫描，或先把光标移到在线行）",
        en: "no online hosts to fingerprint (scan first, or move the cursor to an online row)",
    },
    Entry {
        key: Key::StatusFingerprintRunning,
        zh: "正在对 {} 台主机探测 {} 个端口（只读横幅，不发送协议请求）…",
        en: "probing {} ports on {} hosts (banners are read-only; no protocol requests are sent)...",
    },
    Entry {
        key: Key::StatusFingerprintDone,
        zh: "指纹完成：{} 台主机 × {} 个端口 · 发现 {} 个开放端口（{} 台有开放端口）",
        en: "fingerprint done: {} hosts × {} ports · {} open ports found ({} hosts with open ports)",
    },
    Entry {
        key: Key::StatusFingerprintThreadFail,
        zh: "指纹探测线程异常结束",
        en: "fingerprint worker ended unexpectedly",
    },
    Entry {
        key: Key::FingerprintLinesHeader,
        zh: "端口指纹（附加信息，与在线判定相互独立）",
        en: "Port fingerprints (extra info, independent of the online verdict)",
    },
    Entry {
        key: Key::FingerprintLinesEmpty,
        zh: "（还没有指纹结果：按 F 对选中主机探测）",
        en: "(no fingerprints yet: press F to probe the selected host)",
    },
    Entry {
        key: Key::FingerprintNoOpenPorts,
        zh: "{}  无开放端口",
        en: "{}  no open ports",
    },
    Entry {
        key: Key::TrendHeader,
        zh: "时间跨度 {} 小时 · {} 个时间点 · 累计 {} 轮（保留 {} 天）",
        en: "span {} h · {} points · {} rounds total (kept {} days)",
    },
    Entry {
        key: Key::TrendEmpty1,
        zh: "还没有历史数据：先扫描一轮。",
        en: "no history yet: run one scan first.",
    },
    Entry {
        key: Key::TrendEmpty2,
        zh: "开监控（M）连续跑几轮，这里就会出现每台主机的可用率曲线。",
        en: "enable monitoring (M) for a few rounds and a per-host uptime curve will appear here.",
    },
    Entry {
        key: Key::TrendColHost,
        zh: "主机",
        en: "host",
    },
    Entry {
        key: Key::TrendColUptime,
        zh: "可用率",
        en: "uptime",
    },
    Entry {
        key: Key::TrendColOnlineTotal,
        zh: "在线/总",
        en: "up/total",
    },
    Entry {
        key: Key::TrendColAvgRtt,
        zh: "平均RTT",
        en: "avg RTT",
    },
    Entry {
        key: Key::TrendColWorstRtt,
        zh: "最差RTT",
        en: "worst RTT",
    },
    Entry {
        key: Key::TrendColJitter,
        zh: "抖动",
        en: "jitter",
    },
    Entry {
        key: Key::TrendLegend,
        zh: "曲线：每格一轮在线率（越高越满）；· = 该轮没测到这台主机（≠ 离线）。宽度 {} 格。",
        en: "curve: one cell per round's uptime (fuller = higher); · = not measured that round (≠ offline). Width {} cells.",
    },
    Entry {
        key: Key::AlertLinesDisabled1,
        zh: "告警未启用。",
        en: "Alerts are not enabled.",
    },
    Entry {
        key: Key::AlertLinesDisabled2,
        zh: "按 A 打开面板即启用（默认：新增 / 离线 / 延迟>200ms / 可用率<90%）。",
        en: "Pressing A opens the panel and enables them (defaults: new host / offline / latency > 200 ms / uptime < 90%).",
    },
    Entry {
        key: Key::AlertStateMuted,
        zh: "已静音",
        en: "muted",
    },
    Entry {
        key: Key::AlertStateActive,
        zh: "生效中",
        en: "active",
    },
    Entry {
        key: Key::AlertStatusLine,
        zh: "状态：{} · 活跃规则 {}（{}）· 累计告警 {} 条 · 冷却挡下 {} 条",
        en: "state: {} · active rules {} ({}) · {} alerts total · {} suppressed by cooldown",
    },
    Entry {
        key: Key::AlertColTime,
        zh: "时间",
        en: "time",
    },
    Entry {
        key: Key::AlertColRound,
        zh: "轮次",
        en: "round",
    },
    Entry {
        key: Key::AlertColHost,
        zh: "主机",
        en: "host",
    },
    Entry {
        key: Key::AlertColKind,
        zh: "类别",
        en: "kind",
    },
    Entry {
        key: Key::AlertLinesEmpty1,
        zh: "还没有告警记录：先跑一轮（Enter），或开监控（M）持续观察。",
        en: "no alerts yet: run one round (Enter), or enable monitoring (M) and keep watching.",
    },
    Entry {
        key: Key::AlertLinesEmpty2,
        zh: "「新增主机」在第一轮不会报——否则扫个 /24 会一次刷出 255 条。",
        en: "\"new host\" is not reported in the first round; otherwise a /24 scan would flood 255 entries at once.",
    },
    Entry {
        key: Key::AlertLinesLatest,
        zh: "最近一条：{}",
        en: "latest: {}",
    },
    Entry {
        key: Key::AlertNone,
        zh: "（无）",
        en: "(none)",
    },
    Entry {
        key: Key::AlertNote,
        zh: "，告警 {} 条{}",
        en: ", {} alerts{}",
    },
    Entry {
        key: Key::AlertNoteMutedSuffix,
        zh: "（已静音）",
        en: " (muted)",
    },
    Entry {
        key: Key::StatusMonitorOff,
        zh: "持续监控已关闭",
        en: "continuous monitoring off",
    },
    Entry {
        key: Key::StatusMonitorOn,
        zh: "持续监控已开启：每 {} 秒重扫一次（M 关闭，[ ] 调间隔）",
        en: "continuous monitoring on: rescanning every {} s (M stops it, [ ] adjusts the interval)",
    },
    Entry {
        key: Key::StatusInterval,
        zh: "重扫间隔 = {} 秒",
        en: "rescan interval = {} s",
    },
    Entry {
        key: Key::StatusInvalidInput,
        zh: "输入无效：{}",
        en: "invalid input: {}",
    },
    Entry {
        key: Key::StatusScanStarted,
        zh: "开始探测 {}（{} · 后端 {}）",
        en: "scan started: {} ({} · backend {})",
    },
    Entry {
        key: Key::StatusStartFailed,
        zh: "无法启动：{}",
        en: "cannot start: {}",
    },
    Entry {
        key: Key::StatusCancelRequested,
        zh: "已请求取消：不再调度新目标，已完成结果保留",
        en: "cancel requested: no new targets will be scheduled; finished results are kept",
    },
    Entry {
        key: Key::StatusRoundDone,
        zh: "第 {} 轮完成：{} 个目标，{} 在线，用时 {}s {}{}",
        en: "round {} done: {} targets, {} online, {}s {}{}",
    },
    Entry {
        key: Key::StatusCancelled,
        zh: "已取消：完成 {}/{}，其中 {} 在线，已完成结果保留 {}",
        en: "cancelled: {}/{} done, {} online; finished results kept {}",
    },
    Entry {
        key: Key::ChangeNote,
        zh: "，本轮变化 {} 项（上线 {} / 离线 {}）",
        en: ", {} changes this round ({} up / {} down)",
    },
    Entry {
        key: Key::StatusNoResultsEnrich,
        zh: "尚无扫描结果可补全",
        en: "no scan results to enrich yet",
    },
    Entry {
        key: Key::EnrichTableFailed,
        zh: "读取邻居表失败：{}",
        en: "failed to read the neighbor table: {}",
    },
    Entry {
        key: Key::EnrichUnknownReason,
        zh: "未知原因",
        en: "unknown reason",
    },
    Entry {
        key: Key::EnrichNoFill,
        zh: "邻居表 {} 条，没有可回填的目标（ARP 表只记录近期通信过的地址）",
        en: "neighbor table has {} entries; nothing to fill in (the ARP table only records recently active addresses)",
    },
    Entry {
        key: Key::EnrichFilled,
        zh: "邻居表 {} 条，新填 MAC {} 条、厂商 {} 条",
        en: "neighbor table has {} entries; filled {} MAC and {} vendor values",
    },
    Entry {
        key: Key::StatusHostnameBusy,
        zh: "已有一次主机名反查在进行中，稍候…",
        en: "a hostname lookup is already running, please wait...",
    },
    Entry {
        key: Key::StatusHostnameNoRow,
        zh: "没有可反查的行（先扫描、或用 t 切到表格视图）",
        en: "no row to look up (scan first, or press t for the table view)",
    },
    Entry {
        key: Key::StatusHostnameNoResult,
        zh: "该行尚未探测出结果",
        en: "that row has no probe result yet",
    },
    Entry {
        key: Key::StatusHostnameAlready,
        zh: "{} 已有主机名 {}",
        en: "{} already has hostname {}",
    },
    Entry {
        key: Key::StatusHostnameLooking,
        zh: "正在反查 {} 的主机名（最长 1.5s）…",
        en: "looking up the hostname of {} (up to 1.5 s)...",
    },
    Entry {
        key: Key::StatusHostnameNotFound,
        zh: "{} 没有反向解析记录",
        en: "no reverse DNS record for {}",
    },
    Entry {
        key: Key::StatusHostnameThreadFail,
        zh: "主机名反查线程异常结束",
        en: "hostname lookup worker ended unexpectedly",
    },
    Entry {
        key: Key::StatusNoResultsToSave,
        zh: "尚无扫描结果可保存",
        en: "no scan results to save yet",
    },
    Entry {
        key: Key::StatusSnapshotSaved,
        zh: "已保存快照：{} 条结果（共 {} 个目标）→ {}",
        en: "snapshot saved: {} results (of {} targets) -> {}",
    },
    Entry {
        key: Key::StatusSnapshotSaveFailed,
        zh: "保存快照失败：{}",
        en: "failed to save the snapshot: {}",
    },
    Entry {
        key: Key::StatusPendingFound,
        zh: "发现上次的扫描结果，是否恢复？y 恢复 / n 丢弃",
        en: "found results from last time; restore them? y restore / n discard",
    },
    Entry {
        key: Key::StatusPendingUnreadable,
        zh: "发现一份无法读取的快照，按 n 丢弃",
        en: "found an unreadable snapshot; press n to discard it",
    },
    Entry {
        key: Key::StatusRestoreUnreadable,
        zh: "快照无法读取，已跳过：{}",
        en: "snapshot is unreadable, skipped: {}",
    },
    Entry {
        key: Key::StatusRestoreFailed,
        zh: "恢复失败：{}",
        en: "restore failed: {}",
    },
    Entry {
        key: Key::StatusRestoreBadPrefix,
        zh: "快照中的网段无法解析：{}",
        en: "the snapshot's network prefix cannot be parsed: {}",
    },
    Entry {
        key: Key::StatusRestoreSpecFailed,
        zh: "按快照网段生成目标失败：{}",
        en: "failed to build targets from the snapshot's prefix: {}",
    },
    Entry {
        key: Key::StatusRestoreMismatch,
        zh: "快照与当前目标规则不一致（快照 {} 个目标，按网段算出 {} 个），已跳过",
        en: "snapshot does not match the current target rules (snapshot has {} targets, the prefix yields {}); skipped",
    },
    Entry {
        key: Key::StatusRestoreIndexOob,
        zh: "快照中的下标 {} 超出范围，已跳过",
        en: "index {} in the snapshot is out of range; skipped",
    },
    Entry {
        key: Key::StatusRestored,
        zh: "已恢复 {}（{} 条结果 / {} 个目标，用时 {}s）",
        en: "restored {} ({} results / {} targets, {}s)",
    },
    Entry {
        key: Key::StatusNoSnapshot,
        zh: "没有可恢复的快照：{} 不存在或已经处理过",
        en: "no snapshot to restore: {} does not exist or was already handled",
    },
    Entry {
        key: Key::StatusDismissed,
        zh: "已忽略上次的扫描结果",
        en: "previous scan results ignored",
    },
    Entry {
        key: Key::RestoreDetected,
        zh: "检测到上次未处理完的扫描结果。",
        en: "Unfinished scan results from last time were found.",
    },
    Entry {
        key: Key::RestoreUnreadable,
        zh: "  无法读取：{}",
        en: "  unreadable: {}",
    },
    Entry {
        key: Key::RestoreScanTime,
        zh: "  扫描时间 : {}",
        en: "  scan time : {}",
    },
    Entry {
        key: Key::RestorePrefix,
        zh: "  网段     : {}",
        en: "  network   : {}",
    },
    Entry {
        key: Key::RestoreRows,
        zh: "  结果     : {} 条 / {} 个目标",
        en: "  results   : {} / {} targets",
    },
    Entry {
        key: Key::RestoreElapsed,
        zh: "  用时     : {}s",
        en: "  elapsed   : {}s",
    },
    Entry {
        key: Key::RestoreFile,
        zh: "  文件     : {}",
        en: "  file      : {}",
    },
    Entry {
        key: Key::RestoreSayError,
        zh: "  y 不使用      n / Esc 删除该文件",
        en: "  y discard     n / Esc delete the file",
    },
    Entry {
        key: Key::RestoreSayOk,
        zh: "  y 恢复结果    n / Esc 不使用（默认不恢复）",
        en: "  y restore     n / Esc discard (default: do not restore)",
    },
    Entry {
        key: Key::ChangesEmpty,
        zh: "还没有变化记录。\n\n用 M 开启持续监控后，每轮扫描都会与上一轮比对：\n · 上线 / 离线\n · 延迟档位变化（<50ms / 50–100ms / >100ms）\n · 非在线状态之间的变化\n\n首轮只建立基线，不产生变化事件。",
        en: "No changes recorded yet.\n\nWith monitoring (M) on, each round is compared against the previous one:\n · up / down\n · latency tier changes (<50 ms / 50–100 ms / >100 ms)\n · changes between non-online states\n\nThe first round only establishes a baseline and produces no change events.",
    },
    Entry {
        key: Key::ChangeRow,
        zh: "  第{}轮  {} {} {} → {}",
        en: "  round {}  {} {} {} -> {}",
    },
    Entry {
        key: Key::HelpKeysHeading,
        zh: "快捷键",
        en: "Keys",
    },
    Entry {
        key: Key::HelpLineEnter,
        zh: "  Enter        开始探测（编辑态下表示确认输入）",
        en: "  Enter        start scanning (confirms input in edit mode)",
    },
    Entry {
        key: Key::HelpLineI,
        zh: "  i            进入/退出目标编辑",
        en: "  i            enter/leave target editing",
    },
    Entry {
        key: Key::HelpLineEsc,
        zh: "  Esc          取消探测 / 关闭本窗口",
        en: "  Esc          cancel the scan / close this window",
    },
    Entry {
        key: Key::HelpLineGt,
        zh: "  g / t        切换 矩阵 / 表格 视图",
        en: "  g / t        switch matrix / table view",
    },
    Entry {
        key: Key::HelpLineF,
        zh: "  f            过滤不在线目标",
        en: "  f            hide offline targets",
    },
    Entry {
        key: Key::HelpLineSS,
        zh: "  s / S        导出 CSV / TXT",
        en: "  s / S        export CSV / TXT",
    },
    Entry {
        key: Key::HelpLineJH,
        zh: "  j / H        导出 JSON / HTML 报告",
        en: "  j / H        export JSON / HTML reports",
    },
    Entry {
        key: Key::HelpLineScroll,
        zh: "  ↑ ↓ PgUp PgDn Home End  表格滚动",
        en: "  ↑ ↓ PgUp PgDn Home End  scroll the table",
    },
    Entry {
        key: Key::HeadingM13,
        zh: "资产识别（M13）",
        en: "Asset discovery (M13)",
    },
    Entry {
        key: Key::HelpM13F,
        zh: "  F            端口指纹（选中主机；无选中则全部在线主机）",
        en: "  F            port fingerprint (selected host; all online hosts when none is selected)",
    },
    Entry {
        key: Key::HelpM13NoteA,
        zh: "               端口扫描与在线判定是两回事：端口全关不影响「在线」；",
        en: "               port scanning and the online verdict are independent: closed ports never make a host offline;",
    },
    Entry {
        key: Key::HelpM13NoteB,
        zh: "               横幅只读不写；设备类型是推断，识别不出就留空",
        en: "               banners are read-only; the device type is a guess and stays empty when unknown",
    },
    Entry {
        key: Key::HeadingM14,
        zh: "历史与趋势（M14）",
        en: "History & trend (M14)",
    },
    Entry {
        key: Key::HelpM14U,
        zh: "  U            查看可用率趋势（每台主机一条字符柱状曲线）",
        en: "  U            uptime trend (one character-bar curve per host)",
    },
    Entry {
        key: Key::HelpM14NoteA,
        zh: "               数值与曲线都来自本进程累积的轮次；跨会话的历史",
        en: "               values and curves come from rounds accumulated in this process; for cross-session history",
    },
    Entry {
        key: Key::HelpM14NoteB,
        zh: "               请用 cping-cli --history（两者共用同一套时序核心）",
        en: "               use cping-cli --history (both share the same time-series core)",
    },
    Entry {
        key: Key::HelpM14NoteC,
        zh: "               曲线里的 · 是「该轮没测到这台主机」，不是「离线」",
        en: "               the · in the curve means \"not measured this round\", not \"offline\"",
    },
    Entry {
        key: Key::HeadingM15,
        zh: "告警（M15）",
        en: "Alerts (M15)",
    },
    Entry {
        key: Key::HelpM15A,
        zh: "  A            查看告警记录；**首次按 A 会就地启用告警**（默认规则）",
        en: "  A            view alerts; **the first press of A enables them in place** (default rules)",
    },
    Entry {
        key: Key::HelpM15B,
        zh: "               默认：新增主机 / 离线 / 延迟>200ms / 可用率<90%",
        en: "               defaults: new host / offline / latency > 200 ms / uptime < 90%",
    },
    Entry {
        key: Key::HelpM15Mute,
        zh: "  !            静音开关：规则照常求值、冷却照常推进，只是不通知",
        en: "  !            mute toggle: rules still evaluate and cooldowns still advance, only notifications stop",
    },
    Entry {
        key: Key::HelpM15MuteB,
        zh: "               因此解除静音**不会**补报一堆积压的告警",
        en: "               so unmuting does **not** flush a backlog of alerts",
    },
    Entry {
        key: Key::HelpM15Note,
        zh: "  说明         第一轮不发「新增主机」——否则扫个 /24 会一次刷出 255 条；",
        en: "  note         the first round does not emit \"new host\"; a /24 scan would otherwise flood 255 entries;",
    },
    Entry {
        key: Key::HelpM15NoteB,
        zh: "               阈值用严格不等号，恰好等于阈值不告警；通知失败一定可见",
        en: "               thresholds use strict inequality, so a value exactly at the threshold does not alert; delivery failures are always visible",
    },
    Entry {
        key: Key::HeadingM9,
        zh: "目标与探测方式（M9）",
        en: "Targets & probe mode (M9)",
    },
    Entry {
        key: Key::HelpM9Input,
        zh: "  输入支持     192.168.1（网段）/ 单主机 / CIDR（192.168.1.0/24）/ 多段",
        en: "  input        192.168.1 (network) / single host / CIDR (192.168.1.0/24) / multiple",
    },
    Entry {
        key: Key::HelpM9InputB,
        zh: "               多段用逗号或空格分隔，重复目标只算一次",
        en: "               separate segments with commas or spaces; duplicate targets count once",
    },
    Entry {
        key: Key::HelpM9T,
        zh: "  T            切换 ICMP ↔ TCP（TCP 结果标注为 TCP-OPEN）",
        en: "  T            switch ICMP ↔ TCP (TCP results are labelled TCP-OPEN)",
    },
    Entry {
        key: Key::HelpM9P,
        zh: "  p            轮换常用 TCP 端口（22/23/80/135/139/443/445/3389）",
        en: "  p            cycle common TCP ports (22/23/80/135/139/443/445/3389)",
    },
    Entry {
        key: Key::HeadingM12,
        zh: "探测后端（M12）",
        en: "Probe backends (M12)",
    },
    Entry {
        key: Key::HelpM12Current,
        zh: "  当前          {}",
        en: "  current       {}",
    },
    Entry {
        key: Key::HelpM12Options,
        zh: "  可选          icmp4（ICMP 方式）/ tcp（TCP 方式）",
        en: "  available     icmp4 (ICMP mode) / tcp (TCP mode)",
    },
    Entry {
        key: Key::HelpM12V6,
        zh: "  IPv6 目标     必须走 TCP：按 T 切到 TCP 方式，或 --mode tcp --port N",
        en: "  IPv6 target   must use TCP: press T, or pass --mode tcp --port N",
    },
    Entry {
        key: Key::HelpM12Icmp6,
        zh: "  ICMPv6        尚未实装——登记为「不可用」，不会静默超时",
        en: "  ICMPv6        not implemented yet; registered as unavailable, never a silent timeout",
    },
    Entry {
        key: Key::HelpM12Local,
        zh: "  本机能力      {}",
        en: "  local caps    {}",
    },
    Entry {
        key: Key::HeadingM8,
        zh: "持续监控（M8）",
        en: "Continuous monitoring (M8)",
    },
    Entry {
        key: Key::HelpM8M,
        zh: "  M            开/关自动重扫（默认 30 秒一轮）",
        en: "  M            enable/disable auto-rescan (30 s per round by default)",
    },
    Entry {
        key: Key::HelpM8Bracket,
        zh: "  [ / ]        缩短 / 延长重扫间隔",
        en: "  [ / ]        shorten / lengthen the rescan interval",
    },
    Entry {
        key: Key::HelpM8E,
        zh: "  e            查看变化事件流（上线/离线/延迟档位变化）",
        en: "  e            change feed (up / down / latency tier changes)",
    },
    Entry {
        key: Key::HeadingM7,
        zh: "目标信息（M7）",
        en: "Target info (M7)",
    },
    Entry {
        key: Key::HelpM7M,
        zh: "  m            读邻居表回填 MAC / 厂商",
        en: "  m            fill MAC / vendor from the neighbor table",
    },
    Entry {
        key: Key::HelpM7N,
        zh: "  n            反查当前选中行的主机名",
        en: "  n            reverse-lookup the hostname of the selected row",
    },
    Entry {
        key: Key::HelpM7Wl,
        zh: "  w / l        保存快照 / 从快照恢复",
        en: "  w / l        save snapshot / restore from snapshot",
    },
    Entry {
        key: Key::HelpM7Note,
        zh: "  说明         MAC、厂商来自本机邻居表与 IEEE 注册数据；",
        en: "  note         MAC and vendor come from the local neighbor table and IEEE registry data;",
    },
    Entry {
        key: Key::HelpM7NoteB,
        zh: "               主机名走 DNS 反查，查不到就留空，不猜。",
        en: "               hostnames come from reverse DNS; when not found they stay empty, never guessed.",
    },
    Entry {
        key: Key::HelpLineHelpHint,
        zh: "  ? 或 F1      本帮助",
        en: "  ? or F1      this help",
    },
    Entry {
        key: Key::HelpLineQuit,
        zh: "  q            退出（运行中需再按一次确认）",
        en: "  q            quit (press again to confirm while running)",
    },
    Entry {
        key: Key::HelpLineCtrlC,
        zh: "  Ctrl+C       强制退出（自动恢复终端）",
        en: "  Ctrl+C       force quit (restores the terminal)",
    },
    Entry {
        key: Key::HelpSymbols,
        zh: "  符号  {} 低延迟  {} 中延迟  {} 高延迟  {} 无回应  {} 错误  {} 未探测",
        en: "  symbols  {} low  {} mid  {} high  {} no reply  {} error  {} not probed",
    },
    Entry {
        key: Key::CliHelpTop,
        zh: "cping-tui —— C网群PING工具（终端版）\n\n用法:\n  cping-tui [选项]",
        en: "cping-tui -- CPing (terminal UI)\n\nUsage:\n  cping-tui [options]",
    },
    Entry {
        key: Key::CliHelpOptions,
        zh: "选项:\n  --prefix <前三段>   初始网段前缀，例如 192.168.1\n  --no-color          禁用颜色（无颜色终端下自动降级为符号标识）\n  --hide-offline      启动时即隐藏不在线目标\n  --view <matrix|table>  初始视图\n  --mock              使用模拟探测数据（演示 / UI 调试，不发真实报文）\n  --snapshot-dir <目录>  结果快照的存放目录（默认当前目录）\n  --config <文件>     启动时加载的配置文件（默认当前目录的 cping.toml；模板只填空缺）\n  --alert             启动即启用告警（也可在界面里按 A 就地启用）\n  --alert-kinds <列表>  启用哪几类：appeared,disappeared,latency,uptime\n  --alert-latency <ms>  延迟阈值（默认 200；严格大于才报）\n  --alert-uptime <百分比> 可用率阈值（默认 90；严格小于才报）\n  --alert-cooldown <秒> 同一主机同一类告警的冷却窗口（默认 300）\n  --mute-alerts       启动即静音（规则照常求值，只是不通知）\n  --max-pps <n>       全局速率上限（探测包/秒，默认 500；写 0 = 不限速）\n  --max-targets <n>   目标数上限（只能比结构上限 65536 更严）\n  --allow-reserved    放行组播 / 保留段等特殊网段（默认拒绝）\n  --lang <zh|en>      界面语言（默认按环境推断：CPING_LANG → LC_ALL → LC_MESSAGES → LANG）\n  --help              显示本帮助",
        en: "Options:\n  --prefix <a.b.c>    initial network prefix, e.g. 192.168.1\n  --no-color          disable colour (terminals without colour fall back to symbols)\n  --hide-offline      start with offline targets hidden\n  --view <matrix|table>  initial view\n  --mock              use simulated probe data (demo / UI debugging, sends no packets)\n  --snapshot-dir <dir>    directory for result snapshots (default: current directory)\n  --config <file>     config file loaded at startup (default: cping.toml in the current directory; profiles only fill gaps)\n  --alert             enable alerts at startup (or press A in the UI)\n  --alert-kinds <list>    which kinds to enable: appeared,disappeared,latency,uptime\n  --alert-latency <ms>    latency threshold (default 200; strictly greater than)\n  --alert-uptime <pct>    uptime threshold (default 90; strictly less than)\n  --alert-cooldown <sec>  per-host, per-kind alert cooldown (default 300)\n  --mute-alerts       start muted (rules still evaluate, notifications stop)\n  --max-pps <n>       global rate limit (probe packets/s, default 500; 0 = unlimited)\n  --max-targets <n>   target limit (can only be tighter than the structural limit 65536)\n  --allow-reserved    allow multicast / reserved ranges (denied by default)\n  --lang <zh|en>      UI language (default: inferred from CPING_LANG -> LC_ALL -> LC_MESSAGES -> LANG)\n  --help              show this help",
    },
    Entry {
        key: Key::CliHelpBackend,
        zh: "探测后端:\n  默认使用当前平台的原生 ICMP。若平台拿不到权限（Android 普通应用常见），\n  状态栏会显示相应错误，而不会被当成「探测成功」。",
        en: "Probe backend:\n  Native ICMP is used by default. Where the platform cannot grant permission\n  (common for ordinary Android apps), the status bar shows the error instead of\n  treating the probe as a success.",
    },
    Entry {
        key: Key::CliHelpTargets,
        zh: "目标信息（M7）:\n  扫描结束后会自动读取本机邻居表（ARP）回填 MAC，并据 IEEE 注册数据查厂商；\n  主机名走 DNS 反查，只在按 n 时对当前行发起，查不到就留空——不猜。",
        en: "Target info (M7):\n  After a scan the local neighbor table (ARP) is read to fill in MACs and vendors\n  are looked up in the IEEE registry. Hostnames use reverse DNS and are only\n  requested for the current row when n is pressed; if not found they stay empty.",
    },
    Entry {
        key: Key::CliHelpSnapshot,
        zh: "结果快照:\n  每轮扫描结束后会把结果写入 {} （默认当前目录），\n  下次启动若发现该文件会先问是否恢复，默认**不**恢复。",
        en: "Result snapshot:\n  After each scan the results are written to {} (default: current directory).\n  On the next start, if that file is found you are asked first; the default is\n  **not** to restore it.",
    },
    Entry {
        key: Key::CliHelpWarning,
        zh: "仅对你有权探测的网络使用本工具。并发与目标数量都有限制。",
        en: "Only use this tool on networks you are authorised to probe. Concurrency and target counts are limited.",
    },
    Entry {
        key: Key::CliHelpKeys,
        zh: "交互快捷键:\n  Enter   开始探测（编辑输入框时表示确认）\n  i       进入/退出前缀编辑\n  Esc     取消当前探测 / 关闭弹窗\n  g       矩阵视图\n  t       表格视图\n  f       过滤不在线目标\n  s       导出 CSV\n  S       导出 TXT\n  j       导出 JSON\n  H       导出 HTML\n  m       读邻居表回填 MAC / 厂商\n  n       反查当前行的主机名\n  F       端口指纹\n  U       可用率趋势\n  A       告警记录（首次按即启用告警）\n  !       告警静音开关\n  P       选择扫描模板（配置里的 [[profiles]]）\n  G       安全策略设置（限速 / 目标数上限 / 保留网段）\n  w       保存快照\n  l       从快照恢复\n  ↑/↓ PgUp/PgDn  表格滚动\n  ?或F1   帮助\n  q       退出（运行中需再按一次确认）\n  Ctrl+C  退出（退出前恢复终端）",
        en: "Interaction keys:\n  Enter   start scanning (confirms input while editing)\n  i       enter/leave prefix editing\n  Esc     cancel the current scan / close a popup\n  g       matrix view\n  t       table view\n  f       hide offline targets\n  s       export CSV\n  S       export TXT\n  j       export JSON\n  H       export HTML\n  m       fill MAC / vendor from the neighbor table\n  n       reverse-lookup the hostname of the current row\n  F       port fingerprint\n  U       uptime trend\n  A       alerts (the first press enables them)\n  !       alert mute toggle\n  P       pick a scan profile ([[profiles]] in the config)\n  G       safety policy (rate limit / target cap / reserved ranges)\n  w       save snapshot\n  l       restore from snapshot\n  ↑/↓ PgUp/PgDn  scroll the table\n  ? or F1 help\n  q       quit (press again to confirm while running)\n  Ctrl+C  quit (restores the terminal first)",
    },
    Entry {
        key: Key::ArgIntExpected,
        zh: "{} 需要一个非负整数，得到 {}",
        en: "{} expects a non-negative integer, got {}",
    },
    Entry {
        key: Key::ArgMissingValue,
        zh: "{} 缺少取值",
        en: "{} is missing its value",
    },
    Entry {
        key: Key::ArgLangInvalid,
        zh: "{} 只接受 zh 或 en，得到 {}",
        en: "{} accepts only zh or en, got {}",
    },
    Entry {
        key: Key::ArgWarnSeparator,
        zh: "；",
        en: "; ",
    },
    Entry {
        key: Key::StatusAlertsEnabled,
        zh: "告警面板：已启用（默认阈值）。! 切静音，再按 A 关闭面板",
        en: "alert panel: enabled (default thresholds). ! toggles mute, A closes the panel",
    },
    Entry {
        key: Key::StatusAlertsEmpty,
        zh: "告警面板：还没有告警。跑一轮（Enter）或开监控（M）累积",
        en: "alert panel: no alerts yet. Run a round (Enter) or enable monitoring (M) to accumulate",
    },
    Entry {
        key: Key::StatusAlertsPanel,
        zh: "告警面板",
        en: "alert panel",
    },
    Entry {
        key: Key::StatusAlertsMuted,
        zh: "告警已静音：规则照常求值，只是不通知（! 解除）",
        en: "alerts muted: rules still evaluate, notifications just stop (! to unmute)",
    },
    Entry {
        key: Key::StatusAlertsUnmuted,
        zh: "告警已解除静音",
        en: "alerts unmuted",
    },
    Entry {
        key: Key::StatusNothingToExport,
        zh: "没有可导出的结果",
        en: "no results to export",
    },
    Entry {
        key: Key::StatusNoDoneResults,
        zh: "尚无已完成的结果可导出",
        en: "no finished results to export yet",
    },
    Entry {
        key: Key::StatusExported,
        zh: "已导出 {} 条结果 → {}（{} 字节）",
        en: "exported {} results -> {} ({} bytes)",
    },
    Entry {
        key: Key::StatusWriteFailed,
        zh: "写入失败：{}（目标路径 {}）",
        en: "write failed: {} (target path {})",
    },
    Entry {
        key: Key::StatusExportFailed,
        zh: "导出失败：{}",
        en: "export failed: {}",
    },
    Entry {
        key: Key::LocalIpUnknown,
        zh: "未知",
        en: "unknown",
    },
    Entry {
        key: Key::LocalIpUnknownM5,
        zh: "未知（M5 接入网卡枚举）",
        en: "unknown (adapter enumeration lands in M5)",
    },
    Entry {
        key: Key::HelpCliFlags,
        zh: "  启动参数    见 cping-tui --help（含 --lang zh|en 指定界面语言）",
        en: "  CLI flags   see cping-tui --help (including --lang zh|en for the UI language)",
    },
    Entry {
        key: Key::ProfilesTitle,
        zh: " 扫描模板 ",
        en: " Scan profiles ",
    },
    Entry {
        key: Key::ProfilesHint,
        zh: "↑ ↓ 选择 · Enter 应用 · Esc 取消",
        en: "up/down select, Enter apply, Esc cancel",
    },
    Entry {
        key: Key::ProfilesEmpty,
        zh: "这份配置里没有模板。\n可以写 [[profiles]] 段，或先用 cping-cli --dump-config 生成一份。",
        en: "this config has no profiles.\nadd [[profiles]] sections, or generate one with cping-cli --dump-config.",
    },
    Entry {
        key: Key::ProfilesNoConfig,
        zh: "未加载配置文件：{}（不存在或读取失败）",
        en: "no config file loaded: {} (missing or unreadable)",
    },
    Entry {
        key: Key::ProfilesFilePath,
        zh: "配置文件：{}",
        en: "config file: {}",
    },
    Entry {
        key: Key::ProfilesCurrentMark,
        zh: "  ← 当前",
        en: "  <- current",
    },
    Entry {
        key: Key::ProfilesCliNote,
        zh: "命令行显式给出的目标与探测方式不会被模板覆盖。",
        en: "targets and probe mode given on the command line are never overridden by a profile.",
    },
    Entry {
        key: Key::ProfileNone,
        zh: "未使用模板",
        en: "no profile",
    },
    Entry {
        key: Key::ProfileInUse,
        zh: "模板 {}",
        en: "profile {}",
    },
    Entry {
        key: Key::StatusProfileApplied,
        zh: "已应用模板 {}：{}",
        en: "profile {} applied: {}",
    },
    Entry {
        key: Key::StatusProfileBlocked,
        zh: "探测进行中，无法切换模板（先按 Esc 取消或等本轮结束）",
        en: "cannot switch profiles while scanning (press Esc to cancel or wait for the round to end)",
    },
    Entry {
        key: Key::StatusConfigUnreadable,
        zh: "配置文件无法读取：{}",
        en: "config file is unreadable: {}",
    },
    Entry {
        key: Key::StatusConfigNotes,
        zh: "配置提示：{}",
        en: "config note: {}",
    },
    Entry {
        key: Key::PolicyTitle,
        zh: " 安全策略（M17） ",
        en: " Safety policy (M17) ",
    },
    Entry {
        key: Key::PolicyHint,
        zh: "↑ ↓ 选字段 · 数字键 / Backspace 改数值 · + - 微调 · 空格 切换开关 · Enter 应用 · Esc 取消",
        en: "up/down pick a field, digits/Backspace edit, +/- nudge, space toggles, Enter applies, Esc cancels",
    },
    Entry {
        key: Key::PolicyLinesCurrent,
        zh: "当前策略：{}",
        en: "current policy: {}",
    },
    Entry {
        key: Key::PolicyLinesDefaults,
        zh: "默认：{} pps 限速，目标数上限 {}，默认拒绝组播 / 保留网段。\n上限：速率 ≤ {} pps（0 表示不限速），目标数 ≤ {}。",
        en: "defaults: {} pps rate limit, {} target cap, reserved ranges denied.\nlimits: rate <= {} pps (0 = unlimited), targets <= {}.",
    },
    Entry {
        key: Key::PolicyFieldPps,
        zh: "速率上限（pps）",
        en: "rate limit (pps)",
    },
    Entry {
        key: Key::PolicyFieldTargets,
        zh: "目标数上限",
        en: "target cap",
    },
    Entry {
        key: Key::PolicyFieldReserved,
        zh: "允许保留网段",
        en: "allow reserved ranges",
    },
    Entry {
        key: Key::PolicyFieldLine,
        zh: "{} {}：{}",
        en: "{} {}: {}",
    },
    Entry {
        key: Key::PolicyOn,
        zh: "开",
        en: "on",
    },
    Entry {
        key: Key::PolicyOff,
        zh: "关",
        en: "off",
    },
    Entry {
        key: Key::PolicyWarnReserved,
        zh: "! 已放行组播 / 保留网段：程序会去探测通常拒绝的地址（组播、文档网段等），请仅在确知后果时使用。",
        en: "! reserved/multicast ranges are allowed: the tool will probe addresses it normally refuses (multicast, documentation ranges, ...); enable only when you know the consequences.",
    },
    Entry {
        key: Key::StatusPolicyApplied,
        zh: "安全策略已应用：{}",
        en: "scan policy applied: {}",
    },
    Entry {
        key: Key::HelpM16P,
        zh: "  P            选择扫描模板（配置里的 [[profiles]]），回车应用",
        en: "  P            pick a scan profile ([[profiles]] in the config), Enter applies",
    },
    Entry {
        key: Key::HelpM17G,
        zh: "  G            安全策略设置（限速 / 目标数上限 / 保留网段）",
        en: "  G            safety policy (rate limit / target cap / reserved ranges)",
    },
];

/// 取某个键在指定语言下的模板。
pub fn text(lang: Lang, key: Key) -> &'static str {
    let idx = ordinal(key);
    // 上界用 `KEY_COUNT` 而不是直接 `TABLE.get`：`KEY_COUNT` 是「字典规模」的
    // 唯一声明处，`table_covers_every_key` 拿它做双射断言。这里再用一次，
    // 是为了让「非测试构建」也不会把这两样东西判成死代码。
    if idx < KEY_COUNT {
        if let Some(e) = TABLE.get(idx) {
            // 序号 → 条目必须对得上；错位说明有人复制粘贴时改坏了 `TABLE`。
            // 这是**调试期**的断言（发布版零成本），真正的把关在
            // `table_covers_every_key` 里。
            debug_assert_eq!(e.key, key, "序号 {idx} 指向的条目不是 {key:?}");
            return match lang {
                Lang::Zh => e.zh,
                Lang::En => e.en,
            };
        }
    }
    // `TABLE` 与 `ordinal` 的一致性由 `table_covers_every_key` 守着；真跑到这里
    // 说明有人绕过了那道测试，此时**返回中文兜底**也好过 panic——
    // 界面显示一句中文，比整个程序崩掉好（与核心层同一取舍）。
    match lang {
        Lang::Zh => "（未登记的消息键）",
        Lang::En => "(unregistered message key)",
    }
}

/// 按 TUI 的键渲染。占位符算法与核心层共用，避免两份实现漂移。
pub fn render(lang: Lang, key: Key, args: &[&str]) -> String {
    cping_core::i18n::render_template(text(lang, key), args)
}

// ---------------------------------------------------------------------------
// 枚举展示名：**一律转发核心层**
// ---------------------------------------------------------------------------
//
// 这里原来有一批「枚举 → TUI 键」的桥接函数（分类名、探测状态名、变化类型名、
// 告警类别名、后端名与能力摘要）。它们的中文与核心层一字不差，英文则各写各的
// ——两份翻译迟早漂移，而漂移了没人会发现（两边测试只管自己那半张表）。
//
// 核心层如今提供了 `label_in(lang)` / `status_label_in(status, lang)` /
// `describe_in(lang)` / `summary_in(lang)`，所以 TUI 侧一律直接调用：
//
// - 分类名：`cping_core::ResultClass::label_in`
// - 探测状态名（含 TCP-OPEN 那套自解释的词）：`cping_core::ProbeMode::status_label_in`
//   —— **不再有 `TCP_OPEN` 常量**：它现在是核心字典里的 `LabelModeTcpOpen`，
//   两种语言下都是 `TCP-OPEN`（ASCII 记号，不是可以翻译的句子）。
// - 变化类型名：`cping_core::ChangeKind::label_in`
// - 告警类别名：`cping_core::AlertKind::label_in`
// - 后端名与能力行：`BackendId::label_in` / `BackendCaps::describe_in` /
//   `BackendRegistry::summary_in`
// - 目标摘要：`TargetSpec::summary_in`
//
// 好处不只是少了两份翻译：核心加了新枚举变体时，界面**编译期**就会缺分支，
// 而不是在界面上悄悄冒出一句没翻的话。

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
        // `text()` 是按下标取 `TABLE[ordinal(key)]` 的：序号与顺序一旦错位，
        // 界面会**静默**显示成邻居那条消息（不 panic、不报错），是这里最难查的
        // 一类坏法。所以顺序必须逐条钉死，而不是只验「双射」。
        for (i, e) in TABLE.iter().enumerate() {
            assert_eq!(
                ordinal(e.key),
                i,
                "TABLE 第 {i} 条是 {:?}，但它的序号是 {}——顺序与 ordinal 不一致",
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
        // 少给参数：`{}` 必须**留在原地**，而不是变成空串——
        // 界面上看见一个 `{}` 比看见一句少了关键数字的通顺话更容易定位问题。
        let s = render(Lang::Zh, Key::ProfilesNoConfig, &[]);
        assert_eq!(s, "未加载配置文件：{}（不存在或读取失败）");
        let s = render(Lang::Zh, Key::StatusProfileApplied, &["lan"]);
        assert_eq!(s, "已应用模板 lan：{}");
        let s = render(Lang::En, Key::StatusProfileApplied, &["lan"]);
        assert_eq!(s, "profile lan applied: {}");
        // 多给参数则忽略，不 panic。注意要比槽位**多**给：这条模板只有 1 个
        // `{}`，所以这里给 2 个，才能验证「多出来的被丢掉」。
        let s = render(Lang::Zh, Key::ProfilesNoConfig, &["第一个", "第二个"]);
        assert_eq!(s, "未加载配置文件：第一个（不存在或读取失败）");
        // 正常替换仍按顺序进行。
        let s = render(
            Lang::Zh,
            Key::PolicyFieldLine,
            &[">", "速率上限（pps）", "200"],
        );
        assert_eq!(s, "> 速率上限（pps）：200");
    }

    #[test]
    fn text_is_never_empty_for_any_lang() {
        for lang in Lang::ALL {
            for e in TABLE {
                assert!(!text(lang, e.key).is_empty());
            }
        }
    }
}
