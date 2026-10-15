<script setup lang="ts">
import { computed, onMounted } from 'vue'
import {
  NConfigProvider,
  NMessageProvider,
  dateEnUS,
  dateZhCN,
  enUS,
  zhCN,
} from 'naive-ui'
import DesktopLayout from './layouts/DesktopLayout.vue'
import MobileLayout from './layouts/MobileLayout.vue'
import RestoreSnapshotDialog from './components/RestoreSnapshotDialog.vue'
import { lang } from './i18n'
import { usePingStore } from './stores/ping'

const store = usePingStore()

// Naive UI 自带组件的内置文案（日期、分页、空态等）也要跟着界面语言走，
// 否则会出现「外壳是英文、抽屉里的日期选择器还是中文」这种半截状态。
const naiveLocale = computed(() => (lang.value === 'en' ? enUS : zhCN))
const naiveDateLocale = computed(() => (lang.value === 'en' ? dateEnUS : dateZhCN))

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
  // M12：拉一次后端能力，用于「设置」里如实说明当前平台能做什么（含不能做的）。
  await store.loadBackendCaps()
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
    :locale="naiveLocale"
    :date-locale="naiveDateLocale"
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
