//! 命令行参数解析（手写，零依赖）。
//!
//! 之所以不引 `clap`：本项目的核心约束是**零第三方运行时依赖**，
//! 而 CLI 需要的参数面很小。手写解析配上全覆盖单测，比拉一个 3 万行的
//! 参数库更划算，也更容易保证「非法输入一定报错而不是被忽略」。

use crate::i18n::{self, Key};
use cping_core::config::{MAX_CONCURRENCY, MAX_TIMEOUT_MS};
use cping_core::export::ExportFormat;
use cping_core::i18n::Lang;
use cping_core::spec::ProbeMode;
use cping_core::target::TargetRange;

/// 参数错误（用法错误）退出码。
pub const EXIT_OK: i32 = 0;
/// 有目标不在线。
pub const EXIT_OFFLINE: i32 = 1;
/// 参数/配置错误。
pub const EXIT_USAGE: i32 = 2;
/// 探测后端不可用（权限不足或平台不支持）。
pub const EXIT_BACKEND: i32 = 3;

/// M14：时序默认保留天数。`--keep-days` 的默认值，也是「有没有被显式改过」的判据。
pub const DEFAULT_KEEP_DAYS: u64 = 7;

/// 帮助文本（按语言取）。
///
/// 中英两份正文放在同目录的 `help_zh.txt` / `help_en.txt` 里，用 `include_str!`
/// 在编译期嵌入：帮助文本很长，直接写进 `i18n.rs` 的字典表会把它挤得没法读。
/// 它们仍然是 `&'static str` 条目，一样受「英文不得含中日韩字符」那条测试约束。
pub fn help(lang: Lang) -> &'static str {
    i18n::text(lang, Key::Help)
}

/// 决定本次运行的语言：`--lang` 优先，否则用调用方给的兜底（通常是环境推断）。
///
/// 单独扫一遍 argv 而不是塞进主循环，是因为语言决定了**后面所有**错误信息
/// 用哪种语言渲染——包括「`--lang` 自己取值非法」这一条。多次给 `--lang` 时
/// **最后一次生效**（与主循环保持一致），非法取值一律报错、绝不静默忽略。
pub fn resolve_lang(argv: &[String], fallback: Lang) -> Result<Lang, String> {
    let mut found: Option<Lang> = None;
    let mut i = 0;
    while i < argv.len() {
        let raw = argv[i].as_str();
        let (flag, inline) = match raw.split_once('=') {
            Some((k, v)) if k.starts_with("--") => (k, Some(v)),
            _ => (raw, None),
        };
        if flag == "--lang" {
            let value = match inline {
                Some(v) => v.to_string(),
                None => {
                    i += 1;
                    argv.get(i)
                        .cloned()
                        .ok_or_else(|| i18n::render(fallback, Key::ErrMissingValue, &["--lang"]))?
                }
            };
            found = Some(parse_lang_value(fallback, &value)?);
        }
        i += 1;
    }
    Ok(found.unwrap_or(fallback))
}

/// 解析 `--lang` 的取值。非法取值用 `fallback` 语言报错（那个值本身不能信）。
fn parse_lang_value(fallback: Lang, value: &str) -> Result<Lang, String> {
    // 用 `Debug` 形式裹上引号：空串或带空格的取值在报错里看得见
    let shown = format!("{value:?}");
    Lang::parse(value).ok_or_else(|| i18n::render(fallback, Key::ErrInvalidLang, &[&shown]))
}

/// 解析后的命令行参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub target_spec: String,
    pub range: TargetRange,
    pub mode: ProbeMode,
    pub timeout_ms: u64,
    pub concurrency: usize,
    pub enrich: bool,
    /// M13：对在线主机做端口指纹。
    pub fingerprint: bool,
    /// M13：指纹端口列表；空表示用核心内置的常用集。
    pub fp_ports: Vec<u16>,
    /// M13：是否抓取服务横幅（默认抓，只读）。
    pub fp_grab_banner: bool,
    /// M13：指纹单端口超时。
    pub fp_timeout_ms: u64,
    pub watch_seconds: Option<u64>,
    pub rounds: Option<u64>,
    /// M14：长期时序文件路径（跨会话累积）。
    pub history: Option<String>,
    /// M14：可用率报表输出路径（.csv / .html）。
    pub report: Option<String>,
    /// M14：时序保留天数。
    pub keep_days: u64,
    /// M15：开启告警（阈值规则 + 通知通道）。
    pub alert: bool,
    /// M15：启用的规则类型（`--alert-kinds`）；空 = 全部。
    pub alert_kinds: Vec<cping_core::AlertKind>,
    /// M15：延迟阈值（毫秒）。
    pub alert_latency_ms: u32,
    /// M15：可用率阈值（百分数）。
    pub alert_uptime_pct: u32,
    /// M15：冷却窗口（秒）。
    pub alert_cooldown_secs: u64,
    /// M15：告警去重状态文件（跨会话保留冷却记录）。
    pub alert_state: Option<String>,
    /// M15：静音（规则照常求值，只是不通知）。
    pub alert_muted: bool,
    /// M15：通知通道选择（`--notify`）。
    pub notify_channels: Vec<String>,
    /// M15：webhook 地址（`http://`）。
    pub notify_webhook: Option<String>,
    /// M15：通知命令模板（可重复）。
    pub notify_cmds: Vec<String>,
    /// M15：SMTP `host[:port]`。
    pub notify_smtp: Option<String>,
    /// M15：邮件发件人。
    pub notify_mail_from: Option<String>,
    /// M15：邮件收件人。
    pub notify_mail_to: Vec<String>,
    /// M15：只列出通知通道可用性后退出。
    pub notify_list: bool,
    pub out: Option<(ExportFormat, String)>,
    pub quiet: bool,
    /// M12：只列出后端能力后退出。
    pub list_backends: bool,
    /// M16：配置文件路径（`--config`）。
    pub config: Option<String>,
    /// M16：选用的模板名（`--profile`）。
    pub profile: Option<String>,
    /// M16：只列出配置文件里的模板后退出。
    pub list_profiles: bool,
    /// M16：把生效配置写到该路径后退出。
    pub dump_config: Option<String>,
    /// M17：全局速率上限（探测包/秒）。`0` = 不限速。
    pub max_pps: u32,
    /// M17：单次任务的目标数上限（只能比结构上限更严）。
    pub max_targets: usize,
    /// M17：放行组播 / 保留段等特殊网段。
    pub allow_reserved: bool,
    /// M17：审计日志路径（JSONL 追加写）。
    pub audit: Option<String>,
    /// M16：从 `--config` 读到的文档（已迁移/已校验）。
    pub config_doc: Option<cping_core::ConfigDocument>,
    /// M16：配置相关的说明（迁移、字段来源、被命令行覆盖的项）。
    ///
    /// 主程序**必须把这串说明原样打印**：模板到底改了哪些参数、
    /// 哪一条命令行参数盖掉了模板的值，都要说清楚。
    /// 「我的模板明明设了 60 秒超时」这类疑问只能靠它解答。
    pub config_notes: Vec<String>,
    pub help: bool,
    pub version: bool,
    /// 本次解析采用的界面语言（`--lang` 显式给，否则由环境推断）。
    ///
    /// 放在 `Args` 上而不是每次现算：`describe()` / `validate()` 与解析期的
    /// 错误信息必须用**同一种**语言，否则同一次调用里会中英混排。
    pub lang: Lang,
    /// 显式给出的 `--target`；用于判断「什么都没给」与「给了空串」的区别。
    pub target_given: bool,
    /// M16：以下几项是否被命令行**显式**给过（用于「模板只填空缺」）。
    ///
    /// `pub(crate)` 而非 `pub`：这是解析过程中的记账，不是给外部读写的接口。
    /// 构造 `Args` 时留 `false` 就等于「命令行没给过」，语义是安全的。
    pub(crate) seen_range: bool,
    pub(crate) seen_mode: bool,
    pub(crate) seen_timeout: bool,
    pub(crate) seen_concurrency: bool,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            target_spec: String::new(),
            range: TargetRange::Standard,
            mode: cping_core::platform_default_mode(),
            timeout_ms: cping_core::DEFAULT_TIMEOUT_MS,
            concurrency: cping_core::DEFAULT_CONCURRENCY,
            enrich: false,
            fingerprint: false,
            fp_ports: Vec::new(),
            fp_grab_banner: true,
            fp_timeout_ms: 800,
            watch_seconds: None,
            rounds: None,
            history: None,
            report: None,
            keep_days: DEFAULT_KEEP_DAYS,
            alert: false,
            alert_kinds: Vec::new(),
            alert_latency_ms: cping_core::DEFAULT_LATENCY_THRESHOLD_MS,
            alert_uptime_pct: cping_core::DEFAULT_UPTIME_THRESHOLD_PCT,
            alert_cooldown_secs: cping_core::DEFAULT_COOLDOWN_SECS,
            alert_state: None,
            alert_muted: false,
            notify_channels: Vec::new(),
            notify_webhook: None,
            notify_cmds: Vec::new(),
            notify_smtp: None,
            notify_mail_from: None,
            notify_mail_to: Vec::new(),
            notify_list: false,
            out: None,
            quiet: false,
            list_backends: false,
            max_pps: cping_core::DEFAULT_MAX_PPS,
            max_targets: cping_core::MAX_TARGETS,
            allow_reserved: false,
            audit: None,
            config: None,
            profile: None,
            list_profiles: false,
            dump_config: None,
            config_doc: None,
            config_notes: Vec::new(),
            help: false,
            version: false,
            // 与核心层同样的兜底：没有线索时中文。真正生效的语言在
            // `parse_with` 里由 `--lang` / 环境决定。
            lang: Lang::Zh,
            target_given: false,
            seen_range: false,
            seen_mode: false,
            seen_timeout: false,
            seen_concurrency: false,
        }
    }
}

impl Args {
    /// 解析参数（不含程序名）。返回 `Err` 即为用法错误。
    ///
    /// 语言按「`--lang` 显式给 → 环境推断」决定（见 [`Lang::from_env`]）。
    pub fn parse(argv: &[String]) -> Result<Self, String> {
        Self::parse_with(argv, Lang::from_env())
    }

    /// 同 [`Args::parse`]，但把「环境推断」这一步的结果作为参数传进来。
    ///
    /// 拆出这一层是为了**可测**：测试直接给 `Lang::Zh` / `Lang::En`，
    /// 就不必读写进程环境变量（cargo test 多线程跑，环境变量会互相污染）。
    pub fn parse_with(argv: &[String], env_lang: Lang) -> Result<Self, String> {
        // 语言必须在**任何错误信息产生之前**定下来：包括「--lang 自己取值非法」
        // 这一条——它只能用环境推断出来的语言（那个非法值不能信）。
        let mut args = Args {
            lang: resolve_lang(argv, env_lang)?,
            ..Args::default()
        };
        let lang = args.lang;
        let mut i = 0;
        while i < argv.len() {
            let raw = argv[i].as_str();
            // 支持 --key=value
            let (flag, inline) = match raw.split_once('=') {
                Some((k, v)) if k.starts_with("--") => (k, Some(v.to_string())),
                _ => (raw, None),
            };

            let mut take_value = |name: &str| -> Result<String, String> {
                if let Some(v) = inline.clone() {
                    return Ok(v);
                }
                i += 1;
                argv.get(i)
                    .cloned()
                    .ok_or_else(|| i18n::render(lang, Key::ErrMissingValue, &[name]))
            };

            match flag {
                "-h" | "--help" => args.help = true,
                "-V" | "--version" => args.version = true,
                "-q" | "--quiet" => args.quiet = true,
                "--list-backends" => args.list_backends = true,
                "--enrich" => args.enrich = true,
                "--fingerprint" | "--fp" => args.fingerprint = true,
                "--no-banner" => args.fp_grab_banner = false,
                "--lang" => {
                    // 取值已在 `resolve_lang` 里校验过；这里再定一次，让
                    // 「`--lang` 出现多次时最后一次生效」与它保持一致，
                    // 同时**绝不静默忽略**非法取值。
                    let v = take_value("--lang")?;
                    args.lang = parse_lang_value(lang, &v)?;
                }
                "--fp-ports" => {
                    let v = take_value("--fp-ports")?;
                    // 用 `_in(lang)`：端口列表的解析错误也是给人看的一句话，
                    // 英文界面下不该冒出中文。外层的「--fp-ports is invalid: 」
                    // 是 CLI 自己的框（`Key::ErrFpPorts`），内层由核心按语言给。
                    let ports = cping_core::PortScanConfig::parse_ports_in(&v, lang)
                        .map_err(|e| i18n::render(lang, Key::ErrFpPorts, &[&e]))?;
                    args.fp_ports = ports;
                }
                "--fp-timeout" => {
                    let v = parse_u64("--fp-timeout", &take_value("--fp-timeout")?, lang)?;
                    if v == 0 || v > MAX_TIMEOUT_MS {
                        return Err(i18n::render(
                            lang,
                            Key::ErrFpTimeoutRange,
                            &[&MAX_TIMEOUT_MS.to_string()],
                        ));
                    }
                    args.fp_timeout_ms = v;
                }
                "-t" | "--target" => {
                    let v = take_value("--target")?;
                    if args.target_given && !args.target_spec.is_empty() {
                        args.target_spec.push(',');
                    }
                    args.target_spec.push_str(v.trim());
                    args.target_given = true;
                }
                "--target-file" => {
                    let path = take_value("--target-file")?;
                    let text = std::fs::read_to_string(&path).map_err(|e| {
                        i18n::render(
                            lang,
                            Key::ErrTargetFileRead,
                            &[&format!("{path:?}"), &e.to_string()],
                        )
                    })?;
                    let cleaned: String = text
                        .lines()
                        .map(|l| l.trim())
                        .filter(|l| !l.is_empty() && !l.starts_with('#'))
                        .collect::<Vec<_>>()
                        .join(",");
                    if args.target_given && !args.target_spec.is_empty() {
                        args.target_spec.push(',');
                    }
                    args.target_spec.push_str(&cleaned);
                    args.target_given = true;
                }
                "--range" => {
                    args.seen_range = true;
                    let value = take_value("--range")?.to_ascii_lowercase();
                    args.range = match value.as_str() {
                        "standard" => TargetRange::Standard,
                        "full" => TargetRange::Full,
                        "hosts_only" | "hosts-only" => TargetRange::HostsOnly,
                        other => {
                            return Err(i18n::render(
                                lang,
                                Key::ErrRangeInvalid,
                                &[&format!("{other:?}")],
                            ))
                        }
                    };
                }
                "--mode" => {
                    args.seen_mode = true;
                    let value = take_value("--mode")?.to_ascii_lowercase();
                    match value.as_str() {
                        "icmp" => args.mode = ProbeMode::Icmp,
                        "tcp" => {
                            let port = args.mode.tcp_port().unwrap_or(80);
                            args.mode = ProbeMode::Tcp { port };
                        }
                        other => {
                            return Err(i18n::render(
                                lang,
                                Key::ErrModeInvalid,
                                &[&format!("{other:?}")],
                            ))
                        }
                    }
                }
                "-p" | "--port" => {
                    let port = parse_u16("--port", &take_value("--port")?, lang)?;
                    if port == 0 {
                        return Err(i18n::text(lang, Key::ErrPortZero).to_string());
                    }
                    args.seen_mode = true;
                    args.mode = ProbeMode::Tcp { port };
                }
                "--timeout" => {
                    args.seen_timeout = true;
                    let v = parse_u64("--timeout", &take_value("--timeout")?, lang)?;
                    if v == 0 || v > MAX_TIMEOUT_MS {
                        return Err(i18n::render(
                            lang,
                            Key::ErrTimeoutRange,
                            &[&MAX_TIMEOUT_MS.to_string()],
                        ));
                    }
                    args.timeout_ms = v;
                }
                "-c" | "--concurrency" => {
                    args.seen_concurrency = true;
                    let v = parse_u64("--concurrency", &take_value("--concurrency")?, lang)?;
                    if v == 0 || v > MAX_CONCURRENCY as u64 {
                        return Err(i18n::render(
                            lang,
                            Key::ErrConcurrencyRange,
                            &[&MAX_CONCURRENCY.to_string()],
                        ));
                    }
                    args.concurrency = v as usize;
                }
                "-w" | "--watch" => {
                    let v = parse_u64("--watch", &take_value("--watch")?, lang)?;
                    if v == 0 {
                        return Err(i18n::text(lang, Key::ErrWatchZero).to_string());
                    }
                    args.watch_seconds = Some(v);
                }
                "-n" | "--rounds" => {
                    let v = parse_u64("--rounds", &take_value("--rounds")?, lang)?;
                    if v == 0 {
                        return Err(i18n::text(lang, Key::ErrRoundsZero).to_string());
                    }
                    args.rounds = Some(v);
                }
                "--history" => args.history = Some(take_value("--history")?),
                "--report" => {
                    let path = take_value("--report")?;
                    let lower = path.to_ascii_lowercase();
                    if !lower.ends_with(".csv") && !lower.ends_with(".html") {
                        return Err(i18n::text(lang, Key::ErrReportFormat).to_string());
                    }
                    args.report = Some(path);
                }
                "--keep-days" => {
                    let v = parse_u64("--keep-days", &take_value("--keep-days")?, lang)?;
                    if v == 0 || v > 3650 {
                        return Err(i18n::text(lang, Key::ErrKeepDaysRange).to_string());
                    }
                    args.keep_days = v;
                }
                "--alert" => args.alert = true,
                "--alert-kinds" => {
                    let raw = take_value("--alert-kinds")?;
                    let mut kinds = Vec::new();
                    for part in raw.split(',') {
                        let part = part.trim();
                        if part.is_empty() {
                            continue;
                        }
                        // 未知名字必须报错：静默忽略会让人以为规则开了，其实没开
                        let kind = cping_core::AlertKind::parse_key(part).ok_or_else(|| {
                            i18n::render(lang, Key::ErrAlertKindsUnknown, &[&format!("{part:?}")])
                        })?;
                        if !kinds.contains(&kind) {
                            kinds.push(kind);
                        }
                    }
                    if kinds.is_empty() {
                        return Err(i18n::text(lang, Key::ErrAlertKindsEmpty).to_string());
                    }
                    args.alert_kinds = kinds;
                    args.alert = true;
                }
                "--alert-latency" => {
                    let v = parse_u32("--alert-latency", &take_value("--alert-latency")?, lang)?;
                    args.alert_latency_ms = v;
                    args.alert = true;
                }
                "--alert-uptime" => {
                    let v = parse_u32("--alert-uptime", &take_value("--alert-uptime")?, lang)?;
                    if v > 100 {
                        return Err(i18n::text(lang, Key::ErrAlertUptimeRange).to_string());
                    }
                    args.alert_uptime_pct = v;
                    args.alert = true;
                }
                "--alert-cooldown" => {
                    let v = parse_u64("--alert-cooldown", &take_value("--alert-cooldown")?, lang)?;
                    args.alert_cooldown_secs = v;
                    args.alert = true;
                }
                "--alert-state" => {
                    args.alert_state = Some(take_value("--alert-state")?);
                    args.alert = true;
                }
                "--mute-alerts" => {
                    args.alert_muted = true;
                    args.alert = true;
                }
                "--notify" => {
                    let raw = take_value("--notify")?;
                    for part in raw.split(',') {
                        let p = part.trim().to_ascii_lowercase();
                        if p.is_empty() {
                            continue;
                        }
                        if !matches!(
                            p.as_str(),
                            "console" | "system" | "webhook" | "command" | "smtp"
                        ) {
                            return Err(i18n::render(
                                lang,
                                Key::ErrNotifyUnknown,
                                &[&format!("{p:?}")],
                            ));
                        }
                        if !args.notify_channels.contains(&p) {
                            args.notify_channels.push(p);
                        }
                    }
                    args.alert = true;
                }
                "--notify-webhook" => {
                    args.notify_webhook = Some(take_value("--notify-webhook")?);
                    args.alert = true;
                }
                "--notify-cmd" => {
                    args.notify_cmds.push(take_value("--notify-cmd")?);
                    args.alert = true;
                }
                "--notify-smtp" => {
                    args.notify_smtp = Some(take_value("--notify-smtp")?);
                    args.alert = true;
                }
                "--notify-mail-from" => {
                    args.notify_mail_from = Some(take_value("--notify-mail-from")?);
                    args.alert = true;
                }
                "--notify-mail-to" => {
                    let raw = take_value("--notify-mail-to")?;
                    for part in raw.split(',') {
                        let p = part.trim();
                        if !p.is_empty() && !args.notify_mail_to.iter().any(|x| x == p) {
                            args.notify_mail_to.push(p.to_string());
                        }
                    }
                    args.alert = true;
                }
                "--notify-list" => args.notify_list = true,
                "--max-pps" => {
                    args.max_pps = parse_u32("--max-pps", &take_value("--max-pps")?, lang)?;
                }
                "--max-targets" => {
                    args.max_targets =
                        parse_u64("--max-targets", &take_value("--max-targets")?, lang)? as usize;
                }
                "--allow-reserved" => args.allow_reserved = true,
                "--audit" => args.audit = Some(take_value("--audit")?),
                "--config" => args.config = Some(take_value("--config")?),
                "--profile" => args.profile = Some(take_value("--profile")?),
                "--list-profiles" => args.list_profiles = true,
                "--dump-config" => args.dump_config = Some(take_value("--dump-config")?),
                "-o" | "--out" => {
                    let path = take_value("--out")?;
                    let format = ExportFormat::from_path(&path).ok_or_else(|| {
                        i18n::render(lang, Key::ErrOutExtension, &[&format!("{path:?}")])
                    })?;
                    args.out = Some((format, path));
                }
                "--json" => {
                    let path = take_value("--json")?;
                    args.out = Some((ExportFormat::Json, path));
                }
                other => {
                    return Err(i18n::render(
                        lang,
                        Key::ErrUnknownFlag,
                        &[&format!("{other:?}")],
                    ))
                }
            }
            i += 1;
        }
        args.apply_config()?;
        Ok(args)
    }

    /// M16：把 `--config` / `--profile` 合并进来。
    ///
    /// 规则只有一条，且**明确打印**：**命令行显式给过的参数永远赢**，
    /// 模板只填空缺。模板盖掉用户在命令行上手敲的值是最难查的一类问题，
    /// 所以这里不采用「模板优先」；被盖掉的配置值也会一并说出来。
    ///
    /// 优先级（高 → 低）：命令行 → 模板 → 配置文件的 `[probe]`。
    fn apply_config(&mut self) -> Result<(), String> {
        let lang = self.lang;
        if self.list_profiles && self.config.is_none() {
            return Err(i18n::text(lang, Key::ErrListProfilesNeedsConfig).to_string());
        }
        if self.profile.is_some() && self.config.is_none() {
            return Err(i18n::text(lang, Key::ErrProfileNeedsConfig).to_string());
        }
        let Some(path) = self.config.clone() else {
            return Ok(());
        };
        // `--help` / `--version` 不该因为配置文件有问题就报错
        if self.help || self.version {
            return Ok(());
        }

        // 配置层的错误是核心层产出的：用 `localized` 拿对应语言的句子，
        // 不在 CLI 里复刻它的判定逻辑。
        let (doc, notes) =
            cping_core::ConfigDocument::load_from_path(&path).map_err(|e| e.localized(lang))?;
        self.config_notes.extend(notes);

        let selected = doc
            .select_profile(self.profile.as_deref())
            .map_err(|e| i18n::render(lang, Key::ErrConfigPrefix, &[&path, &e.localized(lang)]))?
            .cloned();
        match &selected {
            Some(p) => self.config_notes.push(i18n::render(
                lang,
                Key::NoteConfigUseProfile,
                &[&path, &p.name, &p.spec],
            )),
            None => self
                .config_notes
                .push(i18n::render(lang, Key::NoteConfigNoProfile, &[&path])),
        }

        // ---- 目标规格：命令行 > 模板 ----
        if let Some(p) = &selected {
            if self.target_given && !self.target_spec.trim().is_empty() {
                // 值一样就不必打扰——同一件事说两遍会把真正重要的说明淹掉
                // （与 range/timeout 那几个字段同一套判据）。
                if self.target_spec.trim() != p.spec.trim() {
                    self.config_notes.push(i18n::render(
                        lang,
                        Key::NoteCliTargetOverridesProfile,
                        &[&p.name, &format!("{:?}", p.spec)],
                    ));
                }
            } else {
                self.target_spec = p.spec.clone();
                self.target_given = true;
                self.config_notes.push(i18n::render(
                    lang,
                    Key::NoteTargetFromProfile,
                    &[&p.name, &p.spec],
                ));
            }
        }

        // ---- 四个探测参数：逐字段决定来源，只报**最终生效**的那一层 ----
        let app_probe = doc.app.probe.clone();
        let profile_probe = selected.as_ref().map(|p| p.overrides());
        let mut from_profile: Vec<&str> = Vec::new();
        let mut from_app: Vec<&str> = Vec::new();
        let mut overridden: Vec<&str> = Vec::new();

        // range
        if self.seen_range {
            if conflict(range_from(&profile_probe, &app_probe), self.range) {
                overridden.push("--range");
            }
        } else if let Some(v) = profile_probe.map(|p| p.range) {
            self.range = v;
            from_profile.push("range");
        } else {
            self.range = app_probe.range;
            from_app.push("range");
        }
        // timeout
        if self.seen_timeout {
            if conflict(timeout_from(&profile_probe, &app_probe), self.timeout_ms) {
                overridden.push("--timeout");
            }
        } else if let Some(v) = profile_probe.map(|p| p.timeout_ms) {
            self.timeout_ms = v;
            from_profile.push("timeout");
        } else {
            self.timeout_ms = app_probe.timeout_ms;
            from_app.push("timeout");
        }
        // concurrency
        if self.seen_concurrency {
            if conflict(
                concurrency_from(&profile_probe, &app_probe),
                self.concurrency,
            ) {
                overridden.push("--concurrency");
            }
        } else if let Some(v) = profile_probe.map(|p| p.concurrency) {
            self.concurrency = v;
            from_profile.push("concurrency");
        } else {
            self.concurrency = app_probe.concurrency;
            from_app.push("concurrency");
        }
        // mode
        if self.seen_mode {
            if conflict(mode_from(&profile_probe, &app_probe), self.mode) {
                overridden.push("--mode/--port");
            }
        } else if let Some(v) = profile_probe.map(|p| p.mode) {
            self.mode = v;
            from_profile.push("mode");
        } else {
            self.mode = app_probe.mode;
            from_app.push("mode");
        }

        if !from_profile.is_empty() {
            self.config_notes.push(i18n::render(
                lang,
                Key::NoteProbeFromProfile,
                &[&from_profile.join(" / ")],
            ));
        }
        if !from_app.is_empty() {
            self.config_notes.push(i18n::render(
                lang,
                Key::NoteProbeFromApp,
                &[&from_app.join(" / ")],
            ));
        }
        if !overridden.is_empty() {
            self.config_notes.push(i18n::render(
                lang,
                Key::NoteCliOverridesConfig,
                &[&overridden.join(" / ")],
            ));
        }

        if let Some(n) = &self.profile {
            if selected.is_none() {
                // `select_profile` 已经会在找不到时报错，这里只是兜底
                return Err(i18n::render(
                    lang,
                    Key::ErrNoSuchProfile,
                    &[&path, &format!("{n:?}")],
                ));
            }
        }
        self.config_doc = Some(doc);
        Ok(())
    }

    /// M17：把安全策略参数收敛成核心层能直接消费的一份策略。
    ///
    /// 这几项**刻意只走命令行**，不进配置文件：限速、目标上限、是否放行保留
    /// 网段都是「本机的合规策略」。让一份从别处拷来的模板能把限速改成
    /// 「不限速」，等于给了个绕过合规的暗门。
    pub fn policy(&self) -> cping_core::ScanPolicy {
        cping_core::ScanPolicy {
            max_pps: self.max_pps,
            max_targets: self.max_targets,
            allow_reserved: self.allow_reserved,
        }
    }

    /// 校验「必需项都在」，返回可执行的配置。
    ///
    /// 核心层来的错误一律 `localized(lang)`，**不**在这里复刻判定逻辑。
    pub fn validate(&self) -> Result<(), String> {
        let lang = self.lang;
        if self.help || self.version {
            return Ok(());
        }
        if !self.target_given || self.target_spec.trim().is_empty() {
            return Err(i18n::text(lang, Key::ErrMissingTarget).to_string());
        }
        self.mode.validate().map_err(|e| e.localized(lang))?;
        // M17：策略越界一律报错，不静默夹取（「我设了 1000 怎么还是 500」
        // 比报错难查得多）。放在这里能让 `--list-profiles` 等早退分支不受影响，
        // 同时保证坏参数走不到发包那一步。
        self.policy()
            .validate_in(lang)
            .map_err(|e| e.localized(lang))?;
        // M14：`--keep-days` 只在写时序文件时才有意义。静默忽略会让人以为
        // 「我明明设了 30 天，怎么还在按 7 天裁」——这类参数必须报错。
        if self.history.is_none() && self.keep_days != DEFAULT_KEEP_DAYS {
            return Err(i18n::text(lang, Key::ErrKeepDaysNeedsHistory).to_string());
        }
        self.validate_alert_flags()?;
        Ok(())
    }

    pub fn describe(&self) -> String {
        let lang = self.lang;
        // 头一段（探测方式 / 并发 / 超时）之后逐段拼「 · xxx」后缀。
        // 用 `push_str` 而不是一个大 `format!`：后缀段落各自由字典渲染，
        // 顺序与「有没有这一段」都在代码里一眼可见。
        let mut out = i18n::render(
            lang,
            Key::DescSummary,
            &[
                &self.mode.describe(),
                &self.concurrency.to_string(),
                &self.timeout_ms.to_string(),
            ],
        );
        // M17：只在限速**被改动**时才提一句。默认值写进每一行只会变成噪音，
        // 而「被关掉限速」是需要摆在明面上的事。
        if self.max_pps != cping_core::DEFAULT_MAX_PPS || self.allow_reserved {
            out.push_str(" · ");
            out.push_str(&self.policy().describe_in(lang));
        }
        // M16：把「这次用了哪份模板」写进摘要。模板决定了扫哪个网段，
        // 不写出来的话，同一份命令行换个默认模板就会扫出完全不同的结果。
        match (&self.config, &self.profile) {
            (Some(path), Some(name)) => out.push_str(&i18n::render(
                lang,
                Key::DescConfigWithProfile,
                &[path, name],
            )),
            (Some(path), None) => {
                out.push_str(&i18n::render(lang, Key::DescConfig, &[path]));
            }
            (None, _) => {}
        }
        if let Some(s) = self.watch_seconds {
            out.push_str(&i18n::render(lang, Key::DescWatch, &[&s.to_string()]));
        }
        if self.fingerprint {
            let ports = if self.fp_ports.is_empty() {
                i18n::render(
                    lang,
                    Key::DescFpCommonPorts,
                    &[&cping_core::DEFAULT_FINGERPRINT_PORTS.len().to_string()],
                )
            } else {
                i18n::render(
                    lang,
                    Key::DescFpListedPorts,
                    &[&self.fp_ports.len().to_string()],
                )
            };
            let banner = if self.fp_grab_banner {
                i18n::text(lang, Key::DescFpBanner)
            } else {
                ""
            };
            out.push_str(&i18n::render(lang, Key::DescFp, &[&ports, banner]));
        }
        // M14：时序与报表
        if self.history.is_some() {
            out.push_str(&i18n::render(
                lang,
                Key::DescHistory,
                &[&self.keep_days.to_string()],
            ));
        }
        if self.report.is_some() {
            out.push_str(i18n::text(lang, Key::DescReport));
        }
        // M15：告警与通知
        if self.alert {
            let kinds = if self.alert_kinds.is_empty() {
                i18n::text(lang, Key::AlertKindAll).to_string()
            } else {
                self.alert_kinds
                    .iter()
                    .map(|k| alert_kind_label(lang, *k))
                    .collect::<Vec<_>>()
                    .join("/")
            };
            let ch = if self.notify_channels.is_empty() {
                "console".to_string()
            } else {
                self.notify_channels.join("+")
            };
            let muted = if self.alert_muted {
                i18n::text(lang, Key::DescAlertMuted)
            } else {
                ""
            };
            out.push_str(&i18n::render(lang, Key::DescAlerts, &[&kinds, muted, &ch]));
        }
        out
    }

    /// M13：把 CLI 参数收敛成一个核心层能直接消费的指纹配置。
    pub fn fingerprint_config(&self) -> cping_core::PortScanConfig {
        let mut cfg = cping_core::PortScanConfig {
            timeout: std::time::Duration::from_millis(self.fp_timeout_ms),
            grab_banner: self.fp_grab_banner,
            ..Default::default()
        };
        if !self.fp_ports.is_empty() {
            cfg.ports = self.fp_ports.clone();
        }
        cfg.normalized()
    }

    /// M15：CLI 参数 → 核心的告警配置。
    pub fn alert_config(&self) -> cping_core::AlertConfig {
        let mut cfg = cping_core::AlertConfig {
            muted: self.alert_muted,
            ..Default::default()
        };
        for rule in &mut cfg.rules {
            rule.threshold = match rule.kind {
                cping_core::AlertKind::LatencyAbove => self.alert_latency_ms,
                cping_core::AlertKind::UptimeBelow => self.alert_uptime_pct,
                _ => rule.threshold,
            };
            rule.cooldown_secs = self.alert_cooldown_secs;
        }
        if !self.alert_kinds.is_empty() {
            cfg.with_kinds(&self.alert_kinds);
        }
        cfg
    }

    /// M15：CLI 参数 → 核心的通知配置。
    ///
    /// 规则只有两条，都好解释：
    /// 1. 给了 `--notify` → **就是**这几条通道（多一个不给、少一个不补）；
    /// 2. 没给 `--notify` → console，外加任何「给了参数」的通道。
    ///
    /// 第 2 条的意思是 `--notify-cmd …` 单独出现时**一定**会生效——
    /// 静默忽略用户给的东西是这个项目里最不能接受的一类 bug。
    pub fn notify_config(&self) -> cping_core::NotifyConfig {
        use cping_core::ChannelConfig;
        let explicit = !self.notify_channels.is_empty();
        let wants = |name: &str| self.notify_channels.iter().any(|c| c == name);
        let on = |name: &str| !explicit || wants(name);

        let mut channels: Vec<ChannelConfig> = Vec::new();
        if on("console") {
            channels.push(ChannelConfig::Console);
        }
        if wants("system") {
            channels.push(ChannelConfig::System {
                app_name: "CPing".into(),
            });
        }
        if on("webhook") {
            if let Some(url) = &self.notify_webhook {
                channels.push(ChannelConfig::Webhook {
                    url: url.clone(),
                    timeout_ms: cping_core::notify::DEFAULT_CHANNEL_TIMEOUT_MS,
                });
            }
        }
        if on("command") {
            for tpl in &self.notify_cmds {
                channels.push(ChannelConfig::Command {
                    template: tpl.clone(),
                    timeout_ms: cping_core::notify::DEFAULT_CHANNEL_TIMEOUT_MS,
                });
            }
        }
        if on("smtp") {
            if let Some(endpoint) = &self.notify_smtp {
                let (host, port) = match endpoint.rsplit_once(':') {
                    Some((h, p)) => (h.to_string(), p.parse::<u16>().unwrap_or(25)),
                    None => (endpoint.clone(), 25),
                };
                channels.push(ChannelConfig::Smtp {
                    account: cping_core::SmtpAccount {
                        host,
                        port,
                        from: self
                            .notify_mail_from
                            .clone()
                            .unwrap_or_else(|| "cping@localhost".into()),
                        to: self.notify_mail_to.clone(),
                        subject_prefix: "[CPing]".into(),
                    },
                    timeout_ms: cping_core::notify::DEFAULT_CHANNEL_TIMEOUT_MS,
                });
            }
        }
        cping_core::NotifyConfig { channels }
    }

    /// M15：告警相关的参数组合校验。
    ///
    /// 三类问题必须报错而不是静默降级：
    /// 1. 点名了通道却不给它的必填参数（会变成「什么也不发」）；
    /// 2. 给了某通道的参数却没在 `--notify` 里选上它（参数被默默丢掉）；
    /// 3. 参数依赖关系缺失（SMTP 没写收件人）。
    fn validate_alert_flags(&self) -> Result<(), String> {
        let lang = self.lang;
        let explicit = !self.notify_channels.is_empty();
        let wants = |name: &str| self.notify_channels.iter().any(|c| c == name);
        let on = |name: &str| !explicit || wants(name);

        if wants("webhook") && self.notify_webhook.is_none() {
            return Err(i18n::text(lang, Key::ErrNotifyWebhookNeedsUrl).to_string());
        }
        if wants("smtp") && self.notify_smtp.is_none() {
            return Err(i18n::text(lang, Key::ErrNotifySmtpNeedsHost).to_string());
        }
        if wants("command") && self.notify_cmds.is_empty() {
            return Err(i18n::text(lang, Key::ErrNotifyCommandNeedsCmd).to_string());
        }
        if self.notify_webhook.is_some() && !on("webhook") {
            return Err(i18n::text(lang, Key::ErrWebhookNotSelected).to_string());
        }
        if self.notify_smtp.is_some() && !on("smtp") {
            return Err(i18n::text(lang, Key::ErrSmtpNotSelected).to_string());
        }
        if !self.notify_cmds.is_empty() && !on("command") {
            return Err(i18n::text(lang, Key::ErrCommandNotSelected).to_string());
        }
        if self.notify_smtp.is_some() && self.notify_mail_to.is_empty() {
            return Err(i18n::text(lang, Key::ErrSmtpNeedsMailTo).to_string());
        }
        if !self.alert && self.alert_state.is_some() {
            return Err(i18n::text(lang, Key::ErrAlertStateNeedsAlert).to_string());
        }
        Ok(())
    }
}

/// 告警规则类型的**展示名**。
///
/// 核心层只提供 `AlertKind::label_zh()`，CLI 面向英文用户需要自己的展示名。
/// 这里做的只是「键 → 展示名」的映射，**不是**复刻核心层的判定逻辑：
/// 判定仍然完全在核心层按 [`cping_core::AlertKind`] 走。中文展示名与核心的
/// `label_zh` 保持一致，避免同一台机器上两个界面说法不同。
fn alert_kind_label(lang: Lang, kind: cping_core::AlertKind) -> String {
    let key = match kind {
        cping_core::AlertKind::Appeared => Key::AlertKindAppeared,
        cping_core::AlertKind::Disappeared => Key::AlertKindDisappeared,
        cping_core::AlertKind::LatencyAbove => Key::AlertKindLatency,
        cping_core::AlertKind::UptimeBelow => Key::AlertKindUptime,
    };
    i18n::text(lang, key).to_string()
}

/// 「命令行给的显式值」与「配置里同一字段的值」是否真的不同。
///
/// 相同就不必打扰用户——同一件事说两遍只会把真正重要的说明淹掉。
fn conflict<T: PartialEq>(from_config: T, from_cli: T) -> bool {
    from_config != from_cli
}

fn range_from(
    profile: &Option<cping_core::ProbeOverrides>,
    app: &cping_core::config::ProbeConfig,
) -> TargetRange {
    profile.map(|p| p.range).unwrap_or(app.range)
}

fn timeout_from(
    profile: &Option<cping_core::ProbeOverrides>,
    app: &cping_core::config::ProbeConfig,
) -> u64 {
    profile.map(|p| p.timeout_ms).unwrap_or(app.timeout_ms)
}

fn concurrency_from(
    profile: &Option<cping_core::ProbeOverrides>,
    app: &cping_core::config::ProbeConfig,
) -> usize {
    profile.map(|p| p.concurrency).unwrap_or(app.concurrency)
}

fn mode_from(
    profile: &Option<cping_core::ProbeOverrides>,
    app: &cping_core::config::ProbeConfig,
) -> ProbeMode {
    profile.map(|p| p.mode).unwrap_or(app.mode)
}

fn parse_u64(name: &str, text: &str, lang: Lang) -> Result<u64, String> {
    text.trim()
        .parse::<u64>()
        .map_err(|_| i18n::render(lang, Key::ErrExpectInteger, &[name, &format!("{text:?}")]))
}

fn parse_u16(name: &str, text: &str, lang: Lang) -> Result<u16, String> {
    let v = parse_u64(name, text, lang)?;
    if v > u16::MAX as u64 {
        return Err(i18n::render(lang, Key::ErrOutOfRange16, &[name]));
    }
    Ok(v as u16)
}

fn parse_u32(name: &str, text: &str, lang: Lang) -> Result<u32, String> {
    let v = parse_u64(name, text, lang)?;
    if v > u32::MAX as u64 {
        return Err(i18n::render(
            lang,
            Key::ErrOutOfRange32,
            &[name, &u32::MAX.to_string()],
        ));
    }
    Ok(v as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    /// 测试统一以中文为兜底解析。
    ///
    /// 显式固定语言（而不是走 `Args::parse` 的 `from_env`）是为了让断言**确定**：
    /// 宿主机 `LANG=en` 之类的环境变量不该改变测试结果。要测英文时用
    /// `Args::parse_with(&argv(...), Lang::Zh)` 并在 argv 里显式给 `--lang en`。
    fn parse(items: &[String]) -> Result<Args, String> {
        Args::parse_with(items, Lang::Zh)
    }

    #[test]
    fn defaults_are_the_documented_ones() {
        let a = parse(&argv(&["-t", "192.168.1"])).unwrap();
        assert_eq!(a.range, TargetRange::Standard);
        assert_eq!(a.timeout_ms, 1000);
        assert_eq!(a.concurrency, 32);
        assert!(!a.quiet);
        assert_eq!(a.out, None);
    }

    #[test]
    fn supports_both_spaced_and_inline_values() {
        let a = parse(&argv(&["--target=10.0.0.0/30", "--timeout=2500"])).unwrap();
        assert_eq!(a.target_spec, "10.0.0.0/30");
        assert_eq!(a.timeout_ms, 2500);
    }

    #[test]
    fn repeated_targets_are_concatenated() {
        let a = parse(&argv(&["-t", "10.0.0.1", "-t", "10.0.0.2"])).unwrap();
        assert_eq!(a.target_spec, "10.0.0.1,10.0.0.2");
    }

    #[test]
    fn mode_and_port_stay_consistent() {
        let a = parse(&argv(&["-t", "10.0.0.1", "--mode", "tcp"])).unwrap();
        assert_eq!(a.mode, ProbeMode::Tcp { port: 80 });
        // 先给 mode 再给 port：port 生效
        let b = parse(&argv(&["-t", "10.0.0.1", "--mode=tcp", "-p", "445"])).unwrap();
        assert_eq!(b.mode, ProbeMode::Tcp { port: 445 });
        // 只给 port：隐含 TCP
        let c = parse(&argv(&["-t", "10.0.0.1", "-p", "22"])).unwrap();
        assert_eq!(c.mode, ProbeMode::Tcp { port: 22 });
        // 后面再写 icmp 可以切回去
        let d = parse(&argv(&["-t", "10.0.0.1", "-p", "22", "--mode", "icmp"])).unwrap();
        assert_eq!(d.mode, ProbeMode::Icmp);
    }

    #[test]
    fn range_accepts_three_spellings() {
        for (input, expect) in [
            ("standard", TargetRange::Standard),
            ("full", TargetRange::Full),
            ("hosts_only", TargetRange::HostsOnly),
            ("hosts-only", TargetRange::HostsOnly),
            ("FULL", TargetRange::Full),
        ] {
            let a = parse(&argv(&["-t", "10.0.0.1", "--range", input])).unwrap();
            assert_eq!(a.range, expect);
        }
        assert!(parse(&argv(&["-t", "10.0.0.1", "--range", "wat"])).is_err());
    }

    #[test]
    fn out_infers_format_from_extension() {
        let a = parse(&argv(&["-t", "10.0.0.1", "--out", "x/y/report.HTML"])).unwrap();
        assert_eq!(a.out, Some((ExportFormat::Html, "x/y/report.HTML".into())));
        let b = parse(&argv(&["-t", "10.0.0.1", "--json", "d.json"])).unwrap();
        assert_eq!(b.out, Some((ExportFormat::Json, "d.json".into())));
    }

    #[test]
    fn unknown_extension_for_out_is_rejected() {
        let err = parse(&argv(&["-t", "10.0.0.1", "--out", "x.xml"])).unwrap_err();
        assert!(err.contains("扩展名"), "{err}");
    }

    #[test]
    fn unknown_flags_are_rejected_not_ignored() {
        assert!(parse(&argv(&["--nope"])).is_err());
        assert!(parse(&argv(&["-t"])).is_err(), "缺值必须报错");
    }

    #[test]
    fn numeric_bounds_are_enforced() {
        assert!(parse(&argv(&["-t", "a", "--timeout", "0"])).is_err());
        assert!(parse(&argv(&["-t", "a", "--timeout", "60001"])).is_err());
        assert!(parse(&argv(&["-t", "a", "--timeout", "60000"])).is_ok());
        assert!(parse(&argv(&["-t", "a", "-c", "0"])).is_err());
        assert!(parse(&argv(&["-t", "a", "-c", "257"])).is_err());
        assert!(parse(&argv(&["-t", "a", "-c", "256"])).is_ok());
        assert!(parse(&argv(&["-t", "a", "-p", "0"])).is_err());
        assert!(parse(&argv(&["-t", "a", "-p", "65536"])).is_err());
        assert!(parse(&argv(&["-t", "a", "--timeout", "abc"])).is_err());
    }

    #[test]
    fn watch_and_rounds_are_validated() {
        let a = parse(&argv(&["-t", "a", "-w", "30", "-n", "3"])).unwrap();
        assert_eq!(a.watch_seconds, Some(30));
        assert_eq!(a.rounds, Some(3));
        assert!(parse(&argv(&["-t", "a", "-w", "0"])).is_err());
        assert!(parse(&argv(&["-t", "a", "-n", "0"])).is_err());
    }

    #[test]
    fn help_and_version_short_circuit_validation() {
        let a = parse(&argv(&["--help"])).unwrap();
        assert!(a.help);
        assert!(a.validate().is_ok(), "帮助不需要目标");
        let b = parse(&argv(&["-V"])).unwrap();
        assert!(b.version);
        assert!(b.validate().is_ok());
    }

    #[test]
    fn missing_target_is_a_usage_error() {
        let a = parse(&argv(&["--quiet"])).unwrap();
        let err = a.validate().unwrap_err();
        assert!(err.contains("缺少目标"), "{err}");
        // 显式给了空目标也算缺失
        let b = parse(&argv(&["-t", "  "])).unwrap();
        assert!(b.validate().is_err());
    }

    #[test]
    fn describe_mentions_mode_concurrency_and_timeout() {
        let a = parse(&argv(&[
            "-t",
            "a",
            "-p",
            "443",
            "-c",
            "64",
            "--timeout",
            "500",
        ]))
        .unwrap();
        let text = a.describe();
        assert!(text.contains("TCP:443"), "{text}");
        assert!(text.contains("64 并发"), "{text}");
        assert!(text.contains("500 ms"), "{text}");
        assert!(!text.contains("重扫"), "{text}");
    }

    #[test]
    fn fingerprint_flags_are_parsed_and_defaults_are_conservative() {
        let a = parse(&argv(&["-t", "10.0.0.1"])).unwrap();
        assert!(!a.fingerprint, "默认不做指纹");
        assert!(a.fp_grab_banner, "默认抓横幅（只读）");
        assert_eq!(a.fp_timeout_ms, 800);
        assert!(a.fp_ports.is_empty(), "默认用核心内置端口集");

        let b = parse(&argv(&[
            "-t",
            "10.0.0.1",
            "--fingerprint",
            "--fp-ports",
            "22,80,443",
            "--no-banner",
            "--fp-timeout",
            "1500",
        ]))
        .unwrap();
        assert!(b.fingerprint);
        assert_eq!(b.fp_ports, vec![22, 80, 443]);
        assert!(!b.fp_grab_banner);
        assert_eq!(b.fp_timeout_ms, 1500);
        // 配置收敛：CLI 参数 → 核心配置
        let cfg = b.fingerprint_config();
        assert_eq!(cfg.ports, vec![22, 80, 443]);
        assert!(!cfg.grab_banner);
        assert_eq!(cfg.timeout.as_millis(), 1500);

        // 短别名
        assert!(parse(&argv(&["-t", "a", "--fp"])).unwrap().fingerprint);
    }

    #[test]
    fn fingerprint_flag_errors_are_reported() {
        assert!(parse(&argv(&["-t", "a", "--fp-ports", "abc"])).is_err());
        assert!(parse(&argv(&["-t", "a", "--fp-ports", "0-5"])).is_err());
        assert!(parse(&argv(&["-t", "a", "--fp-timeout", "0"])).is_err());
        assert!(parse(&argv(&["-t", "a", "--fp-timeout", "99999"])).is_err());
    }

    #[test]
    fn describe_mentions_fingerprint_only_when_enabled() {
        let off = parse(&argv(&["-t", "a"])).unwrap();
        assert!(!off.describe().contains("指纹"));
        let on = parse(&argv(&["-t", "a", "--fingerprint"])).unwrap();
        let text = on.describe();
        assert!(text.contains("端口指纹"), "{text}");
        assert!(text.contains("含横幅"), "{text}");
        let no_banner = parse(&argv(&["-t", "a", "--fingerprint", "--no-banner"])).unwrap();
        assert!(!no_banner.describe().contains("含横幅"));
    }

    #[test]
    fn target_file_is_read_and_comments_are_skipped() {
        // 目录带进程号：固定名会让并行跑的两份测试共用同一目录，
        // 一方删文件、另一方正好在读时就会随机失败（见 `policy.rs` 同名说明）。
        let dir = std::env::temp_dir().join(format!("cping_cli_args_test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("targets.txt");
        std::fs::write(&path, "# 注释\n10.0.0.1\n\n10.0.0.2\n").unwrap();
        let a = parse(&argv(&[
            "--target-file",
            path.to_str().unwrap(),
            "-t",
            "10.0.0.3",
        ]))
        .unwrap();
        assert!(
            a.target_spec.starts_with("10.0.0.1,10.0.0.2"),
            "{}",
            a.target_spec
        );
        assert!(a.target_spec.ends_with(",10.0.0.3"), "{}", a.target_spec);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn missing_target_file_is_a_clear_error() {
        let err = parse(&argv(&["--target-file", "no/such/file"])).unwrap_err();
        assert!(err.contains("读取目标文件"), "{err}");
    }

    // ------------------------------------------------------------------
    // M14：长期时序与可用率报表
    // ------------------------------------------------------------------

    #[test]
    fn history_and_report_default_to_off() {
        let a = parse(&argv(&["-t", "10.0.0.1"])).unwrap();
        assert!(a.history.is_none(), "不指定就不落盘——没写文件是默认行为");
        assert!(a.report.is_none());
        assert_eq!(a.keep_days, DEFAULT_KEEP_DAYS);
        let text = a.describe();
        assert!(!text.contains("长期时序"), "{text}");
        assert!(!text.contains("报表"), "{text}");
    }

    #[test]
    fn history_report_and_keep_days_are_parsed() {
        let a = parse(&argv(&[
            "-t",
            "10.0.0.1",
            "--watch",
            "60",
            "--history",
            "net.ts",
            "--report",
            "uptime.html",
            "--keep-days",
            "30",
        ]))
        .unwrap();
        assert_eq!(a.history.as_deref(), Some("net.ts"));
        assert_eq!(a.report.as_deref(), Some("uptime.html"));
        assert_eq!(a.keep_days, 30);
        let text = a.describe();
        assert!(text.contains("长期时序"), "{text}");
        assert!(text.contains("30 天"), "{text}");
        assert!(text.contains("报表"), "{text}");
    }

    #[test]
    fn report_only_accepts_csv_or_html() {
        assert!(parse(&argv(&["-t", "a", "--report", "out.csv"])).is_ok());
        assert!(parse(&argv(&["-t", "a", "--report", "out.HTML"])).is_ok());
        let err = parse(&argv(&["-t", "a", "--report", "out.json"])).unwrap_err();
        assert!(err.contains("只支持"), "{err}");
    }

    #[test]
    fn keep_days_bounds_and_requires_history() {
        assert!(parse(&argv(&["-t", "a", "--history", "x", "--keep-days", "0"])).is_err());
        assert!(parse(&argv(&["-t", "a", "--history", "x", "--keep-days", "3651"])).is_err());
        assert!(parse(&argv(&["-t", "a", "--history", "x", "--keep-days", "3650"])).is_ok());

        // 单独给 --keep-days 是无效组合，必须报错而不是静默忽略
        let a = parse(&argv(&["-t", "a", "--keep-days", "30"])).unwrap();
        let err = a.validate().unwrap_err();
        assert!(err.contains("--history"), "{err}");
    }

    // ------------------------------------------------------------------
    // M15：告警与通知
    // ------------------------------------------------------------------

    #[test]
    fn alert_flags_default_to_off_and_are_parsed() {
        let a = parse(&argv(&["-t", "10.0.0.1"])).unwrap();
        assert!(!a.alert, "不指定 --alert 就不该有告警");
        assert!(a.alert_kinds.is_empty());
        assert_eq!(a.alert_latency_ms, cping_core::DEFAULT_LATENCY_THRESHOLD_MS);
        assert_eq!(a.alert_uptime_pct, cping_core::DEFAULT_UPTIME_THRESHOLD_PCT);
        assert!(a.notify_channels.is_empty());
        assert!(!a.describe().contains("告警"), "{}", a.describe());

        let b = parse(&argv(&[
            "-t",
            "10.0.0.1",
            "--alert",
            "--alert-kinds",
            "offline,latency",
            "--alert-latency",
            "80",
            "--alert-uptime",
            "99",
            "--alert-cooldown",
            "60",
            "--mute-alerts",
        ]))
        .unwrap();
        assert!(b.alert);
        assert_eq!(
            b.alert_kinds,
            vec![
                cping_core::AlertKind::Disappeared,
                cping_core::AlertKind::LatencyAbove
            ]
        );
        assert!(b.alert_muted);
        let cfg = b.alert_config();
        let lat = cfg.rule(cping_core::AlertKind::LatencyAbove).unwrap();
        assert!(lat.enabled);
        assert_eq!(lat.threshold, 80);
        assert_eq!(lat.cooldown_secs, 60);
        assert!(
            !cfg.rule(cping_core::AlertKind::Appeared).unwrap().enabled,
            "没选的规则要被停用"
        );
        assert!(cfg.muted);
        assert!(b.describe().contains("告警"), "{}", b.describe());
        assert!(b.describe().contains("已静音"), "{}", b.describe());
    }

    #[test]
    fn alert_flag_errors_are_reported_not_ignored() {
        assert!(parse(&argv(&["-t", "a", "--alert-kinds", "nope"])).is_err());
        assert!(parse(&argv(&["-t", "a", "--alert-kinds", ""])).is_err());
        assert!(parse(&argv(&["-t", "a", "--alert-uptime", "101"])).is_err());
        assert!(parse(&argv(&["-t", "a", "--alert-latency", "abc"])).is_err());
        // --alert-state 单独给没有意义
        let a = parse(&argv(&["-t", "a", "--alert-state", "s.txt"])).unwrap();
        assert!(a.alert, "--alert-state 本身即开启告警");
        let b = parse(&argv(&["-t", "a"])).unwrap();
        assert!(b.validate().is_ok());
    }

    #[test]
    fn notify_defaults_to_console_and_channel_params_are_collected() {
        let a = parse(&argv(&["-t", "a", "--alert"])).unwrap();
        assert_eq!(
            a.notify_config().keys(),
            vec!["console"],
            "默认只用 console"
        );

        let b = parse(&argv(&[
            "-t",
            "a",
            "--notify-cmd",
            "curl -s -d {json} http://x/",
            "--notify-cmd",
            "echo {count}",
        ]))
        .unwrap();
        assert_eq!(
            b.notify_config().keys(),
            vec!["console", "command", "command"],
            "没给 --notify 时：console + 任何给了参数的通道；命令模板可重复"
        );
        assert!(b.validate().is_ok());

        let c = parse(&argv(&[
            "-t",
            "a",
            "--notify",
            "webhook",
            "--notify-webhook",
            "http://10.0.0.5:9000/hook",
        ]))
        .unwrap();
        assert_eq!(c.notify_config().keys(), vec!["webhook"]);

        let d = parse(&argv(&[
            "-t",
            "a",
            "--notify",
            "smtp",
            "--notify-smtp",
            "mail.lan:2525",
            "--notify-mail-to",
            "ops@lan,boss@lan",
            "--notify-mail-from",
            "cping@lan",
        ]))
        .unwrap();
        assert_eq!(d.notify_config().keys(), vec!["smtp"]);
        match &d.notify_config().channels[0] {
            cping_core::ChannelConfig::Smtp { account, .. } => {
                assert_eq!(account.host, "mail.lan");
                assert_eq!(account.port, 2525);
                assert_eq!(account.to, vec!["ops@lan", "boss@lan"]);
                assert_eq!(account.from, "cping@lan");
            }
            other => panic!("应为 smtp 通道：{other:?}"),
        }
    }

    #[test]
    fn notify_channel_misuse_is_a_usage_error() {
        // 点名了通道却不给必填参数 → 报错，别静默变成「什么也不发」
        let a = parse(&argv(&["-t", "a", "--notify", "webhook"])).unwrap();
        assert!(a.validate().unwrap_err().contains("--notify-webhook"));

        let b = parse(&argv(&["-t", "a", "--notify", "smtp"])).unwrap();
        assert!(b.validate().unwrap_err().contains("--notify-smtp"));

        let c = parse(&argv(&["-t", "a", "--notify-smtp", "mail.lan"])).unwrap();
        assert!(c.validate().unwrap_err().contains("--notify-mail-to"));

        // 给了 webhook 地址却没在 --notify 里选上它
        let d = parse(&argv(&[
            "-t",
            "a",
            "--notify",
            "console",
            "--notify-webhook",
            "http://x/",
        ]))
        .unwrap();
        assert!(d.validate().unwrap_err().contains("webhook"));

        assert!(parse(&argv(&["-t", "a", "--notify", "telegram"])).is_err());
        assert!(
            parse(&argv(&["-t", "a", "--notify-list"]))
                .unwrap()
                .notify_list
        );
    }

    // ------------------------------------------------------------------
    // M16：配置文件与模板
    // ------------------------------------------------------------------

    /// 写一份临时配置，返回路径。
    ///
    /// 文件名由调用方指定（不用随机数、不引依赖），每个用例一个名字，
    /// 这样并行跑测试时不会互相踩同一个文件；目录本身再带上进程号，
    /// 让「同机并行跑两份测试」也互不干扰（见 `policy.rs` 同名说明）。
    fn write_config(name: &str, text: &str) -> String {
        let dir = std::env::temp_dir().join(format!("cping_cli_m16_test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, text).unwrap();
        path.to_str().unwrap().to_string()
    }

    const CFG: &str = r#"schema_version = 2
default_profile = "lan"

[probe]
prefix = ""
range = "standard"
timeout_ms = 1000
concurrency = 32
mode = "icmp"
tcp_port = 80

[[profiles]]
name = "lan"
spec = "10.1.0.0/30"
range = "hosts_only"
timeout_ms = 700
concurrency = 12
mode = "icmp"
tcp_port = 80
note = "办公网段"

[[profiles]]
name = "dmz"
spec = "10.2.0.0/30"
range = "standard"
timeout_ms = 2500
concurrency = 4
mode = "tcp"
tcp_port = 445
note = ""
"#;

    #[test]
    fn m16_profile_flags_require_config() {
        // 模板来自配置文件，单独给 --profile 只能是个误会，必须报错
        let e = parse(&argv(&["--profile", "lan"])).unwrap_err();
        assert!(e.contains("--config"), "{e}");
        let e = parse(&argv(&["--list-profiles"])).unwrap_err();
        assert!(e.contains("--config"), "{e}");
    }

    #[test]
    fn m16_missing_config_file_is_an_error_not_a_fallback() {
        let e = parse(&argv(&["--config", "no/such/cping.toml", "-t", "10.0.0.1"])).unwrap_err();
        assert!(e.contains("读取配置"), "{e}");
        // 路径拼错绝不能退化成「按默认值跑」——那是最难查的一类问题
        assert!(!e.contains("默认"), "{e}");
    }

    #[test]
    fn m16_profile_fills_target_and_probe_params() {
        let path = write_config("fill.toml", CFG);
        // 命令行一个探测参数都没给，全部由模板填空
        let a = parse(&argv(&["--config", path.as_str(), "--profile", "dmz"])).unwrap();
        assert_eq!(a.target_spec, "10.2.0.0/30");
        assert_eq!(a.timeout_ms, 2500);
        assert_eq!(a.concurrency, 4);
        assert_eq!(a.mode, ProbeMode::Tcp { port: 445 });
        assert_eq!(a.range, TargetRange::Standard);
        assert!(a.validate().is_ok(), "模板填完就该能直接跑");
    }

    #[test]
    fn m16_default_profile_is_used_when_profile_is_omitted() {
        let path = write_config("default.toml", CFG);
        let a = parse(&argv(&["--config", path.as_str()])).unwrap();
        assert_eq!(
            a.target_spec, "10.1.0.0/30",
            "没给 --profile 时用 default_profile"
        );
        assert_eq!(a.timeout_ms, 700);
        assert_eq!(a.concurrency, 12);
        assert_eq!(a.range, TargetRange::HostsOnly);
    }

    #[test]
    fn m16_cli_values_always_win_over_the_profile() {
        let path = write_config("cli-wins.toml", CFG);
        let a = parse(&argv(&[
            "--config",
            path.as_str(),
            "--profile",
            "dmz",
            "-t",
            "10.9.9.0/30",
            "--timeout",
            "1500",
            "-c",
            "9",
            "--range",
            "full",
        ]))
        .unwrap();
        assert_eq!(a.target_spec, "10.9.9.0/30");
        assert_eq!(a.timeout_ms, 1500);
        assert_eq!(a.concurrency, 9);
        assert_eq!(a.range, TargetRange::Full);
        // mode 命令行没给 → 仍由模板提供
        assert_eq!(a.mode, ProbeMode::Tcp { port: 445 });
        // 「盖掉了什么」必须说出来，否则用户会以为模板没生效
        let notes = a.config_notes.join("\n");
        assert!(notes.contains("盖掉了模板"), "{notes}");
        assert!(notes.contains("盖掉了配置里的对应值"), "{notes}");
    }

    #[test]
    fn m16_same_value_is_not_reported_as_a_conflict() {
        // 命令行给的值与模板一致时说「盖掉了」纯属噪音，会把真正的说明淹掉
        let path = write_config("same.toml", CFG);
        let a = parse(&argv(&[
            "--config",
            path.as_str(),
            "--profile",
            "lan",
            "-t",
            "10.1.0.0/30",
            "--timeout",
            "700",
        ]))
        .unwrap();
        let notes = a.config_notes.join("\n");
        assert!(!notes.contains("盖掉了模板"), "{notes}");
        assert!(!notes.contains("盖掉了配置里的对应值"), "{notes}");
    }

    #[test]
    fn m16_unknown_profile_is_an_error_listing_what_exists() {
        let path = write_config("unknown.toml", CFG);
        let e = parse(&argv(&["--config", path.as_str(), "--profile", "prod"])).unwrap_err();
        assert!(e.contains("没有名为 \"prod\" 的模板"), "{e}");
        // 报错要顺手列出现有模板，否则用户只能去翻文件
        assert!(e.contains("lan") && e.contains("dmz"), "{e}");
    }

    #[test]
    fn m16_notes_explain_where_each_field_came_from() {
        let path = write_config("notes.toml", CFG);
        let a = parse(&argv(&["--config", path.as_str()])).unwrap();
        let notes = a.config_notes.join("\n");
        assert!(notes.contains("使用模板 lan"), "{notes}");
        assert!(notes.contains("目标规格取自模板"), "{notes}");
        assert!(notes.contains("模板提供："), "{notes}");
    }

    #[test]
    fn m16_v1_config_is_migrated_and_still_readable() {
        // v1 的写法只有 [probe]，`prefix` 就是目标
        let v1 = "schema_version = 1\n\n[probe]\nprefix = \"10.5.0\"\nrange = \"full\"\n\
                  timeout_ms = 900\nconcurrency = 48\nmode = \"icmp\"\ntcp_port = 80\n";
        let path = write_config("v1.toml", v1);
        let a = parse(&argv(&["--config", path.as_str()])).unwrap();
        assert_eq!(
            a.target_spec, "10.5.0",
            "v1 的 [probe].prefix 要顶上来当目标"
        );
        assert_eq!(a.range, TargetRange::Full);
        assert_eq!(a.timeout_ms, 900);
        assert_eq!(a.concurrency, 48);
        let notes = a.config_notes.join("\n");
        assert!(notes.contains("升级到 v2"), "迁移必须说出来：{notes}");
    }

    #[test]
    fn m16_newer_schema_is_refused_not_guessed() {
        let path = write_config("newer.toml", "schema_version = 99\n");
        let e = parse(&argv(&["--config", path.as_str(), "-t", "10.0.0.1"])).unwrap_err();
        assert!(e.contains("拒绝读取"), "{e}");
    }

    #[test]
    fn m16_broken_config_does_not_break_help_or_version() {
        // 配置坏了也得能查用法——否则用户连「怎么改」都看不到
        let path = write_config("broken-help.toml", "这不是 TOML\n");
        assert!(
            parse(&argv(&["--config", path.as_str(), "--help"]))
                .unwrap()
                .help
        );
        assert!(
            parse(&argv(&["--config", path.as_str(), "-V"]))
                .unwrap()
                .version
        );
    }

    #[test]
    fn m16_help_documents_the_new_flags() {
        for flag in ["--config", "--profile", "--list-profiles", "--dump-config"] {
            assert!(help(Lang::Zh).contains(flag), "帮助里没有 {flag}");
        }
        assert!(
            help(Lang::Zh).contains("双方括号"),
            "要提醒 [[profiles]] 是双方括号"
        );
    }

    // ------------------------------------------------------------------
    // M17：安全策略与审计
    // ------------------------------------------------------------------

    #[test]
    fn m17_policy_defaults_are_limited_and_guarded() {
        let a = parse(&argv(&["-t", "10.0.0.1"])).unwrap();
        assert_eq!(a.max_pps, cping_core::DEFAULT_MAX_PPS);
        assert!(a.max_pps > 0, "默认必须限速，不能指望用户自觉");
        assert_eq!(a.max_targets, cping_core::MAX_TARGETS);
        assert!(!a.allow_reserved, "默认必须拦保留网段");
        assert!(a.audit.is_none(), "审计不默认开——默认往磁盘写文件不合适");
        // 默认值不进摘要（否则每一行都多一句噪音）
        assert!(!a.describe().contains("限速"), "{}", a.describe());
    }

    #[test]
    fn m17_policy_flags_are_parsed_and_shown_in_describe() {
        let a = parse(&argv(&[
            "-t",
            "10.0.0.1",
            "--max-pps",
            "0",
            "--allow-reserved",
        ]))
        .unwrap();
        assert_eq!(a.max_pps, 0);
        assert!(a.allow_reserved);
        let text = a.describe();
        assert!(text.contains("不限速"), "关掉限速必须摆在明面上：{text}");
        assert!(text.contains("允许保留网段"), "{text}");

        let b = parse(&argv(&["-t", "10.0.0.1", "--max-targets", "256"])).unwrap();
        assert_eq!(b.max_targets, 256);
        assert!(b.policy().validate().is_ok());
    }

    #[test]
    fn m17_out_of_range_policy_is_rejected_not_clamped() {
        // 越界的值一律报错：「我设了 1000 怎么还是 500」比报错难查得多
        let a = parse(&argv(&["-t", "10.0.0.1", "--max-pps", "100001"])).unwrap();
        let err = a.validate().unwrap_err();
        assert!(err.contains("100000"), "{err}");
        assert!(err.contains("关闭限速"), "{err}");

        let b = parse(&argv(&["-t", "10.0.0.1", "--max-targets", "70000"])).unwrap();
        let err = b.validate().unwrap_err();
        assert!(err.contains("只能收紧"), "{err}");

        let c = parse(&argv(&["-t", "10.0.0.1", "--max-targets", "0"])).unwrap();
        assert!(c.validate().is_err());

        // 非数字直接是解析错误
        assert!(parse(&argv(&["-t", "10.0.0.1", "--max-pps", "abc"])).is_err());
    }

    #[test]
    fn m17_reserved_targets_are_refused_with_a_reason_and_a_way_out() {
        let a = parse(&argv(&["-t", "224.0.0.0/29"])).unwrap();
        let spec = cping_core::parse_spec(&a.target_spec, a.range).unwrap();
        let err = a.policy().check_targets(&spec.targets).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("保留网段"), "{text}");
        assert!(text.contains("组播"), "{text}");
        // 放行之后就不再拦
        let b = parse(&argv(&["-t", "224.0.0.0/29", "--allow-reserved"])).unwrap();
        assert!(b.policy().check_targets(&spec.targets).is_ok());
    }

    #[test]
    fn m17_private_and_loopback_targets_still_work() {
        // 拦保留网段不能把工具的正常用途一起拦掉
        for spec_text in ["192.168.1", "10.0.0.0/30", "127.0.0.1"] {
            let a = parse(&argv(&["-t", spec_text])).unwrap();
            let spec = cping_core::parse_spec(&a.target_spec, a.range).unwrap();
            assert!(
                a.policy().check_targets(&spec.targets).is_ok(),
                "{spec_text} 不该被拦下"
            );
        }
    }

    #[test]
    fn m17_audit_path_is_collected_and_documented() {
        let a = parse(&argv(&["-t", "10.0.0.1", "--audit", "run.jsonl"])).unwrap();
        assert_eq!(a.audit.as_deref(), Some("run.jsonl"));
        assert!(help(Lang::Zh).contains("--audit"), "帮助里没有 --audit");
        for flag in ["--max-pps", "--max-targets", "--allow-reserved"] {
            assert!(help(Lang::Zh).contains(flag), "帮助里没有 {flag}");
        }
    }

    #[test]
    fn m16_describe_mentions_the_config_and_profile() {
        let path = write_config("describe.toml", CFG);
        let a = parse(&argv(&["--config", path.as_str(), "--profile", "dmz"])).unwrap();
        let text = a.describe();
        assert!(text.contains("模板 dmz"), "{text}");
        // 没给配置文件时不能凭空多出一段
        assert!(!parse(&argv(&["-t", "10.0.0.1"]))
            .unwrap()
            .describe()
            .contains("配置"));
    }

    // ------------------------------------------------------------------
    // --lang：中英双语
    // ------------------------------------------------------------------

    /// 中日韩字符与全角标点的范围（与 `i18n.rs` 里的检查同一套）。
    fn has_cjk(s: &str) -> Option<char> {
        s.chars().find(|c| {
            let u = *c as u32;
            (0x3000..=0x303F).contains(&u)
                || (0x3040..=0x30FF).contains(&u)
                || (0x3400..=0x4DBF).contains(&u)
                || (0x4E00..=0x9FFF).contains(&u)
                || (0xF900..=0xFAFF).contains(&u)
                || (0xFF00..=0xFFEF).contains(&u)
        })
    }

    #[test]
    fn lang_help_is_documented_in_both_languages() {
        // `--lang` 必须出现在中英两份帮助里，否则英文用户查不到怎么切语言
        for lang in Lang::ALL {
            let text = help(lang);
            assert!(text.contains("--lang"), "{lang} 的帮助里没有 --lang");
            assert!(text.contains("zh|en"), "{lang} 的帮助里没写取值");
        }
    }

    #[test]
    fn invalid_lang_is_rejected_not_ignored() {
        // 本项目最不能接受的一类 bug：静默忽略用户显式给的参数
        let e = Args::parse_with(&argv(&["--lang", "fr", "-t", "10.0.0.1"]), Lang::Zh).unwrap_err();
        assert!(e.contains("--lang"), "{e}");
        // 缺取值同样报错
        assert!(Args::parse_with(&argv(&["--lang"]), Lang::Zh).is_err());
        // `--lang=` 空串也拒绝
        assert!(Args::parse_with(&argv(&["--lang="]), Lang::Zh).is_err());
    }

    #[test]
    fn lang_accepts_locale_forms_and_last_one_wins() {
        let a = parse(&argv(&["--lang", "en_US.UTF-8", "-t", "10.0.0.1"])).unwrap();
        assert_eq!(a.lang, Lang::En);
        // 多次给：最后一次生效（与 `resolve_lang` 一致，不静默吞掉前一个）
        let b = parse(&argv(&["--lang", "en", "--lang", "zh", "-t", "10.0.0.1"])).unwrap();
        assert_eq!(b.lang, Lang::Zh);
    }

    #[test]
    fn env_fallback_is_used_when_lang_is_absent() {
        // 不给 --lang 时用调用方给的兜底（生产中来自 `Lang::from_env`）
        assert_eq!(parse(&argv(&["-t", "10.0.0.1"])).unwrap().lang, Lang::Zh);
        assert_eq!(
            Args::parse_with(&argv(&["-t", "10.0.0.1"]), Lang::En)
                .unwrap()
                .lang,
            Lang::En
        );
    }

    #[test]
    fn english_help_errors_and_summary_have_no_cjk() {
        // 要求的**最小覆盖**：帮助文本、参数错误消息、策略摘要
        assert!(has_cjk(help(Lang::En)).is_none(), "英文帮助含中日韩字符");

        for bad in [
            vec!["--lang", "en", "--timeout", "0"],
            vec!["--lang", "en", "--range", "wat"],
            vec!["--lang", "en", "--nope"],
            vec!["--lang", "en", "--notify", "telegram"],
            vec!["--lang", "en", "--fp-timeout", "99999"],
        ] {
            let e = Args::parse_with(&argv(&bad), Lang::Zh).unwrap_err();
            assert!(has_cjk(&e).is_none(), "英文参数错误含中日韩字符：{e}");
        }

        // 缺目标（validate 阶段）
        let a = Args::parse_with(&argv(&["--lang", "en", "--quiet"]), Lang::Zh).unwrap();
        let e = a.validate().unwrap_err();
        assert!(has_cjk(&e).is_none(), "英文校验错误含中日韩字符：{e}");

        // 策略摘要与策略错误（核心层的 describe_in / localized）
        let p = cping_core::ScanPolicy {
            max_pps: 0,
            max_targets: 100,
            allow_reserved: true,
        };
        let summary = p.describe_in(Lang::En);
        assert!(
            has_cjk(&summary).is_none(),
            "英文策略摘要含中日韩字符：{summary}"
        );
        let e = cping_core::ScanPolicy {
            max_pps: 100_001,
            max_targets: cping_core::MAX_TARGETS,
            allow_reserved: false,
        }
        .validate_in(Lang::En)
        .unwrap_err()
        .localized(Lang::En);
        assert!(has_cjk(&e).is_none(), "英文策略错误含中日韩字符：{e}");

        // 状态摘要（Args::describe）在英文下也不该出现中日韩字符
        let a = Args::parse_with(
            &argv(&[
                "--lang",
                "en",
                "-t",
                "10.0.0.1",
                "--max-pps",
                "0",
                "--allow-reserved",
                "--fingerprint",
                "--watch",
                "30",
                "--alert",
            ]),
            Lang::Zh,
        )
        .unwrap();
        let text = a.describe();
        assert!(has_cjk(&text).is_none(), "英文状态摘要含中日韩字符：{text}");
    }
}
