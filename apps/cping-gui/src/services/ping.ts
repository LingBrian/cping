// 与 Rust 侧唯一的通道。所有业务逻辑（目标生成、调度、分类、统计、导出）
// 都在 cping-core 中，前端只转发与展示。

import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type {
  AlertDto,
  AlertEventDto,
  AppConfigDto,
  BackendCapsDto,
  ChangeDto,
  EnrichReportDto,
  ExportFormatKey,
  FingerprintDto,
  HostnameLookupDto,
  PendingSnapshotDto,
  ProbeEventDto,
  ProbeModeKey,
  SnapshotDto,
  SnapshotSavedDto,
  SpecDto,
  TrendDto,
} from '../shared/types'

export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}

export async function parsePrefix(input: string): Promise<string> {
  return invoke<string>('parse_prefix_cmd', { input })
}

/** M9：解析完整目标规格（网段 / 单主机 / CIDR / 多网段）。 */
export async function parseSpec(input: string): Promise<SpecDto> {
  return invoke<SpecDto>('parse_spec_cmd', { input })
}

/** 平台默认探测方式：桌面为 ICMP，Android 为 TCP:80。 */
export async function defaultProbeMode(): Promise<[ProbeModeKey, number]> {
  return invoke<[ProbeModeKey, number]>('default_probe_mode')
}

/** M12：当前平台全部内置后端的能力（含不可用的，前端自行过滤）。 */
export async function backendCaps(): Promise<BackendCapsDto[]> {
  return invoke<BackendCapsDto[]>('backend_caps')
}

/** M12：面向界面的一行能力摘要。 */
export async function backendSummary(): Promise<string> {
  return invoke<string>('backend_summary')
}

/** M13：对在线主机做端口指纹。`indices` 为空表示对全部在线主机做。 */
export async function fingerprintHosts(
  indices: number[] | null = null,
  ports: number[] | null = null,
  grabBanner = true,
  timeoutMs = 800,
): Promise<FingerprintDto[]> {
  return invoke<FingerprintDto[]>('fingerprint_hosts_cmd', {
    indices,
    ports,
    grabBanner,
    timeoutMs,
  })
}

/** M13：清空指纹结果。 */
export async function clearFingerprints(): Promise<void> {
  return invoke<void>('clear_fingerprints')
}

/** M13：当前指纹结果的规模：主机数 / 端口数 / 是否抓横幅。 */
export async function fingerprintState(): Promise<[number, number, boolean]> {
  return invoke<[number, number, boolean]>('fingerprint_state')
}

/** M12：本构建是否带可选异步调度（编译期决定）。 */
export async function asyncSchedulerAvailable(): Promise<boolean> {
  return invoke<boolean>('async_scheduler_available')
}

/** 矩阵视图能表示的目标数上限（256）。 */
export async function matrixViewMax(): Promise<number> {
  return invoke<number>('matrix_view_max')
}

export async function startScan(
  prefix: string,
  timeoutMs: number,
  concurrency: number,
  mode?: ProbeModeKey,
  tcpPort?: number,
): Promise<string> {
  return invoke<string>('start_scan', {
    prefix,
    timeoutMs,
    concurrency,
    mode: mode ?? null,
    tcpPort: tcpPort ?? null,
  })
}

/** M8：变化事件流（最近 limit 条，新的在前）。 */
export async function recentChanges(limit: number): Promise<ChangeDto[]> {
  return invoke<ChangeDto[]>('recent_changes', { limit })
}

/** M10：导出「上一轮 → 本轮」差异报告（.csv）。 */
export async function exportDiff(path: string): Promise<string> {
  return invoke<string>('export_diff', { path })
}

export async function cancelScan(): Promise<void> {
  await invoke('cancel_scan')
}

export async function snapshot(): Promise<SnapshotDto> {
  return invoke<SnapshotDto>('snapshot')
}

export async function exportResults(path: string): Promise<string> {
  return invoke<string>('export_results', { path })
}

/**
 * 移动端（Android）保存路径：不走桌面文件对话框，
 * 由 Rust 侧写到应用可写的公共目录并回报完整路径。
 */
export async function exportToAppDir(format: ExportFormatKey): Promise<string> {
  return invoke<string>('export_to_app_dir', { format })
}

export async function loadConfig(): Promise<AppConfigDto> {
  return invoke<AppConfigDto>('load_config')
}

export async function saveConfig(config: AppConfigDto): Promise<void> {
  await invoke('save_config', { config })
}

export async function localAddresses(): Promise<string[]> {
  return invoke<string[]>('get_local_addresses')
}

// ---------------------------------------------------------------------------
// M7：目标信息补全
// ---------------------------------------------------------------------------

/**
 * 读一次本机邻居表（ARP）回填 MAC，并据 IEEE 注册数据补厂商。
 * 可重复调用：只补空缺，不会覆盖已有值。
 */
export async function enrichResults(): Promise<EnrichReportDto> {
  return invoke<EnrichReportDto>('enrich_results')
}

/** 反查单个目标的主机名。阻塞在后端的独立线程里，最多等 timeoutMs。 */
export async function lookupHostnameFor(
  index: number,
  timeoutMs = 2000,
): Promise<HostnameLookupDto> {
  return invoke<HostnameLookupDto>('lookup_hostname_for', { index, timeoutMs })
}

// ---------------------------------------------------------------------------
// M7：结果快照
// ---------------------------------------------------------------------------

/** 启动时检查有没有上次没处理完的结果；没有则返回 null。 */
export async function pendingSnapshot(): Promise<PendingSnapshotDto | null> {
  return invoke<PendingSnapshotDto | null>('pending_snapshot')
}

/** 保存快照到指定路径（桌面端走文件对话框拿到路径）。 */
export async function saveSnapshot(path: string): Promise<SnapshotSavedDto> {
  return invoke<SnapshotSavedDto>('save_snapshot', { path })
}

/** 移动端：写到应用可写目录并回报完整路径。 */
export async function saveSnapshotToAppDir(): Promise<SnapshotSavedDto> {
  return invoke<SnapshotSavedDto>('save_snapshot_to_app_dir')
}

/** 用户选择「不恢复」。deleteFile 时顺手删掉自动落盘文件。 */
export async function dismissSnapshot(deleteFile: boolean): Promise<void> {
  await invoke('dismiss_snapshot', { deleteFile })
}

/**
 * 恢复快照。path 为空时读自动落盘的那一份。
 * 恢复是**显式动作**：界面必须在用户确认后调用，不做自动恢复。
 */
export async function restoreSnapshot(path?: string): Promise<SnapshotDto> {
  return invoke<SnapshotDto>('restore_snapshot', { path: path ?? null })
}

export function onProbeEvent(cb: (event: ProbeEventDto) => void): Promise<UnlistenFn> {
  return listen<ProbeEventDto>('probe-event', (ev) => cb(ev.payload))
}

// ---------------------------------------------------------------------------
// M14：历史与趋势（可用率）
// ---------------------------------------------------------------------------

/**
 * 取趋势页数据。`chartWidth` 是折线图的像素宽度（默认 160）。
 *
 * 曲线里的 `null` 表示该轮**没测到**这台主机，不是「离线」——
 * 界面必须画成断线，不能连起来也不能补 0。
 */
export async function trendState(chartWidth = 160): Promise<TrendDto> {
  return invoke<TrendDto>('trend_state', { chartWidth })
}

/** 清空趋势历史，返回清掉的点数。 */
export async function clearTrend(): Promise<number> {
  return invoke<number>('clear_trend')
}

/** 导出可用率报表（.csv / .html，按扩展名决定）。 */
export async function exportUptimeReport(path: string): Promise<string> {
  return invoke<string>('export_uptime_report', { path })
}

// ---------------------------------------------------------------------------
// M15：告警
// ---------------------------------------------------------------------------

/** 告警页数据。`limit` 为返回的最新事件条数上限（默认 200）。 */
export async function alertState(limit = 200): Promise<AlertDto> {
  return invoke<AlertDto>('alert_state', { limit })
}

/** 开/关告警。不重置任何状态——关掉再打开不会丢掉冷却记录与基线。 */
export async function setAlertsEnabled(enabled: boolean): Promise<AlertDto> {
  return invoke<AlertDto>('set_alerts_enabled', { enabled })
}

/** 静音开关：规则照常求值，只是不产生事件。 */
export async function setAlertMuted(muted: boolean): Promise<AlertDto> {
  return invoke<AlertDto>('set_alert_muted', { muted })
}

/**
 * 改阈值 / 冷却 / 启用哪几类。
 *
 * 只传需要改的字段；后端只覆盖显式给过的项，并保留冷却记录与基线，
 * 所以调一次阈值不会让「刚报过」的主机立刻重报一遍。
 */
export async function updateAlertRules(patch: {
  latencyMs?: number
  uptimePct?: number
  cooldownSecs?: number
  kinds?: string[]
}): Promise<AlertDto> {
  return invoke<AlertDto>('update_alert_rules', patch)
}

/** 清空告警记录（只丢事件，不动基线与冷却）。 */
export async function clearAlerts(): Promise<AlertDto> {
  return invoke<AlertDto>('clear_alerts')
}

/** 订阅告警事件：一轮扫描里触发的告警会打包成一批推过来。 */
export function onAlertEvent(cb: (events: AlertEventDto[]) => void): Promise<UnlistenFn> {
  return listen<AlertEventDto[]>('alert-event', (ev) => cb(ev.payload))
}
