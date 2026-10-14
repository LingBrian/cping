//! 导出格式：BOM、CRLF、转义、排序、空字段、统计一致性。

use cping_core::classify::Thresholds;
use cping_core::export::{
    csv_escape, display_width, export, export_csv, export_txt, truncate_display, ExportFormat,
    ExportMeta,
};
use cping_core::result::{ProbeResult, ProbeStatus, ScanStats};
use cping_core::{CoreError, Ipv4Prefix};
use std::net::IpAddr;
use std::net::Ipv4Addr;

fn meta() -> ExportMeta {
    ExportMeta {
        scan_time: "2026-10-13 21:40:00".into(),
        local_ip: "192.168.1.100".into(),
        elapsed_ms: 1234,
        ..ExportMeta::default()
    }
}

fn sample() -> Vec<ProbeResult> {
    vec![
        // 故意乱序，验证按 IP 数值排序
        ProbeResult::new(
            2,
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 3)),
            ProbeStatus::Online { rtt_ms: 250 },
        )
        .with_target_info(Some("AA-BB-CC-DD-EE-FF".into()), None, None),
        ProbeResult::new(
            0,
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
            ProbeStatus::Online { rtt_ms: 12 },
        ),
        ProbeResult::new(
            1,
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2)),
            ProbeStatus::Timeout,
        ),
        ProbeResult::new(
            3,
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
            ProbeStatus::PermissionDenied,
        ),
    ]
}

#[test]
fn escape_follows_rfc4180() {
    assert_eq!(csv_escape("plain"), "plain");
    assert_eq!(csv_escape("a,b"), "\"a,b\"");
    assert_eq!(csv_escape("say \"hi\""), "\"say \"\"hi\"\"\"");
    assert_eq!(csv_escape("line1\nline2"), "\"line1\nline2\"");
    assert_eq!(csv_escape("cr\rlf"), "\"cr\rlf\"");
    assert_eq!(csv_escape(""), "");
}

#[test]
fn csv_starts_with_bom_and_has_crlf() {
    let bytes = export_csv(&sample(), &meta()).unwrap();
    assert_eq!(&bytes[..3], &[0xEF, 0xBB, 0xBF], "CSV 必须带 UTF-8 BOM");
    let text = String::from_utf8(bytes[3..].to_vec()).unwrap();
    assert!(
        !text.contains('\n') || !text.contains("\n\n"),
        "不应出现空行"
    );
    // 每一行都以 CRLF 结束
    for line in text.split("\r\n").filter(|l| !l.is_empty()) {
        assert!(!line.contains('\n'), "行内不应有裸 LF：{line:?}");
    }
    assert!(text.ends_with("\r\n"));
}

#[test]
fn csv_header_is_exact() {
    let bytes = export_csv(&sample(), &meta()).unwrap();
    let text = String::from_utf8(bytes[3..].to_vec()).unwrap();
    let header = text.split("\r\n").next().unwrap();
    assert_eq!(
        header, "扫描时间,IP地址,状态,RTT(ms),MAC地址,备注,厂商,主机名,探测方式",
        "M7/M9 新列只能追加在末尾"
    );
    // 前 6 列必须与 M7 之前逐字节一致（旧脚本按列号解析仍然可用）
    let legacy = header.split(',').take(6).collect::<Vec<_>>().join(",");
    assert_eq!(legacy, "扫描时间,IP地址,状态,RTT(ms),MAC地址,备注");
    // 第 7/8 列必须与 M7 逐字节一致（M9 只在第 9 列追加）
    let m7 = header
        .split(',')
        .skip(6)
        .take(2)
        .collect::<Vec<_>>()
        .join(",");
    assert_eq!(m7, "厂商,主机名");
}

#[test]
fn csv_rows_are_sorted_by_numeric_ip() {
    let bytes = export_csv(&sample(), &meta()).unwrap();
    let text = String::from_utf8(bytes[3..].to_vec()).unwrap();
    let ips: Vec<&str> = text
        .split("\r\n")
        .skip(1)
        .filter(|l| !l.is_empty())
        .map(|l| l.split(',').nth(1).unwrap())
        .collect();
    assert_eq!(
        ips,
        vec!["192.168.1.1", "192.168.1.2", "192.168.1.3", "192.168.1.10"],
        "数字排序：.10 必须排在 .3 之后"
    );
}

#[test]
fn csv_keeps_empty_rtt_and_mac() {
    let bytes = export_csv(&sample(), &meta()).unwrap();
    let text = String::from_utf8(bytes[3..].to_vec()).unwrap();
    let timeout_row = text
        .split("\r\n")
        .find(|l| l.contains("192.168.1.2"))
        .unwrap();
    let cols: Vec<&str> = timeout_row.split(',').collect();
    assert_eq!(cols[2], "超时");
    assert_eq!(cols[3], "", "非在线目标的 RTT 必须为空");
    assert_eq!(cols[4], "", "MAC 取不到时必须为空，不得伪造");
}

#[test]
fn csv_records_error_status_text() {
    let bytes = export_csv(&sample(), &meta()).unwrap();
    let text = String::from_utf8(bytes[3..].to_vec()).unwrap();
    let row = text
        .split("\r\n")
        .find(|l| l.contains("192.168.1.10"))
        .unwrap();
    let cols: Vec<&str> = row.split(',').collect();
    assert_eq!(cols[2], "权限不足", "权限错误必须与超时区分");
}

#[test]
fn csv_escapes_mac_with_quotes_if_needed() {
    let results = vec![ProbeResult::new(
        0,
        IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
        ProbeStatus::Online { rtt_ms: 5 },
    )
    .with_target_info(Some("AA,BB".into()), None, None)];
    let bytes = export_csv(&results, &meta()).unwrap();
    let text = String::from_utf8(bytes[3..].to_vec()).unwrap();
    assert!(
        text.contains(",\"AA,BB\","),
        "含逗号的字段必须加引号：{text}"
    );
}

#[test]
fn txt_contains_summary_and_rows() {
    let bytes = export_txt(&sample(), &meta(), &Thresholds::default()).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("扫描时间 : 2026-10-13 21:40:00"));
    assert!(text.contains("本机 IP  : 192.168.1.100"));
    assert!(text.contains("用时     : 1.234 s"));
    assert!(text.contains("目标总数 : 4"));
    assert!(text.contains("在线主机 : 2"));
    assert!(text.contains("超时     : 1"));
    assert!(text.contains("权限不足"));
    assert!(text.contains("192.168.1.10"));
}

#[test]
fn txt_uses_lf_only() {
    let bytes = export_txt(&sample(), &meta(), &Thresholds::default()).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    assert!(!text.contains('\r'), "TXT 使用 LF 换行");
}

#[test]
fn format_dispatch_matches_extension() {
    assert_eq!(
        ExportFormat::from_path("a/b/c.CSV"),
        Some(ExportFormat::Csv)
    );
    assert_eq!(ExportFormat::from_path("out.txt"), Some(ExportFormat::Txt));
    assert_eq!(
        ExportFormat::from_path("data.json"),
        Some(ExportFormat::Json)
    );
    assert_eq!(
        ExportFormat::from_path("report.HTML"),
        Some(ExportFormat::Html)
    );
    assert_eq!(ExportFormat::from_path("noext"), None);
    assert_eq!(ExportFormat::Csv.extension(), "csv");
    assert_eq!(ExportFormat::Txt.extension(), "txt");
    assert_eq!(ExportFormat::Json.extension(), "json");
    assert_eq!(ExportFormat::Html.extension(), "html");

    let csv = export(
        &sample(),
        ExportFormat::Csv,
        &meta(),
        &Thresholds::default(),
    )
    .unwrap();
    assert_eq!(&csv[..3], &[0xEF, 0xBB, 0xBF]);
    let txt = export(
        &sample(),
        ExportFormat::Txt,
        &meta(),
        &Thresholds::default(),
    )
    .unwrap();
    assert_ne!(&txt[..3.min(txt.len())], &[0xEF, 0xBB, 0xBF]);
}

#[test]
fn export_to_path_rejects_unknown_extension() {
    let err =
        cping_core::export::export_to_path(&sample(), "out.bin", &meta(), &Thresholds::default())
            .unwrap_err();
    assert!(matches!(err, CoreError::Export(_)));
}

#[test]
fn stats_are_consistent_with_results() {
    let results = sample();
    let stats = ScanStats::from_results(&results);
    assert_eq!(stats.total, 4);
    assert_eq!(stats.completed, 4);
    assert_eq!(stats.pending, 0);
    assert_eq!(stats.online, 2);
    assert_eq!(stats.timeout, 1);
    assert_eq!(stats.permission_denied, 1);
    assert_eq!(stats.unreachable, 0);
    assert_eq!(stats.error, 0);
    assert_eq!(stats.timeout + stats.permission_denied, 2);
    assert!((stats.progress() - 1.0).abs() < f64::EPSILON);
}

#[test]
fn empty_export_is_still_valid() {
    let bytes = export_csv(&[], &meta()).unwrap();
    let text = String::from_utf8(bytes[3..].to_vec()).unwrap();
    assert_eq!(
        text,
        "扫描时间,IP地址,状态,RTT(ms),MAC地址,备注,厂商,主机名,探测方式\r\n"
    );

    let txt = export_txt(&[], &meta(), &Thresholds::default()).unwrap();
    let t = String::from_utf8(txt).unwrap();
    assert!(t.contains("目标总数 : 0"));
}

/// M7 向后兼容：没有新信息时，前 6 列的内容与 M7 之前必须逐字节一致。
#[test]
fn legacy_columns_are_byte_identical_when_no_new_info() {
    let bytes = export_csv(&sample(), &meta()).unwrap();
    let text = String::from_utf8(bytes[3..].to_vec()).unwrap();
    let mut lines = text.split("\r\n").filter(|l| !l.is_empty());
    lines.next(); // 表头已单独校验
    let mut checked = 0;
    for line in lines {
        let cols: Vec<&str> = line.split(',').collect();
        assert_eq!(cols.len(), 9, "新列必须存在（可为空），不能缺列: {line}");
        assert_eq!(cols[0], "2026-10-13 21:40:00");
        // 后两列在本样例里没有任何信息 → 必须是空串而不是占位文本
        assert_eq!(cols[6], "", "无厂商信息时必须留空: {line}");
        assert_eq!(cols[7], "", "无主机名信息时必须留空: {line}");
        // M9 追加列：默认 ICMP
        assert_eq!(cols[8], "ICMP", "探测方式列必须写出后端语义: {line}");
        checked += 1;
    }
    assert_eq!(checked, 4);
}

/// M7 新列：有信息时正常写出，且遵守 RFC 4180 转义。
#[test]
fn new_columns_are_written_and_escaped() {
    let results = vec![
        ProbeResult::new(
            0,
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            ProbeStatus::Online { rtt_ms: 4 },
        )
        .with_target_info(
            Some("00:00:0C:11:22:33".into()),
            Some("Cisco Systems, Inc".into()),
            Some("router.example.com".into()),
        ),
        // 厂商名里带逗号 → 必须加引号，否则列会错位
        ProbeResult::new(
            1,
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)),
            ProbeStatus::Online { rtt_ms: 9 },
        )
        .with_target_info(None, Some("Acme, Inc.".into()), None),
    ];
    let bytes = export_csv(&results, &meta()).unwrap();
    let text = String::from_utf8(bytes[3..].to_vec()).unwrap();
    let rows: Vec<&str> = text.split("\r\n").filter(|l| !l.is_empty()).collect();

    assert!(
        rows[1].ends_with("\"Cisco Systems, Inc\",router.example.com,ICMP"),
        "含逗号的厂商名会被加引号，主机名紧随其后: {}",
        rows[1]
    );
    assert!(
        rows[2].ends_with(",\"Acme, Inc.\",,ICMP"),
        "含逗号的厂商名必须被引号包裹: {}",
        rows[2]
    );
}

/// M9：TCP 模式下「状态」列不得再显示成「在线」，否则会被误读为 ICMP 可达。
#[test]
fn tcp_mode_relabels_the_status_column() {
    let results = vec![
        ProbeResult::new(
            0,
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            ProbeStatus::Online { rtt_ms: 4 },
        ),
        ProbeResult::new(
            1,
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)),
            ProbeStatus::Timeout,
        ),
    ];
    let meta = ExportMeta {
        mode: cping_core::ProbeMode::Tcp { port: 80 },
        ..meta()
    };
    let text = String::from_utf8(export_csv(&results, &meta).unwrap()[3..].to_vec()).unwrap();
    let rows: Vec<&str> = text.split("\r\n").filter(|l| !l.is_empty()).collect();
    assert!(rows[1].contains("TCP-OPEN"), "{}", rows[1]);
    assert!(!rows[1].contains("在线"), "{}", rows[1]);
    assert!(rows[1].ends_with("TCP:80"), "{}", rows[1]);
    assert!(rows[2].contains("TCP-超时"), "{}", rows[2]);
}

/// TXT 是定宽表：超长厂商名必须按显示宽度截断，否则后续列全部错位。
#[test]
fn txt_truncates_overlong_columns_and_keeps_alignment() {
    let long_vendor = "Some Extremely Long Vendor Name That Cannot Fit At All";
    let results = vec![
        ProbeResult::new(
            0,
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            ProbeStatus::Online { rtt_ms: 4 },
        )
        .with_target_info(
            Some("00:00:0C:11:22:33".into()),
            Some(long_vendor.into()),
            None,
        ),
        ProbeResult::new(
            1,
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)),
            ProbeStatus::Online { rtt_ms: 9 },
        ),
    ];
    let txt = export_txt(&results, &meta(), &Thresholds::default()).unwrap();
    let text = String::from_utf8(txt).unwrap();

    assert!(!text.contains(long_vendor), "超长厂商名应被截断");
    assert!(
        text.contains("Some Extremely Long V…"),
        "截断结果应是「按显示宽度取满 21 列 + 省略号」: {text}"
    );

    // 定宽表的判据：所有数据行的显示宽度必须完全一致。
    // （不能用「最后一个空格」来定位列——厂商名自身就含空格。）
    let rows: Vec<&str> = text.lines().filter(|l| l.starts_with("10.0.0.")).collect();
    assert_eq!(rows.len(), 2);
    let widths: Vec<usize> = rows.iter().map(|r| display_width(r)).collect();
    assert_eq!(
        widths[0], widths[1],
        "截断后两行的显示宽度必须一致，否则列会错位"
    );
    // 超长厂商名那行不得比正常行更长
    assert!(widths[0] <= widths[1], "截断必须保证不超宽");
}

#[test]
fn truncate_display_respects_width() {
    assert_eq!(truncate_display("ABCDEF", 6), "ABCDEF");
    assert_eq!(truncate_display("ABCDEF", 10), "ABCDEF");
    assert_eq!(truncate_display("ABCDEF", 3), "AB…");
    assert_eq!(display_width(&truncate_display("ABCDEF", 3)), 3);
    // CJK 记 2 列
    assert_eq!(truncate_display("中文中文", 5), "中文…");
    assert_eq!(display_width(&truncate_display("中文中文", 5)), 5);
    // 边界
    assert_eq!(truncate_display("ABC", 0), "");
    assert_eq!(truncate_display("", 5), "");
    assert_eq!(truncate_display("AB", 3), "AB");
}

#[test]
fn display_width_counts_cjk_as_two() {
    assert_eq!(display_width("abc"), 3);
    assert_eq!(display_width("在线"), 4);
    assert_eq!(display_width("IP地址"), 6);
}

#[test]
fn prefix_display_roundtrip_in_export() {
    let p = Ipv4Prefix::new(192, 168, 1);
    let results = vec![ProbeResult::new(
        0,
        IpAddr::V4(p.with_host(1)),
        ProbeStatus::Online { rtt_ms: 3 },
    )];
    let csv = export_csv(&results, &meta()).unwrap();
    let text = String::from_utf8(csv[3..].to_vec()).unwrap();
    assert!(text.contains(&p.with_host(1).to_string()));
}
