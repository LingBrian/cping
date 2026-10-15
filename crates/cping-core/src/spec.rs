//! 扫描规格（M9 起：目标规格解析与探测方式）。
//!
//! 原版的输入是「C 类网段前三段」（`192.168.1`），本项目**保留**它作为默认形态，
//! 并在此之上扩展五种写法：
//!
//! | 写法 | 例子 | 目标数 |
//! | --- | --- | --- |
//! | 前缀（原版） | `192.168.1` | 255（`.1`–`.255`） |
//! | 单主机 | `192.168.1.7` | 1 |
//! | CIDR | `192.168.1.0/24` | 254（排除网络号与广播） |
//! | 多段 | `192.168.1, 192.168.31` | 各段之和 |
//! | **IPv6 单主机 / CIDR**（M12） | `::1`、`fd00::1/126` | 1 / 2（/126 排除网络号与末位） |
//!
//! 三条硬规则：
//! 1. **CIDR 的主机位必须为 0**（`192.168.1.5/24` 一律报错）。写错网段却拿到
//!    一份「看起来对」的结果，比直接报错危险得多。
//! 2. 多段按输入顺序拼接，**重复目标只保留首次出现**；每个输入段在最终列表里
//!    仍然是一段连续区间（[`Segment`]），界面据此分段显示。
//! 3. **IPv6 目标会让矩阵视图失效**：矩阵一格绑定一个 IPv4 末段，v6 没有这个
//!    对应关系，所以 [`TargetSpec::matrix_compatible`] 会返回 `false`，
//!    界面必须退化为列表/表格（见 `MATRIX_VIEW_MAX` 的说明）。

use crate::error::{CoreError, CoreResult};
use crate::i18n::{self, Key, Lang};
use crate::target::{build_targets, parse_prefix, Ipv4Prefix, TargetRange, MAX_TARGETS};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// 矩阵视图可表示的目标数上限（16×16 = 256 格）。
///
/// 超过它就**必须**退化为列表/表格视图 —— 这不是性能限制而是语义限制：
/// 矩阵的一格绑定一个 IP 末段，>256 个目标没有位置可放。
pub const MATRIX_VIEW_MAX: usize = 256;

/// 探测方式（M9）。
///
/// **一次任务只有一种方式**，因此「ICMP 在线」与「TCP 开放」在统计上天然不会混计：
/// 不存在同一次扫描里既有 ICMP 结果又有 TCP 结果的情况。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProbeMode {
    /// 原生 ICMP（默认）。
    #[default]
    Icmp,
    /// TCP 连通性：对指定端口发起三次握手。
    Tcp { port: u16 },
}

impl ProbeMode {
    /// 常用端口预设（界面下拉用）。
    pub const COMMON_PORTS: [u16; 8] = [22, 23, 80, 135, 139, 443, 445, 3389];

    pub const fn key(self) -> &'static str {
        match self {
            ProbeMode::Icmp => "icmp",
            ProbeMode::Tcp { .. } => "tcp",
        }
    }

    pub const fn label_zh(self) -> &'static str {
        match self {
            ProbeMode::Icmp => "ICMP",
            ProbeMode::Tcp { .. } => "TCP",
        }
    }

    pub const fn is_tcp(self) -> bool {
        matches!(self, ProbeMode::Tcp { .. })
    }

    pub const fn tcp_port(self) -> Option<u16> {
        match self {
            ProbeMode::Tcp { port } => Some(port),
            ProbeMode::Icmp => None,
        }
    }

    /// 界面与导出里的完整描述，例如 `TCP:80`。
    pub fn describe(self) -> String {
        match self {
            ProbeMode::Icmp => "ICMP".to_string(),
            ProbeMode::Tcp { port } => format!("TCP:{port}"),
        }
    }

    /// 状态标签前缀：TCP 模式下「在线」必须显示成 `TCP-OPEN`，
    /// 否则用户会把 TCP 握手误读成 ICMP 可达。
    pub const fn status_prefix(self) -> &'static str {
        match self {
            ProbeMode::Icmp => "",
            ProbeMode::Tcp { .. } => "TCP-OPEN ",
        }
    }

    /// 该模式下的状态展示文本。
    ///
    /// ICMP 模式沿用 [`crate::result::ProbeStatus::label_zh`]；
    /// TCP 模式换成一套**自解释**的名字，让「TCP 握手成功」永远不可能被当成
    /// 「ICMP 可达」读。
    pub fn status_label(self, status: &crate::result::ProbeStatus) -> String {
        self.status_label_in(status, Lang::Zh)
    }

    /// 按语言渲染该模式下的状态文本。
    ///
    /// ICMP 模式直接转发 [`crate::result::ProbeStatus::label_in`]；
    /// TCP 模式用一套**独立**的词（`TCP-OPEN` / `TCP-超时` / `TCP-关闭`），
    /// 不复用 ICMP 的「在线 / 超时 / 不可达」——两套词长得像，用户就会
    /// 把 TCP 握手成功当成 ICMP 可达。`TCP-OPEN` 两种语言下都一样，
    /// 它在 [`i18n::ASCII_TOKEN_KEYS`] 里，那是因为它是记号不是句子。
    pub fn status_label_in(self, status: &crate::result::ProbeStatus, lang: Lang) -> String {
        use crate::result::ProbeStatus as S;
        match self {
            ProbeMode::Icmp => status.label_in(lang).to_string(),
            ProbeMode::Tcp { .. } => {
                let key = match status {
                    S::Pending => Key::LabelModeTcpPending,
                    S::Online { .. } => Key::LabelModeTcpOpen,
                    S::Timeout => Key::LabelModeTcpTimeout,
                    S::Unreachable => Key::LabelModeTcpClosed,
                    // 这三种失败原因与探测方式无关，沿用通用标签，
                    // 免得同一件事在两种模式下叫两个名字。
                    S::PermissionDenied => Key::LabelStatusPermissionDenied,
                    S::Unsupported => Key::LabelStatusUnsupported,
                    S::Error(_) => Key::LabelStatusError,
                };
                i18n::text(lang, key).to_string()
            }
        }
    }

    pub fn validate(self) -> CoreResult<()> {
        self.validate_in(Lang::Zh)
    }

    /// 校验探测方式自身是否自洽，错误文本按语言产出。
    pub fn validate_in(self, lang: Lang) -> CoreResult<()> {
        if let ProbeMode::Tcp { port } = self {
            if port == 0 {
                return Err(CoreError::InvalidConfig(i18n::render(
                    lang,
                    Key::ErrTcpPortZero,
                    &[],
                )));
            }
        }
        Ok(())
    }

    /// 后端标识（与 `ProbeBackend::name()` 对齐，便于诊断）。
    pub const fn backend_name(self) -> &'static str {
        match self {
            ProbeMode::Icmp => "icmp",
            ProbeMode::Tcp { .. } => "tcp-connect",
        }
    }
}

/// 一个网段在最终目标列表中的位置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// 展示用标签，例如 `192.168.1` 或 `192.168.1.0/24`。
    pub label: String,
    /// 在 `targets` 中的起始下标。
    pub start: usize,
    /// 目标个数（去重之后）。
    pub count: usize,
}

impl Segment {
    /// 结束下标（不含）。
    pub fn end(&self) -> usize {
        self.start + self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// `192.168.1（255 个）`
    pub fn describe(&self) -> String {
        self.describe_in(Lang::Zh)
    }

    /// 按语言渲染 `192.168.1（255 个）` / `192.168.1 (255 targets)`。
    pub fn describe_in(&self, lang: Lang) -> String {
        i18n::render(
            lang,
            Key::SegmentDescribe,
            &[&self.label, &self.count.to_string()],
        )
    }
}

/// 解析并去重后的完整扫描目标规格。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TargetSpec {
    /// 目标列表，按「输入段顺序 + 段内升序」排列，无重复。
    ///
    /// M12 起为 [`IpAddr`]（同时容纳 v4 / v6）。
    pub targets: Vec<IpAddr>,
    /// 各输入段映射到 `targets` 的连续区间。
    pub segments: Vec<Segment>,
}

impl TargetSpec {
    pub fn len(&self) -> usize {
        self.targets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }

    /// 目标里是否含 IPv6 地址。
    pub fn has_ipv6(&self) -> bool {
        self.targets.iter().any(|ip| ip.is_ipv6())
    }

    /// 能否用 16×16 矩阵表示。
    ///
    /// 两个条件缺一不可：目标数 ≤ [`MATRIX_VIEW_MAX`]，且**全部是 IPv4**。
    /// 矩阵的一格绑定一个 IPv4 末段，v6 目标没有格位可放。
    pub fn matrix_compatible(&self) -> bool {
        !self.targets.is_empty() && self.targets.len() <= MATRIX_VIEW_MAX && !self.has_ipv6()
    }

    /// 单段规格（内部与测试用）。
    pub fn single(label: String, targets: Vec<IpAddr>) -> Self {
        let count = targets.len();
        let segments = if count == 0 {
            Vec::new()
        } else {
            vec![Segment {
                label,
                start: 0,
                count,
            }]
        };
        Self { targets, segments }
    }

    /// 供界面标题使用的一行摘要。
    pub fn summary(&self) -> String {
        self.summary_in(Lang::Zh)
    }

    /// 按语言渲染标题摘要。
    pub fn summary_in(&self, lang: Lang) -> String {
        if self.targets.is_empty() {
            return i18n::text(lang, Key::SpecNoTargets).to_string();
        }
        if self.segments.len() == 1 {
            return self.segments[0].describe_in(lang);
        }
        i18n::render(
            lang,
            Key::SpecSegmentsAndTargets,
            &[
                &self.segments.len().to_string(),
                &self.targets.len().to_string(),
            ],
        )
    }

    /// 首尾目标（展示用，例如状态栏「192.168.1.1 – 192.168.1.255」）。
    pub fn span_text(&self) -> Option<String> {
        let first = self.targets.first()?;
        let last = self.targets.last()?;
        if first == last {
            Some(first.to_string())
        } else {
            Some(format!("{first} – {last}"))
        }
    }
}

/// 解析目标规格串。空白、逗号、分号、换行都当作分隔符。
pub fn parse_spec(input: &str, range: TargetRange) -> CoreResult<TargetSpec> {
    let tokens = tokenize(input);
    if tokens.is_empty() {
        return Err(CoreError::InvalidPrefix("目标规格为空".into()));
    }

    let mut targets: Vec<IpAddr> = Vec::new();
    let mut segments: Vec<Segment> = Vec::new();
    // 去重：键是 (族, 数值)。用 u128 承载 v6，避免为 65536 个目标开位图（要 512 MB）。
    let mut seen: std::collections::HashSet<(u8, u128)> =
        std::collections::HashSet::with_capacity(MAX_TARGETS.min(4096));

    for token in tokens {
        let block = parse_token(token, range)?;
        let start = targets.len();
        for ip in block.targets {
            if !seen.insert(addr_sort_key(ip)) {
                continue;
            }
            if targets.len() == MAX_TARGETS {
                return Err(CoreError::TooManyTargets {
                    actual: targets.len() + 1,
                    limit: MAX_TARGETS,
                });
            }
            targets.push(ip);
        }
        segments.push(Segment {
            label: block.label,
            start,
            count: targets.len() - start,
        });
    }

    if targets.is_empty() {
        return Err(CoreError::InvalidPrefix("目标规格未展开出任何目标".into()));
    }

    Ok(TargetSpec { targets, segments })
}

struct TokenBlock {
    label: String,
    targets: Vec<IpAddr>,
}

fn parse_token(token: &str, range: TargetRange) -> CoreResult<TokenBlock> {
    // 含冒号即视为 IPv6（v6 地址里不可能出现冒号以外的分隔符）。
    if token.contains(':') {
        return parse_ipv6_token(token);
    }

    if let Some((addr_text, prefix_text)) = token.split_once('/') {
        return parse_cidr(addr_text, prefix_text);
    }
    let dots = token.matches('.').count();
    match dots {
        // 两段点 = 三段前缀（原版写法，如 192.168.1）
        2 => {
            let prefix = parse_prefix(token)?;
            let targets = build_targets(&prefix, range)?;
            Ok(TokenBlock {
                label: prefix.to_string(),
                targets: targets.into_iter().map(IpAddr::V4).collect(),
            })
        }
        // 三段点 = 完整 IPv4（单主机）
        3 => {
            let ip = parse_ipv4(token)?;
            Ok(TokenBlock {
                label: ip.to_string(),
                targets: vec![IpAddr::V4(ip)],
            })
        }
        _ => Err(CoreError::InvalidPrefix(format!(
            "{token:?} 既不是三段前缀、完整 IP，也不是 CIDR"
        ))),
    }
}

// ---------------------------------------------------------------------------
// IPv6（M12）
// ---------------------------------------------------------------------------

/// 解析 IPv6 目标：`::1`、`fd00::1/126`。
///
/// 与 IPv4 一致的规则：
/// - CIDR 的主机位必须为 0，否则报错并给出正确网段；
/// - `/128` 是单个地址，`/127` 两个地址，其余排除网络号与末位；
/// - 展开后的数量受 [`MAX_TARGETS`] 约束，/64 这类巨型网段会被明确拒绝
///   （不是「扫不完」，而是「本来就不该这么扫」）。
fn parse_ipv6_token(token: &str) -> CoreResult<TokenBlock> {
    let Some((addr_text, prefix_text)) = token.split_once('/') else {
        let ip = parse_ipv6(token)?;
        return Ok(TokenBlock {
            label: ip.to_string(),
            targets: vec![IpAddr::V6(ip)],
        });
    };

    let addr = parse_ipv6(addr_text.trim())?;
    let len_text = prefix_text.trim();
    if len_text.is_empty() || !len_text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(CoreError::InvalidPrefix(format!(
            "IPv6 CIDR 掩码 {len_text:?} 不是数字"
        )));
    }
    let len: u32 = len_text
        .parse()
        .map_err(|_| CoreError::InvalidPrefix(format!("IPv6 CIDR 掩码 {len_text:?} 无法解析")))?;
    if len > 128 {
        return Err(CoreError::InvalidPrefix(format!(
            "IPv6 CIDR 掩码 /{len} 超出 0-128"
        )));
    }

    let bits = u128::from(addr);
    let mask: u128 = if len == 0 {
        0
    } else {
        u128::MAX << (128 - len)
    };
    if bits & !mask != 0 {
        let network = Ipv6Addr::from(bits & mask);
        return Err(CoreError::InvalidPrefix(format!(
            "{addr_text}/{len} 的主机位不为 0；网段应写作 {network}/{len}"
        )));
    }
    let network = bits & mask;
    let last = network | !mask;
    let span = last - network; // 网段内地址总数 - 1

    let mut targets = Vec::new();
    match len {
        128 => targets.push(IpAddr::V6(addr)),
        127 => {
            // 与 IPv4 的 /31（RFC 3021）一致：两点链路两端都可用
            targets.push(IpAddr::V6(Ipv6Addr::from(network)));
            targets.push(IpAddr::V6(Ipv6Addr::from(last)));
        }
        _ => {
            let count = span - 1; // 排除网络号与末位
            if count > MAX_TARGETS as u128 {
                return Err(CoreError::InvalidPrefix(format!(
                    "{addr_text}/{len} 展开后有 {count} 个地址，超过上限 {MAX_TARGETS}；\
                     请改用更具体的掩码（IPv6 里 /113 及更长才有可枚举的规模）"
                )));
            }
            for raw in (network + 1)..=last.saturating_sub(1) {
                targets.push(IpAddr::V6(Ipv6Addr::from(raw)));
            }
        }
    }

    Ok(TokenBlock {
        label: format!("{}/{}", Ipv6Addr::from(network), len),
        targets,
    })
}

/// 宽松的 IPv6 解析：只接受 `std` 认可的标准写法（含 `::` 压缩）。
///
/// 刻意**不**支持 `%zone`（作用域 ID）：带 zone 的地址只在特定接口内有意义，
/// 而扫描目标应该是无歧义的全局地址。
pub fn parse_ipv6(input: &str) -> CoreResult<Ipv6Addr> {
    let text = input.trim();
    if text.is_empty() {
        return Err(CoreError::InvalidPrefix("IPv6 地址为空".into()));
    }
    if text.contains('%') {
        return Err(CoreError::InvalidPrefix(format!(
            "{text:?} 含作用域 ID（%）。扫描目标请用不带 zone 的地址"
        )));
    }
    text.parse::<Ipv6Addr>()
        .map_err(|_| CoreError::InvalidPrefix(format!("{text:?} 不是合法的 IPv6 地址")))
}

/// 通用地址解析：先按 IPv4 试，再按 IPv6 试，最后拒绝。
pub fn parse_addr(input: &str) -> CoreResult<IpAddr> {
    let text = input.trim();
    if text.contains(':') {
        return parse_ipv6(text).map(IpAddr::V6);
    }
    parse_ipv4(text).map(IpAddr::V4)
}

fn parse_cidr(addr_text: &str, prefix_text: &str) -> CoreResult<TokenBlock> {
    let addr = parse_ipv4(addr_text.trim())?;
    let len_text = prefix_text.trim();
    if len_text.is_empty() || !len_text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(CoreError::InvalidPrefix(format!(
            "CIDR 掩码 {len_text:?} 不是数字"
        )));
    }
    let len: u32 = len_text
        .parse()
        .map_err(|_| CoreError::InvalidPrefix(format!("CIDR 掩码 {len_text:?} 无法解析")))?;
    if len > 32 {
        return Err(CoreError::InvalidPrefix(format!(
            "CIDR 掩码 /{len} 超出 0-32"
        )));
    }

    let bits = ipv4_sort_key(addr);
    let mask: u32 = if len == 0 { 0 } else { u32::MAX << (32 - len) };
    if bits & !mask != 0 {
        let network = Ipv4Addr::from(bits & mask);
        return Err(CoreError::InvalidPrefix(format!(
            "{addr_text}/{len} 的主机位不为 0；网段应写作 {network}/{len}"
        )));
    }
    let network = bits & mask;
    let broadcast = network | !mask;

    let mut targets = Vec::new();
    match len {
        // /32 就是单个地址
        32 => targets.push(IpAddr::V4(addr)),
        // /31 按 RFC 3021 两个地址都可用
        31 => {
            targets.push(IpAddr::V4(Ipv4Addr::from(network)));
            targets.push(IpAddr::V4(Ipv4Addr::from(broadcast)));
        }
        // 常规网段：排除网络号与广播地址
        _ => {
            if broadcast - network - 1 > MAX_TARGETS as u32 {
                return Err(CoreError::TooManyTargets {
                    actual: (broadcast - network - 1) as usize,
                    limit: MAX_TARGETS,
                });
            }
            for raw in (network + 1)..broadcast {
                targets.push(IpAddr::V4(Ipv4Addr::from(raw)));
            }
        }
    }

    Ok(TokenBlock {
        label: format!("{}/{}", Ipv4Addr::from(network), len),
        targets,
    })
}

/// 完整 IPv4 解析（不复用 `parse_prefix`，因为后者只接受三段）。
pub fn parse_ipv4(input: &str) -> CoreResult<Ipv4Addr> {
    let text = input.trim();
    let parts: Vec<&str> = text.split('.').collect();
    if parts.len() != 4 {
        return Err(CoreError::InvalidPrefix(format!(
            "{text:?} 需要 4 段，实际 {} 段",
            parts.len()
        )));
    }
    let mut octets = [0u8; 4];
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            return Err(CoreError::InvalidPrefix(format!(
                "{text:?} 第 {} 段为空",
                i + 1
            )));
        }
        if !part.bytes().all(|b| b.is_ascii_digit()) {
            return Err(CoreError::InvalidPrefix(format!(
                "{text:?} 第 {} 段含非数字",
                i + 1
            )));
        }
        if part.len() > 1 && part.starts_with('0') {
            return Err(CoreError::InvalidPrefix(format!(
                "{text:?} 第 {} 段含前导零",
                i + 1
            )));
        }
        let value: u32 = part
            .parse()
            .map_err(|_| CoreError::InvalidPrefix(format!("{text:?} 第 {} 段无法解析", i + 1)))?;
        if value > 255 {
            return Err(CoreError::InvalidPrefix(format!(
                "{text:?} 第 {} 段超出 0-255",
                i + 1
            )));
        }
        octets[i] = value as u8;
    }
    Ok(Ipv4Addr::new(octets[0], octets[1], octets[2], octets[3]))
}

/// 便捷入口：从一个 C 类前缀构造规格（保持 M1–M8 的调用方式不变）。
pub fn spec_from_prefix(prefix: &Ipv4Prefix, range: TargetRange) -> CoreResult<TargetSpec> {
    Ok(TargetSpec::single(
        prefix.to_string(),
        build_targets(prefix, range)?
            .into_iter()
            .map(IpAddr::V4)
            .collect(),
    ))
}

fn tokenize(input: &str) -> Vec<&str> {
    input
        .split(|c: char| c == ',' || c == ';' || c.is_whitespace())
        .filter(|t| !t.is_empty())
        .collect()
}

/// 去重/排序键：`(族, 数值)`。族 0 = IPv4、1 = IPv6，
/// 这样「先 v4 后 v6」的顺序在任何平台上都稳定。
pub fn addr_sort_key(ip: IpAddr) -> (u8, u128) {
    match ip {
        IpAddr::V4(v4) => (0, ipv4_sort_key(v4) as u128),
        IpAddr::V6(v6) => (1, u128::from(v6)),
    }
}

const fn ipv4_sort_key(ip: Ipv4Addr) -> u32 {
    let o = ip.octets();
    ((o[0] as u32) << 24) | ((o[1] as u32) << 16) | ((o[2] as u32) << 8) | (o[3] as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hosts(spec: &TargetSpec) -> Vec<String> {
        spec.targets.iter().map(|ip| ip.to_string()).collect()
    }

    #[test]
    fn plain_prefix_still_produces_the_original_255_targets() {
        let spec = parse_spec("192.168.1", TargetRange::Standard).unwrap();
        assert_eq!(spec.len(), 255);
        assert_eq!(hosts(&spec)[0], "192.168.1.1");
        assert_eq!(hosts(&spec)[254], "192.168.1.255");
        assert_eq!(spec.segments.len(), 1);
        assert_eq!(spec.segments[0].start, 0);
        assert_eq!(spec.segments[0].end(), 255);
    }

    #[test]
    fn cidr_24_excludes_network_and_broadcast() {
        let spec = parse_spec("192.168.1.0/24", TargetRange::Standard).unwrap();
        assert_eq!(spec.len(), 254);
        assert_eq!(hosts(&spec)[0], "192.168.1.1");
        assert_eq!(hosts(&spec)[253], "192.168.1.254");
        assert_eq!(spec.segments[0].label, "192.168.1.0/24");
    }

    #[test]
    fn cidr_with_host_bits_set_is_rejected_with_the_correct_network() {
        let err = parse_spec("192.168.1.5/24", TargetRange::Standard).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("192.168.1.0/24"), "{text}");
    }

    #[test]
    fn cidr_30_has_two_usable_hosts() {
        let spec = parse_spec("10.0.0.0/30", TargetRange::Standard).unwrap();
        assert_eq!(hosts(&spec), vec!["10.0.0.1", "10.0.0.2"]);
    }

    #[test]
    fn cidr_31_and_32_follow_rfc3021() {
        let s31 = parse_spec("10.0.0.0/31", TargetRange::Standard).unwrap();
        assert_eq!(hosts(&s31), vec!["10.0.0.0", "10.0.0.1"]);
        let s32 = parse_spec("10.0.0.9/32", TargetRange::Standard).unwrap();
        assert_eq!(hosts(&s32), vec!["10.0.0.9"]);
    }

    #[test]
    fn single_host_produces_one_target() {
        let spec = parse_spec("192.168.1.7", TargetRange::Standard).unwrap();
        assert_eq!(hosts(&spec), vec!["192.168.1.7"]);
        assert_eq!(spec.segments[0].label, "192.168.1.7");
    }

    #[test]
    fn multiple_segments_keep_input_order_and_contiguous_ranges() {
        let spec = parse_spec(
            "192.168.1, 192.168.31.0/30, 10.0.0.5",
            TargetRange::Standard,
        )
        .unwrap();
        assert_eq!(spec.segments.len(), 3);
        assert_eq!(spec.segments[0].start, 0);
        assert_eq!(spec.segments[0].count, 255);
        assert_eq!(spec.segments[1].start, 255);
        assert_eq!(spec.segments[1].count, 2);
        assert_eq!(spec.segments[2].start, 257);
        assert_eq!(spec.segments[2].count, 1);
        assert_eq!(spec.len(), 258);
        // 每段的区间必须首尾相接、覆盖整个列表
        let mut cursor = 0;
        for seg in &spec.segments {
            assert_eq!(seg.start, cursor);
            cursor = seg.end();
        }
        assert_eq!(cursor, spec.len());
    }

    #[test]
    fn separators_can_be_commas_semicolons_or_newlines() {
        let spec = parse_spec(
            "10.0.0.1\n10.0.0.2;10.0.0.3 10.0.0.4",
            TargetRange::Standard,
        )
        .unwrap();
        assert_eq!(
            hosts(&spec),
            vec!["10.0.0.1", "10.0.0.2", "10.0.0.3", "10.0.0.4"]
        );
    }

    #[test]
    fn duplicates_are_kept_once_and_segments_stay_contiguous() {
        let spec = parse_spec("192.168.1.5, 192.168.1.0/24", TargetRange::Standard).unwrap();
        // 254 个（/24 去掉网络与广播）而不是 255
        assert_eq!(spec.len(), 254);
        assert_eq!(spec.segments[0].count, 1);
        assert_eq!(spec.segments[1].count, 253);
        assert_eq!(spec.segments[1].end(), spec.len(), "去重后区间仍必须连续");
        let mut sorted = spec.targets.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), spec.len(), "结果里不得有重复目标");
    }

    #[test]
    fn range_strategy_applies_to_plain_prefixes_only() {
        let full = parse_spec("192.168.1", TargetRange::Full).unwrap();
        assert_eq!(full.len(), 256);
        let hosts_only = parse_spec("192.168.1", TargetRange::HostsOnly).unwrap();
        assert_eq!(hosts_only.len(), 254);
        // CIDR 有自己的语义，不受 range 影响
        let cidr = parse_spec("192.168.1.0/24", TargetRange::Full).unwrap();
        assert_eq!(cidr.len(), 254);
    }

    #[test]
    fn empty_and_malformed_input_is_rejected() {
        for bad in [
            "",
            "   ",
            "\n",
            ",",
            "192.168.1.1/24/2",
            "192.168.1/24",
            "1.2.3.4.5",
            "abc.def",
        ] {
            assert!(
                parse_spec(bad, TargetRange::Standard).is_err(),
                "{bad:?} 应被拒绝"
            );
        }
    }

    #[test]
    fn cidr_mask_must_be_numeric_and_in_range() {
        assert!(parse_spec("10.0.0.0/abc", TargetRange::Standard).is_err());
        assert!(parse_spec("10.0.0.0/33", TargetRange::Standard).is_err());
        assert!(parse_spec("10.0.0.0/", TargetRange::Standard).is_err());
    }

    #[test]
    fn oversized_cidr_is_rejected_by_the_target_limit() {
        let err = parse_spec("10.0.0.0/8", TargetRange::Standard).unwrap_err();
        assert!(matches!(err, CoreError::TooManyTargets { .. }), "{err:?}");
    }

    #[test]
    fn matrix_compatibility_follows_the_256_cell_rule() {
        assert!(parse_spec("192.168.1", TargetRange::Standard)
            .unwrap()
            .matrix_compatible());
        assert!(parse_spec("192.168.1", TargetRange::Full)
            .unwrap()
            .matrix_compatible());
        assert!(!parse_spec("10.0.0.0/23", TargetRange::Standard)
            .unwrap()
            .matrix_compatible());
        assert!(!TargetSpec::default().matrix_compatible());
    }

    #[test]
    fn summary_and_span_text_are_human_readable() {
        let one = parse_spec("192.168.1", TargetRange::Standard).unwrap();
        assert_eq!(one.summary(), "192.168.1（255 个）");
        assert_eq!(one.span_text().unwrap(), "192.168.1.1 – 192.168.1.255");

        let many = parse_spec("10.0.0.1, 10.0.0.2", TargetRange::Standard).unwrap();
        assert_eq!(many.summary(), "2 个网段 / 2 个目标");
        assert_eq!(many.span_text().unwrap(), "10.0.0.1 – 10.0.0.2");

        let single = parse_spec("10.0.0.1", TargetRange::Standard).unwrap();
        assert_eq!(single.span_text().unwrap(), "10.0.0.1");

        assert_eq!(TargetSpec::default().summary(), "尚未设置目标");
        assert!(TargetSpec::default().span_text().is_none());
    }

    #[test]
    fn probe_mode_semantics_are_stable() {
        assert_eq!(ProbeMode::default(), ProbeMode::Icmp);
        assert_eq!(ProbeMode::Icmp.describe(), "ICMP");
        assert_eq!(ProbeMode::Tcp { port: 80 }.describe(), "TCP:80");
        assert_eq!(ProbeMode::Tcp { port: 80 }.tcp_port(), Some(80));
        assert!(ProbeMode::Tcp { port: 80 }.is_tcp());
        assert_eq!(ProbeMode::Tcp { port: 80 }.status_prefix(), "TCP-OPEN ");
        assert_eq!(ProbeMode::Icmp.status_prefix(), "");
        assert!(ProbeMode::Icmp.validate().is_ok());
        assert!(ProbeMode::Tcp { port: 0 }.validate().is_err());
        assert!(ProbeMode::Tcp { port: 443 }.validate().is_ok());
        assert_eq!(ProbeMode::Tcp { port: 80 }.backend_name(), "tcp-connect");
    }

    #[test]
    fn spec_from_prefix_matches_the_legacy_helper() {
        let prefix = parse_prefix("172.16.5").unwrap();
        let spec = spec_from_prefix(&prefix, TargetRange::Standard).unwrap();
        assert_eq!(spec.len(), 255);
        assert_eq!(spec.segments[0].label, "172.16.5");
    }

    // -----------------------------------------------------------------------
    // M12：IPv6 目标
    // -----------------------------------------------------------------------

    #[test]
    fn ipv6_single_host_is_accepted() {
        let spec = parse_spec("::1", TargetRange::Standard).unwrap();
        assert_eq!(hosts(&spec), vec!["::1"]);
        assert!(spec.has_ipv6());
        assert_eq!(spec.segments[0].label, "::1");
    }

    #[test]
    fn ipv6_cidr_128_is_one_address_and_127_is_two() {
        let one = parse_spec("fd00::1/128", TargetRange::Standard).unwrap();
        assert_eq!(hosts(&one), vec!["fd00::1"]);

        let two = parse_spec("fd00::/127", TargetRange::Standard).unwrap();
        assert_eq!(hosts(&two), vec!["fd00::", "fd00::1"]);
    }

    #[test]
    fn ipv6_cidr_excludes_network_and_last_when_large_enough() {
        let spec = parse_spec("fd00::/126", TargetRange::Standard).unwrap();
        assert_eq!(hosts(&spec), vec!["fd00::1", "fd00::2"]);
        assert_eq!(spec.segments[0].label, "fd00::/126");
    }

    #[test]
    fn ipv6_cidr_with_host_bits_set_is_rejected_with_the_correct_network() {
        // fd00::2 落在 fd00::/126 里（fd00::0–fd00::3），正确写法应被指出
        let err = parse_spec("fd00::2/126", TargetRange::Standard).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("fd00::/126"), "{text}");

        // 换一个网段：fd00::5 属于 fd00::4/126
        let err = parse_spec("fd00::5/126", TargetRange::Standard).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("fd00::4/126"), "{text}");
    }

    #[test]
    fn oversized_ipv6_cidr_is_rejected_instead_of_exploding() {
        // /64 展开后有 2^64 个地址：不是「扫不完」，而是本来就不该这么扫。
        let err = parse_spec("fd00::/64", TargetRange::Standard).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("超过上限"), "{text}");
        assert!(parse_spec("::/0", TargetRange::Standard).is_err());
    }

    #[test]
    fn ipv6_invalid_forms_are_rejected() {
        for bad in [
            "fd00::1/129",
            "fd00::1/abc",
            "fe80::1%eth0",
            "::gggg",
            "fd00::1/",
        ] {
            assert!(
                parse_spec(bad, TargetRange::Standard).is_err(),
                "{bad:?} 应被拒绝"
            );
        }
    }

    #[test]
    fn ipv6_and_ipv4_targets_coexist_and_dedupe() {
        let spec = parse_spec("10.0.0.1, ::1, 10.0.0.1", TargetRange::Standard).unwrap();
        assert_eq!(hosts(&spec), vec!["10.0.0.1", "::1"]);
        assert_eq!(spec.segments.len(), 3);
        // 重复目标被跳过，但区间仍然连续覆盖整个列表
        assert_eq!(spec.segments[0].count, 1);
        assert_eq!(spec.segments[1].count, 1);
        assert_eq!(spec.segments[2].count, 0);
        let mut cursor = 0;
        for seg in &spec.segments {
            assert_eq!(seg.start, cursor);
            cursor = seg.end();
        }
        assert_eq!(cursor, spec.len());
    }

    #[test]
    fn ipv6_forces_matrix_incompatibility() {
        // 单个 v6 目标即使数量远小于 256，也不能用矩阵表示
        let spec = parse_spec("::1", TargetRange::Standard).unwrap();
        assert!(!spec.matrix_compatible());
        assert!(spec.has_ipv6());
        // 纯 v4 仍然可用
        assert!(parse_spec("192.168.1.1", TargetRange::Standard)
            .unwrap()
            .matrix_compatible());
    }

    #[test]
    fn addr_sort_key_puts_ipv4_before_ipv6() {
        assert!(addr_sort_key("10.0.0.1".parse().unwrap()) < addr_sort_key("::1".parse().unwrap()));
        assert!(
            addr_sort_key("10.0.0.1".parse().unwrap()) < addr_sort_key("10.0.0.2".parse().unwrap())
        );
        assert!(
            addr_sort_key("fd00::1".parse().unwrap()) < addr_sort_key("fd00::2".parse().unwrap())
        );
    }

    #[test]
    fn parse_addr_dispatches_on_colon() {
        assert_eq!(parse_addr("10.0.0.1").unwrap().to_string(), "10.0.0.1");
        assert_eq!(parse_addr(" ::1 ").unwrap().to_string(), "::1");
        assert!(parse_addr("10.0.0").is_err());
    }
}
