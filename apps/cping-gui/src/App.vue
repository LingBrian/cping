<script setup lang="ts">
import { computed, onMounted } from 'vue'
import {
  NConfigProvider,
  NMessageProvider,
  darkTheme,
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

/**
 * 默认主题下的 naive 覆盖：与改动前**逐字一致**。
 *
 * 单独拎出来当常量，是为了让「默认主题什么都不改」这件事在代码形状上成立
 * ——`themeOverrides` 在默认主题下恒返回这个对象（同一个引用）。
 */
const DEFAULT_THEME_OVERRIDES = {
  common: {
    primaryColor: '#111827',
    primaryColorHover: '#1f2937',
    primaryColorPressed: '#0b1220',
    borderRadius: '8px',
  },
}

/**
 * naive-ui 内置控件的主题（M18）。
 *
 * **为什么不能只改 CSS 变量**：页签、输入框、开关、按钮这些是 naive 的组件，
 * 它们读的是 naive 自己的主题系统，拿不到 `--cping-*`。默认主题是浅色、
 * naive 默认也是浅色，所以一直相安无事；一旦切到**深色**的高对比 / 色盲友好
 * 主题，naive 的浅色主题就会在深底上画出深灰的页签文字与边框——直接读不出来。
 *
 * 所以这两套深色主题必须同时切 naive 的 `darkTheme`；主色也换成该主题的
 * 强调色，否则「开始 / 保存」这类主按钮会变成深底上一个几乎看不见的深色块。
 * 值全部来自后端给的那套配色（`store.themePalette`），前端不另挑颜色。
 *
 * 边界（有意为之）：`primaryColorHover` / `primaryColorPressed` 与主色取同一个值
 * ——前端**不做颜色运算**（压暗/提亮属于配色逻辑，应该在 `cping_core::a11y` 里，
 * 而不是在这儿再写一套）。代价是这两个主题下主按钮没有悬停变色，
 * 可点性仍由光标与焦点环表达。
 */
const naiveTheme = computed(() =>
  store.themePalette && store.themePalette.theme !== 'default' ? darkTheme : undefined,
)

const themeOverrides = computed(() => {
  const p = store.themePalette
  if (!p || p.theme === 'default') return DEFAULT_THEME_OVERRIDES
  return {
    common: {
      primaryColor: p.accent,
      primaryColorHover: p.accent,
      primaryColorPressed: p.accent,
      bodyColor: p.bg,
      cardColor: p.panel,
      modalColor: p.panel,
      popoverColor: p.panel,
      tableHeaderColor: p.subtle,
      textColorBase: p.fg,
      textColor1: p.fg,
      textColor2: p.fg,
      textColor3: p.muted,
      borderColor: p.border,
      dividerColor: p.border,
      borderRadius: '8px',
    },
  }
})

onMounted(async () => {
  // M18：先把无障碍外观应用上（主题写成 CSS 变量、减少动效打根属性），
  // 再干别的——它只影响外观，越早生效越不容易看到「默认浅色闪一下」。
  await store.loadA11y()
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
    :theme="naiveTheme"
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
