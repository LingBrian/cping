<script setup lang="ts">
import { ref, watch } from 'vue'
import { NButton, NSwitch, NTabPane, NTabs, useMessage } from 'naive-ui'
import { save } from '@tauri-apps/plugin-dialog'
import ColorSettings from '../components/ColorSettings.vue'
import PrefixForm from '../components/PrefixForm.vue'
import ProgressSummary from '../components/ProgressSummary.vue'
import ResultGrid from '../components/ResultGrid.vue'
import ResultTable from '../components/ResultTable.vue'
import StatusBar from '../components/StatusBar.vue'
import { usePingStore } from '../stores/ping'

const store = usePingStore()
const message = useMessage()
const tab = ref<'matrix' | 'table'>(store.view === 'table' ? 'table' : 'matrix')

watch(tab, (v) => {
  store.view = v
})

async function onExport() {
  if (store.stats.completed === 0) {
    message.warning('还没有已完成的结果')
    return
  }
  const path = await save({
    defaultPath: `cping-${store.prefix.replace(/\./g, '_')}.csv`,
    filters: [
      { name: 'CSV 文件', extensions: ['csv'] },
      { name: '文本文件', extensions: ['txt'] },
    ],
  })
  if (!path) return
  await store.exportTo(path)
  if (store.status.startsWith('已导出')) {
    message.success(store.status)
  } else {
    message.error(store.status)
  }
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
    defaultPath: `cping-${store.prefix.replace(/\./g, '_')}.snap`,
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
      <ColorSettings />
      <div class="cping-panel">
        <div style="display: flex; align-items: center; justify-content: space-between">
          <span style="font-size: 13px">仅显示在线</span>
          <NSwitch v-model:value="store.hideOffline" size="small" />
        </div>
        <NButton style="width: 100%; margin-top: 8px" @click="onExport">保存结果（CSV / TXT）</NButton>
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
            <ResultGrid />
          </div>
        </NTabPane>
        <NTabPane name="table" tab="表单显示" display-directive="show">
          <div style="height: 100%; padding-top: 8px; min-height: 0">
            <ResultTable />
          </div>
        </NTabPane>
      </NTabs>
    </div>

    <StatusBar />
  </div>
</template>
