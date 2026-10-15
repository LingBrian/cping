//! 命令行参数解析（手写，零依赖）。
//!
//! 之所以不引 `clap`：本项目的核心约束是**零第三方运行时依赖**，
//! 而 CLI 需要的参数面很小。手写解析配上全覆盖单测，比拉一个 3 万行的
//! 参数库更划算，也更容易保证「非法输入一定报错而不是被忽略」。

use cping_core::config::{MAX_CONCURRENCY, MAX_TIMEOUT_MS};
use cping_core::export::ExportFormat;
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

pub const HELP: &str = r#"CPing 无头命令行 —— 批量 PING，结果可导出，退出码可判

用法：
  cping-cli --target <规格> [选项]

目标规格（三者可混用，逗号/空格/换行分隔）：
  192.168.1             C 类网段前三段（原版写法，默认 .1–.255 共 255 个）
  192.168.1.7           单个地址
  192.168.1.0/24        CIDR（必须写网络号；主机位非 0 会报错）
  10.0.0.0/30, 192.168.1

选项：
  -t, --target <规格>     目标规格（必需，除非设置了 --target-file）
      --target-file <f>   从文件读取目标规格（每行/逗号分隔）
      --range <r>         standard | full | hosts_only（默认 standard）
      --mode <m>          icmp | tcp（默认 icmp；Android 上默认 tcp）
  -p, --port <n>          TCP 模式端口（默认 80）
      --timeout <ms>      单目标超时（默认 1000，上限 60000）
  -c, --concurrency <n>   并发数（默认 32，上限 256）
      --enrich            扫描后补全 MAC 与厂商（读本机邻居表，离线 OUI 库）
      --fingerprint       对在线主机做端口指纹 + 设备类型推断（M13）
      --fp-ports <列表>   指纹探测的端口，如 22,80,443-445（默认内置常用集）
      --no-banner         指纹时**不**抓取服务横幅（默认抓，只读不写）
      --fp-timeout <ms>   指纹的单端口超时（默认 800，上限 60000）
  -w, --watch <秒>        持续监控：每 N 秒重扫一轮，并输出变化
  -n, --rounds <n>        watch 模式总轮数（默认一直跑，Ctrl+C 停止）
      --history <路径>    把每轮追加到长期时序文件（跨会话累积，默认保留 7 天）
      --report <路径>     结束时写出可用率报表（.csv 或 .html）
      --keep-days <天>    时序保留天数（默认 7；仅与 --history 同用）
      --alert             开启阈值告警（新增/离线/延迟/可用率）
      --alert-kinds <列表> 启用哪几类：appeared,disappeared,latency,uptime（默认全部）
      --alert-latency <ms> 延迟阈值（默认 200；严格大于才报）
      --alert-uptime <百分比> 可用率阈值（默认 90；严格小于才报，0-100）
      --alert-cooldown <秒> 同一主机同一类告警的冷却窗口（默认 300）
      --alert-state <路径> 告警去重状态文件（跨会话保留冷却记录）
      --mute-alerts       静音：规则照常求值、状态照常推进，只是不通知
      --notify <列表>     通知通道：console,system,webhook,command,smtp（默认 console）
      --notify-webhook <url> Webhook 地址（**仅 http://**，本构建不含 TLS）
      --notify-cmd <模板> 通知命令模板，可重复；{title}/{body}/{json}/{count} 占位
      --notify-smtp <host[:port]> 明文 SMTP 中继（不支持 TLS 与认证）
      --notify-mail-from <地址>   邮件发件人
      --notify-mail-to <地址,..>  邮件收件人
      --notify-list       列出本平台各通知通道的可用性后退出
      --out <路径>        导出结果；按扩展名决定格式（.csv/.txt/.json/.html）
      --json <路径>       等价于 --out 且强制 JSON
  -q, --quiet             不打印人类可读摘要（只输出文件与退出码）
      --list-backends     列出本平台可用的探测后端与能力（含不可用的），然后退出
      --config <路径>     读配置文件（模板 + [probe]/[colors]/[ui]/[monitor]）
      --profile <名>      选用配置里的哪一份模板（名字不区分大小写）
      --list-profiles     列出配置里的全部模板后退出（需同时给 --config）
      --dump-config <路径> 把**这次实际生效**的参数写成配置文件后退出
  -h, --help              显示本帮助
  -V, --version           显示版本

退出码：
  0  全部目标在线
  1  有目标不在线（超时/不可达/端口关闭）
  2  参数或配置错误
  3  探测后端不可用（权限不足 / 平台不支持）

示例：
  cping-cli -t 192.168.1
  cping-cli -t 192.168.1.0/24 --mode tcp --port 445 --out scan.csv
  cping-cli -t 10.0.0.0/30 --json report.json --enrich
  cping-cli -t 192.168.1 --fingerprint --out assets.csv
  cping-cli -t 192.168.1 --watch 30 --out rounds.json
  cping-cli -t 192.168.1 --watch 60 --history net.ts --report uptime.html
  cping-cli -t 192.168.1 --watch 30 --alert --notify console,system
  cping-cli -t 192.168.1 --watch 30 --alert --notify webhook \
            --notify-webhook http://10.0.0.5:9000/cping
  cping-cli -t 192.168.1 --alert --notify-cmd "curl -s -d {json} https://example.com/h"
  cping-cli --config prod.toml --profile dmz --out dmz.csv
  cping-cli --config prod.toml --list-profiles
  cping-cli -t 10.0.0.0/24 -c 64 --dump-config nightly.toml

关于 --config / --profile：
  配置文件把「扫什么、怎么扫」固化成一份份**命名模板**，CI 与巡检脚本复用同一份：
  写 [[profiles]] 段（**双方括号**；写成 [profiles] 会明确报错而不是被忽略）。
  合并优先级**高 → 低**：命令行 → 模板 → 配置的 [probe]。模板只填空缺，
  绝不覆盖你在命令行上手敲的值；每次都会把「哪些字段来自哪一层」逐条打印出来。
  --profile 点名的模板不存在会**报错退出**，不会悄悄退回 default_profile。
  配置文件带 schema_version：低版本的旧文件（v1 的 [probe] 写法）会被自动迁移，
  高版本会**拒绝读取**（猜着读会吃掉你不认识的项）。不认识的键原样保留，
  但**注释不保留**（子集解析器不建 AST）。
  --dump-config 只写「扫什么、怎么扫」：导出路径、--history、通知通道、告警规则
  刻意不写进去——否则换个模板会把通知一起换掉。

关于 --fingerprint：
  端口指纹是**独立于在线判定的第二趟探测**，只对在线主机做：
  端口全关不会把主机改成离线，端口开放也不会把离线主机变在线。
  服务横幅只读对方主动送出的前 256 字节，**不发送任何协议请求**；
  设备类型是「厂商 + 开放端口」的**推断**结论，识别不出就留空。

关于 --history / --report：
  时序文件带格式版本号（首行 CPING-TS <版本>）；版本不符会**拒绝读取**并退
  出码 2，绝不用猜出来的数据覆盖你已有的历史。每轮都立即落盘，Ctrl+C 也不
  丢已跑过的轮次。报表数值可逐条对账回原始记录：降采样只做同类项相加，
  观测轮数与 RTT 求和在合并前后守恒。注意「从未在线」的主机 RTT 记为 -，
  而不是 0（0 会被读成「极快」）。

关于 --alert / --notify：
  规则匹配是纯函数：第一轮只建立基线（不发「新增主机」，否则扫个 /24 会一次
  刷出 255 条）；阈值用严格不等号，恰好等于阈值不告警。通知通道**失败可见**：
  每个通道单独汇报成功或失败原因，一个通道挂了不影响其它通道，也绝不阻塞
  扫描主流程（投递在后台线程，队列满时丢弃并计数）。
  webhook 只支持 http:// —— 本构建不含 TLS，不会把告警明文降级发出去；
  需要 https 请用 --notify-cmd 调 curl 等自带 TLS 的工具。
"#;

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
            config: None,
            profile: None,
            list_profiles: false,
            dump_config: None,
            config_doc: None,
            config_notes: Vec::new(),
            help: false,
            version: false,
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
    pub fn parse(argv: &[String]) -> Result<Self, String> {
        let mut args = Args::default();
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
                    .ok_or_else(|| format!("{name} 缺少取值"))
            };

            match flag {
                "-h" | "--help" => args.help = true,
                "-V" | "--version" => args.version = true,
                "-q" | "--quiet" => args.quiet = true,
                "--list-backends" => args.list_backends = true,
                "--enrich" => args.enrich = true,
                "--fingerprint" | "--fp" => args.fingerprint = true,
                "--no-banner" => args.fp_grab_banner = false,
                "--fp-ports" => {
                    let v = take_value("--fp-ports")?;
                    let ports = cping_core::PortScanConfig::parse_ports(&v)
                        .map_err(|e| format!("--fp-ports 无效：{e}"))?;
                    args.fp_ports = ports;
                }
                "--fp-timeout" => {
                    let v = parse_u64("--fp-timeout", &take_value("--fp-timeout")?)?;
                    if v == 0 || v > MAX_TIMEOUT_MS {
                        return Err(format!("--fp-timeout 必须在 1..={MAX_TIMEOUT_MS} 之间"));
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
                    let text = std::fs::read_to_string(&path)
                        .map_err(|e| format!("读取目标文件 {path:?} 失败：{e}"))?;
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
                    args.range = match take_value("--range")?.to_ascii_lowercase().as_str() {
                        "standard" => TargetRange::Standard,
                        "full" => TargetRange::Full,
                        "hosts_only" | "hosts-only" => TargetRange::HostsOnly,
                        other => {
                            return Err(format!(
                                "--range {other:?} 无效，只能是 standard / full / hosts_only"
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
                        other => return Err(format!("--mode {other:?} 无效，只能是 icmp / tcp")),
                    }
                }
                "-p" | "--port" => {
                    let port = parse_u16("--port", &take_value("--port")?)?;
                    if port == 0 {
                        return Err("--port 不能为 0".into());
                    }
                    args.seen_mode = true;
                    args.mode = ProbeMode::Tcp { port };
                }
                "--timeout" => {
                    args.seen_timeout = true;
                    let v = parse_u64("--timeout", &take_value("--timeout")?)?;
                    if v == 0 || v > MAX_TIMEOUT_MS {
                        return Err(format!("--timeout 必须在 1..={MAX_TIMEOUT_MS} 之间"));
                    }
                    args.timeout_ms = v;
                }
                "-c" | "--concurrency" => {
                    args.seen_concurrency = true;
                    let v = parse_u64("--concurrency", &take_value("--concurrency")?)?;
                    if v == 0 || v > MAX_CONCURRENCY as u64 {
                        return Err(format!("--concurrency 必须在 1..={MAX_CONCURRENCY} 之间"));
                    }
                    args.concurrency = v as usize;
                }
                "-w" | "--watch" => {
                    let v = parse_u64("--watch", &take_value("--watch")?)?;
                    if v == 0 {
                        return Err("--watch 间隔必须大于 0 秒".into());
                    }
                    args.watch_seconds = Some(v);
                }
                "-n" | "--rounds" => {
                    let v = parse_u64("--rounds", &take_value("--rounds")?)?;
                    if v == 0 {
                        return Err("--rounds 必须大于 0".into());
                    }
                    args.rounds = Some(v);
                }
                "--history" => args.history = Some(take_value("--history")?),
                "--report" => {
                    let path = take_value("--report")?;
                    let lower = path.to_ascii_lowercase();
                    if !lower.ends_with(".csv") && !lower.ends_with(".html") {
                        return Err("--report 只支持 .csv 或 .html".into());
                    }
                    args.report = Some(path);
                }
                "--keep-days" => {
                    let v = parse_u64("--keep-days", &take_value("--keep-days")?)?;
                    if v == 0 || v > 3650 {
                        return Err("--keep-days 必须在 1..=3650 之间".into());
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
                        let kind = cping_core::AlertKind::parse_key(part)
                            .ok_or_else(|| format!("--alert-kinds 不认识 {part:?}（appeared/disappeared/latency/uptime）"))?;
                        if !kinds.contains(&kind) {
                            kinds.push(kind);
                        }
                    }
                    if kinds.is_empty() {
                        return Err("--alert-kinds 至少要给一类".into());
                    }
                    args.alert_kinds = kinds;
                    args.alert = true;
                }
                "--alert-latency" => {
                    let v = parse_u32("--alert-latency", &take_value("--alert-latency")?)?;
                    args.alert_latency_ms = v;
                    args.alert = true;
                }
                "--alert-uptime" => {
                    let v = parse_u32("--alert-uptime", &take_value("--alert-uptime")?)?;
                    if v > 100 {
                        return Err("--alert-uptime 是百分数，必须在 0..=100 之间".into());
                    }
                    args.alert_uptime_pct = v;
                    args.alert = true;
                }
                "--alert-cooldown" => {
                    let v = parse_u64("--alert-cooldown", &take_value("--alert-cooldown")?)?;
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
                            return Err(format!(
                                "--notify 不认识通道 {p:?}（console/system/webhook/command/smtp）"
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
                "--config" => args.config = Some(take_value("--config")?),
                "--profile" => args.profile = Some(take_value("--profile")?),
                "--list-profiles" => args.list_profiles = true,
                "--dump-config" => args.dump_config = Some(take_value("--dump-config")?),
                "-o" | "--out" => {
                    let path = take_value("--out")?;
                    let format = ExportFormat::from_path(&path).ok_or_else(|| {
                        format!("--out {path:?} 的扩展名无法识别，请用 .csv / .txt / .json / .html")
                    })?;
                    args.out = Some((format, path));
                }
                "--json" => {
                    let path = take_value("--json")?;
                    args.out = Some((ExportFormat::Json, path));
                }
                other => return Err(format!("未知参数 {other:?}（用 --help 查看用法）")),
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
        if self.list_profiles && self.config.is_none() {
            return Err("--list-profiles 需要同时给 --config".into());
        }
        if self.profile.is_some() && self.config.is_none() {
            return Err("--profile 需要同时给 --config（模板来自配置文件）".into());
        }
        let Some(path) = self.config.clone() else {
            return Ok(());
        };
        // `--help` / `--version` 不该因为配置文件有问题就报错
        if self.help || self.version {
            return Ok(());
        }

        let (doc, notes) =
            cping_core::ConfigDocument::load_from_path(&path).map_err(|e| e.to_string())?;
        self.config_notes.extend(notes);

        let selected = doc
            .select_profile(self.profile.as_deref())
            .map_err(|e| format!("配置 {path}：{e}"))?
            .cloned();
        match &selected {
            Some(p) => self
                .config_notes
                .push(format!("配置 {path}：使用模板 {}（{}）", p.name, p.spec)),
            None => self.config_notes.push(format!(
                "配置 {path}：没有选中模板，只取 [probe] 的探测参数"
            )),
        }

        // ---- 目标规格：命令行 > 模板 ----
        if let Some(p) = &selected {
            if self.target_given && !self.target_spec.trim().is_empty() {
                // 值一样就不必打扰——同一件事说两遍会把真正重要的说明淹掉
                // （与 range/timeout 那几个字段同一套判据）。
                if self.target_spec.trim() != p.spec.trim() {
                    self.config_notes.push(format!(
                        "命令行 --target 盖掉了模板 {} 的目标规格（模板里是 {:?}）",
                        p.name, p.spec
                    ));
                }
            } else {
                self.target_spec = p.spec.clone();
                self.target_given = true;
                self.config_notes
                    .push(format!("目标规格取自模板 {}：{}", p.name, p.spec));
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
            self.config_notes
                .push(format!("模板提供：{}", from_profile.join(" / ")));
        }
        if !from_app.is_empty() {
            self.config_notes
                .push(format!("配置 [probe] 提供：{}", from_app.join(" / ")));
        }
        if !overridden.is_empty() {
            self.config_notes.push(format!(
                "命令行 {} 盖掉了配置里的对应值",
                overridden.join(" / ")
            ));
        }

        if let Some(n) = &self.profile {
            if selected.is_none() {
                // `select_profile` 已经会在找不到时报错，这里只是兜底
                return Err(format!("配置 {path} 里没有模板 {n:?}"));
            }
        }
        self.config_doc = Some(doc);
        Ok(())
    }

    /// 校验「必需项都在」，返回可执行的配置。
    pub fn validate(&self) -> Result<(), String> {
        if self.help || self.version {
            return Ok(());
        }
        if !self.target_given || self.target_spec.trim().is_empty() {
            return Err("缺少目标：请用 --target 指定（例如 --target 192.168.1），\
                 或用 --config + --profile 选一份带目标的模板"
                .into());
        }
        self.mode.validate().map_err(|e| e.to_string())?;
        // M14：`--keep-days` 只在写时序文件时才有意义。静默忽略会让人以为
        // 「我明明设了 30 天，怎么还在按 7 天裁」——这类参数必须报错。
        if self.history.is_none() && self.keep_days != DEFAULT_KEEP_DAYS {
            return Err("--keep-days 仅在同时指定 --history 时有意义".into());
        }
        self.validate_alert_flags()?;
        Ok(())
    }

    pub fn describe(&self) -> String {
        let watch = match self.watch_seconds {
            Some(s) => format!(" · 每 {s} 秒重扫"),
            None => String::new(),
        };
        let fp = if self.fingerprint {
            let ports = if self.fp_ports.is_empty() {
                format!("{} 个常用端口", cping_core::DEFAULT_FINGERPRINT_PORTS.len())
            } else {
                format!("{} 个指定端口", self.fp_ports.len())
            };
            format!(
                " · 端口指纹（{ports}{}）",
                if self.fp_grab_banner {
                    "，含横幅"
                } else {
                    ""
                }
            )
        } else {
            String::new()
        };
        // M14：时序与报表
        let hist = if self.history.is_some() {
            format!(" · 长期时序（保留 {} 天）", self.keep_days)
        } else {
            String::new()
        };
        let rep = if self.report.is_some() {
            " · 结束时出可用率报表"
        } else {
            ""
        };
        // M15：告警与通知
        let alerts = if self.alert {
            let kinds = if self.alert_kinds.is_empty() {
                "全部".to_string()
            } else {
                self.alert_kinds
                    .iter()
                    .map(|k| k.label_zh())
                    .collect::<Vec<_>>()
                    .join("/")
            };
            let ch = if self.notify_channels.is_empty() {
                "console".to_string()
            } else {
                self.notify_channels.join("+")
            };
            format!(
                " · 告警（{kinds}）{} · 通知 {ch}",
                if self.alert_muted {
                    "［已静音］"
                } else {
                    ""
                }
            )
        } else {
            String::new()
        };
        // M16：把「这次用了哪份模板」写进摘要。模板决定了扫哪个网段，
        // 不写出来的话，同一份命令行换个默认模板就会扫出完全不同的结果。
        let cfg = match (&self.config, &self.profile) {
            (Some(path), Some(name)) => format!(" · 配置 {path}［模板 {name}］"),
            (Some(path), None) => format!(" · 配置 {path}"),
            (None, _) => String::new(),
        };
        format!(
            "{} · {} 并发 · 超时 {} ms{cfg}{watch}{fp}{hist}{rep}{alerts}",
            self.mode.describe(),
            self.concurrency,
            self.timeout_ms,
        )
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
        let explicit = !self.notify_channels.is_empty();
        let wants = |name: &str| self.notify_channels.iter().any(|c| c == name);
        let on = |name: &str| !explicit || wants(name);

        if wants("webhook") && self.notify_webhook.is_none() {
            return Err("--notify webhook 需要同时给 --notify-webhook <http://…>".into());
        }
        if wants("smtp") && self.notify_smtp.is_none() {
            return Err("--notify smtp 需要同时给 --notify-smtp <host[:port]>".into());
        }
        if wants("command") && self.notify_cmds.is_empty() {
            return Err("--notify command 需要同时给 --notify-cmd <模板>".into());
        }
        if self.notify_webhook.is_some() && !on("webhook") {
            return Err("给了 --notify-webhook 却没在 --notify 里选上 webhook".into());
        }
        if self.notify_smtp.is_some() && !on("smtp") {
            return Err("给了 --notify-smtp 却没在 --notify 里选上 smtp".into());
        }
        if !self.notify_cmds.is_empty() && !on("command") {
            return Err("给了 --notify-cmd 却没在 --notify 里选上 command".into());
        }
        if self.notify_smtp.is_some() && self.notify_mail_to.is_empty() {
            return Err("--notify-smtp 需要同时给 --notify-mail-to（至少一个收件人）".into());
        }
        if !self.alert && self.alert_state.is_some() {
            return Err("--alert-state 只在开启告警（--alert 或任一 --alert-*）时有意义".into());
        }
        Ok(())
    }
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

fn parse_u64(name: &str, text: &str) -> Result<u64, String> {
    text.trim()
        .parse::<u64>()
        .map_err(|_| format!("{name} 需要一个非负整数，得到 {text:?}"))
}

fn parse_u16(name: &str, text: &str) -> Result<u16, String> {
    let v = parse_u64(name, text)?;
    if v > u16::MAX as u64 {
        return Err(format!("{name} 超出 0-65535"));
    }
    Ok(v as u16)
}

fn parse_u32(name: &str, text: &str) -> Result<u32, String> {
    let v = parse_u64(name, text)?;
    if v > u32::MAX as u64 {
        return Err(format!("{name} 超出 0-{}", u32::MAX));
    }
    Ok(v as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn defaults_are_the_documented_ones() {
        let a = Args::parse(&argv(&["-t", "192.168.1"])).unwrap();
        assert_eq!(a.range, TargetRange::Standard);
        assert_eq!(a.timeout_ms, 1000);
        assert_eq!(a.concurrency, 32);
        assert!(!a.quiet);
        assert_eq!(a.out, None);
    }

    #[test]
    fn supports_both_spaced_and_inline_values() {
        let a = Args::parse(&argv(&["--target=10.0.0.0/30", "--timeout=2500"])).unwrap();
        assert_eq!(a.target_spec, "10.0.0.0/30");
        assert_eq!(a.timeout_ms, 2500);
    }

    #[test]
    fn repeated_targets_are_concatenated() {
        let a = Args::parse(&argv(&["-t", "10.0.0.1", "-t", "10.0.0.2"])).unwrap();
        assert_eq!(a.target_spec, "10.0.0.1,10.0.0.2");
    }

    #[test]
    fn mode_and_port_stay_consistent() {
        let a = Args::parse(&argv(&["-t", "10.0.0.1", "--mode", "tcp"])).unwrap();
        assert_eq!(a.mode, ProbeMode::Tcp { port: 80 });
        // 先给 mode 再给 port：port 生效
        let b = Args::parse(&argv(&["-t", "10.0.0.1", "--mode=tcp", "-p", "445"])).unwrap();
        assert_eq!(b.mode, ProbeMode::Tcp { port: 445 });
        // 只给 port：隐含 TCP
        let c = Args::parse(&argv(&["-t", "10.0.0.1", "-p", "22"])).unwrap();
        assert_eq!(c.mode, ProbeMode::Tcp { port: 22 });
        // 后面再写 icmp 可以切回去
        let d = Args::parse(&argv(&["-t", "10.0.0.1", "-p", "22", "--mode", "icmp"])).unwrap();
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
            let a = Args::parse(&argv(&["-t", "10.0.0.1", "--range", input])).unwrap();
            assert_eq!(a.range, expect);
        }
        assert!(Args::parse(&argv(&["-t", "10.0.0.1", "--range", "wat"])).is_err());
    }

    #[test]
    fn out_infers_format_from_extension() {
        let a = Args::parse(&argv(&["-t", "10.0.0.1", "--out", "x/y/report.HTML"])).unwrap();
        assert_eq!(a.out, Some((ExportFormat::Html, "x/y/report.HTML".into())));
        let b = Args::parse(&argv(&["-t", "10.0.0.1", "--json", "d.json"])).unwrap();
        assert_eq!(b.out, Some((ExportFormat::Json, "d.json".into())));
    }

    #[test]
    fn unknown_extension_for_out_is_rejected() {
        let err = Args::parse(&argv(&["-t", "10.0.0.1", "--out", "x.xml"])).unwrap_err();
        assert!(err.contains("扩展名"), "{err}");
    }

    #[test]
    fn unknown_flags_are_rejected_not_ignored() {
        assert!(Args::parse(&argv(&["--nope"])).is_err());
        assert!(Args::parse(&argv(&["-t"])).is_err(), "缺值必须报错");
    }

    #[test]
    fn numeric_bounds_are_enforced() {
        assert!(Args::parse(&argv(&["-t", "a", "--timeout", "0"])).is_err());
        assert!(Args::parse(&argv(&["-t", "a", "--timeout", "60001"])).is_err());
        assert!(Args::parse(&argv(&["-t", "a", "--timeout", "60000"])).is_ok());
        assert!(Args::parse(&argv(&["-t", "a", "-c", "0"])).is_err());
        assert!(Args::parse(&argv(&["-t", "a", "-c", "257"])).is_err());
        assert!(Args::parse(&argv(&["-t", "a", "-c", "256"])).is_ok());
        assert!(Args::parse(&argv(&["-t", "a", "-p", "0"])).is_err());
        assert!(Args::parse(&argv(&["-t", "a", "-p", "65536"])).is_err());
        assert!(Args::parse(&argv(&["-t", "a", "--timeout", "abc"])).is_err());
    }

    #[test]
    fn watch_and_rounds_are_validated() {
        let a = Args::parse(&argv(&["-t", "a", "-w", "30", "-n", "3"])).unwrap();
        assert_eq!(a.watch_seconds, Some(30));
        assert_eq!(a.rounds, Some(3));
        assert!(Args::parse(&argv(&["-t", "a", "-w", "0"])).is_err());
        assert!(Args::parse(&argv(&["-t", "a", "-n", "0"])).is_err());
    }

    #[test]
    fn help_and_version_short_circuit_validation() {
        let a = Args::parse(&argv(&["--help"])).unwrap();
        assert!(a.help);
        assert!(a.validate().is_ok(), "帮助不需要目标");
        let b = Args::parse(&argv(&["-V"])).unwrap();
        assert!(b.version);
        assert!(b.validate().is_ok());
    }

    #[test]
    fn missing_target_is_a_usage_error() {
        let a = Args::parse(&argv(&["--quiet"])).unwrap();
        let err = a.validate().unwrap_err();
        assert!(err.contains("缺少目标"), "{err}");
        // 显式给了空目标也算缺失
        let b = Args::parse(&argv(&["-t", "  "])).unwrap();
        assert!(b.validate().is_err());
    }

    #[test]
    fn describe_mentions_mode_concurrency_and_timeout() {
        let a = Args::parse(&argv(&[
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
        let a = Args::parse(&argv(&["-t", "10.0.0.1"])).unwrap();
        assert!(!a.fingerprint, "默认不做指纹");
        assert!(a.fp_grab_banner, "默认抓横幅（只读）");
        assert_eq!(a.fp_timeout_ms, 800);
        assert!(a.fp_ports.is_empty(), "默认用核心内置端口集");

        let b = Args::parse(&argv(&[
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
        assert!(
            Args::parse(&argv(&["-t", "a", "--fp"]))
                .unwrap()
                .fingerprint
        );
    }

    #[test]
    fn fingerprint_flag_errors_are_reported() {
        assert!(Args::parse(&argv(&["-t", "a", "--fp-ports", "abc"])).is_err());
        assert!(Args::parse(&argv(&["-t", "a", "--fp-ports", "0-5"])).is_err());
        assert!(Args::parse(&argv(&["-t", "a", "--fp-timeout", "0"])).is_err());
        assert!(Args::parse(&argv(&["-t", "a", "--fp-timeout", "99999"])).is_err());
    }

    #[test]
    fn describe_mentions_fingerprint_only_when_enabled() {
        let off = Args::parse(&argv(&["-t", "a"])).unwrap();
        assert!(!off.describe().contains("指纹"));
        let on = Args::parse(&argv(&["-t", "a", "--fingerprint"])).unwrap();
        let text = on.describe();
        assert!(text.contains("端口指纹"), "{text}");
        assert!(text.contains("含横幅"), "{text}");
        let no_banner = Args::parse(&argv(&["-t", "a", "--fingerprint", "--no-banner"])).unwrap();
        assert!(!no_banner.describe().contains("含横幅"));
    }

    #[test]
    fn target_file_is_read_and_comments_are_skipped() {
        let dir = std::env::temp_dir().join("cping_cli_args_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("targets.txt");
        std::fs::write(&path, "# 注释\n10.0.0.1\n\n10.0.0.2\n").unwrap();
        let a = Args::parse(&argv(&[
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
        let err = Args::parse(&argv(&["--target-file", "no/such/file"])).unwrap_err();
        assert!(err.contains("读取目标文件"), "{err}");
    }

    // ------------------------------------------------------------------
    // M14：长期时序与可用率报表
    // ------------------------------------------------------------------

    #[test]
    fn history_and_report_default_to_off() {
        let a = Args::parse(&argv(&["-t", "10.0.0.1"])).unwrap();
        assert!(a.history.is_none(), "不指定就不落盘——没写文件是默认行为");
        assert!(a.report.is_none());
        assert_eq!(a.keep_days, DEFAULT_KEEP_DAYS);
        let text = a.describe();
        assert!(!text.contains("长期时序"), "{text}");
        assert!(!text.contains("报表"), "{text}");
    }

    #[test]
    fn history_report_and_keep_days_are_parsed() {
        let a = Args::parse(&argv(&[
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
        assert!(Args::parse(&argv(&["-t", "a", "--report", "out.csv"])).is_ok());
        assert!(Args::parse(&argv(&["-t", "a", "--report", "out.HTML"])).is_ok());
        let err = Args::parse(&argv(&["-t", "a", "--report", "out.json"])).unwrap_err();
        assert!(err.contains("只支持"), "{err}");
    }

    #[test]
    fn keep_days_bounds_and_requires_history() {
        assert!(Args::parse(&argv(&["-t", "a", "--history", "x", "--keep-days", "0"])).is_err());
        assert!(Args::parse(&argv(&["-t", "a", "--history", "x", "--keep-days", "3651"])).is_err());
        assert!(Args::parse(&argv(&["-t", "a", "--history", "x", "--keep-days", "3650"])).is_ok());

        // 单独给 --keep-days 是无效组合，必须报错而不是静默忽略
        let a = Args::parse(&argv(&["-t", "a", "--keep-days", "30"])).unwrap();
        let err = a.validate().unwrap_err();
        assert!(err.contains("--history"), "{err}");
    }

    // ------------------------------------------------------------------
    // M15：告警与通知
    // ------------------------------------------------------------------

    #[test]
    fn alert_flags_default_to_off_and_are_parsed() {
        let a = Args::parse(&argv(&["-t", "10.0.0.1"])).unwrap();
        assert!(!a.alert, "不指定 --alert 就不该有告警");
        assert!(a.alert_kinds.is_empty());
        assert_eq!(a.alert_latency_ms, cping_core::DEFAULT_LATENCY_THRESHOLD_MS);
        assert_eq!(a.alert_uptime_pct, cping_core::DEFAULT_UPTIME_THRESHOLD_PCT);
        assert!(a.notify_channels.is_empty());
        assert!(!a.describe().contains("告警"), "{}", a.describe());

        let b = Args::parse(&argv(&[
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
        assert!(Args::parse(&argv(&["-t", "a", "--alert-kinds", "nope"])).is_err());
        assert!(Args::parse(&argv(&["-t", "a", "--alert-kinds", ""])).is_err());
        assert!(Args::parse(&argv(&["-t", "a", "--alert-uptime", "101"])).is_err());
        assert!(Args::parse(&argv(&["-t", "a", "--alert-latency", "abc"])).is_err());
        // --alert-state 单独给没有意义
        let a = Args::parse(&argv(&["-t", "a", "--alert-state", "s.txt"])).unwrap();
        assert!(a.alert, "--alert-state 本身即开启告警");
        let b = Args::parse(&argv(&["-t", "a"])).unwrap();
        assert!(b.validate().is_ok());
    }

    #[test]
    fn notify_defaults_to_console_and_channel_params_are_collected() {
        let a = Args::parse(&argv(&["-t", "a", "--alert"])).unwrap();
        assert_eq!(
            a.notify_config().keys(),
            vec!["console"],
            "默认只用 console"
        );

        let b = Args::parse(&argv(&[
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

        let c = Args::parse(&argv(&[
            "-t",
            "a",
            "--notify",
            "webhook",
            "--notify-webhook",
            "http://10.0.0.5:9000/hook",
        ]))
        .unwrap();
        assert_eq!(c.notify_config().keys(), vec!["webhook"]);

        let d = Args::parse(&argv(&[
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
        let a = Args::parse(&argv(&["-t", "a", "--notify", "webhook"])).unwrap();
        assert!(a.validate().unwrap_err().contains("--notify-webhook"));

        let b = Args::parse(&argv(&["-t", "a", "--notify", "smtp"])).unwrap();
        assert!(b.validate().unwrap_err().contains("--notify-smtp"));

        let c = Args::parse(&argv(&["-t", "a", "--notify-smtp", "mail.lan"])).unwrap();
        assert!(c.validate().unwrap_err().contains("--notify-mail-to"));

        // 给了 webhook 地址却没在 --notify 里选上它
        let d = Args::parse(&argv(&[
            "-t",
            "a",
            "--notify",
            "console",
            "--notify-webhook",
            "http://x/",
        ]))
        .unwrap();
        assert!(d.validate().unwrap_err().contains("webhook"));

        assert!(Args::parse(&argv(&["-t", "a", "--notify", "telegram"])).is_err());
        assert!(
            Args::parse(&argv(&["-t", "a", "--notify-list"]))
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
    /// 这样并行跑测试时不会互相踩同一个文件。
    fn write_config(name: &str, text: &str) -> String {
        let dir = std::env::temp_dir().join("cping_cli_m16_test");
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
        let e = Args::parse(&argv(&["--profile", "lan"])).unwrap_err();
        assert!(e.contains("--config"), "{e}");
        let e = Args::parse(&argv(&["--list-profiles"])).unwrap_err();
        assert!(e.contains("--config"), "{e}");
    }

    #[test]
    fn m16_missing_config_file_is_an_error_not_a_fallback() {
        let e =
            Args::parse(&argv(&["--config", "no/such/cping.toml", "-t", "10.0.0.1"])).unwrap_err();
        assert!(e.contains("读取配置"), "{e}");
        // 路径拼错绝不能退化成「按默认值跑」——那是最难查的一类问题
        assert!(!e.contains("默认"), "{e}");
    }

    #[test]
    fn m16_profile_fills_target_and_probe_params() {
        let path = write_config("fill.toml", CFG);
        // 命令行一个探测参数都没给，全部由模板填空
        let a = Args::parse(&argv(&["--config", path.as_str(), "--profile", "dmz"])).unwrap();
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
        let a = Args::parse(&argv(&["--config", path.as_str()])).unwrap();
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
        let a = Args::parse(&argv(&[
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
        let a = Args::parse(&argv(&[
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
        let e = Args::parse(&argv(&["--config", path.as_str(), "--profile", "prod"])).unwrap_err();
        assert!(e.contains("没有名为 \"prod\" 的模板"), "{e}");
        // 报错要顺手列出现有模板，否则用户只能去翻文件
        assert!(e.contains("lan") && e.contains("dmz"), "{e}");
    }

    #[test]
    fn m16_notes_explain_where_each_field_came_from() {
        let path = write_config("notes.toml", CFG);
        let a = Args::parse(&argv(&["--config", path.as_str()])).unwrap();
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
        let a = Args::parse(&argv(&["--config", path.as_str()])).unwrap();
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
        let e = Args::parse(&argv(&["--config", path.as_str(), "-t", "10.0.0.1"])).unwrap_err();
        assert!(e.contains("拒绝读取"), "{e}");
    }

    #[test]
    fn m16_broken_config_does_not_break_help_or_version() {
        // 配置坏了也得能查用法——否则用户连「怎么改」都看不到
        let path = write_config("broken-help.toml", "这不是 TOML\n");
        assert!(
            Args::parse(&argv(&["--config", path.as_str(), "--help"]))
                .unwrap()
                .help
        );
        assert!(
            Args::parse(&argv(&["--config", path.as_str(), "-V"]))
                .unwrap()
                .version
        );
    }

    #[test]
    fn m16_help_documents_the_new_flags() {
        for flag in ["--config", "--profile", "--list-profiles", "--dump-config"] {
            assert!(HELP.contains(flag), "帮助里没有 {flag}");
        }
        assert!(HELP.contains("双方括号"), "要提醒 [[profiles]] 是双方括号");
    }

    #[test]
    fn m16_describe_mentions_the_config_and_profile() {
        let path = write_config("describe.toml", CFG);
        let a = Args::parse(&argv(&["--config", path.as_str(), "--profile", "dmz"])).unwrap();
        let text = a.describe();
        assert!(text.contains("模板 dmz"), "{text}");
        // 没给配置文件时不能凭空多出一段
        assert!(!Args::parse(&argv(&["-t", "10.0.0.1"]))
            .unwrap()
            .describe()
            .contains("配置"));
    }
}
