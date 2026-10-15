<script setup lang="ts">
import { computed, ref, watch } from 'vue'
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
import FingerprintPanel from '../components/FingerprintPanel.vue'
import LanguageSettings from '../components/LanguageSettings.vue'
import PrefixForm from '../components/PrefixForm.vue'
import ProgressSummary from '../components/ProgressSummary.vue'
import ResultGrid from '../components/ResultGrid.vue'
import ResultTable from '../components/ResultTable.vue'
import StatusBar from '../components/StatusBar.vue'
import AlertPanel from '../components/AlertPanel.vue'
import TrendPanel from '../components/TrendPanel.vue'
import { t } from '../i18n'
import { MONITOR_INTERVALS, type ExportFormatKey } from '../shared/types'
import { usePingStore } from '../stores/ping'

const store = usePingStore()
const message = useMessage()
const tab = ref<'matrix' | 'table' | 'changes' | 'assets' | 'trend' | 'alerts'>(
  store.view === 'table' ? 'table' : 'matrix',
)

watch(tab, (v) => {
  // 变化页签不是一个「视图偏好」，所以不写进配置
  if (v === 'matrix' || v === 'table') store.view = v
  // M14：切到趋势页时拉一次数据（不做轮询，扫描结束时后端会顺带刷新）
  if (v === 'trend') void store.loadTrend()
  // M15：告警页同理——一处拉取，不做轮询
  if (v === 'alerts') void store.loadAlerts()
})

// 用 computed 而不是常量：语言切换后下拉选项里的「秒 / s」要立刻跟着变
const monitorOptions = computed(() =>
  MONITOR_INTERVALS.map((s) => ({ label: t('unit.seconds', s), value: s })),
)

/**
 * 导出格式由扩展名决定，和 Rust 侧的 `ExportFormat::from_path` 一一对应。
 * 这里的 name 只是文件对话框里的过滤器标题（给人看的），扩展名才是契约。
 */
const exportFilters = computed(() => [
  { name: t('filter.csv'), extensions: ['csv'] },
  { name: t('filter.txt'), extensions: ['txt'] },
  { name: t('filter.json'), extensions: ['json'] },
  { name: t('filter.html'), extensions: ['html'] },
])

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
    message.warning(t('status.noResults'))
    return
  }
  const path = await save({
    defaultPath: `${fileStem()}.csv`,
    filters: exportFilters.value,
  })
  if (!path) return
  const fmt = formatOf(path)
  if (!fmt) {
    message.error(t('msg.exportExt'))
    return
  }
  await store.exportTo(path)
  // 导出成功的提示是**后端**返回的成品文案，这里只能按它的中文前缀判断成败。
  // 该判断与界面语言无关（后端文案不随前端语言变化），所以保持原样，避免改动行为。
  if (store.status.startsWith('已导出')) message.success(store.status)
  else message.error(store.status)
}

/** M10：导出「上一轮 → 本轮」差异（只有 .csv）。 */
async function onExportDiff() {
  if (!store.diffSummary) {
    message.warning(t('msg.noDiffBaseline'))
    return
  }
  const path = await save({
    defaultPath: `${fileStem()}-diff.csv`,
    filters: [{ name: t('filter.csv'), extensions: ['csv'] }],
  })
  if (!path) return
  if (await store.exportDiffTo(path)) message.success(store.status)
  else message.error(store.status)
}

/** M7：读本机邻居表回填 MAC，并据 IEEE 注册数据补厂商。 */
async function onEnrich() {
  if (store.stats.completed === 0) {
    message.warning(t('status.noResults'))
    return
  }
  const msg = await store.enrich()
  if (msg) message.success(msg)
  else message.error(store.status)
}

/** M13：对在线主机做端口指纹。**不动 results**：端口全关的主机照样是在线。 */
async function onFingerprint() {
  if (store.stats.online === 0) {
    message.warning(t('msg.noOnlineForFp'))
    return
  }
  const msg = await store.runFingerprint(null)
  if (msg) {
    message.success(msg)
    tab.value = 'assets'
  } else {
    message.error(store.status)
  }
}

/** M14：切到趋势页并拉一次数据。已在趋势页时 watch 不会触发，所以这里显式拉。 */
async function onShowTrend() {
  tab.value = 'trend'
  await store.loadTrend()
}

/** M15：切到告警页并拉一次状态。 */
async function onShowAlerts() {
  tab.value = 'alerts'
  await store.loadAlerts()
}

/** M7：把当前结果存成快照，下次启动可以直接恢复。 */
async function onSaveSnapshot() {
  if (store.targets.length === 0) {
    message.warning(t('msg.noScanResults'))
    return
  }
  const path = await save({
    defaultPath: `${fileStem()}.snap`,
    filters: [{ name: t('filter.snapshot'), extensions: ['snap'] }],
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
          <span style="font-size: 13px">{{ t('monitor.title') }}</span>
          <NSwitch :value="store.monitorEnabled" size="small" @update:value="store.toggleMonitor()" />
        </div>
        <div style="display: flex; align-items: center; gap: 8px; margin-top: 8px">
          <span style="font-size: 12px; color: #6b7280">{{ t('monitor.interval') }}</span>
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
            {{ t('monitor.status', store.round, store.changes.length, store.changeTotal) }}
            <template v-if="store.secondsToNextRound != null">
              {{ t('monitor.rescanIn', store.secondsToNextRound) }}
            </template>
          </template>
          <template v-else>
            {{ t('monitor.hint') }}
          </template>
        </div>
      </div>

      <ColorSettings />
      <LanguageSettings />
      <div class="cping-panel">
        <div style="display: flex; align-items: center; justify-content: space-between">
          <span style="font-size: 13px">{{ t('common.onlineOnly') }}</span>
          <NSwitch v-model:value="store.hideOffline" size="small" />
        </div>
        <NButton style="width: 100%; margin-top: 8px" @click="onExport">
          {{ t('btn.saveResults') }}
        </NButton>
        <!-- M10：差异报告 -->
        <NButton
          style="width: 100%; margin-top: 8px"
          :disabled="!store.diffSummary"
          @click="onExportDiff"
        >
          {{ t('btn.exportDiff') }}
        </NButton>
        <!-- M7：目标信息补全 与 快照 -->
        <NButton
          style="width: 100%; margin-top: 8px"
          :disabled="store.stats.completed === 0"
          @click="onEnrich"
        >
          {{ t('btn.enrich') }}
        </NButton>
        <NButton
          style="width: 100%; margin-top: 8px"
          :disabled="store.targets.length === 0"
          @click="onSaveSnapshot"
        >
          {{ t('btn.saveSnapshot') }}
        </NButton>
        <!-- M13：资产识别。按钮文案直说边界，避免被误读成「再扫一遍在线」。 -->
        <NButton
          style="width: 100%; margin-top: 8px"
          :loading="store.fingerprintBusy"
          :disabled="store.stats.online === 0"
          @click="onFingerprint"
        >
          {{ t('btn.fingerprint') }}
        </NButton>
        <!-- M14：历史与趋势。没有历史也允许打开——空状态会说明怎么攒数据。 -->
        <NButton style="width: 100%; margin-top: 8px" @click="onShowTrend">
          {{ t('btn.trend') }}
        </NButton>
        <!-- M15：告警。文案直接写明「默认关闭」——不问就发通知比不装还糟。 -->
        <NButton style="width: 100%; margin-top: 8px" @click="onShowAlerts">
          {{ store.alertCount > 0 ? t('btn.alertsWithCount', store.alertCount) : t('btn.alertsOff') }}
        </NButton>
        <div style="font-size: 11px; color: #6b7280; margin-top: 6px">
          {{ t('recognized.summary', store.recognized.mac, store.recognized.vendor, store.recognized.hostname) }}
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
          {{ t('last.export', store.lastExport) }}
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
          {{ t('last.snapshot', store.lastSnapshot) }}
        </div>
      </div>
    </div>

    <div class="desktop__main">
      <NTabs v-model:value="tab" type="line" animated style="height: 100%">
        <NTabPane name="matrix" :tab="t('tab.matrix')" display-directive="show">
          <div style="height: 100%; padding-top: 8px; min-height: 0">
            <!-- M9：目标超过 256 个时矩阵无法表示（一格一 IP 末段），
                 这时明确说明并让用户走表格，而不是悄悄少显示。 -->
            <div v-if="!store.canUseMatrix" class="cping-panel" style="font-size: 12px">
              {{ t('matrix.overflow', store.stats.total) }}
            </div>
            <ResultGrid v-else />
          </div>
        </NTabPane>
        <NTabPane name="table" :tab="t('tab.table')" display-directive="show">
          <div style="height: 100%; padding-top: 8px; min-height: 0">
            <ResultTable />
          </div>
        </NTabPane>
        <!-- M8：变化页签。标签上直接带条数，不涨用户来回切换才知道有没有变化。 -->
        <NTabPane name="changes" :tab="t('tab.changes', store.changes.length)" display-directive="show">
          <div style="height: 100%; padding-top: 8px; min-height: 0">
            <ChangePanel />
          </div>
        </NTabPane>
        <!-- M13：资产识别。标签带主机数，与「变化」页签同一套提示逻辑。 -->
        <NTabPane
          name="assets"
          :tab="t('tab.assets', store.fingerprintedHosts)"
          display-directive="show"
        >
          <div style="height: 100%; padding-top: 8px; min-height: 0">
            <FingerprintPanel />
          </div>
        </NTabPane>
        <!-- M14：历史与趋势。标签带轮数——用户一眼能看出「攒了多少账」。 -->
        <NTabPane
          name="trend"
          :tab="t('tab.trend', store.trend?.rounds ?? 0)"
          display-directive="show"
        >
          <div style="height: 100%; padding-top: 8px; min-height: 0">
            <TrendPanel />
          </div>
        </NTabPane>
        <!-- M15：告警。标签带条数——攒下的告警一眼可见。 -->
        <NTabPane
          name="alerts"
          :tab="t('tab.alerts', store.alertCount)"
          display-directive="show"
        >
          <div style="height: 100%; padding-top: 8px; min-height: 0">
            <AlertPanel />
          </div>
        </NTabPane>
      </NTabs>
    </div>

    <StatusBar />
  </div>
</template>
