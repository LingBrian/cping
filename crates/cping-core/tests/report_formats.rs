//! M10 报告格式：JSON 结构、HTML 自包含、CSV/TXT/JSON 三者口径一致。

use cping_core::classify::Thresholds;
use cping_core::diff::{diff_reports, DiffKind};
use cping_core::export::{ExportFormat, ExportMeta, ScanReport};
use cping_core::monitor::{Change, ChangeKind, MonitorState};
use cping_core::result::{ProbeResult, ProbeStatus, ScanStats};
use cping_core::{config::ColorConfig, ProbeMode};
use std::net::IpAddr;
use std::net::Ipv4Addr;

fn sample() -> Vec<ProbeResult> {
    vec![
        ProbeResult::new(
            1,
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
            ProbeStatus::Online { rtt_ms: 3 },
        )
        .with_target_info(
            Some("AA:BB:CC:DD:EE:01".into()),
            Some("zte corporation".into()),
            Some("gw.local".into()),
        ),
        ProbeResult::new(
            2,
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2)),
            ProbeStatus::Timeout,
        ),
        ProbeResult::new(
            3,
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 3)),
            ProbeStatus::PermissionDenied,
        ),
    ]
}

fn meta() -> ExportMeta {
    ExportMeta {
        scan_time: "2026-10-15 01:00:00".into(),
        local_ip: "192.168.1.10".into(),
        elapsed_ms: 4321,
        mode: ProbeMode::Icmp,
        target_desc: "192.168.1（255 个）".into(),
        round: 2,
    }
}

fn report_with(changes: Vec<Change>) -> ScanReport {
    ScanReport::new(&sample(), meta(), Thresholds::default(), changes)
}

#[test]
fn json_report_declares_version_and_core_fields() {
    let report = report_with(Vec::new());
    let json = report.to_json();
    assert_eq!(json.get("tool"), Some(&cping_core::Json::str("cping")));
    assert_eq!(
        json.get("format_version"),
        Some(&cping_core::Json::UInt(
            cping_core::REPORT_FORMAT_VERSION as u64
        ))
    );
    let meta = json.get("meta").unwrap();
    assert_eq!(meta.get("mode"), Some(&cping_core::Json::str("icmp")));
    assert_eq!(meta.get("round"), Some(&cping_core::Json::UInt(2)));
    let stats = json.get("stats").unwrap();
    assert_eq!(stats.get("total"), Some(&cping_core::Json::UInt(3)));
    assert_eq!(stats.get("online"), Some(&cping_core::Json::UInt(1)));
    assert_eq!(
        stats.get("permission_denied"),
        Some(&cping_core::Json::UInt(1))
    );
}

#[test]
fn json_targets_carry_every_field_and_sort_by_ip() {
    let json = report_with(Vec::new()).to_json();
    let targets = match json.get("targets") {
        Some(cping_core::Json::Array(items)) => items.clone(),
        other => panic!("targets 应为数组：{other:?}"),
    };
    assert_eq!(targets.len(), 3);
    let first = &targets[0];
    assert_eq!(first.get("ip"), Some(&cping_core::Json::str("192.168.1.1")));
    assert_eq!(first.get("status"), Some(&cping_core::Json::str("online")));
    assert_eq!(first.get("rtt_ms"), Some(&cping_core::Json::UInt(3)));
    assert_eq!(first.get("class"), Some(&cping_core::Json::str("low")));
    assert_eq!(
        first.get("vendor"),
        Some(&cping_core::Json::str("zte corporation"))
    );
    // 第二台是超时：rtt 必须是 null 而不是 0
    assert_eq!(targets[1].get("rtt_ms"), Some(&cping_core::Json::Null));
    assert_eq!(
        targets[1].get("online"),
        Some(&cping_core::Json::Bool(false))
    );
}

#[test]
fn json_is_balanced_and_byte_stable() {
    let report = report_with(Vec::new());
    let text = report.to_json_text();
    assert!(cping_core::json_is_balanced(&text), "渲染结果不配平");
    assert_eq!(text, report.to_json_text(), "两次渲染必须字节一致");
    assert!(text.contains("\n  \"targets\""), "应使用两空格缩进");
}

#[test]
fn json_changes_section_reflects_monitor_events() {
    let mut monitor = MonitorState::new(Default::default()).unwrap();
    let t = Thresholds::default();
    monitor.commit_round(
        &[ProbeResult::new(
            0,
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
            ProbeStatus::Timeout,
        )],
        &t,
    );
    let changes = monitor.commit_round(
        &[ProbeResult::new(
            0,
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
            ProbeStatus::Online { rtt_ms: 2 },
        )],
        &t,
    );
    assert_eq!(changes.len(), 1);
    let json = report_with(changes).to_json();
    let arr = match json.get("changes") {
        Some(cping_core::Json::Array(items)) => items.clone(),
        other => panic!("changes 应为数组：{other:?}"),
    };
    assert_eq!(arr.len(), 1);
    assert_eq!(
        arr[0].get("kind"),
        Some(&cping_core::Json::str(ChangeKind::Appeared.key()))
    );
    assert_eq!(arr[0].get("round"), Some(&cping_core::Json::UInt(2)));
}

#[test]
fn html_report_is_self_contained_and_complete() {
    let bytes = report_with(Vec::new())
        .render(ExportFormat::Html, &ColorConfig::default())
        .unwrap();
    let html = String::from_utf8(bytes).unwrap();
    assert!(html.starts_with("<!DOCTYPE html>"));
    for forbidden in [
        "<script", "<link", "<img", "http://", "https://", "@import", "src=",
    ] {
        assert!(
            !html.contains(forbidden),
            "HTML 报告不得引用外部资源：{forbidden}"
        );
    }
    assert!(html.contains("192.168.1.1"));
    assert!(html.contains("zte corporation"));
}

#[test]
fn all_three_text_formats_agree_on_the_online_count() {
    let report = report_with(Vec::new());
    let palette = ColorConfig::default();
    let csv = String::from_utf8(report.render(ExportFormat::Csv, &palette).unwrap()).unwrap();
    let txt = String::from_utf8(report.render(ExportFormat::Txt, &palette).unwrap()).unwrap();
    let json = String::from_utf8(report.render(ExportFormat::Json, &palette).unwrap()).unwrap();
    let html = String::from_utf8(report.render(ExportFormat::Html, &palette).unwrap()).unwrap();

    let stats = ScanStats::from_results(&sample());
    assert_eq!(stats.online, 1);
    assert!(csv.contains("192.168.1.1"), "CSV 缺目标");
    assert!(txt.contains("在线主机 : 1"), "TXT 统计不一致");
    assert!(json.contains("\"online\": 1"), "JSON 统计不一致");
    assert!(html.contains(">1</span>"), "HTML 统计卡片缺在线数");
}

#[test]
fn report_stats_match_core_stats_exactly() {
    let report = report_with(Vec::new());
    assert_eq!(report.stats, ScanStats::from_results(&sample()));
    assert_eq!(report.stats.total, 3);
    assert_eq!(report.stats.completed, 3);
}

#[test]
fn diff_between_two_reports_is_exportable() {
    let base = vec![
        ProbeResult::new(
            0,
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            ProbeStatus::Timeout,
        ),
        ProbeResult::new(
            1,
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)),
            ProbeStatus::Online { rtt_ms: 100 },
        ),
    ];
    let next = vec![
        ProbeResult::new(
            0,
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            ProbeStatus::Online { rtt_ms: 5 },
        ),
        ProbeResult::new(
            1,
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)),
            ProbeStatus::Online { rtt_ms: 400 },
        ),
    ];
    let diff = diff_reports(&base, &next, &Thresholds::default());
    assert_eq!(diff.count_of(DiffKind::Appeared), 1);
    assert_eq!(diff.count_of(DiffKind::LatencyChanged), 1);
    let csv = String::from_utf8(diff.to_csv().unwrap()[3..].to_vec()).unwrap();
    assert!(csv.contains("新上线"));
    assert!(csv.contains("延迟变化"));
    // 差值是「新 - 旧」
    assert!(csv.contains(",300"), "400 - 100 = 300：{csv}");
}

#[test]
fn tcp_mode_json_reports_port_and_relabels_status() {
    let mut m = meta();
    m.mode = ProbeMode::Tcp { port: 445 };
    let report = ScanReport::new(&sample(), m, Thresholds::default(), Vec::new());
    let json = report.to_json();
    let meta = json.get("meta").unwrap();
    assert_eq!(meta.get("mode"), Some(&cping_core::Json::str("tcp")));
    assert_eq!(meta.get("tcp_port"), Some(&cping_core::Json::UInt(445)));
    let targets = match json.get("targets") {
        Some(cping_core::Json::Array(items)) => items.clone(),
        _ => unreachable!(),
    };
    assert_eq!(
        targets[0].get("status_label"),
        Some(&cping_core::Json::str("TCP-OPEN"))
    );
    assert_eq!(
        targets[1].get("status_label"),
        Some(&cping_core::Json::str("TCP-超时"))
    );
}
