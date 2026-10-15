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
use crate::config::ColorConfig;
use crate::error::{CoreError, CoreResult};
use crate::fingerprint::HostFingerprint;
use crate::json::Json;
use crate::monitor::Change;
use crate::result::{ProbeResult, ProbeStatus, ScanStats};
use crate::spec::ProbeMode;
use std::net::IpAddr;

pub const CSV_BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

/// 导出格式的**报告版本号**：结构性变更时递增，消费方据此判断能否解析。
pub const REPORT_FORMAT_VERSION: u32 = 2;

pub const CSV_HEADER: [&str; 11] = [
    "扫描时间",
    "IP地址",
    "状态",
    "RTT(ms)",
    "MAC地址",
    "备注",
    "厂商",
    "主机名",
    // M9 追加列：仍然只在末尾追加，旧列位置与内容不变
    "探测方式",
    // M13 追加列：端口指纹**不参与在线判定**，只作为附加信息列出现
    "开放端口",
    "设备推断",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Csv,
    Txt,
    /// 结构化 JSON（M10）。
    Json,
    /// 单文件自包含 HTML 报告（M10）。
    Html,
}

impl ExportFormat {
    pub const ALL: [ExportFormat; 4] = [
        ExportFormat::Csv,
        ExportFormat::Txt,
        ExportFormat::Json,
        ExportFormat::Html,
    ];

    pub const fn extension(self) -> &'static str {
        match self {
            ExportFormat::Csv => "csv",
            ExportFormat::Txt => "txt",
            ExportFormat::Json => "json",
            ExportFormat::Html => "html",
        }
    }

    pub const fn label_zh(self) -> &'static str {
        match self {
            ExportFormat::Csv => "CSV",
            ExportFormat::Txt => "TXT",
            ExportFormat::Json => "JSON",
            ExportFormat::Html => "HTML 报告",
        }
    }

    /// 由文件名/路径推断格式（大小写不敏感），无法识别时返回 `None`。
    pub fn from_path(path: &str) -> Option<Self> {
        let lower = path.to_ascii_lowercase();
        for fmt in Self::ALL {
            if lower.ends_with(&format!(".{}", fmt.extension())) {
                return Some(fmt);
            }
        }
        None
    }
}

/// 导出时附加的元信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportMeta {
    /// 扫描开始时间（调用方格式化，推荐 `YYYY-MM-DD HH:MM:SS`）。
    pub scan_time: String,
    pub local_ip: String,
    pub elapsed_ms: u64,
    /// 探测方式（M9）：决定状态列显示成「在线」还是 `TCP-OPEN`。
    pub mode: ProbeMode,
    /// 目标范围描述（M9/M10），例如 `192.168.1（255 个）`。
    pub target_desc: String,
    /// 轮次编号（M8 持续监控）；`0` 表示单次扫描。
    pub round: u64,
}

impl Default for ExportMeta {
    fn default() -> Self {
        Self {
            scan_time: String::new(),
            local_ip: String::new(),
            elapsed_ms: 0,
            mode: ProbeMode::Icmp,
            target_desc: String::new(),
            round: 0,
        }
    }
}

impl ExportMeta {
    /// 用时的人类可读形式，例如 `1.234 s`。
    pub fn elapsed_human(&self) -> String {
        format!("{:.3} s", self.elapsed_ms as f64 / 1000.0)
    }

    /// 状态列在该模式下的展示文本。
    pub fn status_label(&self, status: &ProbeStatus) -> String {
        self.mode.status_label(status)
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

/// 目标地址的稳定排序键：`(族, 数值)`，族 0 = IPv4、1 = IPv6。
///
/// 与 [`crate::spec::addr_sort_key`] 同一套规则——排序口径只定义一次，
/// 导出、差异、监控三处不得各写一份。
pub fn ip_sort_key(ip: IpAddr) -> (u8, u128) {
    crate::spec::addr_sort_key(ip)
}

/// 按地址升序返回结果副本（v4 在前、v6 在后，同族内按数值）。
pub fn sorted_by_ip(results: &[ProbeResult]) -> Vec<ProbeResult> {
    let mut out = results.to_vec();
    out.sort_by_key(|r| ip_sort_key(r.ip));
    out
}

/// 生成 CSV 字节（含 BOM）。不带指纹数据——等价于 `export_csv_with_fingerprints(.., &[])`。
pub fn export_csv(results: &[ProbeResult], meta: &ExportMeta) -> CoreResult<Vec<u8>> {
    export_csv_with_fingerprints(results, meta, &[])
}

/// 生成 CSV 字节（含 BOM），并把端口指纹作为**末尾两列**附上。
///
/// 指纹列**只描述端口**，不改变任何既有列（状态/RTT/在线判定一律不动）——
/// 这是「端口扫描与连通性结果分列」这条验收要求在导出侧的落地。
pub fn export_csv_with_fingerprints(
    results: &[ProbeResult],
    meta: &ExportMeta,
    fingerprints: &[HostFingerprint],
) -> CoreResult<Vec<u8>> {
    let mut text = String::new();
    text.push_str(&CSV_HEADER.join(","));
    text.push_str("\r\n");

    for r in sorted_by_ip(results) {
        let rtt = r.status.rtt_ms().map(|v| v.to_string()).unwrap_or_default();
        let mac = r.mac.clone().unwrap_or_default();
        let note = r.status.note().unwrap_or("").to_string();
        let fp = fingerprint_for(fingerprints, r.ip);
        let open_ports = fp.map(|f| f.open_ports_summary()).unwrap_or_default();
        let device = fp.map(|f| f.device_summary()).unwrap_or_default();
        let fields = [
            meta.scan_time.clone(),
            r.ip.to_string(),
            meta.status_label(&r.status),
            rtt,
            mac,
            note,
            // M7 追加列：必须留在末尾
            r.vendor.clone().unwrap_or_default(),
            r.hostname.clone().unwrap_or_default(),
            // M9 追加列：同样只在末尾追加
            meta.mode.describe(),
            // M13 追加列（仅开放端口；关闭端口不出现）
            open_ports,
            device,
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

/// 按 IP 取指纹条目（线性查找：一次扫描的主机数有上限，不值得建 HashMap）。
fn fingerprint_for(fingerprints: &[HostFingerprint], ip: IpAddr) -> Option<&HostFingerprint> {
    fingerprints.iter().find(|f| f.ip() == ip)
}

/// 生成 TXT 字节（UTF-8，定宽对齐）。不带指纹数据。
pub fn export_txt(
    results: &[ProbeResult],
    meta: &ExportMeta,
    thresholds: &Thresholds,
) -> CoreResult<Vec<u8>> {
    export_txt_with_fingerprints(results, meta, thresholds, &[])
}

/// 生成 TXT 字节：定宽明细表 + **独立的**端口指纹小节。
///
/// 指纹小节排在明细表之后，标题里写明「与在线判定相互独立」，
/// 避免读者把「端口全关」误读成「主机离线」。
pub fn export_txt_with_fingerprints(
    results: &[ProbeResult],
    meta: &ExportMeta,
    thresholds: &Thresholds,
    fingerprints: &[HostFingerprint],
) -> CoreResult<Vec<u8>> {
    let sorted = sorted_by_ip(results);
    let stats = ScanStats::from_results(&sorted);

    let mut out = String::new();
    out.push_str("C网群PING工具 · 扫描结果\n");
    out.push_str("============================================================\n");
    out.push_str(&format!("扫描时间 : {}\n", meta.scan_time));
    out.push_str(&format!("探测方式 : {}\n", meta.mode.describe()));
    if !meta.target_desc.is_empty() {
        out.push_str(&format!("目标范围 : {}\n", meta.target_desc));
    }
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
            pad(
                &truncate_display(&meta.status_label(&r.status), W_STATUS),
                W_STATUS
            ),
            pad(&rtt, W_RTT),
            pad(&mac, W_MAC),
            // 定宽表里超长内容必须截断，否则整张表的列会错位
            pad(&truncate_display(r.vendor_or_empty(), W_VENDOR), W_VENDOR),
            pad(&truncate_display(r.hostname_or_empty(), W_HOST), W_HOST),
            class,
        ));
    }

    // --- M13：端口指纹（独立小节，不与在线判定混排）-------------------------
    if !fingerprints.is_empty() {
        out.push('\n');
        out.push_str("端口指纹（附加信息，与上面的在线判定相互独立）\n");
        out.push_str("------------------------------------------------------------\n");
        out.push_str(&format!(
            "探测端口数 : {} 个\n",
            fingerprints[0].report.scanned
        ));
        out.push_str(&format!(
            "开放端口合计 : {} 个（出现在 {} 台主机上）\n",
            fingerprints
                .iter()
                .map(|f| f.report.findings.len())
                .sum::<usize>(),
            fingerprints.iter().filter(|f| f.has_open_ports()).count()
        ));
        out.push_str("------------------------------------------------------------\n");
        for fp in fingerprints {
            let ip = fp.ip().to_string();
            if !fp.has_open_ports() {
                // 端口全关**不等于**主机离线，这里必须说清楚
                out.push_str(&format!("{:<17} 无开放端口（不影响其在线状态）\n", ip));
                continue;
            }
            out.push_str(&format!(
                "{:<17} {}\n",
                ip,
                truncate_display(&fp.open_ports_summary(), 60)
            ));
            for f in &fp.report.findings {
                if let Some(banner) = &f.banner {
                    out.push_str(&format!(
                        "{:<17}   :{:<6} {}\n",
                        "",
                        f.port,
                        truncate_display(banner, 60)
                    ));
                }
            }
            if !fp.device_summary().is_empty() {
                out.push_str(&format!(
                    "{:<17}   {}\n",
                    "",
                    truncate_display(&fp.device_summary(), 90)
                ));
            }
        }
    }

    Ok(out.into_bytes())
}

/// 统一入口（CSV / TXT）。
///
/// JSON / HTML 需要额外的统计与变化信息，请用 [`ScanReport::render`]；
/// 这里为兼容既有调用方保留，遇到 JSON/HTML 时会用最小报告包装。
pub fn export(
    results: &[ProbeResult],
    format: ExportFormat,
    meta: &ExportMeta,
    thresholds: &Thresholds,
) -> CoreResult<Vec<u8>> {
    match format {
        ExportFormat::Csv => export_csv(results, meta),
        ExportFormat::Txt => export_txt(results, meta, thresholds),
        ExportFormat::Json | ExportFormat::Html => {
            ScanReport::new(results, meta.clone(), *thresholds, Vec::new())
                .render(format, &ColorConfig::default())
        }
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
        CoreError::Export(format!(
            "无法从 {path:?} 推断格式，仅支持 .csv / .txt / .json / .html"
        ))
    })?;
    export(results, format, meta, thresholds)
}

// ---------------------------------------------------------------------------
// 扫描报告：CSV / TXT / JSON / HTML 的唯一构造点
// ---------------------------------------------------------------------------

/// 一份完整的扫描报告。
///
/// 四种导出格式都从这一个结构渲染，保证「界面看到的」与「文件里的」
/// 出自同一份数据与同一套口径。
#[derive(Debug, Clone, PartialEq)]
pub struct ScanReport {
    pub meta: ExportMeta,
    /// 已按 IP 数值升序排列。
    pub results: Vec<ProbeResult>,
    pub stats: ScanStats,
    pub thresholds: Thresholds,
    /// 本轮变化（M8）；单次扫描时为空。
    pub changes: Vec<Change>,
    /// 端口指纹（M13）；未启用指纹时为**空**。
    ///
    /// 刻意与 `results` 并列而不是塞进 `ProbeResult`：端口扫描是一趟**独立**
    /// 探测，把它混进结果模型会让「在线」判定出现第二个来源。分开存，
    /// 任何消费方都必须显式选择性的合并。
    pub fingerprints: Vec<HostFingerprint>,
}

impl ScanReport {
    /// 构造报告；`results` 会被排序去引用，原始切片不变。
    pub fn new(
        results: &[ProbeResult],
        meta: ExportMeta,
        thresholds: Thresholds,
        changes: Vec<Change>,
    ) -> Self {
        let sorted = sorted_by_ip(results);
        let stats = ScanStats::from_results_with_total(&sorted, sorted.len());
        Self {
            meta,
            results: sorted,
            stats,
            thresholds,
            changes,
            fingerprints: Vec::new(),
        }
    }

    /// 附上端口指纹（M13）。空切片等价于「没做指纹」。
    pub fn with_fingerprints(mut self, fingerprints: Vec<HostFingerprint>) -> Self {
        self.fingerprints = fingerprints;
        self
    }

    /// 按格式渲染为字节缓冲（调用方决定落盘方式）。
    pub fn render(&self, format: ExportFormat, palette: &ColorConfig) -> CoreResult<Vec<u8>> {
        match format {
            ExportFormat::Csv => {
                export_csv_with_fingerprints(&self.results, &self.meta, &self.fingerprints)
            }
            ExportFormat::Txt => export_txt_with_fingerprints(
                &self.results,
                &self.meta,
                &self.thresholds,
                &self.fingerprints,
            ),
            ExportFormat::Json => Ok(self.to_json_text().into_bytes()),
            ExportFormat::Html => Ok(crate::html::render_report(self, palette).into_bytes()),
        }
    }

    /// 结构化 JSON 文本（两空格缩进，键序稳定）。
    pub fn to_json_text(&self) -> String {
        self.to_json().render_pretty()
    }

    /// 结构化 JSON 值模型。
    pub fn to_json(&self) -> Json {
        let meta = &self.meta;
        let stats = &self.stats;

        let targets = Json::array(self.results.iter().map(|r| {
            let class = classify_status(&r.status, &self.thresholds);
            Json::object([
                ("index", Json::UInt(r.index as u64)),
                ("ip", Json::str(r.ip.to_string())),
                ("status", Json::str(r.status.key())),
                ("status_label", Json::str(meta.status_label(&r.status))),
                ("online", Json::Bool(r.status.is_online())),
                (
                    "rtt_ms",
                    match r.status.rtt_ms() {
                        Some(v) => Json::UInt(v as u64),
                        None => Json::Null,
                    },
                ),
                (
                    "class",
                    match class {
                        Some(c) => Json::str(c.key()),
                        None => Json::Null,
                    },
                ),
                ("mac", Json::opt_str(r.mac.clone())),
                ("vendor", Json::opt_str(r.vendor.clone())),
                ("hostname", Json::opt_str(r.hostname.clone())),
                ("note", Json::opt_str(r.status.note().map(str::to_string))),
            ])
        }));

        let changes = Json::array(self.changes.iter().map(|c| {
            Json::object([
                ("round", Json::UInt(c.round)),
                ("ip", Json::str(c.ip.to_string())),
                ("kind", Json::str(c.kind.key())),
                ("from", Json::str(c.from.key())),
                ("to", Json::str(c.to.key())),
            ])
        }));

        // M13：端口指纹独立成数组。`device.is_inference` 恒为 true，
        // 让机器消费者也无法把推断结论误当成事实。
        let fingerprints = Json::array(self.fingerprints.iter().map(|fp| {
            let open = Json::array(fp.report.findings.iter().map(|f| {
                Json::object([
                    ("port", Json::UInt(f.port as u64)),
                    (
                        "service",
                        match f.service {
                            Some(s) => Json::str(s),
                            None => Json::Null,
                        },
                    ),
                    ("banner", Json::opt_str(f.banner.clone())),
                    ("connect_rtt_ms", Json::UInt(f.connect_rtt_ms as u64)),
                ])
            }));
            let device = match &fp.device {
                Some(d) => Json::object([
                    ("kind", Json::str(d.kind)),
                    ("confidence", Json::str(d.confidence.label_zh())),
                    ("is_inference", Json::Bool(true)),
                    (
                        "basis",
                        Json::array(d.basis.iter().map(|b| Json::str(b.clone()))),
                    ),
                ]),
                None => Json::Null,
            };
            Json::object([
                ("ip", Json::str(fp.ip().to_string())),
                ("scanned", Json::UInt(fp.report.scanned as u64)),
                ("elapsed_ms", Json::UInt(fp.report.elapsed_ms)),
                ("open_ports", open),
                ("device", device),
            ])
        }));

        Json::object([
            ("tool", Json::str("cping")),
            ("version", Json::str(crate::VERSION)),
            ("format_version", Json::UInt(REPORT_FORMAT_VERSION as u64)),
            (
                "meta",
                Json::object([
                    ("scan_time", Json::str(meta.scan_time.clone())),
                    ("local_ip", Json::str(meta.local_ip.clone())),
                    ("elapsed_ms", Json::UInt(meta.elapsed_ms)),
                    ("mode", Json::str(meta.mode.key())),
                    (
                        "tcp_port",
                        match meta.mode.tcp_port() {
                            Some(p) => Json::UInt(p as u64),
                            None => Json::Null,
                        },
                    ),
                    ("target_desc", Json::str(meta.target_desc.clone())),
                    ("round", Json::UInt(meta.round)),
                ]),
            ),
            (
                "stats",
                Json::object([
                    ("total", Json::UInt(stats.total as u64)),
                    ("completed", Json::UInt(stats.completed as u64)),
                    ("pending", Json::UInt(stats.pending as u64)),
                    ("online", Json::UInt(stats.online as u64)),
                    ("timeout", Json::UInt(stats.timeout as u64)),
                    ("unreachable", Json::UInt(stats.unreachable as u64)),
                    (
                        "permission_denied",
                        Json::UInt(stats.permission_denied as u64),
                    ),
                    ("unsupported", Json::UInt(stats.unsupported as u64)),
                    ("error", Json::UInt(stats.error as u64)),
                ]),
            ),
            ("changes", changes),
            ("fingerprints", fingerprints),
            ("targets", targets),
        ])
    }
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
