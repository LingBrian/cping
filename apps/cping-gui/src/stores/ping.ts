import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import type { UnlistenFn } from '@tauri-apps/api/event'
import {
  CLASS_LABELS,
  DEFAULT_COLORS,
  MATRIX_CELLS,
  type AppConfigDto,
  type ColorConfigDto,
  type ProbeResultDto,
  type ResultClassKey,
  type ScanStatsDto,
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

  let unlisten: UnlistenFn | null = null

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

  function colorOf(key: ResultClassKey | null): string {
    if (!key) return 'transparent'
    return colors.value[key]
  }

  function labelOf(key: ResultClassKey | null): string {
    return key ? CLASS_LABELS[key] : '等待'
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
          status.value = `扫描完成：${event.stats.total} 个目标，${event.stats.online} 在线`
          break
        case 'cancelled':
          stats.value = event.stats
          elapsedMs.value = event.elapsedMs
          running.value = false
          cancelled.value = true
          status.value = `已取消：完成 ${event.stats.completed}/${event.stats.total}，已完成结果保留`
          break
      }
    })
  }

  async function refreshSnapshot() {
    const snap: SnapshotDto = await svc.snapshot()
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
  }

  async function start() {
    status.value = '正在启动…'
    try {
      const id = await svc.startScan(prefix.value, timeoutMs.value, concurrency.value)
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
      status.value = `开始探测 ${prefix.value}`
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

  async function loadConfig() {
    try {
      const cfg: AppConfigDto = await svc.loadConfig()
      prefix.value = cfg.probe.prefix || prefix.value
      timeoutMs.value = cfg.probe.timeoutMs
      concurrency.value = cfg.probe.concurrency
      colors.value = cfg.colors
      hideOffline.value = cfg.hideOffline
      view.value = cfg.view
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
      },
      colors: colors.value,
      hideOffline: hideOffline.value,
      view: view.value,
    }
    try {
      await svc.saveConfig(cfg)
      status.value = '设置已保存'
    } catch (e) {
      status.value = `保存设置失败：${String(e)}`
    }
  }

  async function exportTo(path: string) {
    try {
      const msg = await svc.exportResults(path)
      lastExport.value = path
      status.value = msg
    } catch (e) {
      status.value = `导出失败：${String(e)}`
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
    progress,
    visibleRows,
    onlineCount,
    colorOf,
    labelOf,
    attachEvents,
    refreshSnapshot,
    start,
    cancel,
    loadConfig,
    persistConfig,
    exportTo,
    loadLocalAddresses,
    setColor,
  }
})
