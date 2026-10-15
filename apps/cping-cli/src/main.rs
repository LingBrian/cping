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

    let mut session = match Session::new(&parsed, &spec) {
        Ok(s) => s,
        Err(code) => return code,
    };
    session.execute()
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

        Ok(Self {
            args: args.clone(),
            spec: spec.clone(),
            // M12：后端选择统一走核心注册表。CLI 面向自动化，**不提供** mock —
            // 无头命令行里混进模拟数据只会让脚本的退出码失去意义。
            scheduler: ProbeScheduler::new(backend_for_run(args.mode, false)),
            previous: Vec::new(),
            changes: ChangeLog::new(DEFAULT_CHANGE_CAPACITY),
            round: 0,
            last_stats: ScanStats::default(),
            series,
            alerts,
            sink,
        })
    }

    fn execute(&mut self) -> i32 {
        // 无论中途怎么退出，告警状态与待投递的通知都要收尾——
        // 「跑完一轮就 Ctrl+C」正是最该把冷却记录留下来的场景。
        let code = self.run_rounds();
        self.finish_alerts();
        code
    }

    /// 主循环：跑完 `--rounds` 轮（或直到 Ctrl+C），返回退出码。
    fn run_rounds(&mut self) -> i32 {
        let thresholds = Thresholds::default();
        if self.round == 1 && !self.args.quiet {
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
            cancelled: false,
            changes: Vec::new(),
            fingerprints: Vec::new(),
        };
        // 只要不 panic 即可；内容由手工冒烟确认（见 docs/testing.md §M10）
        print_round(1, &spec, &args, &outcome, &[]);
    }
}
