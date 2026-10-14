//! 调度：并发上限、取消语义、超时/不可达/权限错误区分、重复任务防护。

use cping_core::classify::{classify_status, Thresholds};
use cping_core::config::ProbeConfig;
use cping_core::probe::{MockBackend, MockPolicy, MockResponse, RecordingBackend};
use cping_core::result::{ProbeEvent, ProbeStatus, ScanStats};
use cping_core::scheduler::{next_task_id, status_from_error, ProbeScheduler};
use cping_core::target::{build_targets, parse_prefix, TargetRange};
use cping_core::{CoreError, ProbeError};
use std::collections::HashSet;
use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// 测试用：取 IPv4 的第 4 段（测试数据都是 v4）。
fn last4(ip: IpAddr) -> u8 {
    match ip {
        IpAddr::V4(v4) => v4.octets()[3],
        IpAddr::V6(v6) => v6.octets()[15],
    }
}

fn cfg(concurrency: usize) -> ProbeConfig {
    ProbeConfig {
        concurrency,
        timeout_ms: 500,
        ..ProbeConfig::default()
    }
}

fn targets(count_prefix: &str, range: TargetRange) -> Vec<IpAddr> {
    let p = parse_prefix(count_prefix).unwrap();
    build_targets(&p, range)
        .unwrap()
        .into_iter()
        .map(IpAddr::V4)
        .collect()
}

#[test]
fn all_online_run_reports_correct_stats() {
    let backend = MockBackend::new(MockPolicy::AllOnline {
        base_rtt_ms: 1,
        span_ms: 10,
    });
    let sched = ProbeScheduler::new(backend);
    let ts = targets("192.168.1", TargetRange::Standard);
    let handle = sched.start(&cfg(8), ts).unwrap();

    let outcome = handle.run_to_completion();
    assert!(!outcome.cancelled);
    assert_eq!(outcome.stats.total, 255);
    assert_eq!(outcome.stats.completed, 255);
    assert_eq!(outcome.stats.online, 255);
    assert_eq!(outcome.stats.pending, 0);
    assert_eq!(outcome.results.len(), 255);
    assert!((outcome.stats.progress() - 1.0).abs() < f64::EPSILON);
}

#[test]
fn event_stream_starts_with_started_and_ends_with_finished() {
    let backend = MockBackend::new(MockPolicy::AllOnline {
        base_rtt_ms: 1,
        span_ms: 5,
    });
    let sched = ProbeScheduler::new(backend);
    let ts = targets("10.0.0", TargetRange::HostsOnly);
    let handle = sched.start(&cfg(4), ts).unwrap();
    let task_id = handle.task_id.clone();

    let mut seen_started = false;
    let mut seen_finished = false;
    let mut result_count = 0;
    let mut all_same_task = true;

    while let Some(ev) = handle.recv() {
        if ev.task_id() != task_id {
            all_same_task = false;
        }
        match ev {
            ProbeEvent::Started { .. } => seen_started = true,
            ProbeEvent::Result { .. } => result_count += 1,
            ProbeEvent::Progress { .. } => {}
            ProbeEvent::Finished { .. } => {
                seen_finished = true;
                break;
            }
            ProbeEvent::Cancelled { .. } => break,
        }
    }
    assert!(seen_started, "必须先收到 Started");
    assert!(seen_finished, "必须收到 Finished");
    assert_eq!(result_count, 254);
    assert!(all_same_task, "所有事件必须携带同一个 task_id");
}

#[test]
fn concurrency_never_exceeds_limit() {
    for (prefix, limit) in [("172.16.0", 1usize), ("172.16.1", 4), ("172.16.2", 16)] {
        let backend = Arc::new(RecordingBackend::new(MockPolicy::AllOnline {
            base_rtt_ms: 2,
            span_ms: 4,
        }));
        let sched = ProbeScheduler::new(SharedBackend(Arc::clone(&backend)));
        let handle = sched
            .start(&cfg(limit), targets(prefix, TargetRange::HostsOnly))
            .unwrap();
        let outcome = handle.run_to_completion();
        assert_eq!(outcome.stats.completed, 254);
        let peak = backend.peak_concurrency();
        assert!(peak <= limit, "峰值并发 {peak} 不得超过上限 {limit}");
        assert!(peak >= 1, "至少应观测到 1 个并发");
    }
}

/// ProbeBackend 需要 &self 且 Send+Sync，用 Arc 包装以便外部观测。
struct SharedBackend(std::sync::Arc<RecordingBackend>);

impl cping_core::probe::ProbeBackend for SharedBackend {
    fn name(&self) -> &'static str {
        self.0.name()
    }
    fn probe(
        &self,
        target: IpAddr,
        timeout: Duration,
    ) -> Result<cping_core::probe::ProbeOutcome, ProbeError> {
        self.0.probe(target, timeout)
    }
}

#[test]
fn cancel_stops_scheduling_new_targets_and_keeps_results() {
    let backend = std::sync::Arc::new(
        RecordingBackend::new(MockPolicy::AllOnline {
            base_rtt_ms: 1,
            span_ms: 4,
        })
        .with_delay_ms(20),
    );
    let sched = ProbeScheduler::new(SharedBackend(Arc::clone(&backend)));
    let ts = targets("10.10.0", TargetRange::HostsOnly); // 254 个
    let total = ts.len();

    let mut config = cfg(2);
    config.timeout_ms = 500;
    let handle = sched.start(&config, ts).unwrap();

    thread::sleep(Duration::from_millis(90));
    assert!(handle.cancel(), "首次 cancel 应返回 true");
    assert!(handle.is_cancelled());

    let outcome = handle.run_to_completion();
    assert!(outcome.cancelled, "终态必须是 Cancelled");
    assert_eq!(outcome.stats.total, total, "总数保持为目标总数");
    assert!(
        outcome.stats.completed < total,
        "取消后不应跑完全部目标，实际完成 {}",
        outcome.stats.completed
    );
    assert_eq!(
        outcome.results.len(),
        outcome.stats.completed,
        "已完成结果必须全部保留，且数量与统计一致"
    );
    assert_eq!(
        outcome.stats.pending,
        total - outcome.stats.completed,
        "未完成的目标应计为 pending"
    );

    let seen = backend.seen_targets().len();
    assert!(
        seen < total,
        "取消后不得继续调度新目标：已发起 {seen} / 总数 {total}"
    );
    // 允许最多比已完成数多出「在途」的若干个
    assert!(
        seen <= outcome.stats.completed + 2,
        "已发起数 {seen} 与完成数 {} 差距过大",
        outcome.stats.completed
    );
}

#[test]
fn second_start_is_rejected_while_running() {
    let backend = MockBackend::new(MockPolicy::AllOnline {
        base_rtt_ms: 1,
        span_ms: 3,
    });
    let sched = ProbeScheduler::new(backend);
    let ts = targets("10.20.0", TargetRange::HostsOnly);

    let mut slow = cfg(1);
    slow.timeout_ms = 500;
    let handle = sched.start(&slow, ts.clone()).unwrap();

    let err = sched.start(&slow, ts).unwrap_err();
    assert!(
        matches!(err, CoreError::InvalidConfig(_)),
        "重复启动必须被拒绝，实际 {err:?}"
    );
    assert!(sched.is_busy());

    handle.cancel();
    let _ = handle.run_to_completion();
    assert!(!sched.is_busy(), "任务结束后应恢复空闲");
}

#[test]
fn can_start_again_after_completion() {
    let backend = MockBackend::new(MockPolicy::AllOnline {
        base_rtt_ms: 1,
        span_ms: 3,
    });
    let sched = ProbeScheduler::new(backend);
    let ts = targets("10.30.0", TargetRange::HostsOnly);

    let a = sched.start(&cfg(4), ts.clone()).unwrap();
    let out_a = a.run_to_completion();
    let b = sched.start(&cfg(4), ts).unwrap();
    let out_b = b.run_to_completion();

    assert_ne!(out_a.task_id, out_b.task_id, "task_id 必须唯一");
    assert_eq!(out_a.stats.online, out_b.stats.online);
}

#[test]
fn distinct_task_ids() {
    let mut ids = HashSet::new();
    for _ in 0..200 {
        assert!(ids.insert(next_task_id()), "task_id 发生重复");
    }
}

#[test]
fn error_kinds_map_to_distinct_statuses() {
    assert_eq!(
        status_from_error(ProbeError::timeout(1000)),
        ProbeStatus::Timeout
    );
    assert_eq!(
        status_from_error(ProbeError::unreachable("no route")),
        ProbeStatus::Unreachable
    );
    assert_eq!(
        status_from_error(ProbeError::permission_denied("raw socket")),
        ProbeStatus::PermissionDenied
    );
    assert_eq!(
        status_from_error(ProbeError::unsupported("android")),
        ProbeStatus::Unsupported
    );
    assert_eq!(
        status_from_error(ProbeError::internal("boom")),
        ProbeStatus::Error("boom".into())
    );
}

#[test]
fn mixed_outcomes_are_counted_separately() {
    let prefix = parse_prefix("192.168.7").unwrap();
    let scripted = vec![
        (prefix.with_host(1), MockResponse::Online { rtt_ms: 10 }),
        (prefix.with_host(2), MockResponse::Online { rtt_ms: 80 }),
        (prefix.with_host(3), MockResponse::Online { rtt_ms: 300 }),
        (prefix.with_host(4), MockResponse::Timeout),
        (prefix.with_host(5), MockResponse::Unreachable),
        (prefix.with_host(6), MockResponse::PermissionDenied),
        (prefix.with_host(7), MockResponse::Unsupported),
        (prefix.with_host(8), MockResponse::Error("internal".into())),
    ];
    let sched = ProbeScheduler::new(MockBackend::new(MockPolicy::Scripted(
        scripted
            .into_iter()
            .map(|(ip, r)| (IpAddr::V4(ip), r))
            .collect(),
    )));
    let handle = sched
        .start(&cfg(4), targets("192.168.7", TargetRange::Standard))
        .unwrap();
    let outcome = handle.run_to_completion();

    assert_eq!(outcome.stats.online, 3, "3 个在线（10/80/300ms）");
    // 255 总数 - 3 在线 - 1 不可达 - 1 权限 - 1 不支持 - 1 错误 = 248 个超时
    assert_eq!(outcome.stats.timeout, 248, "其余均为超时");
    assert_eq!(outcome.stats.unreachable, 1);
    assert_eq!(outcome.stats.permission_denied, 1);
    assert_eq!(outcome.stats.unsupported, 1);
    assert_eq!(outcome.stats.error, 1);
    assert_eq!(outcome.stats.completed, 255);

    let t = Thresholds::default();
    let by_host = |h: u8| {
        outcome
            .results
            .iter()
            .find(|r| last4(r.ip) == h)
            .map(|r| classify_status(&r.status, &t))
            .unwrap()
    };
    use cping_core::classify::ResultClass;
    assert_eq!(by_host(1), Some(ResultClass::Low));
    assert_eq!(by_host(2), Some(ResultClass::Mid));
    assert_eq!(by_host(3), Some(ResultClass::High));
    assert_eq!(by_host(4), Some(ResultClass::Offline));
    assert_eq!(by_host(6), Some(ResultClass::Error));
}

#[test]
fn results_are_ordered_by_index() {
    let sched = ProbeScheduler::new(MockBackend::new(MockPolicy::AllOnline {
        base_rtt_ms: 1,
        span_ms: 3,
    }));
    let handle = sched
        .start(&cfg(16), targets("192.168.2", TargetRange::Standard))
        .unwrap();
    let outcome = handle.run_to_completion();
    let indices: Vec<u16> = outcome.results.iter().map(|r| r.index).collect();
    let mut sorted = indices.clone();
    sorted.sort_unstable();
    assert_eq!(indices, sorted, "结果必须按下标升序");
    assert_eq!(last4(outcome.results.first().unwrap().ip), 1);
    assert_eq!(last4(outcome.results.last().unwrap().ip), 255);
}

#[test]
fn timeout_policy_marks_everything_timeout() {
    let sched = ProbeScheduler::new(MockBackend::new(MockPolicy::AllTimeout));
    let handle = sched
        .start(&cfg(8), targets("10.40.0", TargetRange::HostsOnly))
        .unwrap();
    let outcome = handle.run_to_completion();
    assert_eq!(outcome.stats.online, 0);
    assert_eq!(outcome.stats.timeout, 254);
    assert!(outcome.results.iter().all(|r| !r.status.is_online()));
}

#[test]
fn slow_backend_respects_timeout_budget() {
    // 每次探测要 200ms，但超时只有 50ms → 必须被判定为超时，而不是「在线」
    let policy = MockPolicy::Scripted(vec![(
        IpAddr::V4(Ipv4Addr::new(10, 50, 0, 1)),
        MockResponse::Slow {
            sleep_ms: 200,
            rtt_ms: 5,
        },
    )]);
    let sched = ProbeScheduler::new(MockBackend::new(policy));
    let mut config = cfg(1);
    config.timeout_ms = 50;
    let handle = sched
        .start(&config, vec![IpAddr::V4(Ipv4Addr::new(10, 50, 0, 1))])
        .unwrap();
    let outcome = handle.run_to_completion();
    assert_eq!(outcome.stats.timeout, 1, "超出超时预算的探测不得记为在线");
    assert_eq!(outcome.stats.online, 0);
}

#[test]
fn stats_from_results_with_total_accounts_pending() {
    let results = vec![cping_core::ProbeResult::new(
        0,
        IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
        ProbeStatus::Online { rtt_ms: 5 },
    )];
    let stats = ScanStats::from_results_with_total(&results, 10);
    assert_eq!(stats.total, 10);
    assert_eq!(stats.completed, 1);
    assert_eq!(stats.pending, 9);
}

#[test]
fn empty_targets_rejected() {
    let sched = ProbeScheduler::new(MockBackend::new(MockPolicy::AllTimeout));
    let err = sched.start(&cfg(4), vec![]).unwrap_err();
    assert!(matches!(err, CoreError::InvalidConfig(_)));
    assert!(!sched.is_busy(), "被拒绝的启动不得占用调度器");
}

#[test]
fn invalid_config_rejected_before_start() {
    let sched = ProbeScheduler::new(MockBackend::new(MockPolicy::AllTimeout));
    let mut bad = cfg(4);
    bad.concurrency = 0;
    assert!(matches!(
        sched.start(&bad, targets("10.0.0", TargetRange::HostsOnly)),
        Err(CoreError::InvalidConfig(_))
    ));
    let mut bad2 = cfg(9999);
    bad2.concurrency = 9999;
    assert!(matches!(
        sched.start(&bad2, targets("10.0.0", TargetRange::HostsOnly)),
        Err(CoreError::InvalidConfig(_))
    ));
}
