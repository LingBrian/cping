//! 安全策略（M17，v2.5.0）：限速、上限、保留网段守卫、审计日志、首次启动确认。
//!
//! 这一模块的存在理由不是「功能」，是**责任边界**。一个批量 PING 工具离
//! 「不小心把自己的网段打瘫」或「被当成扫描器」只有一步之遥，所以三件事必须
//! 由核心层统一把关，而不是靠各端自觉：
//!
//! 1. **限速**：全局包速率上限，默认生效。令牌桶的算术部分是纯函数
//!    （[`Bucket`]），与时钟分离——这样能确定性地测，不靠 `sleep`。
//! 2. **保留网段守卫**：组播（`224/4`）、保留段（`240/4`，含 `255.255.255.255`
//!    广播）、文档示例网段这些「扫了要么没意义、要么会打扰别人」的地址，
//!    默认拒绝，必须显式放行。
//! 3. **审计日志**：每次会话与每轮扫描落一条 JSONL，可导出用于自证合规。
//!    审计里**只放**「谁、什么时候、扫了什么规格、结果计数」——
//!    **不放** MAC、厂商、主机名反查结果等隐私数据，也不逐主机展开明细。
//!
//! 关于「首次启动提示」：本模块提供跨平台的确认状态落盘（[`consent`]），
//! 但**不做阻塞式交互**——CLI 会被 CI 调用，卡在等人按回车等于把工具弄坏。
//! 提示打一次、记一次，就够起到告知作用。

use crate::error::{CoreError, CoreResult};
use crate::i18n::{self, Key, Lang};
use crate::json::Json;
use crate::target::MAX_TARGETS;
use std::net::IpAddr;
use std::path::PathBuf;
use std::time::Instant;

/// 默认全局速率上限（探测包/秒）。
///
/// 取 500 是一个折中：局域网里 255 个目标的正常扫描约 0.5 秒跑完（远快于人类
/// 感知），而突发流量只有「不限速」时的几十分之一。想更快就显式调大，
/// 想彻底关掉就写 `0`——**但要由人显式决定**，不能是默认行为。
pub const DEFAULT_MAX_PPS: u32 = 500;

/// 速率上限的硬天花板。再往上限速就失去意义了（那和关掉没区别），
/// 与其接受一个自欺欺人的数字，不如直接拒绝。
pub const MAX_PPS_LIMIT: u32 = 100_000;

/// 审计日志的首行格式标识。带版本号，方便以后换格式时能拒绝读旧文件。
pub const AUDIT_FORMAT: &str = "CPING-AUDIT";

/// 审计格式版本。
pub const AUDIT_FORMAT_VERSION: u32 = 1;

/// 授权确认的版本。改一次版本等于「重新告知一遍」。
pub const CONSENT_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// 保留网段
// ---------------------------------------------------------------------------

/// 判断地址是否落在「保留 / 特殊用途」网段，返回**给用户看的原因**。
///
/// 刻意**不拦**私有网段（`10/8`、`172.16/12`、`192.168/16`、`fc00::/7`）和
/// 回环（`127/8`、`::1`）——那正是这类工具的正常用途（扫自己的局域网、
/// 用回环做自测）。拦掉它们等于把工具废掉。
///
/// 拦的是这三类：
/// - **会打扰别人**：组播（打到整组）、广播（打到整个网段）
/// - **没有意义**：`0.0.0.0/8`（「本网络」，不是可探测的目标）
/// - **一定会失败**：文档示例网段（`192.0.2/24` 等，公网不路由它们）
///
/// 返回 `Some(原因)` 表示应拒绝；`None` 表示可以扫。
///
/// 这是**中文快捷方式**，等价于 `reserved_kind(ip).map(|k| k.reason(Lang::Zh))`；
/// 需要别的语言请直接用 [`reserved_kind`]。
pub fn reserved_reason(ip: IpAddr) -> Option<&'static str> {
    reserved_kind(ip).map(|k| k.reason(Lang::Zh))
}

/// 保留网段的**类别**。
///
/// 返回类别而不是直接返回句子，有两个理由：同一句原因要被翻成不同语言；
/// 而类别本身是稳定的——写进审计、文档与脚本判断都该用它，
/// 句子措辞随时可以改。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReservedKind {
    /// IPv4 组播 `224.0.0.0/4`。
    MulticastV4,
    /// IPv4 保留段 `240.0.0.0/4`（含 `255.255.255.255` 有限广播）。
    ReservedRangeV4,
    /// `0.0.0.0/8`，「本网络」。
    ThisNetwork,
    /// 文档示例网段 `192.0.2.0/24`（TEST-NET-1）。
    TestNet1,
    /// 文档示例网段 `198.51.100.0/24`（TEST-NET-2）。
    TestNet2,
    /// 文档示例网段 `203.0.113.0/24`（TEST-NET-3）。
    TestNet3,
    /// 基准测试网段 `198.18.0.0/15`（RFC 2544）。
    BenchmarkNet,
    /// IPv6 未指定地址 `::`。
    UnspecifiedV6,
    /// IPv6 组播 `ff00::/8`。
    MulticastV6,
    /// 文档示例网段 `2001:db8::/32`（RFC 3849）。
    DocNetV6,
}

/// 全部类别。供测试穷举，也供界面生成「哪些网段会被拒绝」的说明表。
pub const ALL_RESERVED_KINDS: [ReservedKind; 10] = [
    ReservedKind::MulticastV4,
    ReservedKind::ReservedRangeV4,
    ReservedKind::ThisNetwork,
    ReservedKind::TestNet1,
    ReservedKind::TestNet2,
    ReservedKind::TestNet3,
    ReservedKind::BenchmarkNet,
    ReservedKind::UnspecifiedV6,
    ReservedKind::MulticastV6,
    ReservedKind::DocNetV6,
];

impl ReservedKind {
    /// 稳定的机器可读标识（写进审计、文档与脚本判断；
    /// **不要**用匹配句子文本来判断类别）。
    pub const fn code(self) -> &'static str {
        match self {
            ReservedKind::MulticastV4 => "multicast_v4",
            ReservedKind::ReservedRangeV4 => "reserved_range_v4",
            ReservedKind::ThisNetwork => "this_network",
            ReservedKind::TestNet1 => "test_net_1",
            ReservedKind::TestNet2 => "test_net_2",
            ReservedKind::TestNet3 => "test_net_3",
            ReservedKind::BenchmarkNet => "benchmark_net",
            ReservedKind::UnspecifiedV6 => "unspecified_v6",
            ReservedKind::MulticastV6 => "multicast_v6",
            ReservedKind::DocNetV6 => "doc_net_v6",
        }
    }

    /// 对应的 i18n 键。
    pub const fn key(self) -> Key {
        match self {
            ReservedKind::MulticastV4 => Key::RsrvMulticastV4,
            ReservedKind::ReservedRangeV4 => Key::RsrvReservedRangeV4,
            ReservedKind::ThisNetwork => Key::RsrvThisNetwork,
            ReservedKind::TestNet1 => Key::RsrvTestNet1,
            ReservedKind::TestNet2 => Key::RsrvTestNet2,
            ReservedKind::TestNet3 => Key::RsrvTestNet3,
            ReservedKind::BenchmarkNet => Key::RsrvBenchmarkNet,
            ReservedKind::UnspecifiedV6 => Key::RsrvUnspecifiedV6,
            ReservedKind::MulticastV6 => Key::RsrvMulticastV6,
            ReservedKind::DocNetV6 => Key::RsrvDocNetV6,
        }
    }

    /// 按语言渲染成给人看的原因。
    pub fn reason(self, lang: Lang) -> &'static str {
        i18n::text(lang, self.key())
    }
}

/// 判断地址是否落在保留 / 特殊用途网段，返回**类别**。
pub fn reserved_kind(ip: IpAddr) -> Option<ReservedKind> {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            // 组播 224.0.0.0/4
            if (224..=239).contains(&o[0]) {
                return Some(ReservedKind::MulticastV4);
            }
            // 保留段 240.0.0.0/4（含 255.255.255.255 有限广播）
            if o[0] >= 240 {
                return Some(ReservedKind::ReservedRangeV4);
            }
            // 「本网络」0.0.0.0/8
            if o[0] == 0 {
                return Some(ReservedKind::ThisNetwork);
            }
            // 文档示例网段（RFC 5737）
            if o[0] == 192 && o[1] == 0 && o[2] == 2 {
                return Some(ReservedKind::TestNet1);
            }
            if o[0] == 198 && o[1] == 51 && o[2] == 100 {
                return Some(ReservedKind::TestNet2);
            }
            if o[0] == 203 && o[1] == 0 && o[2] == 113 {
                return Some(ReservedKind::TestNet3);
            }
            // 基准测试网段 198.18.0.0/15（RFC 2544）
            if o[0] == 198 && (o[1] == 18 || o[1] == 19) {
                return Some(ReservedKind::BenchmarkNet);
            }
            None
        }
        IpAddr::V6(v6) => {
            if v6.is_unspecified() {
                return Some(ReservedKind::UnspecifiedV6);
            }
            // 组播 ff00::/8
            if v6.octets()[0] == 0xff {
                return Some(ReservedKind::MulticastV6);
            }
            // 文档示例 2001:db8::/32（RFC 3849）
            let s = v6.segments();
            if s[0] == 0x2001 && s[1] == 0x0db8 {
                return Some(ReservedKind::DocNetV6);
            }
            None
        }
    }
}

// ---------------------------------------------------------------------------
// 策略
// ---------------------------------------------------------------------------

/// 一次扫描要遵守的安全策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanPolicy {
    /// 全局速率上限（探测包/秒）。`0` = 不限速（**必须显式选**）。
    pub max_pps: u32,
    /// 单次任务的目标数上限。
    ///
    /// **只能收紧，不能放宽**：[`MAX_TARGETS`] 是架构硬上限（结果槽位是按它
    /// 预分配的），拿一个参数去突破它只会换来一个更晚的崩溃。
    pub max_targets: usize,
    /// 放行组播 / 保留段等特殊网段。
    pub allow_reserved: bool,
}

impl Default for ScanPolicy {
    fn default() -> Self {
        Self {
            max_pps: DEFAULT_MAX_PPS,
            max_targets: MAX_TARGETS,
            allow_reserved: false,
        }
    }
}

impl ScanPolicy {
    /// 校验策略本身是否自洽（中文，等价于 `validate_in(Lang::Zh)`）。
    pub fn validate(&self) -> CoreResult<()> {
        self.validate_in(Lang::Zh)
    }

    /// 校验策略本身是否自洽，错误信息按 `lang` 渲染。
    ///
    /// **越界的值一律报错，不静默夹取**——「我明明设了 1000 怎么还是 500」
    /// 比报错难查得多。
    pub fn validate_in(&self, lang: Lang) -> CoreResult<()> {
        if self.max_pps > MAX_PPS_LIMIT {
            return Err(CoreError::InvalidConfig(i18n::render(
                lang,
                Key::PolicyPpsTooHigh,
                &[&self.max_pps.to_string(), &MAX_PPS_LIMIT.to_string()],
            )));
        }
        if self.max_targets == 0 {
            return Err(CoreError::InvalidConfig(i18n::render(
                lang,
                Key::PolicyTargetsZero,
                &[],
            )));
        }
        if self.max_targets > MAX_TARGETS {
            return Err(CoreError::InvalidConfig(i18n::render(
                lang,
                Key::PolicyTargetsTooHigh,
                &[&self.max_targets.to_string(), &MAX_TARGETS.to_string()],
            )));
        }
        Ok(())
    }

    /// 按策略检查目标列表（中文，等价于 `check_targets_in(.., Lang::Zh)`）。
    pub fn check_targets(&self, targets: &[IpAddr]) -> CoreResult<()> {
        self.check_targets_in(targets, Lang::Zh)
    }

    /// 按策略检查目标列表：数量上限 + 保留网段，提示按 `lang` 渲染。
    ///
    /// 保留网段**只报第一个**命中的地址并附原因。把 255 条都列出来不会让人
    /// 更快地修好配置，只会把真正的那一句淹掉。
    pub fn check_targets_in(&self, targets: &[IpAddr], lang: Lang) -> CoreResult<()> {
        self.validate_in(lang)?;
        if targets.len() > self.max_targets {
            return Err(CoreError::TooManyTargets {
                actual: targets.len(),
                limit: self.max_targets,
            });
        }
        if self.allow_reserved {
            return Ok(());
        }
        for ip in targets {
            if let Some(kind) = reserved_kind(*ip) {
                return Err(CoreError::ReservedTarget {
                    ip: ip.to_string(),
                    // 原因在**这里**就翻好，而不是留到错误类型里翻：
                    // `CoreError::ReservedTarget` 的 `reason` 是自由文本，
                    // 它不该知道保留网段有哪几个类别。
                    reason: kind.reason(lang).to_string(),
                });
            }
        }
        Ok(())
    }

    /// 一行摘要，供界面与审计记录使用（中文）。
    pub fn describe(&self) -> String {
        self.describe_in(Lang::Zh)
    }

    /// 一行摘要，按 `lang` 渲染。
    pub fn describe_in(&self, lang: Lang) -> String {
        let pps = if self.max_pps == 0 {
            i18n::text(lang, Key::PolicyPpsUnlimited).to_string()
        } else {
            i18n::render(lang, Key::PolicyPpsValue, &[&self.max_pps.to_string()])
        };
        let mut s = i18n::render(
            lang,
            Key::PolicyDescribe,
            &[&pps, &self.max_targets.to_string()],
        );
        if self.allow_reserved {
            // 单独拼一段而不是塞进模板：不显示时不能留下一个悬空的连接符。
            s.push_str(i18n::text(lang, Key::PolicyDescribeReserved));
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 令牌桶
// ---------------------------------------------------------------------------

/// 令牌桶的**纯算术部分**：与时钟完全分离。
///
/// 分开的理由是可测性。「限速对不对」如果只能靠 `sleep` 再数时间来验证，
/// 那这个测试既慢又飘；把「给定时刻与当前令牌数，能不能发、不能发要等多久」
/// 做成纯函数之后，全部边界都能用整数断言钉死。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bucket {
    /// 剩余令牌，单位是「千分之一包」。用整数避免浮点累积误差。
    tokens_milli: u64,
    /// 上次补充令牌的时刻（毫秒，相对某个单调原点）。
    last_ms: u64,
}

impl Bucket {
    /// 造一个**满桶**。满桶意味着「允许一次瞬时突发」，这是刻意的：
    /// 否则第一个包也要等 1/rate 秒，短任务会凭空多出延迟。
    pub fn full(rate_pps: u32) -> Self {
        Self {
            tokens_milli: rate_pps as u64 * 1000,
            last_ms: 0,
        }
    }

    /// 当前毫秒时刻的剩余令牌（按速率补充后，不超过满桶）。
    fn refilled(&mut self, now_ms: u64, rate_pps: u32) {
        let elapsed = now_ms.saturating_sub(self.last_ms);
        self.last_ms = now_ms;
        if elapsed == 0 || rate_pps == 0 {
            return;
        }
        let cap = rate_pps as u64 * 1000;
        // 按「毫秒 × 包/秒 ÷ 1000」补充；先乘后除，不做中间浮点。
        let add = elapsed.saturating_mul(rate_pps as u64);
        self.tokens_milli = (self.tokens_milli + add).min(cap);
    }

    /// 尝试取一个令牌。
    ///
    /// - 取到 → `None`
    /// - 没取到 → `Some(还需等待的毫秒数)`，且**不会**改变桶内令牌
    ///   （等够了再来取，而不是「预订」一个未来的令牌——预订会让多个 worker
    ///   同时算出一堆重叠的等待时间，实际速率反而超标）
    pub fn take(&mut self, now_ms: u64, rate_pps: u32) -> Option<u64> {
        if rate_pps == 0 {
            return None;
        }
        self.refilled(now_ms, rate_pps);
        if self.tokens_milli >= 1000 {
            self.tokens_milli -= 1000;
            return None;
        }
        let missing = 1000 - self.tokens_milli;
        // 向上取整：宁可多等 1 毫秒，也不要把速率挤过上限
        Some(missing.div_ceil(rate_pps as u64))
    }

    /// 剩余整整几个令牌（只用于测试与展示）。
    pub fn tokens(&self) -> u64 {
        self.tokens_milli / 1000
    }
}

/// 带时钟的令牌桶，供调度器在多线程间共享。
pub struct RateLimiter {
    rate_pps: u32,
    bucket: std::sync::Mutex<Bucket>,
    origin: Instant,
}

impl RateLimiter {
    /// `rate_pps == 0` 表示不限速，此时 [`RateLimiter::wait`] 直接返回。
    pub fn new(rate_pps: u32) -> Self {
        Self {
            rate_pps,
            bucket: std::sync::Mutex::new(Bucket::full(rate_pps)),
            origin: Instant::now(),
        }
    }

    /// 速率（包/秒）；`0` 表示不限速。
    pub fn rate_pps(&self) -> u32 {
        self.rate_pps
    }

    /// 是否完全不限速。
    pub fn is_unlimited(&self) -> bool {
        self.rate_pps == 0
    }

    /// 阻塞到「可以发下一个探测」为止。
    ///
    /// `give_up` 每轮醒来都会被问一次，返回 `true` 表示调用方已被取消
    /// ——限速绝不能变成「取消不掉的等待」，那样用户只能杀进程。
    pub fn wait(&self, mut give_up: impl FnMut() -> bool) {
        if self.rate_pps == 0 {
            return;
        }
        // 单次等待不超过 50 毫秒：既让取消及时生效，也不至于退化成自旋。
        const SLICE_MS: u64 = 50;
        loop {
            if give_up() {
                return;
            }
            let now_ms = self.origin.elapsed().as_millis() as u64;
            let wait = {
                let mut b = self.bucket.lock().expect("rate bucket poisoned");
                b.take(now_ms, self.rate_pps)
            };
            match wait {
                None => return,
                Some(ms) => {
                    if give_up() {
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(ms.clamp(1, SLICE_MS)));
                }
            }
        }
    }
}

impl std::fmt::Debug for RateLimiter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RateLimiter")
            .field("rate_pps", &self.rate_pps)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// 审计日志
// ---------------------------------------------------------------------------

/// 审计事件类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditEvent {
    /// 会话开始。记下「谁、什么时候、用什么参数、扫什么规格」。
    SessionStart,
    /// 一轮扫描结束。记结果**计数**，不记逐主机明细。
    Round,
    /// 会话结束（含退出码）。
    SessionEnd,
}

impl AuditEvent {
    pub const fn key(self) -> &'static str {
        match self {
            AuditEvent::SessionStart => "session_start",
            AuditEvent::Round => "round",
            AuditEvent::SessionEnd => "session_end",
        }
    }

    /// 中文标签（等价于 `label_in(Lang::Zh)`）。
    ///
    /// 核心层内部的报表与导出仍在用它——**产物内容是数据契约**：
    /// 同一份 CSV / HTML 在中英文机器上必须一致；只有界面跟着语言变。
    pub fn label_zh(self) -> &'static str {
        self.label_in(Lang::Zh)
    }

    /// 按语言渲染的标签。
    pub fn label_in(self, lang: Lang) -> &'static str {
        i18n::text(
            lang,
            match self {
                AuditEvent::SessionStart => Key::LabelAuditSessionStart,
                AuditEvent::Round => Key::LabelAuditRound,
                AuditEvent::SessionEnd => Key::LabelAuditSessionEnd,
            },
        )
    }
}

/// 一条审计记录。
///
/// **刻意不包含**：逐主机明细、MAC、厂商、反查出的主机名、任何文件内容。
/// 审计要能自证「我扫了什么、扫了多少、结果如何」，而不是变成一份资产清单
/// ——那正是审计日志最容易越界的地方。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AuditRecord {
    /// 事件时刻（Unix 毫秒）。
    pub at_ms: u64,
    pub event: Option<AuditEvent>,
    pub task_id: String,
    /// 发起者：操作系统的登录用户名。取不到就留空——**不猜、不填占位文本**。
    pub operator: String,
    /// 本机主机名。取不到同样留空。
    pub machine: String,
    /// 轮次序号（`SessionStart` / `SessionEnd` 为 0）。
    pub round: u64,
    /// 目标规格摘要（如 `192.168.1.0/24`），不是展开后的地址列表。
    pub target_spec: String,
    pub targets: usize,
    pub online: usize,
    pub timeout: usize,
    pub unreachable: usize,
    pub permission_denied: usize,
    pub unsupported: usize,
    pub error: usize,
    pub elapsed_ms: u64,
    pub mode: String,
    pub range: String,
    pub timeout_ms: u64,
    pub concurrency: usize,
    pub max_pps: u32,
    /// 使用的配置文件（`--config`），没有就是空串。
    pub config_path: String,
    /// 使用的模板名，没有就是空串。
    pub profile: String,
    /// 退出码（仅 `SessionEnd`）。
    pub exit_code: Option<i32>,
}

impl AuditRecord {
    pub fn new(at_ms: u64, event: AuditEvent) -> Self {
        Self {
            at_ms,
            event: Some(event),
            ..Self::default()
        }
    }

    /// 渲染成一行 JSON（不含换行符）。
    ///
    /// 用核心自己的 JSON 实现而不是手搓字符串：目标规格里可能有引号或反斜杠，
    /// 手工拼 JSON 迟早会拼出一个坏文件；而审计文件坏了就等于没有审计。
    pub fn to_json_line(&self) -> String {
        let event = self.event.unwrap_or(AuditEvent::SessionStart);
        let mut pairs: Vec<(&str, Json)> = vec![
            ("at", Json::UInt(self.at_ms)),
            ("event", Json::str(event.key())),
            ("eventLabel", Json::str(event.label_zh())),
            ("task", Json::str(self.task_id.clone())),
            ("operator", Json::str(self.operator.clone())),
            ("machine", Json::str(self.machine.clone())),
            ("round", Json::UInt(self.round)),
            ("targetSpec", Json::str(self.target_spec.clone())),
            ("targets", Json::UInt(self.targets as u64)),
            ("online", Json::UInt(self.online as u64)),
            ("timeout", Json::UInt(self.timeout as u64)),
            ("unreachable", Json::UInt(self.unreachable as u64)),
            (
                "permissionDenied",
                Json::UInt(self.permission_denied as u64),
            ),
            ("unsupported", Json::UInt(self.unsupported as u64)),
            ("error", Json::UInt(self.error as u64)),
            ("elapsedMs", Json::UInt(self.elapsed_ms)),
            ("mode", Json::str(self.mode.clone())),
            ("range", Json::str(self.range.clone())),
            ("timeoutMs", Json::UInt(self.timeout_ms)),
            ("concurrency", Json::UInt(self.concurrency as u64)),
            ("maxPps", Json::UInt(u64::from(self.max_pps))),
            ("configPath", Json::str(self.config_path.clone())),
            ("profile", Json::str(self.profile.clone())),
        ];
        if let Some(code) = self.exit_code {
            pairs.push(("exitCode", Json::Int(code as i64)));
        }
        Json::object(pairs).render()
    }
}

/// 审计日志写入器（追加写，每行一条 JSON）。
///
/// 追加而不是覆盖：审计的价值在于「连续、不可悄悄改」。写入失败会作为
/// `Err` 返回给调用方——**审计写不进去必须让人知道**，安静地丢审计
/// 比没有审计更糟。
#[derive(Debug)]
pub struct AuditLog {
    path: PathBuf,
    written: usize,
}

impl AuditLog {
    /// 打开审计文件。文件**不存在**时写入表头（`CPING-AUDIT <版本>`）；
    /// 存在但不是本程序写的审计文件时**拒绝追加**——往别人的文件里塞东西
    /// 比写不进去危险得多。
    pub fn open(path: impl Into<PathBuf>) -> CoreResult<Self> {
        let path = path.into();
        let text_path = path.to_string_lossy().to_string();
        if path.exists() {
            let head = std::fs::read_to_string(&path).map_err(|e| {
                CoreError::InvalidConfig(format!("读取审计文件 {text_path} 失败：{e}"))
            })?;
            let first = head.lines().next().unwrap_or("");
            if !first.starts_with(AUDIT_FORMAT) {
                return Err(CoreError::InvalidConfig(format!(
                    "{text_path} 不是 CPing 审计文件（首行应为 `{AUDIT_FORMAT} <版本>`），\
                     拒绝追加——请换一个文件名"
                )));
            }
            Ok(Self { path, written: 0 })
        } else {
            let head = format!("{AUDIT_FORMAT} {AUDIT_FORMAT_VERSION}\n");
            std::fs::write(&path, head).map_err(|e| {
                CoreError::InvalidConfig(format!("创建审计文件 {text_path} 失败：{e}"))
            })?;
            Ok(Self { path, written: 0 })
        }
    }

    /// 追加一条记录。
    pub fn append(&mut self, record: &AuditRecord) -> CoreResult<()> {
        use std::io::Write;
        let text_path = self.path.to_string_lossy().to_string();
        let mut line = record.to_json_line();
        line.push('\n');
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&self.path)
            .map_err(|e| CoreError::InvalidConfig(format!("打开审计文件 {text_path} 失败：{e}")))?;
        f.write_all(line.as_bytes())
            .map_err(|e| CoreError::InvalidConfig(format!("写入审计文件 {text_path} 失败：{e}")))?;
        self.written += 1;
        Ok(())
    }

    /// 已写入的记录条数。
    pub fn written(&self) -> usize {
        self.written
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

/// 当前登录用户名。取不到返回**空串**——不猜，也不用「未知用户」这类
/// 看着像真名的占位符污染审计记录。
pub fn operator_name() -> String {
    for key in ["CPING_OPERATOR", "USERNAME", "USER", "LOGNAME"] {
        if let Ok(v) = std::env::var(key) {
            let v = v.trim().to_string();
            if !v.is_empty() {
                return v;
            }
        }
    }
    String::new()
}

/// 本机主机名。取不到返回空串（同 [`operator_name`] 的理由）。
pub fn machine_name() -> String {
    if let Ok(v) = std::env::var("COMPUTERNAME") {
        let v = v.trim().to_string();
        if !v.is_empty() {
            return v;
        }
    }
    if let Ok(v) = std::env::var("HOSTNAME") {
        let v = v.trim().to_string();
        if !v.is_empty() {
            return v;
        }
    }
    String::new()
}

// ---------------------------------------------------------------------------
// 首次启动的授权确认
// ---------------------------------------------------------------------------

/// 「仅用于获授权网络」的确认状态。
pub mod consent {
    use super::*;

    /// 确认状态文件内容（极简 `key = value`，与项目其它落盘格式一致的手写风格）。
    #[derive(Debug, Clone, PartialEq, Eq, Default)]
    pub struct ConsentState {
        /// 已确认的版本号；`0` 表示从未确认过。
        pub version: u32,
        /// 首次确认的时刻（Unix 毫秒）。
        pub at_ms: u64,
    }

    impl ConsentState {
        /// 是否需要向用户展示授权提示。
        ///
        /// 判据是**版本号**而不是「文件存在」：将来提示文案变了就把
        /// [`CONSENT_VERSION`] 加一，用户会重新看到一次新文案——这比
        /// 「文件在就永远不再提」更负责。
        pub fn needs_notice(&self) -> bool {
            self.version < CONSENT_VERSION
        }
    }

    /// 确认状态文件路径：`<家目录>/.cping/consent`。
    ///
    /// 拿不到家目录时返回 `None`。调用方**不应该**因此改写到当前目录
    /// ——在用户的仓库里凭空多出一个 `.cping/` 比少提示一次更讨厌。
    pub fn consent_path() -> Option<PathBuf> {
        let home = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)?;
        if home.as_os_str().is_empty() {
            return None;
        }
        Some(home.join(".cping").join("consent"))
    }

    /// 读取确认状态。**文件不存在 = 从未确认过**（首次运行），不是错误。
    pub fn load() -> ConsentState {
        let Some(path) = consent_path() else {
            return ConsentState::default();
        };
        let Ok(text) = std::fs::read_to_string(path) else {
            return ConsentState::default();
        };
        let mut state = ConsentState::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            match k.trim() {
                "consent_version" => state.version = v.trim().parse().unwrap_or(0),
                "at_ms" => state.at_ms = v.trim().parse().unwrap_or(0),
                _ => {}
            }
        }
        state
    }

    /// 记录「用户已被告知」。
    ///
    /// 写失败**不作为错误上报**：这只是个提示状态，写不进去最多是下次再提示
    /// 一遍，为此拦住一次扫描是小题大做。调用方拿到 `false` 可以顺手提一句。
    pub fn acknowledge(at_ms: u64) -> bool {
        let Some(path) = consent_path() else {
            return false;
        };
        if let Some(dir) = path.parent() {
            if std::fs::create_dir_all(dir).is_err() {
                return false;
            }
        }
        let text = format!(
            "# CPing 授权确认状态（M17）\n\
             # 这个文件只记录「用户已读过一次授权提示」，不含任何个人信息。\n\
             # 删掉它就会在下次启动时重新提示一遍。\n\
             consent_version = {CONSENT_VERSION}\n\
             at_ms = {at_ms}\n"
        );
        std::fs::write(path, text).is_ok()
    }

    /// 「仅用于获授权网络」的提示正文（中文，三端共用同一份文案）。
    ///
    /// M18 起支持英文；这里保留中文签名是为了让既有调用点不用改。
    pub fn notice_text() -> String {
        notice_text_in(Lang::Zh)
    }

    /// 提示正文，按 `lang` 渲染。
    ///
    /// 原来文案里的 `**加粗**` 标记在 M18 去掉了：终端里它是**字面星号**，
    /// 只会让这几句话更难读。要强调的部分改由界面自己排版。
    pub fn notice_text_in(lang: Lang) -> String {
        notice_lines_in(lang).join("\n")
    }

    /// 按行取提示正文，便于界面自行排版（TUI 的弹窗需要按行绘制）。
    pub fn notice_lines_in(lang: Lang) -> Vec<&'static str> {
        [
            Key::ConsentLine1,
            Key::ConsentLine2,
            Key::ConsentLine3,
            Key::ConsentLine4,
        ]
        .into_iter()
        .map(|k| i18n::text(lang, k))
        .collect()
    }
}

// ---------------------------------------------------------------------------
// JSON 转义的自查（防止有人把手写拼接引回来）
// ---------------------------------------------------------------------------

/// 供审计使用的一行 JSON 是否可安全逐行读取（不含裸换行）。
pub fn line_is_single(text: &str) -> bool {
    !text.contains('\n') && !text.contains('\r')
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    fn v4(a: u8, b: u8, c: u8, d: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(a, b, c, d))
    }

    fn v6(text: &str) -> IpAddr {
        IpAddr::V6(text.parse::<Ipv6Addr>().unwrap())
    }

    // ---------------- 保留网段 ----------------

    #[test]
    fn private_and_loopback_are_deliberately_allowed() {
        // 这三类正是本工具的正常用途，拦掉等于把工具废掉
        for ip in [
            v4(10, 0, 0, 1),
            v4(172, 16, 5, 1),
            v4(192, 168, 1, 1),
            v4(127, 0, 0, 1),
            v4(100, 64, 0, 1),  // CGNAT，仍然是一台真实主机
            v4(169, 254, 1, 1), // 链路本地，APIPA 主机是真实存在的
            v6("::1"),
            v6("fe80::1"),
            v6("fd00::1"),      // ULA
            v6("2400:3200::1"), // 公网 v6
        ] {
            assert!(
                reserved_reason(ip).is_none(),
                "{ip} 不该被拦下（{:?}）",
                reserved_reason(ip)
            );
        }
    }

    #[test]
    fn multicast_broadcast_and_reserved_are_rejected_with_a_reason() {
        for (ip, needle) in [
            (v4(224, 0, 0, 1), "组播"),
            (v4(239, 255, 255, 255), "组播"),
            (v4(255, 255, 255, 255), "广播"),
            (v4(240, 0, 0, 1), "保留段"),
            (v4(0, 0, 0, 0), "本网络"),
            (v4(0, 1, 2, 3), "本网络"),
            (v4(192, 0, 2, 1), "TEST-NET-1"),
            (v4(198, 51, 100, 1), "TEST-NET-2"),
            (v4(203, 0, 113, 1), "TEST-NET-3"),
            (v4(198, 18, 0, 1), "基准测试"),
            (v4(198, 19, 255, 1), "基准测试"),
            (v6("::"), "未指定"),
            (v6("ff02::1"), "组播"),
            (v6("2001:db8::1"), "2001:db8"),
        ] {
            let reason = reserved_reason(ip).unwrap_or_else(|| panic!("{ip} 应被拦下"));
            assert!(
                reason.contains(needle),
                "{ip} 的原因 {reason:?} 不含 {needle:?}"
            );
        }
    }

    #[test]
    fn reserved_boundaries_do_not_bleed() {
        // 223/8 不是组播、239 是组播的最后一格、240 是保留段的第一格
        assert!(reserved_reason(v4(223, 255, 255, 255)).is_none());
        assert!(reserved_reason(v4(224, 0, 0, 0)).is_some());
        assert!(reserved_reason(v4(239, 255, 255, 255)).is_some());
        assert!(reserved_reason(v4(240, 0, 0, 0)).is_some());
        // 198.18/15 的两端都命中，198.20 已经出了这段
        assert!(reserved_reason(v4(198, 18, 0, 0)).is_some());
        assert!(reserved_reason(v4(198, 19, 255, 255)).is_some());
        assert!(reserved_reason(v4(198, 20, 0, 0)).is_none());
        // 192.0.2/24 只拦这一段，192.0.3 不拦
        assert!(reserved_reason(v4(192, 0, 2, 255)).is_some());
        assert!(reserved_reason(v4(192, 0, 3, 0)).is_none());
        // IPv6：fe00::/8 里只有 ff00::/8 是组播
        assert!(reserved_reason(v6("fec0::1")).is_none());
        assert!(reserved_reason(v6("ff00::1")).is_some());
        assert!(reserved_reason(v6("2001:db9::1")).is_none());
    }

    // ---------------- 策略校验 ----------------

    #[test]
    fn policy_defaults_are_the_documented_ones() {
        let p = ScanPolicy::default();
        assert_eq!(p.max_pps, DEFAULT_MAX_PPS);
        assert!(p.max_pps > 0, "默认必须限速，不能靠用户自觉");
        assert_eq!(p.max_targets, MAX_TARGETS);
        assert!(!p.allow_reserved, "默认必须拦保留网段");
        assert!(p.validate().is_ok());
    }

    #[test]
    fn policy_rejects_out_of_range_values_instead_of_clamping() {
        let too_fast = ScanPolicy {
            max_pps: MAX_PPS_LIMIT + 1,
            ..ScanPolicy::default()
        };
        assert!(too_fast
            .validate()
            .unwrap_err()
            .to_string()
            .contains("关闭限速"));

        let zero_targets = ScanPolicy {
            max_targets: 0,
            ..ScanPolicy::default()
        };
        assert!(zero_targets.validate().is_err());

        // 只能收紧，不能放宽
        let too_many = ScanPolicy {
            max_targets: MAX_TARGETS + 1,
            ..ScanPolicy::default()
        };
        let err = too_many.validate().unwrap_err().to_string();
        assert!(err.contains("只能收紧"), "{err}");
    }

    #[test]
    fn check_targets_enforces_count_and_reserved() {
        let p = ScanPolicy {
            max_targets: 2,
            ..ScanPolicy::default()
        };
        assert!(p.check_targets(&[v4(10, 0, 0, 1), v4(10, 0, 0, 2)]).is_ok());
        let err = p
            .check_targets(&[v4(10, 0, 0, 1), v4(10, 0, 0, 2), v4(10, 0, 0, 3)])
            .unwrap_err();
        assert!(matches!(
            err,
            CoreError::TooManyTargets {
                actual: 3,
                limit: 2
            }
        ));

        // 数量先于网段判定：数量错的时候说数量
        let err = p.check_targets(&[v4(224, 0, 0, 1); 5]).unwrap_err();
        assert!(matches!(err, CoreError::TooManyTargets { .. }));

        let err = p
            .check_targets(&[v4(10, 0, 0, 1), v4(224, 0, 0, 1)])
            .unwrap_err();
        match err {
            CoreError::ReservedTarget { ip, reason } => {
                assert_eq!(ip, "224.0.0.1");
                assert!(reason.contains("组播"), "{reason}");
            }
            other => panic!("应为保留网段错误：{other:?}"),
        }

        // 显式放行之后就不再拦
        let open = ScanPolicy {
            allow_reserved: true,
            ..p
        };
        assert!(open
            .check_targets(&[v4(10, 0, 0, 1), v4(224, 0, 0, 1)])
            .is_ok());
    }

    #[test]
    fn only_the_first_reserved_target_is_reported() {
        // 列 255 条只会把真正的那一句淹掉
        let p = ScanPolicy::default();
        let err = p
            .check_targets(&[v4(224, 0, 0, 1), v4(224, 0, 0, 2), v4(224, 0, 0, 3)])
            .unwrap_err();
        match err {
            CoreError::ReservedTarget { ip, .. } => assert_eq!(ip, "224.0.0.1"),
            other => panic!("{other:?}"),
        }
    }

    // ---------------- 令牌桶 ----------------

    #[test]
    fn bucket_allows_a_full_burst_then_paces() {
        let mut b = Bucket::full(4);
        assert_eq!(b.tokens(), 4, "满桶 = 一次瞬时突发");
        for i in 0..4 {
            assert_eq!(b.take(0, 4), None, "第 {i} 个突发令牌应立刻拿到");
        }
        // 第 5 个要等：还差 1000 千分令牌，4 包/秒 → 250 毫秒
        assert_eq!(b.take(0, 4), Some(250));
    }

    #[test]
    fn bucket_refills_proportionally_to_elapsed_time() {
        let mut b = Bucket::full(4);
        for _ in 0..4 {
            assert_eq!(b.take(0, 4), None);
        }
        // 过了 250 毫秒 → 攒够 1 个
        assert_eq!(b.take(250, 4), None);
        // 再立刻要就没了
        assert_eq!(b.take(250, 4), Some(250));
        // 过 1 秒 → 满桶（而不是 4 个 + 累积到 8 个）
        assert_eq!(b.take(1250, 4), None);
        for _ in 0..3 {
            assert_eq!(b.take(1250, 4), None);
        }
        assert_eq!(b.take(1250, 4), Some(250), "桶容量上限就是 1 秒的量");
    }

    #[test]
    fn bucket_never_exceeds_the_rate_over_a_long_run() {
        // 600 包/秒跑 1 秒，最多发出 600 + 突发的 600 = 1200
        let rate = 600u32;
        let mut b = Bucket::full(rate);
        let mut sent = 0u32;
        let cap = rate as u64 * 1000;
        let mut tokens_used = 0u64;
        for ms in 0..=1000u64 {
            while b.take(ms, rate).is_none() {
                sent += 1;
                tokens_used += 1000;
            }
        }
        // 用「桶内已用掉的令牌」反推，避免浮点与舍入影响断言
        let bucket_capacity = cap / 1000;
        assert!(
            sent as u64 <= rate as u64 + bucket_capacity,
            "1 秒里发出了 {sent} 个，超过「速率 + 突发容量」{}",
            rate as u64 + bucket_capacity
        );
        assert!(tokens_used > 0);
        assert!(sent >= rate, "不该比速率还慢：{sent}");
    }

    #[test]
    fn bucket_wait_is_rounded_up_never_down() {
        // 3 包/秒：缺 1000 千分令牌时向上取整为 334 毫秒（≈3 包/秒，不会更快）
        let mut b = Bucket::full(3);
        for _ in 0..3 {
            assert_eq!(b.take(0, 3), None);
        }
        assert_eq!(b.take(0, 3), Some(334));
    }

    #[test]
    fn bucket_with_zero_rate_never_blocks() {
        let mut b = Bucket::full(0);
        for _ in 0..1000 {
            assert_eq!(b.take(0, 0), None);
        }
    }

    #[test]
    fn limiter_is_a_no_op_when_unlimited() {
        let limiter = RateLimiter::new(0);
        assert!(limiter.is_unlimited());
        let start = Instant::now();
        for _ in 0..5000 {
            limiter.wait(|| false);
        }
        assert!(start.elapsed().as_millis() < 200, "不限速时不该有任何等待");
    }

    #[test]
    fn limiter_can_be_cancelled_while_waiting() {
        // 限速绝不能变成「取消不掉的等待」
        let limiter = RateLimiter::new(1);
        limiter.wait(|| false); // 用掉突发
        let mut asked = 0;
        let start = Instant::now();
        limiter.wait(|| {
            asked += 1;
            asked > 2 // 第三次询问时表示「已取消」
        });
        assert!(start.elapsed().as_millis() < 500, "取消后应立即返回");
    }

    // ---------------- 审计 ----------------

    #[test]
    fn audit_line_is_single_line_json_with_the_fields_we_promised() {
        let mut r = AuditRecord::new(1_700_000_000_000, AuditEvent::Round);
        r.task_id = "t1-1".into();
        r.operator = "alice".into();
        r.machine = "workstation".into();
        r.round = 3;
        r.target_spec = "192.168.1.0/24".into();
        r.targets = 254;
        r.online = 250;
        r.timeout = 4;
        r.mode = "ICMP".into();
        r.range = "hosts_only".into();
        r.timeout_ms = 1000;
        r.concurrency = 32;
        r.max_pps = 500;
        let line = r.to_json_line();
        assert!(line_is_single(&line), "必须是单行：{line}");
        assert!(line.starts_with('{') && line.ends_with('}'), "{line}");
        for key in [
            "\"at\":1700000000000",
            "\"event\":\"round\"",
            "\"operator\":\"alice\"",
            "\"machine\":\"workstation\"",
            "\"round\":3",
            "\"targets\":254",
            "\"maxPps\":500",
        ] {
            assert!(line.contains(key), "缺少 {key}：{line}");
        }
        // SessionEnd 才有退出码；其它事件不该出现这个键
        assert!(!line.contains("exitCode"), "{line}");
        r.event = Some(AuditEvent::SessionEnd);
        r.exit_code = Some(1);
        assert!(r.to_json_line().contains("\"exitCode\":1"));
    }

    #[test]
    fn audit_line_escapes_hostile_target_specs() {
        // 目标规格来自命令行，可能带引号/反斜杠/换行
        let mut r = AuditRecord::new(1, AuditEvent::SessionStart);
        r.target_spec = "10.0.0.1\"\\\n,evil".into();
        let line = r.to_json_line();
        assert!(line_is_single(&line), "换行必须被转义掉：{line:?}");
        // 换行必须是转义形式，而不是真的换行——否则逐行读会读到半条记录
        assert!(line.contains("\\n"), "{line}");
        assert!(line.contains("\\\""), "{line}");
        assert!(line.contains("\\\\"), "{line}");
        // 核心没有 JSON 解析器（只渲染），能守的底线是「配平且不含裸控制字符」
        assert!(crate::json::is_balanced(&line), "{line}");
        assert!(!line.chars().any(|c| (c as u32) < 0x20), "{line}");
    }

    #[test]
    fn audit_log_refuses_a_file_it_did_not_write() {
        // 目录名必须带上进程号：固定名会让「同一台机器上并行跑两份测试」
        // （本地并行、或 CI 上同一 job 的两次调用）共用同一个目录，
        // 一方在结尾删文件、另一方正好在读，就会随机失败。
        // `snapshot.rs` 的临时目录早就带了 `process::id()`，此处对齐。
        let dir =
            std::env::temp_dir().join(format!("cping_policy_audit_test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let foreign = dir.join("foreign.log");
        std::fs::write(&foreign, "some other tool's log\n").unwrap();
        let err = AuditLog::open(&foreign).unwrap_err().to_string();
        assert!(err.contains("不是 CPing 审计文件"), "{err}");

        let own = dir.join("own.jsonl");
        let _ = std::fs::remove_file(&own);
        let mut log = AuditLog::open(&own).unwrap();
        assert_eq!(log.written(), 0);
        log.append(&AuditRecord::new(1, AuditEvent::SessionStart))
            .unwrap();
        log.append(&AuditRecord::new(2, AuditEvent::SessionEnd))
            .unwrap();
        assert_eq!(log.written(), 2);
        let text = std::fs::read_to_string(&own).unwrap();
        let mut lines = text.lines();
        assert_eq!(lines.next(), Some("CPING-AUDIT 1"));
        assert_eq!(lines.count(), 2);

        // 重新打开（追加模式）不会重写表头，也不会丢已有记录
        let mut again = AuditLog::open(&own).unwrap();
        again
            .append(&AuditRecord::new(3, AuditEvent::Round))
            .unwrap();
        let text = std::fs::read_to_string(&own).unwrap();
        assert_eq!(text.matches("CPING-AUDIT").count(), 1);
        assert_eq!(text.lines().count(), 4);
        let _ = std::fs::remove_file(&own);
        let _ = std::fs::remove_file(&foreign);
    }

    // ---------------- 授权确认 ----------------

    #[test]
    fn consent_needs_notice_until_the_version_is_reached() {
        let fresh = consent::ConsentState::default();
        assert!(fresh.needs_notice(), "首次运行必须提示");
        let done = consent::ConsentState {
            version: CONSENT_VERSION,
            at_ms: 1,
        };
        assert!(!done.needs_notice());
        // 旧版本的确认记录要重新提示一次（文案可能已经变了）
        let old = consent::ConsentState {
            version: CONSENT_VERSION.saturating_sub(1),
            at_ms: 1,
        };
        assert!(old.needs_notice());
    }

    #[test]
    fn consent_notice_text_says_what_it_must_say() {
        let text = consent::notice_text();
        for needle in ["授权", "违法", "速率上限"] {
            assert!(text.contains(needle), "提示文案缺少 {needle}：{text}");
        }
    }

    #[test]
    fn policy_describe_mentions_the_caps() {
        let text = ScanPolicy::default().describe();
        assert!(text.contains("500 包/秒"), "{text}");
        assert!(text.contains(&MAX_TARGETS.to_string()), "{text}");
        let open = ScanPolicy {
            max_pps: 0,
            allow_reserved: true,
            ..ScanPolicy::default()
        };
        let text = open.describe();
        assert!(text.contains("不限速"), "{text}");
        assert!(text.contains("允许保留网段"), "{text}");
    }

    // -----------------------------------------------------------------------
    // M18：本地化
    // -----------------------------------------------------------------------

    #[test]
    fn reserved_kinds_have_unique_codes_and_are_all_reachable() {
        // 每个类别都要能被某个真实地址命中——「定义了却判不出来」的类别
        // 意味着某条规则的位置写错了。
        let samples = [
            ("224.0.0.1", ReservedKind::MulticastV4),
            ("240.0.0.1", ReservedKind::ReservedRangeV4),
            ("0.1.2.3", ReservedKind::ThisNetwork),
            ("192.0.2.7", ReservedKind::TestNet1),
            ("198.51.100.7", ReservedKind::TestNet2),
            ("203.0.113.7", ReservedKind::TestNet3),
            ("198.18.0.7", ReservedKind::BenchmarkNet),
            ("::", ReservedKind::UnspecifiedV6),
            ("ff02::1", ReservedKind::MulticastV6),
            ("2001:db8::1", ReservedKind::DocNetV6),
        ];
        assert_eq!(samples.len(), ALL_RESERVED_KINDS.len());
        let mut codes = std::collections::HashSet::new();
        for (addr, want) in samples {
            let ip: IpAddr = addr.parse().unwrap();
            assert_eq!(reserved_kind(ip), Some(want), "{addr}");
            assert!(codes.insert(want.code()), "{:?} 的 code 重复", want);
        }
        assert_eq!(codes.len(), ALL_RESERVED_KINDS.len());
    }

    #[test]
    fn reserved_reason_is_the_chinese_rendering() {
        // 兼容入口必须与显式中文渲染逐字一致。
        for addr in ["224.0.0.1", "192.0.2.7", "2001:db8::1"] {
            let ip: IpAddr = addr.parse().unwrap();
            let kind = reserved_kind(ip).unwrap();
            assert_eq!(reserved_reason(ip), Some(kind.reason(Lang::Zh)));
        }
    }

    #[test]
    fn reserved_reasons_are_english_when_asked() {
        for kind in ALL_RESERVED_KINDS {
            let zh = kind.reason(Lang::Zh);
            let en = kind.reason(Lang::En);
            assert_ne!(zh, en, "{:?} 的中英原因相同，说明漏译了", kind);
            assert!(en.is_ascii(), "{:?} 的英文原因含非 ASCII 字符：{en}", kind);
        }
    }

    #[test]
    fn policy_validation_messages_are_localized() {
        let bad_pps = ScanPolicy {
            max_pps: MAX_PPS_LIMIT + 1,
            ..ScanPolicy::default()
        };
        let e = bad_pps.validate_in(Lang::En).unwrap_err();
        let s = e.localized(Lang::En);
        assert!(s.contains("rate limit"), "{s}");
        assert!(s.contains(&(MAX_PPS_LIMIT + 1).to_string()), "{s}");

        let zero = ScanPolicy {
            max_targets: 0,
            ..ScanPolicy::default()
        };
        let s = zero.validate_in(Lang::En).unwrap_err().localized(Lang::En);
        assert!(s.contains("greater than 0"), "{s}");

        let too_big = ScanPolicy {
            max_targets: MAX_TARGETS + 1,
            ..ScanPolicy::default()
        };
        let s = too_big
            .validate_in(Lang::En)
            .unwrap_err()
            .localized(Lang::En);
        assert!(s.contains("structural maximum"), "{s}");

        // 中文路径逐字不变（M17 的行为不能被 M18 改掉）。
        assert!(bad_pps
            .validate()
            .unwrap_err()
            .to_string()
            .contains("速率上限"));
    }

    #[test]
    fn reserved_target_error_follows_the_requested_language() {
        let targets: Vec<IpAddr> = vec!["224.0.0.1".parse().unwrap()];
        let e = ScanPolicy::default()
            .check_targets_in(&targets, Lang::En)
            .unwrap_err();
        assert_eq!(e.code(), "reserved_target");
        let s = e.localized(Lang::En);
        assert!(
            s.starts_with("target 224.0.0.1 is in a reserved range:"),
            "{s}"
        );
        assert!(!s.contains("组播"), "{s}");

        // 中文调用点（`check_targets`）仍然给出中文原因。
        let e = ScanPolicy::default().check_targets(&targets).unwrap_err();
        assert!(e.to_string().contains("组播"), "{e}");
    }

    #[test]
    fn policy_describe_is_localized() {
        let p = ScanPolicy::default();
        assert_eq!(p.describe_in(Lang::Zh), p.describe());
        let en = p.describe_in(Lang::En);
        assert!(en.contains("rate limit 500 pps"), "{en}");
        assert!(en.contains("target limit 65536"), "{en}");
        assert!(!en.contains("不限速"), "{en}");

        let open = ScanPolicy {
            max_pps: 0,
            allow_reserved: true,
            ..ScanPolicy::default()
        };
        let en = open.describe_in(Lang::En);
        assert!(en.contains("unlimited"), "{en}");
        assert!(en.contains("reserved ranges allowed"), "{en}");
    }

    #[test]
    fn consent_notice_is_localized_line_by_line() {
        assert_eq!(consent::notice_text(), consent::notice_text_in(Lang::Zh));
        let zh = consent::notice_lines_in(Lang::Zh);
        let en = consent::notice_lines_in(Lang::En);
        assert_eq!(zh.len(), 4);
        assert_eq!(en.len(), 4);
        for (a, b) in zh.iter().zip(en.iter()) {
            assert_ne!(a, b, "有一行没有翻译：{a}");
            assert!(b.is_ascii(), "英文提示含非 ASCII 字符：{b}");
        }
        // 英文版也必须说清「授权」「违法」「限速」这三件事。
        let joined = en.join(" ");
        for needle in ["authorised", "illegal", "rate limit"] {
            assert!(joined.contains(needle), "英文提示缺少 {needle}：{joined}");
        }
    }
}
