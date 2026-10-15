<script setup lang="ts">
import { computed, ref } from 'vue'
import {
  NButton,
  NDrawer,
  NDrawerContent,
  NInput,
  NRadioButton,
  NRadioGroup,
  NSelect,
  NSwitch,
  useMessage,
} from 'naive-ui'
import ChangePanel from '../components/ChangePanel.vue'
import ColorSettings from '../components/ColorSettings.vue'
import LanguageSettings from '../components/LanguageSettings.vue'
import MobileResultList from '../components/MobileResultList.vue'
import ResultGrid from '../components/ResultGrid.vue'
import AlertPanel from '../components/AlertPanel.vue'
import TrendPanel from '../components/TrendPanel.vue'
import { t } from '../i18n'
import {
  COMMON_TCP_PORTS,
  MONITOR_INTERVALS,
  type ExportFormatKey,
  type ProbeModeKey,
} from '../shared/types'
import { usePingStore } from '../stores/ping'

const store = usePingStore()
const message = useMessage()

const tab = ref<'list' | 'matrix' | 'changes' | 'trend' | 'alerts'>('list')
const showSettings = ref(false)
const localSpec = ref(store.prefix)
const specError = ref<string | null>(null)

const percent = computed(() => Math.round(store.progress * 100))
const canStart = computed(() => !store.running)
const canStop = computed(() => store.running)
const elapsed = computed(() => `${(store.elapsedMs / 1000).toFixed(2)}s`)
const isTcp = computed(() => store.mode === 'tcp')

const modeOptions = [
  { key: 'icmp' as ProbeModeKey, label: 'ICMP' },
  { key: 'tcp' as ProbeModeKey, label: 'TCP' },
]
const portOptions = COMMON_TCP_PORTS.map((p) => ({ label: `:${p}`, value: p }))
// computed：语言切换后「秒 / s」要立刻跟着变
const monitorOptions = computed(() =>
  MONITOR_INTERVALS.map((s) => ({ label: t('unit.seconds', s), value: s })),
)

async function onStart() {
  specError.value = null
  const ok = await store.validateSpec(localSpec.value)
  if (!ok) {
    specError.value = store.specError || t('status.specUnparsable')
    message.error(specError.value)
    return
  }
  store.prefix = localSpec.value
  await store.start()
}

async function onStop() {
  await store.cancel()
}

/**
 * Android 不走桌面文件对话框：由 Rust 侧写到应用可写的公共目录并回报路径。
 * 真机验证与更完善的分享流程见 docs/platform-compatibility.md（M4）。
 */
async function onExport(format: ExportFormatKey) {
  if (store.stats.completed === 0) {
    message.warning(t('status.noResults'))
    return
  }
  const path = await store.exportToApp(format)
  if (path) message.success(t('status.savedTo', path))
  else message.error(store.status)
}

/** M7：读邻居表回填 MAC / 厂商。 */
async function onEnrich() {
  if (store.stats.completed === 0) {
    message.warning(t('status.noResults'))
    return
  }
  const msg = await store.enrich()
  if (msg) message.success(msg)
  else message.error(store.status)
}

/** M7：Android 没有文件对话框，由后端写到应用可写目录。 */
async function onSaveSnapshot() {
  if (store.targets.length === 0) {
    message.warning(t('msg.noScanResults'))
    return
  }
  const saved = await store.saveSnapshotToAppDir()
  if (saved) message.success(store.status)
  else message.error(store.status)
}

/** M7：手动再查一次有没有可以恢复的上次结果。 */
async function onCheckRestore() {
  await store.checkPendingSnapshot()
  if (!store.pendingRestore) message.info(t('msg.noRestore'))
}

function openChanges() {
  tab.value = 'changes'
  void store.loadChanges()
}

/** M14：切到趋势页并拉一次数据（不做轮询）。 */
function openTrend() {
  tab.value = 'trend'
  void store.loadTrend()
}

/** M15：切到告警页并拉一次状态（不做轮询）。 */
function openAlerts() {
  tab.value = 'alerts'
  void store.loadAlerts()
}
</script>

<template>
  <div class="mobile">
    <div class="mobile__header">
      <span class="mobile__title">{{ t('app.title') }}</span>
      <div style="display: flex; gap: 8px">
        <NButton size="small" class="touchable" @click="onExport('csv')">{{ t('btn.export') }}</NButton>
        <NButton size="small" class="touchable" @click="showSettings = true">{{ t('btn.settings') }}</NButton>
      </div>
    </div>

    <div class="cping-panel">
      <label class="cping-label">{{ t('label.targetShort') }}</label>
      <NInput
        v-model:value="localSpec"
        :placeholder="t('placeholder.targetShort')"
        inputmode="text"
        :status="specError ? 'error' : undefined"
      />
      <div v-if="specError" style="color: #d03050; font-size: 12px; margin-top: 6px">
        {{ specError }}
      </div>
      <div v-else-if="store.specSummary" style="font-size: 11px; color: #6b7280; margin-top: 6px">
        {{ t('label.specRange', store.specSummary) }}
      </div>

      <!-- M9：方式在移动端尤其重要——Android 普通应用没有 CAP_NET_RAW，
           默认只能走 TCP，这是平台事实，不是用户选择。 -->
      <div style="margin-top: 10px">
        <label class="cping-label">{{ t('label.mode') }}</label>
        <div style="display: flex; gap: 8px; align-items: center">
          <NRadioGroup :value="store.mode" size="small" @update:value="store.setMode">
            <NRadioButton v-for="m in modeOptions" :key="m.key" :value="m.key">
              {{ m.label }}
            </NRadioButton>
          </NRadioGroup>
          <NSelect
            v-if="isTcp"
            v-model:value="store.tcpPort"
            size="small"
            style="width: 110px"
            :options="portOptions"
          />
        </div>
        <div style="font-size: 11px; color: #6b7280; margin-top: 4px">
          <template v-if="isTcp">
            {{ t('hint.tcpMobile') }}
          </template>
          <template v-else>{{ t('hint.icmpMobile') }}</template>
        </div>
      </div>

      <div class="mobile__actions" style="margin-top: 10px">
        <NButton type="primary" class="touchable" :disabled="!canStart" @click="onStart">
          {{ t('btn.start') }}
        </NButton>
        <NButton class="touchable" :disabled="!canStop" @click="onStop">{{ t('btn.stop') }}</NButton>
      </div>

      <div style="margin-top: 10px">
        <div style="display: flex; justify-content: space-between; font-size: 13px">
          <span>{{ t('progress.completedShort', store.stats.completed, store.stats.total) }}</span>
          <span>{{ t('progress.online', store.stats.online) }}</span>
        </div>
        <div
          style="height: 8px; background: #e5e7eb; border-radius: 4px; margin-top: 6px; overflow: hidden"
        >
          <div
            :style="{ width: `${percent}%`, height: '100%', background: '#111827', transition: 'width .2s' }"
          />
        </div>
      </div>

      <!-- M8：持续监控 -->
      <div
        style="display: flex; align-items: center; justify-content: space-between; margin-top: 10px"
      >
        <span style="font-size: 13px">{{ t('monitor.title') }}</span>
        <NSwitch :value="store.monitorEnabled" size="small" @update:value="store.toggleMonitor()" />
      </div>
      <div v-if="store.monitorEnabled" style="font-size: 11px; color: #6b7280; margin-top: 6px">
        {{ t('monitor.mobileStatus', store.round, store.changes.length) }}
        <template v-if="store.secondsToNextRound != null">
          {{ t('monitor.rescanIn', store.secondsToNextRound) }}
        </template>
      </div>

      <!-- M7：目标信息补全（MAC / 厂商）。主机名按行按需反查，见下拉列表。 -->
      <div class="mobile__actions" style="margin-top: 10px">
        <NButton
          class="touchable"
          size="small"
          :disabled="store.stats.completed === 0"
          @click="onEnrich"
        >
          {{ t('btn.enrich') }}
        </NButton>
        <NButton
          class="touchable"
          size="small"
          :disabled="store.targets.length === 0"
          @click="onSaveSnapshot"
        >
          {{ t('btn.saveSnapshotShort') }}
        </NButton>
      </div>
      <div style="font-size: 11px; color: #6b7280; margin-top: 6px">
        {{ t('recognized.summary', store.recognized.mac, store.recognized.vendor, store.recognized.hostname) }}
      </div>
    </div>

    <div class="mobile__tabs">
      <button
        type="button"
        class="mobile__tab"
        :class="{ 'mobile__tab--active': tab === 'list' }"
        @click="tab = 'list'"
      >
        {{ t('tab.list') }}
      </button>
      <button
        type="button"
        class="mobile__tab"
        :class="{ 'mobile__tab--active': tab === 'matrix' }"
        @click="tab = 'matrix'"
      >
        {{ t('tab.matrixShort') }}
      </button>
      <button
        type="button"
        class="mobile__tab"
        :class="{ 'mobile__tab--active': tab === 'changes' }"
        @click="openChanges"
      >
        {{ t('tab.changes', store.changes.length) }}
      </button>
      <button
        type="button"
        class="mobile__tab"
        :class="{ 'mobile__tab--active': tab === 'trend' }"
        @click="openTrend"
      >
        {{ t('tab.trendShort', store.trend?.rounds ?? 0) }}
      </button>
      <button
        type="button"
        class="mobile__tab"
        :class="{ 'mobile__tab--active': tab === 'alerts' }"
        @click="openAlerts"
      >
        {{ t('tab.alerts', store.alertCount) }}
      </button>
      <button
        type="button"
        class="mobile__tab"
        :class="{ 'mobile__tab--active': store.hideOffline }"
        @click="store.hideOffline = !store.hideOffline"
      >
        {{ t('tab.onlineOnly') }}
      </button>
    </div>

    <div class="mobile__body">
      <MobileResultList v-if="tab === 'list'" />
      <ChangePanel v-else-if="tab === 'changes'" />
      <TrendPanel v-else-if="tab === 'trend'" :exportable="false" />
      <AlertPanel v-else-if="tab === 'alerts'" />
      <div v-else-if="!store.canUseMatrix" class="cping-panel" style="font-size: 12px">
        {{ t('matrix.overflowMobile', store.stats.total) }}
      </div>
      <div v-else style="overflow: auto"><ResultGrid /></div>
    </div>

    <div class="mobile__footer">
      <span>{{ store.modeLabel }}</span>
      <span>{{ t('common.elapsed', elapsed) }}</span>
      <span>{{ t('common.localIp', store.localIp) }}</span>
      <span style="flex: 1; text-align: right">{{ store.status }}</span>
    </div>

    <NDrawer v-model:show="showSettings" :width="300" placement="right">
      <NDrawerContent :title="t('btn.settings')" closable>
        <ColorSettings />
        <LanguageSettings />
        <div style="display: flex; align-items: center; justify-content: space-between; margin-top: 12px">
          <span style="font-size: 13px">{{ t('common.onlineOnly') }}</span>
          <NSwitch v-model:value="store.hideOffline" size="small" />
        </div>
        <div style="display: flex; align-items: center; justify-content: space-between; margin-top: 12px">
          <span style="font-size: 13px">{{ t('monitor.interval') }}</span>
          <NSelect
            :value="store.monitorInterval"
            size="small"
            style="width: 120px"
            :options="monitorOptions"
            @update:value="store.setMonitorInterval"
          />
        </div>
        <div style="display: flex; align-items: center; justify-content: space-between; margin-top: 12px">
          <span style="font-size: 13px">{{ t('label.timeout') }}</span>
          <NInput
            :value="String(store.timeoutMs)"
            size="small"
            style="width: 120px"
            inputmode="numeric"
            @update:value="(v: string) => (store.timeoutMs = Number(v) || 1000)"
          />
        </div>
        <div style="font-size: 11px; color: #6b7280; margin-top: 10px">{{ t('label.exportFormat') }}</div>
        <div class="mobile__actions" style="margin-top: 6px">
          <NButton size="small" class="touchable" @click="onExport('csv')">CSV</NButton>
          <NButton size="small" class="touchable" @click="onExport('txt')">TXT</NButton>
        </div>
        <div class="mobile__actions" style="margin-top: 6px">
          <NButton size="small" class="touchable" @click="onExport('json')">JSON</NButton>
          <NButton size="small" class="touchable" @click="onExport('html')">HTML</NButton>
        </div>
        <NButton
          style="width: 100%; margin-top: 12px"
          class="touchable"
          @click="store.persistConfig()"
        >
          {{ t('btn.saveConfig') }}
        </NButton>
        <NButton style="width: 100%; margin-top: 8px" class="touchable" @click="onCheckRestore">
          {{ t('btn.findLastResult') }}
        </NButton>
      </NDrawerContent>
    </NDrawer>
  </div>
</template>
