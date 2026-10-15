//! 可选异步调度（M12，`feature = "async"`）。
//!
//! ## 为什么要有第二个调度器
//!
//! 同步版（[`crate::scheduler::ProbeScheduler`]）用「一个并发位一个 std 线程」，
//! 在 32～256 并发下完全够用，而且是三端（含 Android）都能编的最小方案。
//! 但 M9 把目标上限提到 65536 之后，有人会把 `concurrency` 也调到几千——
//! 那是几千个 OS 线程，内存与调度开销都不合适。
//!
//! 本模块在**语义完全一致**的前提下换成 Tokio：并发度用一个信号量封顶，
//! 每个探测走 `spawn_blocking`（探测本身仍是阻塞系统调用，只是不再各占一个
//! 长期线程）。
//!
//! ## 语义契约（与同步版逐条对齐）
//!
//! | 契约 | 同步版 | 本模块 |
//! | --- | --- | --- |
//! | 事件序列 | `Started → Result* / Progress* → Finished｜Cancelled` | 同 |
//! | 重复任务 | 拒绝并返回 `InvalidConfig` | 同 |
//! | 取消 | 不再取新目标，已发出的探测自然结束，结果全保留 | 同 |
//! | 结果顺序 | 按目标下标升序 | 同 |
//! | 统计口径 | [`ScanStats::from_results_with_total`] | 同 |
//!
//! ## 使用
//!
//! ```no_run
//! # #[cfg(feature = "async")] {
//! use cping_core::asynchronous::AsyncScheduler;
//! use cping_core::{MockBackend, MockPolicy, ProbeConfig};
//!
//! let sched = AsyncScheduler::new(MockBackend::new(MockPolicy::AllOnline {
//!     base_rtt_ms: 5,
//!     span_ms: 10,
//! }));
//! let cfg = ProbeConfig::default();
//! let targets = vec!["192.168.1.1".parse().unwrap()];
//! let handle = sched.start(&cfg, targets).expect("启动失败");
//! let outcome = handle.run_to_completion();
//! println!("{} 在线", outcome.stats.online);
//! # }
//! ```

use crate::config::ProbeConfig;
use crate::error::{CoreError, CoreResult, ProbeError};
use crate::probe::ProbeBackend;
use crate::result::{ProbeEvent, ProbeResult, ProbeStatus, ScanStats, TaskId};
use crate::scheduler::RunOutcome;
use crate::scheduler::{
    next_task_id, status_from_error, CancelToken, EventStats, EVENT_QUEUE_CAPACITY,
    PROGRESS_MIN_INTERVAL_MS,
};
use crate::target::MAX_TARGETS;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

/// 异步调度器。
///
/// 与同步版一样可长期持有并反复 `start`；内部按需创建 Tokio 运行时，
/// 因此**调用方不需要自己建 runtime**，也不需要是 async 上下文。
pub struct AsyncScheduler {
    backend: Arc<dyn ProbeBackend>,
    active: Arc<AtomicBool>,
}

impl AsyncScheduler {
    pub fn new<B: ProbeBackend + 'static>(backend: B) -> Self {
        Self {
            backend: Arc::new(backend),
            active: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn backend_name(&self) -> &'static str {
        self.backend.name()
    }

    pub fn is_busy(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    /// 启动一次探测。签名与 [`crate::scheduler::ProbeScheduler::start`] 一致。
    pub fn start(&self, config: &ProbeConfig, targets: Vec<IpAddr>) -> CoreResult<AsyncHandle> {
        config.validate()?;
        if targets.is_empty() {
            return Err(CoreError::InvalidConfig("目标列表为空".into()));
        }
        if targets.len() > MAX_TARGETS {
            return Err(CoreError::TooManyTargets {
                actual: targets.len(),
                limit: MAX_TARGETS,
            });
        }
        if self
            .active
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(CoreError::InvalidConfig(
                "已有探测任务在运行，请先停止当前任务".into(),
            ));
        }

        let task_id = next_task_id();
        let total = targets.len();
        let concurrency = config.concurrency.min(total).max(1);
        let timeout = Duration::from_millis(config.timeout_ms);
        let (tx, rx) = mpsc::sync_channel::<ProbeEvent>(EVENT_QUEUE_CAPACITY);

        let shared = Arc::new(AsyncShared {
            results: Mutex::new(vec![None; total]),
            cancel: Arc::new(AtomicBool::new(false)),
            completed: AtomicUsize::new(0),
            active: Arc::clone(&self.active),
            tx: tx.clone(),
            task_id: task_id.clone(),
            total,
            started: Instant::now(),
            progress_last_ms: AtomicU64::new(0),
            progress_sent: AtomicUsize::new(0),
            progress_merged: AtomicUsize::new(0),
            progress_dropped: AtomicUsize::new(0),
        });

        let _ = tx.send(ProbeEvent::Started {
            task_id: task_id.clone(),
            total,
        });

        let backend = Arc::clone(&self.backend);
        let coord = Arc::clone(&shared);
        let start = Instant::now();
        let coordinator = thread::Builder::new()
            .name(format!("cping-async-{task_id}"))
            .spawn(move || {
                // 单独的运行时：任务结束后随之销毁，不在进程里留全局 runtime。
                let rt = match tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        // 运行时都建不起来属于内部错误：如实上报，不伪造结果。
                        coord.active.store(false, Ordering::SeqCst);
                        let msg = format!("异步运行时创建失败：{e}");
                        for (i, ip) in targets.into_iter().enumerate() {
                            let mut r =
                                ProbeResult::new(i as u16, ip, ProbeStatus::Error(msg.clone()));
                            r.mac = None;
                            let _ = coord.tx.send(ProbeEvent::Result {
                                task_id: coord.task_id.clone(),
                                result: r,
                            });
                        }
                        let stats = ScanStats::from_results_with_total(&[], coord.total);
                        let _ = coord.tx.send(ProbeEvent::Finished {
                            task_id: coord.task_id.clone(),
                            stats,
                            elapsed_ms: start.elapsed().as_millis() as u64,
                        });
                        return;
                    }
                };

                rt.block_on(run_all(
                    backend,
                    coord.clone(),
                    targets,
                    concurrency,
                    timeout,
                ));

                let elapsed_ms = start.elapsed().as_millis() as u64;
                // M19 第三步：与同步版同一处修正——不再为了让 `from_statuses`
                // 数一遍就把整表克隆出来（那会与结果表同时存活、把峰值翻倍）。
                // 改成持锁用增量口径逐条累加，零分配，口径逐字段相同。
                let stats = {
                    let slots = coord.results.lock().expect("results poisoned");
                    let mut stats = ScanStats::for_total(coord.total);
                    for slot in slots.iter().flatten() {
                        stats.push_status(&slot.status);
                    }
                    stats
                };
                let cancelled = coord.cancel.load(Ordering::SeqCst);
                coord.active.store(false, Ordering::SeqCst);

                let event = if cancelled {
                    ProbeEvent::Cancelled {
                        task_id: coord.task_id.clone(),
                        stats,
                        elapsed_ms,
                    }
                } else {
                    ProbeEvent::Finished {
                        task_id: coord.task_id.clone(),
                        stats,
                        elapsed_ms,
                    }
                };
                let _ = coord.tx.send(event);
            });

        // 线程都建不起来属于「资源/配置」问题，如实报错，并释放 active 标志，
        // 否则调度器会永远卡在「忙」状态。
        let coordinator = match coordinator {
            Ok(h) => h,
            Err(e) => {
                self.active.store(false, Ordering::SeqCst);
                return Err(CoreError::InvalidConfig(format!(
                    "无法创建异步调度线程：{e}"
                )));
            }
        };

        Ok(AsyncHandle {
            task_id,
            total,
            shared,
            events: Some(rx),
            coordinator: Some(coordinator),
        })
    }
}

struct AsyncShared {
    results: Mutex<Vec<Option<ProbeResult>>>,
    cancel: Arc<AtomicBool>,
    completed: AtomicUsize,
    active: Arc<AtomicBool>,
    /// M19 第二步：与同步版一致，改成**有界**通道（`EVENT_QUEUE_CAPACITY`）。
    /// 两套实现必须保持同一套语义，否则「可在两者之间切换」这句话就不成立了。
    tx: SyncSender<ProbeEvent>,
    task_id: TaskId,
    total: usize,
    /// M19：进度节流（字段与语义同同步版的 `Shared`）。
    started: Instant,
    progress_last_ms: AtomicU64,
    progress_sent: AtomicUsize,
    progress_merged: AtomicUsize,
    progress_dropped: AtomicUsize,
}

impl AsyncShared {
    /// M19：这一次要不要发进度事件？语义与同步版 [`crate::scheduler::Shared`] 相同。
    fn claim_progress_slot(&self) -> bool {
        let now = self.started.elapsed().as_millis() as u64;
        let last = self.progress_last_ms.load(Ordering::Relaxed);
        if now.saturating_sub(last) < PROGRESS_MIN_INTERVAL_MS {
            self.progress_merged.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        if self
            .progress_last_ms
            .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
        {
            self.progress_merged.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        true
    }

    fn event_stats(&self) -> EventStats {
        EventStats {
            progress_sent: self.progress_sent.load(Ordering::Relaxed),
            progress_merged: self.progress_merged.load(Ordering::Relaxed),
            progress_dropped: self.progress_dropped.load(Ordering::Relaxed),
        }
    }
}

/// 并发度由信号量封顶；每个探测走 `spawn_blocking`。
///
/// 取消的检查点与同步版一致：**派发前**检查，取消后不再发起新的探测。
async fn run_all(
    backend: Arc<dyn ProbeBackend>,
    shared: Arc<AsyncShared>,
    targets: Vec<IpAddr>,
    concurrency: usize,
    timeout: Duration,
) {
    let sem = Arc::new(Semaphore::new(concurrency));
    let mut tasks = Vec::with_capacity(targets.len());

    for (index, ip) in targets.into_iter().enumerate() {
        if shared.cancel.load(Ordering::SeqCst) {
            break;
        }
        let permit = match Arc::clone(&sem).acquire_owned().await {
            Ok(p) => p,
            Err(_) => break,
        };

        let backend = Arc::clone(&backend);
        let shared = Arc::clone(&shared);
        tasks.push(tokio::task::spawn_blocking(move || {
            // 拿到并发位后、真正发起探测前再确认一次取消
            if shared.cancel.load(Ordering::SeqCst) {
                drop(permit);
                return;
            }

            let (status, mac) = match backend.probe(ip, timeout) {
                Ok(outcome) => (
                    ProbeStatus::Online {
                        rtt_ms: outcome.rtt_ms,
                    },
                    outcome.mac,
                ),
                Err(err) => (status_from_error(err), None),
            };

            let mut result = ProbeResult::new(index as u16, ip, status);
            result.mac = mac;

            {
                let mut slots = shared.results.lock().expect("results poisoned");
                if let Some(slot) = slots.get_mut(index) {
                    *slot = Some(result.clone());
                }
            }

            let done = shared.completed.fetch_add(1, Ordering::SeqCst) + 1;

            // 结果永不丢：队列满时阻塞在发送侧（真背压）。这段跑在
            // `spawn_blocking` 的阻塞线程上，所以阻塞发送不会卡住 async 事件循环。
            if shared
                .tx
                .send(ProbeEvent::Result {
                    task_id: shared.task_id.clone(),
                    result,
                })
                .is_err()
            {
                drop(permit);
                return;
            }

            if shared.claim_progress_slot() {
                match shared.tx.try_send(ProbeEvent::Progress {
                    task_id: shared.task_id.clone(),
                    completed: done,
                    total: shared.total,
                }) {
                    Ok(()) => {
                        shared.progress_sent.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(TrySendError::Full(_)) => {
                        shared.progress_dropped.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(TrySendError::Disconnected(_)) => {
                        drop(permit);
                        return;
                    }
                }
            }
            drop(permit);
        }));
    }

    for t in tasks {
        let _ = t.await;
    }
}

/// 异步任务句柄。**接口与 [`crate::scheduler::SchedulerHandle`] 对齐**，
/// 便于调用方在两套实现之间切换。
pub struct AsyncHandle {
    pub task_id: TaskId,
    pub total: usize,
    shared: Arc<AsyncShared>,
    /// 用 `Option` 包一层，好在 [`Drop`] 里**先丢掉接收端**再 join（同同步版）。
    events: Option<Receiver<ProbeEvent>>,
    coordinator: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for AsyncHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AsyncHandle")
            .field("task_id", &self.task_id)
            .field("total", &self.total)
            .field("cancelled", &self.is_cancelled())
            .field("completed", &self.completed())
            .finish()
    }
}

impl AsyncHandle {
    pub fn cancel_token(&self) -> CancelToken {
        CancelToken::from_flag(Arc::clone(&self.shared.cancel))
    }

    pub fn cancel(&self) -> bool {
        !self.shared.cancel.swap(true, Ordering::SeqCst)
    }

    /// 事件接收端。不变式同同步版：只有 [`Drop`] 会把它取走。
    fn events(&self) -> &Receiver<ProbeEvent> {
        self.events
            .as_ref()
            .expect("事件接收端只在句柄析构时被取走，此处不应为 None")
    }

    /// M19：事件通道的观测数据（发出 / 合并 / 丢弃的进度事件数）。
    pub fn event_stats(&self) -> EventStats {
        self.shared.event_stats()
    }

    pub fn is_cancelled(&self) -> bool {
        self.shared.cancel.load(Ordering::SeqCst)
    }

    /// 已完成的探测数。
    pub fn completed(&self) -> usize {
        self.shared.completed.load(Ordering::SeqCst)
    }

    pub fn recv(&self) -> Option<ProbeEvent> {
        self.events().recv().ok()
    }

    pub fn recv_timeout(&self, timeout: Duration) -> Result<ProbeEvent, RecvTimeoutError> {
        self.events().recv_timeout(timeout)
    }

    pub fn try_recv(&self) -> Result<ProbeEvent, TryRecvError> {
        self.events().try_recv()
    }

    pub fn drain(&self) -> Vec<ProbeEvent> {
        let mut out = Vec::new();
        while let Ok(ev) = self.events().try_recv() {
            out.push(ev);
        }
        out
    }

    /// 当前已完成结果快照（按下标升序）。
    pub fn snapshot(&self) -> Vec<ProbeResult> {
        let slots = self.shared.results.lock().expect("results poisoned");
        slots.iter().flatten().cloned().collect()
    }

    /// 消费事件直到终态。
    pub fn run_to_completion(mut self) -> RunOutcome {
        let mut cancelled = false;
        let mut stats = ScanStats::from_results_with_total(&[], self.total);
        let mut elapsed_ms = 0;
        while let Ok(event) = self.events().recv() {
            match event {
                ProbeEvent::Finished {
                    stats: s,
                    elapsed_ms: e,
                    ..
                } => {
                    stats = s;
                    elapsed_ms = e;
                    break;
                }
                ProbeEvent::Cancelled {
                    stats: s,
                    elapsed_ms: e,
                    ..
                } => {
                    stats = s;
                    elapsed_ms = e;
                    cancelled = true;
                    break;
                }
                _ => {}
            }
        }
        let results = self.snapshot();
        let events = self.event_stats();
        if let Some(handle) = self.coordinator.take() {
            let _ = handle.join();
        }
        RunOutcome {
            task_id: self.task_id.clone(),
            cancelled,
            stats,
            elapsed_ms,
            results,
            events,
        }
    }

    pub fn join(&mut self) {
        if let Some(handle) = self.coordinator.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for AsyncHandle {
    fn drop(&mut self) {
        self.shared.cancel.store(true, Ordering::SeqCst);
        // 与同步版同理且**顺序同样关键**：先丢掉接收端，让可能正阻塞在
        // 有界通道 `send` 上的 worker 立刻拿到 `Disconnected`，否则 join 会死锁。
        drop(self.events.take());
        if let Some(handle) = self.coordinator.take() {
            let _ = handle.join();
        }
    }
}

/// 便于诊断的错误转换（与同步版共用 [`status_from_error`]）。
pub fn status_of(error: ProbeError) -> ProbeStatus {
    status_from_error(error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::{MockBackend, MockPolicy};

    /// 只关心超时与并发度，其余取默认值。
    fn cfg(concurrency: usize) -> ProbeConfig {
        ProbeConfig {
            timeout_ms: 1000,
            concurrency,
            ..ProbeConfig::default()
        }
    }

    fn v4(host: u8) -> IpAddr {
        IpAddr::V4(std::net::Ipv4Addr::new(10, 0, 0, host))
    }

    #[test]
    fn async_run_matches_sync_semantics() {
        let sched = AsyncScheduler::new(MockBackend::new(MockPolicy::AllOnline {
            base_rtt_ms: 5,
            span_ms: 10,
        }));
        let targets: Vec<IpAddr> = (1..=20).map(v4).collect();
        let handle = sched.start(&cfg(4), targets).expect("启动失败");
        let outcome = handle.run_to_completion();

        assert!(!outcome.cancelled);
        assert_eq!(outcome.stats.total, 20);
        assert_eq!(outcome.stats.online, 20);
        assert_eq!(outcome.results.len(), 20);
        // 结果按目标下标升序
        let indexes: Vec<u16> = outcome.results.iter().map(|r| r.index).collect();
        let mut sorted = indexes.clone();
        sorted.sort_unstable();
        assert_eq!(indexes, sorted);
        assert!(!sched.is_busy(), "任务结束后调度器必须回到空闲");
    }

    #[test]
    fn concurrency_is_capped_by_the_semaphore() {
        let backend = Arc::new(
            MockBackend::new(MockPolicy::AllOnline {
                base_rtt_ms: 1,
                span_ms: 1,
            })
            .with_delay_ms(20),
        );
        struct Shared(Arc<MockBackend>);
        impl ProbeBackend for Shared {
            fn name(&self) -> &'static str {
                "shared-mock"
            }
            fn probe(
                &self,
                target: IpAddr,
                timeout: Duration,
            ) -> Result<crate::probe::ProbeOutcome, ProbeError> {
                self.0.probe(target, timeout)
            }
        }

        let sched = AsyncScheduler::new(Shared(Arc::clone(&backend)));
        let targets: Vec<IpAddr> = (1..=24).map(v4).collect();
        let handle = sched.start(&cfg(6), targets).expect("启动失败");
        let _ = handle.run_to_completion();

        let peak = backend.peak_concurrency();
        assert!(peak > 1, "应当真的并发起来（观测到峰值 {peak}）");
        assert!(peak <= 6, "并发不得超过上限 6，实测峰值 {peak}");
    }

    #[test]
    fn duplicate_start_is_rejected_while_running() {
        let sched = AsyncScheduler::new(
            MockBackend::new(MockPolicy::AllOnline {
                base_rtt_ms: 1,
                span_ms: 1,
            })
            .with_delay_ms(40),
        );
        let targets: Vec<IpAddr> = (1..=8).map(v4).collect();
        let handle = sched
            .start(&cfg(2), targets.clone())
            .expect("首次启动应成功");
        assert!(sched.is_busy());
        let err = sched.start(&cfg(2), targets).unwrap_err();
        assert!(matches!(err, CoreError::InvalidConfig(_)), "{err:?}");
        let _ = handle.run_to_completion();
        assert!(!sched.is_busy());
    }

    #[test]
    fn cancel_stops_scheduling_and_keeps_completed_results() {
        let sched = AsyncScheduler::new(
            MockBackend::new(MockPolicy::AllOnline {
                base_rtt_ms: 1,
                span_ms: 1,
            })
            .with_delay_ms(15),
        );
        let targets: Vec<IpAddr> = (1..=64).map(v4).collect();
        let handle = sched.start(&cfg(2), targets).expect("启动失败");
        // 等一小会儿再取消，确保已经有一部分结果落袋
        std::thread::sleep(Duration::from_millis(60));
        assert!(handle.cancel());

        let outcome = handle.run_to_completion();
        assert!(outcome.cancelled, "应标记为已取消");
        assert!(
            outcome.stats.completed < 64,
            "取消后不应把剩下的目标扫完（完成了 {}）",
            outcome.stats.completed
        );
        assert_eq!(
            outcome.results.len(),
            outcome.stats.completed,
            "已完成的结果必须全部保留"
        );
    }

    #[test]
    fn empty_targets_are_rejected() {
        let sched = AsyncScheduler::new(MockBackend::new(MockPolicy::AllTimeout));
        let err = sched.start(&cfg(4), Vec::new()).unwrap_err();
        assert!(matches!(err, CoreError::InvalidConfig(_)), "{err:?}");
        assert!(!sched.is_busy());
    }

    #[test]
    fn invalid_config_is_rejected_before_starting() {
        let sched = AsyncScheduler::new(MockBackend::new(MockPolicy::AllTimeout));
        let bad = ProbeConfig {
            concurrency: 0,
            ..ProbeConfig::default()
        };
        assert!(sched.start(&bad, vec![v4(1)]).is_err());
        assert!(!sched.is_busy(), "校验失败不得占用 active 标志");
    }

    /// M19 第二步：有界通道下丢弃句柄**不能死锁**——与同步版是同一条不变式、
    /// 同一个坑（`Drop` 必须先丢掉接收端再 join）。两套实现要能互换，
    /// 这类生命周期契约就必须一起守。
    #[test]
    fn dropping_an_async_handle_mid_scan_does_not_deadlock() {
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        std::thread::spawn(move || {
            let sched = AsyncScheduler::new(MockBackend::new(MockPolicy::AllOnline {
                base_rtt_ms: 1,
                span_ms: 5,
            }));
            let targets: Vec<IpAddr> = (0..MAX_TARGETS)
                .map(|i| {
                    IpAddr::V4(std::net::Ipv4Addr::new(
                        10,
                        ((i / 65_536) % 256) as u8,
                        ((i / 256) % 256) as u8,
                        (i % 256) as u8,
                    ))
                })
                .collect();
            let handle = sched.start(&cfg(32), targets).expect("启动失败");
            // **刻意不消费**：等队列填满、发送侧阻塞住。
            std::thread::sleep(Duration::from_millis(100));
            drop(handle); // 必须能返回
            let _ = tx.send(());
        });
        assert!(
            rx.recv_timeout(Duration::from_secs(15)).is_ok(),
            "异步句柄在扫描中途被丢弃时死锁了（有界通道 + join 顺序错）"
        );
    }
}
