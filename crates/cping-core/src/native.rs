//! 平台原生 ICMP 后端（M5）。
//!
//! 设计约束：
//! - **手写 FFI**，保持 `cping-core` 零第三方依赖（不需要 `windows-sys` / `libc`）；
//! - Windows 优先 `SOCK_DGRAM + IPPROTO_ICMP`（普通用户可用），失败再回退 `SOCK_RAW`；
//! - Linux / Android 同样先 `SOCK_DGRAM` 再 `SOCK_RAW`，两者都拿不到权限时
//!   **如实返回 `PermissionDenied`**，绝不降级成「看起来成功」；
//! - **不把 TCP 连通性当作 ICMP 成功**：TCP 走 [`TcpConnectBackend`]，语义与状态名都独立。
//!
//! 报文构造与解析写成不依赖平台的纯函数，因此可以在任意平台上做单元测试。

use crate::error::ProbeError;
use crate::i18n::{self, Key, Lang};
use crate::probe::{ProbeBackend, ProbeOutcome};
use std::net::{IpAddr, Ipv4Addr};
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::{Duration, Instant};

/// ICMP 报文头长度。
pub const ICMP_HEADER_LEN: usize = 8;
/// 默认载荷长度：8 + 36 = 44 字节，与原版实测的 `sendto` 载荷长度一致。
pub const DEFAULT_PAYLOAD_LEN: usize = 36;

static SEQ: AtomicU16 = AtomicU16::new(1);

fn next_seq() -> u16 {
    let v = SEQ.fetch_add(1, Ordering::SeqCst);
    if v == 0 {
        SEQ.store(1, Ordering::SeqCst);
        1
    } else {
        v
    }
}

/// 16 位反码和校验（ICMP 用）。
pub fn icmp_checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut chunks = data.chunks_exact(2);
    for c in &mut chunks {
        sum += u16::from_be_bytes([c[0], c[1]]) as u32;
    }
    if let [last] = chunks.remainder() {
        sum += (*last as u32) << 8;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

/// 构造 ICMP Echo Request。
pub fn build_echo_request(id: u16, seq: u16, payload_len: usize) -> Vec<u8> {
    let mut pkt = vec![0u8; ICMP_HEADER_LEN + payload_len];
    pkt[0] = 8; // Echo Request
    pkt[1] = 0; // Code
                // [2..4] 校验和稍后填
    pkt[4..6].copy_from_slice(&id.to_be_bytes());
    pkt[6..8].copy_from_slice(&seq.to_be_bytes());
    for (i, b) in pkt[ICMP_HEADER_LEN..].iter_mut().enumerate() {
        *b = (i & 0xFF) as u8;
    }
    let ck = icmp_checksum(&pkt);
    pkt[2..4].copy_from_slice(&ck.to_be_bytes());
    pkt
}

/// 校验 ICMP 报文校验和是否正确（自检用）。
pub fn verify_checksum(pkt: &[u8]) -> bool {
    pkt.len() >= ICMP_HEADER_LEN && icmp_checksum(pkt) == 0
}

/// 从接收缓冲区里解析出 Echo Reply 的 (id, seq)。
///
/// `SOCK_RAW` 通常带上 IP 头，`SOCK_DGRAM` 在 Windows 上会剥离；
/// 因此这里按首字节判断是否包含 IP 头，两种都能处理。
pub fn parse_echo_reply(buf: &[u8]) -> Option<(u16, u16)> {
    if buf.len() < ICMP_HEADER_LEN {
        return None;
    }
    let version = buf[0] >> 4;
    let ihl = ((buf[0] & 0x0F) as usize) * 4;
    let icmp = if version == 4 && (20..=60).contains(&ihl) && buf.len() > ihl + ICMP_HEADER_LEN {
        &buf[ihl..]
    } else {
        buf
    };
    if icmp.len() < ICMP_HEADER_LEN || icmp[0] != 0 {
        // 0 = Echo Reply；其它类型（如目的不可达 type 3）在这里不算成功
        return None;
    }
    Some((
        u16::from_be_bytes([icmp[4], icmp[5]]),
        u16::from_be_bytes([icmp[6], icmp[7]]),
    ))
}

/// ICMP 目的不可达（type 3）检测，用于把「不可达」与「超时」区分开。
pub fn parse_unreachable(buf: &[u8]) -> Option<u8> {
    let version = buf.first()? >> 4;
    let ihl = ((buf[0] & 0x0F) as usize) * 4;
    let icmp = if version == 4 && (20..=60).contains(&ihl) && buf.len() > ihl + ICMP_HEADER_LEN {
        &buf[ihl..]
    } else {
        buf
    };
    if icmp.len() >= ICMP_HEADER_LEN && icmp[0] == 3 {
        Some(icmp[1]) // code: 0=net, 1=host, 3=port ...
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod win_ffi {
    pub const AF_INET: i32 = 2;
    pub const SOCK_DGRAM: i32 = 2;
    pub const SOCK_RAW: i32 = 3;
    pub const IPPROTO_ICMP: i32 = 1;
    pub const SOL_SOCKET: i32 = 0xFFFF;
    pub const SO_RCVTIMEO: i32 = 0x1006;
    pub const INVALID_SOCKET: usize = usize::MAX;

    pub const WSAETIMEDOUT: i32 = 10060;
    pub const WSAEHOSTUNREACH: i32 = 10065;
    pub const WSAENETUNREACH: i32 = 10051;
    pub const WSAEACCES: i32 = 10013;
    pub const WSAENETDOWN: i32 = 10050;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct SockAddrIn {
        pub sin_family: u16,
        pub sin_port: u16,
        pub sin_addr: u32,
        pub sin_zero: [u8; 8],
    }

    #[link(name = "ws2_32")]
    extern "system" {
        pub fn WSAStartup(version: u16, data: *mut u8) -> i32;
        pub fn socket(af: i32, ty: i32, protocol: i32) -> usize;
        pub fn sendto(
            s: usize,
            buf: *const u8,
            len: i32,
            flags: i32,
            to: *const SockAddrIn,
            tolen: i32,
        ) -> i32;
        pub fn recvfrom(
            s: usize,
            buf: *mut u8,
            len: i32,
            flags: i32,
            from: *mut SockAddrIn,
            fromlen: *mut i32,
        ) -> i32;
        pub fn closesocket(s: usize) -> i32;
        pub fn setsockopt(
            s: usize,
            level: i32,
            optname: i32,
            optval: *const u8,
            optlen: i32,
        ) -> i32;
        pub fn WSAGetLastError() -> i32;
    }
}

/// Windows 原生 ICMP 后端。
#[cfg(windows)]
pub struct WindowsIcmpBackend {
    payload_len: usize,
}

#[cfg(windows)]
impl Default for WindowsIcmpBackend {
    fn default() -> Self {
        Self {
            payload_len: DEFAULT_PAYLOAD_LEN,
        }
    }
}

/// 初始化 WinSock（幂等）。
///
/// Windows 上任何 socket / `getnameinfo` 调用之前都必须先 `WSAStartup`，
/// 因此把这件事集中在这里，ICMP 后端与主机名反查共用同一份状态与锁。
#[cfg(windows)]
pub fn ensure_winsock() -> Result<(), ProbeError> {
    use std::sync::atomic::{AtomicI32, Ordering as O};
    use std::sync::Mutex;
    static STATE: AtomicI32 = AtomicI32::new(0); // 0=未初始化 1=成功 -1=失败
    static LOCK: Mutex<()> = Mutex::new(());

    match STATE.load(O::SeqCst) {
        1 => return Ok(()),
        -1 => return Err(ProbeError::internal("WSAStartup 此前已失败")),
        _ => {}
    }
    let _guard = LOCK.lock().expect("wsa lock");
    match STATE.load(O::SeqCst) {
        1 => return Ok(()),
        -1 => return Err(ProbeError::internal("WSAStartup 此前已失败")),
        _ => {}
    }
    let mut data = [0u8; 512]; // WSADATA 约 400 字节
    let rc = unsafe { win_ffi::WSAStartup(0x0202, data.as_mut_ptr()) };
    if rc == 0 {
        STATE.store(1, O::SeqCst);
        Ok(())
    } else {
        STATE.store(-1, O::SeqCst);
        Err(ProbeError::internal(format!("WSAStartup 返回 {rc}")))
    }
}

#[cfg(windows)]
impl WindowsIcmpBackend {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_payload_len(mut self, len: usize) -> Self {
        self.payload_len = len;
        self
    }

    fn ensure_wsa() -> Result<(), ProbeError> {
        ensure_winsock()
    }

    /// 用给定套接字类型做一次探测；失败时返回可区分的错误。
    unsafe fn try_once(
        sock_type: i32,
        target: Ipv4Addr,
        id: u16,
        seq: u16,
        payload_len: usize,
        timeout: Duration,
    ) -> Result<ProbeOutcome, ProbeError> {
        let s = win_ffi::socket(win_ffi::AF_INET, sock_type, win_ffi::IPPROTO_ICMP);
        if s == win_ffi::INVALID_SOCKET {
            return Err(map_wsa_error(
                win_ffi::WSAGetLastError(),
                "创建 ICMP 套接字失败",
            ));
        }
        // 保证任何提前返回都会关闭套接字
        struct SockGuard(usize);
        impl Drop for SockGuard {
            fn drop(&mut self) {
                unsafe {
                    win_ffi::closesocket(self.0);
                }
            }
        }
        let _guard = SockGuard(s);

        let ms = timeout.as_millis().min(u32::MAX as u128) as u32;
        let tv = ms;
        if win_ffi::setsockopt(
            s,
            win_ffi::SOL_SOCKET,
            win_ffi::SO_RCVTIMEO,
            &tv as *const u32 as *const u8,
            std::mem::size_of::<u32>() as i32,
        ) != 0
        {
            return Err(map_wsa_error(
                win_ffi::WSAGetLastError(),
                "设置接收超时失败",
            ));
        }

        let pkt = build_echo_request(id, seq, payload_len);
        let addr = win_ffi::SockAddrIn {
            sin_family: win_ffi::AF_INET as u16,
            sin_port: 0,
            sin_addr: u32::from_ne_bytes(target.octets()),
            sin_zero: [0; 8],
        };

        let started = Instant::now();
        let sent = win_ffi::sendto(
            s,
            pkt.as_ptr(),
            pkt.len() as i32,
            0,
            &addr,
            std::mem::size_of::<win_ffi::SockAddrIn>() as i32,
        );
        if sent < 0 {
            return Err(map_wsa_error(win_ffi::WSAGetLastError(), "发送 ICMP 失败"));
        }

        let mut buf = [0u8; 1500];
        loop {
            let elapsed = started.elapsed();
            if elapsed >= timeout {
                return Err(ProbeError::timeout(ms as u64));
            }
            let mut from = win_ffi::SockAddrIn::default();
            let mut from_len = std::mem::size_of::<win_ffi::SockAddrIn>() as i32;
            let n = win_ffi::recvfrom(
                s,
                buf.as_mut_ptr(),
                buf.len() as i32,
                0,
                &mut from,
                &mut from_len,
            );
            if n < 0 {
                let code = win_ffi::WSAGetLastError();
                return match code {
                    win_ffi::WSAETIMEDOUT => Err(ProbeError::timeout(ms as u64)),
                    other => Err(map_wsa_error(other, "接收 ICMP 回复失败")),
                };
            }
            let slice = &buf[..n as usize];
            if let Some((rid, rseq)) = parse_echo_reply(slice) {
                if rid == id && rseq == seq {
                    let rtt = started.elapsed().as_millis() as u32;
                    return Ok(ProbeOutcome::online(rtt));
                }
            }
            if let Some(code) = parse_unreachable(slice) {
                return Err(ProbeError::unreachable(format!(
                    "ICMP 目的不可达 code={code}"
                )));
            }
            // 不是我们关心的包（其它目标的回包等），继续等
        }
    }
}

#[cfg(windows)]
fn map_wsa_error(code: i32, ctx: &str) -> ProbeError {
    match code {
        win_ffi::WSAETIMEDOUT => ProbeError::timeout(0),
        win_ffi::WSAEHOSTUNREACH | win_ffi::WSAENETUNREACH | win_ffi::WSAENETDOWN => {
            ProbeError::unreachable(format!("{ctx}: WSA {code}"))
        }
        win_ffi::WSAEACCES => ProbeError::permission_denied(format!(
            "{ctx}: WSA {code}（需要管理员权限或该地址被策略阻止）"
        )),
        other => ProbeError::internal(format!("{ctx}: WSA {other}")),
    }
}

#[cfg(windows)]
impl ProbeBackend for WindowsIcmpBackend {
    fn name(&self) -> &'static str {
        "windows-icmp"
    }

    fn probe(&self, target: IpAddr, timeout: Duration) -> Result<ProbeOutcome, ProbeError> {
        Self::ensure_wsa()?;
        // M12：ICMPv6 需要 Icmp6SendEcho2 与 AF_INET6 套接字，本版未实装。
        // 明确报「不支持」，而不是静默超时——两者对用户的含义完全不同。
        let IpAddr::V4(target) = target else {
            return Err(ProbeError::unsupported(
                "ICMPv6 后端尚未实装；IPv6 目标请改用 TCP 方式（--mode tcp）",
            ));
        };
        let id = std::process::id() as u16;
        let seq = next_seq();
        let payload = self.payload_len;

        // 先 SOCK_DGRAM（普通用户可用的路径），权限/能力不足再退 SOCK_RAW
        let mut last: Option<ProbeError> = None;
        for sock_type in [win_ffi::SOCK_DGRAM, win_ffi::SOCK_RAW] {
            match unsafe { Self::try_once(sock_type, target, id, seq, payload, timeout) } {
                Ok(outcome) => return Ok(outcome),
                Err(err) => last = Some(err),
            }
        }
        Err(last.unwrap_or_else(|| ProbeError::internal("无法创建 ICMP 套接字")))
    }
}

// ---------------------------------------------------------------------------
// Linux / Android
// ---------------------------------------------------------------------------

#[cfg(any(target_os = "linux", target_os = "android"))]
mod posix_ffi {
    pub const AF_INET: i32 = 2;
    pub const SOCK_DGRAM: i32 = 2;
    pub const SOCK_RAW: i32 = 3;
    pub const IPPROTO_ICMP: i32 = 1;
    pub const SOL_SOCKET: i32 = 1;
    pub const SO_RCVTIMEO: i32 = 20;

    pub const EPERM: i32 = 1;
    pub const EACCES: i32 = 13;
    pub const EAGAIN: i32 = 11;
    pub const ETIMEDOUT: i32 = 110;
    pub const EHOSTUNREACH: i32 = 113;
    pub const ENETUNREACH: i32 = 101;
    pub const ENETDOWN: i32 = 100;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct SockAddrIn {
        pub sin_family: u16,
        pub sin_port: u16,
        pub sin_addr: u32,
        pub sin_zero: [u8; 8],
    }

    #[cfg(target_pointer_width = "64")]
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct TimeVal {
        pub tv_sec: i64,
        pub tv_usec: i64,
    }

    #[cfg(target_pointer_width = "32")]
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct TimeVal {
        pub tv_sec: i32,
        pub tv_usec: i32,
    }

    // libc 已由 std 链接，无需额外 #[link]
    extern "C" {
        pub fn socket(domain: i32, ty: i32, protocol: i32) -> i32;
        pub fn sendto(
            fd: i32,
            buf: *const u8,
            len: usize,
            flags: i32,
            to: *const SockAddrIn,
            tolen: u32,
        ) -> isize;
        pub fn recvfrom(
            fd: i32,
            buf: *mut u8,
            len: usize,
            flags: i32,
            from: *mut SockAddrIn,
            fromlen: *mut u32,
        ) -> isize;
        pub fn close(fd: i32) -> i32;
        pub fn setsockopt(fd: i32, level: i32, optname: i32, optval: *const u8, optlen: u32)
            -> i32;
    }

    #[cfg(target_os = "android")]
    extern "C" {
        fn __errno() -> *mut i32;
    }
    #[cfg(target_os = "linux")]
    extern "C" {
        fn __errno_location() -> *mut i32;
    }

    pub fn errno() -> i32 {
        #[cfg(target_os = "android")]
        unsafe {
            *__errno()
        }
        #[cfg(target_os = "linux")]
        unsafe {
            *__errno_location()
        }
    }
}

/// Linux / Android 原生 ICMP 后端。
#[cfg(any(target_os = "linux", target_os = "android"))]
pub struct PosixIcmpBackend {
    payload_len: usize,
}

#[cfg(any(target_os = "linux", target_os = "android"))]
impl Default for PosixIcmpBackend {
    fn default() -> Self {
        Self {
            payload_len: DEFAULT_PAYLOAD_LEN,
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
impl PosixIcmpBackend {
    pub fn new() -> Self {
        Self::default()
    }

    unsafe fn try_once(
        sock_type: i32,
        target: Ipv4Addr,
        id: u16,
        seq: u16,
        payload_len: usize,
        timeout: Duration,
    ) -> Result<ProbeOutcome, ProbeError> {
        let fd = posix_ffi::socket(posix_ffi::AF_INET, sock_type, posix_ffi::IPPROTO_ICMP);
        if fd < 0 {
            return Err(map_errno(posix_ffi::errno(), "创建 ICMP 套接字失败"));
        }
        struct FdGuard(i32);
        impl Drop for FdGuard {
            fn drop(&mut self) {
                unsafe {
                    posix_ffi::close(self.0);
                }
            }
        }
        let _guard = FdGuard(fd);

        let ms = timeout.as_millis().min(u32::MAX as u128);
        let tv = posix_ffi::TimeVal {
            #[cfg(target_pointer_width = "64")]
            tv_sec: (ms / 1000) as i64,
            #[cfg(target_pointer_width = "64")]
            tv_usec: ((ms % 1000) * 1000) as i64,
            #[cfg(target_pointer_width = "32")]
            tv_sec: (ms / 1000) as i32,
            #[cfg(target_pointer_width = "32")]
            tv_usec: ((ms % 1000) * 1000) as i32,
        };
        if posix_ffi::setsockopt(
            fd,
            posix_ffi::SOL_SOCKET,
            posix_ffi::SO_RCVTIMEO,
            &tv as *const posix_ffi::TimeVal as *const u8,
            std::mem::size_of::<posix_ffi::TimeVal>() as u32,
        ) != 0
        {
            return Err(map_errno(posix_ffi::errno(), "设置接收超时失败"));
        }

        let pkt = build_echo_request(id, seq, payload_len);
        let addr = posix_ffi::SockAddrIn {
            sin_family: posix_ffi::AF_INET as u16,
            sin_port: 0,
            sin_addr: u32::from_ne_bytes(target.octets()),
            sin_zero: [0; 8],
        };

        let started = Instant::now();
        let sent = posix_ffi::sendto(
            fd,
            pkt.as_ptr(),
            pkt.len(),
            0,
            &addr,
            std::mem::size_of::<posix_ffi::SockAddrIn>() as u32,
        );
        if sent < 0 {
            return Err(map_errno(posix_ffi::errno(), "发送 ICMP 失败"));
        }

        let mut buf = [0u8; 1500];
        loop {
            if started.elapsed() >= timeout {
                return Err(ProbeError::timeout(ms as u64));
            }
            let mut from = posix_ffi::SockAddrIn::default();
            let mut from_len = std::mem::size_of::<posix_ffi::SockAddrIn>() as u32;
            let n =
                posix_ffi::recvfrom(fd, buf.as_mut_ptr(), buf.len(), 0, &mut from, &mut from_len);
            if n < 0 {
                let e = posix_ffi::errno();
                return match e {
                    posix_ffi::EAGAIN => Err(ProbeError::timeout(ms as u64)),
                    other => Err(map_errno(other, "接收 ICMP 回复失败")),
                };
            }
            let slice = &buf[..n as usize];
            if let Some((rid, rseq)) = parse_echo_reply(slice) {
                if rid == id && rseq == seq {
                    let rtt = started.elapsed().as_millis() as u32;
                    return Ok(ProbeOutcome::online(rtt));
                }
            }
            if let Some(code) = parse_unreachable(slice) {
                return Err(ProbeError::unreachable(format!(
                    "ICMP 目的不可达 code={code}"
                )));
            }
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn map_errno(code: i32, ctx: &str) -> ProbeError {
    match code {
        posix_ffi::EPERM | posix_ffi::EACCES => ProbeError::permission_denied(format!(
            "{ctx}: errno {code}（需要 CAP_NET_RAW 或 root；Android 普通应用通常不具备）"
        )),
        posix_ffi::ETIMEDOUT | posix_ffi::EAGAIN => ProbeError::timeout(0),
        posix_ffi::EHOSTUNREACH | posix_ffi::ENETUNREACH | posix_ffi::ENETDOWN => {
            ProbeError::unreachable(format!("{ctx}: errno {code}"))
        }
        other => ProbeError::internal(format!("{ctx}: errno {other}")),
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
impl ProbeBackend for PosixIcmpBackend {
    fn name(&self) -> &'static str {
        if cfg!(target_os = "android") {
            "android-icmp"
        } else {
            "linux-icmp"
        }
    }

    fn probe(&self, target: IpAddr, timeout: Duration) -> Result<ProbeOutcome, ProbeError> {
        // M12：与 Windows 侧同理，ICMPv6（SOCK_RAW + IPPROTO_ICMPV6）本版未实装。
        let IpAddr::V4(target) = target else {
            return Err(ProbeError::unsupported(
                "ICMPv6 后端尚未实装；IPv6 目标请改用 TCP 方式（--mode tcp）",
            ));
        };
        let id = std::process::id() as u16;
        let seq = next_seq();
        let payload = self.payload_len;

        let mut last: Option<ProbeError> = None;
        for sock_type in [posix_ffi::SOCK_DGRAM, posix_ffi::SOCK_RAW] {
            match unsafe { Self::try_once(sock_type, target, id, seq, payload, timeout) } {
                Ok(outcome) => return Ok(outcome),
                Err(err) => {
                    // 权限不足是「能力问题」，两种套接字都会失败，但仍都试一次以留下明确日志
                    last = Some(err);
                }
            }
        }
        Err(last.unwrap_or_else(|| ProbeError::internal("无法创建 ICMP 套接字")))
    }
}

// ---------------------------------------------------------------------------
// TCP 连通性（**与 ICMP 严格区分**）
// ---------------------------------------------------------------------------

/// TCP 连接探测。
///
/// 用途：在 ICMP 不可用的平台（典型是 Android 普通应用权限）作为**独立模式**提供。
/// 它的 `name()` 明确标注 `tcp-connect`，界面与导出都必须显示为 TCP 语义，
/// **不得**计入「在线(ICMP)」统计。
#[derive(Debug, Clone)]
pub struct TcpConnectBackend {
    port: u16,
}

impl TcpConnectBackend {
    pub fn new(port: u16) -> Self {
        Self { port }
    }
}

impl Default for TcpConnectBackend {
    fn default() -> Self {
        Self { port: 80 }
    }
}

impl ProbeBackend for TcpConnectBackend {
    fn name(&self) -> &'static str {
        "tcp-connect"
    }

    fn probe(&self, target: IpAddr, timeout: Duration) -> Result<ProbeOutcome, ProbeError> {
        // 是否可达用 ICMP 判定；这里只做 TCP 握手能否建立。
        // M12：`SocketAddr::new` 同时支持 v4 / v6，所以 IPv6 目标在这里是
        // 原生可用的——IPv6 支持以 TCP 方式为完整路径。
        let started = std::time::Instant::now();
        match std::net::TcpStream::connect_timeout(
            &std::net::SocketAddr::new(target, self.port),
            timeout,
        ) {
            Ok(_) => Ok(ProbeOutcome::online(started.elapsed().as_millis() as u32)),
            Err(e) => Err(match e.kind() {
                std::io::ErrorKind::TimedOut => ProbeError::timeout(timeout.as_millis() as u64),
                // 端口关闭 ≠ 平台不支持。主机显然有响应（发了 RST），只是该端口没开。
                // 归到 Unreachable（服务不可达），界面在 TCP 模式下会显示成「TCP-关闭」。
                std::io::ErrorKind::ConnectionRefused => {
                    ProbeError::unreachable("TCP 端口未开放（主机有响应）")
                }
                std::io::ErrorKind::PermissionDenied => {
                    ProbeError::permission_denied(e.to_string())
                }
                _ => ProbeError::unreachable(e.to_string()),
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// 工厂
// ---------------------------------------------------------------------------

/// 当前平台的原生 ICMP 后端。
pub fn platform_icmp_backend() -> Box<dyn ProbeBackend> {
    #[cfg(windows)]
    {
        Box::new(WindowsIcmpBackend::new())
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        Box::new(PosixIcmpBackend::new())
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "android")))]
    {
        struct Unsupported;
        impl ProbeBackend for Unsupported {
            fn name(&self) -> &'static str {
                "unsupported"
            }
            fn probe(&self, _t: IpAddr, _to: Duration) -> Result<ProbeOutcome, ProbeError> {
                Err(ProbeError::unsupported("该平台尚未实现原生 ICMP 后端"))
            }
        }
        Box::new(Unsupported)
    }
}

/// 按探测方式返回后端（M9）。
///
/// **一次任务只有一种方式**，所以这里不需要「组合后端」：ICMP 与 TCP 结果
/// 永远不会出现在同一次扫描里，统计口径天然不混。
pub fn backend_for_mode(mode: crate::spec::ProbeMode) -> Box<dyn ProbeBackend> {
    match mode {
        crate::spec::ProbeMode::Icmp => platform_icmp_backend(),
        crate::spec::ProbeMode::Tcp { port } => Box::new(TcpConnectBackend::new(port)),
    }
}

/// 平台默认探测方式。
///
/// - 桌面（Windows/Linux）：默认 ICMP（普通用户即可用，语义最准确）。
/// - **Android：默认 TCP:80**。普通应用拿不到 `CAP_NET_RAW`，ICMP 必然全部
///   「权限不足」；把默认值设成一个能真正出结果的方式，比让用户对着一屏
///   错误发呆更有用。界面与导出都会显著标注 `TCP-OPEN`，不会与 ICMP 混淆。
pub const fn platform_default_mode() -> crate::spec::ProbeMode {
    #[cfg(target_os = "android")]
    {
        crate::spec::ProbeMode::Tcp { port: 80 }
    }
    #[cfg(not(target_os = "android"))]
    {
        crate::spec::ProbeMode::Icmp
    }
}

/// 本机是否可能具备原生 ICMP 能力（仅作提示，**不作为保证**）。
///
/// 真正的判定必须依据 `probe()` 返回的错误种类。
pub fn native_icmp_hint() -> &'static str {
    native_icmp_hint_in(Lang::Zh)
}

/// 按语言渲染的平台提示。
///
/// 平台分支用 `cfg!` 而不是 `#[cfg]`：四种分支都编译进二进制、
/// 由运行时挑一个。这样开发机上也能测到「英文 + Android 那一句」，
/// 而 `#[cfg]` 写法下非本平台的分支根本编不进来，漏翻要等发布才发现。
pub fn native_icmp_hint_in(lang: Lang) -> &'static str {
    let key = if cfg!(windows) {
        Key::HintIcmpWindows
    } else if cfg!(target_os = "android") {
        Key::HintIcmpAndroid
    } else if cfg!(target_os = "linux") {
        Key::HintIcmpLinux
    } else {
        Key::HintIcmpUnknown
    };
    i18n::text(lang, key)
}
