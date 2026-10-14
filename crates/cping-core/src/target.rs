//! 前缀解析、目标范围生成与 16×16 矩阵索引。
//!
//! 矩阵索引规则来自 M0.5 动态复核结论（见 `docs/reverse-findings.md` §5.2）：
//! 原版网格为 16 列 × 16 行 = 256 格，格内显示的数字就是 **IP 的最后一段**，
//! 即 `格号 = row * 16 + col`，且 `格号 == host`。
//! 由于原版目标范围是 `.1`–`.255`，`格号 0`（对应 `.0`）**没有目标**，恒为空。

use crate::error::{CoreError, CoreResult};
use std::net::{IpAddr, Ipv4Addr};
use std::ops::RangeInclusive;

/// 目标数量的硬上限：防止误扫超出预期的网络。
///
/// M9 由 1024 提升到 65536（`/16` 级别批量扫描）；**矩阵视图另有 256 的上限**
/// （见 [`crate::spec::MATRIX_VIEW_MAX`]），那是表示能力限制，不是性能限制。
pub const MAX_TARGETS: usize = 65536;

/// C 类网段前缀（前三段），例如 `192.168.1`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ipv4Prefix {
    pub a: u8,
    pub b: u8,
    pub c: u8,
}

impl Ipv4Prefix {
    pub const fn new(a: u8, b: u8, c: u8) -> Self {
        Self { a, b, c }
    }

    /// 用给定的主机段拼出完整地址。
    pub const fn with_host(self, host: u8) -> Ipv4Addr {
        Ipv4Addr::new(self.a, self.b, self.c, host)
    }
}

impl std::fmt::Display for Ipv4Prefix {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.a, self.b, self.c)
    }
}

/// 目标范围策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TargetRange {
    /// `.1` – `.255`，共 255 个。**默认值，对齐原版行为**。
    #[default]
    Standard,
    /// `.0` – `.255`，共 256 个。仅用于对照/测试，默认不启用。
    Full,
    /// `.1` – `.254`，共 254 个（排除网络地址与广播地址）。
    HostsOnly,
}

impl TargetRange {
    pub const fn hosts(self) -> RangeInclusive<u8> {
        match self {
            TargetRange::Standard => 1..=255,
            TargetRange::Full => 0..=255,
            TargetRange::HostsOnly => 1..=254,
        }
    }

    pub const fn host_count(self) -> usize {
        match self {
            TargetRange::Standard => 255,
            TargetRange::Full => 256,
            TargetRange::HostsOnly => 254,
        }
    }

    pub const fn key(self) -> &'static str {
        match self {
            TargetRange::Standard => "standard",
            TargetRange::Full => "full",
            TargetRange::HostsOnly => "hosts_only",
        }
    }
}

/// 解析网段前缀。
///
/// 接受的输入：`192.168.1`、` 10.0.0 `（两端空白会被裁剪）。
/// 拒绝的输入：空串、段数不是 3、含非数字、含前导零（如 `01.2.3`）、任一段 > 255。
pub fn parse_prefix(input: &str) -> CoreResult<Ipv4Prefix> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(CoreError::InvalidPrefix("输入为空".into()));
    }

    let parts: Vec<&str> = trimmed.split('.').collect();
    if parts.len() != 3 {
        return Err(CoreError::InvalidPrefix(format!(
            "需要 3 段（如 192.168.1），实际 {} 段",
            parts.len()
        )));
    }

    let mut octets = [0u8; 3];
    for (i, part) in parts.iter().enumerate() {
        octets[i] = parse_octet(part).map_err(|reason| {
            CoreError::InvalidPrefix(format!("第 {} 段 {part:?} {reason}", i + 1))
        })?;
    }

    Ok(Ipv4Prefix::new(octets[0], octets[1], octets[2]))
}

fn parse_octet(part: &str) -> Result<u8, String> {
    if part.is_empty() {
        return Err("为空".into());
    }
    if !part.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("含非数字字符 {part:?}"));
    }
    if part.len() > 1 && part.starts_with('0') {
        return Err("含前导零".into());
    }
    if part.len() > 3 {
        return Err("超出 0-255".into());
    }
    let value: u32 = part.parse().map_err(|_| "无法解析为数字".to_string())?;
    if value > 255 {
        return Err("超出 0-255".into());
    }
    Ok(value as u8)
}

/// 按策略生成目标列表（按主机段升序）。
pub fn build_targets(prefix: &Ipv4Prefix, range: TargetRange) -> CoreResult<Vec<Ipv4Addr>> {
    let count = range.host_count();
    if count > MAX_TARGETS {
        return Err(CoreError::TooManyTargets {
            actual: count,
            limit: MAX_TARGETS,
        });
    }
    Ok(range.hosts().map(|h| prefix.with_host(h)).collect())
}

// ---------------------------------------------------------------------------
// 16×16 矩阵索引
// ---------------------------------------------------------------------------

pub const MATRIX_COLS: usize = 16;
pub const MATRIX_ROWS: usize = 16;
pub const MATRIX_CELLS: usize = MATRIX_COLS * MATRIX_ROWS; // 256

/// 格号 → (row, col)。越界返回 `None`。
pub const fn cell_position(index: usize) -> Option<(usize, usize)> {
    if index >= MATRIX_CELLS {
        return None;
    }
    Some((index / MATRIX_COLS, index % MATRIX_COLS))
}

/// (row, col) → 格号。越界返回 `None`。
pub const fn cell_index(row: usize, col: usize) -> Option<usize> {
    if row >= MATRIX_ROWS || col >= MATRIX_COLS {
        return None;
    }
    Some(row * MATRIX_COLS + col)
}

/// 原版语义：格号 == 目标 IP 的最后一段。
///
/// 因此 `.1` 落在 `格号 1`，`.255` 落在 `格号 255`，`格号 0`（`.0`）没有目标。
pub const fn cell_of_host(host: u8) -> usize {
    host as usize
}

/// 目标 IP → 格号（第 4 段）。**仅对 IPv4 有意义**。
pub const fn cell_of_ip(ip: Ipv4Addr) -> usize {
    cell_of_host(ip.octets()[3])
}

/// [`cell_of_ip`] 的通用版本：IPv6 目标没有「最后一跳 = 格号」语义，
/// 因此返回 `None`，由调用方退化为列表视图。
pub const fn cell_of_addr(ip: IpAddr) -> Option<usize> {
    match ip {
        IpAddr::V4(v4) => Some(cell_of_ip(v4)),
        IpAddr::V6(_) => None,
    }
}

/// 「可以当作目标地址」的类型。存在的唯一理由是让 `matrix_slots`
/// 同时接受 `&[IpAddr]`（M12 之后的目标列表）和 `&[Ipv4Addr]`（历史调用点），
/// 不必在每个调用处手动转换。
pub trait AsTargetAddr {
    fn as_target_addr(&self) -> IpAddr;
}

impl AsTargetAddr for IpAddr {
    fn as_target_addr(&self) -> IpAddr {
        *self
    }
}

impl AsTargetAddr for Ipv4Addr {
    fn as_target_addr(&self) -> IpAddr {
        IpAddr::V4(*self)
    }
}

/// 给定一个目标列表，返回 (格号, 目标下标) 映射；`格号 0` 通常为空。
///
/// 返回长度固定为 `MATRIX_CELLS`，元素为 `Option<usize>`（目标在列表中的下标）。
/// IPv6 目标不占格位——它们没有「最后一跳 = 格号」的对应关系，
/// 此时矩阵不完整，界面必须切到表格视图（见 [`crate::spec::TargetSpec::matrix_compatible`]）。
pub fn matrix_slots<T: AsTargetAddr>(targets: &[T]) -> Vec<Option<usize>> {
    let mut slots = vec![None; MATRIX_CELLS];
    for (idx, ip) in targets.iter().enumerate() {
        if let Some(cell) = cell_of_addr(ip.as_target_addr()) {
            if cell < MATRIX_CELLS {
                slots[cell] = Some(idx);
            }
        }
    }
    slots
}
