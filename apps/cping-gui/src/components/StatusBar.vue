<script setup lang="ts">
import { computed } from 'vue'
import { t } from '../i18n'
import { usePingStore } from '../stores/ping'

const store = usePingStore()

const elapsed = computed(() => `${(store.elapsedMs / 1000).toFixed(2)} s`)
const state = computed(() => {
  if (store.running) return t('state.running')
  if (store.cancelled) return t('state.cancelled')
  if (store.stats.completed > 0) return t('state.done')
  return t('state.idle')
})

/** M8：监控倒计时文案。只有在开启监控且空闲时才出现，避免和「探测中」语义打架。 */
const monitorText = computed(() => {
  if (!store.monitorEnabled) return null
  const s = store.secondsToNextRound
  if (store.running || s == null) return t('state.monitoring', store.monitorInterval)
  return t('state.monitoringRescan', s)
})
</script>

<template>
  <div class="cping-panel desktop__footer" style="display: flex; flex-wrap: wrap; gap: 18px; font-size: 12px">
    <span>{{ t('status.label') }}<b>{{ state }}</b></span>
    <!-- M9：方式必须一直可见——它决定了「在线」的含义 -->
    <span>{{ t('status.mode') }}<b>{{ store.modeLabel }}</b></span>
    <span>{{ t('round.n', store.round) }}</span>
    <!-- 进度数字会自己变（扫描过程中每完成一个目标就+1），所以给它
         `role="status"` + `aria-live="polite"`：读屏器会在停顿处播报，
         用户不必反复回来查「跑到哪了」。用 polite 而不是 assertive，
         是因为这不是错误，不该打断正在朗读的内容。 -->
    <span role="status" aria-live="polite">{{
      t('progress.completed', store.stats.completed, store.stats.total)
    }}</span>
    <span>{{ t('status.online') }}<b>{{ store.stats.online }}</b></span>
    <!-- M8：变化计数与摘要。没有变化时明确显示 0，不留空 -->
    <span>
      {{ t('status.changes') }}<b>{{ store.changes.length }}</b>
      <span v-if="store.changeTotal > store.changes.length" style="color: #9ca3af">
        {{ t('status.changeTotal', store.changeTotal) }}
      </span>
    </span>
    <span v-if="monitorText">{{ monitorText }}</span>
    <!-- M15：有告警才出现。写「告警 0 条」是纯噪音，不如不显示。 -->
    <span v-if="store.alertNote" style="color: #b45309">
      <b>{{ store.alertNote }}</b>
    </span>
    <!-- M7：认出了多少台如实报出来；数字直接数结果，不另外维护一份状态 -->
    <span>{{ t('status.mac') }}{{ store.recognized.mac }}</span>
    <span>{{ t('status.vendor') }}{{ store.recognized.vendor }}</span>
    <span>{{ t('status.hostname') }}{{ store.recognized.hostname }}</span>
    <span>{{ t('status.elapsed') }}{{ elapsed }}</span>
    <span>{{ t('status.backend') }}{{ store.taskId ? 'cping-core' : t('status.notStarted') }}</span>
    <span>{{ t('status.localIp') }}{{ store.localIp }}</span>
    <span style="color: #6b7280; margin-left: auto">{{ store.status }}</span>
  </div>
</template>
