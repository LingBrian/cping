//! 本机地址枚举。
//!
//! 本阶段只定义接口与**不做任何猜测**的占位实现：
//! 平台相关的网卡枚举在 M5 接入，这里不允许返回硬编码或伪造的地址。

use crate::error::{CoreError, CoreResult};
use std::net::Ipv4Addr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalAddress {
    pub ip: Ipv4Addr,
    pub is_loopback: bool,
    /// 是否为 RFC1918 / CGNAT 等私有地址。
    pub is_private: bool,
    /// 可选的网卡名或描述（平台相关）。
    pub label: Option<String>,
}

impl LocalAddress {
    pub fn new(ip: Ipv4Addr, label: Option<String>) -> Self {
        Self {
            is_loopback: ip.is_loopback(),
            is_private: is_private_v4(ip),
            ip,
            label,
        }
    }
}

/// 判断私有 IPv4 地址（RFC1918 + link-local + CGNAT）。
pub fn is_private_v4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    o[0] == 10
        || (o[0] == 172 && (16..=31).contains(&o[1]))
        || (o[0] == 192 && o[1] == 168)
        || (o[0] == 169 && o[1] == 254)
        || (o[0] == 100 && (64..=127).contains(&o[1]))
}

pub trait LocalAddressProvider {
    fn list(&self) -> CoreResult<Vec<LocalAddress>>;
}

/// 尚未接入网卡枚举的平台使用它：**明确返回不支持，而不是猜一个地址**。
#[derive(Debug, Default, Clone, Copy)]
pub struct UnsupportedProvider;

impl LocalAddressProvider for UnsupportedProvider {
    fn list(&self) -> CoreResult<Vec<LocalAddress>> {
        Err(CoreError::Unsupported(
            "本平台尚未接入网卡枚举（M5 接入）".into(),
        ))
    }
}

/// 从候选地址中挑选「最可能代表本机」的一个：优先非回环且私有。
pub fn pick_primary(addrs: &[LocalAddress]) -> Option<&LocalAddress> {
    addrs
        .iter()
        .find(|a| !a.is_loopback && a.is_private)
        .or_else(|| addrs.iter().find(|a| !a.is_loopback))
        .or_else(|| addrs.first())
}
