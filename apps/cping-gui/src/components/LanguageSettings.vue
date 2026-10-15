<script setup lang="ts">
import { NRadioButton, NRadioGroup } from 'naive-ui'
import { lang, setLang, t, type Lang } from '../i18n'
import { usePingStore } from '../stores/ping'

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

const store = usePingStore()

/**
 * 切换语言。
 *
 * 界面自身的文案由 `t()` 响应式重渲染，但有几份数据（目标规格摘要、变化流水、
 * 后端能力摘要、趋势 / 告警页）的文本是**后端按当时的语言渲染好的成品**。
 * 不重拉的话，它们会一直停在旧语言——所以要顺手拉一次，
 * 具体取舍见 `refreshLocalizedViews` 的注释。
 *
 * 用 fire-and-forget（`void`）而不是 `await`：语言切换必须立刻见效，
 * 不能被一次可能失败的 IPC 卡住；刷新失败时 store 自己会把原因写进状态栏。
 * 相同语言重复点击直接返回，避免无谓的往返。
 */
function pick(next: Lang) {
  if (next === lang.value) return
  setLang(next)
  void store.refreshLocalizedViews()
}
</script>

<template>
  <div class="cping-panel">
    <div style="font-weight: 600; font-size: 13px; margin-bottom: 6px">{{ t('lang.title') }}</div>
    <NRadioGroup :value="lang" size="small" @update:value="pick">
      <NRadioButton v-for="o in options" :key="o.value" :value="o.value">
        {{ o.label }}
      </NRadioButton>
    </NRadioGroup>
  </div>
</template>
