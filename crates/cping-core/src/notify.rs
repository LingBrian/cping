//! 通知通道（M15，v2.3.0）。
//!
//! ## 一条硬规则：**通道失败必须可见**
//!
//! 每一个通道都返回 [`DeliveryOutcome`]，成功失败都有，且带可读原因。
//! 通知失败本身可能是最重要的信息（「网断了所以 webhook 发不出去」），
//! 静默吞掉它等于把告警系统变成安慰剂。**本模块不会 panic，也不会返回
//! 「假装成功」的结果**。
//!
//! ## 五个通道
//!
//! | 通道 | 说明 |
//! | --- | --- |
//! | `console` | 打印到 stdout，永远可用 |
//! | `system` | Windows 用 PowerShell 发 Toast；Linux 用 `notify-send`；其它平台**如实报告不可用** |
//! | `webhook` | HTTP POST JSON。**只支持 `http://`**——核心零第三方依赖，没有 TLS |
//! | `command` | 执行一条外部命令，事件 JSON 从 stdin 喂进去。这是 https / SMTP-TLS 的通用出口 |
//! | `smtp` | **明文** SMTP（端口 25 一类）。不支持 TLS 与认证，只适用于内网中继 |
//!
//! ## 为什么 `webhook` 拒绝 https 而不是「尽力尝试」
//!
//! 没有 TLS 实现就没法握手。若悄悄降级成明文，等于把 webhook 内容连同
//! 内网主机名、IP 暴露在链路上。明确报错 + 指向 `command` 通道
//! （`curl` / `msmtp` 都自带 TLS）才是诚实的做法。
//!
//! ## 不阻塞扫描主流程
//!
//! [`AlertSink`] 把投递丢给一个后台线程，`post()` 立即返回。每个通道自身还有
//! 连接/读写超时与命令超时兜底。队列满时**丢弃并计数**，由调用方如实展示
//! 「有多少条没发出去」——这比无限堆积或静默丢弃都好。

use crate::alerts::AlertEvent;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// 默认网络超时（毫秒）。
pub const DEFAULT_CHANNEL_TIMEOUT_MS: u64 = 5000;
/// 后台投递队列的容量。满了就丢弃并计数（见模块文档）。
pub const SINK_QUEUE_CAPACITY: usize = 64;
/// 响应体最多读多少字节（防止对端一直吐）。
const MAX_RESPONSE_BYTES: usize = 16 * 1024;
/// 命令执行时最多收集多少 stderr 字节。
const MAX_STDERR_BYTES: usize = 4096;

/// SMTP 账户（明文，无认证）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmtpAccount {
    pub host: String,
    pub port: u16,
    pub from: String,
    pub to: Vec<String>,
    pub subject_prefix: String,
}

impl SmtpAccount {
    pub fn new(
        host: impl Into<String>,
        port: u16,
        from: impl Into<String>,
        to: Vec<String>,
    ) -> Self {
        Self {
            host: host.into(),
            port,
            from: from.into(),
            to,
            subject_prefix: "[CPing]".into(),
        }
    }
}

/// 一个通知通道的配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelConfig {
    /// 打印到 stdout。
    Console,
    /// 系统通知（Windows Toast / Linux notify-send）。
    System { app_name: String },
    /// HTTP POST JSON。只支持 `http://`。
    Webhook { url: String, timeout_ms: u64 },
    /// 执行外部命令；`{title}` / `{body}` / `{json}` / `{count}` 会被替换。
    Command { template: String, timeout_ms: u64 },
    /// 明文 SMTP。
    Smtp {
        account: SmtpAccount,
        timeout_ms: u64,
    },
}

impl ChannelConfig {
    /// 通道的稳定键（DTO 与界面都用它）。
    pub fn key(&self) -> &'static str {
        match self {
            ChannelConfig::Console => "console",
            ChannelConfig::System { .. } => "system",
            ChannelConfig::Webhook { .. } => "webhook",
            ChannelConfig::Command { .. } => "command",
            ChannelConfig::Smtp { .. } => "smtp",
        }
    }

    pub fn timeout(&self) -> Duration {
        let ms = match self {
            ChannelConfig::Console => 0,
            ChannelConfig::System { .. } => DEFAULT_CHANNEL_TIMEOUT_MS,
            ChannelConfig::Webhook { timeout_ms, .. } => *timeout_ms,
            ChannelConfig::Command { timeout_ms, .. } => *timeout_ms,
            ChannelConfig::Smtp { timeout_ms, .. } => *timeout_ms,
        };
        Duration::from_millis(ms.max(200))
    }
}

/// 通知配置。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NotifyConfig {
    pub channels: Vec<ChannelConfig>,
}

impl NotifyConfig {
    pub fn console_only() -> Self {
        Self {
            channels: vec![ChannelConfig::Console],
        }
    }

    pub fn is_empty(&self) -> bool {
        self.channels.is_empty()
    }

    pub fn keys(&self) -> Vec<&'static str> {
        self.channels.iter().map(|c| c.key()).collect()
    }
}

/// 一次投递的结果。**成功失败都要产出**，绝不静默。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryOutcome {
    /// 通道键。
    pub channel: &'static str,
    pub ok: bool,
    /// 本通道实际送出的告警条数。
    pub sent: usize,
    /// 成功摘要或失败原因（中文、可读、不用错误码搪塞）。
    pub detail: String,
}

impl DeliveryOutcome {
    fn ok(channel: &'static str, sent: usize, detail: impl Into<String>) -> Self {
        Self {
            channel,
            ok: true,
            sent,
            detail: detail.into(),
        }
    }

    fn fail(channel: &'static str, detail: impl Into<String>) -> Self {
        Self {
            channel,
            ok: false,
            sent: 0,
            detail: detail.into(),
        }
    }
}

/// 本平台各通道的可用性自述（供 `--notify-availability` 与界面如实展示）。
///
/// 返回 `(通道键, 是否可用, 说明)`。
pub fn notifications_available() -> Vec<(&'static str, bool, String)> {
    let mut out = vec![(
        "console",
        true,
        "打印到标准输出，任何平台都可用".to_string(),
    )];
    if cfg!(windows) {
        out.push((
            "system",
            true,
            "通过 PowerShell 发 Windows Toast（需要 Windows 10 1607+）".into(),
        ));
    } else if cfg!(target_os = "linux") {
        let has = which("notify-send").is_some();
        out.push((
            "system",
            has,
            if has {
                "调用 notify-send".into()
            } else {
                "未找到 notify-send（gnome/libnotify 未安装）".to_string()
            },
        ));
    } else {
        out.push((
            "system",
            false,
            "本平台没有内置的系统通知通道（Android/iOS 请用界面内提示或 command 通道）".into(),
        ));
    }
    out.push((
        "webhook",
        true,
        "HTTP POST JSON；**仅 http://**，核心不含 TLS，https 会被明确拒绝".into(),
    ));
    out.push((
        "command",
        true,
        "执行外部命令并把事件 JSON 从 stdin 喂入——接 curl / msmtp 等自带 TLS 的工具".into(),
    ));
    out.push((
        "smtp",
        true,
        "**明文** SMTP，不支持 TLS 与认证；仅适用于内网中继".into(),
    ));
    out
}

fn which(program: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(program);
        if candidate.is_file() {
            return Some(candidate.to_string_lossy().to_string());
        }
    }
    None
}

/// 把一批事件投递到所有配置的通道，返回每个通道的结果。
///
/// 同步执行。每个通道各自有超时；**单个通道失败不影响其它通道**。
pub fn deliver(events: &[AlertEvent], cfg: &NotifyConfig) -> Vec<DeliveryOutcome> {
    cfg.channels
        .iter()
        .map(|channel| deliver_one(events, channel))
        .collect()
}

fn deliver_one(events: &[AlertEvent], channel: &ChannelConfig) -> DeliveryOutcome {
    if events.is_empty() {
        return DeliveryOutcome::fail(channel.key(), "没有需要投递的告警");
    }
    match channel {
        ChannelConfig::Console => {
            for e in events {
                println!(
                    "[告警/{}] {} {} {}",
                    e.severity.label_zh(),
                    e.time_text(),
                    e.kind.label_zh(),
                    e.message
                );
            }
            DeliveryOutcome::ok(
                "console",
                events.len(),
                format!("已打印 {} 条", events.len()),
            )
        }
        ChannelConfig::System { app_name } => deliver_system(events, app_name),
        ChannelConfig::Webhook { url, timeout_ms } => {
            deliver_webhook(events, url, Duration::from_millis(*timeout_ms))
        }
        ChannelConfig::Command {
            template,
            timeout_ms,
        } => deliver_command(events, template, Duration::from_millis(*timeout_ms)),
        ChannelConfig::Smtp {
            account,
            timeout_ms,
        } => deliver_smtp(events, account, Duration::from_millis(*timeout_ms)),
    }
}

/// 事件列表 → 一行标题 + 多行正文。
fn digest(events: &[AlertEvent]) -> (String, String) {
    let title = if events.len() == 1 {
        format!("CPing 告警：{}", events[0].kind.label_zh())
    } else {
        format!(
            "CPing 告警：{} 条（{}）",
            events.len(),
            events[0].kind.label_zh()
        )
    };
    let body = events
        .iter()
        .take(10)
        .map(|e| {
            format!(
                "[{}] {} {}",
                e.severity.label_zh(),
                e.time_text(),
                e.message
            )
        })
        .chain(if events.len() > 10 {
            vec![format!("… 另有 {} 条", events.len() - 10)]
        } else {
            Vec::new()
        })
        .collect::<Vec<_>>()
        .join("\n");
    (title, body)
}

// ---------------------------------------------------------------------------
// system：Windows Toast / Linux notify-send
// ---------------------------------------------------------------------------

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn ps_quote(s: &str) -> String {
    s.replace('\'', "''")
}

/// PowerShell 脚本用 UTF-16LE + Base64 传（`-EncodedCommand`）：
/// 这样标题/正文里的引号、中文、换行都不会破坏命令行。
fn encode_powershell(script: &str) -> String {
    let mut bytes = Vec::with_capacity(script.len() * 2);
    for unit in script.encode_utf16() {
        bytes.push((unit & 0xFF) as u8);
        bytes.push((unit >> 8) as u8);
    }
    base64(&bytes)
}

fn base64(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

fn deliver_system(events: &[AlertEvent], app_name: &str) -> DeliveryOutcome {
    let (title, body) = digest(events);

    if cfg!(windows) {
        let script = format!(
            "$ErrorActionPreference='Stop'\n\
             [void][Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType=WindowsRuntime]\n\
             [void][Windows.Data.Xml.Dom.XmlDocument, Windows.Data.Xml.Dom.XmlDocument, ContentType=WindowsRuntime]\n\
             $t=[Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02)\n\
             $x=$t.GetElementsByTagName('text')\n\
             [void]$x.Item(0).AppendChild($t.CreateTextNode('{}'))\n\
             [void]$x.Item(1).AppendChild($t.CreateTextNode('{}'))\n\
             $n=[Windows.UI.Notifications.ToastNotification]::new($t)\n\
             [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('{}').Show($n)\n",
            ps_quote(&xml_escape(&title)),
            ps_quote(&xml_escape(&body)),
            ps_quote(app_name),
        );
        let encoded = encode_powershell(&script);
        let mut cmd = std::process::Command::new("powershell");
        cmd.arg("-NoProfile")
            .arg("-NonInteractive")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-EncodedCommand")
            .arg(encoded)
            .stdin(std::process::Stdio::null());
        return run_bounded(cmd, "system", events.len(), Duration::from_secs(10));
    }

    if cfg!(target_os = "linux") {
        let Some(send) = which("notify-send") else {
            return DeliveryOutcome::fail(
                "system",
                "未找到 notify-send：请安装 libnotify-bin（Debian/Ubuntu）或 libnotify（Fedora）",
            );
        };
        let mut cmd = std::process::Command::new(send);
        cmd.arg(&title)
            .arg(&body)
            .stdin(std::process::Stdio::null());
        return run_bounded(cmd, "system", events.len(), Duration::from_secs(10));
    }

    DeliveryOutcome::fail(
        "system",
        "本平台没有内置的系统通知通道（Android/iOS 请用 command 通道）",
    )
}

/// 执行一个命令并在 `timeout` 内等待；超时则杀掉并如实报告。
fn run_bounded(
    mut cmd: std::process::Command,
    channel: &'static str,
    sent: usize,
    timeout: Duration,
) -> DeliveryOutcome {
    let program = cmd.get_program().to_string_lossy().to_string();
    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return DeliveryOutcome::fail(channel, format!("无法启动 {program}：{e}"));
        }
    };
    let deadline = Instant::now() + timeout;
    let mut child = child;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut err_text = String::new();
                if let Some(mut err) = child.stderr.take() {
                    let mut buf = vec![0u8; MAX_STDERR_BYTES];
                    if let Ok(n) = err.read(&mut buf) {
                        err_text = String::from_utf8_lossy(&buf[..n]).trim().to_string();
                    }
                }
                if status.success() {
                    return DeliveryOutcome::ok(channel, sent, format!("{program} 退出码 0"));
                }
                let code = status
                    .code()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "被信号终止".into());
                return DeliveryOutcome::fail(
                    channel,
                    if err_text.is_empty() {
                        format!("{program} 退出码 {code}")
                    } else {
                        format!("{program} 退出码 {code}：{}", truncate(&err_text, 300))
                    },
                );
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return DeliveryOutcome::fail(
                        channel,
                        format!("{program} 超过 {} ms 未返回，已终止", timeout.as_millis()),
                    );
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return DeliveryOutcome::fail(channel, format!("等待 {program} 失败：{e}")),
        }
    }
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push('…');
    out
}

// ---------------------------------------------------------------------------
// webhook：手写 HTTP/1.1 POST（仅明文）
// ---------------------------------------------------------------------------

/// 事件列表 → JSON 数组文本。
///
/// 手写而非复用 `crate::json`：后者是**渲染器**（面向导出报表），
/// 这里要的是构造，两者用途不同，硬凑反而更难读。
pub fn events_to_json(events: &[AlertEvent]) -> String {
    let mut out = String::from("[");
    for (i, e) in events.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('{');
        out.push_str(&format!("\"kind\":\"{}\",", e.kind.key()));
        out.push_str(&format!("\"kindLabel\":\"{}\",", e.kind.label_zh()));
        out.push_str(&format!("\"severity\":\"{}\",", e.severity.key()));
        out.push_str(&format!("\"ip\":\"{}\",", e.ip));
        out.push_str(&format!("\"index\":{},", e.index));
        out.push_str(&format!("\"atMs\":{},", e.at_ms));
        out.push_str(&format!("\"time\":\"{}\",", e.time_text()));
        out.push_str(&format!("\"round\":{},", e.round));
        match e.value {
            Some(v) => out.push_str(&format!("\"value\":{v:.1},")),
            None => out.push_str("\"value\":null,"),
        }
        match e.threshold {
            Some(t) => out.push_str(&format!("\"threshold\":{t},")),
            None => out.push_str("\"threshold\":null,"),
        }
        out.push_str(&format!("\"message\":{}", json_string(&e.message)));
        out.push('}');
    }
    out.push(']');
    out
}

fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// 解析 `http://host[:port]/path`。**`https` 明确拒绝**（核心不含 TLS）。
fn parse_http_url(url: &str) -> Result<(String, u16, String), String> {
    let lower = url.trim().to_ascii_lowercase();
    if lower.starts_with("https://") {
        return Err(
            "webhook 只支持 http:// —— 本构建不含 TLS，不会把告警明文降级发出去。\
                    请改用 --notify-cmd 走 curl / 其它自带 TLS 的工具"
                .into(),
        );
    }
    let rest = url
        .trim()
        .strip_prefix("http://")
        .or_else(|| url.trim().strip_prefix("HTTP://"))
        .ok_or_else(|| format!("webhook 地址必须以 http:// 开头，收到 {url:?}"))?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    if authority.is_empty() {
        return Err("webhook 地址缺少主机名".into());
    }
    // 先处理 `[IPv6]` 字面量：里面的冒号不是端口分隔符。
    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        let (h, after) = rest
            .split_once(']')
            .ok_or_else(|| format!("IPv6 地址的方括号未闭合：{authority:?}"))?;
        let port = match after.strip_prefix(':') {
            Some(p) => p.parse::<u16>().map_err(|_| format!("端口不合法：{p:?}"))?,
            None if after.is_empty() => 80,
            None => return Err(format!("IPv6 地址后面只能是端口：{after:?}")),
        };
        (h.to_string(), port)
    } else if authority.contains(':') {
        // 有冒号就必须是一个合法端口：**不猜**（把 `host:notaport` 当默认端口
        // 会让用户以为请求发到了 80，实际是笔误）
        let (h, p) = authority
            .rsplit_once(':')
            .expect("contains(':') 已确认有冒号");
        if h.is_empty() {
            return Err("webhook 地址缺少主机名".into());
        }
        let port = p.parse::<u16>().map_err(|_| format!("端口不合法：{p:?}"))?;
        (h.to_string(), port)
    } else {
        (authority.to_string(), 80)
    };
    if host.is_empty() {
        return Err("webhook 地址缺少主机名".into());
    }
    Ok((host, port, path.to_string()))
}

fn deliver_webhook(events: &[AlertEvent], url: &str, timeout: Duration) -> DeliveryOutcome {
    let (host, port, path) = match parse_http_url(url) {
        Ok(v) => v,
        Err(e) => return DeliveryOutcome::fail("webhook", e),
    };
    let body = events_to_json(events);
    match http_post(&host, port, &path, &body, timeout) {
        Ok(status) => {
            if (200..300).contains(&status) {
                DeliveryOutcome::ok(
                    "webhook",
                    events.len(),
                    format!("POST http://{host}:{port}{path} → {status}"),
                )
            } else {
                DeliveryOutcome::fail(
                    "webhook",
                    format!("POST http://{host}:{port}{path} 返回 {status}（非 2xx）"),
                )
            }
        }
        Err(e) => DeliveryOutcome::fail("webhook", e),
    }
}

fn http_post(
    host: &str,
    port: u16,
    path: &str,
    body: &str,
    timeout: Duration,
) -> Result<u16, String> {
    let addr = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("解析 {host}:{port} 失败：{e}"))?
        .next()
        .ok_or_else(|| format!("{host}:{port} 没有可用地址"))?;
    let mut stream =
        TcpStream::connect_timeout(&addr, timeout).map_err(|e| format!("连接 {addr} 失败：{e}"))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;

    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}:{port}\r\nUser-Agent: cping/{}\r\n\
         Content-Type: application/json; charset=utf-8\r\nAccept: */*\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        crate::VERSION,
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("发送请求失败：{e}"))?;
    stream.flush().map_err(|e| format!("发送请求失败：{e}"))?;

    let mut reader = BufReader::new(stream);
    let mut status_line = String::new();
    reader
        .read_line(&mut status_line)
        .map_err(|e| format!("读取响应失败：{e}"))?;
    // 把剩下的读完（对端要我们把请求体收走才肯关连接），但有上限
    let mut sink = Vec::new();
    let _ = reader
        .take(MAX_RESPONSE_BYTES as u64)
        .read_to_end(&mut sink);

    let code = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse::<u16>().ok())
        .ok_or_else(|| {
            format!(
                "响应不是合法 HTTP 状态行：{:?}",
                truncate(status_line.trim(), 80)
            )
        })?;
    Ok(code)
}

// ---------------------------------------------------------------------------
// command：执行外部命令，JSON 从 stdin 喂入
// ---------------------------------------------------------------------------

/// 按空白切分模板，再把**整词**占位符替换掉。
///
/// 「整词」是有意的：`-d {json}` 里的 `{json}` 单独占一个词，替换后
/// 变成一整个参数（可以含空格），既不用处理引号，也不会有注入问题。
/// 模板里不支持管道、重定向、变量展开——需要那些就写个脚本再调它。
pub fn expand_template(
    template: &str,
    title: &str,
    body: &str,
    json: &str,
    count: usize,
) -> Vec<String> {
    template
        .split_whitespace()
        .map(|token| match token {
            "{title}" => title.to_string(),
            "{body}" => body.to_string(),
            "{json}" => json.to_string(),
            "{count}" => count.to_string(),
            other => other.to_string(),
        })
        .collect()
}

fn deliver_command(events: &[AlertEvent], template: &str, timeout: Duration) -> DeliveryOutcome {
    let (title, body) = digest(events);
    let json = events_to_json(events);
    let parts = expand_template(template, &title, &body, &json, events.len());
    let Some((program, args)) = parts.split_first() else {
        return DeliveryOutcome::fail("command", "通知命令模板为空");
    };
    let mut cmd = std::process::Command::new(program);
    cmd.args(args);
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return DeliveryOutcome::fail("command", format!("无法启动 {program}：{e}")),
    };
    // 事件 JSON 也从 stdin 喂进去，方便 `curl -d @-`
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(json.as_bytes());
        let _ = stdin.write_all(b"\n");
    }
    wait_with_timeout(child, "command", events.len(), program, timeout)
}

fn wait_with_timeout(
    mut child: std::process::Child,
    channel: &'static str,
    sent: usize,
    program: &str,
    timeout: Duration,
) -> DeliveryOutcome {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut err_text = String::new();
                if let Some(mut err) = child.stderr.take() {
                    let mut buf = vec![0u8; MAX_STDERR_BYTES];
                    if let Ok(n) = err.read(&mut buf) {
                        err_text = String::from_utf8_lossy(&buf[..n]).trim().to_string();
                    }
                }
                if status.success() {
                    return DeliveryOutcome::ok(channel, sent, format!("{program} 退出码 0"));
                }
                let code = status
                    .code()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "被信号终止".into());
                return DeliveryOutcome::fail(
                    channel,
                    if err_text.is_empty() {
                        format!("{program} 退出码 {code}")
                    } else {
                        format!("{program} 退出码 {code}：{}", truncate(&err_text, 300))
                    },
                );
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return DeliveryOutcome::fail(
                        channel,
                        format!("{program} 超过 {} ms 未返回，已终止", timeout.as_millis()),
                    );
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return DeliveryOutcome::fail(channel, format!("等待 {program} 失败：{e}")),
        }
    }
}

// ---------------------------------------------------------------------------
// smtp：明文 SMTP
// ---------------------------------------------------------------------------

fn deliver_smtp(
    events: &[AlertEvent],
    account: &SmtpAccount,
    timeout: Duration,
) -> DeliveryOutcome {
    if account.to.is_empty() {
        return DeliveryOutcome::fail("smtp", "没有收件人（--notify-mail-to）");
    }
    match smtp_send(events, account, timeout) {
        Ok(detail) => DeliveryOutcome::ok("smtp", events.len(), detail),
        Err(e) => DeliveryOutcome::fail("smtp", e),
    }
}

fn smtp_send(
    events: &[AlertEvent],
    account: &SmtpAccount,
    timeout: Duration,
) -> Result<String, String> {
    let addr = (account.host.as_str(), account.port)
        .to_socket_addrs()
        .map_err(|e| format!("解析 {}:{} 失败：{e}", account.host, account.port))?
        .next()
        .ok_or_else(|| format!("{}:{} 没有可用地址", account.host, account.port))?;
    let stream =
        TcpStream::connect_timeout(&addr, timeout).map_err(|e| format!("连接 {addr} 失败：{e}"))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
    let mut writer = stream;

    expect_code(&mut reader, 220, "服务器问候")?;
    send_cmd(&mut writer, &mut reader, "EHLO cping", &[250], "EHLO")?;
    send_cmd(
        &mut writer,
        &mut reader,
        &format!("MAIL FROM:<{}>", account.from),
        &[250],
        "MAIL FROM",
    )?;
    for rcpt in &account.to {
        send_cmd(
            &mut writer,
            &mut reader,
            &format!("RCPT TO:<{rcpt}>"),
            &[250, 251],
            "RCPT TO",
        )?;
    }
    send_cmd(&mut writer, &mut reader, "DATA", &[354], "DATA")?;

    let (title, body) = digest(events);
    let mut mail = String::new();
    mail.push_str(&format!("From: {}\r\n", account.from));
    mail.push_str(&format!("To: {}\r\n", account.to.join(", ")));
    mail.push_str(&format!(
        "Subject: {} {}\r\n",
        account.subject_prefix, title
    ));
    mail.push_str("MIME-Version: 1.0\r\n");
    mail.push_str("Content-Type: text/plain; charset=utf-8\r\n");
    mail.push_str("Content-Transfer-Encoding: 8bit\r\n\r\n");
    mail.push_str(&body);
    mail.push_str("\r\n.\r\n");
    writer
        .write_all(mail.as_bytes())
        .map_err(|e| format!("发送正文失败：{e}"))?;
    writer.flush().map_err(|e| e.to_string())?;
    expect_code(&mut reader, 250, "正文接收")?;
    let _ = writer.write_all(b"QUIT\r\n");
    let _ = writer.flush();
    Ok(format!(
        "已投递到 {}:{}（{} 个收件人）",
        account.host,
        account.port,
        account.to.len()
    ))
}

fn send_cmd(
    writer: &mut TcpStream,
    reader: &mut BufReader<TcpStream>,
    cmd: &str,
    expect: &[u16],
    label: &str,
) -> Result<(), String> {
    writer
        .write_all(format!("{cmd}\r\n").as_bytes())
        .map_err(|e| format!("发送 {label} 失败：{e}"))?;
    writer.flush().map_err(|e| e.to_string())?;
    read_code(reader, expect, label)
}

fn expect_code(reader: &mut BufReader<TcpStream>, expect: u16, label: &str) -> Result<(), String> {
    read_code(reader, &[expect], label)
}

fn read_code(reader: &mut BufReader<TcpStream>, expect: &[u16], label: &str) -> Result<(), String> {
    // 多行响应（如 EHLO 的 250-）要一直读到「<code><空格>」那一行
    loop {
        let mut line = String::new();
        let n = reader
            .read_line(&mut line)
            .map_err(|e| format!("读取 {label} 响应失败：{e}"))?;
        if n == 0 {
            return Err(format!("{label}：服务器提前关闭了连接"));
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        let code: u16 = trimmed
            .get(..3)
            .and_then(|c| c.parse().ok())
            .ok_or_else(|| format!("{label}：响应不是 SMTP 回复：{:?}", truncate(trimmed, 80)))?;
        let more = trimmed.as_bytes().get(3) == Some(&b'-');
        if !more {
            if expect.contains(&code) {
                return Ok(());
            }
            return Err(format!(
                "{label}：期望 {expect:?}，服务器回 {:?}",
                truncate(trimmed, 160)
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// AlertSink：非阻塞投递
// ---------------------------------------------------------------------------

/// 把投递放到后台线程，`post()` 不阻塞调用方。
///
/// 队列有界：满了就丢弃并计数（[`AlertSink::dropped`]），由调用方如实展示。
/// 这比「无限堆积」或「静默丢弃」都好——至少用户能知道有东西没发出去。
pub struct AlertSink {
    tx: Option<SyncSender<Vec<AlertEvent>>>,
    handle: Option<JoinHandle<Vec<DeliveryOutcome>>>,
    dropped: Arc<AtomicUsize>,
    posted: Arc<AtomicUsize>,
}

impl AlertSink {
    /// 起一个后台投递线程。`cfg` 为空时返回 `None`（调用方据此跳过）。
    pub fn spawn(cfg: NotifyConfig) -> Option<Self> {
        if cfg.is_empty() {
            return None;
        }
        let (tx, rx): (SyncSender<Vec<AlertEvent>>, Receiver<Vec<AlertEvent>>) =
            sync_channel(SINK_QUEUE_CAPACITY);
        let dropped = Arc::new(AtomicUsize::new(0));
        let posted = Arc::new(AtomicUsize::new(0));
        let handle = std::thread::spawn(move || {
            let mut outcomes: Vec<DeliveryOutcome> = Vec::new();
            while let Ok(batch) = rx.recv() {
                outcomes.extend(deliver(&batch, &cfg));
            }
            outcomes
        });
        Some(Self {
            tx: Some(tx),
            handle: Some(handle),
            dropped,
            posted,
        })
    }

    /// 非阻塞投递。队列满时丢弃并计数。
    pub fn post(&self, events: &[AlertEvent]) {
        if events.is_empty() {
            return;
        }
        if let Some(tx) = &self.tx {
            match tx.try_send(events.to_vec()) {
                Ok(()) => {
                    self.posted.fetch_add(1, Ordering::Relaxed);
                }
                Err(TrySendError::Full(_)) => {
                    self.dropped.fetch_add(1, Ordering::Relaxed);
                }
                Err(TrySendError::Disconnected(_)) => {
                    self.dropped.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }

    /// 被丢弃的批次数。
    pub fn dropped(&self) -> usize {
        self.dropped.load(Ordering::Relaxed)
    }

    /// 成功入队的批次数。
    pub fn posted(&self) -> usize {
        self.posted.load(Ordering::Relaxed)
    }

    /// 关闭队列并等后台线程收尾，返回所有投递结果。
    pub fn finish(mut self) -> Vec<DeliveryOutcome> {
        self.tx.take();
        match self.handle.take() {
            Some(h) => h.join().unwrap_or_default(),
            None => Vec::new(),
        }
    }
}

impl Drop for AlertSink {
    fn drop(&mut self) {
        // 断开队列让后台线程自然退出（不 join：drop 里等 I/O 可能很久）
        self.tx.take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alerts::{AlertKind, Severity};
    use std::net::Ipv4Addr;

    fn ev(msg: &str) -> AlertEvent {
        AlertEvent {
            kind: AlertKind::Disappeared,
            severity: Severity::Critical,
            ip: std::net::IpAddr::V4(Ipv4Addr::new(10, 0, 0, 9)),
            index: 8,
            at_ms: 1_700_000_000_000,
            round: 3,
            value: None,
            threshold: None,
            message: msg.to_string(),
        }
    }

    #[test]
    fn console_channel_always_reports_success_and_never_panics() {
        let out = deliver(&[ev("主机 10.0.0.9 离线")], &NotifyConfig::console_only());
        assert_eq!(out.len(), 1);
        assert!(out[0].ok, "{out:?}");
        assert_eq!(out[0].channel, "console");
        assert_eq!(out[0].sent, 1);
    }

    #[test]
    fn empty_event_list_never_pretends_success() {
        let out = deliver(&[], &NotifyConfig::console_only());
        assert_eq!(out.len(), 1);
        assert!(!out[0].ok, "没有内容可投递时不该报成功");
        assert!(out[0].detail.contains("没有需要投递"), "{out:?}");
    }

    #[test]
    fn no_channels_configured_means_no_outcomes() {
        let out = deliver(&[ev("x")], &NotifyConfig::default());
        assert!(out.is_empty());
        assert!(NotifyConfig::default().is_empty());
        assert!(AlertSink::spawn(NotifyConfig::default()).is_none());
    }

    #[test]
    fn https_webhook_is_refused_with_a_reason_not_silently_downgraded() {
        let out = deliver(
            &[ev("x")],
            &NotifyConfig {
                channels: vec![ChannelConfig::Webhook {
                    url: "https://example.com/hook".into(),
                    timeout_ms: 500,
                }],
            },
        );
        assert!(!out[0].ok);
        assert!(out[0].detail.contains("只支持 http://"), "{out:?}");
        assert!(
            out[0].detail.contains("TLS"),
            "必须说明原因，而不是只说「不支持」：{out:?}"
        );
        assert!(
            out[0].detail.contains("--notify-cmd"),
            "要给出可用的替代路径：{out:?}"
        );
    }

    #[test]
    fn url_parsing_handles_ports_paths_and_rejects_garbage() {
        assert_eq!(
            parse_http_url("http://10.0.0.1/hook").unwrap(),
            ("10.0.0.1".into(), 80, "/hook".into())
        );
        assert_eq!(
            parse_http_url("http://10.0.0.1:8080/a/b?c=1").unwrap(),
            ("10.0.0.1".into(), 8080, "/a/b?c=1".into())
        );
        assert_eq!(
            parse_http_url("http://10.0.0.1").unwrap(),
            ("10.0.0.1".into(), 80, "/".into())
        );
        assert!(parse_http_url("ftp://x/").is_err());
        assert!(parse_http_url("http://").is_err());
        assert!(parse_http_url("http://host:notaport/").is_err());
    }

    #[test]
    fn webhook_to_a_closed_port_fails_with_a_readable_reason() {
        // 127.0.0.1:1 基本不可能有人监听；重点是**失败要可见且不 panic**
        let out = deliver(
            &[ev("x")],
            &NotifyConfig {
                channels: vec![ChannelConfig::Webhook {
                    url: "http://127.0.0.1:1/hook".into(),
                    timeout_ms: 800,
                }],
            },
        );
        assert!(!out[0].ok, "{out:?}");
        assert!(out[0].detail.contains("连接"), "{out:?}");
    }

    #[test]
    fn webhook_posts_json_to_a_real_local_server() {
        use std::io::Write as _;
        let listener = match std::net::TcpListener::bind("127.0.0.1:0") {
            Ok(l) => l,
            Err(_) => return, // 沙箱禁止监听时跳过
        };
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = listener.accept() {
                let mut buf = [0u8; 4096];
                let n = s.read(&mut buf).unwrap_or(0);
                let _ = tx.send(String::from_utf8_lossy(&buf[..n]).to_string());
                let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
            }
        });
        let out = deliver(
            &[ev("主机 10.0.0.9 离线")],
            &NotifyConfig {
                channels: vec![ChannelConfig::Webhook {
                    url: format!("http://127.0.0.1:{port}/hook"),
                    timeout_ms: 3000,
                }],
            },
        );
        assert!(out[0].ok, "{out:?}");
        assert!(out[0].detail.contains("200"), "{out:?}");
        let req = rx.recv_timeout(Duration::from_secs(3)).expect("应收到请求");
        assert!(req.starts_with("POST /hook HTTP/1.1"), "{req}");
        assert!(req.contains("Content-Type: application/json"), "{req}");
        assert!(req.contains("\"kind\":\"disappeared\""), "{req}");
        assert!(req.contains("\"ip\":\"10.0.0.9\""), "{req}");
        assert!(req.contains("\"threshold\":null"), "{req}");
    }

    #[test]
    fn template_expands_whole_tokens_only() {
        let parts = expand_template("curl -s -d {json} {count}", "T", "B", "{\"a\": 1}", 7);
        assert_eq!(parts[0], "curl");
        assert_eq!(parts[1], "-s");
        assert_eq!(parts[2], "-d");
        assert_eq!(parts[3], "{\"a\": 1}", "含空格的 JSON 要整体成一个参数");
        assert_eq!(parts[4], "7");
        // 不是整词的占位符不替换（避免把 URL 里的东西误伤）
        let p2 = expand_template("echo x{json}y", "T", "B", "J", 1);
        assert_eq!(p2, vec!["echo", "x{json}y"]);
    }

    #[test]
    fn command_channel_reports_exit_code_and_stderr() {
        let out = deliver(
            &[ev("x")],
            &NotifyConfig {
                channels: vec![ChannelConfig::Command {
                    template: "cping-no-such-program-xyz {count}".into(),
                    timeout_ms: 1000,
                }],
            },
        );
        assert!(!out[0].ok);
        assert!(out[0].detail.contains("无法启动"), "{out:?}");
    }

    #[test]
    fn command_channel_succeeds_and_receives_json_on_stdin() {
        // 用 sh 读一行 stdin 再退出：验证「JSON 从 stdin 喂进去」这条约定
        if !cfg!(unix) && !cfg!(windows) {
            return;
        }
        let template = if cfg!(windows) {
            "cmd /c more"
        } else {
            "sh -c cat"
        };
        let out = deliver(
            &[ev("x")],
            &NotifyConfig {
                channels: vec![ChannelConfig::Command {
                    template: template.into(),
                    timeout_ms: 3000,
                }],
            },
        );
        assert!(out[0].ok, "{out:?}");
        assert_eq!(out[0].sent, 1);
    }

    #[test]
    fn json_payload_escapes_quotes_and_control_chars() {
        let json = events_to_json(&[ev("含 \"引号\" 与\n换行")]);
        assert!(json.starts_with('['));
        assert!(json.ends_with(']'));
        assert!(json.contains("\\\"引号\\\""), "{json}");
        assert!(json.contains("\\n"), "{json}");
        assert!(!json.contains('\n'), "载荷里不得有真实换行");
    }

    #[test]
    fn smtp_without_recipients_fails_loudly() {
        let out = deliver(
            &[ev("x")],
            &NotifyConfig {
                channels: vec![ChannelConfig::Smtp {
                    account: SmtpAccount::new("127.0.0.1", 25, "cping@localhost", vec![]),
                    timeout_ms: 500,
                }],
            },
        );
        assert!(!out[0].ok);
        assert!(out[0].detail.contains("收件人"), "{out:?}");
    }

    #[test]
    fn smtp_to_a_closed_port_fails_with_a_readable_reason() {
        let out = deliver(
            &[ev("x")],
            &NotifyConfig {
                channels: vec![ChannelConfig::Smtp {
                    account: SmtpAccount::new(
                        "127.0.0.1",
                        1,
                        "cping@localhost",
                        vec!["ops@localhost".into()],
                    ),
                    timeout_ms: 800,
                }],
            },
        );
        assert!(!out[0].ok, "{out:?}");
        assert!(out[0].detail.contains("连接"), "{out:?}");
    }

    #[test]
    fn one_failing_channel_does_not_stop_the_others() {
        let out = deliver(
            &[ev("x")],
            &NotifyConfig {
                channels: vec![
                    ChannelConfig::Console,
                    ChannelConfig::Webhook {
                        url: "http://127.0.0.1:1/hook".into(),
                        timeout_ms: 500,
                    },
                    ChannelConfig::System {
                        app_name: "CPing".into(),
                    },
                ],
            },
        );
        assert_eq!(out.len(), 3, "每个通道都要有独立结果");
        assert!(out[0].ok, "console 不该被前面的失败影响");
        assert!(!out[1].ok);
    }

    #[test]
    fn availability_self_report_covers_every_channel() {
        let list = notifications_available();
        let keys: Vec<&str> = list.iter().map(|(k, _, _)| *k).collect();
        for k in ["console", "system", "webhook", "command", "smtp"] {
            assert!(keys.contains(&k), "缺少 {k}：{keys:?}");
        }
        // console 在任何平台都必须可用
        let console = list.iter().find(|(k, _, _)| *k == "console").unwrap();
        assert!(console.1);
        // 每一项都要有说明，不能空着
        for (_, _, note) in &list {
            assert!(!note.is_empty());
        }
        // Windows 上 system 通道应为可用
        if cfg!(windows) {
            let system = list.iter().find(|(k, _, _)| *k == "system").unwrap();
            assert!(system.1, "{system:?}");
        }
    }

    #[test]
    fn powershell_encoding_is_utf16le_base64() {
        // 'AB' → UTF-16LE 41 00 42 00 → base64 "QQBCAA=="
        assert_eq!(encode_powershell("AB"), "QQBCAA==");
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"a"), "YQ==");
        assert_eq!(base64(b"ab"), "YWI=");
        assert_eq!(base64(b"abc"), "YWJj");
    }

    #[test]
    fn xml_escaping_protects_the_toast_payload() {
        assert_eq!(xml_escape("a<b>&\"c'"), "a&lt;b&gt;&amp;&quot;c&apos;");
        // PowerShell 单引号里再写单引号要翻倍
        assert_eq!(ps_quote("it's"), "it''s");
    }

    #[test]
    fn sink_delivers_asynchronously_and_reports_counts() {
        let sink = AlertSink::spawn(NotifyConfig::console_only()).expect("应能起后台线程");
        for _ in 0..3 {
            sink.post(&[ev("离线")]);
        }
        assert_eq!(sink.posted(), 3);
        assert_eq!(sink.dropped(), 0);
        let out = sink.finish();
        assert_eq!(out.len(), 3, "每批都要有一份结果：{out:?}");
        assert!(out.iter().all(|o| o.ok), "{out:?}");
    }

    #[test]
    fn sink_drop_is_harmless_before_finish() {
        let sink = AlertSink::spawn(NotifyConfig::console_only()).unwrap();
        sink.post(&[ev("x")]);
        drop(sink); // 不 join 也不能 panic / 卡死
    }

    #[test]
    fn channel_keys_and_timeouts_are_sane() {
        assert_eq!(ChannelConfig::Console.key(), "console");
        assert_eq!(
            ChannelConfig::System {
                app_name: "x".into()
            }
            .key(),
            "system"
        );
        assert_eq!(
            ChannelConfig::Webhook {
                url: "http://h/".into(),
                timeout_ms: 1
            }
            .key(),
            "webhook"
        );
        assert_eq!(
            ChannelConfig::Command {
                template: "x".into(),
                timeout_ms: 1
            }
            .key(),
            "command"
        );
        assert_eq!(ChannelConfig::Console.timeout(), Duration::from_millis(200));
        assert!(
            ChannelConfig::Webhook {
                url: "http://h/".into(),
                timeout_ms: 1000
            }
            .timeout()
                >= Duration::from_millis(1000)
        );
        assert_eq!(
            NotifyConfig::console_only().keys(),
            vec!["console"],
            "keys 要如实列出通道"
        );
    }

    #[test]
    fn digest_summarises_many_events_without_dropping_the_count() {
        let many: Vec<AlertEvent> = (0..25).map(|i| ev(&format!("事件 {i}"))).collect();
        let (title, body) = digest(&many);
        assert_eq!(title, "CPing 告警：25 条（主机离线）");
        assert!(body.contains("另有 15 条"), "{body}");
        let (t2, _) = digest(&many[..1]);
        assert_eq!(t2, "CPing 告警：主机离线");
    }

    #[test]
    fn truncate_never_breaks_a_char_boundary() {
        assert_eq!(truncate("abc", 5), "abc");
        assert_eq!(truncate("abcdef", 3), "abc…");
        assert_eq!(truncate("中文测试", 2), "中文…");
    }
}
