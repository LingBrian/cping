// 与 Rust 侧 `commands.rs` 的 DTO 一一对应。
// 前端**不定义**任何分类/统计口径，只消费后端给出的字段。

export type ProbeStatusKey =
  | 'pending'
  | 'online'
  | 'timeout'
  | 'unreachable'
  | 'permission_denied'
  | 'unsupported'
  | 'error'

export type ResultClassKey = 'low' | 'mid' | 'high' | 'offline' | 'error'

export interface ProbeResultDto {
  index: number
  ip: string
  mac: string | null
  /** M7：由 MAC 前缀查 IEEE 注册数据得到的厂商名；查不到就是 null，不填占位 */
  vendor: string | null
  /** M7：按需 DNS 反查得到的主机名；查不到就是 null */
  hostname: string | null
  statusKey: ProbeStatusKey
  statusLabel: string
  rttMs: number | null
  note: string | null
  classKey: ResultClassKey | null
}

export interface ScanStatsDto {
  total: number
  completed: number
  pending: number
  online: number
  timeout: number
  unreachable: number
  permissionDenied: number
  unsupported: number
  error: number
}

export interface SnapshotDto {
  taskId: string | null
  running: boolean
  cancelled: boolean
  prefix: string
  total: number
  elapsedMs: number
  stats: ScanStatsDto
  results: ProbeResultDto[]
  /** 长度 256，值 = targets 下标；格号 0 通常为 null（.0 不参与探测） */
  cellSlots: (number | null)[]
  targets: string[]
}

export interface ColorConfigDto {
  low: string
  mid: string
  high: string
  offline: string
  error: string
}

export interface ProbeConfigDto {
  prefix: string
  range: 'standard' | 'full' | 'hosts_only'
  timeoutMs: number
  concurrency: number
  lowMaxMs: number
  midMaxMs: number
}

export interface AppConfigDto {
  probe: ProbeConfigDto
  colors: ColorConfigDto
  hideOffline: boolean
  view: 'matrix' | 'table'
}

export type ProbeEventDto =
  | { kind: 'started'; taskId: string; total: number }
  | { kind: 'result'; taskId: string; result: ProbeResultDto }
  | { kind: 'progress'; taskId: string; completed: number; total: number }
  | { kind: 'finished'; taskId: string; stats: ScanStatsDto; elapsedMs: number }
  | { kind: 'cancelled'; taskId: string; stats: ScanStatsDto; elapsedMs: number }

// ---------------------------------------------------------------------------
// M7：目标信息补全 与 结果快照
// ---------------------------------------------------------------------------

/** 一次「读邻居表回填 MAC / 厂商」的结果 */
export interface EnrichReportDto {
  tableOk: boolean
  error: string | null
  entries: number
  macFilled: number
  vendorFilled: number
  withMac: number
  withVendor: number
  withHostname: number
  /** 后端给出的一句话说明，直接展示即可 */
  message: string
  autosavedTo: string | null
}

/** 单条主机名反查结果 */
export interface HostnameLookupDto {
  index: number
  ip: string
  hostname: string | null
  found: boolean
}

/** 启动时发现的「上次未处理的结果」 */
export interface PendingSnapshotDto {
  path: string
  ok: boolean
  error: string | null
  scanTime: string
  prefix: string
  total: number
  rows: number
  elapsedMs: number
  cancelled: boolean
}

export interface SnapshotSavedDto {
  path: string
  rows: number
  total: number
}

export const MATRIX_COLS = 16
export const MATRIX_ROWS = 16
export const MATRIX_CELLS = MATRIX_COLS * MATRIX_ROWS

export const DEFAULT_COLORS: ColorConfigDto = {
  low: '#00FF00',
  mid: '#FFFF00',
  high: '#99B4D1',
  offline: '#C0C0C0',
  error: '#FF6B6B',
}

export const CLASS_LABELS: Record<ResultClassKey, string> = {
  low: '低延迟',
  mid: '中延迟',
  high: '高延迟',
  offline: '无回应',
  error: '错误',
}

/** 无颜色时也必须可区分：每个类别有自己的符号 */
export const CLASS_SYMBOLS: Record<ResultClassKey, string> = {
  low: '#',
  mid: '+',
  high: '*',
  offline: '-',
  error: '!',
}
