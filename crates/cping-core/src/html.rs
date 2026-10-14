//! 单文件自包含 HTML 报告（M10）。
//!
//! 硬约束：**不引用任何外部资源**（无 `<script>`、无外链 CSS、无图片）。
//! 一份报告就是一个文件，可以丢进邮件、U 盘、归档目录，十年后仍能打开。
//!
//! 因此：
//! - 样式全部内联在 `<style>` 里；
//! - 矩阵用 `<div>` + `background` 画，不用 canvas；
//! - 不做任何交互（没有 hover 提示以外的行为），禁 JS 的环境也能看。

use crate::classify::{classify_status, ResultClass};
use crate::config::ColorConfig;
use crate::export::ScanReport;
use crate::monitor::ChangeKind;
use crate::target::MATRIX_COLS;

/// HTML 文本转义。
///
/// 覆盖 `& < > " '`：前四个是防止注入与结构破坏，`'` 是因为属性里可能用单引号。
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// 渲染完整报告。
pub fn render_report(report: &ScanReport, palette: &ColorConfig) -> String {
    let meta = &report.meta;
    let stats = &report.stats;
    let mut html = String::with_capacity(16 * 1024);

    html.push_str("<!DOCTYPE html>\n<html lang=\"zh-CN\">\n<head>\n");
    html.push_str("<meta charset=\"utf-8\">\n");
    html.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    html.push_str(&format!(
        "<title>CPing 扫描报告 · {}</title>\n",
        escape(&meta.scan_time)
    ));
    html.push_str(STYLE);
    html.push_str("</head>\n<body>\n");

    // 标题区
    html.push_str("<header>\n<h1>C网群PING工具 · 扫描报告</h1>\n");
    html.push_str(&format!(
        "<p class=\"sub\">版本 {} · 格式 v{}</p>\n",
        escape(crate::VERSION),
        crate::export::REPORT_FORMAT_VERSION
    ));
    html.push_str("</header>\n");

    // 概览
    html.push_str("<section class=\"meta\">\n<dl>\n");
    for (k, v) in [
        ("扫描时间", meta.scan_time.clone()),
        ("本机 IP", empty_as_dash(&meta.local_ip)),
        ("目标范围", empty_as_dash(&meta.target_desc)),
        ("探测方式", meta.mode.describe()),
        ("用时", format!("{:.3} s", meta.elapsed_ms as f64 / 1000.0)),
        (
            "轮次",
            if meta.round == 0 {
                "-".to_string()
            } else {
                meta.round.to_string()
            },
        ),
    ] {
        html.push_str(&format!("<dt>{}</dt><dd>{}</dd>\n", escape(k), escape(&v)));
    }
    html.push_str("</dl>\n</section>\n");

    // 统计卡片
    html.push_str("<section class=\"cards\">\n");
    let cards: [(&str, usize, &str); 6] = [
        ("目标总数", stats.total, palette.offline.as_str()),
        ("在线", stats.online, palette.low.as_str()),
        ("超时", stats.timeout, palette.offline.as_str()),
        ("不可达", stats.unreachable, palette.offline.as_str()),
        ("权限不足", stats.permission_denied, palette.error.as_str()),
        ("错误", stats.error, palette.error.as_str()),
    ];
    for (label, value, color) in cards {
        html.push_str(&format!(
            "<div class=\"card\"><span class=\"v\" style=\"color:{}\">{}</span>\
             <span class=\"k\">{}</span></div>\n",
            escape(color),
            value,
            escape(label)
        ));
    }
    html.push_str("</section>\n");

    // 变化
    if !report.changes.is_empty() {
        html.push_str("<section>\n<h2>本轮变化</h2>\n<ul class=\"changes\">\n");
        for change in report.changes.iter().rev().take(200) {
            let cls = match change.kind {
                ChangeKind::Appeared => "up",
                ChangeKind::Disappeared => "down",
                _ => "flat",
            };
            html.push_str(&format!(
                "<li class=\"{}\"><code>{}</code> {} · round {}</li>\n",
                cls,
                escape(&change.ip.to_string()),
                escape(change.kind.label_zh()),
                change.round
            ));
        }
        html.push_str("</ul>\n</section>\n");
    }

    // 矩阵（仅当目标数与 16×16 语义相容）
    let online_cells = report.results.len();
    if online_cells <= MATRIX_COLS * MATRIX_COLS && !report.results.is_empty() {
        html.push_str("<section>\n<h2>矩阵视图</h2>\n<div class=\"matrix\">\n");
        let slots =
            crate::target::matrix_slots(&report.results.iter().map(|r| r.ip).collect::<Vec<_>>());
        for (cell, slot) in slots.iter().enumerate() {
            let host = cell as u32;
            match *slot {
                Some(idx) => {
                    let r = &report.results[idx];
                    let class = classify_status(&r.status, &report.thresholds);
                    let color = class_color(class, palette);
                    let symbol = class_symbol(class);
                    html.push_str(&format!(
                        "<div class=\"cell\" style=\"background:{}\" title=\"{}\">{}{}</div>\n",
                        escape(&color),
                        escape(&format!("{} {}", r.ip, r.status.label_zh())),
                        host,
                        escape(symbol)
                    ));
                }
                None => {
                    html.push_str("<div class=\"cell empty\"></div>\n");
                }
            }
        }
        html.push_str("</div>\n</section>\n");
    }

    // 明细表
    html.push_str("<section>\n<h2>明细</h2>\n<div class=\"table-wrap\">\n<table>\n<thead><tr>");
    for h in [
        "IP地址",
        "状态",
        "RTT(ms)",
        "类别",
        "MAC地址",
        "厂商",
        "主机名",
        "备注",
    ] {
        html.push_str(&format!("<th>{}</th>", escape(h)));
    }
    html.push_str("</tr></thead>\n<tbody>\n");
    for r in &report.results {
        let class = classify_status(&r.status, &report.thresholds);
        html.push_str("<tr>");
        html.push_str(&format!("<td>{}</td>", escape(&r.ip.to_string())));
        html.push_str(&format!(
            "<td>{}</td>",
            escape(&meta.mode.status_label(&r.status))
        ));
        html.push_str(&format!(
            "<td>{}</td>",
            r.status.rtt_ms().map(|v| v.to_string()).unwrap_or_default()
        ));
        html.push_str(&format!(
            "<td style=\"color:{}\">{}</td>",
            escape(&class_color(class, palette)),
            escape(class.map(|c| c.label_zh()).unwrap_or("-"))
        ));
        html.push_str(&format!("<td>{}</td>", escape(r.mac_or_empty())));
        html.push_str(&format!("<td>{}</td>", escape(r.vendor_or_empty())));
        html.push_str(&format!("<td>{}</td>", escape(r.hostname_or_empty())));
        html.push_str(&format!(
            "<td>{}</td>",
            escape(r.status.note().unwrap_or(""))
        ));
        html.push_str("</tr>\n");
    }
    html.push_str("</tbody>\n</table>\n</div>\n</section>\n");

    html.push_str(&format!(
        "<footer>由 CPing {} 于 {} 生成 · 「{}」</footer>\n",
        escape(crate::VERSION),
        escape(&meta.scan_time),
        escape(meta.mode.describe().as_str())
    ));
    html.push_str("</body>\n</html>\n");
    html
}

fn empty_as_dash(text: &str) -> String {
    if text.trim().is_empty() {
        "-".to_string()
    } else {
        text.to_string()
    }
}

fn class_color(class: Option<ResultClass>, palette: &ColorConfig) -> String {
    match class {
        Some(ResultClass::Low) => palette.low.clone(),
        Some(ResultClass::Mid) => palette.mid.clone(),
        Some(ResultClass::High) => palette.high.clone(),
        Some(ResultClass::Offline) => palette.offline.clone(),
        Some(ResultClass::Error) => palette.error.clone(),
        None => "#666666".to_string(),
    }
}

/// 颜色不是唯一标识：矩阵格里同时放符号（与 TUI / GUI 同一套约定）。
fn class_symbol(class: Option<ResultClass>) -> &'static str {
    match class {
        Some(ResultClass::Low) => "·",
        Some(ResultClass::Mid) => "+",
        Some(ResultClass::High) => "*",
        Some(ResultClass::Offline) => "-",
        Some(ResultClass::Error) => "!",
        None => " ",
    }
}

const STYLE: &str = r#"<style>
:root{color-scheme:light dark}
body{font-family:-apple-system,"Segoe UI","Microsoft YaHei",sans-serif;margin:0;padding:24px;
  background:#f6f7f9;color:#1b1d21}
header h1{margin:0 0 4px;font-size:22px}
header .sub{margin:0 0 20px;color:#6b7280;font-size:13px}
section{background:#fff;border:1px solid #e5e7eb;border-radius:10px;padding:16px;margin-bottom:16px}
h2{font-size:15px;margin:0 0 12px;color:#374151}
dl{display:grid;grid-template-columns:88px 1fr;gap:6px 12px;margin:0;font-size:13px}
dt{color:#6b7280}
dd{margin:0}
.cards{display:flex;gap:12px;flex-wrap:wrap}
.card{flex:1 1 110px;display:flex;flex-direction:column;align-items:center;gap:2px;
  border:1px solid #e5e7eb;border-radius:8px;padding:10px}
.card .v{font-size:20px;font-weight:600}
.card .k{font-size:12px;color:#6b7280}
.matrix{display:grid;grid-template-columns:repeat(16,1fr);gap:3px;max-width:640px}
.cell{aspect-ratio:1;border-radius:3px;display:flex;align-items:center;justify-content:center;
  font-size:9px;color:#111;overflow:hidden}
.cell.empty{background:#f3f4f6}
.changes{margin:0;padding-left:18px;font-size:13px;max-height:260px;overflow:auto}
.changes .up{color:#0a7d32}
.changes .down{color:#8a8a8a}
.changes .flat{color:#6b7280}
.table-wrap{overflow:auto;max-height:520px}
table{border-collapse:collapse;font-size:12.5px;width:100%}
th,td{border-bottom:1px solid #eef0f3;padding:5px 8px;text-align:left;white-space:nowrap}
th{position:sticky;top:0;background:#fafbfc}
footer{color:#9ca3af;font-size:12px;text-align:center}
@media (prefers-color-scheme:dark){
 body{background:#16181c;color:#e8eaed}
 section{background:#1e2126;border-color:#2c3138}
 h2{color:#c9ced6}
 th{background:#23262c}
 th,td{border-color:#2c3138}
 .card{border-color:#2c3138}
 .cell.empty{background:#2a2e34}
 dt,.card .k,header .sub{color:#9aa1ab}
 footer{color:#6b7280}
}
</style>
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classify::Thresholds;
    use crate::export::{ExportMeta, ScanReport};
    use crate::result::{ProbeResult, ProbeStatus};
    use crate::spec::ProbeMode;
    use std::net::IpAddr;

    fn sample_report() -> ScanReport {
        let results = vec![
            ProbeResult::new(
                1,
                IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 1, 1)),
                ProbeStatus::Online { rtt_ms: 3 },
            )
            .with_target_info(
                Some("AA:BB:CC:DD:EE:FF".into()),
                Some("zte <corp>".into()),
                Some("host1".into()),
            ),
            ProbeResult::new(
                2,
                IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 1, 2)),
                ProbeStatus::Timeout,
            ),
        ];
        let mut meta = ExportMeta {
            scan_time: "2026-10-15 00:30:00".into(),
            local_ip: "192.168.1.10".into(),
            elapsed_ms: 1500,
            target_desc: "192.168.1（255 个）".into(),
            round: 3,
            ..ExportMeta::default()
        };
        meta.mode = ProbeMode::Tcp { port: 80 };
        ScanReport::new(&results, meta, Thresholds::default(), Vec::new())
    }

    #[test]
    fn escapes_every_dangerous_character() {
        assert_eq!(escape("a&b"), "a&amp;b");
        assert_eq!(escape("<script>"), "&lt;script&gt;");
        assert_eq!(escape("\"x\""), "&quot;x&quot;");
        assert_eq!(escape("it's"), "it&#39;s");
    }

    #[test]
    fn report_has_no_external_references() {
        let html = render_report(&sample_report(), &ColorConfig::default());
        for forbidden in [
            "<script", "http://", "https://", "<link", "<img", "@import", "src=",
        ] {
            assert!(!html.contains(forbidden), "报告里不得出现 {forbidden}");
        }
    }

    #[test]
    fn report_is_a_complete_document() {
        let html = render_report(&sample_report(), &ColorConfig::default());
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<meta charset=\"utf-8\">"));
        assert!(html.trim_end().ends_with("</html>"));
        assert!(html.contains("C网群PING工具"));
    }

    #[test]
    fn injected_markup_from_field_values_is_escaped() {
        let report = sample_report();
        let html = render_report(&report, &ColorConfig::default());
        // 厂商名里的尖括号必须被转义
        assert!(html.contains("zte &lt;corp&gt;"), "厂商名未转义");
        assert!(!html.contains("zte <corp>"));
    }

    #[test]
    fn tcp_mode_labels_show_up_in_the_table() {
        let html = render_report(&sample_report(), &ColorConfig::default());
        assert!(html.contains("TCP:80"), "探测方式应出现在概览里");
        assert!(html.contains("TCP-OPEN"), "TCP 在线必须标注为 TCP-OPEN");
    }

    #[test]
    fn stats_and_cells_appear() {
        let html = render_report(&sample_report(), &ColorConfig::default());
        assert!(html.contains("目标总数"));
        // 矩阵 256 格
        assert_eq!(
            html.matches("class=\"cell\"").count() + html.matches("class=\"cell empty\"").count(),
            256
        );
    }

    #[test]
    fn empty_meta_fields_render_as_dash() {
        let report = ScanReport::new(
            &[],
            ExportMeta::default(),
            Thresholds::default(),
            Vec::new(),
        );
        let html = render_report(&report, &ColorConfig::default());
        assert!(html.contains("<dd>-</dd>"));
    }
}
