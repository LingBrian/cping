//! 应用状态机。所有探测与统计口径都来自 `cping-core`，这里**不重新实现**。

use std::io;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use cping_core::classify::{classify_status, ResultClass, Thresholds};
use cping_core::config::{ColorConfig, ProbeConfig, ViewMode};
use cping_core::enrich::{enrich_with_mac_and_vendor, EnrichReport};
use cping_core::export::{export_to_path, ExportMeta};
use cping_core::i18n::Lang;
use cping_core::monitor::{Change, ChangeKind, MonitorConfig, MonitorState};
use cping_core::scheduler::{ProbeScheduler, SchedulerHandle};
use cping_core::snapshot::{load_from_path, save_to_path, Snapshot};
use cping_core::spec::{parse_spec, spec_from_prefix, ProbeMode, TargetSpec};
use cping_core::target::{matrix_slots, parse_prefix, TargetRange, MATRIX_CELLS};
use cping_core::{ProbeEvent, ProbeResult, ScanStats};

use ratatui::backend::Backend;
use ratatui::crossterm::event::KeyEvent;
use ratatui::Terminal;

use crate::events::{next_event, UiEvent};
use crate::i18n::{self, Key};
use crate::input::{map_key, Action};
use crate::ui;

/// 自动落盘快照的文件名（与 GUI 端同名，便于人工排查）。
pub const AUTOSAVE_FILE: &str = "cping-autosave.snap";
/// 「已处理过」的指纹文件名。同一份结果只问一次。
pub const HANDLED_FILE: &str = "cping-autosave.handled";

/// M17：首次运行向用户告知「仅用于获授权网络」。
///
/// 与 CLI 共用 `cping-core::policy::consent` 的同一份状态与同一段文案：
/// 三端提示不一致，用户会以为哪一份是过期的。
///
/// M18：语言必须由调用方传入。这个函数**独立于 [`App`] 实例**（在进入备用屏幕
/// 之前就被 `main` 调用，那时 App 还不存在），所以它不能去读 `self.lang`；
/// 语言只能从命令行选项一路带过来。
pub fn print_authorization_notice(lang: Lang) {
    if !cping_core::policy::consent::load().needs_notice() {
        return;
    }
    println!("{}", cping_core::policy::consent::notice_text_in(lang));
    println!();
    let _ = cping_core::policy::consent::acknowledge(cping_core::unix_millis_now());
}

/// 命令行选项（仅作为初始值，不替代交互界面）。
#[derive(Debug, Clone)]
pub struct Options {
    /// 目标规格（M9 起支持 `192.168.1` / `192.168.1.0/24` / 单主机 / 多网段）。
    pub prefix: String,
    pub no_color: bool,
    pub hide_offline: bool,
    pub view: ViewMode,
    pub help: bool,
    /// 用模拟后端（不做真实探测）；仅用于演示与 UI 调试。
    pub mock: bool,
    /// 探测方式（M9）。
    pub mode: ProbeMode,
    /// 启动即开启持续监控（秒）；`None` 表示不自动重扫（M8）。
    pub watch_seconds: Option<u64>,
    /// 快照文件所在目录；`None` 表示当前工作目录（单元测试会改到临时目录，
    /// 避免测试往仓库里写文件）。
    pub snapshot_dir: Option<PathBuf>,
    /// M15：启动即启用告警（默认关闭，按 A 可在界面里随时打开）。
    pub alert: bool,
    /// M15：告警配置（阈值 / 冷却 / 启用哪几类）。
    pub alert_config: cping_core::AlertConfig,
    /// M17：安全策略（限速 / 目标上限 / 保留网段守卫）。
    pub policy: cping_core::ScanPolicy,
    /// M17：参数解析过程中的问题。
    ///
    /// TUI 是交互式全屏程序，为一个参数拼错就拒绝启动体验很差；但**静默**
    /// 忽略用户显式给的参数是本项目最不能接受的一类问题。折中：照常启动，
    /// 把问题写进状态栏、摆在用户眼前。
    ///
    /// M18：这里存的是**已按当前语言渲染好**的句子（语言在解析结束、`--lang`
    /// 尘埃落定之后才确定，所以不能边解析边拼串）。
    pub arg_warnings: Vec<String>,
    /// M18：界面语言。`--lang` 显式给定时用它，否则用 [`Lang::from_env`]。
    pub lang: Lang,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            prefix: "192.168.1".into(),
            no_color: false,
            hide_offline: false,
            view: ViewMode::Matrix,
            help: false,
            mock: false,
            mode: cping_core::platform_default_mode(),
            watch_seconds: None,
            snapshot_dir: None,
            alert: false,
            alert_config: cping_core::AlertConfig::default(),
            policy: cping_core::ScanPolicy::default(),
            arg_warnings: Vec::new(),
            // 测试与 `Options::default()` 走中文；命令行路径会在 `from_args`
            // 里把它换成 `Lang::from_env()`（见那里的注释）。
            lang: Lang::default(),
        }
    }
}

impl Options {
    pub fn from_args<I: Iterator<Item = String>>(args: I) -> Self {
        let mut opts = Options {
            no_color: std::env::var_os("NO_COLOR").is_some(),
            // M18：不给 `--lang` 时按 POSIX 约定从环境推断（CPING_LANG →
            // LC_ALL → LC_MESSAGES → LANG），兜底中文。显式 `--lang` 会覆盖它。
            lang: Lang::from_env(),
            ..Options::default()
        };
        // 解析过程中先把告警/语言的问题**攒成键 + 参数**，等 `--lang` 定了再渲染：
        // 否则「--lang 写在 --max-pps 后面」会让前面的告警用错语言。
        let mut warnings: Vec<(Key, Vec<String>)> = Vec::new();
        // M15：告警开关先收集，循环结束后一次性折成配置
        let mut alert_kinds: Vec<cping_core::AlertKind> = Vec::new();
        let mut alert_latency: Option<u32> = None;
        let mut alert_uptime: Option<u32> = None;
        let mut alert_cooldown: Option<u64> = None;
        let mut mute_alerts = false;
        let mut it = args.peekable();
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--help" | "-h" => opts.help = true,
                "--no-color" => opts.no_color = true,
                "--hide-offline" => opts.hide_offline = true,
                "--mock" => opts.mock = true,
                "--prefix" | "--target" => {
                    if let Some(v) = it.next() {
                        opts.prefix = v;
                    }
                }
                "--mode" => {
                    if let Some(v) = it.next() {
                        opts.mode = if v.eq_ignore_ascii_case("tcp") {
                            ProbeMode::Tcp {
                                port: opts.mode.tcp_port().unwrap_or(80),
                            }
                        } else {
                            ProbeMode::Icmp
                        };
                    }
                }
                "--port" => {
                    if let Some(v) = it.next() {
                        if let Ok(port) = v.parse::<u16>() {
                            if port > 0 {
                                opts.mode = ProbeMode::Tcp { port };
                            }
                        }
                    }
                }
                "--watch" => {
                    if let Some(v) = it.next() {
                        if let Ok(secs) = v.parse::<u64>() {
                            if secs > 0 {
                                opts.watch_seconds = Some(secs);
                            }
                        }
                    }
                }
                "--snapshot-dir" => {
                    if let Some(v) = it.next() {
                        opts.snapshot_dir = Some(PathBuf::from(v));
                    }
                }
                // M17：安全策略。取值不合法要**说出来**（写进状态栏），
                // 不能像没看见一样按默认值跑。
                "--max-pps" => match it.next() {
                    Some(v) => match v.parse::<u32>() {
                        Ok(n) => opts.policy.max_pps = n,
                        Err(_) => warnings.push((
                            Key::ArgIntExpected,
                            vec!["--max-pps".into(), format!("{v:?}")],
                        )),
                    },
                    None => warnings.push((Key::ArgMissingValue, vec!["--max-pps".into()])),
                },
                "--max-targets" => match it.next() {
                    Some(v) => match v.parse::<usize>() {
                        Ok(n) => opts.policy.max_targets = n,
                        Err(_) => warnings.push((
                            Key::ArgIntExpected,
                            vec!["--max-targets".into(), format!("{v:?}")],
                        )),
                    },
                    None => warnings.push((Key::ArgMissingValue, vec!["--max-targets".into()])),
                },
                // M18：接口语言。取值非法**必须报出来**，不能静默忽略用户
                // 显式指定的语言——「我明明写了 --lang en，界面还是中文」
                // 是最难自查的一类问题。处理方式与上面的策略参数一致：
                // 照常启动，把问题摆到状态栏。
                "--lang" => match it.next() {
                    Some(v) => match Lang::parse(&v) {
                        Some(l) => opts.lang = l,
                        None => warnings
                            .push((Key::ArgLangInvalid, vec!["--lang".into(), format!("{v:?}")])),
                    },
                    None => warnings.push((Key::ArgMissingValue, vec!["--lang".into()])),
                },
                "--allow-reserved" => opts.policy.allow_reserved = true,
                "--view" => {
                    if let Some(v) = it.next() {
                        opts.view = if v == "table" {
                            ViewMode::Table
                        } else {
                            ViewMode::Matrix
                        };
                    }
                }
                "--alert" => opts.alert = true,
                // 下面几个都**同时开启告警**：给了阈值却什么也不做是最难查的一类
                // 配置问题，宁可当场生效也不静默吞掉。
                "--alert-kinds" => {
                    if let Some(v) = it.next() {
                        alert_kinds = parse_alert_kinds(&v);
                        opts.alert = true;
                    }
                }
                "--alert-latency" => {
                    if let Some(v) = it.next() {
                        if let Ok(ms) = v.parse::<u32>() {
                            alert_latency = Some(ms);
                            opts.alert = true;
                        }
                    }
                }
                "--alert-uptime" => {
                    if let Some(v) = it.next() {
                        if let Ok(pct) = v.parse::<u32>() {
                            if pct <= 100 {
                                alert_uptime = Some(pct);
                                opts.alert = true;
                            }
                        }
                    }
                }
                "--alert-cooldown" => {
                    if let Some(v) = it.next() {
                        if let Ok(secs) = v.parse::<u64>() {
                            alert_cooldown = Some(secs);
                            opts.alert = true;
                        }
                    }
                }
                "--mute-alerts" => {
                    mute_alerts = true;
                    opts.alert = true;
                }
                _ => {}
            }
        }
        opts.alert_config = build_alert_config(
            &alert_kinds,
            alert_latency,
            alert_uptime,
            alert_cooldown,
            mute_alerts,
        );
        // 到了这里 `--lang` 已经尘埃落定，可以把攒下的告警渲染成当前语言了。
        opts.arg_warnings = warnings
            .iter()
            .map(|(key, args)| {
                let refs: Vec<&str> = args.iter().map(String::as_str).collect();
                i18n::render(opts.lang, *key, &refs)
            })
            .collect();
        opts
    }
}

/// M15：把命令行上的告警开关折成核心的 `AlertConfig`。
///
/// 只覆盖**用户显式给过**的字段——没给就留核心默认值，这样 TUI 与 CLI 的
/// 默认阈值永远是同一份（都在 `cping_core::alerts` 里定义）。
fn build_alert_config(
    kinds: &[cping_core::AlertKind],
    latency_ms: Option<u32>,
    uptime_pct: Option<u32>,
    cooldown_secs: Option<u64>,
    muted: bool,
) -> cping_core::AlertConfig {
    let mut cfg = cping_core::AlertConfig::default();
    for rule in &mut cfg.rules {
        if let Some(v) = latency_ms {
            if rule.kind == cping_core::AlertKind::LatencyAbove {
                rule.threshold = v;
            }
        }
        if let Some(v) = uptime_pct {
            if rule.kind == cping_core::AlertKind::UptimeBelow {
                rule.threshold = v;
            }
        }
        if let Some(v) = cooldown_secs {
            rule.cooldown_secs = v;
        }
    }
    if !kinds.is_empty() {
        cfg.with_kinds(kinds);
    }
    cfg.muted = muted;
    cfg
}

/// 解析 `--alert-kinds`。不认识的名字**跳过**（TUI 是交互程序，为一个拼错的
/// 名字直接退出会把用户正在看的界面弄丢）；但全部都不认识时保持默认全集，
/// 免得变成「一条告警也不报」。
fn parse_alert_kinds(text: &str) -> Vec<cping_core::AlertKind> {
    text.split([',', ' ', ';'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter_map(cping_core::AlertKind::parse_key)
        .collect()
}

/// 启动时发现的一份「上次没处理完」的快照，等用户决定是否恢复。
#[derive(Debug, Clone)]
pub struct PendingRestore {
    pub path: String,
    pub scan_time: String,
    pub prefix: String,
    pub total: usize,
    pub rows: usize,
    pub elapsed_ms: u64,
    /// 非空表示这份快照读不出来（此时只能选择丢弃）。
    pub error: Option<String>,
}

/// 一次进行中的主机名反查（放到线程里做，避免阻塞界面）。
struct HostnameJob {
    index: usize,
    ip: IpAddr,
    rx: Receiver<Option<String>>,
}

/// 一次进行中的端口指纹（M13）。同样放线程里：单主机 28 端口最坏要几秒。
struct FingerprintJob {
    /// 本次实际探测的主机数。
    hosts: usize,
    ports: usize,
    rx: Receiver<Vec<cping_core::HostFingerprint>>,
}

pub struct App {
    pub prefix: String,
    pub editing: bool,
    pub view: ViewMode,
    pub hide_offline: bool,
    pub thresholds: Thresholds,
    pub colors: ColorConfig,
    pub no_color: bool,
    pub show_help: bool,
    /// M18：界面语言。**唯一**的语言来源：所有界面文案都经 [`App::t`] /
    /// [`App::r`] 从这里取，不允许各处再自己读一遍环境变量（那样会出现
    /// 「一半界面中文、一半英文」的自相矛盾状态）。
    pub lang: Lang,

    pub results: Vec<Option<ProbeResult>>,
    pub targets: Vec<IpAddr>,
    /// 矩阵格号(0..256) → results 下标
    pub cell_slots: Vec<Option<usize>>,
    pub stats: ScanStats,
    pub elapsed_ms: u64,
    pub running: bool,
    /// 最近一次任务是否以「取消」收尾
    pub cancelled: bool,
    pub started_at: Option<Instant>,

    pub status: String,
    pub last_export: Option<String>,
    pub local_ip: String,
    pub scroll: usize,
    /// `q` 的二次确认状态
    pub quit_confirm: bool,
    /// 真正要求退出主循环
    pub quit_requested: bool,

    // ---- M7 ----
    /// 本轮扫描的开始时间（写进快照头部）。
    pub scan_time: String,
    /// 最近一次成功写出的快照路径。
    pub last_snapshot: Option<String>,
    /// 最近一次补全报告的原文，供状态栏与测试断言。
    pub enrich_note: String,
    /// 启动时发现、还未处理的快照；Some 时界面显示确认框。
    pub pending_restore: Option<PendingRestore>,
    /// 快照文件目录（None = 当前工作目录）。
    snapshot_dir: Option<PathBuf>,
    hostname_job: Option<HostnameJob>,

    // ---- M8 持续监控 ----
    /// 当前目标规格（含 CIDR / 多网段展开结果）。
    pub spec: TargetSpec,
    /// 监控状态机（轮次、变化日志、上一轮基线）。
    pub monitor: MonitorState,
    /// 自动重扫间隔（秒）；`None` = 关闭监控。
    pub watch_seconds: Option<u64>,
    /// 早于这个时刻不启动下一轮。
    next_round_at: Option<Instant>,
    /// 是否显示变化事件流面板。
    pub show_changes: bool,
    /// 当前探测方式（ICMP / TCP），切换后需要重建调度器。
    backend_mode: ProbeMode,
    /// 是否使用模拟后端（`--mock`）。
    ///
    /// 必须记住它：切换探测方式时要沿用，否则演示途中按一次 `m`
    /// 就会**静默**从模拟数据换成真实探测，看起来像「结果变了」。
    use_mock: bool,
    /// 当前实际选中的后端标识（M12），与 `scheduler` 里的后端同源。
    backend_id: cping_core::BackendId,

    // ---- M13 资产识别 ----
    /// 最近一次的端口指纹结果（独立于 `results`，不参与在线判定）。
    pub fingerprints: Vec<cping_core::HostFingerprint>,
    /// 是否显示指纹面板。
    pub show_fingerprints: bool,
    fingerprint_job: Option<FingerprintJob>,

    // ---- M14 历史与趋势 ----
    /// 跨轮次的长期时序（TUI 内**常驻内存**：它服务的是「眼前这台机器在变好还是变坏」；
    /// 需要跨会话的那份历史由 `cping-cli --history` 落盘，两者共用一个核心）。
    pub series: cping_core::TimeSeries,
    /// 是否显示趋势面板。
    pub show_trend: bool,
    /// 本轮的起始时刻（Unix 毫秒）：时序点用它打时间戳，而不是「扫描结束的现在」。
    round_at_ms: Option<u64>,

    // ---- M15 告警 ----
    /// 告警引擎；`None` = 未启用（按 `A` 就地用默认规则打开）。
    ///
    /// TUI 只在内存里保留告警日志与冷却记录——跨会话的那份状态由
    /// `cping-cli --alert-state` 落盘，两者共用同一个核心引擎。
    pub alerts: Option<cping_core::AlertEngine>,
    /// 是否显示告警面板。
    pub show_alerts: bool,

    handle: Option<SchedulerHandle>,
    scheduler: ProbeScheduler,

    // ---- M17 安全策略 ----
    /// 限速 / 目标上限 / 保留网段守卫。挂在 App 上而不是调度器上，
    /// 因为切换探测方式时会重建调度器，策略得跟着一起装上。
    policy: cping_core::ScanPolicy,
}

impl App {
    pub fn new(options: Options) -> Self {
        let snapshot_dir = options.snapshot_dir.clone();
        // M15：告警引擎先建好，`options` 里的字段随后会被逐个搬走
        let alerts = if options.alert {
            Some(cping_core::AlertEngine::new(options.alert_config.clone()))
        } else {
            None
        };
        // M17：策略与参数告警也要在字段被搬走之前取出来
        let policy = options.policy;
        let arg_warnings = options.arg_warnings.clone();
        // M18：语言在字段被搬走之前取出。`status` / `local_ip` 的初始文案
        // 也用它——否则第一帧会是中文，下一帧才变成目标语言。
        let lang = options.lang;
        let spec = parse_spec(&options.prefix, TargetRange::Standard)
            .unwrap_or_else(|_| TargetSpec::default());
        let monitor = MonitorState::new(MonitorConfig {
            enabled: options.watch_seconds.is_some(),
            ..MonitorConfig::default()
        })
        .expect("默认监控配置必然合法");
        let mode = options.mode;
        // M12：后端**与它的标识**由核心一次给出，界面不再自己按 mock/方式推一遍
        let (backend_id, backend) = cping_core::resolve_for_run(mode, options.mock);
        let mut app = Self {
            prefix: options.prefix,
            editing: false,
            view: options.view,
            hide_offline: options.hide_offline,
            thresholds: Thresholds::default(),
            colors: ColorConfig::default(),
            no_color: options.no_color,
            show_help: false,
            lang,
            results: Vec::new(),
            targets: Vec::new(),
            cell_slots: vec![None; MATRIX_CELLS],
            stats: ScanStats::default(),
            elapsed_ms: 0,
            running: false,
            cancelled: false,
            started_at: None,
            status: i18n::text(lang, Key::StatusReady).into(),
            last_export: None,
            local_ip: local_ip_label(lang),
            scroll: 0,
            quit_confirm: false,
            quit_requested: false,
            scan_time: String::new(),
            last_snapshot: None,
            enrich_note: String::new(),
            pending_restore: None,
            snapshot_dir,
            hostname_job: None,
            spec,
            monitor,
            watch_seconds: options.watch_seconds,
            next_round_at: None,
            show_changes: false,
            backend_mode: mode,
            use_mock: options.mock,
            backend_id,
            fingerprints: Vec::new(),
            show_fingerprints: false,
            fingerprint_job: None,
            series: cping_core::TimeSeries::new(),
            show_trend: false,
            round_at_ms: None,
            alerts,
            show_alerts: false,
            handle: None,
            // M5 起默认使用**平台原生 ICMP**；`--mock` 可切回模拟后端做演示。
            // M12：后端选择（含 mock 分支）统一走核心注册表，三端共用一处。
            scheduler: ProbeScheduler::new(backend),
            policy,
        };
        // M17：把安全策略装上调度器。策略来自命令行，不合法时写进状态栏——
        // 界面上看得见，而不是静默按默认值跑。
        app.apply_policy();
        if !arg_warnings.is_empty() {
            // 多条告警用「；」拼起来；这个分隔符也随语言变化（英文里用全角分号
            // 会显得像乱码），所以它同样从字典取。
            let sep = i18n::text(lang, Key::ArgWarnSeparator);
            app.status = arg_warnings.join(sep);
        }
        app.check_pending_snapshot();
        app
    }

    /// 取一条不带参数的消息（当前语言）。
    ///
    /// 之所以做成方法而不是各处直接 `i18n::text(self.lang, ...)`：
    /// 语言只有 `self.lang` 一个来源，读错来源就会得到「一半中文一半英文」。
    pub fn t(&self, key: Key) -> &'static str {
        i18n::text(self.lang, key)
    }

    /// 渲染一条带参数的消息（当前语言）。
    pub fn r(&self, key: Key, args: &[&str]) -> String {
        i18n::render(self.lang, key, args)
    }

    /// M17：把当前策略装上调度器。失败时写进状态栏（不 panic、不静默）。
    fn apply_policy(&mut self) {
        if let Err(e) = self.scheduler.set_policy(self.policy) {
            // M18：核心错误用 `localized(lang)`——错误码与话术都在核心层，
            // 界面只负责挑语言，不复刻任何判定。
            let msg = e.localized(self.lang);
            self.status = self.r(Key::StatusPolicyInvalid, &[&msg]);
        }
    }

    pub fn backend_name(&self) -> &'static str {
        self.scheduler.backend_name()
    }

    /// 当前后端的能力（M12）：帮助面板据此如实说明地址族与权限要求。
    pub fn backend_caps(&self) -> cping_core::BackendCaps {
        cping_core::BackendRegistry::new().caps_of(self.backend_id)
    }

    /// 本平台的后端能力摘要（M12）。
    ///
    /// M18：核心只提供 `summary_zh()`（它没有 `_in` 变体，而本次改动不允许动
    /// 核心层），所以这里按 `BackendCaps` 的**公开字段**自己拼一份可本地化的
    /// 摘要。拼的是"怎么显示"，不是"怎么判定"——可用性、权限、地址族全部
    /// 仍来自核心注册表，TUI 一个判断都不做。
    pub fn backend_summary(&self) -> String {
        let reg = cping_core::BackendRegistry::new();
        let mut parts: Vec<String> = Vec::new();
        let icmp = reg.caps_of(cping_core::BackendId::Icmp4);
        if icmp.available {
            let key = if icmp.needs_privilege {
                Key::BackendSummaryIcmpPrivileged
            } else {
                Key::BackendSummaryIcmp
            };
            parts.push(self.t(key).to_string());
        } else {
            parts.push(self.t(Key::BackendSummaryIcmpUnavailable).to_string());
        }
        let tcp = reg.caps_of(cping_core::BackendId::Tcp);
        if tcp.available && tcp.ipv6 {
            parts.push(self.t(Key::BackendSummaryTcp).to_string());
        }
        if !reg.caps_of(cping_core::BackendId::Icmp6).available {
            parts.push(self.t(Key::BackendSummaryIcmp6Missing).to_string());
        }
        parts.join(self.t(Key::ArgWarnSeparator))
    }

    /// 当前后端能力的一行描述，供帮助面板使用。
    ///
    /// 同上：核心的 `BackendCaps::describe()` 只会吐中文，这里按公开字段自己拼。
    /// 后端标识用 `BackendId::key()`（`icmp4` / `tcp` / `mock`）而不是中文标签——
    /// 这是稳定的机器标识，两种语言下都不该变。
    pub fn backend_caps_line(&self) -> String {
        let caps = self.backend_caps();
        let mut parts: Vec<String> = vec![caps.id.key().to_string()];
        if caps.needs_privilege {
            parts.push(self.t(Key::BackendPrivilege).to_string());
        }
        match (caps.ipv4, caps.ipv6) {
            (true, true) => parts.push(self.t(Key::BackendFamiliesBoth).to_string()),
            (true, false) => parts.push(self.t(Key::BackendFamiliesV4).to_string()),
            (false, true) => parts.push(self.t(Key::BackendFamiliesV6).to_string()),
            (false, false) => {}
        }
        if !caps.available {
            parts.push(self.t(Key::BackendUnavailable).to_string());
        }
        parts.join(" · ")
    }

    /// 目标规格的一行摘要，供状态栏使用。
    ///
    /// 核心的 `TargetSpec::summary()` 只会吐中文（没有 `_in` 变体），这里按
    /// **核心已经解析好的** `TargetSpec` 公开字段重新拼一份可本地化的文本。
    /// 段数、目标数、段标签全部来自核心，TUI 不做任何解析或判定。
    pub fn spec_summary(&self) -> String {
        let spec = &self.spec;
        if spec.targets.is_empty() {
            return self.t(Key::SpecSummaryEmpty).to_string();
        }
        if spec.segments.len() == 1 {
            let seg = &spec.segments[0];
            return self.r(
                Key::SpecSummarySingle,
                &[&seg.label, &seg.count.to_string()],
            );
        }
        self.r(
            Key::SpecSummaryMulti,
            &[
                &spec.segments.len().to_string(),
                &spec.targets.len().to_string(),
            ],
        )
    }

    /// 当前探测方式。
    pub fn mode(&self) -> ProbeMode {
        self.backend_mode
    }

    /// 切换 ICMP ↔ TCP（TCP 用当前端口，默认 80）。切换后重建调度器。
    pub fn toggle_mode(&mut self) {
        let next = if self.backend_mode.is_tcp() {
            ProbeMode::Icmp
        } else {
            ProbeMode::Tcp {
                port: self.backend_mode.tcp_port().unwrap_or(80),
            }
        };
        self.set_mode(next);
    }

    /// 在常用端口之间轮换（在 ICMP 模式下会先切到 TCP）。
    pub fn cycle_port(&mut self) {
        let ports = ProbeMode::COMMON_PORTS;
        let current = self.backend_mode.tcp_port().unwrap_or(0);
        let idx = ports.iter().position(|p| *p == current);
        let next = match idx {
            Some(i) => ports[(i + 1) % ports.len()],
            None => ports[0],
        };
        self.set_mode(ProbeMode::Tcp { port: next });
    }

    pub fn set_mode(&mut self, mode: ProbeMode) {
        if self.running {
            self.status = self.t(Key::StatusCantSwitchMode).into();
            return;
        }
        self.backend_mode = mode;
        // 沿用启动时的 mock 选择：演示途中切换方式不该把模拟换成真实探测
        let (backend_id, backend) = cping_core::resolve_for_run(mode, self.use_mock);
        self.backend_id = backend_id;
        self.scheduler = ProbeScheduler::new(backend);
        // 换方式后旧结果不再可比，重置变化基线
        self.monitor.reset_baseline();
        // `mode.describe()` 是 `ICMP` / `TCP:80` 这类协议标识，两种语言下一致，
        // 不进字典（它就是"参数名不翻译"的同类）。
        let desc = mode.describe();
        self.status = self.r(Key::StatusModeSwitched, &[&desc]);
        // M17：换了调度器就得把策略重新装上，否则切一次探测方式，限速与
        // 保留网段守卫就被悄悄丢掉了。放在最后，好让装配失败的信息
        // 盖过上面那句成功提示。
        self.apply_policy();
    }

    pub fn run<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> io::Result<()> {
        loop {
            terminal.draw(|frame| ui::draw(frame, self))?;

            match next_event()? {
                UiEvent::Key(key) => self.on_key(key),
                UiEvent::Resize => self.status = self.t(Key::StatusResized).into(),
                UiEvent::Tick => {}
            }

            self.poll_backend();
            self.poll_hostname_job();
            self.poll_fingerprint_job();
            if self.running {
                if let Some(t0) = self.started_at {
                    self.elapsed_ms = t0.elapsed().as_millis() as u64;
                }
            }
            self.tick_monitor();
            if self.quit_requested {
                break;
            }
        }
        // 退出前取消在跑的任务，避免线程悬挂；顺手再落一次盘。
        self.cancel_scan(true);
        self.autosave();
        Ok(())
    }

    fn on_key(&mut self, key: KeyEvent) {
        let action = map_key(key);

        // 变化事件流面板：任意键关闭（与帮助面板一致）
        if self.show_changes {
            self.show_changes = false;
            if !matches!(action, Action::Quit | Action::ForceQuit) {
                return;
            }
        }

        // 指纹面板：任意键关闭；F 与 Esc 都算「关闭」，符合「面板键」的直觉
        if self.show_fingerprints {
            self.show_fingerprints = false;
            if !matches!(
                action,
                Action::Quit | Action::ForceQuit | Action::Fingerprint
            ) {
                return;
            }
        }

        // 趋势面板：与上面两个面板同规矩——任意键关闭，`U` 自身也当关闭
        if self.show_trend {
            self.show_trend = false;
            if !matches!(action, Action::Quit | Action::ForceQuit | Action::ShowTrend) {
                return;
            }
        }

        // 告警面板：同上——任意键关闭，`A` 自身也当关闭
        if self.show_alerts {
            self.show_alerts = false;
            if !matches!(
                action,
                Action::Quit | Action::ForceQuit | Action::ShowAlerts
            ) {
                return;
            }
        }

        // 「发现上次结果」的确认框优先级最高：此时只有恢复/丢弃两个有效选择，
        // 免得用户在没意识到有弹窗的情况下误按其它键。
        if self.pending_restore.is_some() {
            match action {
                Action::PromptYes => self.restore_snapshot(),
                // 小写 n 平时是「查主机名」，在确认框里按它的字面意思当「不恢复」
                Action::PromptNo | Action::LookupHostname | Action::Cancel => {
                    self.dismiss_restore(true)
                }
                Action::Quit | Action::ForceQuit => self.quit_requested = true,
                _ => {}
            }
            return;
        }

        if self.editing {
            match action {
                Action::Char(c) => {
                    // M9 起目标规格可以是「网段 / 单主机 / CIDR / 多网段」，
                    // 所以除了数字与点，还要放行 / , 空格 -（写 CIDR 与多段用）
                    if c.is_ascii_digit() || matches!(c, '.' | '/' | ',' | ' ' | '-') {
                        self.prefix.push(c);
                    }
                }
                Action::Backspace => {
                    self.prefix.pop();
                }
                Action::Start | Action::ToggleEdit | Action::Cancel => {
                    self.editing = false;
                    self.status = self.t(Key::StatusInputConfirmed).into();
                }
                Action::ForceQuit => self.quit_requested = true,
                _ => {}
            }
            return;
        }

        match action {
            Action::ForceQuit => self.quit_requested = true,
            Action::Quit => {
                if self.running && !self.quit_confirm {
                    self.quit_confirm = true;
                    self.status = self.t(Key::StatusQuitConfirm).into();
                } else {
                    self.quit_requested = true;
                }
            }
            Action::Start => {
                if self.running {
                    self.status = self.t(Key::StatusAlreadyRunning).into();
                } else {
                    self.start_scan();
                }
            }
            Action::Cancel => {
                if self.show_help {
                    self.show_help = false;
                } else if self.running {
                    self.cancel_scan(false);
                } else {
                    self.status = self.t(Key::StatusNoTask).into();
                }
            }
            Action::ToggleEdit => {
                self.editing = true;
                self.status = self.t(Key::StatusEditingPrefix).into();
            }
            Action::ViewMatrix => self.view = ViewMode::Matrix,
            Action::ViewTable => self.view = ViewMode::Table,
            Action::ToggleFilter => {
                self.hide_offline = !self.hide_offline;
                self.scroll = 0;
                self.status = if self.hide_offline {
                    self.t(Key::StatusFilterOn).into()
                } else {
                    self.t(Key::StatusFilterOff).into()
                };
            }
            Action::SaveCsv => self.save("csv"),
            Action::SaveTxt => self.save("txt"),
            Action::SaveJson => self.save("json"),
            Action::SaveHtml => self.save("html"),
            Action::Enrich => self.enrich(false),
            Action::Fingerprint => self.fingerprint_targets(),
            Action::ShowTrend => {
                self.show_trend = !self.show_trend;
                if self.show_trend && self.series.is_empty() {
                    self.status = self.t(Key::StatusTrendEmpty).into();
                }
            }
            Action::ShowAlerts => self.toggle_alerts_panel(),
            Action::ToggleAlertMute => self.toggle_alert_mute(),
            Action::LookupHostname => self.lookup_selected_hostname(),
            Action::SaveSnapshot => self.save_snapshot(),
            Action::LoadSnapshot => self.load_snapshot_from_disk(),
            Action::ToggleMonitor => self.toggle_monitor(),
            Action::ShowChanges => self.show_changes = true,
            Action::IntervalDown => self.shift_interval(-1),
            Action::IntervalUp => self.shift_interval(1),
            Action::ToggleMode => self.toggle_mode(),
            Action::CyclePort => self.cycle_port(),
            Action::ToggleHelp => self.show_help = !self.show_help,
            Action::ScrollUp => self.scroll = self.scroll.saturating_sub(1),
            Action::ScrollDown => self.scroll = self.scroll.saturating_add(1),
            Action::PageUp => self.scroll = self.scroll.saturating_sub(15),
            Action::PageDown => self.scroll = self.scroll.saturating_add(15),
            Action::Top => self.scroll = 0,
            Action::Bottom => {
                let n = self.visible_rows().len();
                self.scroll = n.saturating_sub(1);
            }
            Action::Char(_)
            | Action::Backspace
            | Action::None
            | Action::PromptYes
            | Action::PromptNo => {}
        }
    }

    // ------------------------------------------------------------------
    // M13：资产识别（端口指纹）
    // ------------------------------------------------------------------

    /// 对当前选中主机做端口指纹；没有选中行时对**全部在线主机**做。
    ///
    /// 刻意放在后台线程：单主机 30 个端口、每端口 800 ms 超时，最坏情况要几秒，
    /// 卡住界面比慢几秒更糟。结果只进 `fingerprints`，**绝不动 `results`**——
    /// 端口扫描是独立的一趟探测，不得影响在线判定。
    pub fn fingerprint_targets(&mut self) {
        if self.fingerprint_job.is_some() {
            self.status = self.t(Key::StatusFingerprintBusy).into();
            return;
        }
        let cfg = cping_core::PortScanConfig::default();
        // 选中行优先；否则取全部在线主机（上限 64 台，避免一次按键打出一整网段的端口扫描）
        let selected = self
            .selected_target_index()
            .and_then(|idx| self.results.get(idx).and_then(|r| r.as_ref()))
            .filter(|r| r.status.is_online())
            .map(|r| vec![(r.ip, r.vendor.clone())]);
        let hosts: Vec<(IpAddr, Option<String>)> = match selected {
            Some(v) => v,
            None => self
                .results
                .iter()
                .flatten()
                .filter(|r| r.status.is_online())
                .take(64)
                .map(|r| (r.ip, r.vendor.clone()))
                .collect(),
        };
        if hosts.is_empty() {
            self.status = self.t(Key::StatusFingerprintNoHosts).into();
            return;
        }
        let (tx, rx) = mpsc::channel();
        let job_hosts = hosts.clone();
        let ports = cfg.ports.len();
        std::thread::spawn(move || {
            let out = cping_core::fingerprint_hosts(&job_hosts, &cfg);
            let _ = tx.send(out);
        });
        let reply = self.r(
            Key::StatusFingerprintRunning,
            &[&hosts.len().to_string(), &ports.to_string()],
        );
        self.status = reply;
        self.fingerprint_job = Some(FingerprintJob {
            hosts: hosts.len(),
            ports,
            rx,
        });
    }

    fn poll_fingerprint_job(&mut self) {
        let Some(job) = self.fingerprint_job.as_ref() else {
            return;
        };
        match job.rx.try_recv() {
            Ok(fps) => {
                let open_total: usize = fps.iter().map(|f| f.report.findings.len()).sum();
                let with_open = fps.iter().filter(|f| f.has_open_ports()).count();
                let hosts = job.hosts;
                let ports = job.ports;
                self.fingerprints = fps;
                self.fingerprint_job = None;
                self.show_fingerprints = true;
                self.status = self.r(
                    Key::StatusFingerprintDone,
                    &[
                        &hosts.to_string(),
                        &ports.to_string(),
                        &open_total.to_string(),
                        &with_open.to_string(),
                    ],
                );
                self.autosave();
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.fingerprint_job = None;
                self.status = self.t(Key::StatusFingerprintThreadFail).into();
            }
        }
    }

    /// 指纹面板的展示文本（供测试断言使用，与界面同源）。
    ///
    /// `device_summary()` 原样透传：设备类型与推断依据由**核心**给出
    /// （`DeviceGuess` 的 `kind` / `basis` 都是中文短语），界面重写它等于
    /// 复刻核心的推断逻辑——那是明令禁止的。它保持中文，是本次改动的已知边界。
    pub fn fingerprint_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        lines.push(self.t(Key::FingerprintLinesHeader).to_string());
        if self.fingerprints.is_empty() {
            lines.push(self.t(Key::FingerprintLinesEmpty).to_string());
            return lines;
        }
        for fp in &self.fingerprints {
            if !fp.has_open_ports() {
                lines.push(self.r(Key::FingerprintNoOpenPorts, &[&fp.ip().to_string()]));
                continue;
            }
            lines.push(format!("{}  {}", fp.ip(), fp.open_ports_summary()));
            for f in &fp.report.findings {
                if let Some(banner) = &f.banner {
                    lines.push(format!("    :{} {banner}", f.port));
                }
            }
            if !fp.device_summary().is_empty() {
                lines.push(format!("    {}", fp.device_summary()));
            }
        }
        lines
    }

    // ------------------------------------------------------------------
    // M14：历史与趋势
    // ------------------------------------------------------------------

    /// 趋势面板的展示文本（供测试断言使用，与界面同源）。
    ///
    /// 每台主机一条：可用率 + 平均/最差 RTT + 抖动 + 字符柱状在线率曲线。
    /// 曲线里的 `·` 是「这一轮没测到这台主机」，**不是**「离线」——这是最容易
    /// 被读错的一处，所以图例里必须写明。
    pub fn trend_lines(&self, width: usize) -> Vec<String> {
        let mut lines = Vec::new();
        // `{:.1}` 是 Rust 的格式说明，`render_template` 只认 `{}`，
        // 所以先格式化再当参数传进去（否则会显示出字面的 `{:.1}`）。
        let span = format!("{:.1}", self.series.span_ms() as f64 / 3_600_000.0);
        lines.push(self.r(
            Key::TrendHeader,
            &[
                &span,
                &self.series.len().to_string(),
                &self.series.total_rounds().to_string(),
                &(self.series.retention_secs / 86_400).to_string(),
            ],
        ));
        let report = self.series.report();
        if report.is_empty() {
            lines.push(String::new());
            lines.push(self.t(Key::TrendEmpty1).to_string());
            lines.push(self.t(Key::TrendEmpty2).to_string());
            return lines;
        }
        // 表头列宽与下面的数据行严格一致（17/7/7/9/9/8）——列宽是「排版」不是
        // 「文案」，不随语言变；英文列名比中文长时 Rust 的 `{:>N}` 不会截断，
        // 只是不再补空格，列会自然变宽而不会串位。
        lines.push(format!(
            "{:<17} {:>7} {:>7} {:>9} {:>9} {:>8}",
            self.t(Key::TrendColHost),
            self.t(Key::TrendColUptime),
            self.t(Key::TrendColOnlineTotal),
            self.t(Key::TrendColAvgRtt),
            self.t(Key::TrendColWorstRtt),
            self.t(Key::TrendColJitter),
        ));
        // 曲线宽度：预留给文字列，至少 8 格
        let curve_w = width.saturating_sub(56).max(8);
        for r in &report {
            let curve = self.series.uptime_curve(r.ip);
            let bar = cping_core::char_bar_chart(&curve, curve_w)
                .into_iter()
                .next()
                .unwrap_or_default();
            lines.push(format!(
                "{:<17} {:>7} {:>7} {:>9} {:>9} {:>8}  {}",
                r.ip.to_string(),
                r.uptime_text(),
                format!("{}/{}", r.online, r.samples),
                r.avg_rtt_text(),
                r.worst_rtt_text(),
                r.jitter_text(),
                bar
            ));
        }
        lines.push(String::new());
        lines.push(self.r(Key::TrendLegend, &[&curve_w.to_string()]));
        lines
    }

    // ------------------------------------------------------------------
    // M15：告警
    // ------------------------------------------------------------------

    /// 打开/关闭告警面板。首次打开时**就地启用告警**（默认规则）。
    ///
    /// 为什么不要求用户先跑 `--alert`：TUI 的全部卖点就是「不用查文档」。
    /// 按了 `A` 却只得到一句「请重启并加 --alert」是把人往外推。默认规则与
    /// CLI 完全同源（`cping_core::alerts`），阈值不会两处不一样。
    pub fn toggle_alerts_panel(&mut self) {
        self.show_alerts = !self.show_alerts;
        if self.show_alerts {
            if self.alerts.is_none() {
                self.alerts = Some(cping_core::AlertEngine::new(
                    cping_core::AlertConfig::default(),
                ));
                self.status = self.t(Key::StatusAlertsEnabled).into();
            } else if self.alert_log_is_empty() {
                self.status = self.t(Key::StatusAlertsEmpty).into();
            } else {
                self.status = self.t(Key::StatusAlertsPanel).into();
            }
        }
    }

    fn alert_log_is_empty(&self) -> bool {
        // 不用 `Option::is_none_or`：它要 Rust 1.82，而本项目的 MSRV 是 1.75
        match self.alerts.as_ref() {
            None => true,
            Some(e) => e.log().total_seen() == 0,
        }
    }

    /// 切换静音。
    ///
    /// 静音**不改判定**：规则照常求值、冷却照常推进，只是不产生通知——
    /// 这样解除静音不会「补报」一堆积压告警（核心里有对应的回归测试）。
    pub fn toggle_alert_mute(&mut self) {
        if self.alerts.is_none() {
            self.alerts = Some(cping_core::AlertEngine::new(
                cping_core::AlertConfig::default(),
            ));
        }
        let engine = self.alerts.as_mut().expect("上面刚保证存在");
        let next = !engine.is_muted();
        engine.set_muted(next);
        self.status = if next {
            self.t(Key::StatusAlertsMuted).into()
        } else {
            self.t(Key::StatusAlertsUnmuted).into()
        };
    }

    /// 求值一轮告警。`commit_round` 里调用，**在时序追加之后**——
    /// 可用率规则要用到「含本轮在内」的累计观测数，先追加再求值才自洽。
    fn run_alerts(&mut self) {
        let Some(engine) = self.alerts.as_mut() else {
            return;
        };
        let done: Vec<ProbeResult> = self.results.iter().flatten().cloned().collect();
        if done.is_empty() {
            return;
        }
        let uptime = if self.series.is_empty() {
            Default::default()
        } else {
            self.series.uptime_map()
        };
        // 上一轮基线由引擎自己维护（`seen`），这里传空切片即可——
        // 引擎内部对「没见过的地址」有明确语义，不需要界面再存一份。
        let at = self.round_at_ms.unwrap_or_else(now_ms);
        engine.evaluate(
            at,
            self.monitor.round(),
            &[],
            &done,
            &self.thresholds,
            &uptime,
        );
    }

    /// 告警面板文本（供测试断言，与界面同源）。
    ///
    /// 不接收宽度参数：这里全是等宽定长列，不需要像趋势面板那样按宽度画曲线。
    pub fn alert_lines(&self) -> Vec<String> {
        let Some(engine) = self.alerts.as_ref() else {
            return vec![
                self.t(Key::AlertLinesDisabled1).to_string(),
                self.t(Key::AlertLinesDisabled2).to_string(),
            ];
        };
        let mut lines = Vec::new();
        let cfg = engine.config();
        // 规则名走 TUI 字典（核心只给 `label_zh()`，见 `i18n::alert_kind_key`）。
        let enabled: Vec<&str> = cfg
            .rules
            .iter()
            .filter(|r| r.enabled)
            .map(|r| self.t(i18n::alert_kind_key(r.kind)))
            .collect();
        lines.push(self.r(
            Key::AlertStatusLine,
            &[
                self.t(if engine.is_muted() {
                    Key::AlertStateMuted
                } else {
                    Key::AlertStateActive
                }),
                &enabled.len().to_string(),
                &enabled.join(" / "),
                &engine.log().total_seen().to_string(),
                &engine.suppressed_by_cooldown().to_string(),
            ],
        ));
        lines.push(format!(
            "{:<17} {:>6} {:>8} {:>7}",
            self.t(Key::AlertColTime),
            self.t(Key::AlertColRound),
            self.t(Key::AlertColHost),
            self.t(Key::AlertColKind)
        ));
        let recent: Vec<String> = engine
            .log()
            .iter_recent_first()
            .map(|e| {
                format!(
                    "{:<17} {:>6} {:>8} {:>7}",
                    cping_core::format_unix_local(e.at_ms / 1000),
                    e.round,
                    e.ip.to_string(),
                    self.t(i18n::alert_kind_key(e.kind))
                )
            })
            .take(200)
            .collect();
        if recent.is_empty() {
            lines.push(String::new());
            lines.push(self.t(Key::AlertLinesEmpty1).to_string());
            lines.push(self.t(Key::AlertLinesEmpty2).to_string());
        } else {
            lines.extend(recent);
        }
        lines.push(String::new());
        // `e.message` 由核心生成（`cping_core::alerts` 里只有中文），界面
        // 不能重写它——那等于复刻核心的告警文案。它保持原样，是本次改动的
        // 已知边界（外面那层「最近一条：」是翻了的）。
        lines.push(
            self.r(
                Key::AlertLinesLatest,
                &[&engine
                    .log()
                    .iter_recent_first()
                    .next()
                    .map(|e| e.message.clone())
                    .unwrap_or_else(|| self.t(Key::AlertNone).to_string())],
            ),
        );
        lines
    }

    /// 状态栏用的告警摘要；未启用或没有记录时返回空串（不写「告警 0 条」这种噪音）。
    pub fn alert_note(&self) -> String {
        let Some(engine) = self.alerts.as_ref() else {
            return String::new();
        };
        let total = engine.log().total_seen();
        if total == 0 {
            return String::new();
        }
        // 未静音时后缀就是空串——空串不能进字典（`chinese_entries_are_not_empty...`
        // 会拦下来），所以这一支在代码里给。
        let suffix = if engine.is_muted() {
            self.t(Key::AlertNoteMutedSuffix)
        } else {
            ""
        };
        self.r(Key::AlertNote, &[&total.to_string(), suffix])
    }

    // ------------------------------------------------------------------
    // M8：持续监控
    // ------------------------------------------------------------------

    /// 开/关自动重扫。
    pub fn toggle_monitor(&mut self) {
        if self.watch_seconds.is_some() {
            self.watch_seconds = None;
            self.monitor
                .set_config(MonitorConfig {
                    enabled: false,
                    ..self.monitor.config().clone()
                })
                .ok();
            self.status = self.t(Key::StatusMonitorOff).into();
        } else {
            self.watch_seconds = Some(cping_core::MONITOR_DEFAULT_INTERVAL_MS / 1000);
            self.monitor
                .set_config(MonitorConfig {
                    enabled: true,
                    ..self.monitor.config().clone()
                })
                .ok();
            self.next_round_at = Some(Instant::now());
            let secs = self.watch_seconds.unwrap_or(0).to_string();
            self.status = self.r(Key::StatusMonitorOn, &[&secs]);
        }
    }

    /// 按预设档位调整重扫间隔（5s / 10s / 30s / 60s / 300s / 900s）。
    pub fn shift_interval(&mut self, dir: i32) {
        const STEPS: [u64; 6] = [5, 10, 30, 60, 300, 900];
        let current = self.watch_seconds.unwrap_or(30);
        let idx = STEPS
            .iter()
            .position(|s| *s >= current)
            .unwrap_or(STEPS.len() - 1);
        let next_idx = if dir < 0 {
            idx.saturating_sub(1)
        } else {
            (idx + 1).min(STEPS.len() - 1)
        };
        self.watch_seconds = Some(STEPS[next_idx]);
        self.monitor
            .set_config(MonitorConfig {
                enabled: true,
                interval_ms: STEPS[next_idx] * 1000,
                ..self.monitor.config().clone()
            })
            .ok();
        self.next_round_at = Some(Instant::now());
        let secs = STEPS[next_idx].to_string();
        self.status = self.r(Key::StatusInterval, &[&secs]);
    }

    /// 主循环每帧调用：到点且空闲时自动开下一轮。
    fn tick_monitor(&mut self) {
        if self.watch_seconds.is_none() || self.running {
            return;
        }
        let due = match self.next_round_at {
            Some(t) => Instant::now() >= t,
            None => true,
        };
        if due {
            self.start_scan();
        }
    }

    /// 监控是否处于开启状态。
    pub fn monitor_on(&self) -> bool {
        self.watch_seconds.is_some()
    }

    /// 距离下一轮还有多少秒。
    pub fn seconds_to_next_round(&self) -> u64 {
        match (self.watch_seconds, self.next_round_at) {
            (Some(_), Some(t)) => {
                let now = Instant::now();
                if t <= now {
                    0
                } else {
                    (t - now).as_secs()
                }
            }
            _ => 0,
        }
    }

    /// 本轮变化。
    pub fn last_changes(&self) -> &[Change] {
        self.monitor.last_round_changes()
    }

    /// 变化日志（供事件流面板与状态栏计数）。
    pub fn change_log(&self) -> &cping_core::monitor::ChangeLog {
        self.monitor.log()
    }

    /// 变化最多的前 N 条（新的在前），给事件流面板用。
    pub fn recent_changes(&self, n: usize) -> Vec<Change> {
        self.monitor
            .log()
            .iter_recent_first()
            .take(n)
            .cloned()
            .collect()
    }

    /// 某目标在当前轮次是否刚变化（表格高亮用）。
    pub fn change_kind_of_index(&self, index: usize) -> Option<ChangeKind> {
        self.monitor
            .last_round_changes()
            .iter()
            .find(|c| c.index as usize == index)
            .map(|c| c.kind)
    }

    // ------------------------------------------------------------------
    // 任务控制
    // ------------------------------------------------------------------

    pub fn start_scan(&mut self) {
        let spec = match parse_spec(&self.prefix, TargetRange::Standard) {
            Ok(s) => s,
            Err(e) => {
                // M18：核心错误按当前语言渲染（`localized`），界面只挑语言。
                let msg = e.localized(self.lang);
                self.status = self.r(Key::StatusInvalidInput, &[&msg]);
                self.watch_seconds = None;
                return;
            }
        };
        let targets = spec.targets.clone();
        let config = ProbeConfig {
            prefix: parse_prefix(&self.prefix).ok(),
            range: TargetRange::Standard,
            timeout_ms: 1000,
            concurrency: 32,
            thresholds: self.thresholds,
            mode: self.backend_mode,
        };

        match self.scheduler.start(&config, targets.clone()) {
            Ok(handle) => {
                self.cell_slots = matrix_slots(&targets);
                self.results = vec![None; targets.len()];
                self.targets = targets;
                self.spec = spec;
                self.stats = ScanStats::from_results_with_total(&[], self.targets.len());
                self.elapsed_ms = 0;
                self.started_at = Some(Instant::now());
                self.scan_time = now_string();
                self.round_at_ms = Some(now_ms());
                self.running = true;
                self.cancelled = false;
                self.handle = Some(handle);
                self.last_export = None;
                self.scroll = 0;
                self.quit_confirm = false;
                let summary = self.spec_summary();
                let mode = self.backend_mode.describe();
                let backend = self.backend_name();
                self.status = self.r(Key::StatusScanStarted, &[&summary, &mode, backend]);
            }
            Err(e) => {
                let msg = e.localized(self.lang);
                self.status = self.r(Key::StatusStartFailed, &[&msg]);
                // 启动失败就别继续空转重试
                self.watch_seconds = None;
            }
        }
    }

    pub fn cancel_scan(&mut self, silent: bool) {
        if let Some(handle) = &self.handle {
            if handle.cancel() && !silent {
                self.status = self.t(Key::StatusCancelRequested).into();
            }
        }
        // 拉取剩余事件，让终态落到统计里
        self.poll_backend();
    }

    fn poll_backend(&mut self) {
        let events: Vec<ProbeEvent> = match self.handle.as_ref() {
            Some(h) => h.drain(),
            None => return,
        };
        for event in events {
            self.apply_event(event);
        }
    }

    fn apply_event(&mut self, event: ProbeEvent) {
        match event {
            ProbeEvent::Started { total, .. } => {
                self.stats.total = total;
            }
            ProbeEvent::Result { result, .. } => {
                let idx = result.index as usize;
                if let Some(slot) = self.results.get_mut(idx) {
                    *slot = Some(result);
                }
                self.recompute_stats();
            }
            ProbeEvent::Progress { completed, .. } => {
                // 统计以核心为准，这里只用它兜底
                if self.stats.completed < completed {
                    self.recompute_stats();
                }
            }
            ProbeEvent::Finished {
                stats, elapsed_ms, ..
            } => {
                self.stats = stats;
                self.elapsed_ms = elapsed_ms;
                self.running = false;
                self.handle = None;
                // 扫描刚结束时 ARP 表最新鲜：顺手补一次 MAC/厂商（内部会落盘）
                self.enrich(true);
                self.commit_round();
                self.schedule_next_round();
                // `{:.2}` 不是 `{}` 占位符，先格式化再当参数传。
                let elapsed = format!("{:.2}", elapsed_ms as f64 / 1000.0);
                let round = self.monitor.round().to_string();
                let total = self.stats.total.to_string();
                let online = self.stats.online.to_string();
                let enrich = self.enrich_note.clone();
                let change = self.change_note();
                self.status = self.r(
                    Key::StatusRoundDone,
                    &[&round, &total, &online, &elapsed, &enrich, &change],
                );
            }
            ProbeEvent::Cancelled {
                stats, elapsed_ms, ..
            } => {
                self.stats = stats;
                self.elapsed_ms = elapsed_ms;
                self.running = false;
                self.cancelled = true;
                self.handle = None;
                // 已完成的那些结果一样值得补全，不能因为取消就丢掉
                self.enrich(true);
                self.commit_round();
                // 用户主动取消就不要自动再来一轮
                self.watch_seconds = None;
                self.next_round_at = None;
                let completed = self.stats.completed.to_string();
                let total = self.stats.total.to_string();
                let online = self.stats.online.to_string();
                let enrich = self.enrich_note.clone();
                self.status = self.r(
                    Key::StatusCancelled,
                    &[&completed, &total, &online, &enrich],
                );
            }
        }
    }

    /// 把本轮结果交给监控状态机，算出变化。
    fn commit_round(&mut self) {
        let done: Vec<ProbeResult> = self.results.iter().flatten().cloned().collect();
        if done.is_empty() {
            return;
        }
        self.monitor.commit_round(&done, &self.thresholds);
        // M14：同一份结果也进长期时序。用**本轮起始时刻**打时间戳，而不是「扫描结束的现在」——
        // 一轮本身可能跑几秒，用结束时刻会让曲线的时间轴一点点被拉长。
        let at = self.round_at_ms.unwrap_or_else(now_ms);
        self.series.append_results(at, &done);
        // M15：告警在时序追加**之后**求值——可用率规则要用含本轮在内的累计观测数
        self.run_alerts();
    }

    /// 变化摘要（无变化时返回空串，不写「变化 0 项」这种噪音）。
    fn change_note(&self) -> String {
        let changes = self.monitor.last_round_changes();
        if changes.is_empty() {
            return String::new();
        }
        let up = changes
            .iter()
            .filter(|c| c.kind == ChangeKind::Appeared)
            .count();
        let down = changes
            .iter()
            .filter(|c| c.kind == ChangeKind::Disappeared)
            .count();
        self.r(
            Key::ChangeNote,
            &[
                &changes.len().to_string(),
                &up.to_string(),
                &down.to_string(),
            ],
        )
    }

    fn schedule_next_round(&mut self) {
        self.next_round_at = self
            .watch_seconds
            .map(|secs| Instant::now() + Duration::from_secs(secs));
    }

    fn recompute_stats(&mut self) {
        let done: Vec<ProbeResult> = self.results.iter().flatten().cloned().collect();
        self.stats = ScanStats::from_results_with_total(&done, self.targets.len());
    }

    // ------------------------------------------------------------------
    // M7：目标信息补全（MAC / 厂商 / 主机名）
    // ------------------------------------------------------------------

    /// 读一次邻居表回填 MAC 与厂商。可重复调用（只补空缺）。
    pub fn enrich(&mut self, quiet: bool) {
        if self.results.is_empty() {
            if !quiet {
                self.status = self.t(Key::StatusNoResultsEnrich).into();
            }
            return;
        }
        let mut done: Vec<ProbeResult> = self.results.iter().flatten().cloned().collect();
        let report: EnrichReport = enrich_with_mac_and_vendor(&mut done);
        for r in done {
            if let Some(slot) = self.results.get_mut(r.index as usize) {
                *slot = Some(r);
            }
        }
        self.enrich_note = describe_enrich(self.lang, &report);
        if !quiet {
            self.status = self.enrich_note.clone();
        }
        self.autosave();
    }

    /// 反查「当前选中行」的主机名。放到线程里做，界面不卡。
    pub fn lookup_selected_hostname(&mut self) {
        if self.hostname_job.is_some() {
            self.status = self.t(Key::StatusHostnameBusy).into();
            return;
        }
        let Some(idx) = self.selected_target_index() else {
            self.status = self.t(Key::StatusHostnameNoRow).into();
            return;
        };
        let Some(result) = self.results.get(idx).and_then(|r| r.as_ref()) else {
            self.status = self.t(Key::StatusHostnameNoResult).into();
            return;
        };
        if result.hostname.is_some() {
            let ip = result.ip.to_string();
            let name = result.hostname.clone().unwrap_or_default();
            self.status = self.r(Key::StatusHostnameAlready, &[&ip, &name]);
            return;
        }
        let ip = result.ip;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let name = cping_core::hostname::reverse_lookup(ip, Duration::from_millis(1500));
            let _ = tx.send(name);
        });
        self.hostname_job = Some(HostnameJob { index: idx, ip, rx });
        let ip_text = ip.to_string();
        self.status = self.r(Key::StatusHostnameLooking, &[&ip_text]);
    }

    fn poll_hostname_job(&mut self) {
        let Some(job) = self.hostname_job.as_ref() else {
            return;
        };
        let index = job.index;
        let ip = job.ip;
        let outcome = job.rx.try_recv();
        match outcome {
            Ok(Some(name)) => {
                if let Some(Some(r)) = self.results.get_mut(index) {
                    r.hostname = Some(name.clone());
                }
                // `{ip} → {name}` 全是数据（地址与主机名），不含任何自然语言，
                // 所以不进字典——它就是「数据契约」那一类的同类。
                self.status = format!("{ip} → {name}");
                self.hostname_job = None;
                self.autosave();
            }
            Ok(None) => {
                // 查不到就如实说查不到，不填占位文本
                let ip_text = ip.to_string();
                self.status = self.r(Key::StatusHostnameNotFound, &[&ip_text]);
                self.hostname_job = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.status = self.t(Key::StatusHostnameThreadFail).into();
                self.hostname_job = None;
            }
        }
    }

    /// 表格「当前行」对应的 results 下标：即滚动窗口的第一行（与 `>` 标记一致）。
    pub fn selected_target_index(&self) -> Option<usize> {
        let rows = self.visible_rows();
        if rows.is_empty() {
            return None;
        }
        rows.get(self.scroll.min(rows.len() - 1)).copied()
    }

    // ------------------------------------------------------------------
    // M7：结果快照
    // ------------------------------------------------------------------

    fn snapshot_path(&self, name: &str) -> PathBuf {
        match &self.snapshot_dir {
            Some(dir) => dir.join(name),
            None => PathBuf::from(name),
        }
    }

    /// 由当前内存状态组装快照。
    fn build_snapshot(&self) -> Snapshot {
        Snapshot {
            scan_time: if self.scan_time.is_empty() {
                now_string()
            } else {
                self.scan_time.clone()
            },
            prefix: self.prefix.clone(),
            local_ip: self.local_ip.clone(),
            elapsed_ms: self.elapsed_ms,
            thresholds: self.thresholds,
            total: self.targets.len(),
            cancelled: self.cancelled,
            results: self.results.iter().flatten().cloned().collect(),
        }
    }

    /// 自动落盘。没有目标或没有结果时不写，免得用空快照盖掉上次的真实结果。
    fn autosave(&mut self) {
        let snap = self.build_snapshot();
        if snap.total == 0 || snap.results.is_empty() {
            return;
        }
        let path = self.snapshot_path(AUTOSAVE_FILE);
        if save_to_path(&snap, &path.to_string_lossy()).is_ok() {
            self.last_snapshot = Some(path.to_string_lossy().to_string());
        }
    }

    /// 显式另存为一份带时间戳的快照。
    pub fn save_snapshot(&mut self) {
        if self.targets.is_empty() {
            self.status = self.t(Key::StatusNoResultsToSave).into();
            return;
        }
        let snap = self.build_snapshot();
        let safe_prefix = self.prefix.replace('.', "_");
        let name = format!("cping-{safe_prefix}-{}.snap", file_stamp());
        let path = self.snapshot_path(&name);
        match save_to_path(&snap, &path.to_string_lossy()) {
            Ok(()) => {
                let rows = snap.results.len().to_string();
                let total = snap.total.to_string();
                let shown = path.display().to_string();
                self.status = self.r(Key::StatusSnapshotSaved, &[&rows, &total, &shown]);
                self.last_snapshot = Some(path.to_string_lossy().to_string());
                self.mark_handled(&snap);
            }
            Err(e) => {
                let msg = e.localized(self.lang);
                self.status = self.r(Key::StatusSnapshotSaveFailed, &[&msg]);
            }
        }
    }

    /// 启动时检查有没有「上次没处理完」的快照。
    fn check_pending_snapshot(&mut self) {
        let path = self.snapshot_path(AUTOSAVE_FILE);
        if !path.exists() {
            return;
        }
        match load_from_path(&path.to_string_lossy()) {
            Ok(snap) => {
                if self.read_handled().as_deref() == Some(fingerprint(&snap).as_str()) {
                    return; // 这份结果用户已经处理过，不再打扰
                }
                self.pending_restore = Some(PendingRestore {
                    path: path.to_string_lossy().to_string(),
                    scan_time: snap.scan_time.clone(),
                    prefix: snap.prefix.clone(),
                    total: snap.total,
                    rows: snap.results.len(),
                    elapsed_ms: snap.elapsed_ms,
                    error: None,
                });
                self.status = self.t(Key::StatusPendingFound).into();
            }
            Err(e) => {
                // `PendingRestore.error` 会被渲染到确认框里，所以这里就按当前
                // 语言定稿——核心错误的 `detail` 仍原样透传。
                let msg = e.localized(self.lang);
                self.pending_restore = Some(PendingRestore {
                    path: path.to_string_lossy().to_string(),
                    scan_time: String::new(),
                    prefix: String::new(),
                    total: 0,
                    rows: 0,
                    elapsed_ms: 0,
                    error: Some(msg),
                });
                self.status = self.t(Key::StatusPendingUnreadable).into();
            }
        }
    }

    /// 用户确认恢复。默认**不**自动恢复，必须显式按 y。
    pub fn restore_snapshot(&mut self) {
        let Some(pending) = self.pending_restore.take() else {
            return;
        };
        if let Some(err) = &pending.error {
            let err = err.clone();
            self.status = self.r(Key::StatusRestoreUnreadable, &[&err]);
            return;
        }
        let snap = match load_from_path(&pending.path) {
            Ok(s) => s,
            Err(e) => {
                let msg = e.localized(self.lang);
                self.status = self.r(Key::StatusRestoreFailed, &[&msg]);
                return;
            }
        };
        // 目标列表必须能由「网段 + 当前规则」完整重建，否则下标与 IP 会错位
        let parsed = match parse_prefix(&snap.prefix) {
            Ok(p) => p,
            Err(e) => {
                let msg = e.localized(self.lang);
                self.status = self.r(Key::StatusRestoreBadPrefix, &[&msg]);
                return;
            }
        };
        // 用规格解析器重建目标：它与 `start_scan` 走的是同一条路径，
        // 因此「快照声明的目标数」与「今天算出来的目标数」可直接比较。
        let targets = match spec_from_prefix(&parsed, TargetRange::Standard) {
            Ok(spec) => spec.targets,
            Err(e) => {
                let msg = e.localized(self.lang);
                self.status = self.r(Key::StatusRestoreSpecFailed, &[&msg]);
                return;
            }
        };
        if targets.len() != snap.total {
            let want = snap.total.to_string();
            let got = targets.len().to_string();
            self.status = self.r(Key::StatusRestoreMismatch, &[&want, &got]);
            return;
        }

        let total = targets.len();
        let mut slots: Vec<Option<ProbeResult>> = vec![None; total];
        for r in snap.results.iter() {
            let idx = r.index as usize;
            if idx >= total {
                let idx = idx.to_string();
                self.status = self.r(Key::StatusRestoreIndexOob, &[&idx]);
                return;
            }
            slots[idx] = Some(r.clone());
        }

        self.prefix = snap.prefix.clone();
        self.scan_time = snap.scan_time.clone();
        self.cell_slots = matrix_slots(&targets);
        self.targets = targets;
        self.results = slots;
        self.stats = snap.stats();
        self.elapsed_ms = snap.elapsed_ms;
        self.thresholds = snap.thresholds;
        self.cancelled = snap.cancelled;
        self.running = false;
        self.handle = None;
        self.scroll = 0;
        self.last_export = None;

        self.mark_handled(&snap);
        // 恢复出来的结果同样经不起进程被杀，立刻再落一次盘
        self.autosave();
        let scan_time = snap.scan_time.clone();
        let rows = snap.results.len().to_string();
        let total = snap.total.to_string();
        let elapsed = format!("{:.2}", snap.elapsed_ms as f64 / 1000.0);
        self.status = self.r(Key::StatusRestored, &[&scan_time, &rows, &total, &elapsed]);
    }

    /// `l` 键：手动从自动落盘文件恢复（例如刚才按 n 忽略了、后来又想恢复）。
    pub fn load_snapshot_from_disk(&mut self) {
        self.check_pending_snapshot();
        if self.pending_restore.is_none() {
            let path = self.snapshot_path(AUTOSAVE_FILE).display().to_string();
            self.status = self.r(Key::StatusNoSnapshot, &[&path]);
        }
    }

    /// 放弃恢复；`delete` 时顺手删掉自动落盘文件。
    pub fn dismiss_restore(&mut self, delete: bool) {
        let Some(pending) = self.pending_restore.take() else {
            return;
        };
        if let Ok(snap) = load_from_path(&pending.path) {
            self.mark_handled(&snap);
        }
        if delete {
            let _ = std::fs::remove_file(&pending.path);
        }
        self.status = self.t(Key::StatusDismissed).into();
    }

    fn handled_path(&self) -> PathBuf {
        self.snapshot_path(HANDLED_FILE)
    }

    fn read_handled(&self) -> Option<String> {
        std::fs::read_to_string(self.handled_path())
            .ok()
            .map(|s| s.trim().to_string())
    }

    fn mark_handled(&self, snap: &Snapshot) {
        let _ = std::fs::write(self.handled_path(), fingerprint(snap));
    }

    // ------------------------------------------------------------------
    // 视图辅助
    // ------------------------------------------------------------------

    /// 表格要显示的行（results 下标）。恒按 IP 升序。
    pub fn visible_rows(&self) -> Vec<usize> {
        (0..self.targets.len())
            .filter(|i| {
                if !self.hide_offline {
                    return true;
                }
                matches!(
                    self.results.get(*i).and_then(|r| r.as_ref()).map(|r| &r.status),
                    Some(s) if s.is_online()
                )
            })
            .collect()
    }

    pub fn cell_result(&self, host: u8) -> Option<&ProbeResult> {
        self.cell_slots
            .get(host as usize)
            .and_then(|slot| slot.as_ref())
            .and_then(|idx| self.results.get(*idx))
            .and_then(|r| r.as_ref())
    }

    pub fn class_of(&self, result: Option<&ProbeResult>) -> Option<ResultClass> {
        result.and_then(|r| classify_status(&r.status, &self.thresholds))
    }

    pub fn progress_ratio(&self) -> f64 {
        self.stats.progress()
    }

    pub fn save(&mut self, ext: &str) {
        if self.results.is_empty() {
            self.status = self.t(Key::StatusNothingToExport).into();
            return;
        }
        let done: Vec<ProbeResult> = self.results.iter().flatten().cloned().collect();
        if done.is_empty() {
            self.status = self.t(Key::StatusNoDoneResults).into();
            return;
        }
        let meta = ExportMeta {
            scan_time: now_string(),
            local_ip: self.local_ip.clone(),
            elapsed_ms: self.elapsed_ms,
            mode: self.backend_mode,
            // 刻意用核心的 `summary()`（中文）而不是 TUI 的可本地化版本：
            // 这是**写进导出文件**的内容，必须与界面语言无关——
            // 同一份数据在中文与英文机器上导出的报表应当逐字节可比。
            target_desc: self.spec.summary(),
            round: self.monitor.round(),
        };
        let safe_prefix = self.prefix.replace(['.', '/', ',', ' ', ':'], "_");
        let path = format!("cping-{safe_prefix}-{}.{}", file_stamp(), ext);
        match export_to_path(&done, &path, &meta, &self.thresholds) {
            Ok(bytes) => {
                let written = std::fs::write(&path, &bytes);
                match written {
                    Ok(()) => {
                        let count = done.len().to_string();
                        let size = bytes.len().to_string();
                        self.status = self.r(Key::StatusExported, &[&count, &path, &size]);
                        self.last_export = Some(path);
                    }
                    Err(e) => {
                        // `e` 是 `io::Error`：来自操作系统的细节**原样透传**，
                        // 只本地化外面那层框。
                        let detail = e.to_string();
                        self.status = self.r(Key::StatusWriteFailed, &[&detail, &path]);
                    }
                }
            }
            Err(e) => {
                let msg = e.localized(self.lang);
                self.status = self.r(Key::StatusExportFailed, &[&msg]);
            }
        }
    }
}

/// 把核心的补全报告翻成一句人话（界面只说真实发生的事）。
fn describe_enrich(lang: Lang, report: &EnrichReport) -> String {
    if !report.table_ok {
        let reason = report
            .error
            .as_deref()
            .unwrap_or_else(|| i18n::text(lang, Key::EnrichUnknownReason));
        return i18n::render(lang, Key::EnrichTableFailed, &[reason]);
    }
    if report.mac_filled == 0 && report.vendor_filled == 0 {
        return i18n::render(lang, Key::EnrichNoFill, &[&report.entries.to_string()]);
    }
    i18n::render(
        lang,
        Key::EnrichFilled,
        &[
            &report.entries.to_string(),
            &report.mac_filled.to_string(),
            &report.vendor_filled.to_string(),
        ],
    )
}

/// 同一份结果的判定指纹：任一要素变化即视为「新结果」。
fn fingerprint(snap: &Snapshot) -> String {
    format!(
        "{}|{}|{}|{}|{}",
        snap.scan_time,
        snap.prefix,
        snap.total,
        snap.results.len(),
        snap.elapsed_ms
    )
}

fn local_ip_label(lang: Lang) -> String {
    use cping_core::local_ip::{pick_primary, LocalAddressProvider, UnsupportedProvider};

    let provider = UnsupportedProvider;
    match provider.list() {
        Ok(list) => pick_primary(&list)
            .map(|a| a.ip.to_string())
            .unwrap_or_else(|| i18n::text(lang, Key::LocalIpUnknown).to_string()),
        // M5 接入真实网卡枚举；在此之前如实说明，不猜一个地址
        Err(_) => i18n::text(lang, Key::LocalIpUnknownM5).to_string(),
    }
}

/// `YYYY-MM-DD HH:MM:SS`（本地时区近似为 UTC+8 偏移不处理，保持 UTC）。
pub fn now_string() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_hms(secs)
}

/// 当前 Unix 毫秒（M14 时序点时间戳）。取不到就返回 0，由核心按「时间未知」处理。
pub fn now_ms() -> u64 {
    cping_core::unix_millis_now()
}

/// 文件名用时间戳 `YYYYMMDD-HHMMSS`。
pub fn file_stamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

fn format_hms(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Howard Hinnant 的 civil_from_days（无第三方依赖）。
///
/// M15 起算法下沉到 `cping_core::timefmt`（此前三端各一份拷贝），
/// 这里保留同名包装只是为了不动既有调用点。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    cping_core::civil_from_days(z)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    /// 测试**必须**使用模拟后端：绝不能让单元测试去真实扫描局域网。
    /// 快照目录也指到临时目录，避免测试往仓库里写文件。
    fn opts() -> Options {
        Options {
            no_color: true,
            mock: true,
            snapshot_dir: Some(unique_tmp_dir()),
            ..Options::default()
        }
    }

    /// 每个测试一个独立目录，避免并行执行时互相看到对方的快照文件。
    fn unique_tmp_dir() -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("cping-tui-test-{}-{}", std::process::id(), n));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    /// 用 ratatui 的内存后端真实渲染一帧，返回**剔除全部空白字符**后的可见文本。
    ///
    /// 为什么不按 cell 宽度步进：CJK 自身是宽字符（右侧有占位 cell），而表格边框
    /// `│`/`┌`/`─` 和 `×` 都是**宽度为 1 的非 ASCII** 字符。按 `is_ascii()` 判断宽度
    /// 会误跳过下一个 cell，导致断言出现"丢字"的假象。统一拼接后剔除空白最稳妥。
    fn render(app: &App, w: u16, h: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).expect("terminal");
        terminal
            .draw(|frame| crate::ui::draw(frame, app))
            .expect("draw");
        let buffer = terminal.backend().buffer();
        let mut raw = String::with_capacity(buffer.content.len());
        for cell in &buffer.content {
            raw.push_str(cell.symbol());
        }
        raw.chars().filter(|c| !c.is_whitespace()).collect()
    }

    fn wait_done(app: &mut App, rounds: usize) {
        for _ in 0..rounds {
            std::thread::sleep(std::time::Duration::from_millis(15));
            app.poll_backend();
            if !app.running {
                return;
            }
        }
    }

    #[test]
    fn renders_input_and_matrix_title() {
        let app = App::new(opts());
        let out = render(&app, 100, 32);
        assert!(out.contains("目标"), "应显示目标输入区");
        assert!(out.contains("图形显示"), "应显示矩阵标题");
        assert!(out.contains("192.168.1"), "应显示默认目标");
    }

    #[test]
    fn matrix_contains_all_cell_numbers() {
        let app = App::new(opts());
        let out = render(&app, 100, 32);
        for host in ["0", "1", "16", "127", "240", "255"] {
            assert!(out.contains(host), "矩阵缺少格号 {host}");
        }
    }

    #[test]
    fn help_overlay_renders() {
        let mut app = App::new(opts());
        app.show_help = true;
        let out = render(&app, 130, 64);
        assert!(out.contains("快捷键"));
        assert!(out.contains("Ctrl+C"));
        // M8 / M9 的新键也必须在帮助里
        assert!(out.contains("持续监控"), "帮助应含 M8 段落");
        assert!(out.contains("TCP-OPEN"), "帮助应说明 TCP 模式的状态语义");
        assert!(out.contains("CIDR"), "帮助应说明 M9 的目标写法");
        // M13 段落 + 那条最容易误读的边界说明
        assert!(out.contains("资产识别"), "帮助应含 M13 段落");
        assert!(
            out.contains("端口扫描与在线判定是两回事"),
            "帮助必须写明端口扫描与在线判定互不影响"
        );
        // M14 / M15 段落
        assert!(out.contains("历史与趋势"), "帮助应含 M14 段落");
        assert!(
            out.contains("不是「离线」"),
            "帮助必须写明趋势曲线里 · 的含义"
        );
        assert!(out.contains("告警"), "帮助应含 M15 段落");
        assert!(
            out.contains("第一轮不发"),
            "帮助必须写明「第一轮不刷新增主机」这条最容易踩的边界"
        );
    }

    #[test]
    fn changes_overlay_explains_the_empty_state() {
        let mut app = App::new(opts());
        app.show_changes = true;
        let out = render(&app, 110, 32);
        assert!(out.contains("变化事件流"), "应有变化面板标题");
        assert!(out.contains("首轮只建立基线"), "空态要解释为什么没有变化");
    }

    #[test]
    fn small_terminal_shows_guidance_not_garbage() {
        let app = App::new(opts());
        let out = render(&app, 40, 14);
        assert!(out.contains("终端过小"), "小终端应给出指引");
    }

    #[test]
    fn scan_updates_stats_and_matrix_symbols() {
        let mut app = App::new(opts());
        app.start_scan();
        assert!(app.running, "应进入运行态");
        wait_done(&mut app, 300);

        assert!(!app.running, "模拟探测应在有限时间内结束");
        assert_eq!(app.stats.total, 255);
        assert_eq!(app.stats.completed, 255);
        assert!(app.stats.online > 0, "Realistic 策略应产生在线目标");
        assert_eq!(app.results.iter().flatten().count(), 255);

        let out = render(&app, 100, 32);
        assert!(out.contains("在线"), "状态栏应显示在线数");
        // 符号区分能力：至少出现一种分类符号
        assert!(
            out.contains('#') || out.contains('+') || out.contains('*'),
            "矩阵应出现分类符号"
        );
    }

    #[test]
    fn table_view_and_offline_filter_agree_with_core_stats() {
        let mut app = App::new(opts());
        app.start_scan();
        wait_done(&mut app, 300);
        app.view = ViewMode::Table;

        let out = render(&app, 110, 30);
        assert!(out.contains("表单显示"));

        let before = app.visible_rows().len();
        assert_eq!(before, 255);

        app.hide_offline = true;
        let after = app.visible_rows().len();
        // 与核心统计口径一致：过滤后条数必须等于核心给出的在线数
        assert_eq!(after, app.stats.online, "过滤结果必须与核心统计一致");
    }

    #[test]
    fn cancel_keeps_completed_results_and_marks_cancelled() {
        let mut app = App::new(opts());
        app.start_scan();
        std::thread::sleep(std::time::Duration::from_millis(1));
        app.cancel_scan(false);
        wait_done(&mut app, 300);

        assert!(!app.running);
        assert_eq!(
            app.results.iter().flatten().count(),
            app.stats.completed,
            "已完成结果必须与统计一致地保留"
        );
        assert!(app.stats.completed <= 255);
    }

    #[test]
    fn invalid_prefix_is_rejected_without_starting() {
        let mut app = App::new(opts());
        app.prefix = "999.999.999".into();
        app.start_scan();
        assert!(!app.running, "非法前缀不得启动任务");
        assert!(app.status.contains("输入无效"), "状态: {}", app.status);
    }

    #[test]
    fn export_produces_csv_with_bom_and_txt() {
        let mut app = App::new(opts());
        app.prefix = "127.0.0".into();
        app.start_scan();
        wait_done(&mut app, 300);

        app.save("csv");
        assert!(app.status.starts_with("已导出"), "状态: {}", app.status);
        let csv_path = app.last_export.clone().expect("csv path");
        let bytes = std::fs::read(&csv_path).expect("read csv");
        assert_eq!(&bytes[..3], &[0xEF, 0xBB, 0xBF], "CSV 必须带 UTF-8 BOM");
        let _ = std::fs::remove_file(&csv_path);

        app.save("txt");
        let txt_path = app.last_export.clone().expect("txt path");
        let text = std::fs::read(&txt_path).expect("read txt");
        let s = String::from_utf8(text).expect("utf8");
        assert!(s.contains("在线主机"));
        assert!(s.contains("目标总数 : 255"));
        let _ = std::fs::remove_file(&txt_path);
    }

    #[test]
    fn config_roundtrip_through_core() {
        let cfg = cping_core::AppConfig::default();
        let text = cfg.to_toml();
        let back = cping_core::AppConfig::from_toml(&text).expect("parse");
        assert_eq!(back.colors.low, cfg.colors.low);
        assert_eq!(back.probe.concurrency, cfg.probe.concurrency);
        assert_eq!(back.probe.timeout_ms, cfg.probe.timeout_ms);
    }

    // ------------------------------------------------------------------
    // M7
    // ------------------------------------------------------------------

    use std::path::Path;

    /// 指定快照目录的选项（同一个目录可以被两个 App 实例共享，用来模拟「重启」）。
    fn opts_in(dir: &Path) -> Options {
        Options {
            no_color: true,
            mock: true,
            snapshot_dir: Some(dir.to_path_buf()),
            ..Options::default()
        }
    }

    #[test]
    fn enrich_does_not_change_statuses_or_stats() {
        let mut app = App::new(opts());
        app.prefix = "127.0.0".into();
        app.start_scan();
        wait_done(&mut app, 300);

        let before_stats = app.stats.clone();
        let before: Vec<(IpAddr, cping_core::ProbeStatus)> = app
            .results
            .iter()
            .flatten()
            .map(|r| (r.ip, r.status.clone()))
            .collect();

        app.enrich(false);

        // 补全只允许写 mac/vendor 两个字段，其它一律不动
        assert_eq!(app.stats, before_stats, "补全不得改变统计");
        let after: Vec<(IpAddr, cping_core::ProbeStatus)> = app
            .results
            .iter()
            .flatten()
            .map(|r| (r.ip, r.status.clone()))
            .collect();
        assert_eq!(before, after, "补全不得改变任何探测结果");
        assert!(!app.enrich_note.is_empty(), "必须给出一句可显示的说明");
    }

    #[test]
    fn scan_result_is_autosaved_and_restore_needs_confirmation() {
        let dir = unique_tmp_dir();
        let mut app = App::new(opts_in(&dir));
        app.prefix = "127.0.0".into();
        app.start_scan();
        wait_done(&mut app, 300);
        assert!(!app.running);
        assert!(
            dir.join(AUTOSAVE_FILE).exists(),
            "扫描结束后必须自动落盘（进程随时可能被杀）"
        );

        let before_stats = app.stats.clone();
        let before_rows = app.results.iter().flatten().count();
        let before_elapsed = app.elapsed_ms;

        // 新实例 = 进程被杀后重启：**不得**自动恢复，只能先问
        let mut restarted = App::new(opts_in(&dir));
        assert_eq!(
            restarted.results.iter().flatten().count(),
            0,
            "默认不自动恢复"
        );
        assert!(restarted.pending_restore.is_some(), "启动时应发现上次结果");

        restarted.restore_snapshot();
        assert!(restarted.pending_restore.is_none());
        assert_eq!(restarted.prefix, "127.0.0");
        assert_eq!(restarted.elapsed_ms, before_elapsed);
        assert_eq!(restarted.results.iter().flatten().count(), before_rows);
        // 恢复前后统计必须与快照时刻完全一致
        assert_eq!(restarted.stats, before_stats);
    }

    #[test]
    fn dismissing_the_prompt_stops_further_questions() {
        let dir = unique_tmp_dir();
        let mut app = App::new(opts_in(&dir));
        app.prefix = "127.0.0".into();
        app.start_scan();
        wait_done(&mut app, 300);

        let mut restarted = App::new(opts_in(&dir));
        assert!(restarted.pending_restore.is_some());
        restarted.dismiss_restore(true);
        assert!(!dir.join(AUTOSAVE_FILE).exists(), "选择丢弃应删掉文件");

        let third = App::new(opts_in(&dir));
        assert!(third.pending_restore.is_none(), "同一份结果不应反复询问");
    }

    #[test]
    fn corrupt_snapshot_is_reported_as_unreadable() {
        let dir = unique_tmp_dir();
        std::fs::write(dir.join(AUTOSAVE_FILE), "这不是快照\n").unwrap();
        let app = App::new(opts_in(&dir));
        let pending = app.pending_restore.expect("应发现文件并如实报错");
        assert!(pending.error.is_some());
        assert!(app.status.contains("无法读取"), "状态: {}", app.status);
    }

    #[test]
    fn restore_refuses_snapshot_not_matching_target_rules() {
        let dir = unique_tmp_dir();
        let snap = Snapshot {
            scan_time: "2026-10-14 21:40:00".into(),
            prefix: "127.0.0".into(),
            local_ip: "未知".into(),
            elapsed_ms: 1,
            thresholds: Thresholds::default(),
            // 与「网段 + Standard」能算出的 255 个目标不符
            total: 100,
            cancelled: false,
            results: Vec::new(),
        };
        save_to_path(&snap, &dir.join(AUTOSAVE_FILE).to_string_lossy()).unwrap();

        let mut app = App::new(opts_in(&dir));
        app.restore_snapshot();
        assert!(app.targets.is_empty(), "不一致的快照不得写入状态");
        assert!(app.status.contains("不一致"), "状态: {}", app.status);
    }

    #[test]
    fn explicit_snapshot_save_writes_a_readable_file() {
        let dir = unique_tmp_dir();
        let mut app = App::new(opts_in(&dir));
        app.prefix = "127.0.0".into();
        app.start_scan();
        wait_done(&mut app, 300);

        app.save_snapshot();
        assert!(app.status.starts_with("已保存快照"), "状态: {}", app.status);
        let path = app.last_snapshot.clone().expect("应记录快照路径");
        let snap = load_from_path(&path).expect("写出的快照必须能解析回来");
        assert_eq!(snap.total, app.targets.len());
        assert_eq!(snap.results.len(), app.results.iter().flatten().count());
    }

    #[test]
    fn fingerprint_panel_shows_ports_banners_and_inference_without_touching_online_verdict() {
        use cping_core::{HostFingerprint, PortFinding, PortScanReport};
        let mut app = App::new(opts());
        app.prefix = "127.0.0".into();
        app.start_scan();
        wait_done(&mut app, 300);
        let stats_before = app.stats.clone();

        // 直接注入指纹结果：不依赖本机是否真的开了端口，只验证渲染与边界
        app.fingerprints = vec![
            HostFingerprint {
                report: PortScanReport {
                    ip: "127.0.0.1".parse().unwrap(),
                    findings: vec![PortFinding {
                        port: 22,
                        service: Some("ssh"),
                        banner: Some("SSH-2.0-X".into()),
                        connect_rtt_ms: 1,
                    }],
                    scanned: 28,
                    elapsed_ms: 40,
                },
                device: cping_core::infer_device(Some("TP-Link"), &[22]),
            },
            HostFingerprint {
                report: PortScanReport {
                    ip: "127.0.0.2".parse().unwrap(),
                    findings: Vec::new(),
                    scanned: 28,
                    elapsed_ms: 38,
                },
                device: None,
            },
        ];
        app.show_fingerprints = true;

        let text = render(&app, 100, 30);
        assert!(text.contains("端口指纹"), "{text}");
        assert!(text.contains("与在线判定相互独立"), "标题必须写明这条边界");
        assert!(text.contains("22(ssh)"), "应显示端口与服务");
        assert!(text.contains("SSH-2.0-X"), "横幅应原样显示");
        assert!(text.contains("推断"), "设备结论必须标注为推断");
        assert!(text.contains("无开放端口"), "端口全关的主机也要列出");

        // 关键：端口扫描绝不允许改动在线判定
        assert_eq!(app.stats, stats_before, "指纹不得改动任何统计");
    }

    #[test]
    fn pressing_f_without_online_hosts_explains_instead_of_doing_nothing() {
        let mut app = App::new(opts());
        // 尚未扫描 → 没有任何在线主机
        app.fingerprint_targets();
        assert!(
            app.status.contains("没有在线主机"),
            "应给出明确原因而不是静默无反应：{}",
            app.status
        );
        assert!(app.fingerprints.is_empty());
    }

    #[test]
    fn fingerprint_panel_is_closed_by_any_key() {
        use ratatui::crossterm::event::{KeyCode, KeyModifiers};
        let mut app = App::new(opts());
        app.show_fingerprints = true;
        app.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert!(!app.show_fingerprints, "任意键应关闭面板");
        // 小写 f（过滤）也不该被面板吃掉两次
        app.show_fingerprints = true;
        app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
        assert!(!app.show_fingerprints);
    }

    #[test]
    fn fingerprint_lines_are_available_before_any_scan() {
        let app = App::new(opts());
        let lines = app.fingerprint_lines();
        assert!(lines[0].contains("端口指纹"));
        assert!(
            lines[1].contains("按 F"),
            "空状态要告诉用户怎么触发：{:?}",
            lines
        );
    }

    // ------------------------------------------------------------------
    // M14：历史与趋势
    // ------------------------------------------------------------------

    #[test]
    fn trend_lines_explain_the_empty_state_instead_of_showing_zeros() {
        let app = App::new(opts());
        let lines = app.trend_lines(80);
        assert!(lines[0].contains("时间跨度"), "{lines:?}");
        assert!(lines[0].contains("0 个时间点"), "{lines:?}");
        assert!(
            lines.iter().any(|l| l.contains("还没有历史数据")),
            "空状态必须说清楚，而不是画一堆 0% 让人误读：{lines:?}"
        );
    }

    #[test]
    fn trend_accumulates_one_point_per_round_and_matches_the_core_report() {
        let mut app = App::new(opts());
        app.prefix = "127.0.0".into();
        for _ in 0..3 {
            app.start_scan();
            wait_done(&mut app, 300);
        }
        assert_eq!(app.series.len(), 3, "每完成一轮就该多一个时间点");
        assert_eq!(app.series.total_rounds(), 3);

        // 报表数值与核心同源，不做二次加工：每台主机被观测到 3 次，在线次数不超过样本数
        let report = app.series.report();
        assert_eq!(report.len(), app.targets.len(), "每台目标都应有一行");
        assert!(
            report.iter().any(|r| r.online > 0),
            "整网段必然有在线主机：{report:?}"
        );
        for r in &report {
            assert_eq!(r.samples, 3, "三轮观测，样本数必须是 3：{r:?}");
            assert!(r.online <= r.samples, "{r:?}");
            if r.online == 0 {
                assert_eq!(r.avg_rtt_ms, None, "从未在线不得给 0 冒充 RTT：{r:?}");
            } else {
                assert!(r.avg_rtt_ms.is_some(), "{r:?}");
            }
        }
    }

    #[test]
    fn trend_panel_renders_uptime_bars_and_spells_out_the_dot_meaning() {
        let mut app = App::new(opts());
        app.prefix = "127.0.0".into();
        app.start_scan();
        wait_done(&mut app, 300);
        app.show_trend = true;

        let text = render(&app, 120, 34);
        assert!(text.contains("历史与趋势"), "{text}");
        assert!(text.contains("可用率"), "{text}");
        assert!(text.contains("100.0%"), "应显示可用率数值：{text}");
        // 曲线用的是核心的字符柱状图（满格 █ 表示 100% 在线）
        assert!(text.contains('█'), "在线率曲线应渲染成字符柱：{text}");
        // 「没测到 ≠ 离线」这条边界必须在面板里写明
        let lines = app.trend_lines(100);
        assert!(
            lines.iter().any(|l| l.contains("不是") || l.contains("≠")),
            "必须解释 · 的含义：{lines:?}"
        );
    }

    #[test]
    fn trend_panel_is_closed_by_any_key() {
        use ratatui::crossterm::event::{KeyCode, KeyModifiers};
        let mut app = App::new(opts());
        app.show_trend = true;
        app.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert!(!app.show_trend, "任意键应关闭面板");
        // 再按一次 U 仍能打开（U 在面板里表现为「关闭」，不该被吃掉两次）
        app.on_key(KeyEvent::new(KeyCode::Char('U'), KeyModifiers::NONE));
        assert!(app.show_trend, "U 应重新打开面板");
    }

    #[test]
    fn pressing_u_before_any_scan_says_why_there_is_nothing_to_show() {
        let mut app = App::new(opts());
        app.on_key(KeyEvent::new(
            ratatui::crossterm::event::KeyCode::Char('U'),
            ratatui::crossterm::event::KeyModifiers::NONE,
        ));
        assert!(app.show_trend);
        assert!(
            app.status.contains("还没有历史数据"),
            "应给出明确原因：{}",
            app.status
        );
    }

    #[test]
    fn table_detail_line_shows_mac_vendor_and_hostname() {
        let mut app = App::new(opts());
        app.prefix = "127.0.0".into();
        app.start_scan();
        wait_done(&mut app, 300);
        // 直接造数据，只验证渲染，不依赖本机 ARP / DNS 的实际情况
        for r in app.results.iter_mut().flatten() {
            r.mac = Some("AA:BB:CC:DD:EE:FF".into());
            r.vendor = Some("Acme Networks".into());
            r.hostname = Some("router.local".into());
        }
        app.view = ViewMode::Table;

        let wide = render(&app, 110, 32);
        assert!(wide.contains("厂商"), "宽终端详情行应带厂商标签");
        assert!(wide.contains("Acme"), "详情行应显示厂商名");
        assert!(wide.contains("router.local"), "详情行应显示主机名");

        // 窄终端：MAC 从主行挪到详情行，且被按宽度截断而不是溢出
        let narrow = render(&app, 62, 32);
        assert!(narrow.contains("MAC"), "窄终端 MAC 应出现在详情行");
        assert!(narrow.contains("厂商"), "窄终端也应尽力展示厂商");
    }

    #[test]
    fn hostname_lookup_refuses_when_no_selection() {
        let mut app = App::new(opts());
        // 还没扫描，没有任何可反查的行
        app.lookup_selected_hostname();
        assert!(app.hostname_job.is_none());
        assert!(
            app.status.contains("没有可反查的行"),
            "状态: {}",
            app.status
        );
    }

    // ------------------------------------------------------------------
    // M15：告警
    // ------------------------------------------------------------------

    /// M15 用的选项：默认不开告警（与 `--alert` 的语义一致）。
    fn alert_opts() -> Options {
        Options {
            alert: true,
            ..opts()
        }
    }

    /// 造一台确定在线的主机：`10.0.x.y`（x/y 由序号推出，保证互不相同）。
    fn online(idx: u16, rtt_ms: u32) -> ProbeResult {
        let ip = IpAddr::from([10, 0, (idx >> 8) as u8, (idx & 0xff) as u8]);
        ProbeResult::new(idx, ip, cping_core::ProbeStatus::Online { rtt_ms })
    }

    #[test]
    fn alerts_are_off_unless_asked() {
        let app = App::new(opts());
        assert!(app.alerts.is_none(), "不问就不该有任何告警");
        assert!(app.alert_note().is_empty(), "未启用时不占用状态栏");

        let on = App::new(alert_opts());
        assert!(on.alerts.is_some(), "--alert 应启用告警");
    }

    #[test]
    fn pressing_a_enables_alerts_in_place_instead_of_asking_for_a_restart() {
        let mut app = App::new(opts());
        app.on_key(KeyEvent::new(
            ratatui::crossterm::event::KeyCode::Char('A'),
            ratatui::crossterm::event::KeyModifiers::NONE,
        ));
        assert!(app.show_alerts);
        assert!(app.alerts.is_some(), "首次按 A 应就地启用告警");
        assert!(
            app.status.contains("已启用"),
            "要告诉用户发生了什么：{}",
            app.status
        );
    }

    #[test]
    fn alert_panel_is_closed_by_any_key_and_reopens_with_a() {
        use ratatui::crossterm::event::{KeyCode, KeyModifiers};
        let mut app = App::new(alert_opts());
        app.show_alerts = true;
        app.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert!(!app.show_alerts, "任意键应关闭面板");
        app.on_key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::NONE));
        assert!(app.show_alerts, "A 应重新打开面板");
    }

    #[test]
    fn alert_lines_explain_the_empty_state_instead_of_showing_zero_alerts() {
        let app = App::new(alert_opts());
        let lines = app.alert_lines();
        assert!(lines[0].contains("累计告警 0 条"), "{lines:?}");
        assert!(
            lines.iter().any(|l| l.contains("还没有告警记录")),
            "空状态要说清楚，而不是只写一行 0：{lines:?}"
        );
        assert!(
            lines.iter().any(|l| l.contains("第一轮不会报")),
            "「第一轮不刷屏」这条最容易被误解，必须写在面板里：{lines:?}"
        );
    }

    #[test]
    fn alerts_fire_after_a_round_and_show_up_in_the_status_line() {
        let mut app = App::new(alert_opts());
        let mut cfg = cping_core::AlertConfig::default();
        cfg.with_kinds(&[cping_core::AlertKind::LatencyAbove]);
        for r in &mut cfg.rules {
            if r.kind == cping_core::AlertKind::LatencyAbove {
                r.threshold = 0;
                r.cooldown_secs = 0;
            }
        }
        app.alerts = Some(cping_core::AlertEngine::new(cfg));

        // 直接喂一份确定的结果，不依赖模拟后端这一轮的随机时延
        app.results = vec![Some(online(0, 271)), Some(online(1, 12))];
        app.round_at_ms = Some(1_700_000_000_000);
        app.run_alerts();

        let engine = app.alerts.as_ref().expect("已启用");
        assert_eq!(
            engine.log().total_seen(),
            2,
            "两台主机都超过 0ms 阈值，且严格大于才算命中"
        );
        assert!(
            app.alert_note().contains("告警 2 条"),
            "状态栏应带告警摘要：{}",
            app.alert_note()
        );
        let lines = app.alert_lines();
        assert!(
            lines.iter().any(|l| l.contains("延迟超阈")),
            "面板里要能看到刚才那条：{lines:?}"
        );
        assert!(
            lines.iter().any(|l| l.contains("10.0.0.1")),
            "面板要显示是哪台主机：{lines:?}"
        );
    }

    #[test]
    fn first_round_does_not_report_every_online_host_as_new() {
        let mut app = App::new(alert_opts());
        let mut cfg = cping_core::AlertConfig::default();
        cfg.with_kinds(&[cping_core::AlertKind::Appeared]);
        app.alerts = Some(cping_core::AlertEngine::new(cfg));

        // 256 台主机「一次性全部上线」——正是扫 /24 的实际情况
        app.results = (0..=255)
            .map(|i| Some(online(i as u16, 5)))
            .collect::<Vec<_>>();
        app.round_at_ms = Some(1_700_000_000_000);
        app.run_alerts();

        let engine = app.alerts.as_ref().expect("已启用");
        assert_eq!(
            engine.log().total_seen(),
            0,
            "第一轮只建立基线，不许把整段在线主机当成「新增」刷出来"
        );

        // 第二轮多出一台，才是真正的「新增」
        app.results.push(Some(online(256, 5)));
        app.round_at_ms = Some(1_700_000_060_000);
        app.run_alerts();
        assert_eq!(
            app.alerts.as_ref().expect("已启用").log().total_seen(),
            1,
            "新出现的主机必须报，且只报那一台"
        );
    }

    #[test]
    fn muting_is_a_preference_and_does_not_flush_a_backlog_when_lifted() {
        let mut app = App::new(alert_opts());
        let mut cfg = cping_core::AlertConfig::default();
        cfg.with_kinds(&[cping_core::AlertKind::LatencyAbove]);
        for r in &mut cfg.rules {
            if r.kind == cping_core::AlertKind::LatencyAbove {
                r.threshold = 0;
                r.cooldown_secs = 60;
            }
        }
        app.alerts = Some(cping_core::AlertEngine::new(cfg));
        app.results = vec![Some(online(0, 271))];

        app.toggle_alert_mute();
        assert!(app.alerts.as_ref().expect("已启用").is_muted());

        app.round_at_ms = Some(1_700_000_000_000);
        app.run_alerts();
        let muted = app.alerts.as_ref().expect("已启用");
        assert_eq!(muted.log().total_seen(), 0, "静音期间不产生通知");
        assert!(
            muted.suppressed_by_mute() > 0,
            "静音的是「通知」而不是「求值」：抑制计数必须如实记录"
        );

        // 解除静音后，冷却窗口内的重复仍被挡下 → 不会一次性补报
        app.toggle_alert_mute();
        app.round_at_ms = Some(1_700_000_010_000);
        app.run_alerts();
        let after = app.alerts.as_ref().expect("已启用");
        assert_eq!(after.log().total_seen(), 0, "解除静音不该补报积压告警");
        assert!(
            after.suppressed_by_cooldown() > 0,
            "应是被冷却挡下，而不是规则失灵"
        );
    }

    #[test]
    fn alert_panel_renders_the_engine_state() {
        let mut app = App::new(alert_opts());
        app.prefix = "127.0.0".into();
        app.start_scan();
        wait_done(&mut app, 300);
        app.show_alerts = true;

        let text = render(&app, 120, 34);
        assert!(text.contains("告警"), "{text}");
        assert!(text.contains("延迟超阈"), "应列出启用的规则名：{text}");
        assert!(text.contains("生效中"), "{text}");
    }

    #[test]
    fn alert_flags_are_parsed_and_any_alert_flag_arms_the_engine() {
        use cping_core::AlertKind;
        let a = Options::from_args(
            [
                "--alert-latency",
                "50",
                "--alert-uptime",
                "80",
                "--alert-kinds",
                "latency,offline",
            ]
            .iter()
            .map(|s| s.to_string()),
        );
        assert!(a.alert, "给了任一 --alert-* 就应开启告警");
        let rule = |k: AlertKind| a.alert_config.rules.iter().find(|r| r.kind == k).cloned();
        assert_eq!(rule(AlertKind::LatencyAbove).unwrap().threshold, 50);
        assert_eq!(rule(AlertKind::UptimeBelow).unwrap().threshold, 80);
        assert!(rule(AlertKind::LatencyAbove).unwrap().enabled);
        assert!(
            rule(AlertKind::Disappeared).unwrap().enabled,
            "offline = 离线"
        );
        assert!(
            !rule(AlertKind::Appeared).unwrap().enabled,
            "没点名的要关掉"
        );

        let b = Options::from_args(["--mute-alerts"].iter().map(|s| s.to_string()));
        assert!(b.alert && b.alert_config.muted);

        // 拼错的类别名不改变默认全集（免得变成「一条也不报」）
        let c = Options::from_args(
            ["--alert-kinds", "nope,also-nope"]
                .iter()
                .map(|s| s.to_string()),
        );
        assert!(c.alert_config.rules.iter().all(|r| r.enabled));

        let d = Options::from_args(["--alert"].iter().map(|s| s.to_string()));
        assert!(d.alert_config.rules.iter().all(|r| r.enabled), "默认全集");
    }

    // ------------------------------------------------------------------
    // M17：安全策略
    // ------------------------------------------------------------------

    #[test]
    fn m17_tui_flags_build_the_policy_and_bad_values_are_reported() {
        let a = Options::from_args(
            [
                "--max-pps",
                "120",
                "--max-targets",
                "64",
                "--allow-reserved",
            ]
            .iter()
            .map(|s| s.to_string()),
        );
        assert_eq!(a.policy.max_pps, 120);
        assert_eq!(a.policy.max_targets, 64);
        assert!(a.policy.allow_reserved);
        assert!(a.arg_warnings.is_empty(), "{:?}", a.arg_warnings);

        // 取值不合法必须**说出来**，不能像没看见一样按默认值跑
        let b = Options::from_args(["--max-pps", "abc"].iter().map(|s| s.to_string()));
        assert_eq!(b.policy.max_pps, cping_core::DEFAULT_MAX_PPS);
        assert_eq!(b.arg_warnings.len(), 1);
        assert!(
            b.arg_warnings[0].contains("--max-pps"),
            "{:?}",
            b.arg_warnings
        );

        let c = Options::from_args(["--max-targets"].iter().map(|s| s.to_string()));
        assert_eq!(c.arg_warnings.len(), 1);
    }

    #[test]
    fn m17_policy_is_installed_on_the_scheduler() {
        let mut o = opts();
        o.policy.max_pps = 7;
        let app = App::new(o);
        assert_eq!(app.scheduler.policy().max_pps, 7);
    }

    #[test]
    fn m17_switching_probe_mode_keeps_the_policy() {
        // 切探测方式会重建调度器；策略要是没跟着装回去，一次切换就把限速丢了
        let mut o = opts();
        o.policy.max_pps = 9;
        let mut app = App::new(o);
        app.set_mode(ProbeMode::Icmp);
        assert_eq!(app.scheduler.policy().max_pps, 9);
    }

    #[test]
    fn m17_bad_policy_lands_in_the_status_bar_not_a_panic() {
        let mut o = opts();
        // 直接绕过命令行塞一个越界值（等价于从配置文件里读到的坏值）
        o.policy.max_pps = cping_core::MAX_PPS_LIMIT + 1;
        let app = App::new(o);
        assert!(
            app.status.contains("安全策略无效"),
            "坏策略必须看得见：{}",
            app.status
        );
    }

    // ------------------------------------------------------------------
    // M18：中 / 英双语
    // ------------------------------------------------------------------

    /// 与 `crate::i18n::tests::has_cjk` 同源（那边是私有的，这里再来一份小的）。
    fn has_cjk(s: &str) -> bool {
        s.chars().any(|c| {
            let u = c as u32;
            (0x3000..=0x303F).contains(&u)
                || (0x3040..=0x30FF).contains(&u)
                || (0x3400..=0x4DBF).contains(&u)
                || (0x4E00..=0x9FFF).contains(&u)
                || (0xF900..=0xFAFF).contains(&u)
                || (0xFF00..=0xFFEF).contains(&u)
        })
    }

    /// 英文界面用的选项（其余与 `opts()` 相同：模拟后端 + 临时快照目录）。
    fn en_opts() -> Options {
        Options {
            lang: Lang::En,
            ..opts()
        }
    }

    #[test]
    fn m18_lang_flag_is_parsed_and_bad_values_are_reported() {
        let arg = |s: &str| s.to_string();

        let a = Options::from_args(["--lang", "en"].iter().map(|s| arg(s)));
        assert_eq!(a.lang, Lang::En);
        assert!(a.arg_warnings.is_empty(), "{:?}", a.arg_warnings);

        // 接受 POSIX 风格的语言标识（与核心的 `Lang::parse` 同一套规则）
        let b = Options::from_args(["--lang", "zh_CN.UTF-8"].iter().map(|s| arg(s)));
        assert_eq!(b.lang, Lang::Zh);

        // 取值非法**必须报出来**：静默忽略用户显式给的语言是最难自查的一类问题。
        // 处理方式与既有策略参数一致——照常启动，把问题摆到状态栏。
        let c = Options::from_args(["--lang", "fr"].iter().map(|s| arg(s)));
        assert_eq!(c.arg_warnings.len(), 1, "{:?}", c.arg_warnings);
        assert!(c.arg_warnings[0].contains("--lang"), "{:?}", c.arg_warnings);

        // 缺取值同样要报
        let d = Options::from_args(["--lang"].iter().map(|s| arg(s)));
        assert_eq!(d.arg_warnings.len(), 1, "{:?}", d.arg_warnings);

        // 不给 `--lang` 时按环境推断（这里只断言它是个合法值，具体取值取决于
        // 运行测试的 shell，不能写死）。
        let e = Options::from_args(std::iter::empty::<String>());
        assert!(Lang::ALL.contains(&e.lang));
    }

    #[test]
    fn m18_english_status_messages_and_filters_are_english() {
        let mut app = App::new(en_opts());
        assert!(
            !has_cjk(&app.status),
            "英文模式的首屏状态必须是英文：{}",
            app.status
        );

        // 交互产生的状态栏文案也要跟着语言走
        app.on_key(KeyEvent::new(
            ratatui::crossterm::event::KeyCode::Char('i'),
            ratatui::crossterm::event::KeyModifiers::NONE,
        ));
        assert!(!has_cjk(&app.status), "编辑提示必须是英文：{}", app.status);
        app.on_key(KeyEvent::new(
            ratatui::crossterm::event::KeyCode::Char('f'),
            ratatui::crossterm::event::KeyModifiers::NONE,
        ));
        assert!(!has_cjk(&app.status), "过滤提示必须是英文：{}", app.status);

        // 非法输入走的是**核心的错误**：外框翻了，核心给出的 detail 原样透传
        app.prefix = "999.999.999".into();
        app.start_scan();
        assert!(!app.running);
        assert!(
            app.status.starts_with("invalid input"),
            "核心错误的外框必须本地化：{}",
            app.status
        );
    }

    /// 英文模式下，**典型界面**（主界面 / 表格 / 各面板的空态）不得出现中日韩字符。
    ///
    /// 「典型」是有意圈定的：设备推断（`DeviceGuess`）与告警正文（`AlertEvent::message`）
    /// 由**核心**生成，核心没有 `_in` 变体，界面又不能复刻核心的判定与文案，
    /// 所以它们在英文模式下仍是中文——这是本次改动的已知边界（已在报告里列出）。
    /// 面板的空态、标题、表头、图例、按键帮助全部是本界面自己的话，必须干净。
    #[test]
    fn m18_english_typical_screens_have_no_cjk() {
        // 主界面：输入区 + 状态栏 + 矩阵 + 按键条
        let app = App::new(en_opts());
        let main = render(&app, 130, 34);
        assert!(!has_cjk(&main), "英文主界面出现中文：{main}");

        // 帮助面板（含后端能力行——那两行由本界面按注册表公开字段拼）
        let mut help_app = App::new(en_opts());
        help_app.show_help = true;
        let help = render(&help_app, 130, 64);
        assert!(!has_cjk(&help), "英文帮助面板出现中文：{help}");

        // 面板空态
        let mut empty = App::new(en_opts());
        empty.show_changes = true;
        let changes = render(&empty, 120, 34);
        assert!(!has_cjk(&changes), "英文变化面板出现中文：{changes}");

        let mut trend = App::new(en_opts());
        trend.show_trend = true;
        let trend_out = render(&trend, 120, 34);
        assert!(!has_cjk(&trend_out), "英文趋势面板出现中文：{trend_out}");

        let mut fps = App::new(en_opts());
        fps.show_fingerprints = true;
        let fps_out = render(&fps, 120, 34);
        assert!(!has_cjk(&fps_out), "英文指纹面板出现中文：{fps_out}");

        let mut alerts = App::new(Options {
            alert: true,
            ..en_opts()
        });
        alerts.show_alerts = true;
        let alerts_out = render(&alerts, 120, 34);
        assert!(!has_cjk(&alerts_out), "英文告警面板出现中文：{alerts_out}");
    }

    /// 英文模式下的**数据视图**：状态列、分类列、详情行也得是英文。
    #[test]
    fn m18_english_table_view_has_no_cjk() {
        let mut app = App::new(en_opts());
        app.prefix = "127.0.0".into();
        app.start_scan();
        wait_done(&mut app, 300);
        app.view = ViewMode::Table;
        // 详情行的标签也要跟着语言走（造数据，不依赖本机 ARP / DNS）
        for r in app.results.iter_mut().flatten() {
            r.vendor = Some("Acme Networks".into());
            r.hostname = Some("router.local".into());
        }
        // 状态栏里的「邻居表」失败原因来自操作系统（中文 Windows 上就是中文），
        // 本用例考的是表格本身，所以先把状态栏清空，避开 OS 细节这个变量。
        app.status.clear();
        let out = render(&app, 120, 34);
        assert!(!has_cjk(&out), "英文表格视图出现中文：{out}");
        assert!(out.contains("vendor"), "详情行标签应是英文：{out}");
        assert!(out.contains("host"), "详情行标签应是英文：{out}");
    }

    /// `Options::default()` 必须**不读环境**、恒定中文。
    ///
    /// 测试全都建立在它之上（读过环境的默认值会让同一个用例在不同 shell 下
    /// 得到不同语言）；读环境这件事只发生在 `Options::from_args` 里。
    #[test]
    fn m18_options_default_is_deterministic_chinese() {
        assert_eq!(Options::default().lang, Lang::Zh);
    }
}
