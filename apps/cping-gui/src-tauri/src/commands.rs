//! Tauri Commands：前端与 `cping-core` 之间的唯一桥接。
//!
//! 约束：
//! - 这里**不实现**任何目标生成 / 分类 / 统计 / 导出逻辑，只是转发与 DTO 转换；
//! - 核心类型不带 serde 派生（保持核心零依赖），因此在边界上手写 DTO 映射。

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use cping_core::classify::{classify_status, Thresholds};
use cping_core::config::{AppConfig, ProbeConfig, ViewMode};
use cping_core::export::{export, ExportFormat, ExportMeta};
use cping_core::probe::{MockBackend, MockPolicy};
use cping_core::result::{ProbeResult, ScanStats};
use cping_core::scheduler::{CancelToken, ProbeScheduler, SchedulerHandle};
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

impl AppState {
    pub fn new() -> Self {
        Self {
            // M3 先接 MockBackend；M5 替换为平台 ICMP 后端（Windows / Linux / Android）。
            scheduler: ProbeScheduler::new(MockBackend::new(MockPolicy::Realistic {
                online_percent: 62,
                base_rtt_ms: 6,
                span_ms: 140,
            })),
            shared: Arc::new(Mutex::new(Shared {
                running: false,
                cancelled: false,
                task_id: None,
                prefix: String::new(),
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
        loop {
            let Some(event) = handle.recv() else { break };

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

            let dto = to_event_dto(&event, &thresholds);
            let terminal = event.is_terminal();
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

#[tauri::command]
pub fn snapshot(state: State<'_, AppState>) -> Result<SnapshotDto, String> {
    let sh = state.shared.lock().expect("state poisoned");
    let results = sh
        .results
        .iter()
        .flatten()
        .map(|r| to_result_dto(r, &sh.thresholds))
        .collect();
    Ok(SnapshotDto {
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
    })
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
    if !path.exists() {
        return Ok(config_to_dto(&AppConfig::default()));
    }
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let cfg = AppConfig::from_toml(&text).map_err(|e| e.to_string())?;
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
