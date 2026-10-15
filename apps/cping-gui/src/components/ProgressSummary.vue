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
    <!-- 进度是这块面板里唯一会自己变的内容。`role="status"`（隐式
         `aria-live="polite"`）让读屏器在扫描推进时自动播报，用不着用户
         反复回来查；写 `aria-live` 显式一次是为了不依赖角色默认值的实现差异。 -->
    <div
      role="status"
      aria-live="polite"
      style="display: flex; justify-content: space-between; font-size: 12px; margin-bottom: 4px"
    >
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
