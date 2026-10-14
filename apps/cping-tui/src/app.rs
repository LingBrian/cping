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
use crate::input::{map_key, Action};
use crate::ui;

/// 自动落盘快照的文件名（与 GUI 端同名，便于人工排查）。
pub const AUTOSAVE_FILE: &str = "cping-autosave.snap";
/// 「已处理过」的指纹文件名。同一份结果只问一次。
pub const HANDLED_FILE: &str = "cping-autosave.handled";

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
        }
    }
}

impl Options {
    pub fn from_args<I: Iterator<Item = String>>(args: I) -> Self {
        let mut opts = Options {
            no_color: std::env::var_os("NO_COLOR").is_some(),
            ..Options::default()
        };
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
                "--view" => {
                    if let Some(v) = it.next() {
                        opts.view = if v == "table" {
                            ViewMode::Table
                        } else {
                            ViewMode::Matrix
                        };
                    }
                }
                _ => {}
            }
        }
        opts
    }
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

pub struct App {
    pub prefix: String,
    pub editing: bool,
    pub view: ViewMode,
    pub hide_offline: bool,
    pub thresholds: Thresholds,
    pub colors: ColorConfig,
    pub no_color: bool,
    pub show_help: bool,

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

    handle: Option<SchedulerHandle>,
    scheduler: ProbeScheduler,
}

impl App {
    pub fn new(options: Options) -> Self {
        let snapshot_dir = options.snapshot_dir.clone();
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
            results: Vec::new(),
            targets: Vec::new(),
            cell_slots: vec![None; MATRIX_CELLS],
            stats: ScanStats::default(),
            elapsed_ms: 0,
            running: false,
            cancelled: false,
            started_at: None,
            status: "就绪。按 i 编辑网段，Enter 开始探测，M 开监控，? 查看帮助。".into(),
            last_export: None,
            local_ip: local_ip_label(),
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
            handle: None,
            // M5 起默认使用**平台原生 ICMP**；`--mock` 可切回模拟后端做演示。
            // M12：后端选择（含 mock 分支）统一走核心注册表，三端共用一处。
            scheduler: ProbeScheduler::new(backend),
        };
        app.check_pending_snapshot();
        app
    }

    pub fn backend_name(&self) -> &'static str {
        self.scheduler.backend_name()
    }

    /// 当前后端的能力（M12）：帮助面板据此如实说明地址族与权限要求。
    pub fn backend_caps(&self) -> cping_core::BackendCaps {
        cping_core::BackendRegistry::new().caps_of(self.backend_id)
    }

    /// 本平台的后端能力摘要（M12），与 `cping-cli --list-backends` 同源。
    pub fn backend_summary(&self) -> String {
        cping_core::BackendRegistry::new().summary_zh()
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
            self.status = "任务运行中不能切换探测方式，请先取消（Esc）".into();
            return;
        }
        self.backend_mode = mode;
        // 沿用启动时的 mock 选择：演示途中切换方式不该把模拟换成真实探测
        let (backend_id, backend) = cping_core::resolve_for_run(mode, self.use_mock);
        self.backend_id = backend_id;
        self.scheduler = ProbeScheduler::new(backend);
        // 换方式后旧结果不再可比，重置变化基线
        self.monitor.reset_baseline();
        self.status = format!("探测方式已切换为 {}", mode.describe());
    }

    pub fn run<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> io::Result<()> {
        loop {
            terminal.draw(|frame| ui::draw(frame, self))?;

            match next_event()? {
                UiEvent::Key(key) => self.on_key(key),
                UiEvent::Resize => self.status = "已适配终端尺寸".into(),
                UiEvent::Tick => {}
            }

            self.poll_backend();
            self.poll_hostname_job();
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
                    self.status = "输入已确认".into();
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
                    self.status = "探测仍在进行：再按一次 q 退出（已完成的按 Esc 取消）".into();
                } else {
                    self.quit_requested = true;
                }
            }
            Action::Start => {
                if self.running {
                    self.status = "已有任务在运行".into();
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
                    self.status = "没有正在运行的任务".into();
                }
            }
            Action::ToggleEdit => {
                self.editing = true;
                self.status = "编辑网段前缀（只接受数字与点），Enter 确认".into();
            }
            Action::ViewMatrix => self.view = ViewMode::Matrix,
            Action::ViewTable => self.view = ViewMode::Table,
            Action::ToggleFilter => {
                self.hide_offline = !self.hide_offline;
                self.scroll = 0;
                self.status = if self.hide_offline {
                    "已开启「过滤不在线」".into()
                } else {
                    "已关闭「过滤不在线」".into()
                };
            }
            Action::SaveCsv => self.save("csv"),
            Action::SaveTxt => self.save("txt"),
            Action::SaveJson => self.save("json"),
            Action::SaveHtml => self.save("html"),
            Action::Enrich => self.enrich(false),
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
            self.status = "持续监控已关闭".into();
        } else {
            self.watch_seconds = Some(cping_core::MONITOR_DEFAULT_INTERVAL_MS / 1000);
            self.monitor
                .set_config(MonitorConfig {
                    enabled: true,
                    ..self.monitor.config().clone()
                })
                .ok();
            self.next_round_at = Some(Instant::now());
            self.status = format!(
                "持续监控已开启：每 {} 秒重扫一次（M 关闭，[ ] 调间隔）",
                self.watch_seconds.unwrap_or(0)
            );
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
        self.status = format!("重扫间隔 = {} 秒", STEPS[next_idx]);
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
                self.status = format!("输入无效：{e}");
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
                self.running = true;
                self.cancelled = false;
                self.handle = Some(handle);
                self.last_export = None;
                self.scroll = 0;
                self.quit_confirm = false;
                self.status = format!(
                    "开始探测 {}（{} · 后端 {}）",
                    self.spec.summary(),
                    self.backend_mode.describe(),
                    self.backend_name()
                );
            }
            Err(e) => {
                self.status = format!("无法启动：{e}");
                // 启动失败就别继续空转重试
                self.watch_seconds = None;
            }
        }
    }

    pub fn cancel_scan(&mut self, silent: bool) {
        if let Some(handle) = &self.handle {
            if handle.cancel() && !silent {
                self.status = "已请求取消：不再调度新目标，已完成结果保留".into();
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
                self.status = format!(
                    "第 {} 轮完成：{} 个目标，{} 在线，用时 {:.2}s {}{}",
                    self.monitor.round(),
                    self.stats.total,
                    self.stats.online,
                    elapsed_ms as f64 / 1000.0,
                    self.enrich_note,
                    self.change_note()
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
                self.status = format!(
                    "已取消：完成 {}/{}，其中 {} 在线，已完成结果保留 {}",
                    self.stats.completed, self.stats.total, self.stats.online, self.enrich_note
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
        format!("，本轮变化 {} 项（上线 {up} / 离线 {down}）", changes.len())
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
                self.status = "尚无扫描结果可补全".into();
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
        self.enrich_note = describe_enrich(&report);
        if !quiet {
            self.status = self.enrich_note.clone();
        }
        self.autosave();
    }

    /// 反查「当前选中行」的主机名。放到线程里做，界面不卡。
    pub fn lookup_selected_hostname(&mut self) {
        if self.hostname_job.is_some() {
            self.status = "已有一次主机名反查在进行中，稍候…".into();
            return;
        }
        let Some(idx) = self.selected_target_index() else {
            self.status = "没有可反查的行（先扫描、或用 t 切到表格视图）".into();
            return;
        };
        let Some(result) = self.results.get(idx).and_then(|r| r.as_ref()) else {
            self.status = "该行尚未探测出结果".into();
            return;
        };
        if result.hostname.is_some() {
            self.status = format!(
                "{} 已有主机名 {}",
                result.ip,
                result.hostname.as_deref().unwrap_or_default()
            );
            return;
        }
        let ip = result.ip;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let name = cping_core::hostname::reverse_lookup(ip, Duration::from_millis(1500));
            let _ = tx.send(name);
        });
        self.hostname_job = Some(HostnameJob { index: idx, ip, rx });
        self.status = format!("正在反查 {ip} 的主机名（最长 1.5s）…");
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
                self.status = format!("{ip} → {name}");
                self.hostname_job = None;
                self.autosave();
            }
            Ok(None) => {
                // 查不到就如实说查不到，不填占位文本
                self.status = format!("{ip} 没有反向解析记录");
                self.hostname_job = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.status = "主机名反查线程异常结束".into();
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
            self.status = "尚无扫描结果可保存".into();
            return;
        }
        let snap = self.build_snapshot();
        let safe_prefix = self.prefix.replace('.', "_");
        let name = format!("cping-{safe_prefix}-{}.snap", file_stamp());
        let path = self.snapshot_path(&name);
        match save_to_path(&snap, &path.to_string_lossy()) {
            Ok(()) => {
                self.status = format!(
                    "已保存快照：{} 条结果（共 {} 个目标）→ {}",
                    snap.results.len(),
                    snap.total,
                    path.display()
                );
                self.last_snapshot = Some(path.to_string_lossy().to_string());
                self.mark_handled(&snap);
            }
            Err(e) => self.status = format!("保存快照失败：{e}"),
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
                self.status = "发现上次的扫描结果，是否恢复？y 恢复 / n 丢弃".into();
            }
            Err(e) => {
                self.pending_restore = Some(PendingRestore {
                    path: path.to_string_lossy().to_string(),
                    scan_time: String::new(),
                    prefix: String::new(),
                    total: 0,
                    rows: 0,
                    elapsed_ms: 0,
                    error: Some(e.to_string()),
                });
                self.status = "发现一份无法读取的快照，按 n 丢弃".into();
            }
        }
    }

    /// 用户确认恢复。默认**不**自动恢复，必须显式按 y。
    pub fn restore_snapshot(&mut self) {
        let Some(pending) = self.pending_restore.take() else {
            return;
        };
        if let Some(err) = &pending.error {
            self.status = format!("快照无法读取，已跳过：{err}");
            return;
        }
        let snap = match load_from_path(&pending.path) {
            Ok(s) => s,
            Err(e) => {
                self.status = format!("恢复失败：{e}");
                return;
            }
        };
        // 目标列表必须能由「网段 + 当前规则」完整重建，否则下标与 IP 会错位
        let parsed = match parse_prefix(&snap.prefix) {
            Ok(p) => p,
            Err(e) => {
                self.status = format!("快照中的网段无法解析：{e}");
                return;
            }
        };
        // 用规格解析器重建目标：它与 `start_scan` 走的是同一条路径，
        // 因此「快照声明的目标数」与「今天算出来的目标数」可直接比较。
        let targets = match spec_from_prefix(&parsed, TargetRange::Standard) {
            Ok(spec) => spec.targets,
            Err(e) => {
                self.status = format!("按快照网段生成目标失败：{e}");
                return;
            }
        };
        if targets.len() != snap.total {
            self.status = format!(
                "快照与当前目标规则不一致（快照 {} 个目标，按网段算出 {} 个），已跳过",
                snap.total,
                targets.len()
            );
            return;
        }

        let total = targets.len();
        let mut slots: Vec<Option<ProbeResult>> = vec![None; total];
        for r in snap.results.iter() {
            let idx = r.index as usize;
            if idx >= total {
                self.status = format!("快照中的下标 {idx} 超出范围，已跳过");
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
        self.status = format!(
            "已恢复 {}（{} 条结果 / {} 个目标，用时 {:.2}s）",
            snap.scan_time,
            snap.results.len(),
            snap.total,
            snap.elapsed_ms as f64 / 1000.0
        );
    }

    /// `l` 键：手动从自动落盘文件恢复（例如刚才按 n 忽略了、后来又想恢复）。
    pub fn load_snapshot_from_disk(&mut self) {
        self.check_pending_snapshot();
        if self.pending_restore.is_none() {
            self.status = format!(
                "没有可恢复的快照：{} 不存在或已经处理过",
                self.snapshot_path(AUTOSAVE_FILE).display()
            );
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
        self.status = "已忽略上次的扫描结果".into();
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
            self.status = "没有可导出的结果".into();
            return;
        }
        let done: Vec<ProbeResult> = self.results.iter().flatten().cloned().collect();
        if done.is_empty() {
            self.status = "尚无已完成的结果可导出".into();
            return;
        }
        let meta = ExportMeta {
            scan_time: now_string(),
            local_ip: self.local_ip.clone(),
            elapsed_ms: self.elapsed_ms,
            mode: self.backend_mode,
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
                        self.status = format!(
                            "已导出 {} 条结果 → {path}（{} 字节）",
                            done.len(),
                            bytes.len()
                        );
                        self.last_export = Some(path);
                    }
                    Err(e) => {
                        self.status = format!("写入失败：{e}（目标路径 {path}）");
                    }
                }
            }
            Err(e) => {
                self.status = format!("导出失败：{e}");
            }
        }
    }
}

/// 把核心的补全报告翻成一句人话（界面只说真实发生的事）。
fn describe_enrich(report: &EnrichReport) -> String {
    if !report.table_ok {
        return format!(
            "读取邻居表失败：{}",
            report.error.as_deref().unwrap_or("未知原因")
        );
    }
    if report.mac_filled == 0 && report.vendor_filled == 0 {
        return format!(
            "邻居表 {} 条，没有可回填的目标（ARP 表只记录近期通信过的地址）",
            report.entries
        );
    }
    format!(
        "邻居表 {} 条，新填 MAC {} 条、厂商 {} 条",
        report.entries, report.mac_filled, report.vendor_filled
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

fn local_ip_label() -> String {
    use cping_core::local_ip::{pick_primary, LocalAddressProvider, UnsupportedProvider};

    let provider = UnsupportedProvider;
    match provider.list() {
        Ok(list) => pick_primary(&list)
            .map(|a| a.ip.to_string())
            .unwrap_or_else(|| "未知".into()),
        // M5 接入真实网卡枚举；在此之前如实说明，不猜一个地址
        Err(_) => "未知（M5 接入网卡枚举）".into(),
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
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
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
        let out = render(&app, 130, 40);
        assert!(out.contains("快捷键"));
        assert!(out.contains("Ctrl+C"));
        // M8 / M9 的新键也必须在帮助里
        assert!(out.contains("持续监控"), "帮助应含 M8 段落");
        assert!(out.contains("TCP-OPEN"), "帮助应说明 TCP 模式的状态语义");
        assert!(out.contains("CIDR"), "帮助应说明 M9 的目标写法");
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
}
