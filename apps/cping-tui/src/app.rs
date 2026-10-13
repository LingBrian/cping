//! 应用状态机。所有探测与统计口径都来自 `cping-core`，这里**不重新实现**。

use std::io;
use std::net::Ipv4Addr;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use cping_core::classify::{classify_status, ResultClass, Thresholds};
use cping_core::config::{ColorConfig, ProbeConfig, ViewMode};
use cping_core::export::{export_to_path, ExportMeta};
use cping_core::probe::{MockBackend, MockPolicy};
use cping_core::scheduler::{ProbeScheduler, SchedulerHandle};
use cping_core::target::{build_targets, matrix_slots, parse_prefix, TargetRange, MATRIX_CELLS};
use cping_core::{ProbeEvent, ProbeResult, ScanStats};

use ratatui::backend::Backend;
use ratatui::crossterm::event::KeyEvent;
use ratatui::Terminal;

use crate::events::{next_event, UiEvent};
use crate::input::{map_key, Action};
use crate::ui;

/// 命令行选项（仅作为初始值，不替代交互界面）。
#[derive(Debug, Clone)]
pub struct Options {
    pub prefix: String,
    pub no_color: bool,
    pub hide_offline: bool,
    pub view: ViewMode,
    pub help: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            prefix: "192.168.1".into(),
            no_color: false,
            hide_offline: false,
            view: ViewMode::Matrix,
            help: false,
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
                "--prefix" => {
                    if let Some(v) = it.next() {
                        opts.prefix = v;
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
    pub targets: Vec<Ipv4Addr>,
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

    handle: Option<SchedulerHandle>,
    scheduler: ProbeScheduler,
}

impl App {
    pub fn new(options: Options) -> Self {
        Self {
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
            status: "就绪。按 i 编辑网段，Enter 开始探测，? 查看帮助。".into(),
            last_export: None,
            local_ip: local_ip_label(),
            scroll: 0,
            quit_confirm: false,
            quit_requested: false,
            handle: None,
            // M2 阶段使用模拟探测；M5 换成平台 ICMP 后端
            scheduler: ProbeScheduler::new(MockBackend::new(MockPolicy::Realistic {
                online_percent: 62,
                base_rtt_ms: 6,
                span_ms: 140,
            })),
        }
    }

    pub fn backend_name(&self) -> &'static str {
        self.scheduler.backend_name()
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
            if self.running {
                if let Some(t0) = self.started_at {
                    self.elapsed_ms = t0.elapsed().as_millis() as u64;
                }
            }
            if self.quit_requested {
                break;
            }
        }
        // 退出前取消在跑的任务，避免线程悬挂
        self.cancel_scan(true);
        Ok(())
    }

    fn on_key(&mut self, key: KeyEvent) {
        let action = map_key(key);

        if self.editing {
            match action {
                Action::Char(c) => {
                    if c.is_ascii_digit() || c == '.' {
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
            Action::Char(_) | Action::Backspace | Action::None => {}
        }
    }

    // ------------------------------------------------------------------
    // 任务控制
    // ------------------------------------------------------------------

    pub fn start_scan(&mut self) {
        let prefix = match parse_prefix(&self.prefix) {
            Ok(p) => p,
            Err(e) => {
                self.status = format!("输入无效：{e}");
                return;
            }
        };
        let targets = match build_targets(&prefix, TargetRange::Standard) {
            Ok(t) => t,
            Err(e) => {
                self.status = format!("目标生成失败：{e}");
                return;
            }
        };
        let config = ProbeConfig {
            prefix: Some(prefix),
            range: TargetRange::Standard,
            timeout_ms: 1000,
            concurrency: 32,
            thresholds: self.thresholds,
        };

        match self.scheduler.start(&config, targets.clone()) {
            Ok(handle) => {
                self.cell_slots = matrix_slots(&targets);
                self.results = vec![None; targets.len()];
                self.targets = targets;
                self.stats = ScanStats::from_results_with_total(&[], self.targets.len());
                self.elapsed_ms = 0;
                self.started_at = Some(Instant::now());
                self.running = true;
                self.cancelled = false;
                self.handle = Some(handle);
                self.last_export = None;
                self.scroll = 0;
                self.quit_confirm = false;
                self.status = format!("开始探测 {}（后端 {}）", self.prefix, self.backend_name());
            }
            Err(e) => {
                self.status = format!("无法启动：{e}");
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
                self.status = format!(
                    "扫描完成：{} 个目标，{} 在线，用时 {:.2}s",
                    self.stats.total,
                    self.stats.online,
                    elapsed_ms as f64 / 1000.0
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
                self.status = format!(
                    "已取消：完成 {}/{}，其中 {} 在线，已完成结果保留",
                    self.stats.completed, self.stats.total, self.stats.online
                );
            }
        }
    }

    fn recompute_stats(&mut self) {
        let done: Vec<ProbeResult> = self.results.iter().flatten().cloned().collect();
        self.stats = ScanStats::from_results_with_total(&done, self.targets.len());
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
        };
        let safe_prefix = self.prefix.replace('.', "_");
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

    fn opts() -> Options {
        Options {
            no_color: true,
            ..Options::default()
        }
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
        assert!(out.contains("网段前缀"), "应显示前缀输入区");
        assert!(out.contains("图形显示"), "应显示矩阵标题");
        assert!(out.contains("192.168.1"), "应显示默认前缀");
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
        let out = render(&app, 100, 30);
        assert!(out.contains("快捷键"));
        assert!(out.contains("Ctrl+C"));
        assert!(out.contains("无颜色"));
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
}
