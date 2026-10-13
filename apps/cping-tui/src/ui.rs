//! 绘制。颜色只是辅助：**每个状态都有独立的形状符号**，无颜色终端下同样可读。

use cping_core::classify::ResultClass;
use cping_core::export::pad;
use cping_core::target::{MATRIX_COLS, MATRIX_ROWS};

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Gauge, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::App;

const SYM_LOW: &str = "#";
const SYM_MID: &str = "+";
const SYM_HIGH: &str = "*";
const SYM_OFFLINE: &str = "-";
const SYM_ERROR: &str = "!";
const SYM_PENDING: &str = ".";

pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(5),
        Constraint::Length(3),
        Constraint::Length(1),
    ])
    .split(f.area());

    draw_input(f, app, chunks[0]);
    draw_body(f, app, chunks[1]);
    draw_status(f, app, chunks[2]);
    draw_keys(f, app, chunks[3]);

    if app.show_help {
        draw_help(f, f.area());
    }
}

fn draw_input(f: &mut Frame, app: &App, area: Rect) {
    let style = if app.editing {
        Style::default().fg(Color::Black).bg(Color::Yellow)
    } else {
        Style::default()
    };
    let hint = if app.editing {
        "  ← 输入中：只接受数字与点，Enter 确认"
    } else {
        "  （按 i 编辑）"
    };
    let line = Line::from(vec![
        Span::styled(" 网段前缀: ", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled(format!(" {} ", app.prefix), style),
        Span::styled(hint, Style::default().fg(Color::DarkGray)),
    ]);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" C网群PING工具 · 终端版 ");
    f.render_widget(Paragraph::new(line).block(block), area);
}

fn draw_body(f: &mut Frame, app: &App, area: Rect) {
    match app.view {
        cping_core::config::ViewMode::Matrix => draw_matrix(f, app, area),
        cping_core::config::ViewMode::Table => draw_table(f, app, area),
    }
}

fn draw_matrix(f: &mut Frame, app: &App, area: Rect) {
    let title = format!(
        " 图形显示（16×16 = {} 格，格号 = IP 最后一段） ",
        cping_core::target::MATRIX_CELLS
    );
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height < MATRIX_ROWS as u16 || inner.width < (MATRIX_COLS * 4) as u16 {
        let msg = format!(
            "终端过小：矩阵需要至少 {} 列 × {} 行，当前 {} × {}。可切换表格视图（t）",
            MATRIX_COLS * 4,
            MATRIX_ROWS,
            inner.width,
            inner.height
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
    let title = format!(
        " 表单显示（{} 行{}） ",
        rows.len(),
        if app.hide_offline {
            "，已过滤不在线"
        } else {
            ""
        }
    );
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    f.render_widget(block, area);

    if rows.is_empty() {
        f.render_widget(
            Paragraph::new("没有可显示的行（可尝试关闭过滤：按 f）"),
            inner,
        );
        return;
    }

    // 每个结果占 2 行（主行 + 细分隔），便于在小终端里阅读
    let per_row = 2usize;
    let capacity = (inner.height as usize / per_row).max(1);
    let start = app.scroll.min(rows.len().saturating_sub(1));
    let end = (start + capacity).min(rows.len());

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
                r.status.label_zh().to_string(),
                r.status.rtt_ms().map(|v| v.to_string()).unwrap_or_default(),
                r.mac.clone().unwrap_or_default(),
            ),
            None => ("等待".to_string(), String::new(), String::new()),
        };
        let class_label = class.map(|c| c.label_zh()).unwrap_or("-");
        let selected = idx == start;
        let style = if selected {
            app.style_for(class).add_modifier(Modifier::BOLD)
        } else {
            app.style_for(class)
        };
        lines.push(Line::from(vec![
            Span::styled(if selected { ">" } else { " " }, Style::default()),
            Span::styled(pad(&ip, 17), style),
            Span::styled(pad(&status, 12), style),
            Span::styled(pad(&rtt, 9), style),
            Span::styled(pad(&mac, 19), style),
            Span::styled(class_label.to_string(), style),
        ]));
        lines.push(Line::from(Span::styled(
            "-".repeat((inner.width as usize).min(72)),
            Style::default().fg(Color::DarkGray),
        )));
    }
    f.render_widget(Paragraph::new(Text::from(lines)), inner);
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::vertical([Constraint::Length(1), Constraint::Length(2)]).split(area);

    let ratio = app.progress_ratio();
    let gauge = Gauge::default().ratio(ratio.clamp(0.0, 1.0)).label(format!(
        "{}/{} ({:.0}%)",
        app.stats.completed,
        app.stats.total,
        ratio * 100.0
    ));
    f.render_widget(gauge, chunks[0]);

    let state = if app.running { "探测中" } else { "空闲" };
    let detail = format!(
        " {:6} 在线 {}  超时 {}  不可达 {}  错误 {}  用时 {:.2}s  后端 {}  本机 {}",
        state,
        app.stats.online,
        app.stats.timeout,
        app.stats.unreachable,
        app.stats.error,
        app.elapsed_ms as f64 / 1000.0,
        app.backend_name(),
        app.local_ip,
    );
    f.render_widget(
        Paragraph::new(detail).style(Style::default().fg(Color::Gray)),
        chunks[1],
    );
}

fn draw_keys(f: &mut Frame, app: &App, area: Rect) {
    let base = "Enter 开始 · i 编辑 · g 矩阵 · t 表格 · f 过滤 · s CSV · S TXT · ? 帮助 · q 退出";
    let text = if app.status.is_empty() {
        base.to_string()
    } else {
        format!("{base}  |  {}", app.status)
    };
    f.render_widget(
        Paragraph::new(text).style(Style::default().fg(Color::DarkGray)),
        area,
    );
}

fn draw_help(f: &mut Frame, area: Rect) {
    let popup = centered(area, 64, 18);
    f.render_widget(Clear, popup);
    let text = Text::from(vec![
        Line::from("快捷键"),
        Line::from("  Enter        开始探测（编辑态下表示确认输入）"),
        Line::from("  i            进入/退出网段前缀编辑"),
        Line::from("  Esc          取消探测 / 关闭本窗口"),
        Line::from("  g / t        切换 矩阵 / 表格 视图"),
        Line::from("  f            过滤不在线目标"),
        Line::from("  s / S        导出 CSV / TXT"),
        Line::from("  ↑ ↓ PgUp PgDn Home End  表格滚动"),
        Line::from("  ? 或 F1      本帮助"),
        Line::from("  q            退出（运行中需再按一次确认）"),
        Line::from("  Ctrl+C       退出（自动恢复终端）"),
        Line::from(""),
        Line::from("状态符号（无颜色时同样可读）："),
        Line::from(format!(
            "  {SYM_LOW} 低延迟   {SYM_MID} 中延迟   {SYM_HIGH} 高延迟   {SYM_OFFLINE} 无回应   {SYM_ERROR} 错误   {SYM_PENDING} 未探测"
        )),
    ]);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" 帮助（任意键关闭） ");
    f.render_widget(Paragraph::new(text).block(block), popup);
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
