//! 绘制。颜色只是辅助：**每个状态都有独立的形状符号**，无颜色终端下同样可读。

use cping_core::a11y::{FontScale, Palette, Rgb, Theme};
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

/// 「未探测」格的符号。**这是这里唯一保留的符号常量**：
/// 五个探测结果的符号统一由核心给出（`ResultClass::symbol`），
/// 而「还没测」不是一个探测结果，核心没有对应变体。
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
    } else if app.show_profiles {
        draw_profiles(f, app, f.area());
    } else if app.show_policy {
        draw_policy(f, app, f.area());
    } else if app.show_help {
        draw_help(f, app, f.area());
    }
}

/// M16 扫描模板选择面板。
///
/// 面板里同时标出「光标」与「当前生效的模板」：两者是不同的事，混在一起
/// 会让人以为「选到就等于应用了」。
fn draw_profiles(f: &mut Frame, app: &App, area: Rect) {
    let lines = app.profile_lines();
    let popup = centered(area, 88, (lines.len() as u16 + 2).min(area.height));
    f.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(app.t(Key::ProfilesTitle));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let text: Vec<Line> = lines
        .into_iter()
        .map(|t| Line::from(Span::raw(truncate_display(&t, inner.width as usize))))
        .collect();
    f.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), inner);
}

/// M17 安全策略设置面板。
fn draw_policy(f: &mut Frame, app: &App, area: Rect) {
    let lines = app.policy_lines();
    let popup = centered(area, 92, (lines.len() as u16 + 2).min(area.height));
    f.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(app.t(Key::PolicyTitle));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let text: Vec<Line> = lines
        .into_iter()
        .map(|t| Line::from(Span::raw(truncate_display(&t, inner.width as usize))))
        .collect();
    f.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), inner);
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
    // M18 第三步：编辑态用**反显**而不是「黄底黑字」——反显在无色终端里
    // 同样醒目，而黄底黑字在单色终端下等于没有标记。
    let style = if app.editing {
        app.look.focus()
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
    // M16：把「当前用的是哪个模板」摆在输入区——状态栏太靠右，窄终端一截就没了，
    // 而这里无论多窄都看得见。「没有模板」也照实写出来，不写空白让人猜。
    let profile_tag = match &app.profile_name {
        Some(name) => format!(" [{}]", app.r(Key::ProfileInUse, &[name])),
        None => format!(" [{}]", app.t(Key::ProfileNone)),
    };
    let line = Line::from(vec![
        Span::styled(
            app.t(Key::InputLabelTarget),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {} ", app.prefix), style),
        Span::styled(mode_tag, app.look.accent_bold()),
        Span::styled(profile_tag, app.look.accent_bold()),
        Span::styled(hint, app.look.muted()),
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

    // 字号档会把「够不够大」的门槛按倍率抬高：用户在 200% 字号下
    // 需要两倍的行数才看得到同样多的信息（见 [`Look::min_rows`]）。
    // 提示里报的必须是**抬高之后**的要求值，否则用户按提示去调窗口
    // 却发现还是不够——提示反而成了误导。
    let need_cols = MATRIX_COLS as u16 * 4;
    let need_rows = app.look.min_rows(MATRIX_ROWS as u16);
    if inner.height < need_rows || inner.width < need_cols {
        let msg = app.r(
            Key::TerminalTooSmall,
            &[
                &need_cols.to_string(),
                &need_rows.to_string(),
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
                // M18 第三步：符号上收到核心（`ResultClass::symbol`）。
                // 此前它有三份拷贝——GUI 的 TS、报表的 `class_symbol`、
                // 这里的 `SYM_*`；任何一份被改，用户就会在三个界面看到
                // 同一个状态对应三种符号。颜色可以被关掉（`--no-color`），
                // **符号是最后那条信息通道**，所以它必须只有一个来源。
                Some(c) => c.symbol(),
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
                // 否则会被读成 ICMP 可达。两套名字现在由**核心**给出
                // （`ProbeMode::status_label_in`），TUI 只挑语言。
                app.mode().status_label_in(&r.status, app.lang),
                r.status.rtt_ms().map(|v| v.to_string()).unwrap_or_default(),
                r.mac.clone().unwrap_or_default(),
            ),
            None => (
                app.mode()
                    .status_label_in(&cping_core::ProbeStatus::Pending, app.lang),
                String::new(),
                String::new(),
            ),
        };
        let vendor = result.and_then(|r| r.vendor.clone()).unwrap_or_default();
        let hostname = result.and_then(|r| r.hostname.clone()).unwrap_or_default();

        let class_label = class.map(|c| c.label_in(app.lang)).unwrap_or("-");
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
            Some(k) => format!(" [{}]", k.label_in(app.lang)),
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
            app.look.muted(),
        )));
    }
    f.render_widget(Paragraph::new(Text::from(lines)), inner);
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(area);

    let ratio = app.progress_ratio();
    // 进度条走主题的强调色。**无色模式下不涂色**——`Color::Reset` 看着像
    // 「没上色」，但它会让进度条在深色终端里几乎看不见（这条注释原来就写着
    // 「默认样式在深色主题下几乎不可见」，而 `Reset` 并没有解决它）。
    let gauge = Gauge::default()
        .ratio(ratio.clamp(0.0, 1.0))
        .gauge_style(app.look.accent())
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
    let style = app.look.muted();
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
        // M16/M17 的两个面板键：放在最常用的快捷键段里，好让「帮助里查不到
        // 新按键」这件事不发生。
        Line::from(app.t(Key::HelpM16P)),
        Line::from(app.t(Key::HelpM17G)),
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
        // 取值来自核心（`ResultClass::symbol`），不在这里另抄一份。
        Line::from(app.r(
            Key::HelpSymbols,
            &[
                ResultClass::Low.symbol(),
                ResultClass::Mid.symbol(),
                ResultClass::High.symbol(),
                ResultClass::Offline.symbol(),
                ResultClass::Error.symbol(),
                SYM_PENDING,
            ],
        )),
    ]);
    // 高度**随内容增长**：帮助里每条都是要让人看到的，写死一个高度会在加了
    // 新段落之后悄悄把末尾（退出、符号表）截掉。上限仍是终端高度。
    // `Text::height()` 是 `usize`，而 `centered()` 收 `u16`；这里用 `try_from` 而不是
    // `as` 强制转换，越界时退化成 `u16::MAX`（随后仍会被 `centered` 夹到终端高度），
    // 避免溢出静默截断成一个小数字。
    let height = u16::try_from(text.height())
        .unwrap_or(u16::MAX)
        .saturating_add(2);
    let popup = centered(area, 78, height);
    f.render_widget(Clear, popup);
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
        // 变化类型有**文字标签**（见下面 `kind` 那一列），颜色只是增强。
        // 三档借主题色：上线 / 离线 / 其他（延迟或状态变化）——
        // 语义与类别色不同，但主题里就这几档能表达「好 / 消失 / 注意」。
        let style = match c.kind {
            cping_core::monitor::ChangeKind::Appeared => app.look.class(Some(ResultClass::Low)),
            cping_core::monitor::ChangeKind::Disappeared => {
                app.look.class(Some(ResultClass::Offline))
            }
            _ => app.look.class(Some(ResultClass::Mid)),
        };
        // 行内两列用固定宽度对齐（15 / 6），这是排版而不是文案；英文类别名比
        // 中文长时 `{:<6}` 只是不再补空格，不会串位。
        let ip = format!("{:<15}", c.ip);
        let kind = format!("{:<6}", c.kind.label_in(app.lang));
        // 事件流里的状态名恒用 ICMP 那一套，**不**随当前探测方式切换——
        // 这是既有行为，改了会让同一条历史事件在不同模式下读出不同的字。
        let icmp = cping_core::ProbeMode::Icmp;
        let from = icmp.status_label_in(&c.from, app.lang);
        let to = icmp.status_label_in(&c.to, app.lang);
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

/// 界面观感：主题配色 + 类别色 + 无色模式 + 字号档。
///
/// 为什么要有这一层：颜色此前散落在本文件的十几处
/// `Style::default().fg(Color::X)` 字面量里，而 `--no-color` 只改了其中几处
/// （进度条、表格、变化面板、快捷键栏），输入区的高亮、模式标签、
/// 模板标签照旧上色——**号称无色、实际还有颜色**。
/// 把「语义角色 → 样式」集中到一处之后，无色模式的覆盖范围一眼可见，
/// 也不可能再漏。
#[derive(Debug, Clone, Copy)]
pub struct Look {
    /// 主题配色（界面色：前景 / 弱化 / 强调 / 边框）。
    pub palette: Palette,
    /// 五个类别色，顺序同 [`Palette::class_colors`]。
    classes: [Rgb; 5],
    pub no_color: bool,
    pub font_scale: FontScale,
}

impl Look {
    /// 从「主题 + 用户配置的类别色」构造。
    ///
    /// 优先级取舍：**默认主题沿用配置文件里的 `[colors]`**（那是用户自己挑的，
    /// 原版行为也是「可自定义」）；**显式选了高对比 / 色盲友好主题时，
    /// 类别色以主题为准**——那两个主题的价值就在于「达标」，
    /// 允许用户配置覆盖等于把无障碍开关变成摆设。
    pub fn new(
        theme: Theme,
        no_color: bool,
        font_scale: FontScale,
        user: &cping_core::ColorConfig,
    ) -> Self {
        let palette = theme.palette();
        let mut classes = [palette.offline; 5];
        for (i, (class, fallback)) in palette.class_colors().into_iter().enumerate() {
            classes[i] = if theme == Theme::Default {
                user.get(class.color_key())
                    .and_then(Rgb::parse)
                    .unwrap_or(fallback)
            } else {
                fallback
            };
        }
        Self {
            palette,
            classes,
            no_color,
            font_scale,
        }
    }

    /// 类别 → 交互色。
    pub fn class_color(&self, class: ResultClass) -> Rgb {
        let idx = match class {
            ResultClass::Low => 0,
            ResultClass::Mid => 1,
            ResultClass::High => 2,
            ResultClass::Offline => 3,
            ResultClass::Error => 4,
        };
        self.classes[idx]
    }

    /// 上色。**无色模式下返回默认样式**（这是唯一的着色出口）。
    fn paint(self, color: Rgb) -> Style {
        if self.no_color {
            Style::default()
        } else {
            Style::default().fg(Color::Rgb(color.r, color.g, color.b))
        }
    }

    /// 类别样式。
    pub fn class(self, class: Option<ResultClass>) -> Style {
        match class {
            Some(c) => self.paint(self.class_color(c)),
            None => self.muted(),
        }
    }

    /// 次要信息（提示、详情行）。
    ///
    /// 无色模式下用**弱化（DIM）**而不是灰色：灰色本身也是一种颜色，
    /// 而 DIM 是字形属性，单色终端照样能表达「这是次要的」。
    pub fn muted(self) -> Style {
        if self.no_color {
            Style::default().add_modifier(Modifier::DIM)
        } else {
            self.paint(self.palette.muted)
        }
    }

    /// 强调（模式标签、模板标签、进度条）。
    pub fn accent(self) -> Style {
        self.paint(self.palette.accent)
    }

    /// 强调 + 加粗（页签一类的「当前项」）。
    pub fn accent_bold(self) -> Style {
        self.accent().add_modifier(Modifier::BOLD)
    }

    /// 焦点（输入框编辑态）。
    ///
    /// **无色模式下也必须醒目**，所以用反显而不是换底色。
    /// 原来这里是「黄底黑字」——那是**依赖颜色**的做法：单色终端下
    /// 黄底与普通底色没有区别，焦点就丢了。反显是前景/背景互换，
    /// 在任何终端、任何主题下都成立。
    pub fn focus(self) -> Style {
        Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED)
    }

    /// 「窗口太小」的行数阈值——按字号档放大。
    ///
    /// 用户把终端字号调到 200% 时，同样的信息需要两倍的行数；
    /// 此时若还按原阈值判断「窗口够大」，界面会被硬挤成一团。
    pub fn min_rows(self, base: u16) -> u16 {
        let scaled = f64::from(base) * self.font_scale.factor();
        (scaled.round() as u16).max(1)
    }
}

impl App {
    /// 分类 → 样式。走 [`Look`]，不再直接读用户配置的十六进制值
    /// （那一步在 `Look::new` 里做完了，且保证无色模式覆盖得到）。
    pub fn style_for(&self, class: Option<ResultClass>) -> Style {
        self.look.class(class)
    }
}
