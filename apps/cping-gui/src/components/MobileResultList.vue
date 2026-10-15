<script setup lang="ts">
import { t } from '../i18n'
import { usePingStore } from '../stores/ping'
import { CLASS_SYMBOLS } from '../shared/types'

const store = usePingStore()
</script>

<template>
  <div class="mobile-list">
    <div v-if="store.visibleRows.length === 0" class="cping-panel" style="text-align: center; color: #6b7280">
      {{ t('list.empty') }}
    </div>
    <div
      v-for="row in store.visibleRows"
      :key="row.index"
      class="mobile-list__row mobile-list__row--stacked"
      :class="{ 'mobile-list__row--changed': store.isChanged(row.index) }"
    >
      <div class="mobile-list__head">
        <span class="mobile-list__ip">{{ row.ip }}</span>
        <span>
          <!-- 不用颜色单独表意：符号 + 文字同时给出 -->
          <span class="badge">{{ row.result?.classKey ? CLASS_SYMBOLS[row.result.classKey] : '·' }}</span>
          {{ row.result ? row.result.statusLabel : t('class.waiting') }}
        </span>
        <span class="mobile-list__rtt">
          {{ row.result?.rttMs != null ? `${row.result.rttMs} ms` : '--' }}
        </span>
      </div>
      <!-- M8：本轮变化单独一行，触控下比挤在状态里更容易读 -->
      <div v-if="store.isChanged(row.index)" class="mobile-list__change">
        <span class="badge badge--change">{{ store.changeMap.get(row.index)?.kindLabel ?? t('change.fallback') }}</span>
        {{ store.changeNoteOf(row.index) }}
      </div>
      <!-- M7：MAC / 厂商 / 主机名。触控下多占一行比挤在一行更容易读。 -->
      <div v-if="row.result" class="mobile-list__info">
        <span v-if="row.result.mac" class="cping-mono">{{ row.result.mac }}</span>
        <span v-if="row.result.vendor" :title="row.result.vendor">{{ row.result.vendor }}</span>
        <span v-if="row.result.hostname" class="cping-mono">{{ row.result.hostname }}</span>
        <!-- 反查是阻塞操作，所以只对单行按需发起 -->
        <button
          v-if="!row.result.hostname"
          type="button"
          class="mobile-list__lookup"
          :disabled="store.hostnameBusy !== null"
          @click="store.lookupHostname(row.index)"
        >
          {{ store.hostnameBusy === row.index ? t('btn.lookupBusy') : t('btn.lookup') }}
        </button>
      </div>
    </div>
  </div>
</template>
