<script setup lang="ts">
import { computed } from 'vue'
import { usePingStore } from '../stores/ping'

const store = usePingStore()

const elapsed = computed(() => `${(store.elapsedMs / 1000).toFixed(2)} s`)
const state = computed(() => {
  if (store.running) return '探测中'
  if (store.cancelled) return '已取消'
  if (store.stats.completed > 0) return '已完成'
  return '空闲'
})

/** M8：监控倒计时文案。只有在开启监控且空闲时才出现，避免和「探测中」语义打架。 */
const monitorText = computed(() => {
  if (!store.monitorEnabled) return null
  const s = store.secondsToNextRound
  if (store.running || s == null) return `监控中 · 每 ${store.monitorInterval}s`
  return `监控中 · ${s}s 后重扫`
})
</script>

<template>
  <div class="cping-panel desktop__footer" style="display: flex; flex-wrap: wrap; gap: 18px; font-size: 12px">
    <span>状态：<b>{{ state }}</b></span>
    <!-- M9：方式必须一直可见——它决定了「在线」的含义 -->
    <span>方式：<b>{{ store.modeLabel }}</b></span>
    <span>第 <b>{{ store.round }}</b> 轮</span>
    <span>已完成：{{ store.stats.completed }}/{{ store.stats.total }}</span>
    <span>在线主机数：<b>{{ store.stats.online }}</b></span>
    <!-- M8：变化计数与摘要。没有变化时明确显示 0，不留空 -->
    <span>
      变化：<b>{{ store.changes.length }}</b>
      <span v-if="store.changeTotal > store.changes.length" style="color: #9ca3af">
        / 累计 {{ store.changeTotal }}
      </span>
    </span>
    <span v-if="monitorText">{{ monitorText }}</span>
    <!-- M15：有告警才出现。写「告警 0 条」是纯噪音，不如不显示。 -->
    <span v-if="store.alertNote" style="color: #b45309">
      <b>{{ store.alertNote }}</b>
    </span>
    <!-- M7：认出了多少台如实报出来；数字直接数结果，不另外维护一份状态 -->
    <span>已识别 MAC：{{ store.recognized.mac }}</span>
    <span>厂商：{{ store.recognized.vendor }}</span>
    <span>主机名：{{ store.recognized.hostname }}</span>
    <span>用时：{{ elapsed }}</span>
    <span>后端：{{ store.taskId ? 'cping-core' : '待启动' }}</span>
    <span>本机 IP：{{ store.localIp }}</span>
    <span style="color: #6b7280; margin-left: auto">{{ store.status }}</span>
  </div>
</template>
