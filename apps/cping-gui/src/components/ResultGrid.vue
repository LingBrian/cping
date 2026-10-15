<script setup lang="ts">
import { computed } from 'vue'
import { t } from '../i18n'
import { usePingStore } from '../stores/ping'
import { CLASS_SYMBOLS, MATRIX_CELLS } from '../shared/types'

const store = usePingStore()

interface Cell {
  host: number
  ip: string
  symbol: string
  label: string
  rtt: number | null
  bg: string
  fg: string
  online: boolean
  pending: boolean
  changed: boolean
  changeLabel: string
}

/**
 * 格号 == IP 最后一段（与原版一致，见 docs/reverse-findings.md §5.2）。
 * 格号 0 对应 `.0`，原版不探测它，因此永远为空。
 *
 * M9 之后目标不再一定是单个 C 类网段，所以每格的 IP 直接取
 * `targets[slot]`（后端给的权威地址），只有在没排到槽位时才退回
 * 「前缀 + 格号」这个旧写法。
 */
const cells = computed<Cell[]>(() => {
  const out: Cell[] = []
  for (let host = 0; host < MATRIX_CELLS; host += 1) {
    const slot = store.cellSlots[host]
    const result = slot == null ? null : (store.results[slot] ?? null)
    const key = result?.classKey ?? null
    const changed = slot != null && store.isChanged(slot)
    out.push({
      host,
      ip: slot != null ? (store.targets[slot] ?? `${store.prefix}.${host}`) : `${store.prefix}.${host}`,
      symbol: key ? CLASS_SYMBOLS[key] : '·',
      label: store.labelOf(key),
      rtt: result?.rttMs ?? null,
      bg: key ? store.colorOf(key) : '#ededed',
      fg: key ? '#111827' : '#9ca3af',
      online: result?.statusKey === 'online',
      pending: key === null,
      changed,
      changeLabel: slot != null ? (store.changeMap.get(slot)?.kindLabel ?? '') : '',
    })
  }
  return out
})

function titleOf(c: Cell): string {
  const head = c.pending ? t('grid.untested', c.ip) : t('grid.tested', c.ip, c.label)
  const rtt = c.rtt === null ? '' : ` / ${c.rtt} ms`
  const note = c.changed ? t('grid.changedNote', c.changeLabel) : ''
  return `${head}${rtt}${note}`
}
</script>

<template>
  <div class="grid-matrix" role="grid" :aria-label="t('aria.matrix')">
    <button
      v-for="c in cells"
      :key="c.host"
      type="button"
      class="grid-matrix__cell"
      :class="{
        'grid-matrix__cell--pending': c.pending,
        'grid-matrix__cell--changed': c.changed,
      }"
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
