//! Tauri Commands：前端与 `cping-core` 之间的唯一桥接。
//!
//! 约束：
//! - 这里**不实现**任何目标生成 / 分类 / 统计 / 导出逻辑，只是转发与 DTO 转换；
//! - 核心类型不带 serde 派生（保持核心零依赖），因此在边界上手写 DTO 映射。

use std::path::{Path as FsPath, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use cping_core::classify::{classify_status, Thresholds};
use cping_core::config::{AppConfig, ColorConfig, ProbeConfig, ViewMode};
use cping_core::diff::{diff_reports, ScanDiff};
use cping_core::enrich::{enrich_with_mac_and_vendor, lookup_hostname, EnrichReport};
use cping_core::export::{ExportFormat, ExportMeta, ScanReport};
use cping_core::monitor::{Change, MonitorConfig, MonitorState};
use cping_core::registry::BackendRegistry;
use cping_core::result::{ProbeResult, ScanStats};
use cping_core::scheduler::{CancelToken, ProbeScheduler, SchedulerHandle};
use cping_core::snapshot::{load_from_path, save_to_path, Snapshot};
use cping_core::spec::{parse_spec, ProbeMode, Segment, TargetSpec, MATRIX_VIEW_MAX};
use cping_core::target::{matrix_slots, parse_prefix, TargetRange};
use cping_core::{Lang, ProbeEvent};

use crate::i18n::{self, Key};

// ---------------------------------------------------------------------------
// 语言
// ---------------------------------------------------------------------------

/// 解析命令收到的 `lang` 参数。
///
/// - 前端**没发**这个字段（旧版前端）→ 退化为 [`Lang::from_env`]，而不是报错。
///   向后兼容在这里是硬要求：旧界面配新后端必须照常能扫，语言只是显示层的差异。
/// - 发了一个**认不出的值**（例如 `de`、`C`）→ 同样走环境推断。
///   [`Lang::parse`] 对 `C` / `POSIX` / 空串返回 `None`，那是「没有本地化偏好」，
///   不是「请用英文」——猜错语言比回退到默认更糟。
/// - 环境变量也给不出线索 → [`Lang::default`]（中文）。
fn parse_lang(lang: Option<String>) -> Lang {
    lang.as_deref()
        .and_then(Lang::parse)
        .unwrap_or_else(Lang::from_env)
}

/// 导出格式的展示名。
///
/// `CSV` / `TXT` / `JSON` 是**技术记号**，两种语言下一样，直接沿用核心的
/// [`cping_core::export::ExportFormat::label_zh`] 取值（它返回的就是这几个记号）；
/// 只有 `HTML 报告` 是散文，需要查 GUI 字典。
///
/// 为什么不干脆统一走核心：核心的 `label_zh()` 是**导出产物（数据契约）**用的接口，
/// 它必须恒为中文。界面提示要跟着语言走，所以这里只在「确实是记号」时复用它。
fn export_format_label(fmt: cping_core::export::ExportFormat, lang: Lang) -> String {
    match fmt {
        cping_core::export::ExportFormat::Html => {
            i18n::text(lang, Key::ExportFormatHtml).to_string()
        }
        other => other.label_zh().to_string(),
    }
}

/// 差异摘要——直接交给核心层。
///
/// 这里曾经是 GUI 侧自己用 `count_of` 重拼的一句（当时核心的
/// `ScanDiff::summary()` 还没有 `_in` 变体）。现在核心补上了
/// `summary_in(lang)`，**中文只剩一处来源**：界面侧不再持有任何一份
/// 与核心重复的措辞。两份措辞迟早漂移，而漂移了没人会发现。
fn diff_summary(diff: &ScanDiff, lang: Lang) -> String {
    diff.summary_in(lang)
}

// ---------------------------------------------------------------------------
// DTO
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeResultDto {
    pub index: u16,
    pub ip: String,
    pub mac: Option<String>,
    /// M7：由 MAC 前缀查到的厂商名。
    pub vendor: Option<String>,
    /// M7：按需反查得到的主机名。
    pub hostname: Option<String>,
    pub status_key: String,
    pub status_label: String,
    pub rtt_ms: Option<u32>,
    pub note: Option<String>,
    pub class_key: Option<String>,
}

fn to_result_dto(
    r: &ProbeResult,
    thresholds: &Thresholds,
    mode: ProbeMode,
    lang: Lang,
) -> ProbeResultDto {
    ProbeResultDto {
        index: r.index,
        ip: r.ip.to_string(),
        mac: r.mac.clone(),
        vendor: r.vendor.clone(),
        hostname: r.hostname.clone(),
        status_key: r.status.key().to_string(),
        // M9：TCP 模式下「在线」必须显示成 TCP-OPEN，否则会被读成 ICMP 可达
        status_label: mode.status_label_in(&r.status, lang),
        rtt_ms: r.status.rtt_ms(),
        // `note()` 里装的是**后端/操作系统的原始细节**（例如 `No such file`），
        // 按约定原样透传：翻了反而更难查，而且它本来也不是我们写的句子。
        note: r.status.note().map(|s| s.to_string()),
        class_key: classify_status(&r.status, thresholds).map(|c| c.key().to_string()),
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanStatsDto {
    pub total: usize,
    pub completed: usize,
    pub pending: usize,
    pub online: usize,
    pub timeout: usize,
    pub unreachable: usize,
    pub permission_denied: usize,
    pub unsupported: usize,
    pub error: usize,
}

impl From<&ScanStats> for ScanStatsDto {
    fn from(s: &ScanStats) -> Self {
        Self {
            total: s.total,
            completed: s.completed,
            pending: s.pending,
            online: s.online,
            timeout: s.timeout,
            unreachable: s.unreachable,
            permission_denied: s.permission_denied,
            unsupported: s.unsupported,
            error: s.error,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotDto {
    pub task_id: Option<String>,
    pub running: bool,
    pub cancelled: bool,
    pub prefix: String,
    pub total: usize,
    pub elapsed_ms: u64,
    pub stats: ScanStatsDto,
    pub results: Vec<ProbeResultDto>,
    pub cell_slots: Vec<Option<usize>>,
    pub targets: Vec<String>,

    // ---- M8 / M9 新增 ----
    /// 探测方式键（`icmp` / `tcp`）。
    pub mode: String,
    /// 探测方式展示文本（`ICMP` / `TCP:445`）。
    pub mode_label: String,
    /// TCP 端口（非 TCP 模式为 null）。
    pub tcp_port: Option<u16>,
    /// 目标范围描述，例如 `192.168.1（255 个）`。
    pub spec_summary: String,
    /// 各输入网段在 targets 中的区间（界面分段显示用）。
    pub segments: Vec<SegmentDto>,
    /// 是否能用 16×16 矩阵表示（>256 个目标时必须切表格）。
    pub matrix_compatible: bool,
    /// 已完成轮次（持续监控）。
    pub round: u64,
    /// 本轮变化（含上一轮基线比较的结果）。
    pub changes: Vec<ChangeDto>,
    /// 变化日志累计条数。
    pub change_total: u64,
    /// 与上一轮相比的差异摘要（没有人话可写时为空串）。
    pub diff_summary: String,
}

/// 一个输入网段在目标列表中的区间。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentDto {
    pub label: String,
    pub start: usize,
    pub count: usize,
}

impl From<&Segment> for SegmentDto {
    fn from(s: &Segment) -> Self {
        Self {
            label: s.label.clone(),
            start: s.start,
            count: s.count,
        }
    }
}

/// 一条变化记录（M8）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeDto {
    pub round: u64,
    pub index: u16,
    pub ip: String,
    pub kind: String,
    pub kind_label: String,
    pub from_label: String,
    pub to_label: String,
    /// 一行摘要，前端直接显示即可。
    pub summary: String,
}

/// 变化记录 → DTO。
///
/// 由 `impl From<&Change>` 改成带 `lang` 的自由函数：语言是**每次调用**的入参，
/// 不是 `Change` 自身的属性，塞进 `From` 只能靠全局变量，那才是真坑。
fn change_dto(c: &Change, lang: Lang) -> ChangeDto {
    ChangeDto {
        round: c.round,
        index: c.index,
        ip: c.ip.to_string(),
        kind: c.kind.key().to_string(),
        kind_label: c.kind.label_in(lang).to_string(),
        from_label: c.from.label_in(lang).to_string(),
        to_label: c.to.label_in(lang).to_string(),
        summary: c.summary_in(lang),
    }
}

/// 目标规格解析结果（M9）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpecDto {
    pub summary: String,
    pub total: usize,
    pub segments: Vec<SegmentDto>,
    pub matrix_compatible: bool,
    /// 首尾目标，例如 `192.168.1.1 – 192.168.1.255`。
    pub span: String,
    pub first: Option<String>,
    pub last: Option<String>,
}

// ---------------------------------------------------------------------------
// M7 DTO：目标信息补全 与 结果快照
// ---------------------------------------------------------------------------

/// 一次「MAC/厂商补全」的结果。字段名与核心 [`EnrichReport`] 对齐，
/// 额外给出一句可直接显示的中文说明，省得前端再拼文案。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnrichReportDto {
    /// 邻居表是否读到（false 时下面的计数必然为 0）。
    pub table_ok: bool,
    /// 读取失败原因。
    pub error: Option<String>,
    /// 邻居表条目总数。
    pub entries: usize,
    /// 本次新填上的 MAC 数。
    pub mac_filled: usize,
    /// 本次新填上的厂商数。
    pub vendor_filled: usize,
    /// 全部结果中**累计**带有 MAC / 厂商 / 主机名的条数（用于状态栏）。
    pub with_mac: usize,
    pub with_vendor: usize,
    pub with_hostname: usize,
    /// 面向用户的一句话说明。
    pub message: String,
    /// 本次补全后自动落盘的快照路径（失败或无需落盘时为 None）。
    pub autosaved_to: Option<String>,
}

impl EnrichReportDto {
    fn from_report(
        report: EnrichReport,
        results: &[Option<ProbeResult>],
        autosaved_to: Option<String>,
        lang: Lang,
    ) -> Self {
        let with_mac = results.iter().flatten().filter(|r| r.mac.is_some()).count();
        let with_vendor = results
            .iter()
            .flatten()
            .filter(|r| r.vendor.is_some())
            .count();
        let with_hostname = results
            .iter()
            .flatten()
            .filter(|r| r.hostname.is_some())
            .count();

        let message = if !report.table_ok {
            // `report.error` 来自操作系统（读 /proc、注册表或命令输出），原样透传，
            // 只把外面那层「读取邻居表失败」翻掉。
            let reason = report
                .error
                .as_deref()
                .unwrap_or(i18n::text(lang, Key::UnknownReason));
            i18n::render(lang, Key::EnrichReadFailed, &[reason])
        } else if report.mac_filled == 0 && report.vendor_filled == 0 {
            // 诚实说明：ARP 表只包含近期真正通信过的地址，空表是正常现象
            i18n::render(lang, Key::EnrichNoFill, &[&report.entries.to_string()])
        } else {
            i18n::render(
                lang,
                Key::EnrichFilled,
                &[
                    &report.entries.to_string(),
                    &report.mac_filled.to_string(),
                    &report.vendor_filled.to_string(),
                ],
            )
        };

        Self {
            table_ok: report.table_ok,
            error: report.error,
            entries: report.entries,
            mac_filled: report.mac_filled,
            vendor_filled: report.vendor_filled,
            with_mac,
            with_vendor,
            with_hostname,
            message,
            autosaved_to,
        }
    }
}

/// 单条主机名反查结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostnameLookupDto {
    pub index: u16,
    pub ip: String,
    /// 查到的名字；查不到就是 None，**不填占位文本**。
    pub hostname: Option<String>,
    /// 是否拿到名字（已有名字时也算 true）。
    pub found: bool,
}

/// 启动时发现的「上次未处理的结果」。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingSnapshotDto {
    pub path: String,
    /// 能否解析。false 时只有 `error` 有意义，界面上应提示「无法读取」而不是「恢复」。
    pub ok: bool,
    pub error: Option<String>,
    pub scan_time: String,
    pub prefix: String,
    pub total: usize,
    pub rows: usize,
    pub elapsed_ms: u64,
    pub cancelled: bool,
}

impl PendingSnapshotDto {
    fn unreadable(path: &FsPath, error: String) -> Self {
        Self {
            path: path.to_string_lossy().to_string(),
            ok: false,
            error: Some(error),
            scan_time: String::new(),
            prefix: String::new(),
            total: 0,
            rows: 0,
            elapsed_ms: 0,
            cancelled: false,
        }
    }
}

/// 显式保存快照的结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotSavedDto {
    pub path: String,
    pub rows: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ProbeEventDto {
    Started {
        task_id: String,
        total: usize,
    },
    Result {
        task_id: String,
        result: ProbeResultDto,
    },
    Progress {
        task_id: String,
        completed: usize,
        total: usize,
    },
    Finished {
        task_id: String,
        stats: ScanStatsDto,
        elapsed_ms: u64,
    },
    Cancelled {
        task_id: String,
        stats: ScanStatsDto,
        elapsed_ms: u64,
    },
}

fn to_event_dto(
    ev: &ProbeEvent,
    thresholds: &Thresholds,
    mode: ProbeMode,
    lang: Lang,
) -> ProbeEventDto {
    match ev {
        ProbeEvent::Started { task_id, total } => ProbeEventDto::Started {
            task_id: task_id.clone(),
            total: *total,
        },
        ProbeEvent::Result { task_id, result } => ProbeEventDto::Result {
            task_id: task_id.clone(),
            result: to_result_dto(result, thresholds, mode, lang),
        },
        ProbeEvent::Progress {
            task_id,
            completed,
            total,
        } => ProbeEventDto::Progress {
            task_id: task_id.clone(),
            completed: *completed,
            total: *total,
        },
        ProbeEvent::Finished {
            task_id,
            stats,
            elapsed_ms,
        } => ProbeEventDto::Finished {
            task_id: task_id.clone(),
            stats: stats.into(),
            elapsed_ms: *elapsed_ms,
        },
        ProbeEvent::Cancelled {
            task_id,
            stats,
            elapsed_ms,
        } => ProbeEventDto::Cancelled {
            task_id: task_id.clone(),
            stats: stats.into(),
            elapsed_ms: *elapsed_ms,
        },
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ColorConfigDto {
    pub low: String,
    pub mid: String,
    pub high: String,
    pub offline: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProbeConfigDto {
    pub prefix: String,
    pub range: String,
    pub timeout_ms: u64,
    pub concurrency: usize,
    pub low_max_ms: u32,
    pub mid_max_ms: u32,
    /// M9：`icmp` / `tcp`。旧配置里没有这一项 → 默认 ICMP。
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub tcp_port: Option<u16>,
}

/// 持续监控设置（M8）。独立成块，旧的配置文件缺这一段也能读。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorConfigDto {
    pub enabled: bool,
    pub interval_ms: u64,
}

impl Default for MonitorConfigDto {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_ms: cping_core::MONITOR_DEFAULT_INTERVAL_MS,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppConfigDto {
    pub probe: ProbeConfigDto,
    pub colors: ColorConfigDto,
    pub hide_offline: bool,
    pub view: String,
    /// M8：持续监控设置。
    #[serde(default)]
    pub monitor: MonitorConfigDto,
}

/// 前端给的 DTO → 核心配置（唯一一处转换，含范围字符串解析）。
fn config_from_dto(dto: AppConfigDto) -> AppConfig {
    // 前缀既可能是三段网段，也可能是 M9 的单主机/CIDR；存不下就留空，
    // 不因为配置里的一个旧字段把整份配置作废。
    let prefix = if dto.probe.prefix.trim().is_empty() {
        None
    } else {
        parse_prefix(&dto.probe.prefix).ok()
    };
    let range = match dto.probe.range.as_str() {
        "full" => TargetRange::Full,
        "hosts_only" => TargetRange::HostsOnly,
        _ => TargetRange::Standard,
    };
    let mode = if dto.probe.mode == "tcp" {
        ProbeMode::Tcp {
            port: dto.probe.tcp_port.filter(|p| *p > 0).unwrap_or(80),
        }
    } else {
        ProbeMode::Icmp
    };
    AppConfig {
        probe: ProbeConfig {
            prefix,
            range,
            timeout_ms: if dto.probe.timeout_ms == 0 {
                cping_core::DEFAULT_TIMEOUT_MS
            } else {
                dto.probe.timeout_ms
            },
            concurrency: if dto.probe.concurrency == 0 {
                cping_core::DEFAULT_CONCURRENCY
            } else {
                dto.probe.concurrency
            },
            thresholds: Thresholds::new(
                if dto.probe.low_max_ms == 0 {
                    50
                } else {
                    dto.probe.low_max_ms
                },
                if dto.probe.mid_max_ms == 0 {
                    100
                } else {
                    dto.probe.mid_max_ms
                },
            ),
            mode,
        },
        colors: cping_core::ColorConfig {
            low: dto.colors.low,
            mid: dto.colors.mid,
            high: dto.colors.high,
            offline: dto.colors.offline,
            error: dto.colors.error,
        },
        hide_offline: dto.hide_offline,
        view: if dto.view == "table" {
            ViewMode::Table
        } else {
            ViewMode::Matrix
        },
        // 间隔越界的旧值一律回落到默认值，而不是让保存失败
        monitor: MonitorConfig {
            enabled: dto.monitor.enabled,
            interval_ms: if dto.monitor.interval_ms < cping_core::MONITOR_MIN_INTERVAL_MS
                || dto.monitor.interval_ms > cping_core::MONITOR_MAX_INTERVAL_MS
            {
                cping_core::MONITOR_DEFAULT_INTERVAL_MS
            } else {
                dto.monitor.interval_ms
            },
            ..MonitorConfig::default()
        },
    }
}

fn config_to_dto(cfg: &AppConfig) -> AppConfigDto {
    AppConfigDto {
        probe: ProbeConfigDto {
            prefix: cfg.probe.prefix.map(|p| p.to_string()).unwrap_or_default(),
            range: cfg.probe.range.key().to_string(),
            timeout_ms: cfg.probe.timeout_ms,
            concurrency: cfg.probe.concurrency,
            low_max_ms: cfg.probe.thresholds.low_max_ms,
            mid_max_ms: cfg.probe.thresholds.mid_max_ms,
            mode: cfg.probe.mode.key().to_string(),
            tcp_port: cfg.probe.mode.tcp_port(),
        },
        colors: ColorConfigDto {
            low: cfg.colors.low.clone(),
            mid: cfg.colors.mid.clone(),
            high: cfg.colors.high.clone(),
            offline: cfg.colors.offline.clone(),
            error: cfg.colors.error.clone(),
        },
        hide_offline: cfg.hide_offline,
        view: cfg.view.key().to_string(),
        monitor: MonitorConfigDto {
            enabled: cfg.monitor.enabled,
            interval_ms: cfg.monitor.interval_ms,
        },
    }
}

// ---------------------------------------------------------------------------
// 共享状态
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct Shared {
    running: bool,
    cancelled: bool,
    task_id: Option<String>,
    prefix: String,
    /// 本轮扫描的开始时间（用于快照头部与导出元信息）。
    scan_time: String,
    targets: Vec<std::net::IpAddr>,
    cell_slots: Vec<Option<usize>>,
    results: Vec<Option<ProbeResult>>,
    stats: ScanStats,
    elapsed_ms: u64,
    thresholds: Thresholds,
    cancel: Option<CancelToken>,
    /// 当前探测方式（本轮任务的方式；切换方式在 start_scan 时生效）。
    mode: ProbeMode,
    /// 当前目标规格（含分段信息）。
    spec: TargetSpec,
    /// 持续监控状态机：轮次 + 变化检测 + 变化日志。
    monitor: MonitorState,
    /// 最近一次「上一轮 → 本轮」的差异（导出 diff 用）。
    last_diff: Option<ScanDiff>,
    /// M13：最近一次的端口指纹结果。
    ///
    /// 存在这里是刻意的：导出时必须带上它，而**绝不能**塞进 `results`——
    /// 端口扫描是独立的一趟探测，混进结果模型会让「在线」出现第二个来源。
    fingerprints: Vec<cping_core::HostFingerprint>,
    /// M13：本轮指纹用的端口数与是否抓横幅（供界面如实说明）。
    fingerprint_ports: usize,
    fingerprint_banner: bool,
    /// M14：长期时序（进程内累积，供趋势页与可用率报表使用）。
    series: cping_core::TimeSeries,
    /// M14：本轮扫描的起始时刻（Unix 毫秒）——时序点用它打时间戳，
    /// 而不是「扫描结束的现在」，免得一轮跑几秒就把时间轴拉长。
    round_at_ms: u64,
    /// M15：告警引擎（规则求值 + 冷却去重 + 静音 + 事件缓冲）。
    alerts: cping_core::AlertEngine,
    /// M15：告警是否启用。默认**关闭**——不问就发通知比不装告警还糟。
    alerts_enabled: bool,
}

pub struct AppState {
    shared: Arc<Mutex<Shared>>,
}

/// 选择探测后端（M9：按探测方式分派）。
///
/// `CPING_MOCK=1` 时改用模拟数据（演示用）——**必须显式开启**，
/// 避免把模拟结果误当成真实探测。
///
/// M12：`mock` 分支已收敛到核心的 [`cping_core::backend_for_run`]，
/// 这里的职责只剩「读环境变量」。
fn select_backend(mode: ProbeMode) -> Box<dyn cping_core::probe::ProbeBackend> {
    let mock = std::env::var("CPING_MOCK")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    cping_core::backend_for_run(mode, mock)
}

impl AppState {
    pub fn new() -> Self {
        Self {
            shared: Arc::new(Mutex::new(Shared {
                running: false,
                cancelled: false,
                task_id: None,
                prefix: String::new(),
                scan_time: String::new(),
                targets: Vec::new(),
                cell_slots: vec![None; cping_core::MATRIX_CELLS],
                results: Vec::new(),
                stats: ScanStats::default(),
                elapsed_ms: 0,
                thresholds: Thresholds::default(),
                cancel: None,
                mode: cping_core::platform_default_mode(),
                spec: TargetSpec::default(),
                monitor: MonitorState::new(MonitorConfig::default()).expect("默认监控配置必然合法"),
                last_diff: None,
                fingerprints: Vec::new(),
                fingerprint_ports: 0,
                fingerprint_banner: true,
                series: cping_core::TimeSeries::new(),
                round_at_ms: 0,
                alerts: cping_core::AlertEngine::new(cping_core::AlertConfig::default()),
                alerts_enabled: false,
            })),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn parse_prefix_cmd(input: String, lang: Option<String>) -> Result<String, String> {
    let lang = parse_lang(lang);
    parse_prefix(&input)
        .map(|p| p.to_string())
        .map_err(|e| e.localized(lang))
}

/// M9：解析完整目标规格（网段 / 单主机 / CIDR / 多网段）。
#[tauri::command]
pub fn parse_spec_cmd(input: String, lang: Option<String>) -> Result<SpecDto, String> {
    let lang = parse_lang(lang);
    let spec = parse_spec(&input, TargetRange::Standard).map_err(|e| e.localized(lang))?;
    Ok(spec_dto(&spec, lang))
}

fn spec_dto(spec: &TargetSpec, lang: Lang) -> SpecDto {
    SpecDto {
        summary: spec.summary_in(lang),
        total: spec.len(),
        // `SegmentDto.label` 是网段本身的标签（`192.168.1`），不是句子，
        // 两种语言下完全相同，因此不需要语言。
        segments: spec.segments.iter().map(SegmentDto::from).collect(),
        matrix_compatible: spec.matrix_compatible(),
        // `span_text` / 首尾地址都是地址本身，没有散文，无需语言。
        span: spec.span_text().unwrap_or_default(),
        first: spec.targets.first().map(|ip| ip.to_string()),
        last: spec.targets.last().map(|ip| ip.to_string()),
    }
}

/// 平台默认探测方式（Android 上是 TCP:80，桌面是 ICMP）。
#[tauri::command]
pub fn default_probe_mode() -> Result<(String, u16), String> {
    let mode = cping_core::platform_default_mode();
    Ok((mode.key().to_string(), mode.tcp_port().unwrap_or(80)))
}

/// 矩阵视图能表示多少目标（前端据此决定是否强制切表格）。
#[tauri::command]
pub fn matrix_view_max() -> Result<usize, String> {
    Ok(MATRIX_VIEW_MAX)
}

/// 一个探测后端的对外能力描述（M12）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendCapsDto {
    pub id: String,
    pub label: String,
    pub available: bool,
    pub ipv4: bool,
    pub ipv6: bool,
    pub needs_privilege: bool,
    pub note: String,
    /// 一行完整描述，前端可直接展示。
    pub describe: String,
}

/// 当前平台全部内置后端的能力（含不可用的，前端自行过滤）。
///
/// 「不可用」是**事实**而不是错误：ICMPv6 本版未实装，就如实登记为不可用，
/// 界面据此提示用户改用 TCP，而不是等扫完才发现清一色「不支持」。
#[tauri::command]
pub fn backend_caps(lang: Option<String>) -> Result<Vec<BackendCapsDto>, String> {
    let lang = parse_lang(lang);
    let reg = BackendRegistry::new();
    Ok(reg
        .caps()
        .into_iter()
        .map(|c| BackendCapsDto {
            // `key()` 是机器可读标识，不是文案，不跟语言变。
            id: c.id.key().to_string(),
            label: c.id.label_in(lang).to_string(),
            available: c.available,
            ipv4: c.ipv4,
            ipv6: c.ipv6,
            needs_privilege: c.needs_privilege,
            // `note` 已经是枚举 `CapsNote`：漏翻是编译期问题，不再是运行期的中文残留。
            note: c.note.label_in(lang).to_string(),
            describe: c.describe_in(lang),
        })
        .collect())
}

/// 一个开放端口（M13）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenPortDto {
    pub port: u16,
    /// 已知服务名；未收录端口为 `None`（前端不得自行编一个）。
    pub service: Option<String>,
    /// 服务主动送出的横幅（只读一次，原样保存）。
    pub banner: Option<String>,
    pub connect_rtt_ms: u32,
}

/// 一台主机的端口指纹（M13）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FingerprintDto {
    pub ip: String,
    pub scanned: usize,
    pub elapsed_ms: u64,
    pub open_ports: Vec<OpenPortDto>,
    /// 「22(ssh) 80(http)」；无开放端口时为空串。
    pub open_ports_summary: String,
    /// 设备类型**推断**结论；识别不出时为 `None`（界面留空）。
    pub device_kind: Option<String>,
    /// 推断置信度（低/中）。
    pub device_confidence: Option<String>,
    /// 恒为 true：让前端无法把这条结论误当成事实。
    pub device_is_inference: bool,
    pub device_summary: String,
    /// 推断依据（逐条）。
    pub device_basis: Vec<String>,
}

/// M13：对在线主机做端口指纹。
///
/// - `indices` 为空/缺省 → 对全部在线主机做（上限 64 台，避免一次点击打出一整网段）；
/// - `ports` 为空/缺省 → 用核心内置的常用端口集；
/// - 结果**同时**写进 `Shared::fingerprints`，导出时自动带上。
#[tauri::command]
pub fn fingerprint_hosts_cmd(
    state: State<'_, AppState>,
    indices: Option<Vec<u16>>,
    ports: Option<Vec<u16>>,
    grab_banner: Option<bool>,
    timeout_ms: Option<u64>,
    lang: Option<String>,
) -> Result<Vec<FingerprintDto>, String> {
    let lang = parse_lang(lang);
    let cfg = {
        let mut cfg = cping_core::PortScanConfig {
            timeout: Duration::from_millis(timeout_ms.unwrap_or(800).clamp(100, 10_000)),
            grab_banner: grab_banner.unwrap_or(true),
            ..Default::default()
        };
        if let Some(list) = ports.filter(|v| !v.is_empty()) {
            cfg.ports = list;
        }
        cfg.normalized()
    };

    // 锁内只取副本：端口探测会在锁外跑几秒，绝不能持锁
    let hosts: Vec<(std::net::IpAddr, Option<String>)> = {
        let sh = state.shared.lock().expect("state poisoned");
        match indices.filter(|v| !v.is_empty()) {
            Some(list) => list
                .iter()
                .filter_map(|i| sh.results.get(*i as usize).and_then(|s| s.as_ref()))
                .filter(|r| r.status.is_online())
                .map(|r| (r.ip, r.vendor.clone()))
                .collect(),
            None => sh
                .results
                .iter()
                .flatten()
                .filter(|r| r.status.is_online())
                .take(64)
                .map(|r| (r.ip, r.vendor.clone()))
                .collect(),
        }
    };
    if hosts.is_empty() {
        return Err(i18n::render(lang, Key::FingerprintNoOnlineHosts, &[]));
    }

    let scanned = cfg.ports.len();
    let banner = cfg.grab_banner;
    let fps = cping_core::fingerprint_hosts(&hosts, &cfg);

    let dtos: Vec<FingerprintDto> = fps
        .iter()
        .map(|fp| FingerprintDto {
            ip: fp.ip().to_string(),
            scanned: fp.report.scanned,
            elapsed_ms: fp.report.elapsed_ms,
            open_ports: fp
                .report
                .findings
                .iter()
                .map(|f| OpenPortDto {
                    port: f.port,
                    service: f.service.map(str::to_string),
                    banner: f.banner.clone(),
                    connect_rtt_ms: f.connect_rtt_ms,
                })
                .collect(),
            // `open_ports_summary` 是纯技术串（`22(ssh) 80(http)`），两种语言一样
            open_ports_summary: fp.open_ports_summary(),
            device_kind: fp
                .device
                .as_ref()
                .map(|d| d.kind.label_in(lang).to_string()),
            device_confidence: fp
                .device
                .as_ref()
                .map(|d| d.confidence.label_in(lang).to_string()),
            device_is_inference: true,
            device_summary: fp.device_summary_in(lang),
            // 依据是**结构化信号**（厂商名 / 端口组合），渲染成哪种语言由这里决定。
            // 厂商名本身来自 OUI 库，是数据不是文案，原样保留。
            device_basis: fp
                .device
                .as_ref()
                .map(|d| d.basis.iter().map(|b| b.render_in(lang)).collect())
                .unwrap_or_default(),
        })
        .collect();

    // 写回：导出时会带上；`results` 一个字段都不动
    {
        let mut sh = state.shared.lock().expect("state poisoned");
        sh.fingerprints = fps;
        sh.fingerprint_ports = scanned;
        sh.fingerprint_banner = banner;
    }

    Ok(dtos)
}

/// M13：清空指纹结果（未做指纹时导出里不会出现指纹小节）。
#[tauri::command]
pub fn clear_fingerprints(state: State<'_, AppState>) -> Result<(), String> {
    let mut sh = state.shared.lock().expect("state poisoned");
    sh.fingerprints.clear();
    sh.fingerprint_ports = 0;
    Ok(())
}

/// M13：当前是否已有指纹结果与它的规模（界面据此显示角标）。
#[tauri::command]
pub fn fingerprint_state(state: State<'_, AppState>) -> Result<(usize, usize, bool), String> {
    let sh = state.shared.lock().expect("state poisoned");
    Ok((
        sh.fingerprints.len(),
        sh.fingerprint_ports,
        sh.fingerprint_banner,
    ))
}

/// 面向界面的一行能力摘要（例如「ICMP 可用（需要权限，仅 IPv4）；ICMPv6 未实装…」）。
#[tauri::command]
pub fn backend_summary(lang: Option<String>) -> Result<String, String> {
    Ok(BackendRegistry::new().summary_in(parse_lang(lang)))
}

/// v2.0.0 是否带上可选异步调度（编译期决定，供「关于」页如实显示）。
#[tauri::command]
pub fn async_scheduler_available() -> Result<bool, String> {
    Ok(cfg!(feature = "async-scheduler"))
}

#[tauri::command]
// 8 个参数确实偏多，但这里**不能**合并成一个结构体：命令参数名就是前端
// `invoke` 传上来的字段名，改成嵌套结构会改变 IPC 载荷的形状——
// 那是「行为」的一部分，本轮明确不许动。多出来的那个是 `lang`。
#[allow(clippy::too_many_arguments)]
pub fn start_scan(
    app: AppHandle,
    state: State<'_, AppState>,
    prefix: String,
    timeout_ms: u64,
    concurrency: usize,
    mode: Option<String>,
    tcp_port: Option<u16>,
    lang: Option<String>,
) -> Result<String, String> {
    // 语言是**本次任务**的属性：整轮扫描期间推送的事件文案都用它，
    // 因此 `Lang` 是 `Copy` 的，直接随闭包一起 `move` 进后台线程即可。
    let lang = parse_lang(lang);
    let spec = parse_spec(&prefix, TargetRange::Standard).map_err(|e| e.localized(lang))?;
    let targets = spec.targets.clone();
    let total = targets.len();

    let mut mode = match mode.as_deref() {
        Some("tcp") => ProbeMode::Tcp {
            port: tcp_port.filter(|p| *p > 0).unwrap_or(80),
        },
        Some("icmp") => ProbeMode::Icmp,
        // 未指定时沿用上一次的方式（首次启动为平台默认）
        _ => {
            let sh = state.shared.lock().expect("state poisoned");
            sh.mode
        }
    };
    if let ProbeMode::Tcp { port } = mode {
        if port == 0 {
            mode = ProbeMode::Tcp { port: 80 };
        }
    }
    mode.validate_in(lang).map_err(|e| e.localized(lang))?;

    let thresholds = {
        let sh = state.shared.lock().expect("state poisoned");
        sh.thresholds
    };

    let config = ProbeConfig {
        prefix: parse_prefix(&prefix).ok(),
        range: TargetRange::Standard,
        timeout_ms: if timeout_ms == 0 {
            cping_core::DEFAULT_TIMEOUT_MS
        } else {
            timeout_ms
        },
        concurrency: if concurrency == 0 {
            cping_core::DEFAULT_CONCURRENCY
        } else {
            concurrency
        },
        thresholds,
        mode,
    };

    // 每次扫描按当前方式新建调度器：一次任务只有一种方式，
    // 这样「ICMP 在线」与「TCP 开放」永远不可能混在同一次统计里。
    //
    // M17：这里**刻意**不调用 `set_policy`，用的是 `ScanPolicy::default()`
    // ——桌面端的用户最不可能去读文档，所以「默认限速 + 拒绝保留网段」
    // 必须是无条件的默认值，而不是一个需要用户去找开关的选项。
    // 界面里可调的策略（限速值等）随 M18 的界面工作一起接入。
    let scheduler = ProbeScheduler::new(select_backend(mode));
    let handle: SchedulerHandle = scheduler
        .start(&config, targets.clone())
        .map_err(|e| e.localized(lang))?;

    let task_id = handle.task_id.clone();
    let token = handle.cancel_token();
    let slots = matrix_slots(&targets);

    {
        let mut sh = state.shared.lock().expect("state poisoned");
        sh.running = true;
        sh.cancelled = false;
        sh.task_id = Some(task_id.clone());
        sh.prefix = prefix;
        sh.scan_time = now_string();
        // M14：本轮的时间戳取「开扫这一刻」，供时序点使用。
        sh.round_at_ms = unix_millis();
        sh.targets = targets;
        sh.cell_slots = slots;
        sh.results = vec![None; total];
        sh.stats = ScanStats::from_results_with_total(&[], total);
        sh.elapsed_ms = 0;
        sh.cancel = Some(token);
        sh.mode = mode;
        sh.spec = spec;
    }

    // 后台线程：消费核心事件 → 更新共享状态 + 推送前端
    let shared = Arc::clone(&state.shared);
    std::thread::spawn(move || {
        while let Some(event) = handle.recv() {
            match &event {
                ProbeEvent::Result { result, .. } => {
                    let mut sh = shared.lock().expect("state poisoned");
                    if let Some(slot) = sh.results.get_mut(result.index as usize) {
                        *slot = Some(result.clone());
                    }
                    let done: Vec<ProbeResult> = sh.results.iter().flatten().cloned().collect();
                    sh.stats = ScanStats::from_results_with_total(&done, sh.targets.len());
                }
                ProbeEvent::Finished {
                    stats, elapsed_ms, ..
                } => {
                    let mut sh = shared.lock().expect("state poisoned");
                    sh.stats = stats.clone();
                    sh.elapsed_ms = *elapsed_ms;
                    sh.running = false;
                    sh.cancel = None;
                }
                ProbeEvent::Cancelled {
                    stats, elapsed_ms, ..
                } => {
                    let mut sh = shared.lock().expect("state poisoned");
                    sh.stats = stats.clone();
                    sh.elapsed_ms = *elapsed_ms;
                    sh.running = false;
                    sh.cancelled = true;
                    sh.cancel = None;
                }
                ProbeEvent::Started { .. } | ProbeEvent::Progress { .. } => {}
            }

            let terminal = event.is_terminal();
            // M15：本轮触发的告警。声明在 `terminal` 块外，好在锁释放后 emit 出去。
            let mut fired: Vec<cping_core::AlertEvent> = Vec::new();
            if terminal {
                // 扫描一结束就补一次 MAC/厂商：ARP 表此刻最新鲜（见 core::enrich 的说明）。
                // 必须在 emit 之前做完，这样前端收到终端事件后立刻拉取的 snapshot 已带上新字段。
                let mut sh = shared.lock().expect("state poisoned");
                let mut done = collect_results(&sh);
                enrich_with_mac_and_vendor(&mut done);
                for r in done {
                    if let Some(slot) = sh.results.get_mut(r.index as usize) {
                        *slot = Some(r);
                    }
                }
                // M8：把本轮交给监控状态机（轮次 +1、算变化、更新基线），
                // 并留下「上一轮 → 本轮」的完整差异供导出 diff 报告。
                let current = collect_results(&sh);
                let previous: Vec<ProbeResult> = sh.monitor.previous().to_vec();
                if !previous.is_empty() && !current.is_empty() {
                    let diff = diff_reports(&previous, &current, &sh.thresholds);
                    sh.last_diff = Some(diff);
                }
                let thresholds_now = sh.thresholds;
                sh.monitor.commit_round(&current, &thresholds_now);
                // M14：同一份结果也进长期时序。时间戳用开扫时刻（见 `round_at_ms`）。
                let at = if sh.round_at_ms == 0 {
                    unix_millis()
                } else {
                    sh.round_at_ms
                };
                sh.series.append_results(at, &current);
                // M15：告警求值放在时序追加**之后**——可用率规则要用含本轮在内的
                // 累计观测数，先追加再求值才自洽（与 CLI / TUI 同一处顺序）。
                if sh.alerts_enabled {
                    let uptime = if sh.series.is_empty() {
                        Default::default()
                    } else {
                        sh.series.uptime_map()
                    };
                    let round = sh.monitor.round();
                    // 先把阈值复制出来：下面 `alerts` 是可变借用，再借 `sh.thresholds` 会冲突
                    let thresholds_now = sh.thresholds;
                    // 正文按当前界面语言产出。**告警状态文件里不存 message**
                    // （只存 kind / ip / 时间戳），所以这里跟着语言走不会改变
                    // 落盘内容，数据契约不受影响。
                    fired = sh.alerts.evaluate_in(
                        at,
                        round,
                        &[],
                        &current,
                        &thresholds_now,
                        &uptime,
                        lang,
                    );
                }
                // 进程随时可能被系统回收（Android 上销毁 Activity 即退出进程），
                // 所以结果一落地就顺手写盘，不依赖退出钩子。
                let _ = autosave(&app, &sh, lang);
            }

            let dto = to_event_dto(&event, &thresholds, mode, lang);
            let _ = app.emit("probe-event", dto);
            // 告警单独走一条事件：前端可以据此弹提示，而不必轮询。
            if !fired.is_empty() {
                let fired: Vec<AlertEventDto> =
                    fired.iter().map(|e| alert_event_dto(e, lang)).collect();
                let _ = app.emit("alert-event", fired);
            }
            if terminal {
                break;
            }
        }
    });

    Ok(task_id)
}

#[tauri::command]
pub fn cancel_scan(state: State<'_, AppState>) -> Result<(), String> {
    let sh = state.shared.lock().expect("state poisoned");
    if let Some(token) = &sh.cancel {
        token.cancel();
    }
    Ok(())
}

fn snapshot_dto(sh: &Shared, lang: Lang) -> SnapshotDto {
    let results = sh
        .results
        .iter()
        .flatten()
        .map(|r| to_result_dto(r, &sh.thresholds, sh.mode, lang))
        .collect();
    let diff_summary = sh
        .last_diff
        .as_ref()
        .filter(|d| !d.is_empty())
        .map(|d| diff_summary(d, lang))
        .unwrap_or_default();
    SnapshotDto {
        task_id: sh.task_id.clone(),
        running: sh.running,
        cancelled: sh.cancelled,
        prefix: sh.prefix.clone(),
        total: sh.targets.len(),
        elapsed_ms: sh.elapsed_ms,
        stats: ScanStatsDto::from(&sh.stats),
        results,
        cell_slots: sh.cell_slots.clone(),
        targets: sh.targets.iter().map(|ip| ip.to_string()).collect(),
        mode: sh.mode.key().to_string(),
        // `describe()` 产出 `ICMP` / `TCP:445`——技术记号，两种语言一致。
        mode_label: sh.mode.describe(),
        tcp_port: sh.mode.tcp_port(),
        spec_summary: sh.spec.summary_in(lang),
        segments: sh.spec.segments.iter().map(SegmentDto::from).collect(),
        matrix_compatible: sh.spec.matrix_compatible(),
        round: sh.monitor.round(),
        changes: sh
            .monitor
            .last_round_changes()
            .iter()
            .map(|c| change_dto(c, lang))
            .collect(),
        change_total: sh.monitor.log().total_seen(),
        diff_summary,
    }
}

#[tauri::command]
pub fn snapshot(state: State<'_, AppState>, lang: Option<String>) -> Result<SnapshotDto, String> {
    let lang = parse_lang(lang);
    let sh = state.shared.lock().expect("state poisoned");
    Ok(snapshot_dto(&sh, lang))
}

/// 变化事件流（最近 N 条，新的在前）。
#[tauri::command]
pub fn recent_changes(
    state: State<'_, AppState>,
    limit: usize,
    lang: Option<String>,
) -> Result<Vec<ChangeDto>, String> {
    let lang = parse_lang(lang);
    let sh = state.shared.lock().expect("state poisoned");
    Ok(sh
        .monitor
        .log()
        .iter_recent_first()
        .take(limit.max(1))
        .map(|c| change_dto(c, lang))
        .collect())
}

fn collect_results(sh: &Shared) -> Vec<ProbeResult> {
    sh.results.iter().flatten().cloned().collect()
}

/// 构造导出用的报告（M10：CSV/TXT/JSON/HTML 走同一个构造点）。
///
/// `lang` **只用于「没有结果可导出」这条错误提示**，报告本身的内容恒为中文：
/// 报表是**数据契约**——同一份 CSV / JSON 在中文与英文机器上必须逐字节一致，
/// 否则下游脚本就得按语言分支。所以 `target_desc` 仍走
/// `TargetSpec::summary()`（即 `summary_in(Lang::Zh)`）。
fn build_report(sh: &Shared, lang: Lang) -> Result<ScanReport, String> {
    let done = collect_results(sh);
    if done.is_empty() {
        return Err(i18n::render(lang, Key::ExportNoResults, &[]));
    }
    let meta = ExportMeta {
        scan_time: sh.scan_time.clone(),
        local_ip: local_ip_label(),
        elapsed_ms: sh.elapsed_ms,
        mode: sh.mode,
        target_desc: sh.spec.summary(),
        round: sh.monitor.round(),
    };
    Ok(ScanReport::new(
        &done,
        meta,
        sh.thresholds,
        sh.monitor.last_round_changes().to_vec(),
    )
    .with_fingerprints(sh.fingerprints.clone()))
}

fn write_export(sh: &Shared, path: &str, lang: Lang) -> Result<String, String> {
    let format = ExportFormat::from_path(path)
        .ok_or_else(|| i18n::render(lang, Key::ExportBadExtension, &[]))?;
    let report = build_report(sh, lang)?;
    let rows = report.results.len();
    let bytes = report
        .render(format, &ColorConfig::default())
        .map_err(|e| e.localized(lang))?;
    std::fs::write(path, &bytes)
        .map_err(|e| i18n::render(lang, Key::WriteFailed, &[path, &e.to_string()]))?;
    Ok(i18n::render(
        lang,
        Key::ExportDone,
        &[&rows.to_string(), &export_format_label(format, lang), path],
    ))
}

#[tauri::command]
pub fn export_results(
    state: State<'_, AppState>,
    path: String,
    lang: Option<String>,
) -> Result<String, String> {
    let lang = parse_lang(lang);
    let sh = state.shared.lock().expect("state poisoned");
    write_export(&sh, &path, lang)
}

/// M10：把「上一轮 → 本轮」的差异导出成 CSV / JSON（按扩展名决定）。
#[tauri::command]
pub fn export_diff(
    state: State<'_, AppState>,
    path: String,
    lang: Option<String>,
) -> Result<String, String> {
    let lang = parse_lang(lang);
    let sh = state.shared.lock().expect("state poisoned");
    let diff = sh
        .last_diff
        .as_ref()
        .ok_or_else(|| i18n::render(lang, Key::DiffNoBaseline, &[]))?;
    let bytes = match ExportFormat::from_path(&path) {
        Some(ExportFormat::Csv) => diff.to_csv().map_err(|e| e.localized(lang))?,
        Some(other) => {
            return Err(i18n::render(
                lang,
                Key::DiffOnlyCsv,
                &[&export_format_label(other, lang)],
            ))
        }
        None => return Err(i18n::render(lang, Key::DiffNeedCsv, &[])),
    };
    std::fs::write(&path, &bytes)
        .map_err(|e| i18n::render(lang, Key::WriteFailed, &[&path, &e.to_string()]))?;
    Ok(i18n::render(
        lang,
        Key::DiffExportDone,
        &[&diff.entries.len().to_string(), &path],
    ))
}

// ---------------------------------------------------------------------------
// M14：历史与趋势
// ---------------------------------------------------------------------------

/// 单台主机的可用率报表行。
///
/// 与核心 [`cping_core::HostReport`] 一一对应，只是补了「显示文本」与曲线，
/// 前端不做任何数值加工——报表口径只能有一处。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UptimeRowDto {
    pub ip: String,
    pub samples: u64,
    pub online: u64,
    pub uptime_percent: f64,
    pub uptime_text: String,
    /// 平均 RTT（ms）。**从未在线为 null，不是 0**：0 会被读成「极快」。
    pub avg_rtt_ms: Option<f64>,
    pub avg_rtt_text: String,
    pub worst_rtt_ms: Option<u32>,
    pub worst_rtt_text: String,
    pub jitter_ms: f64,
    pub jitter_text: String,
    /// 出现过该主机的时间点数。
    pub points: usize,
    /// 在线率曲线：每点 0..1，`null` = 该轮**没测到**这台主机（≠ 离线）。
    pub curve: Vec<Option<f64>>,
    /// 内嵌 SVG 折线（核心纯函数产出，不引图表库）。
    pub sparkline: String,
}

/// 趋势页的整体状态。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrendDto {
    pub points: usize,
    pub rounds: u64,
    pub hosts: usize,
    /// 时间跨度（小时）。
    pub span_hours: f64,
    pub retention_days: u64,
    pub rows: Vec<UptimeRowDto>,
    /// 空状态时给一句人话；有数据时为 null。
    pub empty_hint: Option<String>,
}

/// 趋势页数据（M14）。`chart_width` 由前端按容器宽度给定，默认 160。
#[tauri::command]
pub fn trend_state(
    state: State<'_, AppState>,
    chart_width: Option<u32>,
    lang: Option<String>,
) -> Result<TrendDto, String> {
    let lang = parse_lang(lang);
    let sh = state.shared.lock().expect("state poisoned");
    let width = chart_width.unwrap_or(160).clamp(60, 640);
    let report = sh.series.report();
    let rows: Vec<UptimeRowDto> = report
        .iter()
        .map(|r| {
            let curve = sh.series.uptime_curve(r.ip);
            UptimeRowDto {
                ip: r.ip.to_string(),
                samples: r.samples,
                online: r.online,
                uptime_percent: r.uptime_percent(),
                // `*_text` 全是纯数值记号（`99.5%` / `12.3` / `-`），
                // 两种语言下一样，没有散文可翻。
                uptime_text: r.uptime_text(),
                avg_rtt_ms: r.avg_rtt_ms,
                avg_rtt_text: r.avg_rtt_text(),
                worst_rtt_ms: r.worst_rtt_ms,
                worst_rtt_text: r.worst_rtt_text(),
                jitter_ms: r.jitter_ms,
                jitter_text: r.jitter_text(),
                points: r.points,
                sparkline: cping_core::sparkline_svg(&curve, width, 28),
                curve,
            }
        })
        .collect();
    Ok(TrendDto {
        points: sh.series.len(),
        rounds: sh.series.total_rounds(),
        hosts: rows.len(),
        span_hours: sh.series.span_ms() as f64 / 3_600_000.0,
        retention_days: sh.series.retention_secs / 86_400,
        empty_hint: if rows.is_empty() {
            Some(i18n::render(lang, Key::TrendEmptyHint, &[]))
        } else {
            None
        },
        rows,
    })
}

/// 清空趋势历史。返回清掉的点数，好让界面如实回报「清了多少」。
#[tauri::command]
pub fn clear_trend(state: State<'_, AppState>) -> Result<usize, String> {
    let mut sh = state.shared.lock().expect("state poisoned");
    let removed = sh.series.len();
    sh.series = cping_core::TimeSeries::new();
    Ok(removed)
}

/// 把累积的可用率报表导出成 CSV / HTML（按扩展名决定）。
#[tauri::command]
pub fn export_uptime_report(
    state: State<'_, AppState>,
    path: String,
    lang: Option<String>,
) -> Result<String, String> {
    let lang = parse_lang(lang);
    let sh = state.shared.lock().expect("state poisoned");
    if sh.series.is_empty() {
        return Err(i18n::render(lang, Key::TrendNoData, &[]));
    }
    let report = sh.series.report();
    // 报表内容（CSV 列名 / HTML 标题正文）固定中文，理由见 `build_report`。
    let bytes = if path.to_ascii_lowercase().ends_with(".html") {
        cping_core::report_html(&sh.series, &report).into_bytes()
    } else if path.to_ascii_lowercase().ends_with(".csv") {
        cping_core::report_csv(&report)
    } else {
        return Err(i18n::render(lang, Key::UptimeReportOnlyCsvHtml, &[]));
    };
    std::fs::write(&path, &bytes)
        .map_err(|e| i18n::render(lang, Key::WriteFailed, &[&path, &e.to_string()]))?;
    Ok(i18n::render(
        lang,
        Key::UptimeReportDone,
        &[
            &report.len().to_string(),
            &sh.series.total_rounds().to_string(),
            &path,
        ],
    ))
}

// ---------------------------------------------------------------------------
// M15：告警
// ---------------------------------------------------------------------------

/// 一条规则的对外形态。`label` / `usesThreshold` 都来自核心，前端不再自己判断。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertRuleDto {
    pub kind: String,
    pub label: String,
    pub enabled: bool,
    /// 只有延迟/可用率两类用得到阈值；边沿类规则这里恒为 false。
    pub uses_threshold: bool,
    pub threshold: u32,
    pub cooldown_secs: u64,
}

/// 一条告警事件。`time` 已按 UTC 格式化——前端不做时区换算，免得两端口径不一。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertEventDto {
    pub kind: String,
    pub kind_label: String,
    pub severity: String,
    pub severity_label: String,
    pub ip: String,
    pub at_ms: u64,
    pub time: String,
    pub round: u64,
    /// 触发时的实际值（延迟 ms / 可用率百分数）；边沿类规则为 null。
    pub value: Option<f64>,
    pub threshold: Option<u32>,
    pub message: String,
}

/// 告警页的整体状态。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertDto {
    pub enabled: bool,
    pub muted: bool,
    /// 是否已建立基线（第一轮之后为 true）。
    pub primed: bool,
    pub active_rules: usize,
    /// 累计告警条数（「清空」会一并归零）。
    pub total: u64,
    /// 当前缓冲里保留的条数。
    pub kept: usize,
    pub suppressed_by_cooldown: u64,
    pub suppressed_by_mute: u64,
    pub rules: Vec<AlertRuleDto>,
    pub events: Vec<AlertEventDto>,
    /// 延迟阈值（ms）/ 可用率阈值（%）/ 冷却（秒）——便捷项，供前端表单回填。
    pub latency_threshold_ms: u32,
    pub uptime_threshold_pct: u32,
    pub cooldown_secs: u64,
    /// 空状态时给一句人话；有数据时为 null。
    pub empty_hint: Option<String>,
}

fn alert_event_dto(e: &cping_core::AlertEvent, lang: Lang) -> AlertEventDto {
    AlertEventDto {
        kind: e.kind.key().to_string(),
        kind_label: e.kind.label_in(lang).to_string(),
        severity: e.severity.key().to_string(),
        severity_label: e.severity.label_in(lang).to_string(),
        ip: e.ip.to_string(),
        at_ms: e.at_ms,
        time: cping_core::format_unix_local(e.at_ms / 1000),
        round: e.round,
        value: e.value,
        threshold: e.threshold,
        // `message` 由核心的 `AlertEngine::evaluate` 组装，而它内部用的是
        // **不带语言**的 `match_rule`（核心目前只有 `match_rule_in` 这个自由函数，
        // 引擎没有 `evaluate_in`）。界面**不能**重写它——那等于把告警文案在
        // GUI 里复刻一遍，两处一旦改措辞就会漂移。
        //
        // 而且这条消息还会原样落进告警状态文件（数据契约），本就该恒为一种语言。
        // 所以它保持原样，是本次改动的**已知边界**；跟着语言变的是外面的
        // `kind_label` / `severity_label` 与整页的其它文案。
        // TUI 侧有一模一样的处理与说明（`apps/cping-tui/src/app.rs`）。
        message: e.message.clone(),
    }
}

/// 把引擎状态折成 DTO。`limit` 限制返回的事件条数（最新的在前）。
fn alert_dto(sh: &Shared, limit: usize, lang: Lang) -> AlertDto {
    let engine = &sh.alerts;
    let rules: Vec<AlertRuleDto> = engine
        .config()
        .rules
        .iter()
        .map(|r| AlertRuleDto {
            kind: r.kind.key().to_string(),
            label: r.kind.label_in(lang).to_string(),
            enabled: r.enabled,
            uses_threshold: r.kind.uses_threshold(),
            threshold: r.threshold,
            cooldown_secs: r.cooldown_secs,
        })
        .collect();
    let events: Vec<AlertEventDto> = engine
        .log()
        .iter_recent_first()
        .take(limit)
        .map(|e| alert_event_dto(e, lang))
        .collect();
    let threshold_of = |k: cping_core::AlertKind| {
        engine
            .config()
            .rule(k)
            .map(|r| r.threshold)
            .unwrap_or_default()
    };
    let cooldown = engine
        .config()
        .rules
        .first()
        .map(|r| r.cooldown_secs)
        .unwrap_or_default();
    AlertDto {
        enabled: sh.alerts_enabled,
        muted: engine.is_muted(),
        primed: engine.is_primed(),
        active_rules: rules.iter().filter(|r| r.enabled).count(),
        total: engine.log().total_seen(),
        kept: events.len(),
        suppressed_by_cooldown: engine.suppressed_by_cooldown(),
        suppressed_by_mute: engine.suppressed_by_mute(),
        latency_threshold_ms: threshold_of(cping_core::AlertKind::LatencyAbove),
        uptime_threshold_pct: threshold_of(cping_core::AlertKind::UptimeBelow),
        cooldown_secs: cooldown,
        rules,
        empty_hint: if events.is_empty() {
            Some(if sh.alerts_enabled {
                i18n::render(lang, Key::AlertEmptyPrimed, &[])
            } else {
                i18n::render(lang, Key::AlertEmptyDisabled, &[])
            })
        } else {
            None
        },
        events,
    }
}

/// 告警页数据。`limit` 默认 200。
#[tauri::command]
pub fn alert_state(
    state: State<'_, AppState>,
    limit: Option<u32>,
    lang: Option<String>,
) -> Result<AlertDto, String> {
    let lang = parse_lang(lang);
    let sh = state.shared.lock().expect("state poisoned");
    Ok(alert_dto(
        &sh,
        limit.unwrap_or(200).clamp(1, 2000) as usize,
        lang,
    ))
}

/// 开/关告警。**不重置任何状态**：关掉再打开不该丢掉冷却记录与基线。
#[tauri::command]
pub fn set_alerts_enabled(
    state: State<'_, AppState>,
    enabled: bool,
    lang: Option<String>,
) -> Result<AlertDto, String> {
    let lang = parse_lang(lang);
    let mut sh = state.shared.lock().expect("state poisoned");
    sh.alerts_enabled = enabled;
    Ok(alert_dto(&sh, 200, lang))
}

/// 静音开关。规则照常求值、冷却照常推进，只是不产生事件——
/// 所以解除静音**不会**补报一堆积压告警（核心有对应回归测试）。
#[tauri::command]
pub fn set_alert_muted(
    state: State<'_, AppState>,
    muted: bool,
    lang: Option<String>,
) -> Result<AlertDto, String> {
    let lang = parse_lang(lang);
    let mut sh = state.shared.lock().expect("state poisoned");
    sh.alerts.set_muted(muted);
    Ok(alert_dto(&sh, 200, lang))
}

/// 改阈值 / 冷却 / 启用哪几类。
///
/// 只覆盖**显式给过**的字段；`apply_config` 会保留冷却记录与基线，
/// 所以改一次阈值不会把「某台主机刚报过」的记录清掉，也就不会立刻重复上报。
/// 返回 `Result` + 正确文本：界面能直接展示后端给的原因，而不是自己编一句。
#[tauri::command]
pub fn update_alert_rules(
    state: State<'_, AppState>,
    latency_ms: Option<u32>,
    uptime_pct: Option<u32>,
    cooldown_secs: Option<u64>,
    kinds: Option<Vec<String>>,
    lang: Option<String>,
) -> Result<AlertDto, String> {
    let lang = parse_lang(lang);
    let mut sh = state.shared.lock().expect("state poisoned");
    if let Some(pct) = uptime_pct {
        if pct > 100 {
            return Err(i18n::render(
                lang,
                Key::AlertBadUptimeThreshold,
                &[&pct.to_string()],
            ));
        }
    }
    let mut cfg = sh.alerts.config().clone();
    if let Some(v) = latency_ms {
        if let Some(rule) = cfg.rule_mut(cping_core::AlertKind::LatencyAbove) {
            rule.threshold = v;
        }
    }
    if let Some(v) = uptime_pct {
        if let Some(rule) = cfg.rule_mut(cping_core::AlertKind::UptimeBelow) {
            rule.threshold = v;
        }
    }
    if let Some(v) = cooldown_secs {
        for rule in &mut cfg.rules {
            rule.cooldown_secs = v;
        }
    }
    if let Some(names) = kinds {
        let parsed: Vec<cping_core::AlertKind> = names
            .iter()
            .filter_map(|n| cping_core::AlertKind::parse_key(n))
            .collect();
        if parsed.len() != names.len() {
            let known = cping_core::all_kinds()
                .iter()
                .map(|k| k.key())
                .collect::<Vec<_>>()
                .join("/");
            // `names` 是用户/前端传进来的**数据**，用 `{:?}` 原样展示
            // （它可能是任何字符串，翻不了也不该翻）。
            return Err(i18n::render(
                lang,
                Key::AlertUnsupportedKinds,
                &[&known, &format!("{names:?}")],
            ));
        }
        // 空列表 = 关掉全部规则。这是用户的明确意图，照做。
        cfg.with_kinds(&parsed);
    }
    sh.alerts.apply_config(cfg);
    Ok(alert_dto(&sh, 200, lang))
}

/// 清空告警记录。**只丢事件**：`seen` 基线与冷却记录照旧保留，
/// 否则下一轮会把整段在线主机当成「新增」重新刷一遍。
#[tauri::command]
pub fn clear_alerts(state: State<'_, AppState>, lang: Option<String>) -> Result<AlertDto, String> {
    let lang = parse_lang(lang);
    let mut sh = state.shared.lock().expect("state poisoned");
    sh.alerts.log_mut().clear();
    Ok(alert_dto(&sh, 200, lang))
}

/// Android 等无桌面文件对话框的场景：写到平台提供的可写目录并回报完整路径。
#[tauri::command]
pub fn export_to_app_dir(
    app: AppHandle,
    state: State<'_, AppState>,
    format: String,
    lang: Option<String>,
) -> Result<String, String> {
    let lang = parse_lang(lang);
    let dir: PathBuf = app
        .path()
        .document_dir()
        .or_else(|_| app.path().app_data_dir())
        .map_err(|e| i18n::render(lang, Key::CannotDetermineSaveDir, &[&e.to_string()]))?;
    std::fs::create_dir_all(&dir).map_err(|e| {
        i18n::render(
            lang,
            Key::CannotCreateDir,
            &[&dir.display().to_string(), &e.to_string()],
        )
    })?;

    let fmt = match format.as_str() {
        "txt" => ExportFormat::Txt,
        "json" => ExportFormat::Json,
        // M10：HTML 报告在移动端同样可用（单文件、无外部资源）
        "html" => ExportFormat::Html,
        _ => ExportFormat::Csv,
    };
    let path = dir.join(format!("cping-{}.{}", file_stamp(), fmt.extension()));
    let path_str = path.to_string_lossy().to_string();

    let sh = state.shared.lock().expect("state poisoned");
    write_export(&sh, &path_str, lang)?;
    Ok(path_str)
}

fn config_path(app: &AppHandle, lang: Lang) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| i18n::render(lang, Key::CannotDetermineConfigDir, &[&e.to_string()]))?;
    std::fs::create_dir_all(&dir).map_err(|e| {
        i18n::render(
            lang,
            Key::CannotCreateConfigDir,
            &[&dir.display().to_string(), &e.to_string()],
        )
    })?;
    Ok(dir.join("cping.toml"))
}

#[tauri::command]
pub fn load_config(app: AppHandle, lang: Option<String>) -> Result<AppConfigDto, String> {
    let lang = parse_lang(lang);
    let path = config_path(&app, lang)?;
    let mut cfg = if path.exists() {
        let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        AppConfig::from_toml(&text).map_err(|e| e.localized(lang))?
    } else {
        AppConfig::default()
    };

    // 允许用环境变量覆盖初始网段：自动化验证时可以强制指向回环，
    // 避免脚本误扫局域网（例如 CPING_PREFIX=127.0.0）。
    if let Ok(p) = std::env::var("CPING_PREFIX") {
        if let Ok(parsed) = parse_prefix(&p) {
            cfg.probe.prefix = Some(parsed);
        }
    }

    Ok(config_to_dto(&cfg))
}

#[tauri::command]
pub fn save_config(
    app: AppHandle,
    state: State<'_, AppState>,
    config: AppConfigDto,
    lang: Option<String>,
) -> Result<(), String> {
    let lang = parse_lang(lang);
    let cfg = config_from_dto(config);
    // 核心的 `AppConfig::validate` 没有 `_in` 变体，所以外层这层框
    // （`配置无效：…`）按语言渲染，里面的**细节**仍是核心给的中文原话
    // ——不在这里替它重写：那句话的所有权在核心，复刻一遍必然漂移。
    cfg.validate().map_err(|e| e.localized(lang))?;
    let path = config_path(&app, lang)?;
    std::fs::write(&path, cfg.to_toml())
        .map_err(|e| i18n::render(lang, Key::WriteConfigFailed, &[&e.to_string()]))?;

    let mut sh = state.shared.lock().expect("state poisoned");
    sh.thresholds = cfg.probe.thresholds;
    Ok(())
}

#[tauri::command]
pub fn get_local_addresses() -> Result<Vec<String>, String> {
    use cping_core::local_ip::{LocalAddressProvider, UnsupportedProvider};
    let provider = UnsupportedProvider;
    // M5 接入真实网卡枚举；在此之前返回空列表，绝不编造地址
    Ok(provider
        .list()
        .map(|list| list.iter().map(|a| a.ip.to_string()).collect())
        .unwrap_or_default())
}

// ---------------------------------------------------------------------------
// M7 Commands：目标信息补全（MAC / 厂商 / 主机名）
// ---------------------------------------------------------------------------

/// 读一次邻居表并回填 MAC 与厂商。可重复调用（只补空缺，不覆盖已有值）。
#[tauri::command]
pub fn enrich_results(
    app: AppHandle,
    state: State<'_, AppState>,
    lang: Option<String>,
) -> Result<EnrichReportDto, String> {
    let lang = parse_lang(lang);
    let mut sh = state.shared.lock().expect("state poisoned");
    if sh.results.is_empty() {
        return Err(i18n::render(lang, Key::EnrichNoResults, &[]));
    }
    // 取出 → 补全 → 按原下标写回：下标即目标的低 8 位，位置绝不能变。
    let mut done = collect_results(&sh);
    let report = enrich_with_mac_and_vendor(&mut done);
    for r in done {
        if let Some(slot) = sh.results.get_mut(r.index as usize) {
            *slot = Some(r);
        }
    }
    let autosaved_to = autosave(&app, &sh, lang);
    Ok(EnrichReportDto::from_report(
        report,
        &sh.results,
        autosaved_to,
        lang,
    ))
}

/// 按需反查单个目标的主机名（阻塞操作，放在锁外做）。
#[tauri::command]
pub fn lookup_hostname_for(
    app: AppHandle,
    state: State<'_, AppState>,
    index: u16,
    timeout_ms: u64,
    lang: Option<String>,
) -> Result<HostnameLookupDto, String> {
    let lang = parse_lang(lang);
    let timeout = Duration::from_millis(timeout_ms.clamp(200, 10_000));

    // 1) 锁内只取出副本，绝不在持锁状态下做 DNS 查询
    let (ip, mut probe, task_id) = {
        let sh = state.shared.lock().expect("state poisoned");
        let r = sh
            .results
            .get(index as usize)
            .and_then(|s| s.as_ref())
            .ok_or_else(|| i18n::render(lang, Key::IndexNoResult, &[&index.to_string()]))?;
        (r.ip, r.clone(), sh.task_id.clone())
    };

    let found = lookup_hostname(&mut probe, timeout);

    // 2) 写回前确认中间没有开始新一轮扫描，否则会把旧结果写进新任务
    let mut sh = state.shared.lock().expect("state poisoned");
    if sh.task_id != task_id {
        return Err(i18n::render(lang, Key::TaskSwitched, &[]));
    }
    if let Some(slot) = sh.results.get_mut(index as usize) {
        if let Some(cur) = slot.as_mut() {
            cur.hostname = probe.hostname.clone();
        }
    }
    autosave(&app, &sh, lang);

    Ok(HostnameLookupDto {
        index,
        ip: ip.to_string(),
        hostname: probe.hostname,
        found,
    })
}

// ---------------------------------------------------------------------------
// M7 Commands：结果快照的落盘与恢复
// ---------------------------------------------------------------------------

/// 自动落盘文件的固定名（放在应用数据目录，用户无需关心）。
const AUTOSAVE_FILE: &str = "cping-autosave.snap";
/// 「已处理过」的快照指纹。作用：同一份结果只询问一次，避免每次启动都弹框。
const HANDLED_FILE: &str = "cping-autosave.handled";

fn app_data_dir(app: &AppHandle, lang: Lang) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| i18n::render(lang, Key::CannotDetermineAppDataDir, &[&e.to_string()]))?;
    std::fs::create_dir_all(&dir).map_err(|e| {
        i18n::render(
            lang,
            Key::CannotCreateDir,
            &[&dir.display().to_string(), &e.to_string()],
        )
    })?;
    Ok(dir)
}

fn autosave_path(app: &AppHandle, lang: Lang) -> Result<PathBuf, String> {
    Ok(app_data_dir(app, lang)?.join(AUTOSAVE_FILE))
}

fn default_snapshot_name() -> String {
    format!("cping-{}.snap", file_stamp())
}

/// 把当前内存中的结果组装成快照（不含文件系统操作）。
fn build_snapshot(sh: &Shared) -> Snapshot {
    Snapshot {
        scan_time: if sh.scan_time.is_empty() {
            now_string()
        } else {
            sh.scan_time.clone()
        },
        prefix: sh.prefix.clone(),
        local_ip: local_ip_label(),
        elapsed_ms: sh.elapsed_ms,
        thresholds: sh.thresholds,
        total: sh.targets.len(),
        cancelled: sh.cancelled,
        results: collect_results(sh),
    }
}

/// 同一份结果的判定指纹：任一要素变化即视为「新结果」，需要重新询问。
fn fingerprint(snap: &Snapshot) -> String {
    format!(
        "{}|{}|{}|{}|{}",
        snap.scan_time,
        snap.prefix,
        snap.total,
        snap.results.len(),
        snap.elapsed_ms
    )
}

/// 记下「这份快照用户已经处理过」。写失败不致命，最多下次多问一次。
///
/// 这里的 `lang` 只用于格式化那个**被丢掉**的错误——保留参数是为了让
/// 这条链路与其它落盘路径口径一致（错误文案在产生处就按语言定好，
/// 而不是等某个调用方想起来再翻）。
fn mark_handled(app: &AppHandle, snap: &Snapshot, lang: Lang) {
    if let Ok(path) = app_data_dir(app, lang).map(|d| d.join(HANDLED_FILE)) {
        let _ = std::fs::write(path, fingerprint(snap));
    }
}

fn read_handled(app: &AppHandle, lang: Lang) -> Option<String> {
    let path = app_data_dir(app, lang).ok()?.join(HANDLED_FILE);
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
}

fn write_snapshot_file(path: &FsPath, snap: &Snapshot, lang: Lang) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir).map_err(|e| {
                i18n::render(
                    lang,
                    Key::CannotCreateDir,
                    &[&dir.display().to_string(), &e.to_string()],
                )
            })?;
        }
    }
    save_to_path(snap, &path.to_string_lossy()).map_err(|e| e.localized(lang))
}

/// 自动落盘：无目标或结果为空时不落盘（避免用一份空快照盖掉上次的真实结果）。
fn autosave(app: &AppHandle, sh: &Shared, lang: Lang) -> Option<String> {
    let snap = build_snapshot(sh);
    if snap.total == 0 || snap.results.is_empty() {
        return None;
    }
    let path = autosave_path(app, lang).ok()?;
    write_snapshot_file(&path, &snap, lang).ok()?;
    Some(path.to_string_lossy().to_string())
}

/// 供窗口失焦/关闭时调用：只在后台做一次落盘，不改动任何界面状态。
///
/// 这里**没有**前端传来的 `lang`（不是用户触发的命令），只能靠环境推断；
/// 而且本函数的错误本来就被丢弃（自动落盘失败不该打扰用户），
/// 所以语言只影响那个不会被显示出来的错误串。
pub fn autosave_on_background(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let Ok(sh) = state.shared.lock() else { return };
    autosave(app, &sh, Lang::from_env());
}

/// 显式保存到指定路径（桌面端走文件对话框）。
#[tauri::command]
pub fn save_snapshot(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    lang: Option<String>,
) -> Result<SnapshotSavedDto, String> {
    save_snapshot_in(&app, &state, &path, parse_lang(lang))
}

/// [`save_snapshot`] 的实现体。
///
/// 拆出来是为了让 [`save_snapshot_to_app_dir`] 复用同一段逻辑时**不必**
/// 把已经解析好的 `Lang` 再编码成字符串、让下游重新解析一遍——
/// 那种「往返编码」是两处推断可能不一致的温床。
fn save_snapshot_in(
    app: &AppHandle,
    state: &State<'_, AppState>,
    path: &str,
    lang: Lang,
) -> Result<SnapshotSavedDto, String> {
    if path.trim().is_empty() {
        return Err(i18n::render(lang, Key::SnapshotNoPath, &[]));
    }
    let sh = state.shared.lock().expect("state poisoned");
    let snap = build_snapshot(&sh);
    if snap.total == 0 {
        return Err(i18n::render(lang, Key::SnapshotNoResults, &[]));
    }
    let target = PathBuf::from(path);
    write_snapshot_file(&target, &snap, lang)?;
    // 显式保存过就说明结果已经落到用户指定的地方，不再提示恢复
    mark_handled(app, &snap, lang);
    Ok(SnapshotSavedDto {
        path: target.to_string_lossy().to_string(),
        rows: snap.results.len(),
        total: snap.total,
    })
}

/// Android 等无文件对话框的场景：写到平台可写目录。
#[tauri::command]
pub fn save_snapshot_to_app_dir(
    app: AppHandle,
    state: State<'_, AppState>,
    lang: Option<String>,
) -> Result<SnapshotSavedDto, String> {
    let lang = parse_lang(lang);
    let dir: PathBuf = app
        .path()
        .document_dir()
        .or_else(|_| app.path().app_data_dir())
        .map_err(|e| i18n::render(lang, Key::CannotDetermineSaveDir, &[&e.to_string()]))?;
    let target = dir.join(default_snapshot_name());
    save_snapshot_in(&app, &state, &target.to_string_lossy(), lang)
}

/// 启动时检查是否存在「上次没处理完」的快照；没有则返回 None。
#[tauri::command]
pub fn pending_snapshot(
    app: AppHandle,
    lang: Option<String>,
) -> Result<Option<PendingSnapshotDto>, String> {
    let lang = parse_lang(lang);
    let path = autosave_path(&app, lang)?;
    if !path.exists() {
        return Ok(None);
    }
    let text = match std::fs::read_to_string(&path) {
        // 文件读不出来是**操作系统的原话**（`Access is denied.`），原样透传。
        Err(e) => return Ok(Some(PendingSnapshotDto::unreadable(&path, e.to_string()))),
        Ok(t) => t,
    };
    match Snapshot::parse(&text) {
        Ok(snap) => {
            if read_handled(&app, lang).as_deref() == Some(fingerprint(&snap).as_str()) {
                return Ok(None);
            }
            Ok(Some(PendingSnapshotDto {
                path: path.to_string_lossy().to_string(),
                ok: true,
                error: None,
                scan_time: snap.scan_time.clone(),
                prefix: snap.prefix.clone(),
                total: snap.total,
                rows: snap.results.len(),
                elapsed_ms: snap.elapsed_ms,
                cancelled: snap.cancelled,
            }))
        }
        Err(e) => Ok(Some(PendingSnapshotDto::unreadable(
            &path,
            e.localized(lang),
        ))),
    }
}

/// 用户选择「不恢复」。`delete_file=true` 时顺手删掉快照文件。
#[tauri::command]
pub fn dismiss_snapshot(
    app: AppHandle,
    delete_file: bool,
    lang: Option<String>,
) -> Result<(), String> {
    let lang = parse_lang(lang);
    let path = autosave_path(&app, lang)?;
    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Ok(snap) = Snapshot::parse(&text) {
            mark_handled(&app, &snap, lang);
        }
    }
    if delete_file {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(i18n::render(
                    lang,
                    Key::SnapshotDeleteFailed,
                    &[&e.to_string()],
                ))
            }
        }
    }
    Ok(())
}

/// 恢复一份快照。默认读自动落盘文件；也可以传路径（用户显式打开的快照）。
#[tauri::command]
pub fn restore_snapshot(
    app: AppHandle,
    state: State<'_, AppState>,
    path: Option<String>,
    lang: Option<String>,
) -> Result<SnapshotDto, String> {
    let lang = parse_lang(lang);
    let target = match path.as_deref().map(str::trim) {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => autosave_path(&app, lang)?,
    };
    let snap = load_from_path(&target.to_string_lossy()).map_err(|e| e.localized(lang))?;

    // 目标列表必须能由「网段 + 当前规则」完整重建，否则下标与 IP 会对不上，
    // 这种快照宁可拒绝恢复，也不能给出一份看似正常的错位数据。
    //
    // 占位符只有 `{}` 一种，所以原来 `{:?}` 的引号由调用方先格式化好再传进去
    // ——两种语言下都保持 `"192.168.1"` 这个带引号的样子。
    let quoted = format!("{:?}", snap.prefix);
    let parsed = parse_prefix(&snap.prefix)
        .map_err(|e| i18n::render(lang, Key::SnapshotBadPrefix, &[&quoted, &e.localized(lang)]))?;
    // 快照里只存三段网段，所以这里固定用 Standard 规则重建
    let rebuilt = parse_spec(&snap.prefix, TargetRange::Standard)
        .or_else(|_| parse_spec(&parsed.to_string(), TargetRange::Standard))
        .map_err(|e| e.localized(lang))?;
    let targets = rebuilt.targets.clone();
    if targets.len() != snap.total {
        return Err(i18n::render(
            lang,
            Key::SnapshotTargetMismatch,
            &[&snap.total.to_string(), &quoted, &targets.len().to_string()],
        ));
    }
    let total = targets.len();

    let mut slots: Vec<Option<ProbeResult>> = vec![None; total];
    for r in snap.results.iter() {
        let idx = r.index as usize;
        if idx >= total {
            return Err(i18n::render(
                lang,
                Key::SnapshotIndexOutOfRange,
                &[&idx.to_string(), &total.to_string()],
            ));
        }
        slots[idx] = Some(r.clone());
    }

    let dto = {
        let mut sh = state.shared.lock().expect("state poisoned");
        sh.running = false;
        sh.cancelled = snap.cancelled;
        sh.task_id = None;
        sh.prefix = snap.prefix.clone();
        sh.scan_time = snap.scan_time.clone();
        sh.cell_slots = matrix_slots(&targets);
        sh.targets = targets;
        sh.results = slots;
        // 与实时扫描同一个统计口径，保证 total/online/elapsed 与快照时刻完全一致
        sh.stats = snap.stats();
        sh.elapsed_ms = snap.elapsed_ms;
        sh.thresholds = snap.thresholds;
        sh.cancel = None;
        // 恢复的是历史结果：变化基线清空，免得把「上次的结果」当成上一轮去比对
        sh.spec = rebuilt;
        sh.monitor.reset_baseline();
        sh.last_diff = None;
        snapshot_dto(&sh, lang)
    };

    // 恢复后立刻重新落盘：这份数据又回到内存里了，同样经不起进程被杀。
    // 同时记为「已处理」，避免下次启动重复询问同一份结果。
    mark_handled(&app, &snap, lang);
    Ok(dto)
}

// ---------------------------------------------------------------------------
// 时间工具（不引入 chrono，保持依赖面最小）
// ---------------------------------------------------------------------------

/// 本机地址标签。真实网卡枚举（M5）之前恒为「未知」。
///
/// **刻意不本地化**：它只进两个地方——导出报表的 `local_ip` 字段与快照文件，
/// 两者都是**数据契约**（同一份产物在中英文机器上必须逐字节一致）。
/// 界面上显示的本机地址走的是 `get_local_addresses` + 前端文案，与此无关。
fn local_ip_label() -> String {
    "未知".to_string()
}

fn now_string() -> String {
    let secs = unix_secs();
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

fn file_stamp() -> String {
    let secs = unix_secs();
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

fn unix_secs() -> u64 {
    cping_core::unix_secs_now()
}

/// M14：当前 Unix 毫秒（时序点时间戳）。取不到硬故障时返回 0，
/// 由核心按「时间未知」处理，而不是伪造一个看似合理的数值。
fn unix_millis() -> u64 {
    cping_core::unix_millis_now()
}

/// Howard Hinnant 的 civil_from_days。M15 起下沉到 `cping_core::timefmt`
/// （此前 CLI/TUI/GUI 各一份拷贝），此处只做转发。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    cping_core::civil_from_days(z)
}
