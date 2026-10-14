//! 诊断：直接调核心层扫描回环网段，检查是否有目标被误判为离线。
//! `cargo test -p cping-core --test diag_loopback_scan -- --ignored --nocapture`

/// 测试用：取 IPv4 的第 4 段（测试数据都是 v4）。
fn last4(ip: IpAddr) -> u8 {
    match ip {
        IpAddr::V4(v4) => v4.octets()[3],
        IpAddr::V6(v6) => v6.octets()[15],
    }
}

use cping_core::classify::Thresholds;
use cping_core::config::ProbeConfig;
use cping_core::native::platform_icmp_backend;
use cping_core::scheduler::ProbeScheduler;
use cping_core::target::{build_targets, parse_prefix, TargetRange};
use std::net::IpAddr;

#[test]
#[ignore = "真实扫描回环网段"]
fn diag_loopback_scan() {
    let prefix = parse_prefix("127.0.0").unwrap();
    let targets = build_targets(&prefix, TargetRange::Standard).unwrap();
    let cfg = ProbeConfig {
        prefix: Some(prefix),
        range: TargetRange::Standard,
        timeout_ms: 1000,
        concurrency: 32,
        thresholds: Thresholds::default(),
        ..ProbeConfig::default()
    };
    let sched = ProbeScheduler::new(platform_icmp_backend());
    let handle = sched
        .start(&cfg, targets.into_iter().map(IpAddr::V4).collect())
        .unwrap();
    let outcome = handle.run_to_completion();

    println!("后端 = {}", sched.backend_name());
    println!("总耗时 = {} ms", outcome.elapsed_ms);
    println!("统计 = {:?}", outcome.stats);

    let offline: Vec<String> = outcome
        .results
        .iter()
        .filter(|r| !r.status.is_online())
        .map(|r| format!("{}({})", last4(r.ip), r.status.key()))
        .collect();
    println!("非在线目标数 = {} -> {:?}", offline.len(), offline);
}
