//! 原生 ICMP 后端的**纯函数**部分（跨平台可测）。
//!
//! 真实收发需要权限与网络，属于 M5 的实机验证；这里保证报文构造与解析正确，
//! 避免"能编译但报文是错的"这种最容易踩的坑。

use cping_core::native::{
    build_echo_request, icmp_checksum, parse_echo_reply, parse_unreachable, verify_checksum,
    DEFAULT_PAYLOAD_LEN, ICMP_HEADER_LEN,
};

#[test]
fn header_len_matches_icmp_spec() {
    assert_eq!(ICMP_HEADER_LEN, 8);
    // 8 + 36 = 44，与原版实测的 sendto 载荷长度一致
    assert_eq!(ICMP_HEADER_LEN + DEFAULT_PAYLOAD_LEN, 44);
}

#[test]
fn echo_request_has_expected_layout() {
    let pkt = build_echo_request(0x1234, 7, DEFAULT_PAYLOAD_LEN);
    assert_eq!(pkt.len(), 44);
    assert_eq!(pkt[0], 8, "type 必须是 Echo Request(8)");
    assert_eq!(pkt[1], 0, "code 必须是 0");
    assert_eq!(u16::from_be_bytes([pkt[4], pkt[5]]), 0x1234, "id");
    assert_eq!(u16::from_be_bytes([pkt[6], pkt[7]]), 7, "seq");
}

#[test]
fn checksum_is_valid_after_build() {
    let pkt = build_echo_request(1, 1, 32);
    // 校验和字段自身也参与求和，正确时整体反码和为 0
    assert!(verify_checksum(&pkt), "构造出的报文校验和必须自洽");
    assert_ne!(u16::from_be_bytes([pkt[2], pkt[3]]), 0, "校验和不应为 0");
}

#[test]
fn checksum_algorithms_matches_known_vector() {
    // RFC 1071 示例数据
    let data: [u8; 8] = [0x00, 0x01, 0xf2, 0x03, 0xf4, 0xf5, 0xf6, 0xf7];
    assert_eq!(icmp_checksum(&data), 0x220d);
}

#[test]
fn checksum_handles_odd_length() {
    // 奇数字节不应 panic，且补零规则固定
    let a = icmp_checksum(&[0x01, 0x02, 0x03]);
    let b = icmp_checksum(&[0x01, 0x02, 0x03, 0x00]);
    assert_eq!(a, b, "奇数字节等价于末尾补 0");
}

#[test]
fn parse_reply_without_ip_header() {
    // SOCK_DGRAM 场景：缓冲区直接是 ICMP 报文
    let mut pkt = build_echo_request(0xABCD, 9, 16);
    pkt[0] = 0; // 改成 Echo Reply
    let ck = icmp_checksum(&pkt);
    pkt[2..4].copy_from_slice(&ck.to_be_bytes());
    assert_eq!(parse_echo_reply(&pkt), Some((0xABCD, 9)));
}

#[test]
fn parse_reply_with_ip_header() {
    // SOCK_RAW 场景：前面有 20 字节 IP 头
    let mut pkt = build_echo_request(0x00FF, 42, 8);
    pkt[0] = 0;
    let ck = icmp_checksum(&pkt);
    pkt[2..4].copy_from_slice(&ck.to_be_bytes());

    let mut framed = vec![0u8; 20];
    framed[0] = 0x45; // IPv4, IHL=5 => 20 字节
    framed.extend_from_slice(&pkt);
    assert_eq!(parse_echo_reply(&framed), Some((0x00FF, 42)));
}

#[test]
fn parse_ignores_request_and_other_types() {
    let req = build_echo_request(1, 1, 8);
    assert_eq!(parse_echo_reply(&req), None, "Echo Request 不是回复");
    assert_eq!(parse_echo_reply(&[]), None);
    assert_eq!(parse_echo_reply(&[0u8; 4]), None, "过短缓冲区不能 panic");
}

#[test]
fn parse_unreachable_detects_type3() {
    let mut icmp = vec![3u8, 1, 0, 0, 0, 0, 0, 0]; // type=3 code=1 不可达
    let ck = icmp_checksum(&icmp);
    icmp[2..4].copy_from_slice(&ck.to_be_bytes());
    assert_eq!(parse_unreachable(&icmp), Some(1));

    let ok = build_echo_request(1, 1, 8);
    assert_eq!(parse_unreachable(&ok), None, "正常请求不得被判为不可达");
}

#[test]
fn id_and_seq_roundtrip_over_boundaries() {
    for (id, seq) in [(0u16, 0u16), (1, 1), (0x7FFF, 0xFFFF), (0xFFFF, 0x0001)] {
        let mut pkt = build_echo_request(id, seq, 4);
        pkt[0] = 0;
        let ck = icmp_checksum(&pkt);
        pkt[2..4].copy_from_slice(&ck.to_be_bytes());
        assert_eq!(parse_echo_reply(&pkt), Some((id, seq)));
    }
}

#[test]
fn platform_backend_reports_a_stable_name() {
    let backend = cping_core::native::platform_icmp_backend();
    let name = backend.name();
    assert!(
        ["windows-icmp", "linux-icmp", "android-icmp", "unsupported"].contains(&name),
        "后端名必须是稳定标识，实际 {name}"
    );
    // 提示文案必须说明权限要求，不能含糊
    let hint = cping_core::native::native_icmp_hint();
    assert!(!hint.is_empty());
}

#[test]
fn tcp_backend_is_clearly_named_differently() {
    use cping_core::probe::ProbeBackend;
    let tcp = cping_core::native::TcpConnectBackend::new(80);
    assert_eq!(tcp.name(), "tcp-connect");
    assert_ne!(tcp.name(), "windows-icmp", "TCP 后端绝不能冒用 ICMP 的名字");
}
