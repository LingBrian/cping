//! Tauri Commands：前端与 `cping-core` 之间的唯一桥接。
//!
//! 约束：
//! - 这里**不实现**任何目标生成 / 分类 / 统计 / 导出逻辑，只是转发与 DTO 转换；
//! - 核心类型不带 serde 派生（保持核心零依赖），因此在边界上手写 DTO 映射。

use std::path::{Path as FsPath, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use cping_core::classify::{classify_status, Thresholds};
use cping_core::config::{AppConfig, ProbeConfig, ViewMode};
use cping_core::enrich::{enrich_with_mac_and_vendor, lookup_hostname, EnrichReport};
use cping_core::export::{export, ExportFormat, ExportMeta};
use cping_core::probe::{MockBackend, MockPolicy};
use cping_core::result::{ProbeResult, ScanStats};
use cping_core::scheduler::{CancelToken, ProbeScheduler, SchedulerHandle};
use cping_core::snapshot::{load_from_path, save_to_path, Snapshot};
use cping_core::target::{build_targets, matrix_slots, parse_prefix, TargetRange};
use cping_core::ProbeEvent;

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

fn to_result_dto(r: &ProbeResult, thresholds: &Thresholds) -> ProbeResultDto {
    ProbeResultDto {
        index: r.index,
        ip: r.ip.to_string(),
        mac: r.mac.clone(),
        vendor: r.vendor.clone(),
        hostname: r.hostname.clone(),
        status_key: r.status.key().to_string(),
        status_label: r.status.label_zh().to_string(),
        rtt_ms: r.status.rtt_ms(),
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
            format!(
                "读取邻居表失败：{}",
                report.error.as_deref().unwrap_or("未知原因")
            )
        } else if report.mac_filled == 0 && report.vendor_filled == 0 {
            // 诚实说明：ARP 表只包含近期真正通信过的地址，空表是正常现象
            format!(
                "邻居表 {} 条，没有可回填的目标（ARP 表只记录近期通信过的地址）",
                report.entries
            )
        } else {
            format!(
                "邻居表 {} 条，新填 MAC {} 条、厂商 {} 条",
                report.entries, report.mac_filled, report.vendor_filled
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

fn to_event_dto(ev: &ProbeEvent, thresholds: &Thresholds) -> ProbeEventDto {
    match ev {
        ProbeEvent::Started { task_id, total } => ProbeEventDto::Started {
            task_id: task_id.clone(),
            total: *total,
        },
        ProbeEvent::Result { task_id, result } => ProbeEventDto::Result {
            task_id: task_id.clone(),
            result: to_result_dto(result, thresholds),
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
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppConfigDto {
    pub probe: ProbeConfigDto,
    pub colors: ColorConfigDto,
    pub hide_offline: bool,
    pub view: String,
}

/// 前端给的 DTO → 核心配置（唯一一处转换，含范围字符串解析）。
fn config_from_dto(dto: AppConfigDto) -> AppConfig {
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
    targets: Vec<std::net::Ipv4Addr>,
    cell_slots: Vec<Option<usize>>,
    results: Vec<Option<ProbeResult>>,
    stats: ScanStats,
    elapsed_ms: u64,
    thresholds: Thresholds,
    cancel: Option<CancelToken>,
}

pub struct AppState {
    scheduler: ProbeScheduler,
    shared: Arc<Mutex<Shared>>,
}

/// 选择探测后端：默认平台原生 ICMP；设 `CPING_MOCK=1` 时改用模拟数据（演示用）。
///
/// 之所以保留模拟后端：在没有权限或没有网络的机器上仍能演示界面，
/// 但它**必须**通过环境变量显式开启，避免把模拟结果误当成真实探测。
fn select_backend() -> Box<dyn cping_core::probe::ProbeBackend> {
    let mock = std::env::var("CPING_MOCK")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    if mock {
        Box::new(MockBackend::new(MockPolicy::Realistic {
            online_percent: 62,
            base_rtt_ms: 6,
            span_ms: 140,
        }))
    } else {
        cping_core::native::platform_icmp_backend()
    }
}

impl AppState {
    pub fn new() -> Self {
        Self {
            // M5 起默认使用平台原生 ICMP 后端（Windows 已实测；Linux/Android 见文档）。
            scheduler: ProbeScheduler::new(select_backend()),
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
pub fn parse_prefix_cmd(input: String) -> Result<String, String> {
    parse_prefix(&input)
        .map(|p| p.to_string())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn start_scan(
    app: AppHandle,
    state: State<'_, AppState>,
    prefix: String,
    timeout_ms: u64,
    concurrency: usize,
) -> Result<String, String> {
    let parsed = parse_prefix(&prefix).map_err(|e| e.to_string())?;
    let targets = build_targets(&parsed, TargetRange::Standard).map_err(|e| e.to_string())?;
    let total = targets.len();

    let thresholds = {
        let sh = state.shared.lock().expect("state poisoned");
        sh.thresholds
    };

    let config = ProbeConfig {
        prefix: Some(parsed),
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
    };

    let handle: SchedulerHandle = state
        .scheduler
        .start(&config, targets.clone())
        .map_err(|e| e.to_string())?;

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
        sh.targets = targets;
        sh.cell_slots = slots;
        sh.results = vec![None; total];
        sh.stats = ScanStats::from_results_with_total(&[], total);
        sh.elapsed_ms = 0;
        sh.cancel = Some(token);
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
                // 进程随时可能被系统回收（Android 上销毁 Activity 即退出进程），
                // 所以结果一落地就顺手写盘，不依赖退出钩子。
                let _ = autosave(&app, &sh);
            }

            let dto = to_event_dto(&event, &thresholds);
            let _ = app.emit("probe-event", dto);
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

fn snapshot_dto(sh: &Shared) -> SnapshotDto {
    let results = sh
        .results
        .iter()
        .flatten()
        .map(|r| to_result_dto(r, &sh.thresholds))
        .collect();
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
    }
}

#[tauri::command]
pub fn snapshot(state: State<'_, AppState>) -> Result<SnapshotDto, String> {
    let sh = state.shared.lock().expect("state poisoned");
    Ok(snapshot_dto(&sh))
}

fn collect_results(sh: &Shared) -> Vec<ProbeResult> {
    sh.results.iter().flatten().cloned().collect()
}

fn write_export(sh: &Shared, path: &str) -> Result<String, String> {
    let done = collect_results(sh);
    if done.is_empty() {
        return Err("尚无已完成的结果可导出".into());
    }
    let format =
        ExportFormat::from_path(path).ok_or_else(|| "仅支持 .csv / .txt 扩展名".to_string())?;
    let meta = ExportMeta {
        scan_time: now_string(),
        local_ip: local_ip_label(),
        elapsed_ms: sh.elapsed_ms,
    };
    let bytes = export(&done, format, &meta, &sh.thresholds).map_err(|e| e.to_string())?;
    std::fs::write(path, &bytes).map_err(|e| format!("写入 {path} 失败：{e}"))?;
    Ok(format!("已导出 {} 条结果 → {path}", done.len()))
}

#[tauri::command]
pub fn export_results(state: State<'_, AppState>, path: String) -> Result<String, String> {
    let sh = state.shared.lock().expect("state poisoned");
    write_export(&sh, &path)
}

/// Android 等无桌面文件对话框的场景：写到平台提供的可写目录并回报完整路径。
#[tauri::command]
pub fn export_to_app_dir(
    app: AppHandle,
    state: State<'_, AppState>,
    format: String,
) -> Result<String, String> {
    let dir: PathBuf = app
        .path()
        .document_dir()
        .or_else(|_| app.path().app_data_dir())
        .map_err(|e| format!("无法确定保存目录：{e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("无法创建目录 {}：{e}", dir.display()))?;

    let fmt = if format == "txt" {
        ExportFormat::Txt
    } else {
        ExportFormat::Csv
    };
    let path = dir.join(format!("cping-{}.{}", file_stamp(), fmt.extension()));
    let path_str = path.to_string_lossy().to_string();

    let sh = state.shared.lock().expect("state poisoned");
    write_export(&sh, &path_str)?;
    Ok(path_str)
}

fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("无法确定配置目录：{e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("无法创建配置目录：{e}"))?;
    Ok(dir.join("cping.toml"))
}

#[tauri::command]
pub fn load_config(app: AppHandle) -> Result<AppConfigDto, String> {
    let path = config_path(&app)?;
    let mut cfg = if path.exists() {
        let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        AppConfig::from_toml(&text).map_err(|e| e.to_string())?
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
) -> Result<(), String> {
    let cfg = config_from_dto(config);
    cfg.validate().map_err(|e| e.to_string())?;
    let path = config_path(&app)?;
    std::fs::write(&path, cfg.to_toml()).map_err(|e| format!("写入配置失败：{e}"))?;

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
) -> Result<EnrichReportDto, String> {
    let mut sh = state.shared.lock().expect("state poisoned");
    if sh.results.is_empty() {
        return Err("尚无扫描结果可补全".into());
    }
    // 取出 → 补全 → 按原下标写回：下标即目标的低 8 位，位置绝不能变。
    let mut done = collect_results(&sh);
    let report = enrich_with_mac_and_vendor(&mut done);
    for r in done {
        if let Some(slot) = sh.results.get_mut(r.index as usize) {
            *slot = Some(r);
        }
    }
    let autosaved_to = autosave(&app, &sh);
    Ok(EnrichReportDto::from_report(
        report,
        &sh.results,
        autosaved_to,
    ))
}

/// 按需反查单个目标的主机名（阻塞操作，放在锁外做）。
#[tauri::command]
pub fn lookup_hostname_for(
    app: AppHandle,
    state: State<'_, AppState>,
    index: u16,
    timeout_ms: u64,
) -> Result<HostnameLookupDto, String> {
    let timeout = Duration::from_millis(timeout_ms.clamp(200, 10_000));

    // 1) 锁内只取出副本，绝不在持锁状态下做 DNS 查询
    let (ip, mut probe, task_id) = {
        let sh = state.shared.lock().expect("state poisoned");
        let r = sh
            .results
            .get(index as usize)
            .and_then(|s| s.as_ref())
            .ok_or_else(|| format!("下标 {index} 尚无结果"))?;
        (r.ip, r.clone(), sh.task_id.clone())
    };

    let found = lookup_hostname(&mut probe, timeout);

    // 2) 写回前确认中间没有开始新一轮扫描，否则会把旧结果写进新任务
    let mut sh = state.shared.lock().expect("state poisoned");
    if sh.task_id != task_id {
        return Err("扫描任务已切换，本次主机名未写回".into());
    }
    if let Some(slot) = sh.results.get_mut(index as usize) {
        if let Some(cur) = slot.as_mut() {
            cur.hostname = probe.hostname.clone();
        }
    }
    autosave(&app, &sh);

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

fn app_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法确定应用数据目录：{e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("无法创建目录 {}：{e}", dir.display()))?;
    Ok(dir)
}

fn autosave_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_data_dir(app)?.join(AUTOSAVE_FILE))
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
fn mark_handled(app: &AppHandle, snap: &Snapshot) {
    if let Ok(path) = app_data_dir(app).map(|d| d.join(HANDLED_FILE)) {
        let _ = std::fs::write(path, fingerprint(snap));
    }
}

fn read_handled(app: &AppHandle) -> Option<String> {
    let path = app_data_dir(app).ok()?.join(HANDLED_FILE);
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
}

fn write_snapshot_file(path: &FsPath, snap: &Snapshot) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("无法创建目录 {}：{e}", dir.display()))?;
        }
    }
    save_to_path(snap, &path.to_string_lossy()).map_err(|e| e.to_string())
}

/// 自动落盘：无目标或结果为空时不落盘（避免用一份空快照盖掉上次的真实结果）。
fn autosave(app: &AppHandle, sh: &Shared) -> Option<String> {
    let snap = build_snapshot(sh);
    if snap.total == 0 || snap.results.is_empty() {
        return None;
    }
    let path = autosave_path(app).ok()?;
    write_snapshot_file(&path, &snap).ok()?;
    Some(path.to_string_lossy().to_string())
}

/// 供窗口失焦/关闭时调用：只在后台做一次落盘，不改动任何界面状态。
pub fn autosave_on_background(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let Ok(sh) = state.shared.lock() else { return };
    autosave(app, &sh);
}

/// 显式保存到指定路径（桌面端走文件对话框）。
#[tauri::command]
pub fn save_snapshot(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<SnapshotSavedDto, String> {
    if path.trim().is_empty() {
        return Err("未指定保存路径".into());
    }
    let sh = state.shared.lock().expect("state poisoned");
    let snap = build_snapshot(&sh);
    if snap.total == 0 {
        return Err("尚无扫描结果可保存".into());
    }
    let target = PathBuf::from(&path);
    write_snapshot_file(&target, &snap)?;
    // 显式保存过就说明结果已经落到用户指定的地方，不再提示恢复
    mark_handled(&app, &snap);
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
) -> Result<SnapshotSavedDto, String> {
    let dir: PathBuf = app
        .path()
        .document_dir()
        .or_else(|_| app.path().app_data_dir())
        .map_err(|e| format!("无法确定保存目录：{e}"))?;
    save_snapshot(
        app,
        state,
        dir.join(default_snapshot_name())
            .to_string_lossy()
            .to_string(),
    )
}

/// 启动时检查是否存在「上次没处理完」的快照；没有则返回 None。
#[tauri::command]
pub fn pending_snapshot(app: AppHandle) -> Result<Option<PendingSnapshotDto>, String> {
    let path = autosave_path(&app)?;
    if !path.exists() {
        return Ok(None);
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => return Ok(Some(PendingSnapshotDto::unreadable(&path, e.to_string()))),
    };
    match Snapshot::parse(&text) {
        Ok(snap) => {
            if read_handled(&app).as_deref() == Some(fingerprint(&snap).as_str()) {
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
        Err(e) => Ok(Some(PendingSnapshotDto::unreadable(&path, e.to_string()))),
    }
}

/// 用户选择「不恢复」。`delete_file=true` 时顺手删掉快照文件。
#[tauri::command]
pub fn dismiss_snapshot(app: AppHandle, delete_file: bool) -> Result<(), String> {
    let path = autosave_path(&app)?;
    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Ok(snap) = Snapshot::parse(&text) {
            mark_handled(&app, &snap);
        }
    }
    if delete_file {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("删除快照失败：{e}")),
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
) -> Result<SnapshotDto, String> {
    let target = match path.as_deref().map(str::trim) {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => autosave_path(&app)?,
    };
    let snap = load_from_path(&target.to_string_lossy()).map_err(|e| e.to_string())?;

    // 目标列表必须能由「网段 + 当前规则」完整重建，否则下标与 IP 会对不上，
    // 这种快照宁可拒绝恢复，也不能给出一份看似正常的错位数据。
    let parsed = parse_prefix(&snap.prefix)
        .map_err(|e| format!("快照中的网段 {:?} 无法解析：{e}", snap.prefix))?;
    let targets = build_targets(&parsed, TargetRange::Standard).map_err(|e| e.to_string())?;
    if targets.len() != snap.total {
        return Err(format!(
            "快照与当前目标规则不一致（快照记录 {} 个目标，按网段 {:?} 算出 {} 个）",
            snap.total,
            snap.prefix,
            targets.len()
        ));
    }
    let total = targets.len();

    let mut slots: Vec<Option<ProbeResult>> = vec![None; total];
    for r in snap.results.iter() {
        let idx = r.index as usize;
        if idx >= total {
            return Err(format!("快照中的下标 {idx} 超出目标范围（0..{total}）"));
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
        snapshot_dto(&sh)
    };

    // 恢复后立刻重新落盘：这份数据又回到内存里了，同样经不起进程被杀。
    // 同时记为「已处理」，避免下次启动重复询问同一份结果。
    mark_handled(&app, &snap);
    Ok(dto)
}

// ---------------------------------------------------------------------------
// 时间工具（不引入 chrono，保持依赖面最小）
// ---------------------------------------------------------------------------

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
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
