// 与 Rust 侧唯一的通道。所有业务逻辑（目标生成、调度、分类、统计、导出）
// 都在 cping-core 中，前端只转发与展示。

import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { currentLang } from '../i18n'
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
  ProfileApplyDto,
  ProfileListDto,
  ScanPolicyDto,
  SnapshotDto,
  SnapshotSavedDto,
  SpecDto,
  ThemeDto,
  TrendDto,
} from '../shared/types'

export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}

/**
 * 统一的 `invoke` 包装：把**当前界面语言**随每个命令一起发给 Rust。
 *
 * 为什么要包一层，而不是在 30 多个命令里各自手写 `lang`：
 * 逐个手写必然会漏，而漏掉的表现是「就那一屏回到中文」——只有切到英文、
 * 又刚好走到那个功能才会发现，是最难靠肉眼回归的一类缺陷。
 * 包一层之后，「新命令忘了带语言」在代码形状上根本不存在。
 *
 * 多带一个字段是无害的：Rust 侧按参数名逐个取自己声明的项
 * （Tauri 的 `CommandItem` 对每个形参做一次 `payload.get(key)`），
 * 没有 `lang` 形参的命令直接忽略它。于是「后端接没接语言」与
 * 「前端带没带语言」可以各自独立推进，不必同步发版。
 *
 * 语言在**调用那一刻**取（`currentLang()` 实时读 ref），而不是在模块加载时
 * 缓存成一个常量：否则切换语言后这一整个会话都会继续用旧语言。
 */
function invokeT<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  return invoke<T>(cmd, { ...args, lang: currentLang() })
}

export async function parsePrefix(input: string): Promise<string> {
  return invokeT<string>('parse_prefix_cmd', { input })
}

/** M9：解析完整目标规格（网段 / 单主机 / CIDR / 多网段）。 */
export async function parseSpec(input: string): Promise<SpecDto> {
  return invokeT<SpecDto>('parse_spec_cmd', { input })
}

/** 平台默认探测方式：桌面为 ICMP，Android 为 TCP:80。 */
export async function defaultProbeMode(): Promise<[ProbeModeKey, number]> {
  return invokeT<[ProbeModeKey, number]>('default_probe_mode')
}

/** M12：当前平台全部内置后端的能力（含不可用的，前端自行过滤）。 */
export async function backendCaps(): Promise<BackendCapsDto[]> {
  return invokeT<BackendCapsDto[]>('backend_caps')
}

/** M12：面向界面的一行能力摘要。 */
export async function backendSummary(): Promise<string> {
  return invokeT<string>('backend_summary')
}

/** M13：对在线主机做端口指纹。`indices` 为空表示对全部在线主机做。 */
export async function fingerprintHosts(
  indices: number[] | null = null,
  ports: number[] | null = null,
  grabBanner = true,
  timeoutMs = 800,
): Promise<FingerprintDto[]> {
  return invokeT<FingerprintDto[]>('fingerprint_hosts_cmd', {
    indices,
    ports,
    grabBanner,
    timeoutMs,
  })
}

/** M13：清空指纹结果。 */
export async function clearFingerprints(): Promise<void> {
  return invokeT<void>('clear_fingerprints')
}

/** M13：当前指纹结果的规模：主机数 / 端口数 / 是否抓横幅。 */
export async function fingerprintState(): Promise<[number, number, boolean]> {
  return invokeT<[number, number, boolean]>('fingerprint_state')
}

/** M12：本构建是否带可选异步调度（编译期决定）。 */
export async function asyncSchedulerAvailable(): Promise<boolean> {
  return invokeT<boolean>('async_scheduler_available')
}

/** 矩阵视图能表示的目标数上限（256）。 */
export async function matrixViewMax(): Promise<number> {
  return invokeT<number>('matrix_view_max')
}

export async function startScan(
  prefix: string,
  timeoutMs: number,
  concurrency: number,
  mode?: ProbeModeKey,
  tcpPort?: number,
): Promise<string> {
  return invokeT<string>('start_scan', {
    prefix,
    timeoutMs,
    concurrency,
    mode: mode ?? null,
    tcpPort: tcpPort ?? null,
  })
}

/** M8：变化事件流（最近 limit 条，新的在前）。 */
export async function recentChanges(limit: number): Promise<ChangeDto[]> {
  return invokeT<ChangeDto[]>('recent_changes', { limit })
}

/** M10：导出「上一轮 → 本轮」差异报告（.csv）。 */
export async function exportDiff(path: string): Promise<string> {
  return invokeT<string>('export_diff', { path })
}

export async function cancelScan(): Promise<void> {
  await invokeT('cancel_scan')
}

export async function snapshot(): Promise<SnapshotDto> {
  return invokeT<SnapshotDto>('snapshot')
}

export async function exportResults(path: string): Promise<string> {
  return invokeT<string>('export_results', { path })
}

/**
 * 移动端（Android）保存路径：不走桌面文件对话框，
 * 由 Rust 侧写到应用可写的公共目录并回报完整路径。
 */
export async function exportToAppDir(format: ExportFormatKey): Promise<string> {
  return invokeT<string>('export_to_app_dir', { format })
}

export async function loadConfig(): Promise<AppConfigDto> {
  return invokeT<AppConfigDto>('load_config')
}

export async function saveConfig(config: AppConfigDto): Promise<void> {
  await invokeT('save_config', { config })
}

export async function localAddresses(): Promise<string[]> {
  return invokeT<string[]>('get_local_addresses')
}

// ---------------------------------------------------------------------------
// M7：目标信息补全
// ---------------------------------------------------------------------------

/**
 * 读一次本机邻居表（ARP）回填 MAC，并据 IEEE 注册数据补厂商。
 * 可重复调用：只补空缺，不会覆盖已有值。
 */
export async function enrichResults(): Promise<EnrichReportDto> {
  return invokeT<EnrichReportDto>('enrich_results')
}

/** 反查单个目标的主机名。阻塞在后端的独立线程里，最多等 timeoutMs。 */
export async function lookupHostnameFor(
  index: number,
  timeoutMs = 2000,
): Promise<HostnameLookupDto> {
  return invokeT<HostnameLookupDto>('lookup_hostname_for', { index, timeoutMs })
}

// ---------------------------------------------------------------------------
// M7：结果快照
// ---------------------------------------------------------------------------

/** 启动时检查有没有上次没处理完的结果；没有则返回 null。 */
export async function pendingSnapshot(): Promise<PendingSnapshotDto | null> {
  return invokeT<PendingSnapshotDto | null>('pending_snapshot')
}

/** 保存快照到指定路径（桌面端走文件对话框拿到路径）。 */
export async function saveSnapshot(path: string): Promise<SnapshotSavedDto> {
  return invokeT<SnapshotSavedDto>('save_snapshot', { path })
}

/** 移动端：写到应用可写目录并回报完整路径。 */
export async function saveSnapshotToAppDir(): Promise<SnapshotSavedDto> {
  return invokeT<SnapshotSavedDto>('save_snapshot_to_app_dir')
}

/** 用户选择「不恢复」。deleteFile 时顺手删掉自动落盘文件。 */
export async function dismissSnapshot(deleteFile: boolean): Promise<void> {
  await invokeT('dismiss_snapshot', { deleteFile })
}

/**
 * 恢复快照。path 为空时读自动落盘的那一份。
 * 恢复是**显式动作**：界面必须在用户确认后调用，不做自动恢复。
 */
export async function restoreSnapshot(path?: string): Promise<SnapshotDto> {
  return invokeT<SnapshotDto>('restore_snapshot', { path: path ?? null })
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
  return invokeT<TrendDto>('trend_state', { chartWidth })
}

/** 清空趋势历史，返回清掉的点数。 */
export async function clearTrend(): Promise<number> {
  return invokeT<number>('clear_trend')
}

/** 导出可用率报表（.csv / .html，按扩展名决定）。 */
export async function exportUptimeReport(path: string): Promise<string> {
  return invokeT<string>('export_uptime_report', { path })
}

// ---------------------------------------------------------------------------
// M15：告警
// ---------------------------------------------------------------------------

/** 告警页数据。`limit` 为返回的最新事件条数上限（默认 200）。 */
export async function alertState(limit = 200): Promise<AlertDto> {
  return invokeT<AlertDto>('alert_state', { limit })
}

/** 开/关告警。不重置任何状态——关掉再打开不会丢掉冷却记录与基线。 */
export async function setAlertsEnabled(enabled: boolean): Promise<AlertDto> {
  return invokeT<AlertDto>('set_alerts_enabled', { enabled })
}

/** 静音开关：规则照常求值，只是不产生事件。 */
export async function setAlertMuted(muted: boolean): Promise<AlertDto> {
  return invokeT<AlertDto>('set_alert_muted', { muted })
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
  return invokeT<AlertDto>('update_alert_rules', patch)
}

/** 清空告警记录（只丢事件，不动基线与冷却）。 */
export async function clearAlerts(): Promise<AlertDto> {
  return invokeT<AlertDto>('clear_alerts')
}

/** 订阅告警事件：一轮扫描里触发的告警会打包成一批推过来。 */
export function onAlertEvent(cb: (events: AlertEventDto[]) => void): Promise<UnlistenFn> {
  return listen<AlertEventDto[]>('alert-event', (ev) => cb(ev.payload))
}

// ---------------------------------------------------------------------------
// M16 / M17：扫描模板与安全策略
// ---------------------------------------------------------------------------

/**
 * 模板「已填好的值」：`null` = 空缺，模板可以填。
 *
 * 与 CLI 的 `seen_*`、TUI 的 `seen_target` / `seen_mode` 是同一个东西——
 * 把「用户显式给过的值」当作**数据**传给后端，于是「模板只填空缺」这条规则
 * 由 Rust 一处判定，而不是在界面里再猜一遍。
 */
export interface ProfileGaps {
  target?: string | null
  timeoutMs?: number | null
  concurrency?: number | null
  mode?: ProbeModeKey | null
  tcpPort?: number | null
}

/**
 * 列出配置文件里的扫描模板。
 *
 * **读不到配置文件不算失败**：后端返回空列表 + 一句说明（文件在哪、为什么读不到），
 * 所以这里只有真正的传输错误才会抛。
 */
export async function listProfiles(): Promise<ProfileListDto> {
  return invokeT<ProfileListDto>('list_profiles')
}

/**
 * 应用一份模板——**只填空缺**，界面上已填好的值一概不动。
 *
 * 点名不存在的模板时后端返回错误（不会悄悄退回默认模板），
 * 所以调用方必须处理失败分支。
 */
export async function applyProfile(name: string, current: ProfileGaps): Promise<ProfileApplyDto> {
  return invokeT<ProfileApplyDto>('apply_profile', {
    name,
    current: {
      target: current.target ?? null,
      timeoutMs: current.timeoutMs ?? null,
      concurrency: current.concurrency ?? null,
      mode: current.mode ?? null,
      tcpPort: current.tcpPort ?? null,
    },
  })
}

/** 读出当前生效的安全策略（含默认值与上限）。 */
export async function scanPolicy(): Promise<ScanPolicyDto> {
  return invokeT<ScanPolicyDto>('scan_policy')
}

/**
 * 改安全策略。越界的值后端会**拒绝并给出原因**（不静默夹取），
 * 所以调用方拿到的是可直接展示的错误文本。
 */
export async function setScanPolicy(
  maxPps: number,
  maxTargets: number,
  allowReserved: boolean,
): Promise<ScanPolicyDto> {
  return invokeT<ScanPolicyDto>('set_scan_policy', { maxPps, maxTargets, allowReserved })
}

// ---------------------------------------------------------------------------
// M18：无障碍（主题 / 字号）
// ---------------------------------------------------------------------------

/**
 * 取一套界面配色。`theme` 为 `null` 时用**环境默认主题**（`CPING_THEME`）。
 *
 * 传了名字却认不出来时后端**抛错**（不静默回落成默认主题），
 * 所以调用方必须处理失败分支——「以为开了无障碍、其实没开」是本轮要防的事故。
 */
export async function a11yTheme(theme: string | null = null): Promise<ThemeDto> {
  return invokeT<ThemeDto>('a11y_theme', { theme })
}

/**
 * 设置界面缩放（100% / 150% / 200%），返回生效的百分比。
 *
 * 缩放作用于**整个 webview**（连控件一起放大，不是只放大文字）。
 * 平台不支持时（Android 明确不支持）后端抛错，界面必须把原因显示出来——
 * 绝不静默什么都不做。
 */
export async function setFontScale(scale: string): Promise<number> {
  return invokeT<number>('set_font_scale', { scale })
}
