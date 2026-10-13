//! 真实 ICMP 探测（M5 实机验证）。
//!
//! 默认 `#[ignore]`，因为它依赖网络权限与真实收发包；手动运行：
//!
//! ```text
//! cargo test -p cping-core --test native_icmp_real -- --ignored --nocapture
//! ```
//!
//! **只探测回环与显式指定的地址**。默认只碰 `127.0.0.1`，
//! 不会主动扫描任何外部网段。

use cping_core::error::ProbeError;
use cping_core::native::{platform_icmp_backend, TcpConnectBackend};
use cping_core::probe::ProbeBackend;
use std::net::Ipv4Addr;
use std::time::Duration;

/// 回环探测：验证「成功路径」是否真的能收到 Echo Reply。
#[test]
#[ignore = "真实网络探测；手动运行 cargo test -- --ignored --nocapture"]
fn real_icmp_loopback_succeeds() {
    let backend = platform_icmp_backend();
    println!("后端 = {}", backend.name());
    println!("平台提示 = {}", cping_core::native::native_icmp_hint());

    match backend.probe(Ipv4Addr::LOCALHOST, Duration::from_millis(1000)) {
        Ok(o) => {
            println!("127.0.0.1 => 在线, rtt = {} ms", o.rtt_ms);
            assert!(o.rtt_ms <= 1000, "回环 RTT 不应超过超时上限");
        }
        Err(ProbeError::PermissionDenied { detail }) => {
            panic!(
                "回环探测被拒绝权限：{detail}\n\
                 这属于必须如实记录的平台限制，而不是「探测成功」。"
            );
        }
        Err(e) => panic!("回环探测意外失败：{e}（kind={}）", e.kind()),
    }
}

/// 不存在的地址应当**明确返回超时**，而不是被当成在线。
#[test]
#[ignore = "真实网络探测；手动运行 cargo test -- --ignored --nocapture"]
fn real_icmp_absent_target_times_out_not_online() {
    let backend = platform_icmp_backend();
    // 保留地址段，网络上不会有人应答
    let target = Ipv4Addr::new(192, 0, 2, 123);
    let started = std::time::Instant::now();
    let res = backend.probe(target, Duration::from_millis(800));
    let took = started.elapsed();

    println!("{target} => {res:?}，耗时 {} ms", took.as_millis());
    match res {
        Ok(o) => panic!("不存在的地址不应被判为在线：rtt={}", o.rtt_ms),
        Err(ProbeError::Timeout { .. }) => {
            assert!(took < Duration::from_millis(3000), "超时必须及时返回");
        }
        Err(e) => println!("（可接受的其它终态：{} / {}）", e.kind(), e),
    }
}

/// TCP 模式必须与 ICMP 语义分离：名字不同，且端口关闭不算 ICMP 在线。
#[test]
#[ignore = "真实网络探测；手动运行 cargo test -- --ignored --nocapture"]
fn tcp_backend_never_impersonates_icmp() {
    let tcp = TcpConnectBackend::new(9); // discard 端口，通常关闭
    assert_eq!(tcp.name(), "tcp-connect");
    let res = tcp.probe(Ipv4Addr::LOCALHOST, Duration::from_millis(500));
    println!("tcp-connect 127.0.0.1:9 => {res:?}");
    assert_ne!(
        tcp.name(),
        cping_core::native::platform_icmp_backend().name(),
        "TCP 后端不得与 ICMP 后端同名"
    );
}
