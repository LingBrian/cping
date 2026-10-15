//! 并发调度、超时、取消与事件流。
//!
//! 实现选择：**std 线程 + channel**，不引入 Tokio。
//! 理由：ICMP 探测本身是阻塞式系统调用；线程池模型对 32～256 并发完全够用，
//! 且能原样在 Android 上编译运行（Tokio 在 Android 上另有复杂度）。
//!
//! 生命周期契约（对应需求「任务生命周期」）：
//! - 每个任务唯一 `task_id`；
//! - 同一调度器同一时刻只允许一个活跃任务，重复 `start` 会被拒绝；
//! - `cancel()` 之后**不再取新目标**，已发出的探测等其自然结束；
//! - 已完成的结果**全部保留**；
//! - `Finished` / `Cancelled` 是唯一两个终态事件。

use crate::config::ProbeConfig;
use crate::error::{CoreError, CoreResult, ProbeError};
use crate::policy::{RateLimiter, ScanPolicy};
use crate::probe::ProbeBackend;
use crate::result::{ProbeEvent, ProbeResult, ProbeStatus, ScanStats, TaskId};
use std::collections::VecDeque;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// 生成唯一 task_id：`t<毫秒十六进制>-<序号>`。
pub fn next_task_id() -> TaskId {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let seq = COUNTER.fetch_add(1, Ordering::SeqCst);
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    format!("t{ms:x}-{seq}")
}

/// 任务终态摘要。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    pub task_id: TaskId,
    pub cancelled: bool,
    pub stats: ScanStats,
    pub elapsed_ms: u64,
    /// 已完成的结果，按目标下标升序。取消时**只包含已完成的部分**。
    pub results: Vec<ProbeResult>,
}

/// 目标队列项。
type QueueItem = (u16, IpAddr);

struct Shared {
    queue: Mutex<VecDeque<QueueItem>>,
    results: Mutex<Vec<Option<ProbeResult>>>,
    /// 用 `Arc` 包一层，好让 UI 侧能单独持有取消令牌（见 `SchedulerHandle::cancel_token`）
    cancel: Arc<AtomicBool>,
    completed: AtomicUsize,
    active: Arc<AtomicBool>,
    tx: Sender<ProbeEvent>,
    task_id: TaskId,
    total: usize,
    timeout: Duration,
    /// M17：全局限速。所有 worker 共用同一个令牌桶。
    limiter: Arc<RateLimiter>,
}

/// 调度器。可长期持有，反复 `start`。
pub struct ProbeScheduler {
    backend: Arc<dyn ProbeBackend>,
    active: Arc<AtomicBool>,
    /// M17：安全策略（限速 / 目标上限 / 保留网段守卫）。
    ///
    /// 挂在调度器上而不是塞进 [`ProbeConfig`]：`ProbeConfig` 会被配置文件
    /// 序列化、被模板覆盖，而「限速多少」是**本机合规策略**，不该被一份
    /// 从别处拷来的模板悄悄改掉。
    policy: ScanPolicy,
}

impl ProbeScheduler {
    pub fn new<B: ProbeBackend + 'static>(backend: B) -> Self {
        Self {
            backend: Arc::new(backend),
            active: Arc::new(AtomicBool::new(false)),
            policy: ScanPolicy::default(),
        }
    }

    /// 换一套安全策略。三端在 `start` 之前调用。
    pub fn set_policy(&mut self, policy: ScanPolicy) -> CoreResult<()> {
        policy.validate()?;
        self.policy = policy;
        Ok(())
    }

    pub fn policy(&self) -> ScanPolicy {
        self.policy
    }

    pub fn backend_name(&self) -> &'static str {
        self.backend.name()
    }

    /// 是否有任务正在运行。
    pub fn is_busy(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    /// 启动一次探测。目标必须是已生成好的列表（顺序即 `index`）。
    ///
    /// 会在**起线程之前**按 [`ScanPolicy`] 校验：目标数上限与保留网段。
    /// 保留网段在这里拦而不是在解析 `--target` 时拦，是因为「展开后到底是
    /// 哪些地址」只有到这里才确定（`224.0.0.0/29` 这种写法在解析阶段看不出来）。
    pub fn start(&self, config: &ProbeConfig, targets: Vec<IpAddr>) -> CoreResult<SchedulerHandle> {
        config.validate()?;
        if targets.is_empty() {
            return Err(CoreError::InvalidConfig("目标列表为空".into()));
        }
        // M17：数量上限与保留网段守卫（一条路，不做两处判断）
        self.policy.check_targets(&targets)?;
        // 重复任务防护
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
        let worker_count = config.concurrency.min(total).max(1);
        let (tx, rx) = mpsc::channel::<ProbeEvent>();
        let start = Instant::now();

        let shared = Arc::new(Shared {
            queue: Mutex::new(
                targets
                    .into_iter()
                    .enumerate()
                    .map(|(i, ip)| (i as u16, ip))
                    .collect(),
            ),
            results: Mutex::new(vec![None; total]),
            cancel: Arc::new(AtomicBool::new(false)),
            completed: AtomicUsize::new(0),
            active: Arc::clone(&self.active),
            tx: tx.clone(),
            task_id: task_id.clone(),
            total,
            timeout: Duration::from_millis(config.timeout_ms),
            limiter: Arc::new(RateLimiter::new(self.policy.max_pps)),
        });

        let _ = tx.send(ProbeEvent::Started {
            task_id: task_id.clone(),
            total,
        });

        let backend = Arc::clone(&self.backend);
        let coord_shared = Arc::clone(&shared);
        let coordinator = thread::spawn(move || {
            let mut workers: Vec<JoinHandle<()>> = Vec::with_capacity(worker_count);
            for _ in 0..worker_count {
                let s = Arc::clone(&coord_shared);
                let b = Arc::clone(&backend);
                workers.push(thread::spawn(move || worker_loop(b, s)));
            }
            for w in workers {
                let _ = w.join();
            }

            let elapsed_ms = start.elapsed().as_millis() as u64;
            let collected: Vec<ProbeResult> = {
                let slots = coord_shared.results.lock().expect("results poisoned");
                slots.iter().flatten().cloned().collect()
            };
            let stats = ScanStats::from_results_with_total(&collected, coord_shared.total);
            let cancelled = coord_shared.cancel.load(Ordering::SeqCst);
            coord_shared.active.store(false, Ordering::SeqCst);

            let event = if cancelled {
                ProbeEvent::Cancelled {
                    task_id: coord_shared.task_id.clone(),
                    stats,
                    elapsed_ms,
                }
            } else {
                ProbeEvent::Finished {
                    task_id: coord_shared.task_id.clone(),
                    stats,
                    elapsed_ms,
                }
            };
            let _ = coord_shared.tx.send(event);
        });

        let _ = tx; // 主线程不再直接发送，靠 shared.tx

        Ok(SchedulerHandle {
            task_id,
            total,
            shared,
            events: rx,
            coordinator: Some(coordinator),
        })
    }
}

/// worker：不断从队列取出目标并探测，直到队列空或被取消。
fn worker_loop(backend: Arc<dyn ProbeBackend>, shared: Arc<Shared>) {
    loop {
        if shared.cancel.load(Ordering::SeqCst) {
            break;
        }
        let item = {
            let mut q = shared.queue.lock().expect("queue poisoned");
            q.pop_front()
        };
        let Some((index, ip)) = item else { break };

        // 取到目标后再次确认取消状态：取消后绝不发起新的探测
        if shared.cancel.load(Ordering::SeqCst) {
            break;
        }

        // M17：全局限速。放在「已拿到目标、确认没取消」之后、「真正发包」之前
        // ——这样被限速挡下的目标不会被算作已发出，取消也能立刻生效
        //（限速绝不能变成取消不掉的等待）。
        shared.limiter.wait(|| shared.cancel.load(Ordering::SeqCst));
        if shared.cancel.load(Ordering::SeqCst) {
            break;
        }

        let (status, mac) = match backend.probe(ip, shared.timeout) {
            Ok(outcome) => (
                ProbeStatus::Online {
                    rtt_ms: outcome.rtt_ms,
                },
                outcome.mac,
            ),
            Err(err) => (status_from_error(err), None),
        };

        // 用构造函数 + 赋值而不是结构体字面量：M7 之后 ProbeResult 会继续长字段，
        // 这样新增字段时这里无需跟着改。
        let mut result = ProbeResult::new(index, ip, status);
        result.mac = mac;

        {
            let mut slots = shared.results.lock().expect("results poisoned");
            if let Some(slot) = slots.get_mut(index as usize) {
                *slot = Some(result.clone());
            }
        }

        let done = shared.completed.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = shared.tx.send(ProbeEvent::Result {
            task_id: shared.task_id.clone(),
            result,
        });
        let _ = shared.tx.send(ProbeEvent::Progress {
            task_id: shared.task_id.clone(),
            completed: done,
            total: shared.total,
        });
    }
}

/// 错误 → 状态。**权限不足 / 不支持 / 内部错误不得混同为超时或不可达。**
pub fn status_from_error(error: ProbeError) -> ProbeStatus {
    match error {
        ProbeError::Timeout { .. } => ProbeStatus::Timeout,
        ProbeError::Unreachable { .. } => ProbeStatus::Unreachable,
        ProbeError::PermissionDenied { .. } => ProbeStatus::PermissionDenied,
        ProbeError::Unsupported { .. } => ProbeStatus::Unsupported,
        ProbeError::Internal { detail } => ProbeStatus::Error(detail),
    }
}

/// 可克隆、可跨线程的取消令牌。
///
/// GUI 场景下任务句柄通常被移进后台线程消费事件，UI 侧只保留这个令牌用于「停止」。
#[derive(Debug, Clone)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    /// 由共享标志构造。crate 内使用：异步调度器（`asynchronous`）与同步调度器
    /// 必须操作**同一个**取消标志，否则两套实现的取消语义会分叉。
    #[cfg(feature = "async")]
    pub(crate) fn from_flag(flag: Arc<AtomicBool>) -> Self {
        Self(flag)
    }

    /// 请求取消；返回是否本次调用真正触发了取消。
    pub fn cancel(&self) -> bool {
        !self.0.swap(true, Ordering::SeqCst)
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// 任务句柄：取消、消费事件、等待收敛。
pub struct SchedulerHandle {
    pub task_id: TaskId,
    pub total: usize,
    shared: Arc<Shared>,
    events: Receiver<ProbeEvent>,
    coordinator: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for SchedulerHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SchedulerHandle")
            .field("task_id", &self.task_id)
            .field("total", &self.total)
            .field("cancelled", &self.is_cancelled())
            .field("completed", &self.started_count())
            .finish()
    }
}

impl SchedulerHandle {
    /// 取出可跨线程的取消令牌（可在把句柄移进后台线程后继续使用）。
    pub fn cancel_token(&self) -> CancelToken {
        CancelToken(Arc::clone(&self.shared.cancel))
    }

    /// 请求取消。返回是否本次调用真正触发了取消。
    pub fn cancel(&self) -> bool {
        !self.shared.cancel.swap(true, Ordering::SeqCst)
    }

    pub fn is_cancelled(&self) -> bool {
        self.shared.cancel.load(Ordering::SeqCst)
    }

    /// 已发送的探测数（不含被取消跳过的目标）。
    pub fn started_count(&self) -> usize {
        self.shared.completed.load(Ordering::SeqCst)
    }

    /// 阻塞接收下一个事件；所有发送端关闭后返回 `None`。
    pub fn recv(&self) -> Option<ProbeEvent> {
        self.events.recv().ok()
    }

    pub fn recv_timeout(&self, timeout: Duration) -> Result<ProbeEvent, RecvTimeoutError> {
        self.events.recv_timeout(timeout)
    }

    /// 非阻塞地取出当前已就绪的全部事件。
    pub fn drain(&self) -> Vec<ProbeEvent> {
        let mut out = Vec::new();
        while let Ok(ev) = self.events.try_recv() {
            out.push(ev);
        }
        out
    }

    /// 当前已完成结果快照（按下标升序）。
    pub fn snapshot(&self) -> Vec<ProbeResult> {
        let slots = self.shared.results.lock().expect("results poisoned");
        slots.iter().flatten().cloned().collect()
    }

    /// 消费事件直到终态，返回完整摘要。测试与 CLI 场景使用。
    pub fn run_to_completion(mut self) -> RunOutcome {
        let mut cancelled = false;
        let mut stats = ScanStats::from_results_with_total(&[], self.total);
        let mut elapsed_ms = 0;
        while let Ok(event) = self.events.recv() {
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
        if let Some(handle) = self.coordinator.take() {
            let _ = handle.join();
        }
        RunOutcome {
            task_id: self.task_id.clone(),
            cancelled,
            stats,
            elapsed_ms,
            results,
        }
    }

    /// 等待协调线程结束（通常 `run_to_completion` 已足够）。
    pub fn join(&mut self) {
        if let Some(handle) = self.coordinator.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for SchedulerHandle {
    fn drop(&mut self) {
        // 句柄被丢弃时确保工作线程不会悬挂。
        self.shared.cancel.store(true, Ordering::SeqCst);
        if let Some(handle) = self.coordinator.take() {
            let _ = handle.join();
        }
    }
}
