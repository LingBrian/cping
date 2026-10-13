<script setup lang="ts">
import { computed, onMounted } from 'vue'
import { NConfigProvider, NMessageProvider, dateZhCN, zhCN } from 'naive-ui'
import DesktopLayout from './layouts/DesktopLayout.vue'
import MobileLayout from './layouts/MobileLayout.vue'
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
})
</script>

<template>
  <n-config-provider :locale="zhCN" :date-locale="dateZhCN" :theme-overrides="themeOverrides">
    <n-message-provider>
      <MobileLayout v-if="isMobile" />
      <DesktopLayout v-else />
    </n-message-provider>
  </n-config-provider>
</template>
