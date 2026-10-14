<script setup lang="ts">
import { computed, ref } from 'vue'
import { NButton, NDrawer, NDrawerContent, NInput, NSwitch, useMessage } from 'naive-ui'
import ColorSettings from '../components/ColorSettings.vue'
import MobileResultList from '../components/MobileResultList.vue'
import ResultGrid from '../components/ResultGrid.vue'
import * as svc from '../services/ping'
import { usePingStore } from '../stores/ping'

const store = usePingStore()
const message = useMessage()

const tab = ref<'list' | 'matrix'>('list')
const showSettings = ref(false)
const localPrefix = ref(store.prefix)

const percent = computed(() => Math.round(store.progress * 100))
const canStart = computed(() => !store.running)
const canStop = computed(() => store.running)
const elapsed = computed(() => `${(store.elapsedMs / 1000).toFixed(2)}s`)

async function onStart() {
  try {
    await svc.parsePrefix(localPrefix.value)
    store.prefix = localPrefix.value
  } catch (e) {
    message.error(String(e))
    return
  }
  await store.start()
}

async function onStop() {
  await store.cancel()
}

/**
 * Android 不走桌面文件对话框：由 Rust 侧写到应用可写的公共目录并回报路径。
 * 真机验证与更完善的分享流程见 docs/platform-compatibility.md（M4）。
 */
async function onExport(format: 'csv' | 'txt') {
  if (store.stats.completed === 0) {
    message.warning('还没有已完成的结果')
    return
  }
  try {
    const path = await svc.exportToAppDir(format)
    message.success(`已保存：${path}`)
    store.lastExport = path
  } catch (e) {
    message.error(String(e))
  }
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
      <label class="cping-label">网段前缀（C 类前三段）</label>
      <NInput v-model:value="localPrefix" placeholder="192.168.1" inputmode="numeric" />
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
        :class="{ 'mobile__tab--active': store.hideOffline }"
        @click="store.hideOffline = !store.hideOffline"
      >
        仅在线
      </button>
    </div>

    <div class="mobile__body">
      <MobileResultList v-if="tab === 'list'" />
      <div v-else style="overflow: auto"><ResultGrid /></div>
    </div>

    <div class="mobile__footer">
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
        <NButton style="width: 100%; margin-top: 12px" class="touchable" @click="store.persistConfig()">
          保存设置
        </NButton>
        <NButton style="width: 100%; margin-top: 8px" class="touchable" @click="onCheckRestore">
          查找上次结果
        </NButton>
      </NDrawerContent>
    </NDrawer>
  </div>
</template>
