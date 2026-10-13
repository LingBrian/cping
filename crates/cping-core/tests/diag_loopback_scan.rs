//! 诊断：直接调核心层扫描回环网段，检查是否有目标被误判为离线。
//! `cargo test -p cping-core --test diag_loopback_scan -- --ignored --nocapture`

use cping_core::classify::Thresholds;
use cping_core::config::ProbeConfig;
use cping_core::native::platform_icmp_backend;
use cping_core::scheduler::ProbeScheduler;
use cping_core::target::{build_targets, parse_prefix, TargetRange};

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
    };
    let sched = ProbeScheduler::new(platform_icmp_backend());
    let handle = sched.start(&cfg, targets).unwrap();
    let outcome = handle.run_to_completion();

    println!("后端 = {}", sched.backend_name());
    println!("总耗时 = {} ms", outcome.elapsed_ms);
    println!("统计 = {:?}", outcome.stats);

    let offline: Vec<String> = outcome
        .results
        .iter()
        .filter(|r| !r.status.is_online())
        .map(|r| format!("{}({})", r.ip.octets()[3], r.status.key()))
        .collect();
    println!("非在线目标数 = {} -> {:?}", offline.len(), offline);
}
