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
