//! 主机名反查（M7）：IP → PTR 记录。
//!
//! ## 为什么不做成扫描的一部分
//!
//! DNS 反解是**阻塞**的，且 255 个目标里绝大多数没有 PTR 记录。若在扫描循环里
//! 顺带反查，扫描耗时会从「秒级」退化到「分钟级」，这违背 M1 定下的节奏。
//! 因此本模块只提供**按需调用**的接口，由界面在用户明确请求时逐行触发，
//! 结果进缓存，同一 IP 不重复查。
//!
//! ## 超时是硬约束
//!
//! `getnameinfo` 自身没有超时参数，因此这里把调用放到短命线程里，由主线程
//! `recv_timeout` 限时等待。超时后**丢弃该次结果且不写缓存**——因为「超时」
//! 不等于「没有 PTR」，把超时记成否定结论会在下次请求时给出错误答案。
//!
//! ## 只接受真实记录
//!
//! 使用 `NI_NAMEREQD`：没有 PTR 时直接失败，**不允许**把 IP 字符串本身
//! 当成主机名返回（那是伪造）。
//!
//! ## 未实现的部分（如实记录）
//!
//! 计划里的「Windows NetBIOS 后缀查询」本轮未实现——它需要 UDP/137 的
//! NetBIOS 名称服务报文，属于独立协议实现，且没有可验证的目标环境。
//! 这里只做了 PTR 结果的**后缀规范化**（见 [`normalize_ptr_name`]）。
//! 该缺口已记入 `docs/plan.md`。

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{mpsc, Mutex, OnceLock};
use std::time::Duration;

/// 默认超时：局域网 PTR 慢于这个值就当作查不到。
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(2);

// getnameinfo 需要正确的地址族与对应长度的 sockaddr。这两个常量按平台硬编码，
// 与 `native.rs` 里 ICMP 套接字用的是同一套数值。
#[cfg(windows)]
const AF_INET6: u16 = 23;
#[cfg(any(target_os = "linux", target_os = "android"))]
const AF_INET6: u16 = 10;

/// IPv6 版 `sockaddr`（`sockaddr_in6`）。
///
/// M12 新增：反查对 v4/v6 一视同仁，只是要传对结构体。
#[cfg(any(windows, target_os = "linux", target_os = "android"))]
#[repr(C)]
struct SockAddrIn6 {
    sin6_family: u16,
    sin6_port: u16,
    sin6_flowinfo: u32,
    sin6_addr: [u8; 16],
    sin6_scope_id: u32,
}

static CACHE: OnceLock<Mutex<HashMap<IpAddr, Option<String>>>> = OnceLock::new();

fn cache() -> &'static Mutex<HashMap<IpAddr, Option<String>>> {
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

// ---------------------------------------------------------------------------
// 结果规范化（纯函数）
// ---------------------------------------------------------------------------

/// 规范化 PTR 结果：
/// - 去掉首尾空白与结尾的点（FQDN 形式）；
/// - 空串、或看起来是 IP 字面量的结果一律拒绝（防御性：宁可没有名字）；
/// - 去掉 NetBIOS 的纯数字后缀标签（例如 `PC1.20.0.0.0.0` → `PC1`）。
///
/// 后缀只在「有 ≥4 个连续纯数字尾标签」时才裁剪，避免误伤
/// 形如 `a.1.2` 的正常域名。
pub fn normalize_ptr_name(raw: &str) -> Option<String> {
    let name = raw.trim().trim_end_matches('.');
    if name.is_empty() {
        return None;
    }
    if name.parse::<IpAddr>().is_ok() || name.parse::<std::net::Ipv6Addr>().is_ok() {
        return None;
    }
    let labels: Vec<&str> = name.split('.').collect();
    if labels.len() > 1 {
        let mut trailing_numeric = 0usize;
        for label in labels[1..].iter().rev() {
            if !label.is_empty() && label.bytes().all(|b| b.is_ascii_digit()) {
                trailing_numeric += 1;
            } else {
                break;
            }
        }
        if trailing_numeric >= 4 {
            let head = labels[0];
            if !head.is_empty() && !head.bytes().all(|b| b.is_ascii_digit()) {
                return Some(head.to_string());
            }
        }
    }
    // 去掉连续的重复点（某些实现会返回 "host..domain"）
    let cleaned: Vec<&str> = labels.iter().copied().filter(|l| !l.is_empty()).collect();
    Some(cleaned.join("."))
}

// ---------------------------------------------------------------------------
// 平台 FFI
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct SockAddrIn {
    sin_family: u16,
    sin_port: u16,
    sin_addr: u32,
    sin_zero: [u8; 8],
}

const AF_INET: i32 = 2;
/// `NI_NAMEREQD`：没有 PTR 记录时报错，而不是回退成数字形式。
const NI_NAMEREQD: i32 = 4;

#[cfg(windows)]
mod ffi {
    use super::{IpAddr, SockAddrIn, SockAddrIn6, AF_INET, AF_INET6, NI_NAMEREQD};

    #[link(name = "ws2_32")]
    extern "system" {
        fn getnameinfo(
            sa: *const SockAddrIn,
            salen: i32,
            host: *mut u8,
            hostlen: u32,
            serv: *mut u8,
            servlen: u32,
            flags: i32,
        ) -> i32;
    }

    /// 返回 `Ok(主机名)`；无 PTR 记录或出错返回 `Err(说明)`。
    ///
    /// M12：v4 与 v6 走同一套 `getnameinfo`，只是传入的 `sockaddr` 结构体不同。
    pub fn ptr(ip: IpAddr) -> Result<String, String> {
        // getnameinfo 依赖 WinSock 已初始化
        crate::native::ensure_winsock().map_err(|e| e.to_string())?;
        let mut buf = [0u8; 256];
        let rc = match ip {
            IpAddr::V4(v4) => {
                let sa = SockAddrIn {
                    sin_family: AF_INET as u16,
                    sin_port: 0,
                    sin_addr: u32::from_ne_bytes(v4.octets()),
                    sin_zero: [0; 8],
                };
                unsafe {
                    getnameinfo(
                        &sa,
                        std::mem::size_of::<SockAddrIn>() as i32,
                        buf.as_mut_ptr(),
                        buf.len() as u32,
                        std::ptr::null_mut(),
                        0,
                        NI_NAMEREQD,
                    )
                }
            }
            IpAddr::V6(v6) => {
                let sa = SockAddrIn6 {
                    sin6_family: AF_INET6,
                    sin6_port: 0,
                    sin6_flowinfo: 0,
                    sin6_addr: v6.octets(),
                    sin6_scope_id: 0,
                };
                unsafe {
                    // getnameinfo 只按 (指针, 长度) 解释内存，转成同一类型指针是安全的；
                    // 长度由下面显式传入，两个结构体各自的大小都是准确的。
                    getnameinfo(
                        (&sa as *const SockAddrIn6).cast::<SockAddrIn>(),
                        std::mem::size_of::<SockAddrIn6>() as i32,
                        buf.as_mut_ptr(),
                        buf.len() as u32,
                        std::ptr::null_mut(),
                        0,
                        NI_NAMEREQD,
                    )
                }
            }
        };
        if rc != 0 {
            return Err(format!("getnameinfo 返回 {rc}（通常表示没有 PTR 记录）"));
        }
        let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
        String::from_utf8(buf[..end].to_vec()).map_err(|e| e.to_string())
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
mod ffi {
    use super::{IpAddr, SockAddrIn, SockAddrIn6, AF_INET, AF_INET6, NI_NAMEREQD};

    extern "C" {
        fn getnameinfo(
            sa: *const SockAddrIn,
            salen: u32,
            host: *mut u8,
            hostlen: u32,
            serv: *mut u8,
            servlen: u32,
            flags: i32,
        ) -> i32;
    }

    pub fn ptr(ip: IpAddr) -> Result<String, String> {
        let mut buf = [0u8; 256];
        let rc = match ip {
            IpAddr::V4(v4) => {
                let sa = SockAddrIn {
                    sin_family: AF_INET as u16,
                    sin_port: 0,
                    sin_addr: u32::from_ne_bytes(v4.octets()),
                    sin_zero: [0; 8],
                };
                unsafe {
                    getnameinfo(
                        &sa,
                        std::mem::size_of::<SockAddrIn>() as u32,
                        buf.as_mut_ptr(),
                        buf.len() as u32,
                        std::ptr::null_mut(),
                        0,
                        NI_NAMEREQD,
                    )
                }
            }
            IpAddr::V6(v6) => {
                let sa = SockAddrIn6 {
                    sin6_family: AF_INET6,
                    sin6_port: 0,
                    sin6_flowinfo: 0,
                    sin6_addr: v6.octets(),
                    sin6_scope_id: 0,
                };
                unsafe {
                    getnameinfo(
                        (&sa as *const SockAddrIn6).cast::<SockAddrIn>(),
                        std::mem::size_of::<SockAddrIn6>() as u32,
                        buf.as_mut_ptr(),
                        buf.len() as u32,
                        std::ptr::null_mut(),
                        0,
                        NI_NAMEREQD,
                    )
                }
            }
        };
        if rc != 0 {
            return Err(format!("getnameinfo 返回 {rc}（通常表示没有 PTR 记录）"));
        }
        let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
        String::from_utf8(buf[..end].to_vec()).map_err(|e| e.to_string())
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "android")))]
mod ffi {
    pub fn ptr(_ip: std::net::IpAddr) -> Result<String, String> {
        Err("该平台尚未实现主机名反查".to_string())
    }
}

// ---------------------------------------------------------------------------
// 对外接口
// ---------------------------------------------------------------------------

/// 反查单个 IP 的主机名（带超时，结果入缓存）。
///
/// - 命中缓存直接返回（包括「已确认无 PTR」的否定缓存）；
/// - 超时返回 `None` 且**不写缓存**；
/// - 没有 PTR 记录返回 `None` 并写否定缓存。
pub fn reverse_lookup(ip: IpAddr, timeout: Duration) -> Option<String> {
    if let Some(cached) = cache().lock().ok()?.get(&ip).cloned() {
        return cached;
    }

    let (tx, rx) = mpsc::channel();
    // 线程只负责一次系统调用，命名便于在调试器里识别
    let spawned = std::thread::Builder::new()
        .name(format!("cping-ptr-{ip}"))
        .spawn(move || {
            let result = ffi::ptr(ip).ok().and_then(|raw| normalize_ptr_name(&raw));
            let _ = tx.send(result);
        });
    if spawned.is_err() {
        return None;
    }

    match rx.recv_timeout(timeout) {
        Ok(result) => {
            if let Ok(mut c) = cache().lock() {
                c.insert(ip, result.clone());
            }
            result
        }
        // 超时：线程可能仍在等待 DNS，这里直接放弃本次结果且不缓存
        Err(_) => None,
    }
}

/// 用默认超时反查。
pub fn reverse_lookup_default(ip: IpAddr) -> Option<String> {
    reverse_lookup(ip, DEFAULT_TIMEOUT)
}

/// 清空缓存（测试用；界面不需要）。
pub fn clear_cache() {
    if let Ok(mut c) = cache().lock() {
        c.clear();
    }
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_trailing_dot_and_blank() {
        assert_eq!(
            normalize_ptr_name("router.local."),
            Some("router.local".into())
        );
        assert_eq!(normalize_ptr_name("  router  "), Some("router".into()));
        assert_eq!(normalize_ptr_name(""), None);
        assert_eq!(normalize_ptr_name("   "), None);
        assert_eq!(normalize_ptr_name("."), None);
    }

    #[test]
    fn normalize_rejects_ip_literals() {
        // 防御性：NI_NAMEREQD 下不该出现，但真出现了也绝不当作主机名
        assert_eq!(normalize_ptr_name("192.168.1.1"), None);
        assert_eq!(normalize_ptr_name("::1"), None);
    }

    #[test]
    fn normalize_handles_netbios_numeric_suffix() {
        assert_eq!(
            normalize_ptr_name("PC1.20.0.0.0.0.0.0"),
            Some("PC1".to_string())
        );
        assert_eq!(
            normalize_ptr_name("WORKSTATION.0.0.0.0.0.0.0.0.0.0.0.0"),
            Some("WORKSTATION".to_string())
        );
    }

    #[test]
    fn normalize_keeps_ordinary_domains() {
        // 只有一两个数字标签的正常域名不能被裁剪
        assert_eq!(normalize_ptr_name("a.1.2"), Some("a.1.2".to_string()));
        assert_eq!(
            normalize_ptr_name("web01.corp.example.com"),
            Some("web01.corp.example.com".to_string())
        );
        // 数字开头但非纯数字的主机名要保留
        assert_eq!(
            normalize_ptr_name("srv-2.example.com"),
            Some("srv-2.example.com".to_string())
        );
    }

    #[test]
    fn normalize_collapses_double_dots() {
        assert_eq!(
            normalize_ptr_name("host..example.com"),
            Some("host.example.com".to_string())
        );
    }

    #[test]
    fn reverse_lookup_returns_none_for_reserved_unresolvable() {
        // RFC 5737 TEST-NET-1：保留给文档，不可能有 PTR
        clear_cache();
        let ip = IpAddr::V4(std::net::Ipv4Addr::new(192, 0, 2, 123));
        let t0 = std::time::Instant::now();
        let got = reverse_lookup(ip, Duration::from_millis(1500));
        assert_eq!(got, None, "TEST-NET 地址不应有主机名");
        assert!(t0.elapsed() < Duration::from_secs(5), "必须受超时约束");
    }

    #[test]
    fn reverse_lookup_respects_tiny_timeout() {
        // 1ms 超时必须立刻返回 None，而不是等 DNS 自己放弃
        clear_cache();
        let ip = IpAddr::V4(std::net::Ipv4Addr::new(198, 51, 100, 7)); // RFC 5737 TEST-NET-2
        let t0 = std::time::Instant::now();
        let got = reverse_lookup(ip, Duration::from_millis(1));
        assert_eq!(got, None);
        assert!(
            t0.elapsed() < Duration::from_millis(500),
            "超时未生效，耗时 {:?}",
            t0.elapsed()
        );
    }

    #[test]
    fn negative_result_is_cached_and_reused() {
        clear_cache();
        let ip = IpAddr::V4(std::net::Ipv4Addr::new(203, 0, 113, 5)); // RFC 5737 TEST-NET-3
        assert_eq!(reverse_lookup(ip, Duration::from_millis(800)), None);
        // 第二次应命中缓存：即使给极短超时也立刻返回
        let t0 = std::time::Instant::now();
        assert_eq!(reverse_lookup(ip, Duration::from_millis(1)), None);
        assert!(
            t0.elapsed() < Duration::from_millis(100),
            "否定结果未走缓存"
        );
    }

    #[test]
    fn timeout_does_not_poison_cache() {
        clear_cache();
        let ip = IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1));
        // 用 0 纳秒超时强制走超时分支
        let _ = reverse_lookup(ip, Duration::from_nanos(0));
        let poisoned = cache().lock().unwrap().contains_key(&ip);
        // 唯一允许的例外是线程在超时前已完成（回环解析极快），此时缓存是正确的
        if poisoned {
            assert!(
                cache().lock().unwrap().get(&ip).unwrap().is_some(),
                "若已写入缓存，必须是确认过的结果而不是超时占位"
            );
        }
    }
}
