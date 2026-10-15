import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import type { UnlistenFn } from '@tauri-apps/api/event'
import {
  CLASS_LABEL_KEYS,
  DEFAULT_COLORS,
  MATRIX_CELLS,
  MONITOR_INTERVALS,
  PROFILE_FIELD_LABEL_KEYS,
  THEME_LABEL_KEYS,
  type AlertDto,
  type AlertEventDto,
  type AppConfigDto,
  type BackendCapsDto,
  type ChangeDto,
  type ColorConfigDto,
  type ExportFormatKey,
  type FingerprintDto,
  type FontScaleKey,
  type PendingSnapshotDto,
  type ProbeModeKey,
  type ProbeResultDto,
  type ProfileApplyDto,
  type ProfileDto,
  type ProfileFieldDto,
  type ResultClassKey,
  type ScanPolicyDto,
  type ScanStatsDto,
  type SegmentDto,
  type SnapshotDto,
  type ThemeDto,
  type ThemeKey,
  type TrendDto,
} from '../shared/types'
import * as svc from '../services/ping'
import { t } from '../i18n'

const EMPTY_STATS: ScanStatsDto = {
  total: 0,
  completed: 0,
  pending: 0,
  online: 0,
  timeout: 0,
  unreachable: 0,
  permissionDenied: 0,
  unsupported: 0,
  error: 0,
}

/** 变化停留时长（毫秒）：这段时间内对应行/格保持高亮。 */
export const CHANGE_HIGHLIGHT_MS = 8000

export const usePingStore = defineStore('ping', () => {
  const prefix = ref('192.168.1')
  const timeoutMs = ref(1000)
  const concurrency = ref(32)

  const running = ref(false)
  const cancelled = ref(false)
  const taskId = ref<string | null>(null)
  const elapsedMs = ref(0)
  const stats = ref<ScanStatsDto>({ ...EMPTY_STATS })
  const results = ref<(ProbeResultDto | null)[]>([])
  const cellSlots = ref<(number | null)[]>(new Array(MATRIX_CELLS).fill(null))
  const targets = ref<string[]>([])

  const view = ref<'matrix' | 'table'>('matrix')
  const hideOffline = ref(false)
  const colors = ref<ColorConfigDto>({ ...DEFAULT_COLORS })
  // 初始文案在 store 创建时取一次词条。注意：`status` 里的内容是「事件日志」——
  // 记录事件发生那一刻的语言，切换语言后不会回溯改写（它本身就是一句历史记录，
  // 而不是需要跟着界面重排的标签）。界面标签一律走模板里的 `t()`，是响应式的。
  const localIp = ref(t('common.unknown'))
  const status = ref(t('status.ready'))
  const lastExport = ref<string | null>(null)

  // ---- M7 ----
  /** 启动时发现的「上次未处理的结果」；非 null 时界面必须显示确认框 */
  const pendingRestore = ref<PendingSnapshotDto | null>(null)
  /** 最近一次补全的说明（来自后端） */
  const enrichNote = ref('')
  /** 最近一次保存的快照路径 */
  const lastSnapshot = ref<string | null>(null)
  /** 正在进行主机名反查的目标下标（用于按钮 loading） */
  const hostnameBusy = ref<number | null>(null)

  // ---- M9：目标规格与探测方式 ----
  /** 探测方式。一次任务只有一种方式，所以在线/开放永远不混计。 */
  const mode = ref<ProbeModeKey>('icmp')
  /** TCP 方式下的端口（非 TCP 方式时保留上次选择，切回来不用重填） */
  const tcpPort = ref(80)
  /** 各输入网段在 targets 中的区间（分段显示用） */
  const segments = ref<SegmentDto[]>([])
  /** 后端给的目标范围描述，例如 `192.168.1（255 个）` */
  const specSummary = ref('')
  /** 是否能用 16×16 矩阵表示；false 时必须切表格视图 */
  const matrixCompatible = ref(true)
  /** M9 目标规格的实时校验提示（空串表示没问题） */
  const specError = ref('')
  /** M12：平台后端能力（含不可用的）；「设置」里如实展示 */
  const backendCaps = ref<BackendCapsDto[]>([])
  /** M12：一行能力摘要 */
  const backendSummary = ref('')
  /** M12：本构建是否带可选异步调度（编译期决定） */
  const asyncEnabled = ref(false)

  // ---- M13：资产识别（端口指纹）----
  /**
   * 端口指纹结果。**与 results 分开放**：端口扫描是独立的一趟探测，
   * 混进结果里会让「在线」出现第二个来源。
   */
  const fingerprints = ref<FingerprintDto[]>([])
  /** 指纹面板是否展开 */
  const showFingerprints = ref(false)
  /** 正在进行指纹探测（按钮 loading） */
  const fingerprintBusy = ref(false)
  /** 本次指纹探测的端口数（说明文字用） */
  const fingerprintPorts = ref(0)
  /** 是否抓取服务横幅 */
  const fingerprintBanner = ref(true)

  // ---- M8：持续监控与变化 ----
  /** 已完成轮次 */
  const round = ref(0)
  /** 本轮变化（后端在终态时算好的） */
  const changes = ref<ChangeDto[]>([])
  /** 变化日志累计条数 */
  const changeTotal = ref(0)
  /** 与上一轮相比的一句话差异摘要（无差异时为空串） */
  const diffSummary = ref('')
  /** 持续监控开关 */
  const monitorEnabled = ref(false)
  /** 重扫间隔（秒） */
  const monitorInterval = ref(30)
  /** 变化面板是否展开 */
  const showChanges = ref(false)
  /** 变化流水（新的在前），来自后端变化日志 */
  const changeFeed = ref<ChangeDto[]>([])
  /** 变化高亮的过期时刻（毫秒时间戳），到点后自动淡出 */
  const highlightUntil = ref(0)
  /** 距离下一轮的剩余秒数（监控关闭或正在扫描时为 null） */
  const countdown = ref<number | null>(null)

  let unlisten: UnlistenFn | null = null
  let unlistenAlerts: UnlistenFn | null = null
  let highlightTimer: ReturnType<typeof setTimeout> | null = null
  let monitorTimer: ReturnType<typeof setInterval> | null = null
  /** 下一轮的计划发起时刻（毫秒）。0 表示尚未排期。 */
  let nextRoundAt = 0

  const progress = computed(() =>
    stats.value.total === 0 ? 0 : stats.value.completed / stats.value.total,
  )

  /** 表格显示的行（恒按 IP 升序，即目标顺序） */
  const visibleRows = computed(() => {
    const out: { index: number; ip: string; result: ProbeResultDto | null }[] = []
    for (let i = 0; i < targets.value.length; i += 1) {
      const result = results.value[i] ?? null
      if (hideOffline.value && result?.statusKey !== 'online') continue
      out.push({ index: i, ip: targets.value[i], result })
    }
    return out
  })

  const onlineCount = computed(() => stats.value.online)

  /** M9：探测方式展示文本。直接拼给界面，后端也有一份（快照里的 modeLabel）。 */
  const modeLabel = computed(() => (mode.value === 'tcp' ? `TCP:${tcpPort.value}` : 'ICMP'))

  /** M8：本轮有变化的目标下标 → 变化条目。用于行/格高亮与行内标记。 */
  const changeMap = computed(() => {
    const map = new Map<number, ChangeDto>()
    for (const c of changes.value) map.set(c.index, c)
    return map
  })

  /** M8：高亮中的目标下标集合；过期后为空集。 */
  const changedIndexes = computed(() => {
    if (Date.now() >= highlightUntil.value) return new Set<number>()
    return new Set(changes.value.map((c) => c.index))
  })

  /** M8：倒计时秒数（仅监控开启且空闲时给值，否则为 null）。 */
  const secondsToNextRound = computed(() => countdown.value)

  /** M9：矩阵不可用时界面必须切表格，这里给出「矩阵是否可用」。 */
  const canUseMatrix = computed(() => matrixCompatible.value)

  /**
   * M12：后端能力明细（一行一个），给能力提示做悬停说明。
   * `backendCaps` 是核心给的事实数据，这里只负责展示拼装，不做任何推断。
   * 注意 `c.label` / `c.describe` 是**后端产出的原文**，按约定原样透传，不在此翻译。
   */
  const backendCapDetails = computed(() =>
    backendCaps.value
      .map((c) =>
        t(
          'status.capDetail',
          c.label,
          c.id,
          c.available ? t('common.available') : t('common.unavailable'),
          c.describe,
        ),
      )
      .join('\n'),
  )

  /** M7：认出厂商 / 主机名 / MAC 的条数。直接数结果，不另存一份状态。 */
  const recognized = computed(() => {
    let mac = 0
    let vendor = 0
    let hostname = 0
    for (const r of results.value) {
      if (!r) continue
      if (r.mac) mac += 1
      if (r.vendor) vendor += 1
      if (r.hostname) hostname += 1
    }
    return { mac, vendor, hostname }
  })

  function colorOf(key: ResultClassKey | null): string {
    if (!key) return 'transparent'
    return colors.value[key]
  }

  function labelOf(key: ResultClassKey | null): string {
    return key ? t(CLASS_LABEL_KEYS[key]) : t('class.waiting')
  }

  /** 某一行的变化说明（没有变化则返回空串）。 */
  function changeNoteOf(index: number): string {
    const c = changeMap.value.get(index)
    return c ? c.summary : ''
  }

  function isChanged(index: number): boolean {
    return changedIndexes.value.has(index)
  }

  /** 变化高亮启动：`CHANGE_HIGHLIGHT_MS` 之后自动淡出，不依赖用户操作。 */
  function armHighlight() {
    if (changes.value.length === 0) {
      highlightUntil.value = 0
      return
    }
    highlightUntil.value = Date.now() + CHANGE_HIGHLIGHT_MS
    if (highlightTimer) clearTimeout(highlightTimer)
    highlightTimer = setTimeout(() => {
      highlightUntil.value = 0
      highlightTimer = null
    }, CHANGE_HIGHLIGHT_MS + 50)
  }

  async function attachEvents() {
    if (!unlistenAlerts) {
      // M15：告警走一条独立事件。这里只负责「让界面知道有新告警」——
      // 判定、冷却、静音全在后端算好，前端不做二次解释。
      unlistenAlerts = await svc.onAlertEvent((events) => {
        if (events.length === 0) return
        // 这里只负责「有新告警」这一层信号；面板数据统一由 `resyncFromBackend`
        // 拉一次权威快照，避免同一轮里两条路径各写一份状态。
        alertFeed.value = events
      })
    }
    if (unlisten) return
    unlisten = await svc.onProbeEvent((event) => {
      if (taskId.value && 'taskId' in event && event.taskId !== taskId.value) {
        // 旧任务的事件一律丢弃，避免串到新任务界面
        return
      }
      switch (event.kind) {
        case 'started':
          stats.value.total = event.total
          break
        case 'result':
          if (event.result.index < results.value.length) {
            results.value[event.result.index] = event.result
          }
          break
        case 'progress':
          if (stats.value.completed < event.completed) {
            stats.value.completed = event.completed
            stats.value.pending = Math.max(0, stats.value.total - event.completed)
          }
          break
        case 'finished':
          stats.value = event.stats
          elapsedMs.value = event.elapsedMs
          running.value = false
          // 终态时从后端做一次权威校准：扫描很快时（如回环仅几十毫秒）
          // 事件会瞬时涌入，逐条事件可能来不及全部落到数组上。
          // 后端在发出终态事件前已经补过一遍 MAC/厂商，所以校准回来就带上了。
          void resyncFromBackend(true)
          break
        case 'cancelled':
          stats.value = event.stats
          elapsedMs.value = event.elapsedMs
          running.value = false
          cancelled.value = true
          void resyncFromBackend(true)
          break
      }
    })
  }

  /**
   * 全量刷新。**只在扫描开始前/结束后调用**。
   * 扫描进行中不要调用：它会整体覆盖 `results`，与事件流写入竞争。
   */
  async function refreshSnapshot() {
    applySnapshot(await svc.snapshot())
  }

  /** 把后端给的一份快照整体应用到界面状态（刷新与「恢复」共用同一条路径）。 */
  function applySnapshot(snap: SnapshotDto) {
    taskId.value = snap.taskId
    running.value = snap.running
    cancelled.value = snap.cancelled
    elapsedMs.value = snap.elapsedMs
    stats.value = snap.stats
    cellSlots.value = snap.cellSlots
    targets.value = snap.targets
    const list: (ProbeResultDto | null)[] = new Array(snap.targets.length).fill(null)
    for (const r of snap.results) list[r.index] = r
    results.value = list

    // ---- M8 / M9 ----
    // 方式以快照为准：后端才是唯一权威（并发任务、恢复快照都会改它）。
    mode.value = snap.mode
    if (snap.tcpPort != null) tcpPort.value = snap.tcpPort
    specSummary.value = snap.specSummary
    segments.value = snap.segments
    matrixCompatible.value = snap.matrixCompatible
    round.value = snap.round
    changes.value = snap.changes
    changeTotal.value = snap.changeTotal
    diffSummary.value = snap.diffSummary
    if (!snap.matrixCompatible && view.value === 'matrix') view.value = 'table'
    armHighlight()
  }

  /**
   * 终态校准：后端持有权威结果，这里整体对齐一次。
   * 之所以需要它：真实探测很快时（回环几十毫秒），255 个 Result 事件几乎同时到达，
   * 逐条写入可能来不及全部反映，只有从后端拉一次才能保证与核心统计一致。
   *
   * `withInfo=true` 时额外把「认出了多少台」写进状态文案——补全是在后端
   * 发终态事件之前做的，所以这次校准拉到的数据里已经带上了 MAC/厂商。
   */
  async function resyncFromBackend(withInfo = false) {
    try {
      await refreshSnapshot()
      // 变化流水要在快照之后再拉：后端是在终态处理里写的变化日志。
      await loadChanges(200)
      // M14：本轮也已并入后端时序。只有「趋势已经被看过」时才顺手刷新，
      // 免得从没打开过趋势页的用户每轮都白白多一次 IPC。
      if (trend.value !== null) await loadTrend()
      // M15：同上——只有「告警页已经被看过」时才顺手刷新。
      if (alerts.value !== null) await loadAlerts()
      if (withInfo) {
        const n = recognized.value
        const base = cancelled.value
          ? t('status.cancelledDone', stats.value.completed, stats.value.total)
          : t('status.scanDone', stats.value.total, stats.value.online)
        const tail = diffSummary.value ? t('status.diffSuffix', diffSummary.value) : ''
        status.value = t('status.scanDoneInfo', base, n.mac, n.vendor, tail)
      }
    } catch (e) {
      status.value = t('status.resyncFailed', String(e))
    }
  }

  async function start() {
    status.value = t('status.starting')
    try {
      // 方式随请求一起发出去：一次任务只有一种方式，后端据此新建调度器。
      const id = await svc.startScan(
        prefix.value,
        timeoutMs.value,
        concurrency.value,
        mode.value,
        tcpPort.value,
      )
      taskId.value = id
      cancelled.value = false
      results.value = []
      cellSlots.value = new Array(MATRIX_CELLS).fill(null)
      targets.value = []
      stats.value = { ...EMPTY_STATS }
      elapsedMs.value = 0
      lastExport.value = null
      await refreshSnapshot()
      running.value = true
      status.value = t('status.scanStarted', specSummary.value || prefix.value, modeLabel.value)
    } catch (e) {
      status.value = t('status.startFailed', String(e))
    }
  }

  async function cancel() {
    try {
      await svc.cancelScan()
      await refreshSnapshot()
      status.value = t('status.cancelRequested')
    } catch (e) {
      status.value = t('status.cancelFailed', String(e))
    }
  }

  // -------------------------------------------------------------------------
  // M9：目标规格与探测方式
  // -------------------------------------------------------------------------

  /** 交给 Rust 核心校验目标规格：前端不做第二套解析规则。 */
  async function validateSpec(input: string): Promise<boolean> {
    specError.value = ''
    try {
      const spec = await svc.parseSpec(input)
      specSummary.value = spec.summary
      segments.value = spec.segments
      matrixCompatible.value = spec.matrixCompatible
      if (!spec.matrixCompatible && view.value === 'matrix') view.value = 'table'
      return true
    } catch (e) {
      specError.value = String(e)
      specSummary.value = ''
      return false
    }
  }

  /** 平台默认方式：桌面 ICMP，Android TCP:80。只在首次启动时调用。 */
  async function loadDefaultMode() {
    try {
      const [key, port] = await svc.defaultProbeMode()
      mode.value = key
      if (port > 0) tcpPort.value = port
    } catch {
      // 拿不到就用 ICMP，不因为一个提示性调用失败而卡住界面
    }
  }

  /**
   * M12：拉一次后端能力。失败不影响使用——它只用于「设置」里的说明文字，
   * 探测本身完全不需要它。
   */
  async function loadBackendCaps() {
    try {
      backendCaps.value = await svc.backendCaps()
      backendSummary.value = await svc.backendSummary()
      asyncEnabled.value = await svc.asyncSchedulerAvailable()
    } catch {
      backendCaps.value = []
      backendSummary.value = ''
    }
  }

  function setMode(key: ProbeModeKey) {
    if (mode.value === key) return
    mode.value = key
    // M16：这是**用户**改的方式，模板从此不再覆盖它（「只填空缺」）
    markEdited('mode')
    status.value = t('status.modeChanged', modeLabel.value)
  }

  // -------------------------------------------------------------------------
  // M13：资产识别（端口指纹）
  // -------------------------------------------------------------------------

  /** 已识别出设备类型的主机数（角标用）。 */
  const fingerprintedHosts = computed(() => fingerprints.value.length)
  const fingerprintOpenPorts = computed(() =>
    fingerprints.value.reduce((n, f) => n + f.openPorts.length, 0),
  )
  const fingerprintWithOpenPorts = computed(
    () => fingerprints.value.filter((f) => f.openPorts.length > 0).length,
  )

  /**
   * 对在线主机做端口指纹。
   *
   * `indices` 为空 = 对全部在线主机做。**不动 results**：端口全关的主机
   * 仍然是「在线」，这条边界不能被这次调用破坏。
   */
  async function runFingerprint(indices: number[] | null = null): Promise<string | null> {
    if (fingerprintBusy.value) {
      status.value = t('status.fpBusy')
      return null
    }
    fingerprintBusy.value = true
    try {
      const list = await svc.fingerprintHosts(indices, null, fingerprintBanner.value)
      fingerprints.value = list
      fingerprintPorts.value = list.length > 0 ? list[0].scanned : 0
      showFingerprints.value = true
      const openTotal = list.reduce((n, f) => n + f.openPorts.length, 0)
      const withOpen = list.filter((f) => f.openPorts.length > 0).length
      const msg = t('status.fpDone', list.length, openTotal, withOpen)
      status.value = msg
      return msg
    } catch (e) {
      status.value = t('status.fpFailed', String(e))
      return null
    } finally {
      fingerprintBusy.value = false
    }
  }

  /** 清空指纹结果（导出里随之不再出现指纹小节）。 */
  async function dropFingerprints(): Promise<void> {
    try {
      await svc.clearFingerprints()
    } catch {
      // 清空失败不影响界面状态：本地也清掉，避免显示与后端不一致
    }
    fingerprints.value = []
    fingerprintPorts.value = 0
    showFingerprints.value = false
    status.value = t('status.fpCleared')
  }

  /** 指纹面板里的一行行文本（与界面同源，便于测试）。 */
  const fingerprintLines = computed<string[]>(() => {
    const lines: string[] = [t('fp.notePlain')]
    if (fingerprints.value.length === 0) {
      lines.push(t('fp.emptyLine'))
      return lines
    }
    for (const f of fingerprints.value) {
      if (f.openPorts.length === 0) {
        lines.push(t('fp.noOpenPortsLine', f.ip))
        continue
      }
      // 这两行是「IP + 端口摘要 / 设备摘要」，摘要文本来自后端，原样透传
      lines.push(`${f.ip}  ${f.openPortsSummary}`)
      for (const p of f.openPorts) {
        if (p.banner) lines.push(`    :${p.port} ${p.banner}`)
      }
      if (f.deviceSummary) lines.push(`    ${f.deviceSummary}`)
    }
    return lines
  })

  // -------------------------------------------------------------------------
  // M14：历史与趋势（可用率）
  // -------------------------------------------------------------------------

  /**
   * 趋势页数据。**不做轮询**：只在切到「趋势」页签、以及每轮扫描结束时拉一次。
   * 时序本身在后端累积（跨轮次、跨会话），前端只是看一眼当前的账。
   */
  const trend = ref<TrendDto | null>(null)
  const trendBusy = ref(false)

  /** 拉一次趋势。`chartWidth` 由页面按容器宽度给定。 */
  async function loadTrend(chartWidth = 160): Promise<void> {
    trendBusy.value = true
    try {
      trend.value = await svc.trendState(chartWidth)
    } catch (e) {
      status.value = t('status.trendFailed', String(e))
    } finally {
      trendBusy.value = false
    }
  }

  /** 清空趋势历史。回报清掉了多少个时间点，别让用户以为「没反应」。 */
  async function dropTrend(): Promise<void> {
    try {
      const removed = await svc.clearTrend()
      trend.value = null
      status.value = t('status.trendCleared', removed)
    } catch (e) {
      status.value = t('status.trendClearFailed', String(e))
    }
  }

  /** 导出可用率报表（.csv / .html）。 */
  async function exportTrend(path: string): Promise<string | null> {
    try {
      const msg = await svc.exportUptimeReport(path)
      status.value = msg
      return msg
    } catch (e) {
      status.value = t('status.trendExportFailed', String(e))
      return null
    }
  }

  /** 趋势页里出现过的在线主机数（角标用）。 */
  const trendHostCount = computed(() => trend.value?.rows.length ?? 0)

  // -------------------------------------------------------------------------
  // M15：告警
  // -------------------------------------------------------------------------

  /**
   * 告警状态。与趋势一样**不做轮询**：切到「告警」页签时拉一次，
   * 每轮扫描结束后（若已经看过告警页）再拉一次。真正的求值在后端。
   */
  const alerts = ref<AlertDto | null>(null)
  const alertsBusy = ref(false)
  /** 最近一批推送过来的告警（供布局层就地提示）；空数组表示还没有。 */
  const alertFeed = ref<AlertEventDto[]>([])

  async function loadAlerts(): Promise<void> {
    alertsBusy.value = true
    try {
      alerts.value = await svc.alertState()
    } catch (e) {
      status.value = t('status.alertsFailed', String(e))
    } finally {
      alertsBusy.value = false
    }
  }

  /**
   * 开/关告警。
   *
   * 首次打开时后端**不重建引擎**：规则、冷却记录都在，所以「关掉再打开」
   * 不会把基线丢掉，也不会在下一轮把整段在线主机当成「新增」重报一遍。
   */
  async function setAlertsEnabled(enabled: boolean): Promise<void> {
    try {
      alerts.value = await svc.setAlertsEnabled(enabled)
      status.value = enabled ? t('status.alertsOn') : t('status.alertsOff')
    } catch (e) {
      status.value = t('status.alertToggleFailed', String(e))
    }
  }

  /** 静音开关。静音不改判定，因此解除静音不会补报积压告警。 */
  async function setAlertMuted(muted: boolean): Promise<void> {
    try {
      alerts.value = await svc.setAlertMuted(muted)
      status.value = muted ? t('status.mutedOn') : t('status.mutedOff')
    } catch (e) {
      status.value = t('status.muteToggleFailed', String(e))
    }
  }

  /** 改阈值 / 冷却 / 启用哪几类。后端会校验并给出原因。 */
  async function updateAlertRules(patch: {
    latencyMs?: number
    uptimePct?: number
    cooldownSecs?: number
    kinds?: string[]
  }): Promise<boolean> {
    try {
      alerts.value = await svc.updateAlertRules(patch)
      status.value = t('status.alertRulesUpdated')
      return true
    } catch (e) {
      // 后端给的是可直接展示的原因原文（可能是中文），不要再包一层
      status.value = String(e)
      return false
    }
  }

  /** 清空记录（只丢事件，不动基线与冷却）。 */
  async function dropAlerts(): Promise<void> {
    try {
      alerts.value = await svc.clearAlerts()
      status.value = t('status.alertsCleared')
    } catch (e) {
      status.value = t('status.alertsClearFailed', String(e))
    }
  }

  /** 未读视角的告警摘要：给状态栏/角标用；没有告警时为空串（不写「0 条」这种噪音）。 */
  const alertNote = computed(() => {
    const a = alerts.value
    if (!a || a.total === 0) return ''
    return a.muted ? t('alert.summaryMuted', a.total) : t('alert.summary', a.total)
  })

  const alertCount = computed(() => alerts.value?.total ?? 0)

  // -------------------------------------------------------------------------
  // M16：扫描模板（配置文件里的 [[profiles]]）
  // -------------------------------------------------------------------------

  /**
   * 配置文件里的模板列表、路径，以及「为什么没有模板」的说明。
   *
   * `profilesNotes` 存在的理由：空列表是最需要解释的状态——文件不在 / 读不动 /
   * 里面没有 `[[profiles]]`，三种情况的处理方式完全不同。
   * 后端把原因一起给过来了（读不到文件**不算失败**），界面照原样展示即可。
   */
  const profiles = ref<ProfileDto[]>([])
  const profilesPath = ref('')
  const profilesNotes = ref<string[]>([])
  /** 是否已经拉过一次（切语言时据此决定要不要重拉，与趋势/告警同一条规矩） */
  const profilesLoaded = ref(false)
  const profileBusy = ref(false)
  /** 最近一次成功应用的模板名（界面据此打「当前」标记） */
  const activeProfile = ref<string | null>(null)
  /** 最近一次应用模板的完整结果：界面用它说清「哪个字段来自模板、哪个没动」 */
  const lastProfileApply = ref<ProfileApplyDto | null>(null)

  /**
   * 界面上「**用户亲手填过**」的字段——M16 的「模板只填空缺」就靠它。
   *
   * 为什么不去看「值是否等于默认值」：默认值（1000 ms / 32 并发）本身就是一个
   * 用户可能真心想要的取值，拿它当「没填过」等于把用户的选择吃掉。
   * 只有真实交互（输入、切换）才置位；**模板自己填上的值不置位**——
   * 这样「被模板填过的空缺」对下一份模板仍然是空缺，用户可以换个模板再试。
   */
  const edited = ref({ target: false, timeout: false, concurrency: false, mode: false })

  function markEdited(field: 'target' | 'timeout' | 'concurrency' | 'mode'): void {
    edited.value = { ...edited.value, [field]: true }
  }

  /** 界面输入目标的唯一入口：写入 + 记账（「用户填过」）。 */
  function setTargetSpec(value: string): void {
    prefix.value = value
    markEdited('target')
  }

  function setTimeoutMs(value: number): void {
    if (!Number.isFinite(value) || value <= 0) return
    timeoutMs.value = Math.floor(value)
    markEdited('timeout')
  }

  function setConcurrency(value: number): void {
    if (!Number.isFinite(value) || value <= 0) return
    concurrency.value = Math.floor(value)
    markEdited('concurrency')
  }

  /**
   * 把「哪些字段、值是多少」折成一句人话。
   *
   * 标签查界面字典（`PROFILE_FIELD_LABEL_KEYS`），而 `field` 机器码由后端给——
   * 后端拼文本就翻不了，前端硬编码字段名就会与后端漂移，所以两边各管一半。
   */
  function fieldSummary(list: ProfileFieldDto[]): string {
    return list
      .map((f) => t(PROFILE_FIELD_LABEL_KEYS[f.field] ?? 'field.unknown', f.value))
      .join(' / ')
  }

  /** 拉一次模板列表。**读不到配置文件不算失败**，后端返回空列表 + 说明。 */
  async function loadProfiles(): Promise<void> {
    try {
      const list = await svc.listProfiles()
      profiles.value = list.profiles
      profilesPath.value = list.path
      profilesNotes.value = list.notes
    } catch (e) {
      // 只有真正的传输错误会走到这里（文件层面的问题后端已当正常情况处理）
      profiles.value = []
      profilesNotes.value = []
      status.value = t('status.profilesFailed', String(e))
    } finally {
      profilesLoaded.value = true
    }
  }

  /**
   * 应用一份模板。
   *
   * **只填空缺**：界面上已经填好的项以非 `null` 传给后端，后端一个字都不会动它们；
   * 返回的 `filled` / `skipped` 两条清单交给界面把「来源」说清楚。
   * 「模板悄悄换掉了用户输入」是本项目最不能接受的一类行为，所以这里宁可多显示一行字。
   */
  async function applyProfileByName(name: string): Promise<boolean> {
    if (profileBusy.value) return false
    profileBusy.value = true
    try {
      const r = await svc.applyProfile(name, {
        target: edited.value.target ? prefix.value : null,
        timeoutMs: edited.value.timeout ? timeoutMs.value : null,
        concurrency: edited.value.concurrency ? concurrency.value : null,
        // 方式连同端口一起报：TCP 下端口不同就是不同的探测目标
        mode: edited.value.mode ? mode.value : null,
        tcpPort: edited.value.mode ? tcpPort.value : null,
      })
      // 后端只返回**这次真正要填**的项（`null` = 模板没填这一项），其余保持界面原值
      if (r.target != null) prefix.value = r.target
      if (r.timeoutMs != null) timeoutMs.value = r.timeoutMs
      if (r.concurrency != null) concurrency.value = r.concurrency
      if (r.mode != null) mode.value = r.mode
      if (r.tcpPort != null) tcpPort.value = r.tcpPort
      activeProfile.value = r.name
      lastProfileApply.value = r
      // 目标换了就重新解析一次：摘要、分段、矩阵可用性全由后端算，
      // 前端不实现第二套解析规则（解析不出来时后端给的那句话会进 specError）。
      if (r.target != null) await validateSpec(prefix.value)
      status.value =
        r.filled.length > 0
          ? t('status.profileApplied', r.name, fieldSummary(r.filled))
          : t('status.profileAppliedNoGap', r.name)
      return true
    } catch (e) {
      // 后端给的是可直接展示的原因（点名不存在的模板 / 配置文件读不动……），不要再包一层
      status.value = String(e)
      return false
    } finally {
      profileBusy.value = false
    }
  }

  // -------------------------------------------------------------------------
  // M17 / M18：安全策略
  // -------------------------------------------------------------------------

  /**
   * 当前生效的安全策略。`null` = 还没从后端拉过。
   *
   * 后端在没有人为设置时给的是**安全的默认值**（限速 500 包/秒 + 拒绝保留网段）：
   * 把「可调」做出来，但绝不因此把默认值放松。
   */
  const policy = ref<ScanPolicyDto | null>(null)
  const policyBusy = ref(false)

  async function loadPolicy(): Promise<void> {
    policyBusy.value = true
    try {
      policy.value = await svc.scanPolicy()
    } catch (e) {
      status.value = t('status.policyFailed', String(e))
    } finally {
      policyBusy.value = false
    }
  }

  /**
   * 改策略。
   *
   * 越界的值后端**拒绝并给出原因、且不改动状态**（不静默夹取），
   * 界面把原因原样显示出来——自己不夹取、也不改写那句话。
   */
  async function updatePolicy(
    maxPps: number,
    maxTargets: number,
    allowReserved: boolean,
  ): Promise<boolean> {
    policyBusy.value = true
    try {
      const p = await svc.setScanPolicy(maxPps, maxTargets, allowReserved)
      policy.value = p
      status.value = t('status.policyApplied', p.describe)
      return true
    } catch (e) {
      status.value = String(e)
      return false
    } finally {
      policyBusy.value = false
    }
  }

  function setTcpPort(port: number) {
    if (!Number.isFinite(port) || port <= 0 || port > 65535) return
    tcpPort.value = Math.floor(port)
    // 端口是方式的一部分：用户手改过端口，模板就不该再动「怎么扫」
    markEdited('mode')
    if (mode.value !== 'tcp') setMode('tcp')
    else status.value = t('status.modeChanged', modeLabel.value)
  }

  /**
   * 切换语言后，把「文本由后端渲染」的那几份数据重新拉一遍。
   *
   * 为什么需要这一步：界面文案由模板里的 `t()` 响应式重渲染，但
   * `specSummary` / `changes` / `backendSummary` 这些是后端**按当时的语言
   * 渲染好的成品**——前端没有、也不该有「它对应哪个键」的信息。不重拉的话，
   * 切到英文后这些**已经拿到的**文本会继续显示中文，直到用户碰巧再操作一次。
   *
   * 边界（每一条都是有意选择的）：
   * - **扫描进行中不碰快照**：`refreshSnapshot` 会整体覆盖 `results`，
   *   与事件流写入竞争（见上面那条注释）。
   * - **趋势 / 告警只在「已经看过」时才拉**：与 `resyncFromBackend` 同一条规矩，
   *   不为了切语言白花 IPC。
   * - **不重拉端口指纹**：那是真实端口扫描的产物，重拉等于再扫一遍网段，
   *   代价与收益不成比例；要换语言就得用户自己再点一次「指纹」。
   * - **不回溯改写 `status` / `enrichNote`**：它们是事件日志，记录事件发生
   *   那一刻的语言（既有约定，见 `status` 的声明处）。
   * - **模板与策略照「已经看过才拉」的规矩办**：它们的摘要/描述是后端按当时的
   *   语言渲染好的成品，不重拉就会一直停在旧语言。
   */
  async function refreshLocalizedViews(): Promise<void> {
    await loadBackendCaps()
    if (!running.value) {
      try {
        await refreshSnapshot()
      } catch (e) {
        // 与 `resyncFromBackend` 一致：拉不到就留着旧数据，不把界面清空
        status.value = t('status.resyncFailed', String(e))
      }
    }
    await loadChanges(200)
    if (trend.value !== null) await loadTrend()
    if (alerts.value !== null) await loadAlerts()
    if (profilesLoaded.value) await loadProfiles()
    if (policy.value !== null) await loadPolicy()
  }

  /** 在常用端口之间轮换（`T` 之外的鼠标友好路径）。 */
  function cycleTcpPort(pool: readonly number[]): void {
    const idx = pool.indexOf(tcpPort.value)
    const next = pool[(idx + 1) % pool.length]
    setTcpPort(next)
  }

  // -------------------------------------------------------------------------
  // M18：无障碍（主题 / 字号 / 减少动效）
  // -------------------------------------------------------------------------

  /**
   * 无障碍状态。
   *
   * `theme` / `fontScale` **不落盘**：字号是 zoom 在**后端窗口**上的状态，
   * 前端没有反过来读它的接口；持久化只会造出「界面显示标准、实际缩放 150%」
   * 这种对不上的状态，那比不记住更糟。本轮只要求「当前会话可达」。
   *
   * `reduceMotion` 的初值取系统偏好（`prefers-reduced-motion`），让开关的
   * 位置与实际生效的动效一致——CSS 侧本来就有一条同名的媒体查询，
   * 开关是给「系统没设、但这个用户不想看动画」的人准备的显式入口。
   */
  const a11y = ref({
    theme: 'default' as ThemeKey,
    fontScale: 'normal' as FontScaleKey,
    /** 当前生效的字号百分比（由后端回报；失败时保持上一次的值） */
    percent: 100,
    reduceMotion: systemPrefersReducedMotion(),
  })

  function systemPrefersReducedMotion(): boolean {
    try {
      return window.matchMedia?.('(prefers-reduced-motion: reduce)').matches === true
    } catch {
      // 拿不到媒体查询（旧 WebView / 非浏览器环境）：当作「不减少」，不影响使用
      return false
    }
  }

  /**
   * 最近一次应用到的配色（`null` = 还没从后端拿到）。
   *
   * 为什么要留一份：`styles.css` 里**自绘**的部分靠 CSS 变量就够了，但
   * naive-ui 的内置控件（页签、输入框、按钮）走它自己的主题系统，读不到
   * CSS 变量。`App.vue` 据此决定「要不要切 naive 的深色主题」以及主色取什么
   * ——深色主题下不切的话，页签文字会是深灰配深底，直接读不出来。
   */
  const themePalette = ref<ThemeDto | null>(null)

  /**
   * 把一套配色写成根元素的 CSS 变量，并标出主题键。
   *
   * 写成 `document.documentElement` 上的**内联变量**（而不是往 stylesheet 里
   * 注入规则）：`:root` 里的浅色默认值因此一直留在样式表里，未加载主题时
   * 视觉与改动前完全一致；同时也让「哪几个令牌参与主题」在 CSS 里一眼可见。
   */
  function applyTheme(dto: ThemeDto): void {
    const root = document.documentElement
    root.style.setProperty('--cping-bg', dto.bg)
    root.style.setProperty('--cping-fg', dto.fg)
    root.style.setProperty('--cping-muted', dto.muted)
    root.style.setProperty('--cping-border', dto.border)
    root.style.setProperty('--cping-panel', dto.panel)
    root.style.setProperty('--cping-subtle', dto.subtle)
    root.style.setProperty('--cping-accent', dto.accent)
    root.dataset.theme = dto.theme
  }

  /** 减少动效：给根元素打标，`styles.css` 里有一条同名的属性选择器覆盖。 */
  function applyReduceMotion(on: boolean): void {
    const root = document.documentElement
    if (on) root.dataset.reduceMotion = 'true'
    else delete root.dataset.reduceMotion
  }

  /** 启动时读一次主题（不传名字 = 用环境默认）。失败只写状态栏，不影响探测。 */
  async function loadA11y(): Promise<void> {
    applyReduceMotion(a11y.value.reduceMotion)
    try {
      const dto = await svc.a11yTheme(null)
      applyTheme(dto)
      themePalette.value = dto
      a11y.value = { ...a11y.value, theme: dto.theme as ThemeKey }
    } catch (e) {
      // 主题读不到不拦路：界面继续用 `:root` 里的浅色默认值
      status.value = t('status.a11yThemeFailed', String(e))
    }
  }

  /**
   * 切主题。
   *
   * **不做本地白名单校验**：认得/认不得只有一处判定（核心的 `Theme::parse`），
   * 界面再抄一份名单迟早与之漂移。后端认不出会报错，这里原样显示那个原因。
   */
  async function setTheme(name: ThemeKey): Promise<void> {
    try {
      const dto = await svc.a11yTheme(name)
      applyTheme(dto)
      themePalette.value = dto
      a11y.value = { ...a11y.value, theme: dto.theme as ThemeKey }
      const label = (THEME_LABEL_KEYS as Record<string, string>)[dto.theme] ?? dto.theme
      status.value = t('status.a11yThemeSet', label)
    } catch (e) {
      status.value = String(e)
    }
  }

  /**
   * 切字号（整个 webview 缩放）。
   *
   * 平台不支持或调用失败时**后端会抛错**，这里原样显示原因——用户点了没反应
   * 是最难查的一类问题，所以这条路径绝不吞异常。
   */
  async function setFontScale(name: FontScaleKey): Promise<void> {
    try {
      const percent = await svc.setFontScale(name)
      a11y.value = { ...a11y.value, fontScale: name, percent }
      status.value = t('status.a11yFontScaleSet', percent)
    } catch (e) {
      status.value = String(e)
    }
  }

  /** 减少动效开关。纯界面行为，不需要后端参与，因此不会失败。 */
  function setReduceMotion(on: boolean): void {
    a11y.value = { ...a11y.value, reduceMotion: on }
    applyReduceMotion(on)
    status.value = on ? t('status.reduceMotionOn') : t('status.reduceMotionOff')
  }

  // -------------------------------------------------------------------------
  // M8：持续监控
  // -------------------------------------------------------------------------
  //
  // 轮次由界面驱动，与 TUI 的 `tick_monitor()` 对称：一轮扫描结束后等
  // `interval` 秒再发起下一轮。核心不持有定时器，因此暂停/关闭监控时
  // 不会在后台留下任何线程。
  //
  // 语义要点（与 core::monitor 对齐）：
  // - 首轮只建立基线，`diff_scans` 对空基线恒返回空，所以第一轮不会报「全部上线」；
  // - 正在扫描时不排下一轮，避免并发两个任务互相覆盖统计。

  function ensureMonitorTimer() {
    if (monitorTimer) return
    monitorTimer = setInterval(tickMonitor, 1000)
  }

  function stopMonitorTimer() {
    if (monitorTimer) {
      clearInterval(monitorTimer)
      monitorTimer = null
    }
    nextRoundAt = 0
    countdown.value = null
  }

  /** 每秒一次：排下一轮、更新倒计时、到点发起扫描。 */
  function tickMonitor() {
    if (!monitorEnabled.value) {
      stopMonitorTimer()
      return
    }
    if (running.value) {
      // 扫描进行中：本轮结束前不排期
      nextRoundAt = 0
      countdown.value = null
      return
    }
    const now = Date.now()
    if (nextRoundAt === 0) {
      nextRoundAt = now + monitorInterval.value * 1000
      countdown.value = monitorInterval.value
      return
    }
    const left = Math.ceil((nextRoundAt - now) / 1000)
    if (left <= 0) {
      nextRoundAt = 0
      countdown.value = null
      void start()
    } else {
      countdown.value = left
    }
  }

  function toggleMonitor() {
    monitorEnabled.value = !monitorEnabled.value
    if (monitorEnabled.value) {
      ensureMonitorTimer()
      // 一轮结果都没有时立刻跑第一轮，否则只是接着排期
      if (stats.value.completed === 0 && !running.value) {
        nextRoundAt = 0
        void start()
      } else {
        nextRoundAt = 0
        tickMonitor()
      }
      status.value = t('status.monitorOn', monitorInterval.value)
    } else {
      stopMonitorTimer()
      status.value = t('status.monitorOff')
    }
  }

  function setMonitorInterval(seconds: number) {
    if (!Number.isFinite(seconds) || seconds < 5 || seconds > 3600) return
    monitorInterval.value = Math.floor(seconds)
    // 改间隔立即重新排期，不等当前倒计时走完
    nextRoundAt = 0
    if (monitorEnabled.value) {
      countdown.value = monitorInterval.value
      status.value = t('status.monitorIntervalChanged', monitorInterval.value)
    }
  }

  /** 在预设档位之间移动（`[` / `]` 或下拉框）。 */
  function shiftMonitorInterval(direction: 1 | -1) {
    const pool: readonly number[] = MONITOR_INTERVALS
    const idx = pool.indexOf(monitorInterval.value as (typeof pool)[number])
    const next = idx < 0 ? pool[2] : pool[Math.min(pool.length - 1, Math.max(0, idx + direction))]
    setMonitorInterval(next)
  }

  /** 拉一次变化流水（新的在前）。 */
  async function loadChanges(limit = 200) {
    try {
      changeFeed.value = await svc.recentChanges(limit)
    } catch {
      // 变化流水是辅助信息，拉不到就把上次的结果留着，不清空
    }
  }

  function openChanges() {
    showChanges.value = true
    void loadChanges()
  }

  function closeChanges() {
    showChanges.value = false
  }

  // -------------------------------------------------------------------------
  // 配置
  // -------------------------------------------------------------------------

  async function loadConfig() {
    try {
      const cfg: AppConfigDto = await svc.loadConfig()
      prefix.value = cfg.probe.prefix || prefix.value
      timeoutMs.value = cfg.probe.timeoutMs
      concurrency.value = cfg.probe.concurrency
      colors.value = cfg.colors
      hideOffline.value = cfg.hideOffline
      view.value = cfg.view
      // M9：旧配置里没有 mode → 用平台默认
      const key = cfg.probe.mode
      if (key === 'icmp' || key === 'tcp') {
        mode.value = key
      } else {
        await loadDefaultMode()
      }
      if (cfg.probe.tcpPort && cfg.probe.tcpPort > 0) tcpPort.value = cfg.probe.tcpPort
      // M8
      if (cfg.monitor) {
        monitorEnabled.value = cfg.monitor.enabled
        const sec = Math.round(cfg.monitor.intervalMs / 1000)
        if (sec >= 5 && sec <= 3600) monitorInterval.value = sec
      }
      // 配置里开着监控：进程刚起来时先排期，不抢在用户点「开始」之前自动扫，
      // 所以这里只起定时器，等首轮结果出现后再进入正常轮次节奏。
      if (monitorEnabled.value) ensureMonitorTimer()
    } catch {
      // 首次运行没有配置属于正常情况
    }
  }

  async function persistConfig() {
    const cfg: AppConfigDto = {
      probe: {
        prefix: prefix.value,
        range: 'standard',
        timeoutMs: timeoutMs.value,
        concurrency: concurrency.value,
        lowMaxMs: 50,
        midMaxMs: 100,
        mode: mode.value,
        tcpPort: mode.value === 'tcp' ? tcpPort.value : null,
      },
      colors: colors.value,
      hideOffline: hideOffline.value,
      view: view.value,
      monitor: {
        enabled: monitorEnabled.value,
        intervalMs: monitorInterval.value * 1000,
      },
    }
    try {
      await svc.saveConfig(cfg)
      status.value = t('status.configSaved')
    } catch (e) {
      status.value = t('status.configSaveFailed', String(e))
    }
  }

  // -------------------------------------------------------------------------
  // M10：导出
  // -------------------------------------------------------------------------

  /** 按扩展名导出（CSV / TXT / JSON / HTML 由后端决定格式）。 */
  async function exportTo(path: string) {
    try {
      // 后端返回的是一句成品提示（含文件路径），原样展示
      const msg = await svc.exportResults(path)
      lastExport.value = path
      status.value = msg
    } catch (e) {
      status.value = t('status.exportFailed', String(e))
    }
  }

  /** 移动端：没有文件对话框，由后端写到应用可写目录。 */
  async function exportToApp(format: ExportFormatKey): Promise<string | null> {
    try {
      const path = await svc.exportToAppDir(format)
      lastExport.value = path
      status.value = t('status.savedTo', path)
      return path
    } catch (e) {
      status.value = t('status.exportFailed', String(e))
      return null
    }
  }

  /** M10：导出「上一轮 → 本轮」差异报告（.csv）。 */
  async function exportDiffTo(path: string): Promise<boolean> {
    try {
      status.value = await svc.exportDiff(path)
      return true
    } catch (e) {
      status.value = t('status.exportDiffFailed', String(e))
      return false
    }
  }

  async function loadLocalAddresses() {
    try {
      const list = await svc.localAddresses()
      localIp.value = list.length > 0 ? list.join(', ') : t('common.unknown')
    } catch {
      localIp.value = t('common.unknown')
    }
  }

  function setColor(key: ResultClassKey, value: string) {
    colors.value = { ...colors.value, [key]: value }
  }

  // -------------------------------------------------------------------------
  // M7：目标信息补全
  // -------------------------------------------------------------------------

  /** 读邻居表回填 MAC / 厂商；返回给调用方一句可直接提示的话。 */
  async function enrich(): Promise<string | null> {
    if (stats.value.completed === 0) {
      status.value = t('status.noResults')
      return null
    }
    try {
      const report = await svc.enrichResults()
      // report.message 由后端拼好，原样展示
      enrichNote.value = report.message
      status.value = report.message
      await refreshSnapshot()
      return report.message
    } catch (e) {
      status.value = t('status.enrichFailed', String(e))
      return null
    }
  }

  /** 按需反查某一行的主机名。查不到就保持为空，不填占位文本。 */
  async function lookupHostname(index: number, timeoutMs = 2000): Promise<string | null> {
    hostnameBusy.value = index
    try {
      const r = await svc.lookupHostnameFor(index, timeoutMs)
      const row = results.value[index]
      if (row) results.value[index] = { ...row, hostname: r.hostname }
      const msg = r.hostname ? `${r.ip} → ${r.hostname}` : t('status.noReverseDns', r.ip)
      status.value = msg
      return msg
    } catch (e) {
      status.value = t('status.hostnameLookupFailed', String(e))
      return null
    } finally {
      hostnameBusy.value = null
    }
  }

  // -------------------------------------------------------------------------
  // M7：结果快照
  // -------------------------------------------------------------------------

  /** 启动时检查有没有上次没处理完的结果。找到了就等用户表态，绝不自动恢复。 */
  async function checkPendingSnapshot(): Promise<void> {
    try {
      pendingRestore.value = await svc.pendingSnapshot()
    } catch (e) {
      status.value = t('status.checkPendingFailed', String(e))
    }
  }

  /** 用户点了「恢复」之后才真正恢复。 */
  async function restoreSnapshot(path?: string): Promise<boolean> {
    try {
      const snap = await svc.restoreSnapshot(path)
      applySnapshot(snap)
      pendingRestore.value = null
      status.value = t('status.restored', snap.results.length, snap.specSummary, snap.total)
      return true
    } catch (e) {
      status.value = t('status.restoreFailed', String(e))
      return false
    }
  }

  async function dismissSnapshot(deleteFile: boolean): Promise<void> {
    try {
      await svc.dismissSnapshot(deleteFile)
    } catch (e) {
      status.value = t('status.dismissFailed', String(e))
    } finally {
      pendingRestore.value = null
    }
  }

  async function saveSnapshot(path: string): Promise<string | null> {
    try {
      const saved = await svc.saveSnapshot(path)
      lastSnapshot.value = saved.path
      status.value = t('status.snapshotSaved', saved.rows, saved.total, saved.path)
      return saved.path
    } catch (e) {
      status.value = t('status.snapshotSaveFailed', String(e))
      return null
    }
  }

  /** 移动端：没有文件对话框，由后端写到应用可写目录。 */
  async function saveSnapshotToAppDir(): Promise<string | null> {
    try {
      const saved = await svc.saveSnapshotToAppDir()
      lastSnapshot.value = saved.path
      status.value = t('status.snapshotSavedApp', saved.rows, saved.path)
      return saved.path
    } catch (e) {
      status.value = t('status.snapshotSaveFailed', String(e))
      return null
    }
  }

  return {
    prefix,
    timeoutMs,
    concurrency,
    running,
    cancelled,
    taskId,
    elapsedMs,
    stats,
    results,
    cellSlots,
    targets,
    view,
    hideOffline,
    colors,
    localIp,
    status,
    lastExport,
    pendingRestore,
    enrichNote,
    lastSnapshot,
    hostnameBusy,
    // M9
    mode,
    tcpPort,
    segments,
    specSummary,
    matrixCompatible,
    specError,
    backendCaps,
    backendSummary,
    backendCapDetails,
    asyncEnabled,
    // M13
    fingerprints,
    showFingerprints,
    fingerprintBusy,
    fingerprintPorts,
    fingerprintBanner,
    fingerprintedHosts,
    fingerprintOpenPorts,
    fingerprintWithOpenPorts,
    fingerprintLines,
    runFingerprint,
    dropFingerprints,
    // M14
    trend,
    trendBusy,
    trendHostCount,
    loadTrend,
    dropTrend,
    exportTrend,
    // M15
    alerts,
    alertsBusy,
    alertFeed,
    alertNote,
    alertCount,
    loadAlerts,
    setAlertsEnabled,
    setAlertMuted,
    updateAlertRules,
    dropAlerts,
    // M16：扫描模板
    profiles,
    profilesPath,
    profilesNotes,
    profileBusy,
    activeProfile,
    lastProfileApply,
    loadProfiles,
    applyProfileByName,
    fieldSummary,
    // M17 / M18：安全策略
    policy,
    policyBusy,
    loadPolicy,
    updatePolicy,
    // M18：无障碍（主题 / 字号 / 减少动效）
    a11y,
    themePalette,
    loadA11y,
    setTheme,
    setFontScale,
    setReduceMotion,
    // M8
    round,
    changes,
    changeTotal,
    diffSummary,
    monitorEnabled,
    monitorInterval,
    showChanges,
    changeFeed,
    highlightUntil,
    countdown,
    // computed
    progress,
    visibleRows,
    onlineCount,
    recognized,
    modeLabel,
    changeMap,
    changedIndexes,
    secondsToNextRound,
    canUseMatrix,
    // helpers
    colorOf,
    labelOf,
    changeNoteOf,
    isChanged,
    attachEvents,
    refreshSnapshot,
    start,
    cancel,
    validateSpec,
    loadDefaultMode,
    loadBackendCaps,
    setMode,
    setTcpPort,
    cycleTcpPort,
    // M16：用户在界面上「填过」的唯一入口（模板「只填空缺」据此判定）
    setTargetSpec,
    setTimeoutMs,
    setConcurrency,
    toggleMonitor,
    setMonitorInterval,
    shiftMonitorInterval,
    loadChanges,
    openChanges,
    closeChanges,
    loadConfig,
    persistConfig,
    // 切语言后重拉「由后端渲染文本」的那几份数据
    refreshLocalizedViews,
    exportTo,
    exportToApp,
    exportDiffTo,
    loadLocalAddresses,
    setColor,
    enrich,
    lookupHostname,
    checkPendingSnapshot,
    restoreSnapshot,
    dismissSnapshot,
    saveSnapshot,
    saveSnapshotToAppDir,
  }
})
