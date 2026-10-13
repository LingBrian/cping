<script setup lang="ts">
import { usePingStore } from '../stores/ping'
import { CLASS_SYMBOLS } from '../shared/types'

const store = usePingStore()
</script>

<template>
  <div class="mobile-list">
    <div v-if="store.visibleRows.length === 0" class="cping-panel" style="text-align: center; color: #6b7280">
      暂无数据（可关闭「仅在线」过滤后查看）
    </div>
    <div v-for="row in store.visibleRows" :key="row.index" class="mobile-list__row">
      <span class="mobile-list__ip">{{ row.ip }}</span>
      <span>
        <!-- 不用颜色单独表意：符号 + 文字同时给出 -->
        <span class="badge">{{ row.result?.classKey ? CLASS_SYMBOLS[row.result.classKey] : '·' }}</span>
        {{ row.result ? row.result.statusLabel : '等待' }}
      </span>
      <span class="mobile-list__rtt">
        {{ row.result?.rttMs != null ? `${row.result.rttMs} ms` : '--' }}
      </span>
    </div>
  </div>
</template>
