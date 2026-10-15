//! 阈值告警（M15，v2.3.0）。
//!
//! 与前两个阶段（M13 指纹、M14 时序）同一套纪律：**规则匹配是纯函数**，
//! 冷却去重、静音、状态落盘都是外层的簿记。这样「什么情况下该报」这件事
//! 可以逐条边界测，而不用去构造一个完整的扫描流程。
//!
//! ## 四类规则与它们的触发面
//!
//! | 规则 | 触发条件 | 性质 |
//! | --- | --- | --- |
//! | [`AlertKind::Appeared`] | 第一次见到这台主机，且它在线 | 边沿 |
//! | [`AlertKind::Disappeared`] | 上一轮在线、本轮离线（且状态已定） | 边沿 |
//! | [`AlertKind::LatencyAbove`] | 在线且 `rtt_ms > threshold` | 电平 |
//! | [`AlertKind::UptimeBelow`] | `可用率% < threshold` | 电平 |
//!
//! ## 三条刻意定死的边界
//!
//! 1. **第一轮不发 `Appeared`**。否则扫一个 /24 会一次性报 255 条「新增主机」，
//!    把真正的告警淹掉。第一轮只建立基线（`primed`）。
//! 2. **阈值用严格不等号**：`LatencyAbove` 是「超过」（`>`），`UptimeBelow` 是
//!    「低于」（`<`）。恰好等于阈值**不告警**——这条写死并在测试里钉住，
//!    免得日后有人「顺手」改成 `>=` 而没人发现。
//! 3. **静音不改变状态**：静音期间规则照常求值、`seen`/`last_fired` 照常推进，
//!    只是事件不返回。这样取消静音后不会因为「冷却还没过」而漏报一堆。
//!
//! ## 为什么 `Disappeared` 要求 `is_final()`
//!
//! 「离线」有多个来源（超时 / 不可达 / 权限不足 / 不支持 / 错误），
//! 但**未探测完**（`Pending`）不是其中之一。扫描进行到一半时不能报警，
//! 否则每轮开头都会刷出一片「主机离线」。

use crate::classify::Thresholds;
use crate::i18n::{self, Key, Lang};
use crate::result::{ProbeResult, ProbeStatus};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::net::IpAddr;

/// 告警状态文件签名。
pub const ALERT_STATE_SIGNATURE: &str = "CPING-ALERTS";
/// 告警状态文件格式版本。
pub const ALERT_STATE_FORMAT_VERSION: u32 = 1;
/// 告警事件环形缓冲的默认容量。
pub const DEFAULT_ALERT_CAPACITY: usize = 512;

/// 告警类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AlertKind {
    /// 第一次观测到这台主机且它在线。
    Appeared,
    /// 上一轮在线、本轮离线。
    Disappeared,
    /// 延迟超过阈值。
    LatencyAbove,
    /// 累计可用率低于阈值。
    UptimeBelow,
}

impl AlertKind {
    pub const ALL: [AlertKind; 4] = [
        AlertKind::Appeared,
        AlertKind::Disappeared,
        AlertKind::LatencyAbove,
        AlertKind::UptimeBelow,
    ];

    /// 稳定的机器可读键（落盘与 DTO 都用它）。
    pub fn key(&self) -> &'static str {
        match self {
            AlertKind::Appeared => "appeared",
            AlertKind::Disappeared => "disappeared",
            AlertKind::LatencyAbove => "latency_above",
            AlertKind::UptimeBelow => "uptime_below",
        }
    }

    /// 中文标签（等价于 `label_in(Lang::Zh)`）。
    ///
    /// 核心层内部的报表与导出仍在用它——**产物内容是数据契约**：
    /// 同一份 CSV / HTML 在中英文机器上必须一致；只有界面跟着语言变。
    pub fn label_zh(&self) -> &'static str {
        self.label_in(Lang::Zh)
    }

    /// 按语言渲染的标签。
    pub fn label_in(&self, lang: Lang) -> &'static str {
        i18n::text(
            lang,
            match self {
                AlertKind::Appeared => Key::LabelAlertAppeared,
                AlertKind::Disappeared => Key::LabelAlertDisappeared,
                AlertKind::LatencyAbove => Key::LabelAlertLatencyAbove,
                AlertKind::UptimeBelow => Key::LabelAlertUptimeBelow,
            },
        )
    }

    /// CLI 里 `--alert-kinds` 接受的名字。
    pub fn parse_key(text: &str) -> Option<AlertKind> {
        match text.trim().to_ascii_lowercase().as_str() {
            "appeared" | "new" | "新增" => Some(AlertKind::Appeared),
            "disappeared" | "offline" | "离线" => Some(AlertKind::Disappeared),
            "latency" | "latency_above" | "延迟" => Some(AlertKind::LatencyAbove),
            "uptime" | "uptime_below" | "可用率" => Some(AlertKind::UptimeBelow),
            _ => None,
        }
    }

    /// 该规则是否用 `threshold`；不用的规则其阈值被忽略。
    pub fn uses_threshold(&self) -> bool {
        matches!(self, AlertKind::LatencyAbove | AlertKind::UptimeBelow)
    }
}

/// 告警严重度。刻意只有三档：多了没人按它分流。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warning,
    Critical,
}

impl Severity {
    pub fn key(&self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Critical => "critical",
        }
    }

    /// 中文标签（等价于 `label_in(Lang::Zh)`）。
    ///
    /// 核心层内部的报表与导出仍在用它——**产物内容是数据契约**：
    /// 同一份 CSV / HTML 在中英文机器上必须一致；只有界面跟着语言变。
    pub fn label_zh(&self) -> &'static str {
        self.label_in(Lang::Zh)
    }

    /// 按语言渲染的标签。
    pub fn label_in(&self, lang: Lang) -> &'static str {
        i18n::text(
            lang,
            match self {
                Severity::Info => Key::LabelSeverityInfo,
                Severity::Warning => Key::LabelSeverityWarning,
                Severity::Critical => Key::LabelSeverityCritical,
            },
        )
    }
}

/// 一条规则。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertRule {
    pub kind: AlertKind,
    pub enabled: bool,
    /// `LatencyAbove` 用毫秒；`UptimeBelow` 用百分数（0..=100）。其它规则忽略。
    pub threshold: u32,
    /// 冷却窗口（秒）：同一 `(类型, 主机)` 在这段时间内只报一次。
    pub cooldown_secs: u64,
}

impl AlertRule {
    pub fn new(kind: AlertKind, threshold: u32, cooldown_secs: u64) -> Self {
        Self {
            kind,
            enabled: true,
            threshold,
            cooldown_secs,
        }
    }
}

/// 默认延迟阈值（毫秒）。
pub const DEFAULT_LATENCY_THRESHOLD_MS: u32 = 200;
/// 默认可用率阈值（百分数）。
pub const DEFAULT_UPTIME_THRESHOLD_PCT: u32 = 90;
/// 默认冷却窗口（秒）。
pub const DEFAULT_COOLDOWN_SECS: u64 = 300;

/// 默认规则集：四类都开，阈值用上面的默认值。
pub fn default_rules() -> Vec<AlertRule> {
    vec![
        AlertRule::new(AlertKind::Appeared, 0, DEFAULT_COOLDOWN_SECS),
        AlertRule::new(AlertKind::Disappeared, 0, DEFAULT_COOLDOWN_SECS),
        AlertRule::new(
            AlertKind::LatencyAbove,
            DEFAULT_LATENCY_THRESHOLD_MS,
            DEFAULT_COOLDOWN_SECS,
        ),
        AlertRule::new(
            AlertKind::UptimeBelow,
            DEFAULT_UPTIME_THRESHOLD_PCT,
            DEFAULT_COOLDOWN_SECS,
        ),
    ]
}

/// 告警配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertConfig {
    pub rules: Vec<AlertRule>,
    /// 一键静音：求值照常，但事件不返回。
    pub muted: bool,
    /// 事件环形缓冲容量。
    pub capacity: usize,
}

impl Default for AlertConfig {
    fn default() -> Self {
        Self {
            rules: default_rules(),
            muted: false,
            capacity: DEFAULT_ALERT_CAPACITY,
        }
    }
}

impl AlertConfig {
    /// 只保留给定类型，其余规则置为 `enabled = false`（**不删除**：
    /// 保留阈值便于界面展示「这条规则被关了，阈值是多少」）。
    pub fn with_kinds(&mut self, kinds: &[AlertKind]) {
        for r in &mut self.rules {
            r.enabled = kinds.contains(&r.kind);
        }
    }

    pub fn rule(&self, kind: AlertKind) -> Option<&AlertRule> {
        self.rules.iter().find(|r| r.kind == kind)
    }

    pub fn rule_mut(&mut self, kind: AlertKind) -> Option<&mut AlertRule> {
        self.rules.iter_mut().find(|r| r.kind == kind)
    }
}

/// 一次规则求值所需的全部输入。
///
/// 刻意做成一个独立结构：规则匹配因此是纯函数 `(rule, ctx) -> Option<Event>`，
/// 不依赖引擎状态、不依赖时钟、不做 I/O。
#[derive(Debug, Clone, Copy)]
pub struct AlertContext<'a> {
    pub ip: IpAddr,
    pub index: u16,
    pub current: &'a ProbeResult,
    /// 本轮之前是否观测过这台主机；`None` = 从没见过。
    pub was_online: Option<bool>,
    /// 累计可用率（0.0..=1.0）；没有历史时为 `None`（此时不发 `UptimeBelow`）。
    pub uptime_ratio: Option<f64>,
    pub at_ms: u64,
    pub round: u64,
}

/// 一条告警事件。
#[derive(Debug, Clone, PartialEq)]
pub struct AlertEvent {
    pub kind: AlertKind,
    pub severity: Severity,
    pub ip: IpAddr,
    pub index: u16,
    pub at_ms: u64,
    pub round: u64,
    /// 触发时的实际值（延迟毫秒 / 可用率百分数）；边沿类规则为 `None`。
    pub value: Option<f64>,
    /// 触发时的阈值；边沿类规则为 `None`。
    pub threshold: Option<u32>,
    /// 一行可读摘要。
    pub message: String,
}

impl AlertEvent {
    /// `(类型, 主机)` —— 冷却与去重的键。
    pub fn dedup_key(&self) -> (AlertKind, IpAddr) {
        (self.kind, self.ip)
    }

    /// `2026-10-15 12:00:00` 形式的时间（UTC，与其余模块同口径）。
    pub fn time_text(&self) -> String {
        crate::format_unix_local(self.at_ms / 1000)
    }
}

/// **纯函数**：判定一条规则是否命中，命中则给出事件。
///
/// 不含冷却、不含静音、不读时钟——那些都是 [`AlertEngine`] 的职责。
pub fn match_rule(rule: &AlertRule, ctx: &AlertContext) -> Option<AlertEvent> {
    if !rule.enabled {
        return None;
    }
    let online = ctx.current.status.is_online();
    let note = ctx.current.status.note().unwrap_or("");

    let (severity, value, threshold, message) = match rule.kind {
        AlertKind::Appeared => {
            // 「新增」= 这一轮**第一次**见到它。第一轮的批量基线由引擎的 primed 抑制。
            if ctx.was_online.is_some() || !online {
                return None;
            }
            (
                Severity::Info,
                None,
                None,
                format!("新增主机 {} 上线", ctx.ip),
            )
        }
        AlertKind::Disappeared => {
            if ctx.was_online != Some(true) || online || !ctx.current.status.is_final() {
                return None;
            }
            (
                Severity::Critical,
                None,
                None,
                if note.is_empty() {
                    format!(
                        "主机 {} 变为离线（{}）",
                        ctx.ip,
                        ctx.current.status.label_zh()
                    )
                } else {
                    format!(
                        "主机 {} 变为离线（{}：{note}）",
                        ctx.ip,
                        ctx.current.status.label_zh()
                    )
                },
            )
        }
        AlertKind::LatencyAbove => {
            if !online {
                return None;
            }
            let rtt = ctx.current.status.rtt_ms().unwrap_or(0);
            // 严格大于：等于阈值不告警（见模块文档第 2 条）
            if u64::from(rtt) <= u64::from(rule.threshold) {
                return None;
            }
            (
                Severity::Warning,
                Some(f64::from(rtt)),
                Some(rule.threshold),
                format!(
                    "主机 {} 延迟 {rtt} ms，超过阈值 {} ms",
                    ctx.ip, rule.threshold
                ),
            )
        }
        AlertKind::UptimeBelow => {
            let ratio = ctx.uptime_ratio?;
            let pct = ratio * 100.0;
            // 严格小于：等于阈值不告警
            if pct >= f64::from(rule.threshold) {
                return None;
            }
            (
                Severity::Warning,
                Some(pct),
                Some(rule.threshold),
                format!(
                    "主机 {} 可用率 {pct:.1}%，低于阈值 {}%",
                    ctx.ip, rule.threshold
                ),
            )
        }
    };

    Some(AlertEvent {
        kind: rule.kind,
        severity,
        ip: ctx.ip,
        index: ctx.index,
        at_ms: ctx.at_ms,
        round: ctx.round,
        value,
        threshold,
        message,
    })
}

/// 告警事件的环形缓冲（供界面展示最近若干条）。
#[derive(Debug, Clone, PartialEq)]
pub struct AlertLog {
    items: VecDeque<AlertEvent>,
    capacity: usize,
    total_seen: u64,
}

impl AlertLog {
    pub fn new(capacity: usize) -> Self {
        Self {
            items: VecDeque::new(),
            capacity: capacity.max(1),
            total_seen: 0,
        }
    }

    pub fn push(&mut self, event: AlertEvent) {
        self.total_seen += 1;
        if self.items.len() == self.capacity {
            self.items.pop_front();
        }
        self.items.push_back(event);
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// 累计产生过的条数（含已被挤掉的）。
    pub fn total_seen(&self) -> u64 {
        self.total_seen
    }

    /// 最新在前。
    pub fn iter_recent_first(&self) -> impl Iterator<Item = &AlertEvent> {
        self.items.iter().rev()
    }

    /// 清空记录。
    ///
    /// **连 `total_seen` 一起归零**：界面上点了「清空」却还写着「累计 5 条」，
    /// 用户只会以为按钮坏了。需要「自启动以来的总量」应该另外记账，不是这里的事。
    pub fn clear(&mut self) {
        self.items.clear();
        self.total_seen = 0;
    }
}

/// 告警引擎：规则求值 + 冷却去重 + 静音 + 事件缓冲。
#[derive(Debug, Clone)]
pub struct AlertEngine {
    config: AlertConfig,
    log: AlertLog,
    /// 上一次真正上报某 `(类型, 主机)` 的时刻（毫秒）。
    last_fired: BTreeMap<(AlertKind, IpAddr), u64>,
    /// 见过的主机 → 上一轮是否在线。
    seen: BTreeMap<IpAddr, bool>,
    /// 是否已经完成过至少一次求值（第一轮不发 `Appeared`）。
    primed: bool,
    /// 被冷却挡下的事件数。
    suppressed_by_cooldown: u64,
    /// 被静音挡下的事件数。
    suppressed_by_mute: u64,
}

impl Default for AlertEngine {
    fn default() -> Self {
        Self::new(AlertConfig::default())
    }
}

impl AlertEngine {
    pub fn new(config: AlertConfig) -> Self {
        let capacity = config.capacity;
        Self {
            config,
            log: AlertLog::new(capacity),
            last_fired: BTreeMap::new(),
            seen: BTreeMap::new(),
            primed: false,
            suppressed_by_cooldown: 0,
            suppressed_by_mute: 0,
        }
    }

    pub fn config(&self) -> &AlertConfig {
        &self.config
    }

    pub fn set_muted(&mut self, muted: bool) {
        self.config.muted = muted;
    }

    /// 用一份新配置覆盖**规则与静音开关**，但保留冷却记录与基线。
    ///
    /// 用于「从状态文件载入冷却记录后，套用本次命令行的阈值」——状态文件里
    /// 的阈值是上次跑的时候的，拿它盖掉用户这次的参数只会让人莫名其妙。
    pub fn apply_config(&mut self, config: AlertConfig) {
        self.config.rules = config.rules;
        self.config.muted = config.muted;
        // capacity 只影响日志环形缓冲，随配置走即可（不重建 log，已有的条目留着）
        self.config.capacity = config.capacity.max(1);
    }

    pub fn is_muted(&self) -> bool {
        self.config.muted
    }

    pub fn log(&self) -> &AlertLog {
        &self.log
    }

    /// 可变的日志句柄——**只用来清空/裁剪**，不暴露给规则求值。
    ///
    /// 界面上的「清空告警记录」应当只丢事件、不动 `seen` 基线与冷却记录：
    /// 动了基线，下一轮就会把整段在线主机当成「新增」重新报一遍。
    pub fn log_mut(&mut self) -> &mut AlertLog {
        &mut self.log
    }

    pub fn suppressed_by_cooldown(&self) -> u64 {
        self.suppressed_by_cooldown
    }

    pub fn suppressed_by_mute(&self) -> u64 {
        self.suppressed_by_mute
    }

    /// 一次性建立基线：把一批结果记为「已见过」，不产生任何事件。
    ///
    /// 用于恢复快照后接着跑的场景——恢复出来的那批结果不该被当成「新增」。
    pub fn prime_with(&mut self, results: &[ProbeResult]) {
        for r in results {
            self.seen.insert(r.ip, r.status.is_online());
        }
        self.primed = true;
    }

    pub fn is_primed(&self) -> bool {
        self.primed
    }

    /// 求值一轮。
    ///
    /// - `previous`：上一轮结果（用于判断「上一轮是否在线」）。首轮传空切片。
    /// - `uptime`：各主机的累计可用率（0..=1）；由调用方从 [`crate::TimeSeries`]
    ///   一次性算好传进来，避免在规则里做 O(点数×主机数) 的重复扫描。
    /// - `_thresholds`：**不读**。它是分类用的低/中/高延迟档位，与告警阈值是两回事
    ///   ——告警阈值在 [`AlertRule::threshold`] 里。保留这个参数只是为了让三端的
    ///   调用点形状一致，别把它当成告警阈值的来源。
    ///
    /// 返回值已按严重度降序、同严重度按主机序号升序排好：**严重的在最前面**，
    /// 这样界面截断显示时先看到的是要紧的那几条。
    pub fn evaluate(
        &mut self,
        at_ms: u64,
        round: u64,
        previous: &[ProbeResult],
        current: &[ProbeResult],
        _thresholds: &Thresholds,
        uptime: &BTreeMap<IpAddr, f64>,
    ) -> Vec<AlertEvent> {
        let prev_online: BTreeMap<IpAddr, bool> = previous
            .iter()
            .map(|r| (r.ip, r.status.is_online()))
            .collect();
        let suppress_appeared = !self.primed;

        let mut fired: Vec<AlertEvent> = Vec::new();
        let mut suppressed_here: u64 = 0;

        for r in current {
            let was_online = prev_online
                .get(&r.ip)
                .copied()
                .or_else(|| self.seen.get(&r.ip).copied());
            let ctx = AlertContext {
                ip: r.ip,
                index: r.index,
                current: r,
                was_online,
                uptime_ratio: uptime.get(&r.ip).copied(),
                at_ms,
                round,
            };
            for rule in &self.config.rules {
                if suppress_appeared && rule.kind == AlertKind::Appeared {
                    continue;
                }
                let Some(event) = match_rule(rule, &ctx) else {
                    continue;
                };
                // 冷却：同一 (类型, 主机) 在窗口内只报一次
                if let Some(last) = self.last_fired.get(&event.dedup_key()) {
                    let elapsed = at_ms.saturating_sub(*last);
                    if elapsed < rule.cooldown_secs.saturating_mul(1000) {
                        suppressed_here += 1;
                        continue;
                    }
                }
                // 静音：**状态照常推进**（下面统一记 last_fired），只是不返回
                self.last_fired.insert(event.dedup_key(), at_ms);
                if self.config.muted {
                    self.suppressed_by_mute += 1;
                    continue;
                }
                fired.push(event);
            }
        }
        self.suppressed_by_cooldown += suppressed_here;

        // 更新基线：无论是否静音/冷却，观测事实都要记下来
        for r in current {
            self.seen.insert(r.ip, r.status.is_online());
        }
        self.primed = true;

        fired.sort_by(|a, b| {
            b.severity
                .cmp(&a.severity)
                .then(a.index.cmp(&b.index))
                .then(a.kind.cmp(&b.kind))
        });
        for e in &fired {
            self.log.push(e.clone());
        }
        fired
    }

    /// 清空事件缓冲与去重状态（**不**清静音开关：静音是用户偏好，不是运行状态）。
    pub fn reset(&mut self) {
        self.log.clear();
        self.last_fired.clear();
        self.seen.clear();
        self.primed = false;
        self.suppressed_by_cooldown = 0;
        self.suppressed_by_mute = 0;
    }

    pub fn seen_hosts(&self) -> usize {
        self.seen.len()
    }

    // ------------------------------------------------------------------
    // 状态落盘（行式文本，零依赖）
    // ------------------------------------------------------------------

    /// 序列化去重状态。字段之间是真制表符（下面示例用空格代替，制表符在
    /// 文档注释里会被 clippy 判为不规范）：
    ///
    /// ```text
    /// CPING-ALERTS 1
    /// muted 0
    /// primed 1
    /// seen 192.168.1.1 1
    /// fired disappeared 192.168.1.9 1700000000000
    /// ```
    pub fn to_text(&self) -> String {
        let mut out = String::with_capacity(64 + (self.seen.len() + self.last_fired.len()) * 32);
        out.push_str(&format!(
            "{ALERT_STATE_SIGNATURE}\t{ALERT_STATE_FORMAT_VERSION}\n"
        ));
        out.push_str(&format!("muted\t{}\n", u8::from(self.config.muted)));
        out.push_str(&format!("primed\t{}\n", u8::from(self.primed)));
        for (ip, online) in &self.seen {
            out.push_str(&format!("seen\t{ip}\t{}\n", u8::from(*online)));
        }
        for ((kind, ip), at) in &self.last_fired {
            out.push_str(&format!("fired\t{}\t{ip}\t{at}\n", kind.key()));
        }
        out
    }

    /// 从文本恢复。返回 `(引擎, 跳过的坏行数)`。
    ///
    /// 与 M14 的时序文件同一套策略：**版本不符显式拒绝**，坏行只跳过并计数。
    /// 状态文件坏掉的代价只是「冷却窗口重新开始」，远小于拒绝启动。
    pub fn from_text(text: &str) -> Result<(Self, usize), String> {
        let mut engine = Self::new(AlertConfig::default());
        let mut skipped = 0usize;
        let mut lines = text.lines();

        match lines.next() {
            Some(first) => {
                let mut parts = first.split('\t');
                let sig = parts.next().unwrap_or("");
                if sig != ALERT_STATE_SIGNATURE {
                    return Err(format!("不是 CPing 告警状态文件（头 {sig:?}）"));
                }
                match parts.next().and_then(|v| v.parse::<u32>().ok()) {
                    Some(v) if v == ALERT_STATE_FORMAT_VERSION => {}
                    Some(v) => {
                        return Err(format!(
                        "告警状态文件版本 {v} 与当前 {ALERT_STATE_FORMAT_VERSION} 不符，拒绝读取"
                    ))
                    }
                    None => return Err("告警状态文件缺少版本号".into()),
                }
            }
            None => return Err("告警状态文件为空".into()),
        }

        for line in lines {
            let line = line.trim_end_matches('\r');
            if line.is_empty() {
                continue;
            }
            let mut it = line.split('\t');
            match it.next() {
                Some("muted") => match it.next() {
                    Some("1") => engine.config.muted = true,
                    Some("0") => engine.config.muted = false,
                    _ => skipped += 1,
                },
                Some("primed") => match it.next() {
                    Some("1") => engine.primed = true,
                    Some("0") => engine.primed = false,
                    _ => skipped += 1,
                },
                Some("seen") => {
                    let ip = it.next().and_then(|v| v.parse::<IpAddr>().ok());
                    let online = match it.next() {
                        Some("1") => Some(true),
                        Some("0") => Some(false),
                        _ => None,
                    };
                    match (ip, online) {
                        (Some(ip), Some(online)) => {
                            engine.seen.insert(ip, online);
                        }
                        _ => skipped += 1,
                    }
                }
                Some("fired") => {
                    let kind = it.next().and_then(AlertKind::parse_key);
                    let ip = it.next().and_then(|v| v.parse::<IpAddr>().ok());
                    let at = it.next().and_then(|v| v.parse::<u64>().ok());
                    match (kind, ip, at) {
                        (Some(kind), Some(ip), Some(at)) => {
                            engine.last_fired.insert((kind, ip), at);
                        }
                        _ => skipped += 1,
                    }
                }
                _ => skipped += 1,
            }
        }
        Ok((engine, skipped))
    }

    pub fn save_to_path(&self, path: &str) -> Result<(), String> {
        std::fs::write(path, self.to_text()).map_err(|e| format!("写入 {path} 失败：{e}"))
    }

    pub fn load_from_path(path: &str) -> Result<(Self, usize), String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("读取 {path} 失败：{e}"))?;
        Self::from_text(&text)
    }
}

/// `AlertKind` 的展示顺序（界面里按这个顺序列规则）。
pub fn rule_kinds_in_display_order() -> [AlertKind; 4] {
    [
        AlertKind::Appeared,
        AlertKind::Disappeared,
        AlertKind::LatencyAbove,
        AlertKind::UptimeBelow,
    ]
}

/// 把一组结果按「主机 → 是否在线」收集（供 `prime_with` 之类的调用方复用）。
pub fn online_map(results: &[ProbeResult]) -> BTreeMap<IpAddr, bool> {
    results
        .iter()
        .map(|r| (r.ip, r.status.is_online()))
        .collect()
}

/// 便捷构造：某主机的一轮「离线」结果（测试与界面演示用）。
pub fn offline_status() -> ProbeStatus {
    ProbeStatus::Timeout
}

/// 全部告警类型的集合（`--alert-kinds` 省略时的默认）。
pub fn all_kinds() -> BTreeSet<AlertKind> {
    AlertKind::ALL.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn ip(a: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(10, 0, 0, a))
    }

    fn online(a: u8, idx: u16, rtt: u32) -> ProbeResult {
        ProbeResult::new(idx, ip(a), ProbeStatus::Online { rtt_ms: rtt })
    }

    fn offline(a: u8, idx: u16) -> ProbeResult {
        ProbeResult::new(idx, ip(a), ProbeStatus::Timeout)
    }

    fn ctx<'a>(r: &'a ProbeResult, was_online: Option<bool>) -> AlertContext<'a> {
        AlertContext {
            ip: r.ip,
            index: r.index,
            current: r,
            was_online,
            uptime_ratio: None,
            at_ms: 1_700_000_000_000,
            round: 2,
        }
    }

    fn empty_uptime() -> BTreeMap<IpAddr, f64> {
        BTreeMap::new()
    }

    #[test]
    fn appeared_only_fires_for_a_genuinely_new_host() {
        let rule = AlertRule::new(AlertKind::Appeared, 0, 0);
        let r = online(1, 0, 5);
        // 纯函数层：`was_online == None` 就是「从没见过」，此时在线才算「新增上线」。
        // 「第一轮不刷屏」由引擎的 primed 负责，见下面 first_round_* 的测试。
        let ev = match_rule(&rule, &ctx(&r, None)).expect("从没见过且在线 → 新增");
        assert_eq!(ev.kind, AlertKind::Appeared);
        assert_eq!(ev.severity, Severity::Info);
        assert!(ev.message.contains("新增主机"), "{ev:?}");
        // 见过（无论当时在线与否）都不再算新增
        assert!(match_rule(&rule, &ctx(&r, Some(true))).is_none());
        assert!(match_rule(&rule, &ctx(&r, Some(false))).is_none());
        // 从没见过但**离线** → 不算「新增主机上线」
        let off = offline(2, 1);
        assert!(match_rule(&rule, &ctx(&off, None)).is_none());
    }

    #[test]
    fn disappeared_needs_a_previous_online_and_a_final_status() {
        let rule = AlertRule::new(AlertKind::Disappeared, 0, 0);
        let off = offline(1, 0);
        assert!(
            match_rule(&rule, &ctx(&off, None)).is_none(),
            "从没见过不算离线"
        );
        assert!(
            match_rule(&rule, &ctx(&off, Some(false))).is_none(),
            "一直离线不该每轮都报"
        );
        let ev = match_rule(&rule, &ctx(&off, Some(true))).expect("在线转离线必须命中");
        assert_eq!(ev.severity, Severity::Critical);

        // Pending 不是「离线」：扫描跑一半不能报警
        let pending = ProbeResult::new(0, ip(1), ProbeStatus::Pending);
        assert!(
            match_rule(&rule, &ctx(&pending, Some(true))).is_none(),
            "未探测完不能当成离线"
        );
    }

    #[test]
    fn latency_rule_uses_a_strict_greater_than() {
        let rule = AlertRule::new(AlertKind::LatencyAbove, 100, 0);
        assert!(match_rule(&rule, &ctx(&online(1, 0, 99), Some(true))).is_none());
        assert!(
            match_rule(&rule, &ctx(&online(1, 0, 100), Some(true))).is_none(),
            "恰好等于阈值不告警（严格大于）"
        );
        let ev = match_rule(&rule, &ctx(&online(1, 0, 101), Some(true))).expect("超过阈值必须命中");
        assert_eq!(ev.value, Some(101.0));
        assert_eq!(ev.threshold, Some(100));
        // 离线主机不按延迟告警（它没有 RTT）
        assert!(match_rule(&rule, &ctx(&offline(1, 0), Some(false))).is_none());
    }

    #[test]
    fn uptime_rule_uses_a_strict_less_than_and_needs_history() {
        let rule = AlertRule::new(AlertKind::UptimeBelow, 90, 0);
        let online_host = online(1, 0, 5);
        let mut c = ctx(&online_host, Some(true));
        assert!(
            match_rule(&rule, &c).is_none(),
            "没有历史数据就不该报可用率——那是臆造"
        );
        c.uptime_ratio = Some(0.90);
        assert!(match_rule(&rule, &c).is_none(), "恰好等于阈值不告警");
        c.uptime_ratio = Some(0.899);
        let ev = match_rule(&rule, &c).expect("低于阈值必须命中");
        assert_eq!(ev.threshold, Some(90));
        assert!((ev.value.unwrap() - 89.9).abs() < 1e-9);
        // 离线主机也可能可用率偏低——这条规则不看在线与否
        let offline_host = offline(1, 0);
        let mut c2 = ctx(&offline_host, Some(false));
        c2.uptime_ratio = Some(0.5);
        assert!(match_rule(&rule, &c2).is_some());
    }

    #[test]
    fn disabled_rule_never_matches_whatever_the_input() {
        let mut rule = AlertRule::new(AlertKind::LatencyAbove, 1, 0);
        rule.enabled = false;
        assert!(match_rule(&rule, &ctx(&online(1, 0, 9999), Some(true))).is_none());
    }

    #[test]
    fn first_round_never_reports_the_whole_subnet_as_new() {
        let mut engine = AlertEngine::new(AlertConfig::default());
        let results: Vec<ProbeResult> = (1..=20).map(|i| online(i, i as u16, 5)).collect();
        let events = engine.evaluate(
            1_000,
            1,
            &[],
            &results,
            &Thresholds::default(),
            &empty_uptime(),
        );
        assert!(
            events.is_empty(),
            "第一轮只建立基线，不该刷出 20 条「新增主机」：{events:?}"
        );
        assert_eq!(engine.seen_hosts(), 20);
        // 第二轮：都见过了，也不该报新增
        let events2 = engine.evaluate(
            2_000,
            2,
            &results,
            &results,
            &Thresholds::default(),
            &empty_uptime(),
        );
        assert!(events2.is_empty(), "{events2:?}");
    }

    #[test]
    fn a_truly_new_host_alerts_after_the_baseline() {
        let mut engine = AlertEngine::new(AlertConfig::default());
        let first = vec![online(1, 0, 5)];
        engine.evaluate(
            1_000,
            1,
            &[],
            &first,
            &Thresholds::default(),
            &empty_uptime(),
        );
        let second = vec![online(1, 0, 5), online(2, 1, 6)];
        let events = engine.evaluate(
            2_000,
            2,
            &first,
            &second,
            &Thresholds::default(),
            &empty_uptime(),
        );
        let appeared: Vec<_> = events
            .iter()
            .filter(|e| e.kind == AlertKind::Appeared)
            .collect();
        assert_eq!(appeared.len(), 1, "{events:?}");
        assert_eq!(appeared[0].ip, ip(2));
    }

    #[test]
    fn cooldown_suppresses_a_level_rule_within_the_window() {
        let cfg = AlertConfig {
            rules: vec![AlertRule::new(AlertKind::LatencyAbove, 10, 60)],
            ..AlertConfig::default()
        };
        let mut engine = AlertEngine::new(cfg);
        let slow = vec![online(1, 0, 500)];
        let t0 = 1_700_000_000_000u64;

        let first = engine.evaluate(t0, 1, &slow, &slow, &Thresholds::default(), &empty_uptime());
        assert_eq!(first.len(), 1, "首次超阈必须报");
        // 冷却窗口内
        let again = engine.evaluate(
            t0 + 30_000,
            2,
            &slow,
            &slow,
            &Thresholds::default(),
            &empty_uptime(),
        );
        assert!(again.is_empty(), "冷却窗口内不得重复上报：{again:?}");
        assert_eq!(engine.suppressed_by_cooldown(), 1);
        // 窗口外
        let later = engine.evaluate(
            t0 + 61_000,
            3,
            &slow,
            &slow,
            &Thresholds::default(),
            &empty_uptime(),
        );
        assert_eq!(later.len(), 1, "冷却窗口过后应恢复上报");
    }

    #[test]
    fn cooldown_is_per_host_and_per_kind() {
        let cfg = AlertConfig {
            rules: vec![AlertRule::new(AlertKind::LatencyAbove, 10, 3600)],
            ..AlertConfig::default()
        };
        let mut engine = AlertEngine::new(cfg);
        let t0 = 1_700_000_000_000u64;
        let a = vec![online(1, 0, 500)];
        let b = vec![online(2, 1, 500)];
        assert_eq!(
            engine
                .evaluate(t0, 1, &a, &a, &Thresholds::default(), &empty_uptime())
                .len(),
            1
        );
        assert_eq!(
            engine
                .evaluate(
                    t0 + 1_000,
                    2,
                    &b,
                    &b,
                    &Thresholds::default(),
                    &empty_uptime()
                )
                .len(),
            1,
            "另一台主机有自己的冷却窗口，不该被前一台的记录挡下"
        );
    }

    #[test]
    fn mute_silences_events_but_keeps_the_dedup_state_advancing() {
        let cfg = AlertConfig {
            rules: vec![AlertRule::new(AlertKind::LatencyAbove, 10, 60)],
            muted: true,
            ..AlertConfig::default()
        };
        let mut engine = AlertEngine::new(cfg);
        let slow = vec![online(1, 0, 500)];
        let t0 = 1_700_000_000_000u64;
        assert!(engine
            .evaluate(t0, 1, &slow, &slow, &Thresholds::default(), &empty_uptime())
            .is_empty());
        assert_eq!(engine.suppressed_by_mute(), 1);
        assert!(engine.is_muted());

        // 取消静音后，冷却窗口内的重复仍被挡下 → 不会「一解除静音就补报一堆」
        engine.set_muted(false);
        assert!(engine
            .evaluate(
                t0 + 10_000,
                2,
                &slow,
                &slow,
                &Thresholds::default(),
                &empty_uptime()
            )
            .is_empty());
        assert_eq!(engine.suppressed_by_cooldown(), 1, "被冷却挡下而不是补报");
    }

    #[test]
    fn events_are_sorted_by_severity_then_index() {
        let cfg = AlertConfig {
            rules: vec![
                AlertRule::new(AlertKind::LatencyAbove, 10, 0),
                AlertRule::new(AlertKind::Disappeared, 0, 0),
            ],
            ..AlertConfig::default()
        };
        let mut engine = AlertEngine::new(cfg);
        let prev = vec![online(1, 0, 5), online(2, 1, 5), online(3, 2, 5)];
        let cur = vec![online(1, 0, 999), offline(2, 1), online(3, 2, 999)];
        let events = engine.evaluate(
            2_000,
            2,
            &prev,
            &cur,
            &Thresholds::default(),
            &empty_uptime(),
        );
        assert_eq!(events.len(), 3, "{events:?}");
        assert_eq!(
            events[0].severity,
            Severity::Critical,
            "严重度最高的必须排最前：{events:?}"
        );
        assert_eq!(events[0].ip, ip(2));
        assert_eq!(events[1].ip, ip(1), "同严重度按主机序号");
        assert_eq!(events[2].ip, ip(3));
    }

    #[test]
    fn uptime_map_is_consumed_not_recomputed() {
        let cfg = AlertConfig {
            rules: vec![AlertRule::new(AlertKind::UptimeBelow, 90, 0)],
            ..AlertConfig::default()
        };
        let mut engine = AlertEngine::new(cfg);
        let cur = vec![online(1, 0, 5)];
        let mut uptime = BTreeMap::new();
        uptime.insert(ip(1), 0.5);
        let events = engine.evaluate(2_000, 1, &[], &cur, &Thresholds::default(), &uptime);
        assert_eq!(events.len(), 1);
        assert!((events[0].value.unwrap() - 50.0).abs() < 1e-9);
    }

    #[test]
    fn state_roundtrip_is_byte_stable() {
        let mut engine = AlertEngine::new(AlertConfig::default());
        let cur = vec![online(1, 0, 999), offline(2, 1)];
        engine.evaluate(1_000, 1, &[], &cur, &Thresholds::default(), &empty_uptime());
        engine.set_muted(true);
        let text = engine.to_text();
        let (back, skipped) = AlertEngine::from_text(&text).unwrap();
        assert_eq!(skipped, 0);
        assert!(back.is_muted());
        assert!(back.is_primed());
        assert_eq!(back.seen_hosts(), engine.seen_hosts());
        assert_eq!(back.to_text(), text, "写出 → 读回 → 再写出必须逐字节一致");
    }

    #[test]
    fn state_version_mismatch_is_rejected_and_bad_lines_are_counted() {
        let err = AlertEngine::from_text("CPING-ALERTS\t9\n").unwrap_err();
        assert!(err.contains("版本"), "{err}");
        assert!(AlertEngine::from_text("WHATEVER\t1\n").is_err());
        assert!(AlertEngine::from_text("").is_err());

        let mut text = String::from("CPING-ALERTS\t1\n");
        text.push_str("primed\t1\n");
        text.push_str("这行是垃圾\n");
        text.push_str("seen\tnot-an-ip\t1\n");
        text.push_str("fired\tnope\t10.0.0.1\t1\n");
        let (engine, skipped) = AlertEngine::from_text(&text).unwrap();
        assert_eq!(skipped, 3, "三行坏数据都要计数");
        assert!(engine.is_primed(), "合法行仍要生效");
    }

    #[test]
    fn reset_keeps_the_mute_preference() {
        let mut engine = AlertEngine::new(AlertConfig::default());
        engine.set_muted(true);
        let cur = vec![offline(1, 0)];
        engine.evaluate(1_000, 1, &[], &cur, &Thresholds::default(), &empty_uptime());
        engine.reset();
        assert!(engine.is_muted(), "静音是用户偏好，不该被 reset 清掉");
        assert!(engine.log().is_empty());
        assert_eq!(engine.seen_hosts(), 0);
        assert!(!engine.is_primed(), "reset 后第一轮要重新建立基线");
    }

    #[test]
    fn log_is_a_bounded_ring_buffer() {
        let cfg = AlertConfig {
            capacity: 3,
            rules: vec![AlertRule::new(AlertKind::LatencyAbove, 1, 0)],
            ..AlertConfig::default()
        };
        let mut engine = AlertEngine::new(cfg);
        for i in 0..5u64 {
            let cur = vec![online(1, 0, 100 + i as u32)];
            engine.evaluate(
                1_000 + i * 60_000,
                i + 1,
                &cur,
                &cur,
                &Thresholds::default(),
                &empty_uptime(),
            );
        }
        assert_eq!(engine.log().len(), 3, "缓冲不得超过容量");
        assert_eq!(engine.log().total_seen(), 5, "累计条数要如实记录");
        let recent: Vec<_> = engine.log().iter_recent_first().take(1).collect();
        assert!(!recent.is_empty());
    }

    #[test]
    fn kind_keys_round_trip_and_unknown_is_rejected() {
        for k in AlertKind::ALL {
            assert_eq!(AlertKind::parse_key(k.key()), Some(k));
            assert!(!k.label_zh().is_empty());
        }
        assert_eq!(
            AlertKind::parse_key("offline"),
            Some(AlertKind::Disappeared)
        );
        assert_eq!(AlertKind::parse_key("nope"), None);
        assert!(!AlertKind::Appeared.uses_threshold());
        assert!(AlertKind::LatencyAbove.uses_threshold());
        assert!(AlertKind::UptimeBelow.uses_threshold());
    }

    #[test]
    fn with_kinds_disables_without_dropping_the_rule() {
        let mut cfg = AlertConfig::default();
        cfg.with_kinds(&[AlertKind::Disappeared]);
        assert_eq!(cfg.rules.len(), 4, "规则不删除，只置为停用");
        assert!(cfg.rule(AlertKind::Disappeared).unwrap().enabled);
        assert!(!cfg.rule(AlertKind::LatencyAbove).unwrap().enabled);
        assert_eq!(
            cfg.rule(AlertKind::LatencyAbove).unwrap().threshold,
            DEFAULT_LATENCY_THRESHOLD_MS,
            "阈值要留着，界面才能说明「这条被关了，阈值是多少」"
        );
        assert_eq!(all_kinds().len(), 4);
        assert_eq!(rule_kinds_in_display_order().len(), 4);
    }

    #[test]
    fn time_text_is_utc_and_readable() {
        let ev = AlertEvent {
            kind: AlertKind::Appeared,
            severity: Severity::Info,
            ip: ip(1),
            index: 0,
            at_ms: 0,
            round: 1,
            value: None,
            threshold: None,
            message: String::new(),
        };
        assert_eq!(ev.time_text(), "1970-01-01 00:00:00 UTC");
        assert_eq!(offline_status(), ProbeStatus::Timeout);
    }
}
