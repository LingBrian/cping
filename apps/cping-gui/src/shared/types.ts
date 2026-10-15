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

  // ---- M8 / M9 新增 ----
  /** 探测方式：`icmp` / `tcp` */
  mode: ProbeModeKey
  /** 展示文本：`ICMP` / `TCP:445` */
  modeLabel: string
  tcpPort: number | null
  /** 目标范围描述，例如 `192.168.1（255 个）` */
  specSummary: string
  segments: SegmentDto[]
  /** 是否能用 16×16 矩阵表示；false 时必须切表格视图 */
  matrixCompatible: boolean
  /** 已完成轮次（持续监控） */
  round: number
  /** 本轮变化 */
  changes: ChangeDto[]
  /** 变化日志累计条数 */
  changeTotal: number
  /** 与上一轮相比的差异摘要（无差异时为空串） */
  diffSummary: string
}

export type ProbeModeKey = 'icmp' | 'tcp'

/** 一个输入网段在目标列表中的区间（M9 多网段） */
export interface SegmentDto {
  label: string
  start: number
  count: number
}

/** 一条变化记录（M8） */
export interface ChangeDto {
  round: number
  index: number
  ip: string
  kind: ChangeKindKey
  kindLabel: string
  fromLabel: string
  toLabel: string
  summary: string
}

export type ChangeKindKey =
  | 'appeared'
  | 'disappeared'
  | 'rtt_class_changed'
  | 'status_changed'

/** 目标规格解析结果（M9） */
export interface SpecDto {
  summary: string
  total: number
  segments: SegmentDto[]
  matrixCompatible: boolean
  span: string
  first: string | null
  last: string | null
}

/**
 * 一个探测后端的对外能力描述（M12）。
 *
 * `available: false` 是事实陈述而不是错误：ICMPv6 尚未实装就如实登记为不可用，
 * 界面据此提示改用 TCP，而不是等扫完才发现清一色「不支持」。
 */
export interface BackendCapsDto {
  id: string
  label: string
  available: boolean
  ipv4: boolean
  ipv6: boolean
  needsPrivilege: boolean
  note: string
  describe: string
}

/** 导出格式（M10） */
export type ExportFormatKey = 'csv' | 'txt' | 'json' | 'html'
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
  /** M9：`icmp` / `tcp`。旧配置缺这一项时后端按 ICMP 处理 */
  mode?: ProbeModeKey
  tcpPort?: number | null
}

/** 持续监控设置（M8） */
export interface MonitorConfigDto {
  enabled: boolean
  intervalMs: number
}

export interface AppConfigDto {
  probe: ProbeConfigDto
  colors: ColorConfigDto
  hideOffline: boolean
  view: 'matrix' | 'table'
  /** M8：缺这一段时后端用默认值（关闭，30 秒） */
  monitor?: MonitorConfigDto
}

/** M8：常用重扫间隔（秒） */
export const MONITOR_INTERVALS = [5, 10, 30, 60, 300, 900] as const

/** M9：常用 TCP 端口 */
export const COMMON_TCP_PORTS = [22, 23, 80, 135, 139, 443, 445, 3389] as const

export const CHANGE_LABELS: Record<ChangeKindKey, string> = {
  appeared: '上线',
  disappeared: '离线',
  rtt_class_changed: '延迟变化',
  status_changed: '状态变化',
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

// ---------------------------------------------------------------------------
// M13 资产识别（端口指纹）
// ---------------------------------------------------------------------------

/** 一个开放端口。关闭端口不会出现在这里。 */
export interface OpenPortDto {
  port: number
  /** 已知服务名；未收录的端口为 null（界面不得自行编一个）。 */
  service: string | null
  /** 服务主动送出的横幅（只读一次，原样保存）。 */
  banner: string | null
  connectRttMs: number
}

/**
 * 一台主机的端口指纹。
 *
 * 注意：这是**附加信息**，与在线判定无关——端口全关的主机照样是「在线」。
 */
export interface FingerprintDto {
  ip: string
  scanned: number
  elapsedMs: number
  openPorts: OpenPortDto[]
  openPortsSummary: string
  /** 设备类型推断结论；识别不出为 null（界面留空，不写「未知」）。 */
  deviceKind: string | null
  deviceConfidence: string | null
  /** 恒为 true：提醒消费方这是**推断**而不是事实。 */
  deviceIsInference: boolean
  deviceSummary: string
  deviceBasis: string[]
}


// ---------------------------------------------------------------------------
// M14 历史与趋势（可用率）
// ---------------------------------------------------------------------------

/**
 * 一台主机的可用率行。
 *
 * `avgRttMs` / `worstRttMs` 为 `null` 表示**从未在线**——不是 0。
 * 界面必须显示成 `-`，写成 0 会被读成「极快」。
 */
export interface UptimeRowDto {
  ip: string
  samples: number
  online: number
  uptimePercent: number
  uptimeText: string
  avgRttMs: number | null
  avgRttText: string
  worstRttMs: number | null
  worstRttText: string
  jitterMs: number
  jitterText: string
  points: number
  /** 在线率曲线：每点 0..1；`null` = 该轮**没测到**这台主机（≠ 离线）。 */
  curve: (number | null)[]
  /** 内嵌 SVG 折线（后端纯函数产出，无脚本）。 */
  sparkline: string
}

/** 趋势页整体状态。 */
export interface TrendDto {
  points: number
  rounds: number
  hosts: number
  spanHours: number
  retentionDays: number
  rows: UptimeRowDto[]
  /** 空状态时的一句人话；有数据时为 null。 */
  emptyHint: string | null
}

// ---------------------------------------------------------------------------
// M15：告警
// ---------------------------------------------------------------------------

/**
 * 一条告警规则。
 *
 * `label` 与 `usesThreshold` 都来自核心——界面不再自己判断「这条规则用不用阈值」，
 * 否则加一类新规则就要改两处。
 */
export interface AlertRuleDto {
  kind: string
  label: string
  enabled: boolean
  /** 只有延迟 / 可用率两类用得到阈值；边沿类（新增、离线）恒为 false。 */
  usesThreshold: boolean
  threshold: number
  cooldownSecs: number
}

/** 一条告警事件。`time` 已由后端按 UTC 格式化，前端不做时区换算。 */
export interface AlertEventDto {
  kind: string
  kindLabel: string
  severity: string
  severityLabel: string
  ip: string
  atMs: number
  time: string
  round: number
  /** 触发时的实际值（延迟 ms / 可用率百分数）；边沿类规则为 null。 */
  value: number | null
  threshold: number | null
  message: string
}

/** 告警页整体状态。 */
export interface AlertDto {
  enabled: boolean
  muted: boolean
  /** 是否已建立基线（第一轮之后为 true）。 */
  primed: boolean
  activeRules: number
  /** 累计告警条数（「清空」会一并归零）。 */
  total: number
  kept: number
  suppressedByCooldown: number
  suppressedByMute: number
  rules: AlertRuleDto[]
  events: AlertEventDto[]
  latencyThresholdMs: number
  uptimeThresholdPct: number
  cooldownSecs: number
  /** 空状态时的一句人话；有数据时为 null。 */
  emptyHint: string | null
}
