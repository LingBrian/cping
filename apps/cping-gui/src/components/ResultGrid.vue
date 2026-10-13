<script setup lang="ts">
import { computed } from 'vue'
import { usePingStore } from '../stores/ping'
import { CLASS_SYMBOLS, MATRIX_CELLS } from '../shared/types'

const store = usePingStore()

interface Cell {
  host: number
  symbol: string
  label: string
  rtt: number | null
  bg: string
  fg: string
  online: boolean
  pending: boolean
}

/**
 * 格号 == IP 最后一段（与原版一致，见 docs/reverse-findings.md §5.2）。
 * 格号 0 对应 `.0`，原版不探测它，因此永远为空。
 */
const cells = computed<Cell[]>(() => {
  const out: Cell[] = []
  for (let host = 0; host < MATRIX_CELLS; host += 1) {
    const slot = store.cellSlots[host]
    const result = slot == null ? null : (store.results[slot] ?? null)
    const key = result?.classKey ?? null
    out.push({
      host,
      symbol: key ? CLASS_SYMBOLS[key] : '·',
      label: store.labelOf(key),
      rtt: result?.rttMs ?? null,
      bg: key ? store.colorOf(key) : '#ededed',
      fg: key ? '#111827' : '#9ca3af',
      online: result?.statusKey === 'online',
      pending: key === null,
    })
  }
  return out
})

function titleOf(c: Cell): string {
  if (c.pending) return `${store.prefix}.${c.host} —— 未探测`
  const rtt = c.rtt === null ? '—' : `${c.rtt} ms`
  return `${store.prefix}.${c.host} —— ${c.label} / ${rtt}`
}
</script>

<template>
  <div class="grid-matrix" role="grid" aria-label="探测结果矩阵">
    <button
      v-for="c in cells"
      :key="c.host"
      type="button"
      class="grid-matrix__cell"
      :class="{ 'grid-matrix__cell--pending': c.pending }"
      :style="{ background: c.bg, color: c.fg }"
      :title="titleOf(c)"
      role="gridcell"
    >
      <span class="grid-matrix__sym">{{ c.symbol }}</span>{{ c.host }}
      <span class="sr-only">{{ titleOf(c) }}</span>
    </button>
  </div>
</template>

<style scoped>
.sr-only {
  position: absolute;
  width: 1px;
  height: 1px;
  overflow: hidden;
  clip: rect(0 0 0 0);
  white-space: nowrap;
}
</style>
