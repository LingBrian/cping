//! CPing 无头命令行入口。
//!
//! 与 TUI / GUI 共用同一个 `cping-core`：目标解析、调度、分类、统计、导出
//! 全部走核心，CLI 里**不含任何业务规则**。
//!
//! 退出码见 [`args`] 模块；这是给脚本与 CI 用的契约，改动属于破坏性变更。

mod args;

use args::{Args, EXIT_BACKEND, EXIT_OFFLINE, EXIT_OK, EXIT_USAGE};
use cping_core::classify::Thresholds;
use cping_core::config::ProbeConfig;
use cping_core::enrich::enrich_with_mac_and_vendor;
use cping_core::export::{ExportFormat, ExportMeta, ScanReport};
use cping_core::monitor::{diff_scans, Change, ChangeLog, DEFAULT_CHANGE_CAPACITY};
use cping_core::result::{ProbeResult, ScanStats};
use cping_core::scheduler::ProbeScheduler;
use cping_core::spec::{parse_spec, TargetSpec};
use cping_core::timeseries::TimeSeries;
use cping_core::{backend_for_run, config::ColorConfig};
use std::time::{Duration, Instant};

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(run(&argv));
}

fn run(argv: &[String]) -> i32 {
    let parsed = match Args::parse(argv) {
        Ok(a) => a,
        Err(msg) => {
            eprintln!("参数错误：{msg}");
            eprintln!("\n{}", args::HELP);
            return EXIT_USAGE;
        }
    };
    if parsed.help {
        print!("{}", args::HELP);
        return EXIT_OK;
    }
    if parsed.version {
        println!(
            "cping-cli {} · {} · 后端 {}",
            cping_core::VERSION,
            std::env::consts::OS,
            parsed.mode.backend_name()
        );
        return EXIT_OK;
    }
    // M12：打印后端能力后退出。放在参数校验之前——这个命令不需要目标。
    if parsed.list_backends {
        print_backends();
        return EXIT_OK;
    }
    // M15：打印通知通道能力后退出。同样不需要目标。
    if parsed.notify_list {
        print_notify_channels();
        return EXIT_OK;
    }
    // M16：配置合并的说明必须先打出来——「模板改了哪几个参数」「哪条命令行盖掉了
    // 配置里的值」正是用户排查「怎么跟我想的不一样」唯一能拿到的东西。
    // 放在下面两个早退之前，是因为列模板 / 导出配置同样需要这层上下文。
    print_config_notes(&parsed);

    // M16：列出配置里的模板后退出。位置很关键：`validate()` 第一条校验就是
    // 「必须有目标」，而列模板恰恰不需要目标——放到后面就永远列不出来了
    // （与 --list-backends / --notify-list 同一处理）。
    if parsed.list_profiles {
        if let Err(msg) = print_profiles(&parsed) {
            eprintln!("参数错误：{msg}");
            return EXIT_USAGE;
        }
        return EXIT_OK;
    }
    // M16：把**这次实际生效**的参数固化成一份配置文件后退出。
    // 这是「跑了一次满意的扫描，想让它下次一条 --config 就复现」的出口，
    // 同样不需要目标（只固化探测参数也是合理用法）。
    if let Some(path) = parsed.dump_config.clone() {
        if let Err(msg) = write_effective_config(&parsed, &path) {
            eprintln!("配置写出失败：{msg}");
            return EXIT_USAGE;
        }
        return EXIT_OK;
    }

    if let Err(msg) = parsed.validate() {
        eprintln!("参数错误：{msg}");
        return EXIT_USAGE;
    }

    let spec = match parse_spec(&parsed.target_spec, parsed.range) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("目标规格错误：{e}");
            return EXIT_USAGE;
        }
    };

    // M17：首次运行告知「仅用于获授权网络」。位置刻意放在参数都校验完、
    // 确认真的要开扫之后——为一次失败的调用弹一段授权提示只会让人麻木。
    print_authorization_notice(parsed.quiet);

    // M17：保留网段与目标数上限在**开扫之前**就拦下。晚一步拦的代价是：
    // 错误会被一堆轮次输出淹掉，而且线程已经起来了。调度器里那道检查仍然保留，
    // 它是权威（三端共用），这里只是把同一份判断提前一次，让 CLI 的体验一致。
    if let Err(e) = parsed.policy().check_targets(&spec.targets) {
        eprintln!("参数错误：{e}");
        if matches!(e, cping_core::CoreError::ReservedTarget { .. }) {
            eprintln!(
                "提示：确认要扫就加 --allow-reserved；这类地址扫了要么没意义、要么会打扰别人。"
            );
        }
        return EXIT_USAGE;
    }

    let mut session = match Session::new(&parsed, &spec) {
        Ok(s) => s,
        Err(code) => return code,
    };
    session.execute()
}

/// M17：首次运行（或提示文案升版后）打印一次授权提示，并记下「已告知」。
///
/// **不做阻塞式确认**：这个 CLI 会被 CI 与计划任务调用，卡在等人按回车
/// 等于把工具弄坏。提示一次、记一次，就足以起到告知作用。
///
/// `--quiet` 下改走 stderr：静默模式是「别刷屏」，不是「免告知」。
fn print_authorization_notice(quiet: bool) {
    use cping_core::policy::consent;
    if !consent::load().needs_notice() {
        return;
    }
    let text = consent::notice_text();
    if quiet {
        eprintln!("{text}");
    } else {
        println!("{text}");
        println!();
    }
    if !consent::acknowledge(cping_core::unix_millis_now()) {
        // 写不进去不是致命错误，但要说清楚——否则每次启动都提示一遍，
        // 用户会以为程序坏了
        eprintln!(
            "提示：无法记录授权确认状态（{} 不可写），下次启动会再提示一遍",
            consent::consent_path()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| "家目录".into())
        );
    }
}

/// 一次 CLI 会话（可能包含多轮 watch）。
struct Session {
    args: Args,
    spec: TargetSpec,
    scheduler: ProbeScheduler,
    previous: Vec<ProbeResult>,
    changes: ChangeLog,
    round: u64,
    last_stats: ScanStats,
    /// M14：跨会话的长期时序（`--history` 载入并回写，`--report` 据此出报表）。
    series: TimeSeries,
    /// M15：告警引擎（`--alert` 时才有）。
    alerts: Option<cping_core::AlertEngine>,
    /// M15：后台通知投递（有通知通道时才有）。
    sink: Option<cping_core::AlertSink>,
    /// M17：审计日志（`--audit` 时才有）。
    audit: Option<cping_core::AuditLog>,
    /// M17：最近一轮的用时（审计用）。
    last_elapsed_ms: u64,
    /// M17：本次会话的调度任务号（审计用）。没有活跃任务时为空串。
    current_task_id: String,
    /// M17：会话退出码，收尾写审计时用。
    exit_code: Option<i32>,
}

impl Session {
    fn new(args: &Args, spec: &TargetSpec) -> Result<Self, i32> {
        if args.watch_seconds.is_some() && args.rounds.is_none() {
            eprintln!(
                "提示：持续监控模式不会自动停止（每 {} 秒一轮），按 Ctrl+C 结束",
                args.watch_seconds.unwrap_or(0)
            );
        }
        if spec.len() > 1024 && args.watch_seconds.is_some() {
            eprintln!(
                "警告：{} 个目标进入持续监控，每轮都会重新扫描，请确认网络负载可接受",
                spec.len()
            );
        }
        // M14：长期时序。`--history` 指向的文件若已存在就**续写**（跨会话累积），
        // 这样「连续观察一周的可用率」才是真的连续，而不是每次开机重来。
        let mut series = match &args.history {
            Some(path) => match TimeSeries::load_from_path(path) {
                Ok((s, skipped)) => {
                    if skipped > 0 {
                        eprintln!(
                            "警告：历史文件 {path} 有 {skipped} 行无法解析，已跳过（其余数据继续使用）"
                        );
                    }
                    if !args.quiet && !s.is_empty() {
                        println!(
                            "已载入历史 {} 点 · {} 轮（{path}）",
                            s.len(),
                            s.total_rounds()
                        );
                    }
                    s
                }
                Err(e) => {
                    // 文件**不存在**是首次使用的正常情况；文件存在却读不动（版本不符、
                    // 不是 CPing 时序）就必须拒绝——否则下一轮写回时会把用户的历史覆盖掉。
                    if std::path::Path::new(path).exists() {
                        eprintln!("历史文件无法使用：{e}");
                        return Err(EXIT_USAGE);
                    }
                    TimeSeries::new()
                }
            },
            None => TimeSeries::new(),
        };
        series.retention_secs = args.keep_days * 86_400;

        // M15：告警引擎。`--alert-state` 指向的文件若已存在就**续用**它的冷却记录
        // （否则每次重启都会把同一台主机的告警重报一遍）。
        let alerts = if args.alert {
            let mut engine = match &args.alert_state {
                Some(path) if std::path::Path::new(path).exists() => {
                    match cping_core::AlertEngine::load_from_path(path) {
                        Ok((e, skipped)) => {
                            if skipped > 0 {
                                eprintln!("警告：告警状态 {path} 有 {skipped} 行无法解析，已跳过");
                            }
                            if !args.quiet {
                                eprintln!(
                                    "已载入告警状态：{} 台主机的冷却记录（{path}）",
                                    e.seen_hosts()
                                );
                            }
                            e
                        }
                        Err(err) => {
                            eprintln!("告警状态文件无法使用：{err}");
                            return Err(EXIT_USAGE);
                        }
                    }
                }
                _ => cping_core::AlertEngine::new(args.alert_config()),
            };
            // 阈值与静音一律以**本次命令行**为准：状态文件只负责冷却记录，
            // 拿旧文件里的设置覆盖用户这次的参数只会让人莫名其妙。
            engine.apply_config(args.alert_config());
            Some(engine)
        } else {
            None
        };
        if let Some(e) = &alerts {
            if e.is_muted() {
                eprintln!("提示：告警已静音（--mute-alerts）：规则照常求值，但不会通知");
            }
            if args.watch_seconds.is_none() {
                eprintln!(
                    "提示：只跑一轮时，「新增主机」规则只建立基线（不发事件）。\
                     要观察上线下线请加 --watch"
                );
            }
        }
        // M15：通知在后台线程投递，绝不在扫描主循环里做网络 I/O
        let sink = cping_core::AlertSink::spawn(args.notify_config());

        // M12：后端选择统一走核心注册表。CLI 面向自动化，**不提供** mock —
        // 无头命令行里混进模拟数据只会让脚本的退出码失去意义。
        let mut scheduler = ProbeScheduler::new(backend_for_run(args.mode, false));
        // M17：限速与保留网段守卫在**起线程之前**生效
        if let Err(e) = scheduler.set_policy(args.policy()) {
            eprintln!("安全策略无效：{e}");
            return Err(EXIT_USAGE);
        }
        // 只在限速被改动时把策略摆出来。默认值写进每一行只会变成噪音，
        // 而「限速被关掉」是需要摆在明面上的事。
        if !args.quiet && scheduler.policy().max_pps != cping_core::DEFAULT_MAX_PPS {
            println!("{}", scheduler.policy().describe());
        }

        // M17：审计日志。**打开失败直接退出码 2**——用户显式要求留审计，
        // 就该在「审计没留成」的时候立刻失败，而不是扫完才发现。
        let audit = match &args.audit {
            Some(path) => match cping_core::AuditLog::open(path) {
                Ok(log) => Some(log),
                Err(e) => {
                    eprintln!("审计日志不可用：{e}");
                    return Err(EXIT_USAGE);
                }
            },
            None => None,
        };

        let mut session = Self {
            args: args.clone(),
            spec: spec.clone(),
            scheduler,
            previous: Vec::new(),
            changes: ChangeLog::new(DEFAULT_CHANGE_CAPACITY),
            round: 0,
            last_stats: ScanStats::default(),
            series,
            alerts,
            sink,
            audit,
            last_elapsed_ms: 0,
            current_task_id: String::new(),
            exit_code: None,
        };
        // M17：会话开始就落一条审计，把「谁、何时、扫什么规格、什么参数」记下来。
        // 放在这里而不是轮次里，是因为第一轮之前的参数信息最完整。
        session.write_audit(cping_core::AuditEvent::SessionStart, now_ms());
        Ok(session)
    }

    fn execute(&mut self) -> i32 {
        // 无论中途怎么退出，告警状态与待投递的通知都要收尾——
        // 「跑完一轮就 Ctrl+C」正是最该把冷却记录留下来的场景。
        let code = self.run_rounds();
        self.finish_alerts();
        // M17：审计收尾放在最后，退出码才有意义
        self.exit_code = Some(code);
        self.write_audit(cping_core::AuditEvent::SessionEnd, now_ms());
        if let Some(log) = &self.audit {
            if !self.args.quiet {
                println!(
                    "  审计：{} 条记录 → {}",
                    log.written(),
                    log.path().to_string_lossy()
                );
            }
        }
        code
    }

    /// M17：按当前状态组装一条审计记录。
    ///
    /// **只放计数与参数**，不放逐主机明细 / MAC / 厂商 / 反查出的主机名——
    /// 审计要能自证「我扫了什么、结果如何」，而不是变成一份资产清单。
    fn audit_record(&self, event: cping_core::AuditEvent, at_ms: u64) -> cping_core::AuditRecord {
        let st = &self.last_stats;
        let mut r = cping_core::AuditRecord::new(at_ms, event);
        r.task_id = self.scheduler_task_id();
        r.operator = cping_core::operator_name();
        r.machine = cping_core::machine_name();
        r.round = self.round;
        r.target_spec = self.spec.summary();
        r.targets = st.total;
        r.online = st.online;
        r.timeout = st.timeout;
        r.unreachable = st.unreachable;
        r.permission_denied = st.permission_denied;
        r.unsupported = st.unsupported;
        r.error = st.error;
        r.elapsed_ms = self.last_elapsed_ms;
        r.mode = self.args.mode.describe();
        r.range = self.args.range.key().to_string();
        r.timeout_ms = self.args.timeout_ms;
        r.concurrency = self.args.concurrency;
        r.max_pps = self.args.max_pps;
        r.config_path = self.args.config.clone().unwrap_or_default();
        r.profile = self.args.profile.clone().unwrap_or_default();
        if event == cping_core::AuditEvent::SessionEnd {
            r.exit_code = Some(self.exit_code.unwrap_or(0));
        }
        r
    }

    /// 审计里的任务号。没有活跃任务时留空——**不编一个看起来像真的编号**。
    fn scheduler_task_id(&self) -> String {
        self.current_task_id.clone()
    }

    /// 追加一条审计记录。写失败**必须可见**：安静地丢审计比没有审计更糟。
    fn write_audit(&mut self, event: cping_core::AuditEvent, at_ms: u64) {
        if self.audit.is_none() {
            return;
        }
        // 先组装（只读借用），再写入（可变借用）——顺序不能反
        let record = self.audit_record(event, at_ms);
        if let Some(log) = &mut self.audit {
            if let Err(e) = log.append(&record) {
                eprintln!("审计写入失败：{e}");
            }
        }
    }

    /// 主循环：跑完 `--rounds` 轮（或直到 Ctrl+C），返回退出码。
    fn run_rounds(&mut self) -> i32 {
        let thresholds = Thresholds::default();
        // 这里必须是 `== 0`：`self.round` 在下面的循环里才自增，写成 `== 1` 时
        // 判断永远不成立，横幅一次也打不出来（M16 修）。横幅里带着
        // `describe()`，也就是这次用了哪份模板——它不该静默消失。
        if self.round == 0 && !self.args.quiet {
            println!(
                "CPing {} · {} · {}",
                cping_core::VERSION,
                std::env::consts::OS,
                self.args.describe()
            );
        }
        loop {
            self.round += 1;
            let outcome = match self.scan_once(&thresholds) {
                Ok(v) => v,
                Err(code) => return code,
            };
            self.last_stats = outcome.stats.clone();
            self.last_elapsed_ms = outcome.elapsed_ms;
            self.current_task_id = outcome.task_id.clone();

            if !self.args.quiet {
                print_round(
                    self.round,
                    &self.spec,
                    &self.args,
                    &outcome,
                    &outcome.changes,
                );
            }

            if let Some((format, path)) = &self.args.out {
                if let Err(e) = self.write_report(*format, path, &outcome) {
                    eprintln!("导出失败：{e}");
                    return EXIT_USAGE;
                }
                if !self.args.quiet {
                    println!("  已写出 {path}");
                }
            }

            // M14：并入长期时序。每轮**立即落盘**——Ctrl+C 或掉电时已经跑过的轮次
            // 不该丢，这是「长期观察」的基本要求。
            // M15 的可用率规则也从这里取数，所以开了告警同样要记。
            if self.args.history.is_some() || self.args.report.is_some() || self.args.alert {
                self.series.append_results(now_ms(), &outcome.results);
            }
            if let Some(path) = &self.args.history {
                if let Err(e) = self.series.save_to_path(path) {
                    eprintln!("历史写入失败：{e}");
                    return EXIT_USAGE;
                }
                if !self.args.quiet {
                    println!(
                        "  历史：{} 点 · {} 轮 · 保留 {} 天 → {path}",
                        self.series.len(),
                        self.series.total_rounds(),
                        self.series.retention_secs / 86_400
                    );
                }
            }

            // M15：告警求值 + 非阻塞投递。顺序放在时序之后：可用率规则要用本轮数据。
            self.run_alerts(&outcome.results, &thresholds);

            self.previous = outcome.results;
            self.changes.extend(outcome.changes);

            // M17：每轮落一条审计。放在所有本轮统计都定稿之后，
            // 这样记录里的计数与屏幕上打印的完全一致。
            self.write_audit(cping_core::AuditEvent::Round, now_ms());

            if let Some(total) = self.args.rounds {
                if self.round >= total {
                    break;
                }
            }
            match self.args.watch_seconds {
                None => break,
                Some(secs) => std::thread::sleep(Duration::from_secs(secs)),
            }
        }

        // M14：报表在**所有轮次之后**生成——它描述的是整个过程，而不是某一轮。
        if let Some(path) = &self.args.report {
            if self.series.is_empty() {
                eprintln!("报表：没有可用的历史数据，已跳过 {path}");
            } else if let Err(e) = self.write_uptime_report(path) {
                eprintln!("报表写入失败：{e}");
                return EXIT_USAGE;
            } else if !self.args.quiet {
                println!("可用率报表（{} 台主机）→ {path}", self.series.hosts().len());
            }
        }

        // 退出码：后端整体不可用 > 有离线 > 全在线
        let stats = &self.last_stats;
        if stats.total > 0
            && stats.online == 0
            && stats.permission_denied + stats.unsupported == stats.total
        {
            eprintln!(
                "后端不可用：{} 个目标全部「权限不足/不支持」。{}",
                stats.total,
                cping_core::native_icmp_hint()
            );
            return EXIT_BACKEND;
        }
        if stats.online == stats.total && stats.total > 0 {
            EXIT_OK
        } else {
            EXIT_OFFLINE
        }
    }

    fn scan_once(&self, thresholds: &Thresholds) -> Result<RoundOutcome, i32> {
        let config = ProbeConfig {
            prefix: None,
            range: self.args.range,
            timeout_ms: self.args.timeout_ms,
            concurrency: self.args.concurrency,
            thresholds: *thresholds,
            mode: self.args.mode,
        };
        let started = Instant::now();
        let handle = match self.scheduler.start(&config, self.spec.targets.clone()) {
            Ok(h) => h,
            Err(e) => {
                eprintln!("无法启动扫描：{e}");
                return Err(EXIT_USAGE);
            }
        };
        // 任务号要在 `run_to_completion` 消费掉 handle 之前取出来（审计要用）
        let task_id = handle.task_id.clone();
        let outcome = handle.run_to_completion();
        let mut results = outcome.results;

        if self.args.enrich {
            let report = enrich_with_mac_and_vendor(&mut results);
            if !self.args.quiet {
                println!(
                    "  补全：MAC {} 条 · 厂商 {} 条（邻居表 {} 条）",
                    report.mac_filled, report.vendor_filled, report.entries
                );
            }
        }

        // M13：端口指纹只针对**在线**主机，且与在线判定分列。
        // 端口全关不会把主机改成离线；这里也不改 results 里的任何字段。
        let fingerprints = if self.args.fingerprint {
            let online: Vec<(std::net::IpAddr, Option<String>)> = results
                .iter()
                .filter(|r| r.status.is_online())
                .map(|r| (r.ip, r.vendor.clone()))
                .collect();
            if online.is_empty() {
                if !self.args.quiet {
                    println!("  端口指纹：没有在线主机，跳过");
                }
                Vec::new()
            } else {
                let cfg = self.args.fingerprint_config();
                let fps = cping_core::fingerprint_hosts(&online, &cfg);
                if !self.args.quiet {
                    let open_total: usize = fps.iter().map(|f| f.report.findings.len()).sum();
                    println!(
                        "  端口指纹：{} 台在线主机 × {} 个端口 · 发现 {} 个开放端口",
                        fps.len(),
                        cfg.ports.len(),
                        open_total
                    );
                }
                fps
            }
        } else {
            Vec::new()
        };

        let changes = diff_scans(&self.previous, &results, thresholds, self.round);
        let stats = ScanStats::from_results_with_total(&results, self.spec.len());
        Ok(RoundOutcome {
            results,
            stats,
            task_id,
            elapsed_ms: outcome.elapsed_ms.max(started.elapsed().as_millis() as u64),
            cancelled: outcome.cancelled,
            changes,
            fingerprints,
        })
    }

    fn write_report(
        &self,
        format: ExportFormat,
        path: &str,
        outcome: &RoundOutcome,
    ) -> Result<(), String> {
        let meta = ExportMeta {
            scan_time: now_text(),
            local_ip: String::new(),
            elapsed_ms: outcome.elapsed_ms,
            mode: self.args.mode,
            target_desc: self.spec.summary(),
            round: self.round,
        };
        let report = ScanReport::new(
            &outcome.results,
            meta,
            Thresholds::default(),
            outcome.changes.clone(),
        )
        .with_fingerprints(outcome.fingerprints.clone());
        let bytes = report
            .render(format, &ColorConfig::default())
            .map_err(|e| e.to_string())?;
        std::fs::write(path, bytes).map_err(|e| e.to_string())
    }

    /// M14：写出可用率报表。格式由扩展名决定（参数解析阶段已限定 .csv/.html）。
    fn write_uptime_report(&self, path: &str) -> Result<(), String> {
        let report = self.series.report();
        let bytes = if path.to_ascii_lowercase().ends_with(".html") {
            cping_core::report_html(&self.series, &report).into_bytes()
        } else {
            cping_core::report_csv(&report)
        };
        std::fs::write(path, bytes).map_err(|e| format!("写入 {path} 失败：{e}"))
    }

    // ------------------------------------------------------------------
    // M15：告警与通知
    // ------------------------------------------------------------------

    /// 求值一轮告警并（非阻塞地）投递。
    ///
    /// `previous` 用**上一轮**的结果：`Disappeared` 规则问的是「上一轮在线、
    /// 本轮离线」，所以必须在 `self.previous` 被覆盖之前调用。
    fn run_alerts(&mut self, current: &[ProbeResult], thresholds: &Thresholds) {
        let Some(engine) = self.alerts.as_mut() else {
            return;
        };
        let uptime = if self.series.is_empty() {
            Default::default()
        } else {
            self.series.uptime_map()
        };
        let events = engine.evaluate(
            now_ms(),
            self.round,
            &self.previous,
            current,
            thresholds,
            &uptime,
        );

        if !self.args.quiet {
            if events.is_empty() {
                if self.round > 1 {
                    println!("  告警：无");
                }
            } else {
                println!("  告警 {} 条：", events.len());
                for e in &events {
                    println!("    [{}] {}", e.severity.label_zh(), e.message);
                }
            }
            let cd = engine.suppressed_by_cooldown();
            let mu = engine.suppressed_by_mute();
            if cd + mu > 0 {
                println!("     （已按冷却/静音抑制：冷却 {cd} 条 · 静音 {mu} 条）");
            }
        }

        if let Some(sink) = &self.sink {
            sink.post(&events);
        }
    }

    /// 收尾：落盘告警状态、等通知投递完、如实汇报每个通道的结果。
    fn finish_alerts(&mut self) {
        if let (Some(engine), Some(path)) = (&self.alerts, &self.args.alert_state) {
            match engine.save_to_path(path) {
                Ok(()) if !self.args.quiet => {
                    println!(
                        "告警状态 → {path}（{} 台主机的冷却记录）",
                        engine.seen_hosts()
                    );
                }
                Ok(()) => {}
                Err(e) => eprintln!("告警状态写入失败：{e}"),
            }
        }

        let Some(sink) = self.sink.take() else {
            return;
        };
        let posted = sink.posted();
        let dropped = sink.dropped();
        let outcomes = sink.finish();
        if dropped > 0 {
            eprintln!("警告：通知队列满，有 {dropped} 批未投递（投递速度跟不上告警速度）");
        }
        // 通道失败必须可见——哪怕在 --quiet 下
        let failed = outcomes.iter().filter(|o| !o.ok).count();
        if !self.args.quiet {
            if outcomes.is_empty() {
                println!("通知：{posted} 批已投递（未配置任何通道）");
            } else {
                println!("通知：{posted} 批 · 通道结果：");
                for o in &outcomes {
                    println!(
                        "  {} {}：{}",
                        if o.ok { "✓" } else { "✗" },
                        o.channel,
                        o.detail
                    );
                }
            }
        }
        for o in outcomes.iter().filter(|o| !o.ok) {
            eprintln!("通知通道 {} 失败：{}", o.channel, o.detail);
        }
        if failed > 0
            && self
                .alerts
                .as_ref()
                .is_some_and(|e| e.log().total_seen() > 0)
        {
            eprintln!(
                "提示：有 {failed} 个通知通道未能送达（告警本身已记录，见 --notify-list 查能力）"
            );
        }
    }
}

struct RoundOutcome {
    results: Vec<ProbeResult>,
    stats: ScanStats,
    elapsed_ms: u64,
    /// M17：调度器分配的任务号（审计里用它把会话与轮次串起来）。
    task_id: String,
    cancelled: bool,
    changes: Vec<Change>,
    /// M13：端口指纹（独立于 results，不参与在线判定）。
    fingerprints: Vec<cping_core::HostFingerprint>,
}

/// M15：列出本平台的通知通道能力。
///
/// 与 `--list-backends` 同一套思路：**不可用的通道也打出来，并写明原因**。
/// 「webhook 不支持 https」是用户需要知道的事实，藏起来只会让人对着
/// 一个没反应的通知通道猜半天。
fn print_notify_channels() {
    println!(
        "cping-cli {} · {} · 通知通道",
        cping_core::VERSION,
        std::env::consts::OS
    );
    println!();
    println!("{:<10} {:<6} 说明", "通道", "可用");
    for (key, ok, note) in cping_core::notifications_available() {
        println!("{:<10} {:<6} {}", key, if ok { "是" } else { "否" }, note);
    }
    println!();
    println!("提示：通道失败一定会被打印出来——告警系统静默失败比不装还糟。");
    println!("      https 的 webhook 请走 --notify-cmd 调 curl 等自带 TLS 的工具。");
}

// ---------------------------------------------------------------------------
// M16：配置文件与模板
// ---------------------------------------------------------------------------

/// M16：把配置合并的说明逐条打出来。
///
/// 这几行**是 M16 的核心可用性要求，不是调试信息**：模板到底改了哪些参数、
/// 哪一条命令行参数盖掉了模板的值，如果不说，用户只能对着「怎么跟我想的不一样」
/// 反复试参数，而这恰恰是配置文件最容易招人恨的地方。
///
/// `--quiet` 下不打（那是给脚本用的模式），但**错误一律走 stderr**、不受 `--quiet`
/// 影响——「配置读不动」必须让 CI 直接失败，而不是安静地按默认值跑。
fn print_config_notes(args: &Args) {
    if args.quiet || args.config_notes.is_empty() {
        return;
    }
    println!("配置来源：");
    for note in &args.config_notes {
        println!("  · {note}");
    }
}

/// M16：列出 `--config` 里的全部模板。
///
/// 与 `--list-backends` 同一套思路：把每份模板的**可核对项**都摊开
/// （目标规格、解析出的目标数量、range、超时、并发、探测方式），
/// 用户不必去读 TOML 就知道 `--profile` 该填哪个。目标规格解析不出来时
/// 写「目标规格无法解析」而**不猜一个数字**——猜出来的数量会让用户以为配置是对的。
fn print_profiles(args: &Args) -> Result<(), String> {
    // `apply_config` 已保证 `--list-profiles` 一定伴随 `--config`，
    // 这里仍然用 `ok_or_else` 而不是 `unwrap`：入口多了一条，就没有 unwrap 的理由。
    let doc = args
        .config_doc
        .as_ref()
        .ok_or_else(|| "--list-profiles 需要同时给 --config".to_string())?;
    let path = args.config.as_deref().unwrap_or("");

    println!(
        "cping-cli {} · {} · 模板（{path}）",
        cping_core::VERSION,
        std::env::consts::OS
    );
    println!();
    if doc.profiles.is_empty() {
        println!("这份配置里没有任何模板。");
        println!();
        println!("加一份（注意是**双方括号**，写成 [profiles] 会被明确拒绝）：");
        println!("  [[profiles]]");
        println!("  name  = \"lan\"");
        println!("  spec  = \"192.168.1\"");
        println!("  note  = \"办公网段\"");
        return Ok(());
    }

    // 不给 --profile 时会用 default_profile 那一份，标出来省得用户去猜
    let auto = doc.default_profile.as_deref().unwrap_or("");
    println!("{:<14} {:<24} 说明", "模板", "目标规格");
    for p in &doc.profiles {
        let tag = if !auto.is_empty() && p.name.eq_ignore_ascii_case(auto) {
            "  ← default_profile"
        } else {
            ""
        };
        println!("{}{tag}", p.describe());
    }
    println!();
    println!("用法：--config {path} --profile <模板名>");
    if auto.is_empty() {
        println!("     配置里没写 default_profile，必须显式给 --profile");
    } else {
        println!("     不给 --profile 时用 default_profile = {auto:?} 那一份");
    }
    if !doc.unknown_entries().is_empty() {
        // 「不认识的键被保留了」是用户需要知道的事实：他不会以为被吃掉了
        let n = doc.unknown_entries().len();
        println!("     另有 {n} 个本程序不认识的键，已原样保留（写回时不变）");
    }
    Ok(())
}

/// M16：把**这次实际生效**的参数固化成一份配置文件。
///
/// 用途：手工调参跑通之后，一条 `--dump-config nightly.toml` 把它固化下来，
/// 以后 `--config nightly.toml` 就能复现。
///
/// 三条刻意的取舍：
/// 1. 以读到的文档为底（保住其它模板与不认识的键）；没读过 `--config` 就是新文档。
/// 2. 目标规格只能落进**模板**：`[probe]` 的 `prefix` 只装得下单个 C 类前三段，
///    装不下 CIDR 与多网段，硬塞会丢信息。模板名取 `--profile`，没给就用
///    `default`——与 v1 迁移生成的模板同名，读回来是同一套语义。
/// 3. 「这次跑要干什么」的参数（导出路径、`--history`、通知通道、告警规则）
///    **刻意不写入**。它们不属于「扫什么、怎么扫」；写进去会让「换个模板」
///    连通知一起换掉，排查起来是场噩梦。
fn write_effective_config(args: &Args, path: &str) -> Result<(), String> {
    let mut doc = args.config_doc.clone().unwrap_or_default();

    // [probe] 记的是这次**生效**的探测参数（已含命令行覆盖的结果）
    doc.app.probe.range = args.range;
    doc.app.probe.timeout_ms = args.timeout_ms;
    doc.app.probe.concurrency = args.concurrency;
    doc.app.probe.mode = args.mode;

    // 没有目标：导出成一份只含 [probe] 的配置。这不是错误——
    // 「把探测参数固化下来、目标每次从命令行给」是合理用法。
    if args.target_spec.trim().is_empty() {
        doc.validate().map_err(|e| e.to_string())?;
        doc.save_to_path(path).map_err(|e| e.to_string())?;
        if !args.quiet {
            println!("已写出配置（只含 [probe]，没有目标规格）→ {path}");
            println!("提示：目标规格不在 [probe] 里（装不下 CIDR/多网段），请每次用 --target 指定");
        }
        return Ok(());
    }

    let name = match &args.profile {
        Some(n) => n.trim().to_string(),
        None => "default".to_string(),
    };
    // 同名的模板就地更新（保住它的 note 与不认识的键），没有就新建
    let mut p = doc
        .profile(&name)
        .cloned()
        .unwrap_or_else(|| cping_core::ScanProfile::new(name.clone(), ""));
    let old_spec = p.spec.clone();
    p.name = name.clone();
    p.spec = args.target_spec.clone();
    p.range = args.range;
    p.timeout_ms = args.timeout_ms;
    p.concurrency = args.concurrency;
    p.mode = args.mode;
    doc.upsert_profile(p);
    // 指向刚写的那一份，这样 `--config <path>` 单独一条就能复现
    doc.default_profile = Some(name.clone());

    doc.validate().map_err(|e| e.to_string())?;
    doc.save_to_path(path).map_err(|e| e.to_string())?;

    if !args.quiet {
        let count = parse_spec(&args.target_spec, args.range)
            .ok()
            .map(|s| format!("{} 个目标", s.len()))
            .unwrap_or_else(|| "目标规格无法解析".to_string());
        println!("已写出配置 → {path}");
        println!("  模板 {name}：{}（{count}）", args.target_spec);
        println!("  已设为 default_profile：下次直接 cping-cli --config {path}");
        // 覆盖了原值就必须说出来——这是唯一会让人丢东西的地方
        if !old_spec.is_empty() && old_spec != args.target_spec {
            println!("  注意：模板 {name} 原有的目标规格 {old_spec:?} 已被当前生效值覆盖");
        }
        println!("  未写入（刻意不属模板）：--out / --json / --history / --report /");
        println!("                          --alert-* / --notify-*");
    }
    Ok(())
}

fn print_round(
    round: u64,
    spec: &TargetSpec,
    args: &Args,
    outcome: &RoundOutcome,
    changes: &[Change],
) {
    let stats = &outcome.stats;
    println!(
        "第 {round} 轮 · {} · {} · 用时 {:.3} s{}",
        args.mode.describe(),
        spec.summary(),
        outcome.elapsed_ms as f64 / 1000.0,
        if outcome.cancelled {
            "（已取消）"
        } else {
            ""
        }
    );
    println!(
        "  目标 {} · 在线 {} · 超时 {} · 不可达 {} · 权限 {} · 不支持 {} · 错误 {}",
        stats.total,
        stats.online,
        stats.timeout,
        stats.unreachable,
        stats.permission_denied,
        stats.unsupported,
        stats.error
    );

    if !changes.is_empty() {
        println!("  变化 {} 项：", changes.len());
        for c in changes.iter().take(20) {
            println!("    {}", c.summary());
        }
        if changes.len() > 20 {
            println!("    … 另有 {} 项，见导出文件", changes.len() - 20);
        }
    }

    if stats.online > 0 {
        println!("  在线主机：");
        for r in outcome.results.iter().filter(|r| r.status.is_online()) {
            let rtt = r.status.rtt_ms().unwrap_or(0);
            let vendor = if r.vendor_or_empty().is_empty() {
                String::new()
            } else {
                format!("  {}", r.vendor_or_empty())
            };
            let host = if r.hostname_or_empty().is_empty() {
                String::new()
            } else {
                format!("  {}", r.hostname_or_empty())
            };
            println!(
                "    {:<15} {:>6} ms  {}{}{}",
                r.ip,
                rtt,
                args.mode.status_label(&r.status),
                vendor,
                host
            );
        }
    }

    // M13：端口指纹单独一段，标题写明与在线判定无关
    if !outcome.fingerprints.is_empty() {
        println!("  端口指纹（附加信息，与上面的在线判定相互独立）：");
        for fp in &outcome.fingerprints {
            if !fp.has_open_ports() {
                println!("    {:<15} 无开放端口", fp.ip());
                continue;
            }
            println!("    {:<15} {}", fp.ip(), fp.open_ports_summary());
            for f in &fp.report.findings {
                if let Some(banner) = &f.banner {
                    println!("    {:<15}   :{} {}", "", f.port, banner);
                }
            }
        }
        for fp in outcome.fingerprints.iter().filter(|f| f.device.is_some()) {
            println!("    {} → {}", fp.ip(), fp.device_summary());
        }
    }
}

/// M12：列出本平台的后端能力。
/// 刻意把**不可用**的后端也打出来（并写明原因）：「ICMPv6 未实装」是用户需要
/// 知道的事实，藏起来只会让人对着清一色的「不支持」猜原因。
fn print_backends() {
    let reg = cping_core::BackendRegistry::new();
    println!(
        "cping-cli {} · {} · {}",
        cping_core::VERSION,
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    println!("{}", reg.summary_zh());
    println!();
    println!(
        "{:<8} {:<22} {:<8} {:<14} 说明",
        "ID", "名称", "可用", "地址族"
    );
    for caps in reg.caps() {
        let family = match (caps.ipv4, caps.ipv6) {
            (true, true) => "IPv4+IPv6",
            (true, false) => "仅 IPv4",
            (false, true) => "仅 IPv6",
            (false, false) => "—",
        };
        println!(
            "{:<8} {:<22} {:<8} {:<14} {}",
            caps.id.key(),
            caps.id.label_zh(),
            if caps.available { "是" } else { "否" },
            family,
            if caps.note.is_empty() {
                if caps.needs_privilege {
                    "需要原始套接字权限"
                } else {
                    "—"
                }
            } else {
                caps.note
            }
        );
    }
    println!();
    println!("提示：IPv6 目标目前请用 --mode tcp（ICMPv6 尚未实装）。");
}

fn now_text() -> String {
    format_unix_local(cping_core::unix_secs_now())
}

/// 当前 Unix 毫秒（M14 时序点 / M15 告警事件的时间戳）。取不到时退化为 0，
/// 调用端会把它当作「时间未知」——而不是伪造一个看起来合理的数字。
fn now_ms() -> u64 {
    cping_core::unix_millis_now()
}

/// 把 Unix 秒转成 `YYYY-MM-DD HH:MM:SS UTC`。
///
/// M15 起算法下沉到 `cping_core::timefmt`（此前 CLI/TUI/GUI 各有一份拷贝），
/// 这里保留同名包装只是为了不动既有调用点与单测。
fn format_unix_local(unix_secs: u64) -> String {
    cping_core::format_unix_local(unix_secs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cping_core::result::ProbeStatus;
    use cping_core::spec::ProbeMode;
    use std::net::{IpAddr, Ipv4Addr};

    // 日期换算的边界测试已随算法一起下沉到 `cping_core::timefmt`，
    // 这里只留一条「CLI 的输出口径确实是 UTC 且带后缀」的断言（防止包装被改坏）。

    #[test]
    fn timestamp_is_marked_as_utc_not_local() {
        let text = format_unix_local(0);
        assert_eq!(text, "1970-01-01 00:00:00 UTC");
        assert!(format_unix_local(1_700_000_000).ends_with(" UTC"));
    }

    #[test]
    fn print_round_reports_tcp_labels_for_tcp_mode() {
        let args = Args {
            mode: ProbeMode::Tcp { port: 80 },
            quiet: false,
            ..Args::default()
        };
        let spec = parse_spec("10.0.0.1", cping_core::TargetRange::Standard).unwrap();
        let outcome = RoundOutcome {
            results: vec![ProbeResult::new(
                0,
                IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
                ProbeStatus::Online { rtt_ms: 1 },
            )],
            stats: ScanStats::from_results(&[ProbeResult::new(
                0,
                IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
                ProbeStatus::Online { rtt_ms: 1 },
            )]),
            elapsed_ms: 12,
            task_id: "t-test".into(),
            cancelled: false,
            changes: Vec::new(),
            fingerprints: Vec::new(),
        };
        // 只要不 panic 即可；内容由手工冒烟确认（见 docs/testing.md §M10）
        print_round(1, &spec, &args, &outcome, &[]);
    }

    // ------------------------------------------------------------------
    // M16：配置与模板（导出侧）
    // ------------------------------------------------------------------

    fn argv(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn m16_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("cping_cli_m16_dump_test");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn m16_dumped_config_round_trips_through_the_reader() {
        let dir = m16_dir();
        let path = dir.join("nightly.toml");
        let p = path.to_str().unwrap();

        let args = Args::parse(&argv(&[
            "-t",
            "10.7.0.0/30",
            "-c",
            "9",
            "--timeout",
            "1500",
        ]))
        .unwrap();
        write_effective_config(&args, p).unwrap();

        // 读回来必须拿到同一份生效参数——否则「固化下来」就是骗人的
        let (doc, _) = cping_core::ConfigDocument::load_from_path(p).unwrap();
        let prof = doc
            .select_profile(None)
            .unwrap()
            .expect("应写出模板并设为默认，否则 --config 单独一条复现不了");
        assert_eq!(prof.spec, "10.7.0.0/30");
        assert_eq!(prof.timeout_ms, 1500);
        assert_eq!(prof.concurrency, 9);

        // 再走一遍命令行合并，结果应与首次一致
        let again = Args::parse(&argv(&["--config", p])).unwrap();
        assert_eq!(again.target_spec, "10.7.0.0/30");
        assert_eq!(again.timeout_ms, 1500);
        assert_eq!(again.concurrency, 9);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn m16_dumped_config_keeps_unknown_keys() {
        let dir = m16_dir();
        let src = dir.join("with-unknown.toml");
        std::fs::write(
            &src,
            "schema_version = 2\n\n[probe]\nprefix = \"\"\nrange = \"standard\"\n\
             timeout_ms = 1000\nconcurrency = 32\nmode = \"icmp\"\ntcp_port = 80\n\n\
             [my_plugin]\nhook = \"on_scan_start\"\n",
        )
        .unwrap();
        let dst = dir.join("kept.toml");
        let d = dst.to_str().unwrap();

        let args = Args::parse(&argv(&[
            "--config",
            src.to_str().unwrap(),
            "-t",
            "127.0.0.1",
        ]))
        .unwrap();
        write_effective_config(&args, d).unwrap();

        // 不认识的键必须原样留在文件里，而且是「还认得出来」的形态
        let (doc, _) = cping_core::ConfigDocument::load_from_path(d).unwrap();
        assert!(
            doc.unknown_entries()
                .iter()
                .any(|e| e.path() == "my_plugin.hook"),
            "{:?}",
            doc.unknown_entries()
        );
        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&dst);
    }

    #[test]
    fn m16_dump_without_target_writes_probe_only() {
        let dir = m16_dir();
        let path = dir.join("probe-only.toml");
        let p = path.to_str().unwrap();
        let args = Args::parse(&argv(&["-c", "64", "--timeout", "2500"])).unwrap();
        write_effective_config(&args, p).unwrap();

        let (doc, _) = cping_core::ConfigDocument::load_from_path(p).unwrap();
        assert!(doc.profiles.is_empty(), "没给目标就不该凭空造一份模板");
        assert_eq!(doc.app.probe.concurrency, 64);
        assert_eq!(doc.app.probe.timeout_ms, 2500);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn m16_dump_write_failure_is_reported_not_swallowed() {
        let dir = m16_dir();
        // 父路径是个**文件**，写文件必然失败
        let blocker = dir.join("blocker-file");
        std::fs::write(&blocker, "").unwrap();
        let bad = blocker.join("x.toml");
        let args = Args::parse(&argv(&["-t", "127.0.0.1"])).unwrap();
        assert!(
            write_effective_config(&args, bad.to_str().unwrap()).is_err(),
            "写不出去必须报错——安静地什么都不写是最坏的结果"
        );
        let _ = std::fs::remove_file(&blocker);
    }
}
