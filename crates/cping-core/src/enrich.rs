//! 目标信息补全（M7）：把邻居表 MAC、IEEE 厂商名、按需主机名写回结果模型。
//!
//! 这一层存在的意义是**让 GUI 与 TUI 不各写一遍**：两侧都只调用这里。
//!
//! ## 为什么 MAC/厂商是「扫描后」补，而不是「探测中」补
//!
//! ARP 表只有在**本机已经向目标发过包**之后才可能有对应条目。因此正确顺序是
//! 「先扫完 → 再读邻居表 → 回填」。这也是原版工具的行为：扫描前 ARP 表里
//! 往往什么都没有。
//!
//! ## 为什么主机名是「按需」补
//!
//! DNS 反查是阻塞的（见 [`crate::hostname`]）。批量 255 个目标反查会把
//! 秒级扫描拖成分钟级，因此这里只提供单条接口，由界面在用户点击时调用。

use crate::neighbor::{self, NeighborEntry};
use crate::oui::{self, mac_to_string};
use crate::result::ProbeResult;
use std::collections::HashMap;
use std::net::IpAddr;
use std::time::Duration;

/// 一次补全的结果统计，供界面如实提示（而不是只说「完成」）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnrichReport {
    /// 邻居表是否成功读到（失败时下面的计数必然为 0）。
    pub table_ok: bool,
    /// 邻居表读取失败的原始原因；成功时为 `None`。
    pub error: Option<String>,
    /// 邻居表条目总数。
    pub entries: usize,
    /// 本次新填上的 MAC 条数。
    pub mac_filled: usize,
    /// 本次新填上的厂商名条数（可能少于 MAC，因为存在未注册/本地管理前缀）。
    pub vendor_filled: usize,
}

/// 把邻居表写入结果：**只补空缺，不覆盖已有值**（可安全重复调用）。
///
/// 返回 `(新填 MAC 数, 新填厂商数)`。厂商名由 `oui` 决定，查不到就留空。
pub fn apply_neighbors(results: &mut [ProbeResult], table: &[NeighborEntry]) -> (usize, usize) {
    if table.is_empty() {
        return (0, 0);
    }
    // ARP/邻居表只覆盖 IPv4，因此索引键统一转成 `IpAddr` 后，IPv6 目标查不到
    // 任何条目——这是事实，不是缺陷：v6 目标的 mac 恒为 None。
    let index: HashMap<IpAddr, [u8; 6]> = table.iter().map(|e| (IpAddr::V4(e.ip), e.mac)).collect();
    let (mut mac_filled, mut vendor_filled) = (0, 0);
    for r in results.iter_mut() {
        if r.mac.is_some() {
            continue;
        }
        let Some(mac) = index.get(&r.ip).copied() else {
            continue;
        };
        r.mac = Some(mac_to_string(mac));
        mac_filled += 1;
        if r.vendor.is_none() {
            if let Some(v) = oui::vendor_of(mac) {
                r.vendor = Some(v.to_string());
                vendor_filled += 1;
            }
        }
    }
    (mac_filled, vendor_filled)
}

/// 读平台邻居表并回填 MAC/厂商。这是 GUI/TUI 应当调用的入口。
pub fn enrich_with_mac_and_vendor(results: &mut [ProbeResult]) -> EnrichReport {
    match neighbor::neighbor_table() {
        Ok(table) => {
            let (mac_filled, vendor_filled) = apply_neighbors(results, &table);
            EnrichReport {
                table_ok: true,
                error: None,
                entries: table.len(),
                mac_filled,
                vendor_filled,
            }
        }
        Err(e) => EnrichReport {
            table_ok: false,
            error: Some(e),
            entries: 0,
            mac_filled: 0,
            vendor_filled: 0,
        },
    }
}

/// 只补厂商名（已有 MAC 但缺厂商时用，例如从快照恢复后重新查表）。
pub fn fill_vendors_from_mac(results: &mut [ProbeResult]) -> usize {
    let mut filled = 0;
    for r in results.iter_mut() {
        if r.vendor.is_some() {
            continue;
        }
        let Some(mac_s) = r.mac.as_deref() else {
            continue;
        };
        let Some(mac) = oui::parse_mac(mac_s) else {
            continue;
        };
        if let Some(v) = oui::vendor_of(mac) {
            r.vendor = Some(v.to_string());
            filled += 1;
        }
    }
    filled
}

/// 按需反查单个目标的主机名；返回是否查到。
///
/// 不覆盖已有主机名（重复点击不会因为一次超时而把名字擦掉）。
pub fn lookup_hostname(result: &mut ProbeResult, timeout: Duration) -> bool {
    if result.hostname.is_some() {
        return true;
    }
    match crate::hostname::reverse_lookup(result.ip, timeout) {
        Some(name) => {
            result.hostname = Some(name);
            true
        }
        None => false,
    }
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result::ProbeStatus;

    fn table(entries: &[(&str, [u8; 6])]) -> Vec<NeighborEntry> {
        entries
            .iter()
            .map(|(ip, mac)| NeighborEntry {
                ip: ip.parse().unwrap(),
                mac: *mac,
            })
            .collect()
    }

    #[test]
    fn fills_mac_and_vendor_for_matching_ips() {
        let mut results = vec![
            ProbeResult::new(
                0,
                "192.168.1.1".parse().unwrap(),
                ProbeStatus::Online { rtt_ms: 3 },
            ),
            ProbeResult::new(1, "192.168.1.2".parse().unwrap(), ProbeStatus::Timeout),
        ];
        // 00:00:0C 是真实的 Cisco MA-L 前缀
        let t = table(&[("192.168.1.1", [0x00, 0x00, 0x0C, 0x11, 0x22, 0x33])]);
        let (mac_filled, vendor_filled) = apply_neighbors(&mut results, &t);
        assert_eq!(mac_filled, 1);
        assert_eq!(vendor_filled, 1);
        assert_eq!(results[0].mac.as_deref(), Some("00:00:0C:11:22:33"));
        assert_eq!(results[0].vendor.as_deref(), Some("Cisco Systems, Inc"));
        // 邻居表里没有的目标保持 None，不得编造
        assert_eq!(results[1].mac, None);
        assert_eq!(results[1].vendor, None);
    }

    #[test]
    fn does_not_overwrite_existing_values() {
        let mut results = vec![ProbeResult::new(
            0,
            "10.0.0.1".parse().unwrap(),
            ProbeStatus::Online { rtt_ms: 1 },
        )
        .with_target_info(
            Some("DE:AD:BE:EF:00:01".into()),
            Some("既有厂商".into()),
            Some("既有主机".into()),
        )];
        let t = table(&[("10.0.0.1", [0x00, 0x00, 0x0C, 0x11, 0x22, 0x33])]);
        let (mac_filled, vendor_filled) = apply_neighbors(&mut results, &t);
        assert_eq!((mac_filled, vendor_filled), (0, 0), "已有值不应被覆盖");
        assert_eq!(results[0].mac.as_deref(), Some("DE:AD:BE:EF:00:01"));
        assert_eq!(results[0].vendor.as_deref(), Some("既有厂商"));
    }

    #[test]
    fn local_administered_mac_gets_no_vendor() {
        let mut results = vec![ProbeResult::new(
            0,
            "10.0.0.5".parse().unwrap(),
            ProbeStatus::Online { rtt_ms: 1 },
        )];
        // 首字节 0x02 = 本地管理地址，不属于 IEEE 分配 → 不能编厂商
        let t = table(&[("10.0.0.5", [0x02, 0x11, 0x22, 0x33, 0x44, 0x55])]);
        let (mac_filled, vendor_filled) = apply_neighbors(&mut results, &t);
        assert_eq!(mac_filled, 1);
        assert_eq!(vendor_filled, 0);
        assert_eq!(results[0].mac.as_deref(), Some("02:11:22:33:44:55"));
        assert_eq!(results[0].vendor, None);
    }

    #[test]
    fn empty_table_is_a_noop() {
        let mut results = vec![ProbeResult::new(
            0,
            "10.0.0.5".parse().unwrap(),
            ProbeStatus::Timeout,
        )];
        let before = results.clone();
        assert_eq!(apply_neighbors(&mut results, &[]), (0, 0));
        assert_eq!(results, before);
    }

    #[test]
    fn fill_vendors_from_existing_mac() {
        let mut results = vec![ProbeResult::new(
            0,
            "10.0.0.9".parse().unwrap(),
            ProbeStatus::Online { rtt_ms: 1 },
        )
        .with_target_info(Some("00:00:0C:AA:BB:CC".into()), None, None)];
        assert_eq!(fill_vendors_from_mac(&mut results), 1);
        assert_eq!(results[0].vendor.as_deref(), Some("Cisco Systems, Inc"));
        // 再调用一次不应重复计数
        assert_eq!(fill_vendors_from_mac(&mut results), 0);
    }

    #[test]
    fn fill_vendors_ignores_unparsable_mac() {
        let mut results = vec![ProbeResult::new(
            0,
            "10.0.0.9".parse().unwrap(),
            ProbeStatus::Online { rtt_ms: 1 },
        )
        .with_target_info(Some("not-a-mac".into()), None, None)];
        assert_eq!(fill_vendors_from_mac(&mut results), 0);
        assert_eq!(results[0].vendor, None);
    }

    #[test]
    fn hostname_lookup_is_not_overwritten() {
        let mut r = ProbeResult::new(0, "10.0.0.9".parse().unwrap(), ProbeStatus::Timeout)
            .with_target_info(None, None, Some("keep.me".into()));
        // 已有主机名时直接返回 true，不做任何网络访问
        assert!(lookup_hostname(&mut r, Duration::from_millis(1)));
        assert_eq!(r.hostname.as_deref(), Some("keep.me"));
    }

    #[test]
    fn hostname_lookup_on_unresolvable_returns_false() {
        let mut r = ProbeResult::new(
            0,
            "192.0.2.99".parse().unwrap(), // RFC 5737 TEST-NET-1
            ProbeStatus::Timeout,
        );
        assert!(!lookup_hostname(&mut r, Duration::from_millis(800)));
        assert_eq!(r.hostname, None, "查不到必须是 None，不能填占位");
    }
}
