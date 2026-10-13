<script setup lang="ts">
import { usePingStore } from '../stores/ping'
import { CLASS_LABELS, CLASS_SYMBOLS, type ResultClassKey } from '../shared/types'

const store = usePingStore()

// 与原版的差异已在 docs/reverse-findings.md 中记录：
// 原版禁止修改「无回应」颜色，新项目允许（Q4 决定）。
const keys: ResultClassKey[] = ['low', 'mid', 'high', 'offline', 'error']
</script>

<template>
  <div class="cping-panel">
    <div style="font-weight: 600; font-size: 13px; margin-bottom: 8px">时延配色</div>
    <div v-for="key in keys" :key="key" style="display: flex; align-items: center; gap: 8px; margin-bottom: 6px">
      <input
        type="color"
        :value="store.colors[key]"
        style="width: 42px; height: 26px; border: 1px solid var(--cping-border); border-radius: 4px; padding: 0"
        @input="store.setColor(key, ($event.target as HTMLInputElement).value)"
      />
      <span style="font-size: 12px; flex: 1">
        <span class="badge">{{ CLASS_SYMBOLS[key] }}</span>{{ CLASS_LABELS[key] }}
      </span>
      <code style="font-size: 11px; color: #6b7280">{{ store.colors[key] }}</code>
    </div>
    <div style="font-size: 11px; color: #9ca3af; margin-top: 6px">
      颜色不是唯一标识：每类都有独立符号，无颜色终端/色觉障碍下同样可读。
    </div>
  </div>
</template>
