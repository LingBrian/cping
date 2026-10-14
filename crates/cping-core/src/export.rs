//! CSV / TXT 导出。**GUI 与 TUI 必须调用同一份实现**。
//!
//! 格式约定（新项目定义，非原版复刻）：
//! - CSV：`UTF-8 with BOM` + `CRLF` + RFC 4180 转义
//! - 字段：`扫描时间, IP地址, 状态, RTT(ms), MAC地址, 备注, 厂商, 主机名`
//! - 排序：按 IPv4 数值升序
//! - 空字段写空串（不回填 `N/A` 之外的占位）
//!
//! ## 向后兼容规则（M7 起）
//!
//! 新增的 `厂商`、`主机名` **只能追加在末尾**，已有列的顺序与内容不得改动。
//! 这样按固定列号解析旧版 CSV 的脚本仍然可用；而 TXT 是定宽表，新列若超宽
//! 会破坏对齐，因此 TXT 侧对超长内容做**显示宽度截断**（见 [`truncate_display`]），
//! CSV 侧则完整保留原文。

use crate::classify::{classify_status, Thresholds};
use crate::error::{CoreError, CoreResult};
use crate::result::{ProbeResult, ScanStats};
use std::net::Ipv4Addr;

pub const CSV_BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];
pub const CSV_HEADER: [&str; 8] = [
    "扫描时间",
    "IP地址",
    "状态",
    "RTT(ms)",
    "MAC地址",
    "备注",
    "厂商",
    "主机名",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Csv,
    Txt,
}

impl ExportFormat {
    pub const fn extension(self) -> &'static str {
        match self {
            ExportFormat::Csv => "csv",
            ExportFormat::Txt => "txt",
        }
    }

    /// 由文件名/路径推断格式（大小写不敏感），无法识别时返回 `None`。
    pub fn from_path(path: &str) -> Option<Self> {
        let lower = path.to_ascii_lowercase();
        if lower.ends_with(".csv") {
            Some(ExportFormat::Csv)
        } else if lower.ends_with(".txt") {
            Some(ExportFormat::Txt)
        } else {
            None
        }
    }
}

/// 导出时附加的元信息。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExportMeta {
    /// 扫描开始时间（调用方格式化，推荐 `YYYY-MM-DD HH:MM:SS`）。
    pub scan_time: String,
    pub local_ip: String,
    pub elapsed_ms: u64,
}

impl ExportMeta {
    /// 用时的人类可读形式，例如 `1.234 s`。
    pub fn elapsed_human(&self) -> String {
        format!("{:.3} s", self.elapsed_ms as f64 / 1000.0)
    }
}

/// RFC 4180 字段转义。
pub fn csv_escape(field: &str) -> String {
    let needs_quote = field
        .chars()
        .any(|c| c == ',' || c == '"' || c == '\n' || c == '\r');
    if !needs_quote {
        return field.to_string();
    }
    let mut out = String::with_capacity(field.len() + 2);
    out.push('"');
    for ch in field.chars() {
        if ch == '"' {
            out.push('"');
        }
        out.push(ch);
    }
    out.push('"');
    out
}

/// IPv4 数值排序键。
pub const fn ip_sort_key(ip: Ipv4Addr) -> u32 {
    let o = ip.octets();
    ((o[0] as u32) << 24) | ((o[1] as u32) << 16) | ((o[2] as u32) << 8) | (o[3] as u32)
}

/// 按 IPv4 数值升序返回结果副本。
pub fn sorted_by_ip(results: &[ProbeResult]) -> Vec<ProbeResult> {
    let mut out = results.to_vec();
    out.sort_by_key(|r| ip_sort_key(r.ip));
    out
}

/// 生成 CSV 字节（含 BOM）。
pub fn export_csv(results: &[ProbeResult], meta: &ExportMeta) -> CoreResult<Vec<u8>> {
    let mut text = String::new();
    text.push_str(&CSV_HEADER.join(","));
    text.push_str("\r\n");

    for r in sorted_by_ip(results) {
        let rtt = r.status.rtt_ms().map(|v| v.to_string()).unwrap_or_default();
        let mac = r.mac.clone().unwrap_or_default();
        let note = r.status.note().unwrap_or("").to_string();
        let fields = [
            meta.scan_time.clone(),
            r.ip.to_string(),
            r.status.label_zh().to_string(),
            rtt,
            mac,
            note,
            // M7 追加列：必须留在末尾
            r.vendor.clone().unwrap_or_default(),
            r.hostname.clone().unwrap_or_default(),
        ];
        let line: Vec<String> = fields.iter().map(|f| csv_escape(f)).collect();
        text.push_str(&line.join(","));
        text.push_str("\r\n");
    }

    let mut bytes = Vec::with_capacity(CSV_BOM.len() + text.len());
    bytes.extend_from_slice(&CSV_BOM);
    bytes.extend_from_slice(text.as_bytes());
    Ok(bytes)
}

/// 生成 TXT 字节（UTF-8，定宽对齐）。
pub fn export_txt(
    results: &[ProbeResult],
    meta: &ExportMeta,
    thresholds: &Thresholds,
) -> CoreResult<Vec<u8>> {
    let sorted = sorted_by_ip(results);
    let stats = ScanStats::from_results(&sorted);

    let mut out = String::new();
    out.push_str("C网群PING工具 · 扫描结果\n");
    out.push_str("============================================================\n");
    out.push_str(&format!("扫描时间 : {}\n", meta.scan_time));
    out.push_str(&format!("本机 IP  : {}\n", meta.local_ip));
    out.push_str(&format!("用时     : {}\n", meta.elapsed_human()));
    out.push_str(&format!("目标总数 : {}\n", stats.total));
    out.push_str(&format!("在线主机 : {}\n", stats.online));
    out.push_str(&format!("超时     : {}\n", stats.timeout));
    out.push_str(&format!("不可达   : {}\n", stats.unreachable));
    if stats.error > 0 {
        out.push_str(&format!("错误     : {}\n", stats.error));
    }
    out.push_str("============================================================\n");

    const W_IP: usize = 17;
    const W_STATUS: usize = 12;
    const W_RTT: usize = 9;
    const W_MAC: usize = 19;
    const W_VENDOR: usize = 22;
    const W_HOST: usize = 18;
    /// 尾部「类别」列宽度（最长为「无回应」= 6 列）。
    const W_CLASS: usize = 8;
    const TOTAL_W: usize = W_IP + W_STATUS + W_RTT + W_MAC + W_VENDOR + W_HOST + W_CLASS;

    out.push_str(&format!(
        "{}{}{}{}{}{}{}\n",
        pad("IP地址", W_IP),
        pad("状态", W_STATUS),
        pad("RTT(ms)", W_RTT),
        pad("MAC地址", W_MAC),
        pad("厂商", W_VENDOR),
        pad("主机名", W_HOST),
        "类别",
    ));
    out.push_str(&"-".repeat(TOTAL_W));
    out.push('\n');

    for r in &sorted {
        let rtt = r.status.rtt_ms().map(|v| v.to_string()).unwrap_or_default();
        let mac = r.mac.clone().unwrap_or_default();
        let class = classify_status(&r.status, thresholds)
            .map(|c| c.label_zh())
            .unwrap_or("-");
        out.push_str(&format!(
            "{}{}{}{}{}{}{}\n",
            pad(&r.ip.to_string(), W_IP),
            pad(r.status.label_zh(), W_STATUS),
            pad(&rtt, W_RTT),
            pad(&mac, W_MAC),
            // 定宽表里超长内容必须截断，否则整张表的列会错位
            pad(&truncate_display(r.vendor_or_empty(), W_VENDOR), W_VENDOR),
            pad(&truncate_display(r.hostname_or_empty(), W_HOST), W_HOST),
            class,
        ));
    }

    Ok(out.into_bytes())
}

/// 统一入口。
pub fn export(
    results: &[ProbeResult],
    format: ExportFormat,
    meta: &ExportMeta,
    thresholds: &Thresholds,
) -> CoreResult<Vec<u8>> {
    match format {
        ExportFormat::Csv => export_csv(results, meta),
        ExportFormat::Txt => export_txt(results, meta, thresholds),
    }
}

/// 根据扩展名写出文件。GUI 的 Android 端不走这里（走平台文件保存），TUI 与桌面端使用。
pub fn export_to_path(
    results: &[ProbeResult],
    path: &str,
    meta: &ExportMeta,
    thresholds: &Thresholds,
) -> CoreResult<Vec<u8>> {
    let format = ExportFormat::from_path(path).ok_or_else(|| {
        CoreError::Export(format!("无法从 {path:?} 推断格式，仅支持 .csv / .txt"))
    })?;
    export(results, format, meta, thresholds)
}

// ---------------------------------------------------------------------------
// 定宽对齐：按显示宽度计算（CJK 记 2 列）
// ---------------------------------------------------------------------------

pub fn display_width(s: &str) -> usize {
    s.chars().map(char_width).sum()
}

/// 单个字符的显示宽度（CJK / 全角记 2 列）。
pub fn char_width(c: char) -> usize {
    let cp = c as u32;
    // 粗略覆盖 CJK / 全角区间
    if (0x1100..=0x115F).contains(&cp)
        || (0x2E80..=0xA4CF).contains(&cp)
        || (0xAC00..=0xD7A3).contains(&cp)
        || (0xF900..=0xFAFF).contains(&cp)
        || (0xFE30..=0xFE6F).contains(&cp)
        || (0xFF00..=0xFF60).contains(&cp)
        || (0xFFE0..=0xFFE6).contains(&cp)
    {
        2
    } else {
        1
    }
}

/// 按显示宽度截断，超宽时以 `…` 结尾，保证结果宽度 **不超过** `width`。
///
/// 定宽文本表（TXT 导出）必须用它：厂商名最长可达 60+ 字符，直接写入会把
/// 后面所有列挤走，整张表就失去可读性了。
pub fn truncate_display(s: &str, width: usize) -> String {
    if display_width(s) <= width {
        return s.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let budget = width - 1; // 留一列给省略号
    let mut out = String::with_capacity(s.len());
    let mut used = 0usize;
    for ch in s.chars() {
        let w = char_width(ch);
        if used + w > budget {
            break;
        }
        out.push(ch);
        used += w;
    }
    out.push('…');
    out
}

pub fn pad(s: &str, width: usize) -> String {
    let w = display_width(s);
    if w >= width {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(width - w))
    }
}
