<script setup lang="ts">
import { NButton, useMessage } from 'naive-ui'
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
</script>

<template>
  <div class="result-table__scroll">
    <!-- 列宽用百分比而非固定像素：表格是 table-layout: fixed，
         百分比让七列永远按比例填满容器，不会因为内容变长而撑出横向滚动条。
         放不下的内容由 .result-table__ellipsis 截断，完整值放 title。 -->
    <table class="result-table">
      <thead>
        <tr>
          <th style="width: 13%">IP 地址</th>
          <th style="width: 14%">状态</th>
          <th style="width: 6%">RTT</th>
          <th style="width: 16%">MAC 地址</th>
          <th style="width: 21%">厂商</th>
          <th style="width: 20%">主机名</th>
          <th style="width: 10%">类别</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="row in store.visibleRows" :key="row.index">
          <td class="cping-mono" :title="row.ip">{{ row.ip }}</td>
          <td :title="row.result ? row.result.statusLabel : '等待'">
            <span v-if="row.result" class="badge">{{
              row.result.classKey ? CLASS_SYMBOLS[row.result.classKey] : '·'
            }}</span>
            {{ row.result ? row.result.statusLabel : '等待' }}
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
                @click="onLookup(row.index)"
              >
                反查
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
