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
        draw_restore_prompt(f, pending, f.area());
    } else if app.show_help {
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
                r.status.label_zh().to_string(),
                r.status.rtt_ms().map(|v| v.to_string()).unwrap_or_default(),
                r.mac.clone().unwrap_or_default(),
            ),
            None => ("等待".to_string(), String::new(), String::new()),
        };
        let vendor = result.and_then(|r| r.vendor.clone()).unwrap_or_default();
        let hostname = result.and_then(|r| r.hostname.clone()).unwrap_or_default();

        let class_label = class.map(|c| c.label_zh()).unwrap_or("-");
        let selected = idx == start;
        let style = if selected {
            app.style_for(class).add_modifier(Modifier::BOLD)
        } else {
            app.style_for(class)
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
        spans.push(Span::styled(class_label.to_string(), style));
        lines.push(Line::from(spans));

        // 详情行：只列真有内容的字段；一个都没有就退回分隔线
        let mut parts: Vec<String> = Vec::new();
        if !wide && !mac.is_empty() {
            parts.push(format!("MAC {mac}"));
        }
        if !vendor.is_empty() {
            parts.push(format!("厂商 {vendor}"));
        }
        if !hostname.is_empty() {
            parts.push(format!("主机 {hostname}"));
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

    let state = if app.running { "探测中" } else { "空闲" };
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
    let detail = format!(
        " {:6} 在线 {}  超时 {}  不可达 {}  错误 {}  用时 {:.2}s  后端 {}  本机 {}  厂商 {}  主机名 {}",
        state,
        app.stats.online,
        app.stats.timeout,
        app.stats.unreachable,
        app.stats.error,
        app.elapsed_ms as f64 / 1000.0,
        app.backend_name(),
        app.local_ip,
        with_vendor,
        with_host,
    );
    // 用默认前景色，避免在浅色/深色主题下出现"看不见的文字"
    f.render_widget(
        Paragraph::new(truncate_display(&detail, area.width as usize)),
        chunks[1],
    );
}

fn draw_keys(f: &mut Frame, app: &App, area: Rect) {
    let base = "Enter 开始 · i 编辑 · g/t 视图 · f 过滤 · s/S 导出 · m 补厂商 · n 查主机名 · w 存快照 · l 读快照 · ? 帮助 · q 退出";
    let text = if app.status.is_empty() {
        base.to_string()
    } else {
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
fn draw_restore_prompt(f: &mut Frame, pending: &PendingRestore, area: Rect) {
    let mut lines = vec![Line::from("检测到上次未处理完的扫描结果。")];
    lines.push(Line::from(""));
    match &pending.error {
        Some(err) => {
            lines.push(Line::from(format!("  无法读取：{err}")));
        }
        None => {
            lines.push(Line::from(format!("  扫描时间 : {}", pending.scan_time)));
            lines.push(Line::from(format!("  网段     : {}", pending.prefix)));
            lines.push(Line::from(format!(
                "  结果     : {} 条 / {} 个目标",
                pending.rows, pending.total
            )));
            lines.push(Line::from(format!(
                "  用时     : {:.2}s",
                pending.elapsed_ms as f64 / 1000.0
            )));
        }
    }
    lines.push(Line::from(format!(
        "  文件     : {}",
        truncate_display(&pending.path, 50)
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(match pending.error {
        Some(_) => "  y 不使用      n / Esc 删除该文件",
        None => "  y 恢复结果    n / Esc 不使用（默认不恢复）",
    }));

    let height = (lines.len() as u16).saturating_add(2);
    let popup = centered(area, 72, height);
    f.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" CPing · 发现上次的结果 ");
    f.render_widget(Paragraph::new(Text::from(lines)).block(block), popup);
}

fn draw_help(f: &mut Frame, area: Rect) {
    let popup = centered(area, 66, 23);
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
        Line::from(""),
        Line::from("目标信息（M7）"),
        Line::from("  m            读邻居表回填 MAC / 厂商"),
        Line::from("  n            反查当前选中行的主机名"),
        Line::from("  w / l        保存快照 / 从快照恢复"),
        Line::from("  说明          MAC、厂商来自本机邻居表与 IEEE 注册数据；"),
        Line::from("                主机名走 DNS 反查，查不到就留空，不猜。"),
        Line::from(""),
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
