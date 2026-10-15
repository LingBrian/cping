//! 绘制。颜色只是辅助：**每个状态都有独立的形状符号**，无颜色终端下同样可读。

use cping_core::classify::ResultClass;
use cping_core::export::{pad, truncate_display};
use cping_core::target::{MATRIX_COLS, MATRIX_ROWS};

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Gauge, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{App, PendingRestore};
use crate::i18n::{self, Key};

const SYM_LOW: &str = "#";
const SYM_MID: &str = "+";
const SYM_HIGH: &str = "*";
const SYM_OFFLINE: &str = "-";
const SYM_ERROR: &str = "!";
const SYM_PENDING: &str = ".";

/// 表格里 MAC 单列所需宽度（含前导空格）。
const MAC_COL_W: usize = 19;
/// 终端宽度达到这个值才把 MAC 放进主行；否则把它挪到第二行的详情里，
/// 免得主行被挤断、IP 与状态看起来"错位"。
const WIDE_TABLE: usize = 74;

pub fn draw(f: &mut Frame, app: &App) {
    // 状态区放在主体**上方**：当终端高度不足（Windows console 的 buffer 常大于可视窗口）
    // 时，进度与统计仍然一定可见，而不是被挤到可视区域外。
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(2),
        Constraint::Min(5),
        Constraint::Length(1),
    ])
    .split(f.area());

    draw_input(f, app, chunks[0]);
    draw_status(f, app, chunks[1]);
    draw_body(f, app, chunks[2]);
    draw_keys(f, app, chunks[3]);

    // 快照确认框优先级高于帮助：它需要用户明确回答
    if let Some(pending) = &app.pending_restore {
        draw_restore_prompt(f, app, pending, f.area());
    } else if app.show_changes {
        draw_changes(f, app, f.area());
    } else if app.show_fingerprints {
        draw_fingerprints(f, app, f.area());
    } else if app.show_trend {
        draw_trend(f, app, f.area());
    } else if app.show_alerts {
        draw_alerts(f, app, f.area());
    } else if app.show_help {
        draw_help(f, app, f.area());
    }
}

/// M15 告警面板。
///
/// 标题里写「首次打开即启用」：这个面板既是**查看**入口，也是**开关**——
/// 按 A 之前没有任何告警是设计如此（不问就发通知比不装还糟）。
fn draw_alerts(f: &mut Frame, app: &App, area: Rect) {
    let popup = centered(area, 92, (area.height.saturating_sub(4)).min(28));
    f.render_widget(Clear, popup);
    let total = app.alerts.as_ref().map_or(0, |e| e.log().total_seen());
    let title = app.r(Key::AlertsTitle, &[&total.to_string()]);
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let lines: Vec<Line> = app
        .alert_lines()
        .into_iter()
        .map(|t| Line::from(Span::raw(truncate_display(&t, inner.width as usize))))
        .collect();
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

/// M14 历史与趋势面板。标题里点明 `·` 的含义：它是「没测到」，不是「离线」。
fn draw_trend(f: &mut Frame, app: &App, area: Rect) {
    let popup = centered(area, 92, (area.height.saturating_sub(4)).min(28));
    f.render_widget(Clear, popup);
    let block = Block::default().borders(Borders::ALL).title(app.r(
        Key::TrendTitle,
        &[
            &app.series.hosts().len().to_string(),
            &app.series.len().to_string(),
            &app.series.total_rounds().to_string(),
        ],
    ));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let lines: Vec<Line> = app
        .trend_lines(inner.width as usize)
        .into_iter()
        .map(|t| Line::from(Span::raw(truncate_display(&t, inner.width as usize))))
        .collect();
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

/// M13 端口指纹面板。
///
/// 标题里写明「与在线判定无关」：这是本项目最容易被人误读的一处——
/// 端口全关的主机照样是「在线」，面板不能让人以为扫描把主机判成了离线。
fn draw_fingerprints(f: &mut Frame, app: &App, area: Rect) {
    let popup = centered(area, 88, (area.height.saturating_sub(4)).min(26));
    f.render_widget(Clear, popup);
    let block = Block::default().borders(Borders::ALL).title(app.r(
        Key::FingerprintTitle,
        &[&app.fingerprints.len().to_string()],
    ));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let lines: Vec<Line> = app
        .fingerprint_lines()
        .into_iter()
        .map(|t| Line::from(Span::raw(truncate_display(&t, inner.width as usize))))
        .collect();
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn draw_input(f: &mut Frame, app: &App, area: Rect) {
    let style = if app.editing {
        Style::default().fg(Color::Black).bg(Color::Yellow)
    } else {
        Style::default()
    };
    let hint = if app.editing {
        app.t(Key::InputHintEditing)
    } else {
        app.t(Key::InputHintIdle)
    };
    let mode_tag = if app.mode().is_tcp() {
        format!(" [{}]", app.mode().describe())
    } else {
        String::new()
    };
    let line = Line::from(vec![
        Span::styled(
            app.t(Key::InputLabelTarget),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {} ", app.prefix), style),
        Span::styled(
            mode_tag,
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Cyan),
        ),
        Span::styled(hint, Style::default().fg(Color::DarkGray)),
    ]);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(app.t(Key::AppTitle));
    f.render_widget(Paragraph::new(line).block(block), area);
}

fn draw_body(f: &mut Frame, app: &App, area: Rect) {
    match app.view {
        cping_core::config::ViewMode::Matrix => draw_matrix(f, app, area),
        cping_core::config::ViewMode::Table => draw_table(f, app, area),
    }
}

fn draw_matrix(f: &mut Frame, app: &App, area: Rect) {
    let title = app.r(
        Key::MatrixTitle,
        &[&cping_core::target::MATRIX_CELLS.to_string()],
    );
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height < MATRIX_ROWS as u16 || inner.width < (MATRIX_COLS * 4) as u16 {
        let msg = app.r(
            Key::TerminalTooSmall,
            &[
                &(MATRIX_COLS * 4).to_string(),
                &MATRIX_ROWS.to_string(),
                &inner.width.to_string(),
                &inner.height.to_string(),
            ],
        );
        f.render_widget(Paragraph::new(msg).wrap(Wrap { trim: true }), inner);
        return;
    }

    let mut lines: Vec<Line> = Vec::with_capacity(MATRIX_ROWS);
    for row in 0..MATRIX_ROWS {
        let mut spans: Vec<Span> = Vec::with_capacity(MATRIX_COLS);
        for col in 0..MATRIX_COLS {
            let host = (row * MATRIX_COLS + col) as u8;
            let result = app.cell_result(host);
            let class = app.class_of(result);
            let sym = match class {
                Some(ResultClass::Low) => SYM_LOW,
                Some(ResultClass::Mid) => SYM_MID,
                Some(ResultClass::High) => SYM_HIGH,
                Some(ResultClass::Offline) => SYM_OFFLINE,
                Some(ResultClass::Error) => SYM_ERROR,
                None => SYM_PENDING,
            };
            let style = app.style_for(class);
            spans.push(Span::styled(format!("{sym}{host:>3}"), style));
        }
        lines.push(Line::from(spans));
    }
    f.render_widget(Paragraph::new(Text::from(lines)), inner);
}

fn draw_table(f: &mut Frame, app: &App, area: Rect) {
    let rows = app.visible_rows();
    let filter_note = if app.hide_offline {
        app.t(Key::TableFilteredSuffix)
    } else {
        ""
    };
    let title = app.r(Key::TableTitle, &[&rows.len().to_string(), filter_note]);
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    f.render_widget(block, area);

    if rows.is_empty() {
        f.render_widget(Paragraph::new(app.t(Key::TableEmpty)), inner);
        return;
    }

    // 每个结果占 2 行：主行（IP/状态/RTT/分类）+ 详情行（MAC/厂商/主机名），
    // 便于在小终端里阅读，也让 M7 新增信息有地方放。
    let per_row = 2usize;
    let capacity = (inner.height as usize / per_row).max(1);
    let start = app.scroll.min(rows.len().saturating_sub(1));
    let end = (start + capacity).min(rows.len());
    let wide = inner.width as usize >= WIDE_TABLE;
    let inner_w = inner.width as usize;

    let mut lines: Vec<Line> = Vec::new();
    for (idx, &target_idx) in rows.iter().enumerate().take(end).skip(start) {
        let ip = app
            .targets
            .get(target_idx)
            .map(|ip| ip.to_string())
            .unwrap_or_default();
        let result = app.results.get(target_idx).and_then(|r| r.as_ref());
        let class = app.class_of(result);
        let (status, rtt, mac) = match result {
            Some(r) => (
                // M9：TCP 模式下状态必须显示成 TCP-OPEN / TCP-关闭，
                // 否则会被读成 ICMP 可达。M18：这两套名字现在由 TUI 字典提供
                // （核心只给中文 `label_zh()`），但 `TCP-OPEN` 仍是常量。
                i18n::status_label(app.lang, app.mode().is_tcp(), &r.status),
                r.status.rtt_ms().map(|v| v.to_string()).unwrap_or_default(),
                r.mac.clone().unwrap_or_default(),
            ),
            None => (
                app.t(Key::StPending).to_string(),
                String::new(),
                String::new(),
            ),
        };
        let vendor = result.and_then(|r| r.vendor.clone()).unwrap_or_default();
        let hostname = result.and_then(|r| r.hostname.clone()).unwrap_or_default();

        let class_label = class
            .map(|c| app.t(i18n::result_class_key(c)))
            .unwrap_or("-");
        let selected = idx == start;
        let base_style = app.style_for(class);
        // M8：本轮刚变化的行加粗 + 后缀标记，颜色之外还有形状，无颜色终端同样可读
        let change = app.change_kind_of_index(target_idx);
        let style = match (selected, change) {
            (true, _) => base_style.add_modifier(Modifier::BOLD | Modifier::REVERSED),
            (false, Some(_kind)) => base_style.add_modifier(Modifier::BOLD),
            (false, None) => base_style,
        };
        let marker = match change {
            Some(k) => format!(" [{}]", app.t(i18n::change_kind_key(k))),
            None => String::new(),
        };
        let mut spans = vec![
            Span::styled(if selected { ">" } else { " " }, Style::default()),
            Span::styled(pad(&ip, 17), style),
            Span::styled(pad(&status, 12), style),
            Span::styled(pad(&rtt, 9), style),
        ];
        if wide {
            spans.push(Span::styled(pad(&mac, MAC_COL_W), style));
        }
        spans.push(Span::styled(format!("{class_label}{marker}"), style));
        lines.push(Line::from(spans));

        // 详情行：只列真有内容的字段；一个都没有就退回分隔线
        // `MAC` 是协议名，不进字典；「厂商 / 主机」是两个普通名词，要翻。
        let mut parts: Vec<String> = Vec::new();
        if !wide && !mac.is_empty() {
            parts.push(format!("MAC {mac}"));
        }
        if !vendor.is_empty() {
            parts.push(app.r(Key::DetailVendor, &[&vendor]));
        }
        if !hostname.is_empty() {
            parts.push(app.r(Key::DetailHostname, &[&hostname]));
        }
        let detail = if parts.is_empty() {
            "-".repeat(inner_w.min(72))
        } else {
            format!("  {}", parts.join("   "))
        };
        lines.push(Line::from(Span::styled(
            truncate_display(&detail, inner_w),
            Style::default().fg(Color::DarkGray),
        )));
    }
    f.render_widget(Paragraph::new(Text::from(lines)), inner);
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(area);

    let ratio = app.progress_ratio();
    // 显式指定 gauge 配色：默认样式在深色主题下几乎不可见
    let gauge = Gauge::default()
        .ratio(ratio.clamp(0.0, 1.0))
        .gauge_style(Style::default().fg(if app.no_color {
            Color::Reset
        } else {
            Color::Rgb(0x2e, 0xc4, 0x6b)
        }))
        .label(format!(
            "{}/{} ({:.0}%)",
            app.stats.completed,
            app.stats.total,
            ratio * 100.0
        ));
    f.render_widget(gauge, chunks[0]);

    let state = if app.running {
        app.t(Key::StateScanning)
    } else if app.monitor_on() {
        app.t(Key::StateMonitoring)
    } else {
        app.t(Key::StateIdle)
    };
    // M7：把「认出了多少台」如实报出来。计数直接数结果，不另存一份状态。
    let with_vendor = app
        .results
        .iter()
        .flatten()
        .filter(|r| r.vendor.is_some())
        .count();
    let with_host = app
        .results
        .iter()
        .flatten()
        .filter(|r| r.hostname.is_some())
        .count();
    // M8：轮次与变化
    let watch = if app.monitor_on() {
        let secs = app.watch_seconds.unwrap_or(0).to_string();
        let next = app.seconds_to_next_round().to_string();
        i18n::render(app.lang, Key::MonitorWatch, &[&secs, &next])
    } else {
        String::new()
    };
    // 状态栏是**一整行**排版（列宽、间距都是设计的一部分），所以用一条带
    // 15 个占位符的模板整体渲染，而不是把零碎句子拼起来——后者的中英语序
    // 无法各自保持对齐。
    // `{:6}` 与 `{:.2}` 是 Rust 的格式说明，`render_template` 不认，先算好再传参。
    let detail = i18n::render(
        app.lang,
        Key::StatusDetail,
        &[
            &format!("{state:<6}"),
            &app.monitor.round().to_string(),
            &app.mode().describe(),
            &app.stats.online.to_string(),
            &app.stats.timeout.to_string(),
            &app.stats.unreachable.to_string(),
            &app.stats.error.to_string(),
            &app.last_changes().len().to_string(),
            &format!("{:.2}", app.elapsed_ms as f64 / 1000.0),
            app.backend_name(),
            &app.local_ip,
            &with_vendor.to_string(),
            &with_host.to_string(),
            &watch,
            &app.alert_note(),
        ],
    );
    // 用默认前景色，避免在浅色/深色主题下出现"看不见的文字"
    f.render_widget(
        Paragraph::new(truncate_display(&detail, area.width as usize)),
        chunks[1],
    );
}

fn draw_keys(f: &mut Frame, app: &App, area: Rect) {
    let base = app.t(Key::ShortcutBar);
    let text = if app.status.is_empty() {
        base.to_string()
    } else {
        // `|` 与两侧空格是纯排版分隔符，不随语言变。
        format!("{base}  |  {}", app.status)
    };
    let style = if app.no_color {
        Style::default()
    } else {
        Style::default().fg(Color::DarkGray)
    };
    f.render_widget(
        Paragraph::new(truncate_display(&text, area.width as usize)).style(style),
        area,
    );
}

/// 启动时「发现上次结果」的确认框。默认落在**不恢复**一侧：用户不表态就不改界面状态。
///
/// M18：多接收一个 `app`——语言只能从 `App` 拿（函数本身是纯 `&App` 借用，
/// 不改任何状态）。
fn draw_restore_prompt(f: &mut Frame, app: &App, pending: &PendingRestore, area: Rect) {
    let mut lines = vec![Line::from(app.t(Key::RestoreDetected))];
    lines.push(Line::from(""));
    match &pending.error {
        Some(err) => {
            lines.push(Line::from(app.r(Key::RestoreUnreadable, &[err])));
        }
        None => {
            lines.push(Line::from(
                app.r(Key::RestoreScanTime, &[&pending.scan_time]),
            ));
            lines.push(Line::from(app.r(Key::RestorePrefix, &[&pending.prefix])));
            lines.push(Line::from(app.r(
                Key::RestoreRows,
                &[&pending.rows.to_string(), &pending.total.to_string()],
            )));
            lines.push(Line::from(app.r(
                Key::RestoreElapsed,
                &[&format!("{:.2}", pending.elapsed_ms as f64 / 1000.0)],
            )));
        }
    }
    let shown_path = truncate_display(&pending.path, 50);
    lines.push(Line::from(app.r(Key::RestoreFile, &[&shown_path])));
    lines.push(Line::from(""));
    lines.push(Line::from(match pending.error {
        Some(_) => app.t(Key::RestoreSayError),
        None => app.t(Key::RestoreSayOk),
    }));

    let height = (lines.len() as u16).saturating_add(2);
    let popup = centered(area, 72, height);
    f.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(app.t(Key::RestoreTitle));
    f.render_widget(Paragraph::new(Text::from(lines)).block(block), popup);
}

fn draw_help(f: &mut Frame, app: &App, area: Rect) {
    // 高度随内容增长：帮助面板里的每一条都是要让人看到的，截断等于没说
    let popup = centered(area, 78, 60);
    f.render_widget(Clear, popup);
    // M12：如实告诉用户「现在用的是哪个后端、它能干什么、不能干什么」。
    // M18：后端能力行由 TUI 用注册表的公开字段拼（核心的 `describe()` /
    // `summary_zh()` 只有中文），判定仍全部来自核心。
    let caps_line = app.backend_caps_line();
    let summary = truncate_display(&app.backend_summary(), 60);
    let text = Text::from(vec![
        Line::from(app.t(Key::HelpKeysHeading)),
        Line::from(app.t(Key::HelpLineEnter)),
        Line::from(app.t(Key::HelpLineI)),
        Line::from(app.t(Key::HelpLineEsc)),
        Line::from(app.t(Key::HelpLineGt)),
        Line::from(app.t(Key::HelpLineF)),
        Line::from(app.t(Key::HelpLineSS)),
        Line::from(app.t(Key::HelpLineJH)),
        Line::from(app.t(Key::HelpLineScroll)),
        Line::from(""),
        Line::from(app.t(Key::HeadingM13)),
        Line::from(app.t(Key::HelpM13F)),
        Line::from(app.t(Key::HelpM13NoteA)),
        Line::from(app.t(Key::HelpM13NoteB)),
        Line::from(""),
        Line::from(app.t(Key::HeadingM14)),
        Line::from(app.t(Key::HelpM14U)),
        Line::from(app.t(Key::HelpM14NoteA)),
        Line::from(app.t(Key::HelpM14NoteB)),
        Line::from(app.t(Key::HelpM14NoteC)),
        Line::from(""),
        Line::from(app.t(Key::HeadingM15)),
        Line::from(app.t(Key::HelpM15A)),
        Line::from(app.t(Key::HelpM15B)),
        Line::from(app.t(Key::HelpM15Mute)),
        Line::from(app.t(Key::HelpM15MuteB)),
        Line::from(app.t(Key::HelpM15Note)),
        Line::from(app.t(Key::HelpM15NoteB)),
        Line::from(""),
        Line::from(app.t(Key::HeadingM9)),
        Line::from(app.t(Key::HelpM9Input)),
        Line::from(app.t(Key::HelpM9InputB)),
        Line::from(app.t(Key::HelpM9T)),
        Line::from(app.t(Key::HelpM9P)),
        Line::from(""),
        Line::from(app.t(Key::HeadingM12)),
        Line::from(app.r(Key::HelpM12Current, &[&caps_line])),
        Line::from(app.t(Key::HelpM12Options)),
        Line::from(app.t(Key::HelpM12V6)),
        Line::from(app.t(Key::HelpM12Icmp6)),
        Line::from(app.r(Key::HelpM12Local, &[&summary])),
        Line::from(""),
        Line::from(app.t(Key::HeadingM8)),
        Line::from(app.t(Key::HelpM8M)),
        Line::from(app.t(Key::HelpM8Bracket)),
        Line::from(app.t(Key::HelpM8E)),
        Line::from(""),
        Line::from(app.t(Key::HeadingM7)),
        Line::from(app.t(Key::HelpM7M)),
        Line::from(app.t(Key::HelpM7N)),
        Line::from(app.t(Key::HelpM7Wl)),
        Line::from(app.t(Key::HelpM7Note)),
        Line::from(app.t(Key::HelpM7NoteB)),
        Line::from(""),
        Line::from(app.t(Key::HelpLineHelpHint)),
        Line::from(app.t(Key::HelpLineQuit)),
        Line::from(app.t(Key::HelpLineCtrlC)),
        Line::from(app.t(Key::HelpCliFlags)),
        // 状态符号原样保留：它们是「无颜色终端下仍可读」的实现，翻了就失去意义。
        Line::from(app.r(
            Key::HelpSymbols,
            &[
                SYM_LOW,
                SYM_MID,
                SYM_HIGH,
                SYM_OFFLINE,
                SYM_ERROR,
                SYM_PENDING,
            ],
        )),
    ]);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(app.t(Key::HelpTitle));
    f.render_widget(Paragraph::new(text).block(block), popup);
}

/// M8 变化事件流：最新在上。
fn draw_changes(f: &mut Frame, app: &App, area: Rect) {
    let popup = centered(area, 84, (area.height.saturating_sub(4)).min(24));
    f.render_widget(Clear, popup);
    let block = Block::default().borders(Borders::ALL).title(app.r(
        Key::ChangesTitle,
        &[
            &app.change_log().total_seen().to_string(),
            &app.change_log().capacity().to_string(),
        ],
    ));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    if app.change_log().is_empty() {
        f.render_widget(
            Paragraph::new(app.t(Key::ChangesEmpty)).wrap(Wrap { trim: false }),
            inner,
        );
        return;
    }

    let rows = (inner.height as usize).saturating_sub(1);
    let mut lines: Vec<Line> = Vec::with_capacity(rows);
    for c in app.recent_changes(rows) {
        let style = if app.no_color {
            Style::default()
        } else {
            match c.kind {
                cping_core::monitor::ChangeKind::Appeared => Style::default().fg(Color::Green),
                cping_core::monitor::ChangeKind::Disappeared => Style::default().fg(Color::Gray),
                _ => Style::default().fg(Color::Yellow),
            }
        };
        // 行内两列用固定宽度对齐（15 / 6），这是排版而不是文案；英文类别名比
        // 中文长时 `{:<6}` 只是不再补空格，不会串位。
        let ip = format!("{:<15}", c.ip);
        let kind = format!("{:<6}", app.t(i18n::change_kind_key(c.kind)));
        // 事件流里的状态名恒用 ICMP 那一套（等价于核心的 `label_zh()`），
        // **不**随当前探测方式切换——这是既有行为，改了会让同一条历史事件
        // 在不同模式下读出不同的字。
        let from = i18n::status_label(app.lang, false, &c.from);
        let to = i18n::status_label(app.lang, false, &c.to);
        let row = app.r(
            Key::ChangeRow,
            &[&c.round.to_string(), &ip, &kind, &from, &to],
        );
        lines.push(Line::from(Span::styled(
            truncate_display(&row, inner.width as usize),
            style,
        )));
    }
    f.render_widget(Paragraph::new(Text::from(lines)), inner);
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    }
}

impl App {
    /// 分类 → 样式。`no_color` 时只保留符号差异。
    pub fn style_for(&self, class: Option<ResultClass>) -> Style {
        if self.no_color {
            return Style::default();
        }
        let hex = match class {
            Some(ResultClass::Low) => self.colors.low.as_str(),
            Some(ResultClass::Mid) => self.colors.mid.as_str(),
            Some(ResultClass::High) => self.colors.high.as_str(),
            Some(ResultClass::Offline) => self.colors.offline.as_str(),
            Some(ResultClass::Error) => self.colors.error.as_str(),
            None => return Style::default().fg(Color::DarkGray),
        };
        Style::default().fg(parse_hex(hex))
    }
}

/// `#RRGGBB` → ratatui 颜色；非法值退化为默认色，不 panic。
pub fn parse_hex(hex: &str) -> Color {
    let bytes = hex.as_bytes();
    if bytes.len() == 7 && bytes[0] == b'#' {
        let parse = |s: &str| u8::from_str_radix(s, 16);
        if let (Ok(r), Ok(g), Ok(b)) = (parse(&hex[1..3]), parse(&hex[3..5]), parse(&hex[5..7])) {
            return Color::Rgb(r, g, b);
        }
    }
    Color::Reset
}
