<script setup lang="ts">
import { NRadioButton, NRadioGroup } from 'naive-ui'
import { lang, setLang, t, type Lang } from '../i18n'

/**
 * 语言选择。两个选项用**各自语言的自称**显示（中文 / English），
 * 这是语言切换器的通行做法：用户看不懂当前界面语言时也能找到自己的语言。
 *
 * 正因为是「自称」，这两条文案故意**不进字典**——它们在任何语言下都长一样。
 */
const options: { value: Lang; label: string }[] = [
  { value: 'zh', label: '中文' },
  { value: 'en', label: 'English' },
]
</script>

<template>
  <div class="cping-panel">
    <div style="font-weight: 600; font-size: 13px; margin-bottom: 6px">{{ t('lang.title') }}</div>
    <NRadioGroup :value="lang" size="small" @update:value="(v: Lang) => setLang(v)">
      <NRadioButton v-for="o in options" :key="o.value" :value="o.value">
        {{ o.label }}
      </NRadioButton>
    </NRadioGroup>
  </div>
</template>
