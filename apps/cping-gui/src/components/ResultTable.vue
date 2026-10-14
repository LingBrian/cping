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
    <table class="result-table">
      <thead>
        <tr>
          <th style="width: 130px">IP 地址</th>
          <th style="width: 110px">状态</th>
          <th style="width: 70px">RTT</th>
          <th style="width: 160px">MAC 地址</th>
          <th style="width: 190px">厂商</th>
          <th style="width: 190px">主机名</th>
          <th style="width: 90px">类别</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="row in store.visibleRows" :key="row.index">
          <td class="cping-mono">{{ row.ip }}</td>
          <td>
            <span v-if="row.result" class="badge">{{ row.result.classKey ? CLASS_SYMBOLS[row.result.classKey] : '·' }}</span>
            {{ row.result ? row.result.statusLabel : '等待' }}
          </td>
          <td class="cping-mono">{{ row.result?.rttMs ?? '' }}</td>
          <td class="cping-mono">{{ row.result?.mac ?? '' }}</td>
          <td :title="row.result?.vendor ?? ''">{{ row.result?.vendor ?? '' }}</td>
          <td>
            <span class="cping-mono">{{ row.result?.hostname ?? '' }}</span>
            <NButton
              v-if="row.result && !row.result.hostname"
              size="tiny"
              quaternary
              :loading="store.hostnameBusy === row.index"
              :disabled="store.hostnameBusy !== null"
              style="margin-left: 4px"
              @click="onLookup(row.index)"
            >
              反查
            </NButton>
          </td>
          <td>{{ store.labelOf(row.result?.classKey ?? null) }}</td>
        </tr>
      </tbody>
    </table>
  </div>
</template>
