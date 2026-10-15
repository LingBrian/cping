//! 资产识别（M13，v2.1.0）。
//!
//! 三件事，边界刻意分清：
//!
//! 1. **端口指纹**：对单个主机并行 TCP 连接一组常见端口，输出「开放端口」列表。
//!    这是**独立于在线判定**的第二趟探测——端口全关不影响「在线」结论，
//!    端口开放也不把离线主机变在线。两份结果在界面与导出里**分列**。
//! 2. **服务横幅**：连接开放端口后**只读**服务主动送出的前若干字节
//!    （SSH/FTP/SMTP 这类先说话的协议才有内容；HTTP 不发就不会有）。
//!    结果原样保存，**绝不猜测协议或版本**——「不猜测」写在这里就是要被执行的约束。
//! 3. **设备类型推断**：基于「厂商 + 开放端口组合」给一条**推断**结论。
//!    返回类型的名字就叫 `DeviceGuess`（guess，不是 fact），界面必须标注「推断」。
//!    未识别的组合返回 `None`——**留空而非臆造**。
//!
//! 指纹规则表是纯数据（常量数组），匹配是纯函数，全部可离线单测。

use std::net::{IpAddr, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// 端口 → 服务名（纯数据；只列有把握的，宁可留空也不写「可能是」）
// ---------------------------------------------------------------------------

/// 已知端口 → 服务名。**纯数据表**，新增条目不涉及逻辑改动。
pub const PORT_SERVICES: &[(u16, &str)] = &[
    (21, "ftp"),
    (22, "ssh"),
    (23, "telnet"),
    (25, "smtp"),
    (53, "dns"),
    (80, "http"),
    (110, "pop3"),
    (123, "ntp"),
    (135, "msrpc"),
    (139, "netbios-ssn"),
    (143, "imap"),
    (161, "snmp"),
    (389, "ldap"),
    (443, "https"),
    (445, "smb"),
    (465, "smtps"),
    (587, "submission"),
    (631, "ipp"),
    (993, "imaps"),
    (995, "pop3s"),
    (1433, "mssql"),
    (1521, "oracle"),
    (1883, "mqtt"),
    (2049, "nfs"),
    (3000, "dev-http"),
    (3306, "mysql"),
    (3389, "rdp"),
    (5000, "upnp/dev-http"),
    (5353, "mdns"),
    (5432, "postgresql"),
    (5555, "adb"),
    (5900, "vnc"),
    (6379, "redis"),
    (8000, "dev-http"),
    (8080, "http-alt"),
    (8443, "https-alt"),
    (8888, "http-alt"),
    (9000, "dev-http"),
    (9100, "printer"),
    (27017, "mongodb"),
];

/// 已知端口的服务名；未收录的端口返回 `None`（不猜）。
pub fn port_service_name(port: u16) -> Option<&'static str> {
    PORT_SERVICES
        .iter()
        .find(|(p, _)| *p == port)
        .map(|(_, name)| *name)
}

/// 默认指纹端口集：跨平台常见且连接成本低（避免 >10000 的高位埠长尾）。
pub const DEFAULT_FINGERPRINT_PORTS: &[u16] = &[
    21, 22, 23, 25, 53, 80, 111, 135, 139, 443, 445, 631, 1433, 1883, 3000, 3306, 3389, 5000, 5432,
    5555, 5900, 6379, 8000, 8080, 8443, 8888, 9000, 9100,
];

/// 单主机最多探测的端口数（防止用户传入 65536 全端口把一台主机变成扫描器）。
pub const MAX_PORTS_PER_HOST: usize = 512;

/// 横幅抓取的最大字节数。**只读这么多**，绝不写一个字节（不做协议交互）。
pub const BANNER_MAX_BYTES: usize = 256;

// ---------------------------------------------------------------------------
// 配置与结果模型
// ---------------------------------------------------------------------------

/// 端口指纹扫描配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortScanConfig {
    /// 要探测的端口列表（会去重、升序、截断到 [`MAX_PORTS_PER_HOST`]）。
    pub ports: Vec<u16>,
    /// **每个端口独立**的连接超时。
    pub timeout: Duration,
    /// 同时打向同一主机的连接数上限（对单主机限速，别把对端打满）。
    pub max_concurrent: usize,
    /// 开放端口是否尝试抓取横幅。
    pub grab_banner: bool,
    /// 横幅最大保留字节。
    pub banner_max_bytes: usize,
}

impl Default for PortScanConfig {
    fn default() -> Self {
        Self {
            ports: DEFAULT_FINGERPRINT_PORTS.to_vec(),
            timeout: Duration::from_millis(800),
            max_concurrent: 8,
            grab_banner: true,
            banner_max_bytes: BANNER_MAX_BYTES,
        }
    }
}

impl PortScanConfig {
    /// 归一化：去重、升序、去掉 0、截断。构造后应立即调用一次。
    pub fn normalized(mut self) -> Self {
        self.ports.retain(|p| *p != 0);
        self.ports.sort_unstable();
        self.ports.dedup();
        self.ports.truncate(MAX_PORTS_PER_HOST);
        if self.max_concurrent == 0 {
            self.max_concurrent = 1;
        }
        if self.banner_max_bytes == 0 {
            self.banner_max_bytes = BANNER_MAX_BYTES;
        }
        self
    }

    /// 从「22,80,443-445,8080」这类文本解析；支持单个与闭区间。
    pub fn parse_ports(text: &str) -> Result<Vec<u16>, String> {
        let mut out: Vec<u16> = Vec::new();
        for part in text.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            if let Some((lo, hi)) = part.split_once('-') {
                let lo: u64 = lo
                    .trim()
                    .parse()
                    .map_err(|_| format!("端口区间起点 {lo:?} 不是数字"))?;
                let hi: u64 = hi
                    .trim()
                    .parse()
                    .map_err(|_| format!("端口区间终点 {hi:?} 不是数字"))?;
                if lo == 0 || hi == 0 || lo > hi || hi > u16::MAX as u64 {
                    return Err(format!(
                        "端口区间 {part:?} 无效（需 1-65535 且起点 ≤ 终点）"
                    ));
                }
                if hi - lo > MAX_PORTS_PER_HOST as u64 {
                    return Err(format!(
                        "端口区间 {part:?} 跨度超过上限 {MAX_PORTS_PER_HOST}"
                    ));
                }
                out.extend((lo..=hi).map(|p| p as u16));
            } else {
                let p: u64 = part
                    .parse()
                    .map_err(|_| format!("端口 {part:?} 不是数字"))?;
                if p == 0 || p > u16::MAX as u64 {
                    return Err(format!("端口 {part:?} 超出 1-65535"));
                }
                out.push(p as u16);
            }
        }
        if out.is_empty() {
            return Err("端口列表为空".into());
        }
        Ok(out)
    }
}

/// 单个开放端口的发现结果。关闭端口**不产生**条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortFinding {
    pub port: u16,
    /// 已知服务名；未收录端口为 `None`（不猜）。
    pub service: Option<&'static str>,
    /// 服务主动送出的横幅（可选，只在 `grab_banner` 且对方先说话时有）。
    pub banner: Option<String>,
    /// TCP 握手耗时（毫秒）。
    pub connect_rtt_ms: u32,
}

impl PortFinding {
    /// 「22(ssh)」风格的紧凑展示。
    pub fn summary(&self) -> String {
        match self.service {
            Some(name) => format!("{}({name})", self.port),
            None => format!("{}", self.port),
        }
    }
}

/// 一台主机的端口指纹报告。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortScanReport {
    pub ip: IpAddr,
    /// 只含**开放**端口，按端口号升序。
    pub findings: Vec<PortFinding>,
    /// 实际尝试的端口数（≠ findings.len()，差值就是关闭/超时的）。
    pub scanned: usize,
    /// 本次探测总耗时（毫秒）。
    pub elapsed_ms: u64,
}

impl PortScanReport {
    /// 端口列表的紧凑展示：「22(ssh) 80(http) 443(https)」。全关时为空串。
    pub fn open_ports_summary(&self) -> String {
        self.findings
            .iter()
            .map(|f| f.summary())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

// ---------------------------------------------------------------------------
// 网络动作：单端口探测 / 横幅 / 整机指纹
// ---------------------------------------------------------------------------

/// 尝试单个 TCP 端口。开放返回握手耗时，未开放/超时/被拒返回 `None`。
///
/// 判定口径与 `TcpConnectBackend` 一致：`ConnectionRefused` 是「主机有响应、
/// 端口没开」，其余错误一律视为「未开放」——指纹阶段不区分失败原因，
/// 失败原因属于连通性扫描（ICMP/TCP 模式）的职责。
pub fn probe_port(ip: IpAddr, port: u16, timeout: Duration) -> Option<u32> {
    let started = Instant::now();
    match TcpStream::connect_timeout(&SocketAddr::new(ip, port), timeout) {
        Ok(_) => Some(started.elapsed().as_millis() as u32),
        Err(_) => None,
    }
}

/// 抓取横幅：连接成功后**只读**一次（读到 EOF / 超时 / 上限为止），
/// 把字节清洗成可打印文本。失败一律返回 `None`——横幅是锦上添花，不是必需品。
pub fn grab_banner(ip: IpAddr, port: u16, timeout: Duration, max_bytes: usize) -> Option<String> {
    let mut stream = TcpStream::connect_timeout(&SocketAddr::new(ip, port), timeout).ok()?;
    stream.set_read_timeout(Some(timeout)).ok()?;
    stream.set_write_timeout(Some(timeout)).ok()?;
    let cap = max_bytes.min(BANNER_MAX_BYTES);
    let mut buf = vec![0u8; cap];
    let mut taken = 0;
    loop {
        if taken >= cap {
            break;
        }
        match std::io::Read::read(&mut stream, &mut buf[taken..]) {
            Ok(0) => break,
            Ok(n) => taken += n,
            Err(_) => break, // 超时或被重置：拿多少算多少
        }
    }
    if taken == 0 {
        return None;
    }
    sanitize_banner(&buf[..taken])
}

/// 清洗横幅：控制字符（除 \t）替换为空格、去掉首尾空白、截断到合理长度。
/// 纯函数，字符串处理无网络。
pub fn sanitize_banner(bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(bytes);
    let cleaned: String = text
        .chars()
        .map(|c| {
            if c == '\t' || c == '\n' || c == '\r' || (!c.is_control() && c != '\u{fffd}') {
                c
            } else {
                ' '
            }
        })
        .collect();
    let trimmed = cleaned.split('\n').next().unwrap_or("").trim();
    if trimmed.is_empty() {
        None
    } else {
        let mut out: String = trimmed.chars().take(120).collect();
        // 丢掉可能在字符边界截出的半个多字节序列的风险已由 chars() 规避；
        // 120 字符截断后如果原文更长，补一个省略标记。
        if trimmed.chars().count() > 120 {
            out.push('…');
        }
        Some(out)
    }
}

/// 对一台主机做端口指纹扫描（并行受 `config.max_concurrent` 限制）。
///
/// 实现说明：`std::thread::scope` + 简单分块——单主机端口数被
/// [`MAX_PORTS_PER_HOST`]（512）封顶，直接按并发数分块串行每块，
/// 块内线程并行，避免引线程池依赖。
pub fn scan_ports(ip: IpAddr, config: &PortScanConfig) -> PortScanReport {
    let cfg = config.clone().normalized();
    let started = Instant::now();
    let mut findings: Vec<PortFinding> = Vec::new();

    for chunk in cfg.ports.chunks(cfg.max_concurrent) {
        let results: Vec<Option<PortFinding>> = std::thread::scope(|scope| {
            let handles: Vec<_> = chunk
                .iter()
                .map(|port| {
                    let port = *port;
                    scope.spawn(move || {
                        let rtt = probe_port(ip, port, cfg.timeout)?;
                        let banner = if cfg.grab_banner {
                            grab_banner(ip, port, cfg.timeout, cfg.banner_max_bytes)
                        } else {
                            None
                        };
                        Some(PortFinding {
                            service: port_service_name(port),
                            port,
                            banner,
                            connect_rtt_ms: rtt,
                        })
                    })
                })
                .collect();
            handles
                .into_iter()
                // 线程 panic 不应带崩整个扫描；该端口视为未开放（`None`）
                .map(|h| h.join().unwrap_or_default())
                .collect()
        });
        findings.extend(results.into_iter().flatten());
    }

    findings.sort_unstable_by_key(|f| f.port);
    PortScanReport {
        ip,
        scanned: cfg.ports.len(),
        findings,
        elapsed_ms: started.elapsed().as_millis() as u64,
    }
}

// ---------------------------------------------------------------------------
// 设备类型推断（推断，不是事实）
// ---------------------------------------------------------------------------

/// 推断置信度。**没有 High**——TCP 指纹本就给不出「确定」，写了就是撒谎。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    /// 单一弱信号（比如只有一个通用端口开放）。
    Low,
    /// 多个信号互相印证（厂商 + 端口组合）。
    Medium,
}

impl Confidence {
    pub fn label_zh(&self) -> &'static str {
        match self {
            Confidence::Low => "低",
            Confidence::Medium => "中",
        }
    }
}

/// 设备类型**推断**结论。名字里带 Guess：界面必须标注「推断」而非事实。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceGuess {
    /// 设备/角色类型（中文短语，如「Windows 主机」「网络设备」）。
    pub kind: &'static str,
    pub confidence: Confidence,
    /// 推断依据（人可读，逐条列出信号）。
    pub basis: Vec<String>,
}

impl DeviceGuess {
    /// 单行展示：「Windows 主机（推断，置信中；依据：TCP 3389 开放）」。
    pub fn summary(&self) -> String {
        format!(
            "{}（推断，置信{}；依据：{}）",
            self.kind,
            self.confidence.label_zh(),
            self.basis.join("、")
        )
    }
}

/// 常见家用/网络设备厂商关键字（小写比较）。纯数据。
pub const NETWORK_VENDOR_KEYS: &[&str] = &[
    "tp-link",
    "asustek",
    "asus",
    "d-link",
    "huawei",
    "h3c",
    "ruijie",
    "zte",
    "xiaomi",
    "tplink",
    "cisco",
    "mikrotik",
    "netgear",
    "tenda",
    "mercury",
    "fiberhome",
    "gwdt",
    "ubiquiti",
    "aruba",
    "brocade",
    "juniper",
];

/// 检查厂商名是否命中网络设备关键字（大小写无关）。
pub fn vendor_is_network_equipment(vendor: Option<&str>) -> bool {
    let Some(v) = vendor else {
        return false;
    };
    let lower = v.to_ascii_lowercase();
    NETWORK_VENDOR_KEYS.iter().any(|k| lower.contains(k))
}

/// 基于厂商 + 开放端口推断设备类型。**未识别的组合返回 `None`（留空而非臆造）**。
///
/// 规则按特异性排序：多信号组合优先于单端口。全部规则都是纯数据驱动，
/// 加规则 = 加分支，改完跑单测即可。
pub fn infer_device(vendor: Option<&str>, open_ports: &[u16]) -> Option<DeviceGuess> {
    let has = |p: u16| open_ports.contains(&p);
    let net_vendor = vendor_is_network_equipment(vendor);

    let basis_ports = |ports: &[u16]| -> Vec<String> {
        ports
            .iter()
            .map(|p| match port_service_name(*p) {
                Some(name) => format!("TCP {p}({name}) 开放"),
                None => format!("TCP {p} 开放"),
            })
            .collect()
    };

    let mut basis: Vec<String> = Vec::new();
    if let Some(v) = vendor {
        basis.push(format!("厂商 {v}"));
    }

    // 规则 1：RDP + （SMB 或厂商含 Windows 系线索）→ Windows 主机
    if has(3389) {
        basis.extend(basis_ports(&[3389]));
        let windows_hint = has(445) || has(135) || has(139);
        if windows_hint {
            let extra: Vec<u16> = [445u16, 135, 139].into_iter().filter(|p| has(*p)).collect();
            basis.extend(basis_ports(&extra));
        }
        return Some(DeviceGuess {
            kind: "Windows 主机",
            confidence: if windows_hint {
                Confidence::Medium
            } else {
                Confidence::Low
            },
            basis,
        });
    }

    // 规则 2：网络设备厂商 + 管理口（80/443/8080/22 任一）→ 网络设备
    if net_vendor && (has(80) || has(443) || has(8080) || has(22) || has(23)) {
        let mgmt: Vec<u16> = [80, 443, 8080, 22, 23]
            .iter()
            .copied()
            .filter(|p| has(*p))
            .collect();
        basis.extend(basis_ports(&mgmt));
        return Some(DeviceGuess {
            kind: "网络设备（路由器/交换机）",
            confidence: Confidence::Medium,
            basis,
        });
    }

    // 规则 3：SMB → Windows/Samba 文件服务
    if has(445) {
        basis.extend(basis_ports(&[445]));
        return Some(DeviceGuess {
            kind: "Windows/Samba 文件服务",
            confidence: Confidence::Low,
            basis,
        });
    }

    // 规则 4：SSH → Linux/Unix 主机
    if has(22) {
        basis.extend(basis_ports(&[22]));
        return Some(DeviceGuess {
            kind: "Linux/Unix 主机",
            confidence: Confidence::Low,
            basis,
        });
    }

    // 规则 5：打印端口 → 打印机
    if has(9100) || has(631) {
        let ps: Vec<u16> = [9100, 631].iter().copied().filter(|p| has(*p)).collect();
        basis.extend(basis_ports(&ps));
        return Some(DeviceGuess {
            kind: "打印机",
            confidence: if has(9100) && has(631) {
                Confidence::Medium
            } else {
                Confidence::Low
            },
            basis,
        });
    }

    // 规则 6：VNC → 远程桌面/带外管理
    if has(5900) {
        basis.extend(basis_ports(&[5900]));
        return Some(DeviceGuess {
            kind: "远程桌面/VNC 端点",
            confidence: Confidence::Low,
            basis,
        });
    }

    // 规则 7：数据库端口 → 数据库服务器
    if has(3306) || has(5432) || has(1433) || has(6379) || has(27017) {
        let dbs: Vec<u16> = [3306, 5432, 1433, 6379, 27017]
            .iter()
            .copied()
            .filter(|p| has(*p))
            .collect();
        basis.extend(basis_ports(&dbs));
        return Some(DeviceGuess {
            kind: "数据库服务器",
            confidence: Confidence::Low,
            basis,
        });
    }

    // 规则 8：只有 Web 端口 → Web 服务（最弱的收尾规则）
    if has(80) || has(443) || has(8080) {
        let webs: Vec<u16> = [80, 443, 8080]
            .iter()
            .copied()
            .filter(|p| has(*p))
            .collect();
        basis.extend(basis_ports(&webs));
        return Some(DeviceGuess {
            kind: "Web 服务",
            confidence: Confidence::Low,
            basis,
        });
    }

    None
}

/// 便利封装：对一台主机完成「端口指纹 + 推断」。
pub fn fingerprint_host(
    ip: IpAddr,
    vendor: Option<&str>,
    config: &PortScanConfig,
) -> (PortScanReport, Option<DeviceGuess>) {
    let report = scan_ports(ip, config);
    let ports: Vec<u16> = report.findings.iter().map(|f| f.port).collect();
    let guess = infer_device(vendor, &ports);
    (report, guess)
}

/// 一台主机的完整指纹条目：端口报告 + 设备推断。
///
/// 这是**三端共享**的收敛点：CLI / TUI / GUI 都从这里取结果，
/// 谁也不得自行组织「哪些端口算开放」「怎么推断设备」。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostFingerprint {
    pub report: PortScanReport,
    /// 推断结论；未识别组合为 `None`（界面留空，不得填占位文本）。
    pub device: Option<DeviceGuess>,
}

impl HostFingerprint {
    pub fn ip(&self) -> IpAddr {
        self.report.ip
    }

    /// 「22(ssh) 80(http)」风格；无开放端口时为空串。
    pub fn open_ports_summary(&self) -> String {
        self.report.open_ports_summary()
    }

    /// 设备推断摘要；无结论时为空串（**不是**「未知」）。
    pub fn device_summary(&self) -> String {
        self.device
            .as_ref()
            .map(|d| d.summary())
            .unwrap_or_default()
    }

    /// 是否发现了开放端口。
    pub fn has_open_ports(&self) -> bool {
        !self.report.findings.is_empty()
    }
}

/// 批量指纹：给定主机（及可选厂商名），逐台完成端口指纹 + 推断。
///
/// 主机之间**串行**、单主机内部按 `max_concurrent` 并行——这是刻意的：
/// 对整网段同时开端口扫描会把自己的上行打满，也会在对方设备上留下
/// 「同源并发扫描」的明显痕迹。指纹是补充信息，不该喧宾夺主。
pub fn fingerprint_hosts(
    hosts: &[(IpAddr, Option<String>)],
    config: &PortScanConfig,
) -> Vec<HostFingerprint> {
    hosts
        .iter()
        .map(|(ip, vendor)| {
            let (report, device) = fingerprint_host(*ip, vendor.as_deref(), config);
            HostFingerprint { report, device }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;

    fn open_local_listener() -> (TcpListener, u16) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        (listener, port)
    }

    /// 起一个「问候式」服务端线程：在 `serve_for` 时间窗内接受任意数量的连接，
    /// 每个连接写 `greeting` 后停一会儿再处理下一个。
    ///
    /// **必须非阻塞 poll**：阻塞式 `accept()` 会在最后一个连接之后永久等待，
    /// 让 `join()` 把测试挂死（本项目真实踩过这个坑）。
    fn spawn_greeter(
        listener: TcpListener,
        greeting: &'static str,
        serve_for: Duration,
    ) -> std::thread::JoinHandle<()> {
        listener.set_nonblocking(true).expect("nonblocking");
        std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + serve_for;
            while std::time::Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut conn, _)) => {
                        let _ = conn.write_all(greeting.as_bytes());
                        // 故意不关：让读端自然超时（这正是横幅抓取的现实情形）
                        std::thread::sleep(Duration::from_millis(200));
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        })
    }

    // --- 端口服务表 --------------------------------------------------------

    #[test]
    fn port_service_table_covers_documented_entries() {
        assert_eq!(port_service_name(22), Some("ssh"));
        assert_eq!(port_service_name(3389), Some("rdp"));
        assert_eq!(port_service_name(445), Some("smb"));
        assert_eq!(port_service_name(9100), Some("printer"));
        // 未收录端口必须 None，不得猜
        assert_eq!(port_service_name(1), None);
        assert_eq!(port_service_name(40000), None);
    }

    #[test]
    fn service_table_is_sorted_and_unique() {
        let mut ports: Vec<u16> = PORT_SERVICES.iter().map(|(p, _)| *p).collect();
        let sorted = ports.clone();
        ports.sort_unstable();
        assert_eq!(ports, sorted, "表应按端口升序");
        ports.dedup();
        assert_eq!(ports.len(), PORT_SERVICES.len(), "端口不得重复");
    }

    #[test]
    fn default_ports_all_have_reasonable_services_or_are_common() {
        for p in DEFAULT_FINGERPRINT_PORTS {
            assert_ne!(*p, 0);
        }
        // 常见管理端口必须在默认集里（这是「资产识别」的基本盘）
        for expected in [22u16, 80, 443, 445, 3389] {
            assert!(
                DEFAULT_FINGERPRINT_PORTS.contains(&expected),
                "默认端口集缺少 {expected}"
            );
        }
    }

    // --- 端口列表解析 ------------------------------------------------------

    #[test]
    fn parse_ports_accepts_single_range_and_mix() {
        assert_eq!(
            PortScanConfig::parse_ports("22,80,443-445").unwrap(),
            vec![22, 80, 443, 444, 445]
        );
        assert_eq!(PortScanConfig::parse_ports(" 8080 ").unwrap(), vec![8080]);
        assert_eq!(PortScanConfig::parse_ports("").unwrap_err(), "端口列表为空");
    }

    #[test]
    fn parse_ports_rejects_garbage_and_overflow() {
        assert!(PortScanConfig::parse_ports("abc").is_err());
        assert!(PortScanConfig::parse_ports("0").is_err());
        assert!(PortScanConfig::parse_ports("65536").is_err());
        assert!(PortScanConfig::parse_ports("445-80").is_err(), "起点>终点");
        assert!(PortScanConfig::parse_ports("1-99999").is_err());
        assert!(PortScanConfig::parse_ports("1-600").is_err(), "跨度超上限");
    }

    #[test]
    fn normalized_config_dedups_sorts_and_truncates() {
        let cfg = PortScanConfig {
            ports: vec![80, 22, 80, 0, 443],
            max_concurrent: 0,
            banner_max_bytes: 0,
            ..Default::default()
        }
        .normalized();
        assert_eq!(cfg.ports, vec![22, 80, 443]);
        assert_eq!(cfg.max_concurrent, 1, "0 并发应收敛为 1");
        assert_eq!(cfg.banner_max_bytes, BANNER_MAX_BYTES);

        let many: Vec<u16> = (1..=2000).collect();
        let cfg2 = PortScanConfig {
            ports: many,
            ..Default::default()
        }
        .normalized();
        assert_eq!(cfg2.ports.len(), MAX_PORTS_PER_HOST);
    }

    // --- 横幅清洗（纯函数）-------------------------------------------------

    #[test]
    fn sanitize_banner_keeps_printable_and_drops_control() {
        assert_eq!(
            sanitize_banner(b"SSH-2.0-OpenSSH_9.2\r\n").as_deref(),
            Some("SSH-2.0-OpenSSH_9.2")
        );
        assert_eq!(
            sanitize_banner(b"hello\x01\x02world").as_deref(),
            Some("hello  world")
        );
        assert_eq!(sanitize_banner(b"tab\tkept").as_deref(), Some("tab\tkept"));
        assert!(sanitize_banner(b"").is_none(), "空横幅为 None");
        assert!(sanitize_banner(b"\r\n\r\n").is_none(), "纯空行为 None");
    }

    #[test]
    fn sanitize_banner_takes_first_line_and_truncates_long_text() {
        let long = "x".repeat(300);
        let out = sanitize_banner(long.as_bytes()).unwrap();
        assert_eq!(out.chars().count(), 121, "120 字符 + 省略号");
        assert!(out.ends_with('…'));
    }

    // --- 真实 TCP 冒烟（本机回环，不依赖外网）-------------------------------

    #[test]
    fn probe_port_detects_open_and_closed_on_loopback() {
        let (listener, port) = open_local_listener();
        // 保持监听 2 秒后自动退出（非阻塞 poll，避免线程永久阻塞）
        let _keep = spawn_greeter(listener, "", Duration::from_secs(2));
        assert!(probe_port("127.0.0.1".parse().unwrap(), port, Duration::from_secs(2)).is_some());
        // 关闭端口：回环上内核立刻 RST
        assert!(probe_port("127.0.0.1".parse().unwrap(), 1, Duration::from_millis(500)).is_none());
    }

    #[test]
    fn scan_ports_finds_open_port_with_banner() {
        let (listener, port) = open_local_listener();
        // scan_ports 会连两次（探测一次 + 抓横幅一次），必须持续接受
        let greeter = spawn_greeter(listener, "SSH-2.0-CPingTest\r\n", Duration::from_secs(2));
        let cfg = PortScanConfig {
            ports: vec![port, 1],
            timeout: Duration::from_millis(400),
            max_concurrent: 4,
            grab_banner: true,
            banner_max_bytes: BANNER_MAX_BYTES,
        }
        .normalized();
        let report = scan_ports("127.0.0.1".parse().unwrap(), &cfg);
        assert_eq!(report.scanned, 2);
        assert_eq!(report.findings.len(), 1, "只应发现开放端口");
        let f = &report.findings[0];
        assert_eq!(f.port, port);
        assert!(f.connect_rtt_ms <= 400, "握手耗时应 ≤ 超时");
        assert_eq!(
            f.banner.as_deref(),
            Some("SSH-2.0-CPingTest"),
            "横幅原样抓取"
        );
        assert!(report.open_ports_summary().contains(&port.to_string()));
        greeter.join().unwrap();
    }

    #[test]
    fn scan_ports_reports_empty_when_all_closed() {
        let cfg = PortScanConfig {
            ports: vec![1, 2, 3],
            timeout: Duration::from_millis(300),
            max_concurrent: 3,
            grab_banner: false,
            banner_max_bytes: 64,
        }
        .normalized();
        let report = scan_ports("127.0.0.1".parse().unwrap(), &cfg);
        assert!(report.findings.is_empty());
        assert_eq!(report.scanned, 3);
        assert_eq!(report.open_ports_summary(), "");
    }

    #[test]
    fn scan_ports_respects_no_banner_mode() {
        let (listener, port) = open_local_listener();
        let greeter = spawn_greeter(listener, "SECRET-BANNER\r\n", Duration::from_millis(900));
        let cfg = PortScanConfig {
            ports: vec![port],
            timeout: Duration::from_millis(300),
            max_concurrent: 1,
            grab_banner: false,
            banner_max_bytes: 64,
        }
        .normalized();
        let report = scan_ports("127.0.0.1".parse().unwrap(), &cfg);
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].banner, None, "关闭 banner 时必须不读");
        greeter.join().unwrap();
    }

    // --- 设备推断（纯函数，全边界）-----------------------------------------

    #[test]
    fn rdp_wins_and_windows_hints_raise_confidence() {
        let g = infer_device(None, &[3389]).unwrap();
        assert_eq!(g.kind, "Windows 主机");
        assert_eq!(g.confidence, Confidence::Low);
        let g2 = infer_device(None, &[3389, 445]).unwrap();
        assert_eq!(g2.confidence, Confidence::Medium);
        assert!(g2.summary().contains("推断"), "展示必须带「推断」字样");
        assert!(g2.summary().contains("3389"));
    }

    #[test]
    fn network_vendor_plus_mgmt_port_means_network_device() {
        let g = infer_device(Some("TP-Link Technologies"), &[80]).unwrap();
        assert_eq!(g.kind, "网络设备（路由器/交换机）");
        assert_eq!(g.confidence, Confidence::Medium);
        // 同样端口但没有网络设备厂商 → 落到 Web 服务
        let g2 = infer_device(Some("Dell Inc."), &[80]).unwrap();
        assert_eq!(g2.kind, "Web 服务");
        assert_eq!(g2.confidence, Confidence::Low);
        // 厂商大小写无关
        assert!(vendor_is_network_equipment(Some("ASUSTek COMPUTER INC.")));
        assert!(vendor_is_network_equipment(Some("tp-link")));
        assert!(!vendor_is_network_equipment(Some("Dell")));
        assert!(!vendor_is_network_equipment(None));
    }

    #[test]
    fn printer_db_vnc_and_web_fallback_rules() {
        assert_eq!(infer_device(None, &[9100]).unwrap().kind, "打印机");
        assert_eq!(infer_device(None, &[631]).unwrap().kind, "打印机");
        assert_eq!(infer_device(None, &[3306]).unwrap().kind, "数据库服务器");
        assert_eq!(
            infer_device(None, &[5900]).unwrap().kind,
            "远程桌面/VNC 端点"
        );
        assert_eq!(infer_device(None, &[443]).unwrap().kind, "Web 服务");
        assert_eq!(infer_device(None, &[8080]).unwrap().kind, "Web 服务");
    }

    #[test]
    fn unrecognized_combination_is_none_not_a_guess() {
        // 没有任何已知端口
        assert_eq!(infer_device(None, &[19999]), None);
        // 空端口列表
        assert_eq!(infer_device(Some("TP-Link"), &[]), None);
        // 无厂商无端口
        assert_eq!(infer_device(None, &[]), None);
        // 高位埠不因为「像」什么就被归类
        assert_eq!(infer_device(Some("Raspberry Pi"), &[42000]), None);
    }

    #[test]
    fn fingerprint_hosts_batches_in_order_and_keeps_absent_device_empty() {
        let (listener, port) = open_local_listener();
        let _keep = spawn_greeter(listener, "", Duration::from_secs(1));
        let cfg = PortScanConfig {
            ports: vec![port],
            timeout: Duration::from_millis(400),
            max_concurrent: 2,
            grab_banner: false,
            banner_max_bytes: 64,
        }
        .normalized();
        let hosts = vec![
            (
                "127.0.0.1".parse::<IpAddr>().unwrap(),
                Some("TP-Link".to_string()),
            ),
            ("127.0.0.2".parse::<IpAddr>().unwrap(), None),
        ];
        let out = fingerprint_hosts(&hosts, &cfg);
        assert_eq!(out.len(), 2, "每台主机一条，顺序与入参一致");
        assert_eq!(out[0].ip(), "127.0.0.1".parse::<IpAddr>().unwrap());
        assert!(out[0].has_open_ports());
        // 动态端口不在规则表内 → 推断留空，展示为空串而非「未知」
        assert_eq!(out[0].device, None);
        assert_eq!(out[0].device_summary(), "");
        // 127.0.0.2 上大概率没有该端口的监听（回环子网），失败也不得 panic
        assert!(!out[1].device_summary().contains("未知"));
    }

    #[test]
    fn host_fingerprint_summary_marks_inference_when_present() {
        let hp = HostFingerprint {
            report: PortScanReport {
                ip: "10.0.0.5".parse().unwrap(),
                findings: vec![
                    PortFinding {
                        port: 22,
                        service: Some("ssh"),
                        banner: Some("SSH-2.0-x".into()),
                        connect_rtt_ms: 1,
                    },
                    PortFinding {
                        port: 80,
                        service: Some("http"),
                        banner: None,
                        connect_rtt_ms: 2,
                    },
                ],
                scanned: 2,
                elapsed_ms: 3,
            },
            device: infer_device(Some("TP-Link"), &[22, 80]),
        };
        assert_eq!(hp.open_ports_summary(), "22(ssh) 80(http)");
        assert!(hp.device_summary().contains("推断"));
        assert!(hp.device_summary().contains("网络设备"));
    }
}
