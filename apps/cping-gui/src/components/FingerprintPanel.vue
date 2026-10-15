<script setup lang="ts">
import { NButton, NCheckbox, NEmpty, NTag } from 'naive-ui'
import { usePingStore } from '../stores/ping'

const store = usePingStore()
</script>

<template>
  <div class="fp-panel">
    <div class="fp-panel__head">
      <span>
        已探测 <b>{{ store.fingerprintedHosts }}</b> 台 · 开放端口
        <b>{{ store.fingerprintOpenPorts }}</b> 个（{{ store.fingerprintWithOpenPorts }} 台有开放端口）
        <span v-if="store.fingerprintPorts > 0" class="fp-panel__meta">
          · 每次探测 {{ store.fingerprintPorts }} 个端口
        </span>
      </span>
      <div class="fp-panel__actions">
        <NCheckbox v-model:checked="store.fingerprintBanner" size="small">抓取服务横幅</NCheckbox>
        <NButton
          size="tiny"
          type="primary"
          :loading="store.fingerprintBusy"
          :disabled="store.stats.online === 0"
          @click="store.runFingerprint(null)"
        >
          探测全部在线主机
        </NButton>
        <NButton
          size="tiny"
          quaternary
          :disabled="store.fingerprints.length === 0"
          @click="store.dropFingerprints()"
        >
          清空
        </NButton>
      </div>
    </div>

    <!-- 边界说明必须显眼：端口全关 ≠ 主机离线 -->
    <div class="fp-panel__note">
      端口指纹是**独立于在线判定**的一趟探测：端口全关不会把主机改成离线，
      端口开放也不会把离线主机变在线。横幅只读对方主动送出的字节，不发送任何协议请求。
      设备类型是「厂商 + 端口」的<b>推断</b>结论，识别不出就留空。
    </div>

    <div v-if="store.fingerprints.length === 0" class="fp-panel__empty">
      <NEmpty description="还没有端口指纹">
        <template #extra>
          <div style="font-size: 12px; color: #6b7280; max-width: 380px; line-height: 1.6">
            先扫描出在线主机，再点上面的「探测全部在线主机」（或按 F）。
            端口扫描只在**你已获授权**的网络上做。
          </div>
        </template>
      </NEmpty>
    </div>

    <ul v-else class="fp-panel__list">
      <li v-for="f in store.fingerprints" :key="f.ip" class="fp-panel__item">
        <div class="fp-panel__row">
          <code class="cping-mono fp-panel__ip">{{ f.ip }}</code>
          <span v-if="f.openPorts.length > 0" class="fp-panel__ports">
            <NTag
              v-for="p in f.openPorts"
              :key="p.port"
              size="tiny"
              :bordered="false"
              type="info"
            >
              {{ p.port }}{{ p.service ? `(${p.service})` : '' }}
            </NTag>
          </span>
          <span v-else class="fp-panel__closed">无开放端口（不影响其在线状态）</span>
        </div>

        <div v-if="f.deviceSummary" class="fp-panel__device">
          <NTag size="tiny" type="warning" :bordered="false">推断</NTag>
          {{ f.deviceSummary }}
        </div>

        <ul v-if="f.openPorts.some((p) => p.banner)" class="fp-panel__banners">
          <li v-for="p in f.openPorts.filter((x) => x.banner)" :key="`b-${p.port}`">
            <code class="cping-mono">:{{ p.port }}</code>
            <span class="fp-panel__banner">{{ p.banner }}</span>
          </li>
        </ul>
      </li>
    </ul>
  </div>
</template>

<style scoped>
.fp-panel {
  height: 100%;
  overflow: auto;
  font-size: 12.5px;
}
.fp-panel__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  flex-wrap: wrap;
}
.fp-panel__meta {
  color: #6b7280;
}
.fp-panel__actions {
  display: flex;
  align-items: center;
  gap: 8px;
}
.fp-panel__note {
  margin: 8px 0 12px;
  padding: 8px 10px;
  background: #f6f7f9;
  border: 1px solid #e5e7eb;
  border-radius: 8px;
  color: #4b5563;
  line-height: 1.7;
}
.fp-panel__empty {
  padding: 24px 0;
}
.fp-panel__list {
  list-style: none;
  margin: 0;
  padding: 0;
}
.fp-panel__item {
  border-bottom: 1px solid #eef0f3;
  padding: 8px 0;
}
.fp-panel__row {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.fp-panel__ip {
  min-width: 130px;
}
.fp-panel__ports {
  display: flex;
  gap: 4px;
  flex-wrap: wrap;
}
.fp-panel__closed {
  color: #6b7280;
}
.fp-panel__device {
  margin-top: 4px;
  color: #4b5563;
}
.fp-panel__banners {
  list-style: none;
  margin: 4px 0 0;
  padding-left: 12px;
  color: #374151;
}
.fp-panel__banner {
  margin-left: 6px;
}
</style>
