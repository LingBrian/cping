<script setup lang="ts">
import { computed, onMounted } from 'vue'
import { NConfigProvider, NMessageProvider, dateZhCN, zhCN } from 'naive-ui'
import DesktopLayout from './layouts/DesktopLayout.vue'
import MobileLayout from './layouts/MobileLayout.vue'
import RestoreSnapshotDialog from './components/RestoreSnapshotDialog.vue'
import { usePingStore } from './stores/ping'

const store = usePingStore()

/**
 * Android 使用独立的移动布局：不缩小桌面界面，而是列表优先的触控界面。
 * 这里同时看 UA 与指针精度，避免平板被误判。
 */
const isMobile = computed(() => {
  if (typeof navigator === 'undefined') return false
  const ua = navigator.userAgent
  const coarse =
    typeof window !== 'undefined' && window.matchMedia?.('(pointer: coarse)').matches === true
  return /Android|iPhone|iPad|iPod|Mobile/i.test(ua) || (coarse && window.innerWidth < 900)
})

const themeOverrides = {
  common: {
    primaryColor: '#111827',
    primaryColorHover: '#1f2937',
    primaryColorPressed: '#0b1220',
    borderRadius: '8px',
  },
}

onMounted(async () => {
  await store.loadConfig()
  await store.attachEvents()
  await store.loadLocalAddresses()
  // M7：进程随时可能被系统回收（Android 上销毁 Activity 即退出进程），
  // 所以启动时先看看有没有上次没处理完的结果。只检查、不自动恢复。
  await store.checkPendingSnapshot()
})
</script>

<template>
  <!-- 两层 provider 都要撑满高度，否则 .desktop 的 height:100% 解析为 auto，
       窗口一矮整页就会滚动、底部状态栏被挤出可视区 -->
  <n-config-provider
    class="app-shell"
    :locale="zhCN"
    :date-locale="dateZhCN"
    :theme-overrides="themeOverrides"
  >
    <n-message-provider class="app-shell">
      <MobileLayout v-if="isMobile" />
      <DesktopLayout v-else />
      <!-- 两种布局共用同一个确认框；它自己判断有没有要问的事 -->
      <RestoreSnapshotDialog />
    </n-message-provider>
  </n-config-provider>
</template>
