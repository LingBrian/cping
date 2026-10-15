import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import type { UnlistenFn } from '@tauri-apps/api/event'
import {
  CLASS_LABELS,
  DEFAULT_COLORS,
  MATRIX_CELLS,
  MONITOR_INTERVALS,
  type AppConfigDto,
  type BackendCapsDto,
  type ChangeDto,
  type ColorConfigDto,
  type ExportFormatKey,
  type FingerprintDto,
  type PendingSnapshotDto,
  type ProbeModeKey,
  type ProbeResultDto,
  type ResultClassKey,
  type ScanStatsDto,
  type SegmentDto,
  type SnapshotDto,
} from '../shared/types'
import * as svc from '../services/ping'

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
  const localIp = ref('未知')
  const status = ref('就绪')
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
   */
  const backendCapDetails = computed(() =>
    backendCaps.value
      .map((c) => `${c.label}（${c.id}）：${c.available ? '可用' : '不可用'} · ${c.describe}`)
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
    return key ? CLASS_LABELS[key] : '等待'
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
      if (withInfo) {
        const n = recognized.value
        const base = cancelled.value
          ? `已取消：完成 ${stats.value.completed}/${stats.value.total}，已完成结果保留`
          : `扫描完成：${stats.value.total} 个目标，${stats.value.online} 在线`
        const tail = diffSummary.value ? `；${diffSummary.value}` : ''
        status.value = `${base}；识别 MAC ${n.mac}、厂商 ${n.vendor}${tail}`
      }
    } catch (e) {
      status.value = `结果校准失败：${String(e)}`
    }
  }

  async function start() {
    status.value = '正在启动…'
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
      status.value = `开始探测 ${specSummary.value || prefix.value}（${modeLabel.value}）`
    } catch (e) {
      status.value = `无法启动：${String(e)}`
    }
  }

  async function cancel() {
    try {
      await svc.cancelScan()
      await refreshSnapshot()
      status.value = '已请求取消'
    } catch (e) {
      status.value = `取消失败：${String(e)}`
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
    status.value = `探测方式：${modeLabel.value}（下次探测生效）`
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
      status.value = '已有一次指纹探测在进行中'
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
      const msg = `端口指纹：${list.length} 台主机 · ${openTotal} 个开放端口（${withOpen} 台有开放端口）`
      status.value = msg
      return msg
    } catch (e) {
      status.value = `端口指纹失败：${String(e)}`
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
    status.value = '已清空端口指纹'
  }

  /** 指纹面板里的一行行文本（与界面同源，便于测试）。 */
  const fingerprintLines = computed<string[]>(() => {
    const lines: string[] = ['端口指纹（附加信息，与在线判定相互独立）']
    if (fingerprints.value.length === 0) {
      lines.push('还没有指纹结果：点「端口指纹」或按 F 开始')
      return lines
    }
    for (const f of fingerprints.value) {
      if (f.openPorts.length === 0) {
        lines.push(`${f.ip}  无开放端口`)
        continue
      }
      lines.push(`${f.ip}  ${f.openPortsSummary}`)
      for (const p of f.openPorts) {
        if (p.banner) lines.push(`    :${p.port} ${p.banner}`)
      }
      if (f.deviceSummary) lines.push(`    ${f.deviceSummary}`)
    }
    return lines
  })

  function setTcpPort(port: number) {
    if (!Number.isFinite(port) || port <= 0 || port > 65535) return
    tcpPort.value = Math.floor(port)
    if (mode.value !== 'tcp') setMode('tcp')
    else status.value = `探测方式：${modeLabel.value}（下次探测生效）`
  }

  /** 在常用端口之间轮换（`T` 之外的鼠标友好路径）。 */
  function cycleTcpPort(pool: readonly number[]): void {
    const idx = pool.indexOf(tcpPort.value)
    const next = pool[(idx + 1) % pool.length]
    setTcpPort(next)
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
      status.value = `已开启持续监控，每 ${monitorInterval.value} 秒重扫一次`
    } else {
      stopMonitorTimer()
      status.value = '已关闭持续监控'
    }
  }

  function setMonitorInterval(seconds: number) {
    if (!Number.isFinite(seconds) || seconds < 5 || seconds > 3600) return
    monitorInterval.value = Math.floor(seconds)
    // 改间隔立即重新排期，不等当前倒计时走完
    nextRoundAt = 0
    if (monitorEnabled.value) {
      countdown.value = monitorInterval.value
      status.value = `持续监控间隔已改为 ${monitorInterval.value} 秒`
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
      status.value = '设置已保存'
    } catch (e) {
      status.value = `保存设置失败：${String(e)}`
    }
  }

  // -------------------------------------------------------------------------
  // M10：导出
  // -------------------------------------------------------------------------

  /** 按扩展名导出（CSV / TXT / JSON / HTML 由后端决定格式）。 */
  async function exportTo(path: string) {
    try {
      const msg = await svc.exportResults(path)
      lastExport.value = path
      status.value = msg
    } catch (e) {
      status.value = `导出失败：${String(e)}`
    }
  }

  /** 移动端：没有文件对话框，由后端写到应用可写目录。 */
  async function exportToApp(format: ExportFormatKey): Promise<string | null> {
    try {
      const path = await svc.exportToAppDir(format)
      lastExport.value = path
      status.value = `已保存：${path}`
      return path
    } catch (e) {
      status.value = `导出失败：${String(e)}`
      return null
    }
  }

  /** M10：导出「上一轮 → 本轮」差异报告（.csv）。 */
  async function exportDiffTo(path: string): Promise<boolean> {
    try {
      status.value = await svc.exportDiff(path)
      return true
    } catch (e) {
      status.value = `导出差异失败：${String(e)}`
      return false
    }
  }

  async function loadLocalAddresses() {
    try {
      const list = await svc.localAddresses()
      localIp.value = list.length > 0 ? list.join(', ') : '未知'
    } catch {
      localIp.value = '未知'
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
      status.value = '还没有已完成的结果'
      return null
    }
    try {
      const report = await svc.enrichResults()
      enrichNote.value = report.message
      status.value = report.message
      await refreshSnapshot()
      return report.message
    } catch (e) {
      status.value = `补全失败：${String(e)}`
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
      const msg = r.hostname ? `${r.ip} → ${r.hostname}` : `${r.ip} 没有反向解析记录`
      status.value = msg
      return msg
    } catch (e) {
      status.value = `主机名反查失败：${String(e)}`
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
      status.value = `检查历史结果失败：${String(e)}`
    }
  }

  /** 用户点了「恢复」之后才真正恢复。 */
  async function restoreSnapshot(path?: string): Promise<boolean> {
    try {
      const snap = await svc.restoreSnapshot(path)
      applySnapshot(snap)
      pendingRestore.value = null
      status.value = `已恢复 ${snap.results.length} 条结果（${snap.specSummary}，共 ${snap.total} 个目标）`
      return true
    } catch (e) {
      status.value = `恢复失败：${String(e)}`
      return false
    }
  }

  async function dismissSnapshot(deleteFile: boolean): Promise<void> {
    try {
      await svc.dismissSnapshot(deleteFile)
    } catch (e) {
      status.value = `忽略历史结果失败：${String(e)}`
    } finally {
      pendingRestore.value = null
    }
  }

  async function saveSnapshot(path: string): Promise<string | null> {
    try {
      const saved = await svc.saveSnapshot(path)
      lastSnapshot.value = saved.path
      status.value = `已保存快照：${saved.rows} 条结果（共 ${saved.total} 个目标）→ ${saved.path}`
      return saved.path
    } catch (e) {
      status.value = `保存快照失败：${String(e)}`
      return null
    }
  }

  /** 移动端：没有文件对话框，由后端写到应用可写目录。 */
  async function saveSnapshotToAppDir(): Promise<string | null> {
    try {
      const saved = await svc.saveSnapshotToAppDir()
      lastSnapshot.value = saved.path
      status.value = `已保存快照：${saved.rows} 条结果 → ${saved.path}`
      return saved.path
    } catch (e) {
      status.value = `保存快照失败：${String(e)}`
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
    toggleMonitor,
    setMonitorInterval,
    shiftMonitorInterval,
    loadChanges,
    openChanges,
    closeChanges,
    loadConfig,
    persistConfig,
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
