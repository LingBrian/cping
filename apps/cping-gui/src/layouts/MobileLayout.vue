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
import MobileResultList from '../components/MobileResultList.vue'
import ResultGrid from '../components/ResultGrid.vue'
import TrendPanel from '../components/TrendPanel.vue'
import {
  COMMON_TCP_PORTS,
  MONITOR_INTERVALS,
  type ExportFormatKey,
  type ProbeModeKey,
} from '../shared/types'
import { usePingStore } from '../stores/ping'

const store = usePingStore()
const message = useMessage()

const tab = ref<'list' | 'matrix' | 'changes' | 'trend'>('list')
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
const monitorOptions = MONITOR_INTERVALS.map((s) => ({ label: `${s} 秒`, value: s }))

async function onStart() {
  specError.value = null
  const ok = await store.validateSpec(localSpec.value)
  if (!ok) {
    specError.value = store.specError || '目标无法解析'
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
    message.warning('还没有已完成的结果')
    return
  }
  const path = await store.exportToApp(format)
  if (path) message.success(`已保存：${path}`)
  else message.error(store.status)
}

/** M7：读邻居表回填 MAC / 厂商。 */
async function onEnrich() {
  if (store.stats.completed === 0) {
    message.warning('还没有已完成的结果')
    return
  }
  const msg = await store.enrich()
  if (msg) message.success(msg)
  else message.error(store.status)
}

/** M7：Android 没有文件对话框，由后端写到应用可写目录。 */
async function onSaveSnapshot() {
  if (store.targets.length === 0) {
    message.warning('还没有扫描结果')
    return
  }
  const saved = await store.saveSnapshotToAppDir()
  if (saved) message.success(store.status)
  else message.error(store.status)
}

/** M7：手动再查一次有没有可以恢复的上次结果。 */
async function onCheckRestore() {
  await store.checkPendingSnapshot()
  if (!store.pendingRestore) message.info('没有可恢复的历史结果')
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
</script>

<template>
  <div class="mobile">
    <div class="mobile__header">
      <span class="mobile__title">C网群PING工具</span>
      <div style="display: flex; gap: 8px">
        <NButton size="small" class="touchable" @click="onExport('csv')">导出</NButton>
        <NButton size="small" class="touchable" @click="showSettings = true">设置</NButton>
      </div>
    </div>

    <div class="cping-panel">
      <label class="cping-label">目标（网段 / 单主机 / CIDR）</label>
      <NInput
        v-model:value="localSpec"
        placeholder="192.168.1 或 192.168.1.10"
        inputmode="text"
        :status="specError ? 'error' : undefined"
      />
      <div v-if="specError" style="color: #d03050; font-size: 12px; margin-top: 6px">
        {{ specError }}
      </div>
      <div v-else-if="store.specSummary" style="font-size: 11px; color: #6b7280; margin-top: 6px">
        本次范围：{{ store.specSummary }}
      </div>

      <!-- M9：方式在移动端尤其重要——Android 普通应用没有 CAP_NET_RAW，
           默认只能走 TCP，这是平台事实，不是用户选择。 -->
      <div style="margin-top: 10px">
        <label class="cping-label">探测方式</label>
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
            TCP 只反映端口是否开放，状态显示为 TCP-OPEN，不等价于 ICMP 可达。
          </template>
          <template v-else>ICMP 在 Android 上通常被系统拒绝，会记成「权限不足」。</template>
        </div>
      </div>

      <div class="mobile__actions" style="margin-top: 10px">
        <NButton type="primary" class="touchable" :disabled="!canStart" @click="onStart">
          开始探测
        </NButton>
        <NButton class="touchable" :disabled="!canStop" @click="onStop">停止</NButton>
      </div>

      <div style="margin-top: 10px">
        <div style="display: flex; justify-content: space-between; font-size: 13px">
          <span>已完成 {{ store.stats.completed }}/{{ store.stats.total }}</span>
          <span>在线 <b>{{ store.stats.online }}</b></span>
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
        <span style="font-size: 13px">持续监控</span>
        <NSwitch :value="store.monitorEnabled" size="small" @update:value="store.toggleMonitor()" />
      </div>
      <div v-if="store.monitorEnabled" style="font-size: 11px; color: #6b7280; margin-top: 6px">
        第 {{ store.round }} 轮 · 变化 {{ store.changes.length }} 条
        <template v-if="store.secondsToNextRound != null">
          · {{ store.secondsToNextRound }}s 后重扫
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
          补全 MAC / 厂商
        </NButton>
        <NButton
          class="touchable"
          size="small"
          :disabled="store.targets.length === 0"
          @click="onSaveSnapshot"
        >
          存快照
        </NButton>
      </div>
      <div style="font-size: 11px; color: #6b7280; margin-top: 6px">
        已识别 MAC {{ store.recognized.mac }} · 厂商 {{ store.recognized.vendor }} · 主机名
        {{ store.recognized.hostname }}
      </div>
    </div>

    <div class="mobile__tabs">
      <button
        type="button"
        class="mobile__tab"
        :class="{ 'mobile__tab--active': tab === 'list' }"
        @click="tab = 'list'"
      >
        列表
      </button>
      <button
        type="button"
        class="mobile__tab"
        :class="{ 'mobile__tab--active': tab === 'matrix' }"
        @click="tab = 'matrix'"
      >
        矩阵
      </button>
      <button
        type="button"
        class="mobile__tab"
        :class="{ 'mobile__tab--active': tab === 'changes' }"
        @click="openChanges"
      >
        变化 ({{ store.changes.length }})
      </button>
      <button
        type="button"
        class="mobile__tab"
        :class="{ 'mobile__tab--active': tab === 'trend' }"
        @click="openTrend"
      >
        趋势 ({{ store.trend?.rounds ?? 0 }})
      </button>
      <button
        type="button"
        class="mobile__tab"
        :class="{ 'mobile__tab--active': store.hideOffline }"
        @click="store.hideOffline = !store.hideOffline"
      >
        仅在线
      </button>
    </div>

    <div class="mobile__body">
      <MobileResultList v-if="tab === 'list'" />
      <ChangePanel v-else-if="tab === 'changes'" />
      <TrendPanel v-else-if="tab === 'trend'" :exportable="false" />
      <div v-else-if="!store.canUseMatrix" class="cping-panel" style="font-size: 12px">
        目标 {{ store.stats.total }} 个，超过矩阵上限 256 个，请用列表查看。
      </div>
      <div v-else style="overflow: auto"><ResultGrid /></div>
    </div>

    <div class="mobile__footer">
      <span>{{ store.modeLabel }}</span>
      <span>用时 {{ elapsed }}</span>
      <span>本机 {{ store.localIp }}</span>
      <span style="flex: 1; text-align: right">{{ store.status }}</span>
    </div>

    <NDrawer v-model:show="showSettings" :width="300" placement="right">
      <NDrawerContent title="设置" closable>
        <ColorSettings />
        <div style="display: flex; align-items: center; justify-content: space-between; margin-top: 12px">
          <span style="font-size: 13px">仅显示在线</span>
          <NSwitch v-model:value="store.hideOffline" size="small" />
        </div>
        <div style="display: flex; align-items: center; justify-content: space-between; margin-top: 12px">
          <span style="font-size: 13px">监控间隔</span>
          <NSelect
            :value="store.monitorInterval"
            size="small"
            style="width: 120px"
            :options="monitorOptions"
            @update:value="store.setMonitorInterval"
          />
        </div>
        <div style="display: flex; align-items: center; justify-content: space-between; margin-top: 12px">
          <span style="font-size: 13px">超时 (ms)</span>
          <NInput
            :value="String(store.timeoutMs)"
            size="small"
            style="width: 120px"
            inputmode="numeric"
            @update:value="(v: string) => (store.timeoutMs = Number(v) || 1000)"
          />
        </div>
        <div style="font-size: 11px; color: #6b7280; margin-top: 10px">导出格式</div>
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
          保存设置
        </NButton>
        <NButton style="width: 100%; margin-top: 8px" class="touchable" @click="onCheckRestore">
          查找上次结果
        </NButton>
      </NDrawerContent>
    </NDrawer>
  </div>
</template>
