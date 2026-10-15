<script setup lang="ts">
import { usePingStore } from '../stores/ping'
import { t } from '../i18n'
import { CLASS_LABEL_KEYS, CLASS_SYMBOLS, type ResultClassKey } from '../shared/types'

const store = usePingStore()

// 与原版的差异已在 docs/reverse-findings.md 中记录：
// 原版禁止修改「无回应」颜色，新项目允许（Q4 决定）。
const keys: ResultClassKey[] = ['low', 'mid', 'high', 'offline', 'error']
</script>

<template>
  <div class="cping-panel">
    <div style="font-weight: 600; font-size: 13px; margin-bottom: 6px">{{ t('color.title') }}</div>
    <div v-for="key in keys" :key="key" style="display: flex; align-items: center; gap: 8px; margin-bottom: 4px">
      <input
        type="color"
        :value="store.colors[key]"
        :aria-label="t('aria.colorForClass', t(CLASS_LABEL_KEYS[key]))"
        style="width: 34px; height: 20px; border: 1px solid var(--cping-border); border-radius: 4px; padding: 0"
        @input="store.setColor(key, ($event.target as HTMLInputElement).value)"
      />
      <span style="font-size: 12px; flex: 1">
        <span class="badge">{{ CLASS_SYMBOLS[key] }}</span>{{ t(CLASS_LABEL_KEYS[key]) }}
      </span>
      <code style="font-size: 11px; color: #6b7280">{{ store.colors[key] }}</code>
    </div>
    <div style="font-size: 11px; color: #9ca3af; margin-top: 4px">
      {{ t('color.note') }}
    </div>
  </div>
</template>
