<script setup lang="ts">
import { usePingStore } from '../stores/ping'
import { CLASS_SYMBOLS } from '../shared/types'

const store = usePingStore()
</script>

<template>
  <div class="result-table__scroll">
    <table class="result-table">
      <thead>
        <tr>
          <th style="width: 150px">IP 地址</th>
          <th style="width: 110px">状态</th>
          <th style="width: 80px">RTT</th>
          <th style="width: 170px">MAC 地址</th>
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
          <td>{{ store.labelOf(row.result?.classKey ?? null) }}</td>
        </tr>
      </tbody>
    </table>
  </div>
</template>
