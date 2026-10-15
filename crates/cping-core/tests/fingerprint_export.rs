//! M13 端口指纹的导出契约：与在线判定**分列**、空值不留占位、JSON 自述是推断。

use cping_core::classify::Thresholds;
use cping_core::export::{export_csv_with_fingerprints, export_txt_with_fingerprints, ExportMeta};
use cping_core::fingerprint::{
    infer_device, DeviceGuess, HostFingerprint, PortFinding, PortScanConfig, PortScanReport,
};
use cping_core::result::{ProbeResult, ProbeStatus};
use cping_core::{json_is_balanced, ProbeMode};
use std::net::{IpAddr, Ipv4Addr};

fn meta() -> ExportMeta {
    ExportMeta {
        scan_time: "2026-10-15 10:00:00".into(),
        local_ip: "192.168.1.100".into(),
        elapsed_ms: 900,
        mode: ProbeMode::Icmp,
        target_desc: "192.168.1.0/24".into(),
        round: 1,
    }
}

fn results() -> Vec<ProbeResult> {
    vec![
        ProbeResult::new(
            0,
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
            ProbeStatus::Online { rtt_ms: 4 },
        ),
        ProbeResult::new(
            1,
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2)),
            ProbeStatus::Online { rtt_ms: 7 },
        ),
    ]
}

fn fingerprints() -> Vec<HostFingerprint> {
    vec![
        HostFingerprint {
            report: PortScanReport {
                ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
                findings: vec![
                    PortFinding {
                        port: 22,
                        service: Some("ssh"),
                        banner: Some("SSH-2.0-OpenSSH_9.2".into()),
                        connect_rtt_ms: 2,
                    },
                    PortFinding {
                        port: 80,
                        service: Some("http"),
                        banner: None,
                        connect_rtt_ms: 3,
                    },
                ],
                scanned: 28,
                elapsed_ms: 420,
            },
            device: infer_device(Some("TP-Link Technologies Co.,Ltd."), &[22, 80]),
        },
        HostFingerprint {
            // 端口全关：**不得**影响它在 results 里的在线状态
            report: PortScanReport {
                ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2)),
                findings: Vec::new(),
                scanned: 28,
                elapsed_ms: 380,
            },
            device: None,
        },
    ]
}

#[test]
fn csv_appends_fingerprint_columns_without_touching_online_columns() {
    let bytes = export_csv_with_fingerprints(&results(), &meta(), &fingerprints()).unwrap();
    let text = String::from_utf8(bytes[3..].to_vec()).unwrap();
    let mut lines = text.split("\r\n");
    let header: Vec<&str> = lines.next().unwrap().split(',').collect();
    assert_eq!(header[9], "开放端口");
    assert_eq!(header[10], "设备推断");

    let row1: Vec<&str> = lines.next().unwrap().split(',').collect();
    // 前 9 列与不带指纹时**逐字节一致**（在线判定不受指纹影响）
    let plain = export_csv_with_fingerprints(&results(), &meta(), &[]).unwrap();
    let plain_text = String::from_utf8(plain[3..].to_vec()).unwrap();
    let plain_row1: Vec<&str> = plain_text
        .split("\r\n")
        .nth(1)
        .unwrap()
        .split(',')
        .collect();
    assert_eq!(
        row1[..9],
        plain_row1[..9],
        "指纹只能追加列，绝不能改动既有列"
    );
    assert_eq!(row1[1], "192.168.1.1");
    assert_eq!(row1[2], "在线");
    assert_eq!(row1[9], "22(ssh) 80(http)");
    assert!(row1[10].contains("推断"), "设备列必须写明推断");
    assert!(row1[10].contains("网络设备"));

    // 第二台：在线但端口全关 → 开放端口列空、设备列空，状态仍是「在线」
    let row2: Vec<&str> = lines.next().unwrap().split(',').collect();
    assert_eq!(row2[2], "在线", "端口全关不得把主机变成离线");
    assert_eq!(row2[9], "");
    assert_eq!(row2[10], "");
}

#[test]
fn txt_has_a_separate_fingerprint_section_that_says_so() {
    let bytes =
        export_txt_with_fingerprints(&results(), &meta(), &Thresholds::default(), &fingerprints())
            .unwrap();
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("端口指纹"), "应有独立小节");
    assert!(
        text.contains("与上面的在线判定相互独立"),
        "小节标题必须写明与在线判定无关"
    );
    assert!(text.contains("SSH-2.0-OpenSSH_9.2"), "横幅原样出现在 TXT");
    assert!(text.contains(":22"), "横幅行应能对应到端口");
    // 「无开放端口」必须明确说明不影响在线状态
    assert!(text.contains("无开放端口（不影响其在线状态）"));
    // 明细表里的在线判定仍然只由 results 决定
    assert!(text.contains("在线主机 : 2"));
}

#[test]
fn txt_without_fingerprints_has_no_section() {
    let bytes =
        export_txt_with_fingerprints(&results(), &meta(), &Thresholds::default(), &[]).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    assert!(!text.contains("端口指纹"), "未做指纹时不得凭空出现小节");
}

#[test]
fn json_fingerprints_are_separate_and_self_describe_as_inference() {
    let report =
        cping_core::export::ScanReport::new(&results(), meta(), Thresholds::default(), Vec::new())
            .with_fingerprints(fingerprints());
    let text = report.to_json_text();
    assert!(json_is_balanced(&text), "JSON 必须自平衡");
    assert!(text.contains("\"fingerprints\""), "指纹独立成数组");
    assert!(text.contains("\"open_ports\""));
    assert!(
        text.contains("\"is_inference\": true"),
        "推断必须机器可读地标注"
    );
    assert!(text.contains("\"banner\": \"SSH-2.0-OpenSSH_9.2\""));
    // 未识别设备写 null，而不是空对象或占位字符串
    assert!(text.contains("\"device\": null"));
    // 在线判定仍来自 targets，不受指纹影响
    assert!(text.contains("\"online\": true"));
}

#[test]
fn html_fingerprint_section_is_self_contained_and_labeled() {
    let report =
        cping_core::export::ScanReport::new(&results(), meta(), Thresholds::default(), Vec::new())
            .with_fingerprints(fingerprints());
    let html = report
        .render(
            cping_core::ExportFormat::Html,
            &cping_core::config::ColorConfig::default(),
        )
        .unwrap();
    let html = String::from_utf8(html).unwrap();
    assert!(html.contains("端口指纹"));
    assert!(html.contains("与在线判定相互独立"), "HTML 也要写明这条边界");
    assert!(html.contains("SSH-2.0-OpenSSH_9.2"));
    assert!(html.contains("无开放端口（不影响在线状态）"));
    assert!(html.contains("推断"));
    assert!(!html.contains("<script"), "报告必须自包含、无脚本");
}

#[test]
fn empty_port_list_config_never_scans_anything() {
    // 构造一个「没有端口可扫」的配置：不应 panic，也不应产生条目
    let cfg = PortScanConfig {
        ports: vec![],
        ..Default::default()
    }
    .normalized();
    assert!(cfg.ports.is_empty());
    let report = cping_core::fingerprint::scan_ports(IpAddr::V4(Ipv4Addr::LOCALHOST), &cfg);
    assert!(report.findings.is_empty());
    assert_eq!(report.scanned, 0);
}

#[test]
fn device_guess_summary_is_stable_and_marks_inference() {
    let g: DeviceGuess = infer_device(Some("TP-Link"), &[80]).unwrap();
    let s = g.summary();
    assert!(
        s.starts_with("网络设备（路由器/交换机）（推断，置信中；依据："),
        "{s}"
    );
    assert!(s.ends_with('）'), "{s}");
}
