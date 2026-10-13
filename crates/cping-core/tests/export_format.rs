//! 导出格式：BOM、CRLF、转义、排序、空字段、统计一致性。

use cping_core::classify::Thresholds;
use cping_core::export::{
    csv_escape, display_width, export, export_csv, export_txt, ExportFormat, ExportMeta,
};
use cping_core::result::{ProbeResult, ProbeStatus, ScanStats};
use cping_core::{CoreError, Ipv4Prefix};
use std::net::Ipv4Addr;

fn meta() -> ExportMeta {
    ExportMeta {
        scan_time: "2026-10-13 21:40:00".into(),
        local_ip: "192.168.1.100".into(),
        elapsed_ms: 1234,
    }
}

fn sample() -> Vec<ProbeResult> {
    vec![
        // 故意乱序，验证按 IP 数值排序
        ProbeResult {
            index: 2,
            ip: Ipv4Addr::new(192, 168, 1, 3),
            mac: Some("AA-BB-CC-DD-EE-FF".into()),
            status: ProbeStatus::Online { rtt_ms: 250 },
        },
        ProbeResult {
            index: 0,
            ip: Ipv4Addr::new(192, 168, 1, 1),
            mac: None,
            status: ProbeStatus::Online { rtt_ms: 12 },
        },
        ProbeResult {
            index: 1,
            ip: Ipv4Addr::new(192, 168, 1, 2),
            mac: None,
            status: ProbeStatus::Timeout,
        },
        ProbeResult {
            index: 3,
            ip: Ipv4Addr::new(192, 168, 1, 10),
            mac: None,
            status: ProbeStatus::PermissionDenied,
        },
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
    assert_eq!(header, "扫描时间,IP地址,状态,RTT(ms),MAC地址,备注");
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
    let results = vec![ProbeResult {
        index: 0,
        ip: Ipv4Addr::new(10, 0, 0, 1),
        mac: Some("AA,BB".into()),
        status: ProbeStatus::Online { rtt_ms: 5 },
    }];
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
    assert_eq!(ExportFormat::from_path("data.json"), None);
    assert_eq!(ExportFormat::Csv.extension(), "csv");
    assert_eq!(ExportFormat::Txt.extension(), "txt");

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
    assert_eq!(text, "扫描时间,IP地址,状态,RTT(ms),MAC地址,备注\r\n");

    let txt = export_txt(&[], &meta(), &Thresholds::default()).unwrap();
    let t = String::from_utf8(txt).unwrap();
    assert!(t.contains("目标总数 : 0"));
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
    let results = vec![ProbeResult {
        index: 0,
        ip: p.with_host(1),
        mac: None,
        status: ProbeStatus::Online { rtt_ms: 3 },
    }];
    let csv = export_csv(&results, &meta()).unwrap();
    let text = String::from_utf8(csv[3..].to_vec()).unwrap();
    assert!(text.contains(&p.with_host(1).to_string()));
}
