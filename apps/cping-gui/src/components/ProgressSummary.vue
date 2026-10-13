<script setup lang="ts">
import { computed } from 'vue'
import { NProgress } from 'naive-ui'
import { usePingStore } from '../stores/ping'

const store = usePingStore()

const percent = computed(() => Math.round(store.progress * 100))
const elapsed = computed(() => `${(store.elapsedMs / 1000).toFixed(2)}s`)
</script>

<template>
  <div class="cping-panel">
    <div style="display: flex; justify-content: space-between; font-size: 12px; margin-bottom: 6px">
      <span>已完成 {{ store.stats.completed }} / {{ store.stats.total }}</span>
      <span>在线 <b>{{ store.stats.online }}</b></span>
    </div>
    <NProgress
      type="line"
      :percentage="percent"
      :height="10"
      :show-indicator="false"
      :status="store.cancelled ? 'warning' : store.running ? 'info' : 'success'"
    />
    <div style="display: flex; justify-content: space-between; font-size: 12px; color: #6b7280; margin-top: 6px">
      <span>用时 {{ elapsed }}</span>
      <span>超时 {{ store.stats.timeout }} · 不可达 {{ store.stats.unreachable }} · 错误 {{ store.stats.error }}</span>
    </div>
  </div>
</template>
