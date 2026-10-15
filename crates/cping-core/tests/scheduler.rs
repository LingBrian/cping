//! 调度：并发上限、取消语义、超时/不可达/权限错误区分、重复任务防护。

use cping_core::classify::{classify_status, Thresholds};
use cping_core::config::ProbeConfig;
use cping_core::probe::{MockBackend, MockPolicy, MockResponse, RecordingBackend};
use cping_core::result::{ProbeEvent, ProbeStatus, ScanStats};
use cping_core::scheduler::{next_task_id, status_from_error, ProbeScheduler};
use cping_core::target::{build_targets, parse_prefix, TargetRange, MAX_TARGETS};
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

/// M19 第三步：终态事件里的统计**不再**由「克隆整表再全量数一遍」得出，
/// 而是持锁用**增量口径**逐条累加——省掉 65536 目标下一次性 7.5 MiB 的整表副本。
///
/// 这条守的是那次改写的口径：把**调度器给出的终态统计**与「拿它给的结果按批量
/// 口径重算」对一遍，必须逐字段相同。否则「调度器说完成 500、导出的报表里 498」
/// 这种事会从核心层冒出来，而那时没人知道该信哪个。
#[test]
fn terminal_stats_match_a_batch_recompute_of_the_returned_results() {
    let sched = ProbeScheduler::new(MockBackend::new(MockPolicy::Realistic {
        online_percent: 30,
        base_rtt_ms: 1,
        span_ms: 100,
    }));
    let outcome = sched
        .start(&cfg(16), targets("192.168.1", TargetRange::Standard))
        .unwrap()
        .run_to_completion();

    let batch = ScanStats::from_results_with_total(&outcome.results, outcome.stats.total);
    assert_eq!(
        outcome.stats, batch,
        "终态统计必须与「按结果重算」的批量口径逐字段相同"
    );
    assert_eq!(
        outcome.stats.completed,
        outcome.results.len(),
        "completed 必须等于真实结果条数"
    );
    assert_eq!(
        outcome.stats.completed + outcome.stats.pending,
        outcome.stats.total,
        "已完成 + 未完成必须恒等于总数"
    );
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

/// M19：`Result` 事件的**完整性**必须断言，`Progress` 的**顺序与完整性都不能**。
///
/// 完整性：一次扫描里结果事件必须**一条不少**（`Result` 条数 == 目标数），
/// 且只有一条 `Started`、一条终态。这条在任何并发度下都必须成立。
///
/// `Progress` 则两条都不能信，原因都是「多个 worker 线程 + 一个协调线程
/// 共用一个 `Sender` 但并发入队」：
///
/// 1. **不保序**：同一个 worker 确实是先发 `Result` 再发 `Progress`，但跨线程
///    的入队顺序不保证，于是 `Progress` 会**领先于它自己的 `Result`** 到达。
/// 2. **尾部会丢**：通道按「块」缓冲，最后一个未填满的块要等填满（或发送端
///    析构）才可见；而终态是**协调线程**用另一个 `Sender` 发的，可能先被取走。
///    结果是收到的最后一条 `Progress` 可能明显小于目标数（实测 232/255）。
///
/// 实测口径（255 目标 / 32 并发）：结果事件 5/5 轮一条不少；`Progress` 超前
/// 0–42 次、最多超前 3 条，末值 232–255。
///
/// 所以这条测试**只断言完整性**，并把上面两条写进注释——它们是 M19 第二步的
/// 直接依据：既然 `Progress` 既不保序又不完整，就把「进度」的权威来源定为
/// `Result`（每条都到、且每次恰好加一），`Progress` 降级成粗粒度心跳。
#[test]
fn result_events_are_complete_but_progress_is_neither_ordered_nor_complete() {
    const TARGETS: usize = 255;
    for trial in 0..3 {
        let backend = MockBackend::new(MockPolicy::AllOnline {
            base_rtt_ms: 1,
            span_ms: 10,
        });
        let sched = ProbeScheduler::new(backend);
        let handle = sched
            .start(&cfg(32), targets("10.1.1", TargetRange::Standard))
            .unwrap();

        let (mut results, mut started, mut terminal) = (0usize, 0usize, 0usize);
        let mut last_progress = 0usize;
        let mut progress_seen = 0usize;

        loop {
            match handle.recv() {
                Some(ProbeEvent::Started { .. }) => started += 1,
                Some(ProbeEvent::Result { .. }) => results += 1,
                Some(ProbeEvent::Progress { completed, .. }) => {
                    progress_seen += 1;
                    last_progress = completed;
                }
                Some(ProbeEvent::Finished { .. }) | Some(ProbeEvent::Cancelled { .. }) => {
                    terminal += 1;
                    break;
                }
                None => break,
            }
        }

        assert_eq!(results, TARGETS, "第 {trial} 轮：结果事件必须一条不少");
        assert_eq!(started, 1, "第 {trial} 轮：Started 只应有一条");
        assert_eq!(terminal, 1, "第 {trial} 轮：终态只应有一条");
        // 不断言 `last_progress == TARGETS`（会随机红灯，见文档）；
        // 只断言它**永不超出事实**——这是 `completed` 的定义决定的。
        assert!(
            last_progress <= TARGETS,
            "第 {trial} 轮：进度值不得超出目标数（实测 {last_progress} / {TARGETS}）"
        );
        assert!(
            progress_seen <= TARGETS,
            "第 {trial} 轮：进度事件条数不得多于目标数"
        );
    }
}

// ---------------------------------------------------------------------------
// M19 第二步：事件通道背压与进度合并
// ---------------------------------------------------------------------------

/// 造 `n` 个目标地址（不经过网段解析——这些测试要的是**规模**，不是地址语义）。
fn many_targets(n: usize) -> Vec<IpAddr> {
    (0..n)
        .map(|i| {
            IpAddr::V4(Ipv4Addr::new(
                10,
                ((i / 65_536) % 256) as u8,
                ((i / 256) % 256) as u8,
                (i % 256) as u8,
            ))
        })
        .collect()
}

/// 把策略放宽到硬天花板。这些测试量的是**通道**行为，
/// 不能被 M17 的默认限速（500 pps）盖过去——65536 目标按默认限速要跑 131 秒。
fn unthrottled(sched: &mut ProbeScheduler) {
    sched
        .set_policy(cping_core::ScanPolicy {
            max_pps: cping_core::policy::MAX_PPS_LIMIT,
            ..cping_core::ScanPolicy::default()
        })
        .expect("策略校验失败");
}

/// M19 第二步：进度事件必须被**合并**，而不是每条结果发一条。
///
/// 之前是「每条结果一条进度」，基准实测合并比 **1.0×**——一个合并比为 1 的
/// 「合并」等于没合并，而且让通道流量翻倍。
#[test]
fn progress_events_are_merged_instead_of_one_per_result() {
    // 让扫描持续 300 ms 以上（255 目标 / 8 并发 × 10 ms 延迟 ≈ 320 ms），
    // 这样 200 ms 的节流间隔一定会被跨过。
    let backend = MockBackend::new(MockPolicy::AllOnline {
        base_rtt_ms: 1,
        span_ms: 5,
    })
    .with_delay_ms(10);
    let mut sched = ProbeScheduler::new(backend);
    unthrottled(&mut sched);

    let handle = sched.start(&cfg(8), many_targets(255)).expect("启动失败");

    let mut results = 0usize;
    let mut progress = 0usize;
    while let Some(ev) = handle.recv() {
        match ev {
            ProbeEvent::Result { .. } => results += 1,
            ProbeEvent::Progress { .. } => progress += 1,
            ProbeEvent::Finished { .. } | ProbeEvent::Cancelled { .. } => break,
            ProbeEvent::Started { .. } => {}
        }
    }
    let events = handle.event_stats();

    assert_eq!(results, 255, "结果事件一条不少");
    assert_eq!(
        events.progress_sent, progress,
        "句柄上的统计必须与实际收到的一致"
    );
    assert!(
        progress <= 8,
        "255 目标 / 约 320 ms 的扫描不该发出 {progress} 条进度事件（节流间隔 200 ms）"
    );
    assert!(
        events.progress_merged >= 100,
        "绝大多数进度事件应当被合并掉，实测只合并了 {} 条",
        events.progress_merged
    );
    let ratio = events.merge_ratio(results);
    assert!(ratio >= 20.0, "合并比应当显著大于 1，实测只有 {ratio:.1}×");
}

/// M19 第二步：慢消费者下**结果一条不能丢**，且内存有界（生产者被背压挡住）。
///
/// 结果数（65536）远大于通道容量，所以生产者一定会因为队列满而阻塞，
/// 直到消费者把队列抽走——这就是背压。丢结果比慢一点糟得多。
#[test]
fn a_slow_consumer_never_loses_results_and_stays_bounded() {
    let backend = MockBackend::new(MockPolicy::AllOnline {
        base_rtt_ms: 1,
        span_ms: 5,
    });
    let mut sched = ProbeScheduler::new(backend);
    unthrottled(&mut sched);

    let handle = sched
        .start(&cfg(32), many_targets(MAX_TARGETS))
        .expect("启动失败");

    let mut results = 0usize;
    let mut rounds = 0usize;
    let mut saw_terminal = false;
    for _ in 0..20_000 {
        // 故意慢：每轮先睡 1 ms，生产端只能等着。
        thread::sleep(Duration::from_millis(1));
        rounds += 1;
        let mut terminal = false;
        for ev in handle.drain() {
            match ev {
                ProbeEvent::Result { .. } => results += 1,
                ProbeEvent::Finished { .. } | ProbeEvent::Cancelled { .. } => terminal = true,
                _ => {}
            }
        }
        if terminal {
            saw_terminal = true;
            break;
        }
    }

    assert!(saw_terminal, "慢消费者下没有等到终态（可能卡住了）");
    assert_eq!(results, MAX_TARGETS, "慢消费者下结果仍然必须一条不少");
    assert!(
        rounds > 1,
        "这轮测试应当真的发生了多轮抽取（否则没测到背压）"
    );
}

/// **M19 第二步最容易出人命的路径**：有界通道满时 worker 阻塞在 `send` 上，
/// 此时若直接 join 协调线程就会**死锁**——没人在取事件，而 `Receiver`
/// 还活着不会让 `send` 返回 `Err`。所以 `Drop` 必须**先丢掉接收端**再 join。
///
/// 这里用带超时的看门狗：真死锁会报错，而不是把整个测试套件挂住。
#[test]
fn dropping_a_handle_mid_scan_does_not_deadlock() {
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    thread::spawn(move || {
        let backend = MockBackend::new(MockPolicy::AllOnline {
            base_rtt_ms: 1,
            span_ms: 5,
        });
        let mut sched = ProbeScheduler::new(backend);
        unthrottled(&mut sched);
        let handle = sched
            .start(&cfg(32), many_targets(MAX_TARGETS))
            .expect("启动失败");
        // **刻意不消费**：等队列填满、worker 全部阻塞在 `send` 上。
        thread::sleep(Duration::from_millis(100));
        drop(handle); // 必须能返回
        let _ = tx.send(());
    });

    assert!(
        rx.recv_timeout(Duration::from_secs(15)).is_ok(),
        "句柄在扫描中途被丢弃时死锁了（有界通道 + join 顺序错）"
    );
}

/// 有界通道确实是**有界**的：容量与节流间隔之间的关系在**编译期**就定死，
/// 而不是「应该不会太大」这种运行期期望。
#[test]
fn event_queue_capacity_is_a_real_bound() {
    assert_eq!(
        cping_core::EVENT_QUEUE_CAPACITY,
        4096,
        "改容量要同时改这条断言与文档里的内存估算"
    );

    // 编译期断言：队列上限 ≤ 1 MiB（相对 7.5 MiB 的结果表是小头），
    // 且节流间隔不至于短到让「合并」失去意义。
    const _: () = assert!(
        cping_core::EVENT_QUEUE_CAPACITY * std::mem::size_of::<ProbeEvent>() <= 1024 * 1024,
        "事件队列容量超过 1 MiB，超出了「相对结果表是小头」的前提"
    );
    const _: () = assert!(
        cping_core::PROGRESS_MIN_INTERVAL_MS >= 100,
        "节流间隔太短会让「合并」失去意义"
    );
}
