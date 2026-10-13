//! 探测后端抽象。
//!
//! 调度器只依赖 [`ProbeBackend`]，因此：
//! - 单元测试可以用 [`MockBackend`] 完全离线地驱动调度逻辑；
//! - 平台实现（Windows / Linux / Android）在 M5 接入，且**必须输出同一套结果模型**；
//! - **TCP 连通性绝不在这里伪装成 ICMP 成功** —— 若 Android 只能做 TCP，
//!   它应当实现为一个语义独立的策略（见 `docs/platform-compatibility.md`）。

use crate::error::ProbeError;
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// 一次成功探测的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeOutcome {
    pub rtt_ms: u32,
    /// 仅当后端能**可靠**取得时才填；取不到必须是 `None`。
    pub mac: Option<String>,
}

impl ProbeOutcome {
    pub fn online(rtt_ms: u32) -> Self {
        Self { rtt_ms, mac: None }
    }
}

/// 让 `Box<dyn ProbeBackend>` 也能直接当作后端使用，
/// 这样 `platform_icmp_backend()` 的返回值可以原样交给调度器。
impl<T: ProbeBackend + ?Sized> ProbeBackend for Box<T> {
    fn name(&self) -> &'static str {
        (**self).name()
    }

    fn probe(&self, target: Ipv4Addr, timeout: Duration) -> Result<ProbeOutcome, ProbeError> {
        (**self).probe(target, timeout)
    }
}

/// 平台探测能力。
pub trait ProbeBackend: Send + Sync {
    /// 后端标识，用于诊断与自我声明（如 `mock`、`windows-icmp`、`linux-raw`）。
    fn name(&self) -> &'static str;

    /// 探测单个目标。**必须**在 `timeout` 量级内返回，不得无限阻塞。
    fn probe(&self, target: Ipv4Addr, timeout: Duration) -> Result<ProbeOutcome, ProbeError>;
}

// ---------------------------------------------------------------------------
// MockBackend：离线驱动调度与 UI
// ---------------------------------------------------------------------------

/// 模拟响应。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MockResponse {
    Online {
        rtt_ms: u32,
    },
    Timeout,
    Unreachable,
    PermissionDenied,
    Unsupported,
    Error(String),
    /// 先睡 `sleep_ms` 再返回；若超过 timeout 则返回超时。
    Slow {
        sleep_ms: u64,
        rtt_ms: u32,
    },
}

impl MockResponse {
    fn to_result(&self, timeout: Duration) -> Result<ProbeOutcome, ProbeError> {
        let budget = timeout.as_millis() as u64;
        match self {
            MockResponse::Online { rtt_ms } => Ok(ProbeOutcome::online(*rtt_ms)),
            MockResponse::Timeout => Err(ProbeError::Timeout { timeout_ms: budget }),
            MockResponse::Unreachable => Err(ProbeError::unreachable("mock: host unreachable")),
            MockResponse::PermissionDenied => {
                Err(ProbeError::permission_denied("mock: 需要更高权限"))
            }
            MockResponse::Unsupported => Err(ProbeError::unsupported("mock: 平台不支持 ICMP")),
            MockResponse::Error(msg) => Err(ProbeError::internal(msg.clone())),
            MockResponse::Slow { sleep_ms, rtt_ms } => {
                if *sleep_ms > budget {
                    std::thread::sleep(timeout);
                    Err(ProbeError::Timeout { timeout_ms: budget })
                } else {
                    std::thread::sleep(Duration::from_millis(*sleep_ms));
                    Ok(ProbeOutcome::online(*rtt_ms))
                }
            }
        }
    }
}

/// 模拟策略。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MockPolicy {
    /// 全部在线，RTT = `base + host % span`。
    AllOnline {
        base_rtt_ms: u32,
        span_ms: u32,
    },
    AllTimeout,
    AllUnreachable,
    AllPermissionDenied,
    AllUnsupported,
    /// 满足 `host % online_every == 0` 的在线，其余超时。
    Every {
        online_every: u8,
        rtt_ms: u32,
    },
    /// 较接近真实网段的分布：约 `online_percent`% 在线，RTT 覆盖低/中/高三档。
    /// 完全由主机段决定，因此结果可重复。
    Realistic {
        online_percent: u8,
        base_rtt_ms: u32,
        span_ms: u32,
    },
    /// 显式脚本；未列出的目标按超时处理。
    Scripted(Vec<(Ipv4Addr, MockResponse)>),
}

/// 可观测的模拟后端：记录调用次数与峰值并发，便于断言「并发不超过上限」。
#[derive(Debug)]
pub struct MockBackend {
    policy: MockPolicy,
    current: AtomicUsize,
    peak: AtomicUsize,
    calls: AtomicUsize,
    /// 每次 probe 的固定开销，用于让并发观测更稳定（默认 0）。
    delay_ms: u64,
}

impl MockBackend {
    pub fn new(policy: MockPolicy) -> Self {
        Self {
            policy,
            current: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            calls: AtomicUsize::new(0),
            delay_ms: 0,
        }
    }

    /// 让每次探测先睡眠 `ms`，便于稳定地观察并发行为。
    pub fn with_delay_ms(mut self, ms: u64) -> Self {
        self.delay_ms = ms;
        self
    }

    pub fn policy(&self) -> &MockPolicy {
        &self.policy
    }

    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    /// 观测到的峰值并发。
    pub fn peak_concurrency(&self) -> usize {
        self.peak.load(Ordering::SeqCst)
    }

    fn resolve(&self, target: Ipv4Addr) -> MockResponse {
        let host = target.octets()[3];
        match &self.policy {
            MockPolicy::AllOnline {
                base_rtt_ms,
                span_ms,
            } => MockResponse::Online {
                rtt_ms: base_rtt_ms + (host as u32) % (*span_ms).max(1),
            },
            MockPolicy::AllTimeout => MockResponse::Timeout,
            MockPolicy::AllUnreachable => MockResponse::Unreachable,
            MockPolicy::AllPermissionDenied => MockResponse::PermissionDenied,
            MockPolicy::AllUnsupported => MockResponse::Unsupported,
            MockPolicy::Every {
                online_every,
                rtt_ms,
            } => {
                if *online_every != 0 && host % *online_every == 0 {
                    MockResponse::Online { rtt_ms: *rtt_ms }
                } else {
                    MockResponse::Timeout
                }
            }
            MockPolicy::Realistic {
                online_percent,
                base_rtt_ms,
                span_ms,
            } => {
                let h = host as u32;
                if (h * 37) % 100 < (*online_percent as u32).min(100) {
                    MockResponse::Online {
                        rtt_ms: base_rtt_ms + (h * 7) % (*span_ms).max(1),
                    }
                } else {
                    MockResponse::Timeout
                }
            }
            MockPolicy::Scripted(entries) => entries
                .iter()
                .find(|(ip, _)| *ip == target)
                .map(|(_, r)| r.clone())
                .unwrap_or(MockResponse::Timeout),
        }
    }
}

/// 并发计数守卫，保证即使 panic 也会归还计数。
struct ConcurrencyGuard<'a>(&'a AtomicUsize);

impl Drop for ConcurrencyGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl ProbeBackend for MockBackend {
    fn name(&self) -> &'static str {
        "mock"
    }

    fn probe(&self, target: Ipv4Addr, timeout: Duration) -> Result<ProbeOutcome, ProbeError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let running = self.current.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(running, Ordering::SeqCst);
        let _guard = ConcurrencyGuard(&self.current);

        if self.delay_ms > 0 {
            std::thread::sleep(Duration::from_millis(self.delay_ms));
        }
        self.resolve(target).to_result(timeout)
    }
}

/// 记录所有调用目标，用于验证「取消后不再调度新目标」。
#[derive(Debug)]
pub struct RecordingBackend {
    inner_policy: MockPolicy,
    seen: std::sync::Mutex<Vec<Ipv4Addr>>,
    current: AtomicUsize,
    peak: AtomicUsize,
    delay_ms: u64,
}

impl RecordingBackend {
    pub fn new(policy: MockPolicy) -> Self {
        Self {
            inner_policy: policy,
            seen: std::sync::Mutex::new(Vec::new()),
            current: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            delay_ms: 0,
        }
    }

    /// 让每次探测先睡眠 `ms`。取消类测试必须设置它，否则 254 个模拟目标会在微秒级跑完。
    pub fn with_delay_ms(mut self, ms: u64) -> Self {
        self.delay_ms = ms;
        self
    }

    pub fn seen_targets(&self) -> Vec<Ipv4Addr> {
        self.seen.lock().expect("poisoned").clone()
    }

    pub fn peak_concurrency(&self) -> usize {
        self.peak.load(Ordering::SeqCst)
    }
}

impl ProbeBackend for RecordingBackend {
    fn name(&self) -> &'static str {
        "recording"
    }

    fn probe(&self, target: Ipv4Addr, timeout: Duration) -> Result<ProbeOutcome, ProbeError> {
        {
            let mut seen = self.seen.lock().expect("poisoned");
            seen.push(target);
        }
        let running = self.current.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(running, Ordering::SeqCst);
        let _guard = ConcurrencyGuard(&self.current);

        if self.delay_ms > 0 {
            std::thread::sleep(Duration::from_millis(self.delay_ms));
        }
        MockBackend::new(self.inner_policy.clone()).probe(target, timeout)
    }
}
