<script setup lang="ts">
import { computed } from 'vue'
import { NProgress } from 'naive-ui'
import { t } from '../i18n'
import { usePingStore } from '../stores/ping'

const store = usePingStore()

const percent = computed(() => Math.round(store.progress * 100))
const elapsed = computed(() => `${(store.elapsedMs / 1000).toFixed(2)}s`)
</script>

<template>
  <div class="cping-panel">
    <div style="display: flex; justify-content: space-between; font-size: 12px; margin-bottom: 4px">
      <span>{{ t('progress.completed', store.stats.completed, store.stats.total) }}</span>
      <span>{{ t('progress.online', store.stats.online) }}</span>
    </div>
    <NProgress
      type="line"
      :percentage="percent"
      :height="10"
      :show-indicator="false"
      :status="store.cancelled ? 'warning' : store.running ? 'info' : 'success'"
    />
    <div style="display: flex; justify-content: space-between; font-size: 12px; color: #6b7280; margin-top: 4px">
      <span>{{ t('common.elapsed', elapsed) }}</span>
      <span>{{ t('progress.detail', store.stats.timeout, store.stats.unreachable, store.stats.error) }}</span>
    </div>
  </div>
</template>
