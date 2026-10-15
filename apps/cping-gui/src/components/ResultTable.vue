<script setup lang="ts">
import { NButton, useMessage } from 'naive-ui'
import { t } from '../i18n'
import { usePingStore } from '../stores/ping'
import { CLASS_SYMBOLS } from '../shared/types'

const store = usePingStore()
const message = useMessage()

/**
 * 主机名走 DNS 反查，是阻塞操作，所以只在用户点击时对单行发起。
 * 查不到就保持为空——界面上不出现「未知主机」这类占位文本。
 */
async function onLookup(index: number) {
  const msg = await store.lookupHostname(index)
  if (!msg) message.error(store.status)
}

/** M8：把本轮变化拼进单元格 title，鼠标悬停就能看到「从什么变成了什么」。 */
function changeTitle(index: number, base: string): string {
  const c = store.changeMap.get(index)
  const note = store.changeNoteOf(index)
  return c && note ? `${base} —— ${note}` : base
}
</script>

<template>
  <div class="result-table__scroll">
    <!-- 列宽用百分比而非固定像素：表格是 table-layout: fixed，
         百分比让七列永远按比例填满容器，不会因为内容变长而撑出横向滚动条。
         放不下的内容由 .result-table__ellipsis 截断，完整值放 title。 -->
    <table class="result-table" :aria-label="t('aria.resultTable')">
      <thead>
        <tr>
          <!-- `scope="col"` 让读屏器把「单元格 ↔ 表头」的关系说清楚：
               否则念到某一格时只会报一串数字，用户不知道那是 RTT 还是厂商。 -->
          <th scope="col" style="width: 13%">{{ t('table.ip') }}</th>
          <th scope="col" style="width: 14%">{{ t('table.status') }}</th>
          <!-- RTT 是语言中立的技术缩写，中英文一致，故不入字典 -->
          <th scope="col" style="width: 6%">RTT</th>
          <th scope="col" style="width: 16%">{{ t('table.mac') }}</th>
          <th scope="col" style="width: 21%">{{ t('table.vendor') }}</th>
          <th scope="col" style="width: 20%">{{ t('table.hostname') }}</th>
          <th scope="col" style="width: 10%">{{ t('table.class') }}</th>
        </tr>
      </thead>
      <tbody>
        <tr
          v-for="row in store.visibleRows"
          :key="row.index"
          :class="{ 'result-table__row--changed': store.isChanged(row.index) }"
        >
          <td class="cping-mono" :title="row.ip">{{ row.ip }}</td>
          <td :title="changeTitle(row.index, row.result ? row.result.statusLabel : t('class.waiting'))">
            <!-- M8：变化用「符号 + 文字」双通道标出，不靠颜色单独表意 -->
            <span v-if="store.isChanged(row.index)" class="badge badge--change">{{
              store.changeMap.get(row.index)?.kindLabel ?? t('change.fallback')
            }}</span>
            <span v-if="row.result" class="badge">{{
              row.result.classKey ? CLASS_SYMBOLS[row.result.classKey] : '·'
            }}</span>
            {{ row.result ? row.result.statusLabel : t('class.waiting') }}
          </td>
          <td class="cping-mono">{{ row.result?.rttMs ?? '' }}</td>
          <td class="cping-mono" :title="row.result?.mac ?? ''">{{ row.result?.mac ?? '' }}</td>
          <td :title="row.result?.vendor ?? ''">
            <span class="result-table__ellipsis">{{ row.result?.vendor ?? '' }}</span>
          </td>
          <td>
            <div class="result-table__cell-flex">
              <span class="result-table__ellipsis cping-mono" :title="row.result?.hostname ?? ''">{{
                row.result?.hostname ?? ''
              }}</span>
              <NButton
                v-if="row.result && !row.result.hostname"
                size="tiny"
                quaternary
                :loading="store.hostnameBusy === row.index"
                :disabled="store.hostnameBusy !== null"
                :aria-label="t('aria.lookup', row.ip)"
                @click="onLookup(row.index)"
              >
                {{ t('btn.lookupShort') }}
              </NButton>
            </div>
          </td>
          <td :title="store.labelOf(row.result?.classKey ?? null)">
            {{ store.labelOf(row.result?.classKey ?? null) }}
          </td>
        </tr>
      </tbody>
    </table>
  </div>
</template>
