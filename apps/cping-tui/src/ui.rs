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
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" 告警（累计 {total} 条 · 任意键关闭 · ! 切静音） "));
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
    let block = Block::default().borders(Borders::ALL).title(format!(
        " 历史与趋势（{} 台主机 · {} 点 / {} 轮） ",
        app.series.hosts().len(),
        app.series.len(),
        app.series.total_rounds()
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
    let block = Block::default().borders(Borders::ALL).title(format!(
        " 端口指纹（{} 台主机 · 附加信息，与在线判定相互独立） ",
        app.fingerprints.len()
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
        "  ← 输入中：网段 / 单主机 / CIDR / 多网段，Enter 确认"
    } else {
        "  （按 i 编辑）"
    };
    let mode_tag = if app.mode().is_tcp() {
        format!(" [{}]", app.mode().describe())
    } else {
        String::new()
    };
    let line = Line::from(vec![
        Span::styled(" 目标: ", Style::default().add_modifier(Modifier::BOLD)),
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
                // M9：TCP 模式下状态必须显示成 TCP-OPEN / TCP-关闭，
                // 否则会被读成 ICMP 可达
                app.mode().status_label(&r.status),
                r.status.rtt_ms().map(|v| v.to_string()).unwrap_or_default(),
                r.mac.clone().unwrap_or_default(),
            ),
            None => ("等待".to_string(), String::new(), String::new()),
        };
        let vendor = result.and_then(|r| r.vendor.clone()).unwrap_or_default();
        let hostname = result.and_then(|r| r.hostname.clone()).unwrap_or_default();

        let class_label = class.map(|c| c.label_zh()).unwrap_or("-");
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
            Some(k) => format!(" [{}]", k.label_zh()),
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

    let state = if app.running {
        "探测中"
    } else if app.monitor_on() {
        "监控待扫"
    } else {
        "空闲"
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
        format!(
            "  监控 {}s（{}s 后）",
            app.watch_seconds.unwrap_or(0),
            app.seconds_to_next_round()
        )
    } else {
        String::new()
    };
    let detail = format!(
        " {:6} 第{}轮  {}  在线 {}  超时 {}  不可达 {}  错误 {}  变化 {}  用时 {:.2}s  后端 {}  本机 {}  厂商 {}  主机名 {}{}{}",
        state,
        app.monitor.round(),
        app.mode().describe(),
        app.stats.online,
        app.stats.timeout,
        app.stats.unreachable,
        app.stats.error,
        app.last_changes().len(),
        app.elapsed_ms as f64 / 1000.0,
        app.backend_name(),
        app.local_ip,
        with_vendor,
        with_host,
        watch,
        app.alert_note(),
    );
    // 用默认前景色，避免在浅色/深色主题下出现"看不见的文字"
    f.render_widget(
        Paragraph::new(truncate_display(&detail, area.width as usize)),
        chunks[1],
    );
}

fn draw_keys(f: &mut Frame, app: &App, area: Rect) {
    let base = "Enter 开始 · i 编辑 · g/t 视图 · f 过滤 · s/S/j/H 导出 · m 补厂商 · n 查主机名 · F 指纹 · U 趋势 · A 告警 · ! 静音 · w/l 快照 · M 监控 · e 变化 · T/p 方式 · ? 帮助 · q 退出";
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

fn draw_help(f: &mut Frame, app: &App, area: Rect) {
    // 高度随内容增长：帮助面板里的每一条都是要让人看到的，截断等于没说
    let popup = centered(area, 78, 60);
    f.render_widget(Clear, popup);
    // M12：如实告诉用户「现在用的是哪个后端、它能干什么、不能干什么」。
    // 内容来自核心注册表，与 `cping-cli --list-backends` 同源，不会两边说法不一。
    let caps = app.backend_caps();
    let text = Text::from(vec![
        Line::from("快捷键"),
        Line::from("  Enter        开始探测（编辑态下表示确认输入）"),
        Line::from("  i            进入/退出目标编辑"),
        Line::from("  Esc          取消探测 / 关闭本窗口"),
        Line::from("  g / t        切换 矩阵 / 表格 视图"),
        Line::from("  f            过滤不在线目标"),
        Line::from("  s / S        导出 CSV / TXT"),
        Line::from("  j / H        导出 JSON / HTML 报告"),
        Line::from("  ↑ ↓ PgUp PgDn Home End  表格滚动"),
        Line::from(""),
        Line::from("资产识别（M13）"),
        Line::from("  F            端口指纹（选中主机；无选中则全部在线主机）"),
        Line::from("               端口扫描与在线判定是两回事：端口全关不影响「在线」；"),
        Line::from("               横幅只读不写；设备类型是推断，识别不出就留空"),
        Line::from(""),
        Line::from("历史与趋势（M14）"),
        Line::from("  U            查看可用率趋势（每台主机一条字符柱状曲线）"),
        Line::from("               数值与曲线都来自本进程累积的轮次；跨会话的历史"),
        Line::from("               请用 cping-cli --history（两者共用同一套时序核心）"),
        Line::from("               曲线里的 · 是「该轮没测到这台主机」，不是「离线」"),
        Line::from(""),
        Line::from("告警（M15）"),
        Line::from("  A            查看告警记录；**首次按 A 会就地启用告警**（默认规则）"),
        Line::from("               默认：新增主机 / 离线 / 延迟>200ms / 可用率<90%"),
        Line::from("  !            静音开关：规则照常求值、冷却照常推进，只是不通知"),
        Line::from("               因此解除静音**不会**补报一堆积压的告警"),
        Line::from("  说明         第一轮不发「新增主机」——否则扫个 /24 会一次刷出 255 条；"),
        Line::from("               阈值用严格不等号，恰好等于阈值不告警；通知失败一定可见"),
        Line::from(""),
        Line::from("目标与探测方式（M9）"),
        Line::from("  输入支持     192.168.1（网段）/ 单主机 / CIDR（192.168.1.0/24）/ 多段"),
        Line::from("               多段用逗号或空格分隔，重复目标只算一次"),
        Line::from("  T            切换 ICMP ↔ TCP（TCP 结果标注为 TCP-OPEN）"),
        Line::from("  p            轮换常用 TCP 端口（22/23/80/135/139/443/445/3389）"),
        Line::from(""),
        Line::from("探测后端（M12）"),
        Line::from(format!("  当前          {}", caps.describe())),
        Line::from("  可选          icmp4（ICMP 方式）/ tcp（TCP 方式）"),
        Line::from("  IPv6 目标     必须走 TCP：按 T 切到 TCP 方式，或 --mode tcp --port N"),
        Line::from("  ICMPv6        尚未实装——登记为「不可用」，不会静默超时"),
        Line::from(format!(
            "  本机能力      {}",
            truncate_display(&app.backend_summary(), 60)
        )),
        Line::from(""),
        Line::from("持续监控（M8）"),
        Line::from("  M            开/关自动重扫（默认 30 秒一轮）"),
        Line::from("  [ / ]        缩短 / 延长重扫间隔"),
        Line::from("  e            查看变化事件流（上线/离线/延迟档位变化）"),
        Line::from(""),
        Line::from("目标信息（M7）"),
        Line::from("  m            读邻居表回填 MAC / 厂商"),
        Line::from("  n            反查当前选中行的主机名"),
        Line::from("  w / l        保存快照 / 从快照恢复"),
        Line::from("  说明         MAC、厂商来自本机邻居表与 IEEE 注册数据；"),
        Line::from("               主机名走 DNS 反查，查不到就留空，不猜。"),
        Line::from(""),
        Line::from("  ? 或 F1      本帮助"),
        Line::from("  q            退出（运行中需再按一次确认）"),
        Line::from("  Ctrl+C       强制退出（自动恢复终端）"),
        Line::from(format!(
            "  符号  {SYM_LOW} 低延迟  {SYM_MID} 中延迟  {SYM_HIGH} 高延迟  {SYM_OFFLINE} 无回应  {SYM_ERROR} 错误  {SYM_PENDING} 未探测"
        )),
    ]);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" 帮助（任意键关闭） ");
    f.render_widget(Paragraph::new(text).block(block), popup);
}

/// M8 变化事件流：最新在上。
fn draw_changes(f: &mut Frame, app: &App, area: Rect) {
    let popup = centered(area, 84, (area.height.saturating_sub(4)).min(24));
    f.render_widget(Clear, popup);
    let block = Block::default().borders(Borders::ALL).title(format!(
        " 变化事件流（累计 {} 条，缓冲上限 {}） ",
        app.change_log().total_seen(),
        app.change_log().capacity()
    ));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    if app.change_log().is_empty() {
        f.render_widget(
            Paragraph::new(
                "还没有变化记录。\n\n用 M 开启持续监控后，每轮扫描都会与上一轮比对：\n\
                 · 上线 / 离线\n · 延迟档位变化（<50ms / 50–100ms / >100ms）\n · 非在线状态之间的变化\n\n\
                 首轮只建立基线，不产生变化事件。",
            )
            .wrap(Wrap { trim: false }),
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
        lines.push(Line::from(Span::styled(
            truncate_display(
                &format!(
                    "  第{}轮  {:<15} {:<6} {} → {}",
                    c.round,
                    c.ip.to_string(),
                    c.kind.label_zh(),
                    c.from.label_zh(),
                    c.to.label_zh()
                ),
                inner.width as usize,
            ),
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
