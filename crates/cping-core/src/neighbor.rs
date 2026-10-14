//! 邻居表（ARP）→ 局域网内 `IP ↔ MAC` 对应关系（M7）。
//!
//! 两个平台路径，**解析逻辑与取数逻辑分离**，解析部分是纯函数便于单测：
//! - Windows：`iphlpapi!GetIpNetTable2(AF_INET)` + `FreeMibTable`（手写 FFI，零依赖）；
//! - Linux / Android：读 `/proc/net/arp`。
//!
//! ## 取不到时的行为
//!
//! 一律**留空**，绝不用网关 MAC 或 `FF:FF:FF:FF:FF:FF` 之类顶替。ARP 表本身
//! 也只在**近期有过二层交互**时才有条目，因此「在线但拿不到 MAC」是正常现象，
//! 不是错误——界面应显示为空而不是报错。
//!
//! ## 何时补 MAC
//!
//! ARP 表只有在扫描**已经发包**之后才可能填上目标条目，所以补 MAC 必须发生在
//! 扫描结束之后（见 [`crate::enrich`]），而不是探测过程中。

use crate::oui::parse_mac;
use std::net::Ipv4Addr;

/// 一条邻居（ARP）记录。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NeighborEntry {
    pub ip: Ipv4Addr,
    pub mac: [u8; 6],
}

/// 解析 `/proc/net/arp`。
///
/// 只接受「完整」（`Flags` 含 `0x2`，即 `ATF_COM`）、以太网类型、MAC 非全零的条目；
/// 表头与畸形行直接跳过。同一 IP 出现多次时保留**最后一条**（内核会原地更新）。
pub fn parse_proc_net_arp(text: &str) -> Vec<NeighborEntry> {
    const ATF_COM: u32 = 0x2;
    let mut out: Vec<NeighborEntry> = Vec::new();
    for line in text.lines() {
        let mut it = line.split_whitespace();
        let (Some(ip_s), Some(hw_type), Some(flags), Some(mac_s)) =
            (it.next(), it.next(), it.next(), it.next())
        else {
            continue; // 表头或列数不足
        };
        if ip_s == "IP" {
            continue; // 表头
        }
        let Ok(ip) = ip_s.parse::<Ipv4Addr>() else {
            continue;
        };
        // HW type 0x1 = ARPHRD_ETHER；其它类型（如 0x20 InfiniBand）不按 MAC 解释
        let Ok(hw) = u32::from_str_radix(hw_type.trim_start_matches("0x"), 16) else {
            continue;
        };
        if hw != 0x1 {
            continue;
        }
        let Ok(flags_v) = u32::from_str_radix(flags.trim_start_matches("0x"), 16) else {
            continue;
        };
        if flags_v & ATF_COM == 0 {
            continue; // 未完成/无效条目
        }
        let Some(mac) = parse_mac(mac_s) else {
            continue;
        };
        if !is_unicast_mac(mac) {
            continue; // 占位零地址或组播/广播地址，不是主机身份
        }
        if let Some(slot) = out.iter_mut().find(|e| e.ip == ip) {
            *slot = NeighborEntry { ip, mac };
        } else {
            out.push(NeighborEntry { ip, mac });
        }
    }
    out
}

/// 该 MAC 是否可以当作「某台主机的链路层地址」。
///
/// 只接受**单播**且非全零的 MAC：
/// - 组播位（首字节 `0x01`）置位的地址（`01:00:5E:..`、`FF:FF:FF:FF:FF:FF`）
///   是组播/广播身份，**不是主机身份**；
/// - 全零是占位。
///
/// 这条规则来自实测：Windows 的 ARP 缓存里会存在子网广播地址条目
/// （如 `192.168.0.255 → FF:FF:FF:FF:FF:FF`），而 `.255` 本身就是我们会扫描的
/// 目标之一。若不拦住，界面会在 `192.168.0.255` 这一行显示广播 MAC，
/// 让人误以为那里有台机器。
pub fn is_unicast_mac(mac: [u8; 6]) -> bool {
    mac != [0u8; 6] && mac[0] & 0x01 == 0
}

/// 从邻居表条目里提取可用 MAC：必须是 IPv4、长度 6、单播且非全零。
///
/// 单独抽出来是为了能在非 Windows 环境覆盖测试。
pub fn accept_ipv4_entry(
    family: u16,
    ip_octets: [u8; 4],
    mac_len: u32,
    mac: &[u8],
) -> Option<NeighborEntry> {
    const AF_INET: u16 = 2;
    if family != AF_INET || mac_len != 6 || mac.len() < 6 {
        return None;
    }
    let mut m = [0u8; 6];
    m.copy_from_slice(&mac[..6]);
    if !is_unicast_mac(m) {
        return None;
    }
    Some(NeighborEntry {
        ip: Ipv4Addr::from(ip_octets),
        mac: m,
    })
}

// ---------------------------------------------------------------------------
// Windows：GetIpNetTable2
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod iphlpapi {
    /// `SOCKADDR_INET` 是不超过 28 字节的联合体（`SOCKADDR_IN` 16 字节 /
    /// `SOCKADDR_IN6` 28 字节）。
    ///
    /// 这里刻意把它拆成**具名字段**而不是 `[u32; 7]`：`sin_addr` 必须按
    /// **字节**读取。若把它读成 `u32`，小端机器上会得到反序的 IP
    /// （例如 192.168.0.1 变成 1.0.168.192）——这是实测踩到的坑。
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct SockAddrInet {
        pub sin_family: u16,
        pub sin_port: u16,
        /// 网络序的 4 个字节，**直接就是 octets**。
        pub sin_addr: [u8; 4],
        pub sin_zero: [u8; 8],
        /// 补齐到 `SOCKADDR_IN6` 的 28 字节。
        pub sin6_tail: [u32; 3],
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct MibIpNetRow2 {
        pub address: SockAddrInet,
        pub interface_index: u32,
        pub interface_luid: u64,
        pub physical_address: [u8; 32],
        pub physical_address_length: u32,
        pub state: i32,
        pub flags: u32,
        pub reachability_time: u32,
    }

    /// `MIB_IPNET_ROW2.PhysicalAddress` 的容量（`IF_MAX_PHYS_ADDRESS_LENGTH`）。
    pub const PHYS_ADDR_LEN: usize = 32;

    #[link(name = "iphlpapi")]
    extern "system" {
        pub fn GetIpNetTable2(family: u16, table: *mut *mut u8) -> u32;
        pub fn FreeMibTable(memory: *mut core::ffi::c_void);
    }
}

#[cfg(windows)]
fn windows_neighbor_table() -> Result<Vec<NeighborEntry>, String> {
    use iphlpapi::{MibIpNetRow2, PHYS_ADDR_LEN};
    use std::mem::{align_of, size_of};

    const AF_INET: u16 = 2;
    const NO_ERROR: u32 = 0;
    const ERROR_NOT_SUPPORTED: u32 = 50;

    let mut raw: *mut u8 = std::ptr::null_mut();
    let rc = unsafe { iphlpapi::GetIpNetTable2(AF_INET, &mut raw as *mut *mut u8) };
    if rc != NO_ERROR {
        return Err(match rc {
            ERROR_NOT_SUPPORTED => "IP 协议栈不可用（GetIpNetTable2 返回 50）".to_string(),
            other => format!("GetIpNetTable2 返回错误码 {other}"),
        });
    }
    if raw.is_null() {
        return Ok(Vec::new());
    }
    // 从 GetIpNetTable2 拿到的内存必须用 FreeMibTable 释放
    struct TableGuard(*mut u8);
    impl Drop for TableGuard {
        fn drop(&mut self) {
            unsafe { iphlpapi::FreeMibTable(self.0 as *mut core::ffi::c_void) }
        }
    }
    let _guard = TableGuard(raw);

    // 布局：ULONG NumEntries 之后按 MIB_IPNET_ROW2 的对齐补齐，再是行数组
    let num = unsafe { std::ptr::read_unaligned(raw as *const u32) } as usize;
    let row_align = align_of::<MibIpNetRow2>();
    let header = size_of::<u32>();
    let rows_off = header.next_multiple_of(row_align);
    let rows = unsafe { raw.add(rows_off) } as *const MibIpNetRow2;

    let mut out: Vec<NeighborEntry> = Vec::with_capacity(num);
    for i in 0..num {
        let row = unsafe { std::ptr::read_unaligned(rows.add(i)) };
        // 地址按**字节**读取：sin_addr 在内存里就是网络序的 4 个 octet
        let ip_octets = row.address.sin_addr;
        let mac = &row.physical_address[..PHYS_ADDR_LEN];
        if let Some(entry) = accept_ipv4_entry(
            row.address.sin_family,
            ip_octets,
            row.physical_address_length,
            mac,
        ) {
            if let Some(slot) = out.iter_mut().find(|e| e.ip == entry.ip) {
                *slot = entry;
            } else {
                out.push(entry);
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Linux / Android：/proc/net/arp
// ---------------------------------------------------------------------------

#[cfg(any(target_os = "linux", target_os = "android"))]
fn proc_neighbor_table() -> Result<Vec<NeighborEntry>, String> {
    match std::fs::read_to_string("/proc/net/arp") {
        Ok(text) => Ok(parse_proc_net_arp(&text)),
        Err(e) => Err(format!("读取 /proc/net/arp 失败：{e}")),
    }
}

// ---------------------------------------------------------------------------
// 平台分派
// ---------------------------------------------------------------------------

/// 读取当前平台的邻居表。失败时返回带原因的 `Err`，由调用方决定如何提示。
///
/// **返回的是邻居表的忠实视图**：Windows 的 ARP 缓存里会出现组播地址
/// （如 `224.0.0.251` mDNS、`239.255.255.250` SSDP）等非探测目标条目，
/// 这里不替调用方过滤——过滤应当发生在「与本轮目标列表求交」的时候
/// （见 [`crate::enrich::apply_neighbors`]），这样语义最清晰。
pub fn neighbor_table() -> Result<Vec<NeighborEntry>, String> {
    #[cfg(windows)]
    {
        windows_neighbor_table()
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        proc_neighbor_table()
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "android")))]
    {
        Err("该平台尚未实现邻居表读取".to_string())
    }
}

/// 查单个 IP 的 MAC；查不到或平台不支持时返回 `None`（**不报错**）。
pub fn mac_for(ip: Ipv4Addr) -> Option<[u8; 6]> {
    neighbor_table()
        .ok()?
        .into_iter()
        .find(|e| e.ip == ip)
        .map(|e| e.mac)
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
IP address       HW type     Flags       HW address            Mask     Device
192.168.1.1      0x1         0x2         00:11:22:33:44:55     *        wlan0
192.168.1.99     0x1         0x0         00:00:00:00:00:00     *        wlan0
192.168.1.7      0x1         0x2         a4:5e:60:12:34:ab     *        eth0
10.0.0.1         0x20        0x2         00:11:22:33:44:66     *        ib0
";

    #[test]
    fn parses_complete_entries_only() {
        let got = parse_proc_net_arp(SAMPLE);
        assert_eq!(got.len(), 2, "未完成/非以太网条目应被跳过: {got:?}");
        assert_eq!(got[0].ip, Ipv4Addr::new(192, 168, 1, 1));
        assert_eq!(got[0].mac, [0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
        assert_eq!(got[1].ip, Ipv4Addr::new(192, 168, 1, 7));
        assert_eq!(got[1].mac, [0xA4, 0x5E, 0x60, 0x12, 0x34, 0xAB]);
    }

    #[test]
    fn skips_header_malformed_and_zero_mac() {
        let text = "\
IP address       HW type     Flags       HW address            Mask     Device
garbage line
192.168.1.1      0x1         0x2
192.168.1.2      xx          0x2         00:11:22:33:44:55     *        eth0
192.168.1.3      0x1         yy          00:11:22:33:44:55     *        eth0
192.168.1.4      0x1         0x2         not-a-mac             *        eth0
192.168.1.5      0x1         0x2         00:00:00:00:00:00     *        eth0
";
        assert!(parse_proc_net_arp(text).is_empty());
    }

    #[test]
    fn later_entry_for_same_ip_wins() {
        let text = "\
192.168.1.1      0x1         0x2         00:11:22:33:44:55     *        wlan0
192.168.1.1      0x1         0x2         00:11:22:33:44:66     *        wlan0
";
        let got = parse_proc_net_arp(text);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].mac, [0x00, 0x11, 0x22, 0x33, 0x44, 0x66]);
    }

    #[test]
    fn empty_and_crlf_input() {
        assert!(parse_proc_net_arp("").is_empty());
        let crlf = "192.168.1.1 0x1 0x2 00:11:22:33:44:55 * eth0\r\n";
        assert_eq!(parse_proc_net_arp(crlf).len(), 1);
    }

    #[test]
    fn accept_entry_rules() {
        let mac = [0x00u8, 0x11, 0x22, 0x33, 0x44, 0x55];
        // 正常 IPv4
        assert_eq!(
            accept_ipv4_entry(2, [192, 168, 1, 1], 6, &mac),
            Some(NeighborEntry {
                ip: Ipv4Addr::new(192, 168, 1, 1),
                mac
            })
        );
        // 非 IPv4（AF_INET6=23）应拒绝
        assert_eq!(accept_ipv4_entry(23, [192, 168, 1, 1], 6, &mac), None);
        // 长度不是 6（如 8 字节 EUI-64）应拒绝
        assert_eq!(accept_ipv4_entry(2, [192, 168, 1, 1], 8, &mac), None);
        // 零 MAC 应拒绝
        assert_eq!(accept_ipv4_entry(2, [192, 168, 1, 1], 6, &[0u8; 6]), None);
        // 缓冲区不足应拒绝而不是 panic
        assert_eq!(accept_ipv4_entry(2, [192, 168, 1, 1], 6, &[1, 2, 3]), None);
    }

    /// 实测结论：子网广播地址会在 ARP 缓存里出现，且 `.255` 本身是扫描目标之一，
    /// 因此广播/组播 MAC 必须在入口就被拦掉。
    #[test]
    fn broadcast_and_multicast_macs_are_rejected() {
        assert!(!is_unicast_mac([0u8; 6]));
        assert!(!is_unicast_mac([0xFF; 6]), "广播地址不是主机身份");
        assert!(
            !is_unicast_mac([0x01, 0x00, 0x5E, 0x00, 0x00, 0x01]),
            "IPv4 组播"
        );
        assert!(
            !is_unicast_mac([0x01, 0x00, 0x5E, 0x7F, 0xFF, 0xFA]),
            "SSDP 组播"
        );
        assert!(is_unicast_mac([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]));
        assert!(
            is_unicast_mac([0xE2, 0x7B, 0x18, 0x8E, 0x06, 0x56]),
            "本地管理仍算单播"
        );

        // 端到端：广播条目即使 IP 正确也不得进入邻居表
        assert_eq!(
            accept_ipv4_entry(2, [192, 168, 0, 255], 6, &[0xFF; 6]),
            None
        );
        assert_eq!(
            accept_ipv4_entry(
                2,
                [224, 0, 0, 251],
                6,
                &[0x01, 0x00, 0x5E, 0x00, 0x00, 0xFB]
            ),
            None
        );
    }

    /// 字节序回归测试：sin_addr 必须按字节读取。
    /// （曾经用 `u32::to_be_bytes` 读，导致 192.168.0.1 显示成 1.0.168.192。）
    #[cfg(windows)]
    #[test]
    fn ip_octets_are_not_byteswapped() {
        use iphlpapi::SockAddrInet;
        let sa = SockAddrInet {
            sin_family: 2,
            sin_addr: [192, 168, 0, 1],
            ..Default::default()
        };
        assert_eq!(sa.sin_addr, [192, 168, 0, 1], "按字节读必须原样保留八位组");
        // 修复前的写法是「整体当 u32，再 to_be_bytes」，八位组会整个反过来：
        let as_u32 = u32::from_ne_bytes(sa.sin_addr);
        assert_ne!(as_u32.to_be_bytes(), sa.sin_addr);
        assert_eq!(
            as_u32.to_be_bytes(),
            [1, 0, 168, 192],
            "这正是当时显示出来的错值 `1.0.168.192`"
        );
    }

    /// 结构体布局必须与 C 侧 `MIB_IPNET_ROW2` 一致（x64 / x86 均为 88 字节）。
    #[cfg(windows)]
    #[test]
    fn layout_is_stable() {
        use iphlpapi::{MibIpNetRow2, SockAddrInet};
        use std::mem::{align_of, size_of};
        assert_eq!(size_of::<SockAddrInet>(), 28, "SOCKADDR_INET 固定 28 字节");
        assert_eq!(size_of::<MibIpNetRow2>(), 88);
        assert_eq!(align_of::<MibIpNetRow2>(), 8);
        let probe = MibIpNetRow2 {
            address: iphlpapi::SockAddrInet::default(),
            interface_index: 0,
            interface_luid: 0,
            physical_address: [0u8; 32],
            physical_address_length: 0,
            state: 0,
            flags: 0,
            reachability_time: 0,
        };
        let base = &probe as *const _ as usize;
        let off = |p: *const u8| p as usize - base;
        assert_eq!(off(std::ptr::addr_of!(probe.interface_index).cast()), 28);
        assert_eq!(off(std::ptr::addr_of!(probe.interface_luid).cast()), 32);
        assert_eq!(off(std::ptr::addr_of!(probe.physical_address).cast()), 40);
        assert_eq!(
            off(std::ptr::addr_of!(probe.physical_address_length).cast()),
            72
        );
        assert_eq!(off(std::ptr::addr_of!(probe.reachability_time).cast()), 84);
    }

    /// 真实调用：只要求不 panic、错误可解释；ARP 表为空是合法结果。
    ///
    /// 广播/组播条目在入口就被过滤，所以这里可以断言「表里全是单播 MAC」——
    /// 这也是「`.255` 这类扫描目标不会被填上 `FF:FF:FF:FF:FF:FF`」的保证。
    #[test]
    fn real_table_call_does_not_panic() {
        match neighbor_table() {
            Ok(entries) => {
                for e in &entries {
                    assert!(
                        is_unicast_mac(e.mac),
                        "非单播 MAC 不应出现在邻居表里: {} -> {}",
                        e.ip,
                        crate::oui::mac_to_string(e.mac)
                    );
                }
                println!("邻居表条目数: {}（已过滤广播/组播）", entries.len());
            }
            Err(msg) => println!("邻居表不可用（可接受）: {msg}"),
        }
    }
}
