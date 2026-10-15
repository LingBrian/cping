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
  /** 类别标签（等待 / 低延迟 / …），来自 store 的 `labelOf` */
  label: string
  /** 状态文字（在线 / 超时 / …）；未探测时为「等待」 */
  status: string
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
      status: result ? result.statusLabel : t('class.waiting'),
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

/** 鼠标悬停的说明。 */
function titleOf(c: Cell): string {
  const head = c.pending ? t('grid.untested', c.ip) : t('grid.tested', c.ip, c.label)
  const rtt = c.rtt === null ? '' : ` / ${c.rtt} ms`
  const note = c.changed ? t('grid.changedNote', c.changeLabel) : ''
  return `${head}${rtt}${note}`
}

/**
 * 读屏用的单元格说明：**IP、状态、类别**三样都要有。
 *
 * 只给 `title` 是不够的：`title` 在触屏与纯键盘操作下根本不会被念出来
 * （它依赖鼠标悬停），而 `aria-label` 是读屏器逐格浏览时唯一稳定的来源。
 * 顺序固定为「IP → 状态 → 类别」，方便用户在两格之间做对比。
 */
function cellLabel(c: Cell): string {
  return t('aria.gridCell', c.ip, c.status, c.label)
}
</script>

<template>
  <div class="grid-matrix" role="grid" :aria-label="t('aria.matrix')">
    <!--
      单元格刻意**不是** <button>，也**不加 tabindex**。

      原来它是 <button>（等于 255 个 Tab 停靠点），但格子本身没有任何
      可触发动作——它只是「这个地址现在什么状态」的展示，悬停看详情、
      按 Tab 却什么也做不了。所以改成普通元素：不再进 Tab 序（255 次 Tab
      才能走出去的导航等于没有导航），鼠标悬停的轮廓与提示照旧保留，
      可达性交给 `role="gridcell"` + `aria-label`（读屏器可逐格朗读）。

      这是本轮明确划定的边界：**不是漏了 tabindex，是有意不给**。
    -->
    <div
      v-for="c in cells"
      :key="c.host"
      class="grid-matrix__cell"
      :class="{
        'grid-matrix__cell--pending': c.pending,
        'grid-matrix__cell--changed': c.changed,
      }"
      :style="{ background: c.bg, color: c.fg }"
      :title="titleOf(c)"
      role="gridcell"
      :aria-label="cellLabel(c)"
    >
      <span class="grid-matrix__sym">{{ c.symbol }}</span>{{ c.host }}
    </div>
  </div>
</template>
