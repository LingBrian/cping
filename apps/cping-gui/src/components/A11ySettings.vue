<script setup lang="ts">
import { NRadioButton, NRadioGroup, NSwitch } from 'naive-ui'
import { t } from '../i18n'
import { usePingStore } from '../stores/ping'
import {
  FONT_SCALE_KEYS,
  FONT_SCALE_LABEL_KEYS,
  THEME_KEYS,
  THEME_LABEL_KEYS,
  type FontScaleKey,
  type ThemeKey,
} from '../shared/types'

/**
 * M18：无障碍设置（主题 / 字号 / 减少动效）。
 *
 * 三种控件都在这一块里，因为它们服务于同一件事——「在无颜色、看不清、
 * 不想看动画的环境下仍然可达」。与 `ColorSettings`（类别色自定义）刻意分开：
 * 那个管的是「探测结果长什么样」，这个管的是「界面本身能不能用」。
 *
 * 这里**不持有任何一组颜色**：主题的三套配色由 `cping_core::a11y` 定义
 * （对比度 ≥ 4.5/7:1、色盲友好主题的任意两类在四种视觉下可区分，都有测试守着），
 * 前端只把后端算好的值写成 CSS 变量。
 */
const store = usePingStore()
</script>

<template>
  <div class="cping-panel">
    <div style="font-weight: 600; font-size: 13px; margin-bottom: 6px">{{ t('a11y.title') }}</div>

    <div class="cping-label">{{ t('a11y.theme') }}</div>
    <NRadioGroup
      :value="store.a11y.theme"
      size="small"
      @update:value="(v: ThemeKey) => store.setTheme(v)"
    >
      <NRadioButton v-for="key in THEME_KEYS" :key="key" :value="key">
        {{ t(THEME_LABEL_KEYS[key]) }}
      </NRadioButton>
    </NRadioGroup>

    <div class="cping-label" style="margin-top: 10px">{{ t('a11y.fontScale') }}</div>
    <NRadioGroup
      :value="store.a11y.fontScale"
      size="small"
      @update:value="(v: FontScaleKey) => store.setFontScale(v)"
    >
      <NRadioButton v-for="key in FONT_SCALE_KEYS" :key="key" :value="key">
        {{ t(FONT_SCALE_LABEL_KEYS[key]) }}
      </NRadioButton>
    </NRadioGroup>

    <div
      style="display: flex; align-items: center; justify-content: space-between; margin-top: 10px"
    >
      <span style="font-size: 13px">{{ t('a11y.reduceMotion') }}</span>
      <NSwitch
        :value="store.a11y.reduceMotion"
        size="small"
        @update:value="store.setReduceMotion"
      />
    </div>

    <div style="font-size: 11px; color: var(--cping-muted); margin-top: 6px">
      {{ t('a11y.note') }}
    </div>
  </div>
</template>
