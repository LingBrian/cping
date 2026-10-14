//! 配置模型与（受限子集）TOML 读写。
//!
//! 说明：核心 crate 目前零第三方依赖，因此这里实现的是 TOML 的**受限子集**：
//! 顶层键值、`[section]`、`#` 注释、字符串/整数/布尔。
//! 足够覆盖本项目的配置项；将来引入 serde 时可整体替换，接口保持不变。

use crate::classify::Thresholds;
use crate::error::{CoreError, CoreResult};
use crate::monitor::{MonitorConfig, DEFAULT_INTERVAL_MS, MAX_INTERVAL_MS, MIN_INTERVAL_MS};
use crate::spec::ProbeMode;
use crate::target::{Ipv4Prefix, TargetRange};

pub const DEFAULT_TIMEOUT_MS: u64 = 1000;
pub const DEFAULT_CONCURRENCY: usize = 32;
pub const MAX_CONCURRENCY: usize = 256;
pub const MAX_TIMEOUT_MS: u64 = 60_000;

/// 默认配色。前三项与原版设计期默认值一致；
/// 「高延迟」原版用的是**系统非激活标题栏色**，跨平台无法复现，这里固定为 `#99B4D1`。
pub const DEFAULT_COLOR_LOW: &str = "#00FF00";
pub const DEFAULT_COLOR_MID: &str = "#FFFF00";
pub const DEFAULT_COLOR_HIGH: &str = "#99B4D1";
pub const DEFAULT_COLOR_OFFLINE: &str = "#C0C0C0";
pub const DEFAULT_COLOR_ERROR: &str = "#FF6B6B";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    #[default]
    Matrix,
    Table,
}

impl ViewMode {
    pub const fn key(self) -> &'static str {
        match self {
            ViewMode::Matrix => "matrix",
            ViewMode::Table => "table",
        }
    }
}

/// 探测参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeConfig {
    pub prefix: Option<Ipv4Prefix>,
    pub range: TargetRange,
    pub timeout_ms: u64,
    pub concurrency: usize,
    pub thresholds: Thresholds,
    /// 探测方式（M9）：ICMP 或 TCP:<port>。
    pub mode: ProbeMode,
}

impl Default for ProbeConfig {
    fn default() -> Self {
        Self {
            prefix: None,
            range: TargetRange::Standard,
            timeout_ms: DEFAULT_TIMEOUT_MS,
            concurrency: DEFAULT_CONCURRENCY,
            thresholds: Thresholds::default(),
            mode: ProbeMode::default(),
        }
    }
}

impl ProbeConfig {
    pub fn validate(&self) -> CoreResult<()> {
        if self.timeout_ms == 0 {
            return Err(CoreError::InvalidConfig("超时必须大于 0".into()));
        }
        if self.timeout_ms > MAX_TIMEOUT_MS {
            return Err(CoreError::InvalidConfig(format!(
                "超时 {} ms 超过上限 {MAX_TIMEOUT_MS} ms",
                self.timeout_ms
            )));
        }
        if self.concurrency == 0 {
            return Err(CoreError::InvalidConfig("并发必须大于 0".into()));
        }
        if self.concurrency > MAX_CONCURRENCY {
            return Err(CoreError::InvalidConfig(format!(
                "并发 {} 超过上限 {MAX_CONCURRENCY}",
                self.concurrency
            )));
        }
        self.thresholds.validate()?;
        self.mode.validate()
    }
}

/// 分类配色。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorConfig {
    pub low: String,
    pub mid: String,
    pub high: String,
    pub offline: String,
    pub error: String,
}

impl Default for ColorConfig {
    fn default() -> Self {
        Self {
            low: DEFAULT_COLOR_LOW.into(),
            mid: DEFAULT_COLOR_MID.into(),
            high: DEFAULT_COLOR_HIGH.into(),
            offline: DEFAULT_COLOR_OFFLINE.into(),
            error: DEFAULT_COLOR_ERROR.into(),
        }
    }
}

impl ColorConfig {
    pub fn get(&self, key: &str) -> Option<&str> {
        match key {
            "low" => Some(&self.low),
            "mid" => Some(&self.mid),
            "high" => Some(&self.high),
            "offline" => Some(&self.offline),
            "error" => Some(&self.error),
            _ => None,
        }
    }

    pub fn set(&mut self, key: &str, value: &str) -> CoreResult<()> {
        validate_hex_color(value)?;
        match key {
            "low" => self.low = value.to_string(),
            "mid" => self.mid = value.to_string(),
            "high" => self.high = value.to_string(),
            "offline" => self.offline = value.to_string(),
            "error" => self.error = value.to_string(),
            _ => return Err(CoreError::InvalidConfig(format!("未知颜色键 {key}"))),
        }
        Ok(())
    }

    pub fn validate(&self) -> CoreResult<()> {
        for key in ["low", "mid", "high", "offline", "error"] {
            let v = self.get(key).expect("key checked above");
            validate_hex_color(v)?;
        }
        Ok(())
    }
}

/// 校验 `#RRGGBB`。
pub fn validate_hex_color(value: &str) -> CoreResult<()> {
    let bytes = value.as_bytes();
    if bytes.len() != 7 || bytes[0] != b'#' {
        return Err(CoreError::InvalidConfig(format!(
            "颜色 {value:?} 必须是 #RRGGBB 格式"
        )));
    }
    if !bytes[1..].iter().all(|b| b.is_ascii_hexdigit()) {
        return Err(CoreError::InvalidConfig(format!(
            "颜色 {value:?} 含非法十六进制字符"
        )));
    }
    Ok(())
}

/// 应用配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppConfig {
    pub probe: ProbeConfig,
    pub colors: ColorConfig,
    pub hide_offline: bool,
    pub view: ViewMode,
    /// 持续监控（M8）。
    pub monitor: MonitorConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            probe: ProbeConfig::default(),
            colors: ColorConfig::default(),
            hide_offline: false,
            view: ViewMode::Matrix,
            monitor: MonitorConfig::default(),
        }
    }
}

impl AppConfig {
    pub fn validate(&self) -> CoreResult<()> {
        self.probe.validate()?;
        self.colors.validate()?;
        self.monitor.validate().map_err(CoreError::InvalidConfig)
    }

    /// 序列化为 TOML 文本。
    pub fn to_toml(&self) -> String {
        let mut out = String::new();
        out.push_str("# CPing 配置\n");
        out.push_str("[probe]\n");
        out.push_str(&format!(
            "prefix = {}\n",
            match &self.probe.prefix {
                Some(p) => format!("\"{p}\""),
                None => "\"\"".to_string(),
            }
        ));
        out.push_str(&format!("range = \"{}\"\n", self.probe.range.key()));
        out.push_str(&format!("timeout_ms = {}\n", self.probe.timeout_ms));
        out.push_str(&format!("concurrency = {}\n", self.probe.concurrency));
        out.push_str(&format!("mode = \"{}\"\n", self.probe.mode.key()));
        out.push_str(&format!(
            "tcp_port = {}\n",
            self.probe.mode.tcp_port().unwrap_or(80)
        ));
        out.push_str(&format!(
            "low_max_ms = {}\n",
            self.probe.thresholds.low_max_ms
        ));
        out.push_str(&format!(
            "mid_max_ms = {}\n",
            self.probe.thresholds.mid_max_ms
        ));
        out.push_str("\n[colors]\n");
        out.push_str(&format!("low = \"{}\"\n", self.colors.low));
        out.push_str(&format!("mid = \"{}\"\n", self.colors.mid));
        out.push_str(&format!("high = \"{}\"\n", self.colors.high));
        out.push_str(&format!("offline = \"{}\"\n", self.colors.offline));
        out.push_str(&format!("error = \"{}\"\n", self.colors.error));
        out.push_str("\n[ui]\n");
        out.push_str(&format!("hide_offline = {}\n", self.hide_offline));
        out.push_str(&format!("view = \"{}\"\n", self.view.key()));
        out.push_str("\n[monitor]\n");
        out.push_str(&format!("enabled = {}\n", self.monitor.enabled));
        out.push_str(&format!("interval_ms = {}\n", self.monitor.interval_ms));
        out.push_str(&format!(
            "notify_appeared = {}\n",
            self.monitor.notify_appeared
        ));
        out.push_str(&format!(
            "notify_disappeared = {}\n",
            self.monitor.notify_disappeared
        ));
        out.push_str(&format!("capacity = {}\n", self.monitor.capacity));
        out
    }

    /// 从 TOML 文本解析。未知键会被忽略，缺失键使用默认值。
    pub fn from_toml(text: &str) -> CoreResult<Self> {
        let mut cfg = AppConfig::default();
        let mut section = String::new();
        let mut prefix_text: Option<String> = None;
        // 端口先单独收着：`mode` 与 `tcp_port` 谁先出现不影响结果
        let mut mode_key = cfg.probe.mode.key().to_string();
        let mut tcp_port: u16 = cfg.probe.mode.tcp_port().unwrap_or(80);

        for (lineno, raw) in text.lines().enumerate() {
            let line = strip_comment(raw).trim();
            if line.is_empty() {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                section = line[1..line.len() - 1].trim().to_string();
                continue;
            }
            let (key, value) = line.split_once('=').ok_or_else(|| {
                CoreError::InvalidConfig(format!("第 {} 行不是 key = value：{line:?}", lineno + 1))
            })?;
            let key = key.trim();
            let value = value.trim();
            match (section.as_str(), key) {
                ("probe", "prefix") => prefix_text = Some(parse_string(value)?),
                ("probe", "range") => {
                    cfg.probe.range = match parse_string(value)?.as_str() {
                        "full" => TargetRange::Full,
                        "hosts_only" => TargetRange::HostsOnly,
                        _ => TargetRange::Standard,
                    }
                }
                ("probe", "timeout_ms") => cfg.probe.timeout_ms = parse_u64(value)?,
                ("probe", "concurrency") => cfg.probe.concurrency = parse_u64(value)? as usize,
                ("probe", "mode") => mode_key = parse_string(value)?,
                ("probe", "tcp_port") => {
                    let raw_port = parse_u64(value)?;
                    if raw_port > u16::MAX as u64 {
                        return Err(CoreError::InvalidConfig(format!(
                            "第 {} 行 tcp_port {raw_port} 超出 0-65535",
                            lineno + 1
                        )));
                    }
                    tcp_port = raw_port as u16;
                }
                ("probe", "low_max_ms") => {
                    cfg.probe.thresholds.low_max_ms = parse_u64(value)? as u32
                }
                ("probe", "mid_max_ms") => {
                    cfg.probe.thresholds.mid_max_ms = parse_u64(value)? as u32
                }
                ("colors", "low") => cfg.colors.low = parse_string(value)?,
                ("colors", "mid") => cfg.colors.mid = parse_string(value)?,
                ("colors", "high") => cfg.colors.high = parse_string(value)?,
                ("colors", "offline") => cfg.colors.offline = parse_string(value)?,
                ("colors", "error") => cfg.colors.error = parse_string(value)?,
                ("ui", "hide_offline") => cfg.hide_offline = parse_bool(value)?,
                ("ui", "view") => {
                    cfg.view = match parse_string(value)?.as_str() {
                        "table" => ViewMode::Table,
                        _ => ViewMode::Matrix,
                    }
                }
                ("monitor", "enabled") => cfg.monitor.enabled = parse_bool(value)?,
                ("monitor", "interval_ms") => cfg.monitor.interval_ms = parse_u64(value)?,
                ("monitor", "notify_appeared") => cfg.monitor.notify_appeared = parse_bool(value)?,
                ("monitor", "notify_disappeared") => {
                    cfg.monitor.notify_disappeared = parse_bool(value)?
                }
                ("monitor", "capacity") => cfg.monitor.capacity = parse_u64(value)? as usize,
                _ => {} // 未知键：忽略，保证向前兼容
            }
        }

        cfg.probe.prefix = match prefix_text {
            Some(s) if !s.trim().is_empty() => Some(crate::target::parse_prefix(&s)?),
            _ => None,
        };
        cfg.probe.mode = match mode_key.as_str() {
            "tcp" => ProbeMode::Tcp { port: tcp_port },
            _ => ProbeMode::Icmp,
        };

        Ok(cfg)
    }

    /// 配置里当前的监控间隔是否合法（界面在改值时用）。
    pub fn monitor_interval_bounds() -> (u64, u64) {
        (MIN_INTERVAL_MS, MAX_INTERVAL_MS)
    }
}

/// 监控间隔默认值（供界面初始化）。
pub const fn default_monitor_interval_ms() -> u64 {
    DEFAULT_INTERVAL_MS
}

fn strip_comment(line: &str) -> &str {
    let mut in_string = false;
    for (i, ch) in line.char_indices() {
        match ch {
            '"' => in_string = !in_string,
            '#' if !in_string => return &line[..i],
            _ => {}
        }
    }
    line
}

fn parse_string(value: &str) -> CoreResult<String> {
    let v = value.trim();
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
        Ok(v[1..v.len() - 1].to_string())
    } else {
        Err(CoreError::InvalidConfig(format!(
            "期望字符串，得到 {value:?}"
        )))
    }
}

fn parse_u64(value: &str) -> CoreResult<u64> {
    value
        .trim()
        .parse::<u64>()
        .map_err(|_| CoreError::InvalidConfig(format!("期望整数，得到 {value:?}")))
}

fn parse_bool(value: &str) -> CoreResult<bool> {
    match value.trim() {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(CoreError::InvalidConfig(format!(
            "期望布尔，得到 {other:?}"
        ))),
    }
}
