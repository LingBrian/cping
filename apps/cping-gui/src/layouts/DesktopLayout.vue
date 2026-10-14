<script setup lang="ts">
import { ref, watch } from 'vue'
import {
  NButton,
  NInputNumber,
  NSelect,
  NSwitch,
  NTabPane,
  NTabs,
  useMessage,
} from 'naive-ui'
import { save } from '@tauri-apps/plugin-dialog'
import ChangePanel from '../components/ChangePanel.vue'
import ColorSettings from '../components/ColorSettings.vue'
import PrefixForm from '../components/PrefixForm.vue'
import ProgressSummary from '../components/ProgressSummary.vue'
import ResultGrid from '../components/ResultGrid.vue'
import ResultTable from '../components/ResultTable.vue'
import StatusBar from '../components/StatusBar.vue'
import { MONITOR_INTERVALS, type ExportFormatKey } from '../shared/types'
import { usePingStore } from '../stores/ping'

const store = usePingStore()
const message = useMessage()
const tab = ref<'matrix' | 'table' | 'changes'>(store.view === 'table' ? 'table' : 'matrix')

watch(tab, (v) => {
  // 变化页签不是一个「视图偏好」，所以不写进配置
  if (v === 'matrix' || v === 'table') store.view = v
})

const monitorOptions = MONITOR_INTERVALS.map((s) => ({ label: `${s} 秒`, value: s }))

/** 导出格式由扩展名决定，和 Rust 侧的 `ExportFormat::from_path` 一一对应。 */
const EXPORT_FILTERS = [
  { name: 'CSV 文件', extensions: ['csv'] },
  { name: '文本文件', extensions: ['txt'] },
  { name: 'JSON 文件', extensions: ['json'] },
  { name: 'HTML 报告', extensions: ['html'] },
]

function formatOf(path: string): ExportFormatKey | null {
  const lower = path.toLowerCase()
  if (lower.endsWith('.csv')) return 'csv'
  if (lower.endsWith('.txt')) return 'txt'
  if (lower.endsWith('.json')) return 'json'
  if (lower.endsWith('.html')) return 'html'
  return null
}

/** 目标可能是多段，文件名里只留安全字符。 */
function fileStem(): string {
  const base = store.specSummary || store.prefix
  return `cping-${base.replace(/[^0-9A-Za-z]+/g, '_').replace(/^_+|_+$/g, '')}`
}

async function onExport() {
  if (store.stats.completed === 0) {
    message.warning('还没有已完成的结果')
    return
  }
  const path = await save({
    defaultPath: `${fileStem()}.csv`,
    filters: EXPORT_FILTERS,
  })
  if (!path) return
  const fmt = formatOf(path)
  if (!fmt) {
    message.error('请用 .csv / .txt / .json / .html 扩展名')
    return
  }
  await store.exportTo(path)
  if (store.status.startsWith('已导出')) message.success(store.status)
  else message.error(store.status)
}

/** M10：导出「上一轮 → 本轮」差异（只有 .csv）。 */
async function onExportDiff() {
  if (!store.diffSummary) {
    message.warning('还没有可对比的上一轮结果（至少完成两轮扫描）')
    return
  }
  const path = await save({
    defaultPath: `${fileStem()}-diff.csv`,
    filters: [{ name: 'CSV 文件', extensions: ['csv'] }],
  })
  if (!path) return
  if (await store.exportDiffTo(path)) message.success(store.status)
  else message.error(store.status)
}

/** M7：读本机邻居表回填 MAC，并据 IEEE 注册数据补厂商。 */
async function onEnrich() {
  if (store.stats.completed === 0) {
    message.warning('还没有已完成的结果')
    return
  }
  const msg = await store.enrich()
  if (msg) message.success(msg)
  else message.error(store.status)
}

/** M7：把当前结果存成快照，下次启动可以直接恢复。 */
async function onSaveSnapshot() {
  if (store.targets.length === 0) {
    message.warning('还没有扫描结果')
    return
  }
  const path = await save({
    defaultPath: `${fileStem()}.snap`,
    filters: [{ name: 'CPing 快照', extensions: ['snap'] }],
  })
  if (!path) return
  const saved = await store.saveSnapshot(path)
  if (saved) message.success(store.status)
  else message.error(store.status)
}
</script>

<template>
  <div class="desktop">
    <div class="desktop__side">
      <PrefixForm />
      <ProgressSummary />

      <!-- M8：持续监控。间隔用下拉而不是输入框，避免随手填出 1 秒把网段打满。 -->
      <div class="cping-panel">
        <div style="display: flex; align-items: center; justify-content: space-between">
          <span style="font-size: 13px">持续监控</span>
          <NSwitch :value="store.monitorEnabled" size="small" @update:value="store.toggleMonitor()" />
        </div>
        <div style="display: flex; align-items: center; gap: 8px; margin-top: 8px">
          <span style="font-size: 12px; color: #6b7280">间隔</span>
          <NSelect
            :value="store.monitorInterval"
            size="small"
            style="flex: 1"
            :options="monitorOptions"
            :disabled="!store.monitorEnabled"
            @update:value="store.setMonitorInterval"
          />
          <NInputNumber
            :value="store.monitorInterval"
            size="small"
            style="width: 96px"
            :min="5"
            :max="3600"
            :disabled="!store.monitorEnabled"
            @update:value="(v: number | null) => v && store.setMonitorInterval(v)"
          />
        </div>
        <div style="font-size: 11px; color: #6b7280; margin-top: 6px">
          <template v-if="store.monitorEnabled">
            第 {{ store.round }} 轮 · 变化 {{ store.changes.length }} 条 · 累计
            {{ store.changeTotal }} 条
            <template v-if="store.secondsToNextRound != null">
              · {{ store.secondsToNextRound }}s 后重扫
            </template>
          </template>
          <template v-else>
            开启后按间隔自动重扫；首轮只建立基线，不报变化。
          </template>
        </div>
      </div>

      <ColorSettings />
      <div class="cping-panel">
        <div style="display: flex; align-items: center; justify-content: space-between">
          <span style="font-size: 13px">仅显示在线</span>
          <NSwitch v-model:value="store.hideOffline" size="small" />
        </div>
        <NButton style="width: 100%; margin-top: 8px" @click="onExport">
          保存结果（CSV / TXT / JSON / HTML）
        </NButton>
        <!-- M10：差异报告 -->
        <NButton
          style="width: 100%; margin-top: 8px"
          :disabled="!store.diffSummary"
          @click="onExportDiff"
        >
          导出本轮差异（CSV）
        </NButton>
        <!-- M7：目标信息补全 与 快照 -->
        <NButton
          style="width: 100%; margin-top: 8px"
          :disabled="store.stats.completed === 0"
          @click="onEnrich"
        >
          补全 MAC / 厂商
        </NButton>
        <NButton
          style="width: 100%; margin-top: 8px"
          :disabled="store.targets.length === 0"
          @click="onSaveSnapshot"
        >
          保存快照
        </NButton>
        <div style="font-size: 11px; color: #6b7280; margin-top: 6px">
          已识别 MAC {{ store.recognized.mac }} · 厂商 {{ store.recognized.vendor }} · 主机名
          {{ store.recognized.hostname }}
        </div>
        <div
          v-if="store.lastExport"
          style="
            font-size: 11px;
            color: #6b7280;
            margin-top: 4px;
            white-space: nowrap;
            overflow: hidden;
            text-overflow: ellipsis;
          "
          :title="store.lastExport"
        >
          上次导出：{{ store.lastExport }}
        </div>
        <div
          v-if="store.lastSnapshot"
          style="
            font-size: 11px;
            color: #6b7280;
            margin-top: 2px;
            white-space: nowrap;
            overflow: hidden;
            text-overflow: ellipsis;
          "
          :title="store.lastSnapshot"
        >
          上次快照：{{ store.lastSnapshot }}
        </div>
      </div>
    </div>

    <div class="desktop__main">
      <NTabs v-model:value="tab" type="line" animated style="height: 100%">
        <NTabPane name="matrix" tab="图形显示" display-directive="show">
          <div style="height: 100%; padding-top: 8px; min-height: 0">
            <!-- M9：目标超过 256 个时矩阵无法表示（一格一 IP 末段），
                 这时明确说明并让用户走表格，而不是悄悄少显示。 -->
            <div v-if="!store.canUseMatrix" class="cping-panel" style="font-size: 12px">
              当前目标 {{ store.stats.total }} 个，超过矩阵视图上限 256 个（一格对应一个 IP 末段），
              无法用图形显示完整覆盖。请切到「表单显示」查看全部结果。
            </div>
            <ResultGrid v-else />
          </div>
        </NTabPane>
        <NTabPane name="table" tab="表单显示" display-directive="show">
          <div style="height: 100%; padding-top: 8px; min-height: 0">
            <ResultTable />
          </div>
        </NTabPane>
        <!-- M8：变化页签。标签上直接带条数，不涨用户来回切换才知道有没有变化。 -->
        <NTabPane name="changes" :tab="`变化 (${store.changes.length})`" display-directive="show">
          <div style="height: 100%; padding-top: 8px; min-height: 0">
            <ChangePanel />
          </div>
        </NTabPane>
      </NTabs>
    </div>

    <StatusBar />
  </div>
</template>
