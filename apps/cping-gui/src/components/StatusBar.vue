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
</script>

<template>
  <div class="cping-panel desktop__footer" style="display: flex; flex-wrap: wrap; gap: 18px; font-size: 12px">
    <span>状态：<b>{{ state }}</b></span>
    <span>已完成：{{ store.stats.completed }}/{{ store.stats.total }}</span>
    <span>在线主机数：<b>{{ store.stats.online }}</b></span>
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
