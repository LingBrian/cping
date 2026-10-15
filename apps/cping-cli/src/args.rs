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
      --out <路径>        导出结果；按扩展名决定格式（.csv/.txt/.json/.html）
      --json <路径>       等价于 --out 且强制 JSON
  -q, --quiet             不打印人类可读摘要（只输出文件与退出码）
      --list-backends     列出本平台可用的探测后端与能力（含不可用的），然后退出
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

关于 --fingerprint：
  端口指纹是**独立于在线判定的第二趟探测**，只对在线主机做：
  端口全关不会把主机改成离线，端口开放也不会把离线主机变在线。
  服务横幅只读对方主动送出的前 256 字节，**不发送任何协议请求**；
  设备类型是「厂商 + 开放端口」的**推断**结论，识别不出就留空。
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
    pub out: Option<(ExportFormat, String)>,
    pub quiet: bool,
    /// M12：只列出后端能力后退出。
    pub list_backends: bool,
    pub help: bool,
    pub version: bool,
    /// 显式给出的 `--target`；用于判断「什么都没给」与「给了空串」的区别。
    pub target_given: bool,
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
            out: None,
            quiet: false,
            list_backends: false,
            help: false,
            version: false,
            target_given: false,
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
                    args.mode = ProbeMode::Tcp { port };
                }
                "--timeout" => {
                    let v = parse_u64("--timeout", &take_value("--timeout")?)?;
                    if v == 0 || v > MAX_TIMEOUT_MS {
                        return Err(format!("--timeout 必须在 1..={MAX_TIMEOUT_MS} 之间"));
                    }
                    args.timeout_ms = v;
                }
                "-c" | "--concurrency" => {
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
        Ok(args)
    }

    /// 校验「必需项都在」，返回可执行的配置。
    pub fn validate(&self) -> Result<(), String> {
        if self.help || self.version {
            return Ok(());
        }
        if !self.target_given || self.target_spec.trim().is_empty() {
            return Err("缺少目标：请用 --target 指定（例如 --target 192.168.1）".into());
        }
        self.mode.validate().map_err(|e| e.to_string())?;
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
        format!(
            "{} · {} 并发 · 超时 {} ms{}{}",
            self.mode.describe(),
            self.concurrency,
            self.timeout_ms,
            watch,
            fp
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
}
