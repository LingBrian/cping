<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { NAlert, NButton, NEmpty, NStatistic, NTag, useMessage } from 'naive-ui'
import { save } from '@tauri-apps/plugin-dialog'
import { usePingStore } from '../stores/ping'

const props = withDefaults(
  defineProps<{
    /**
     * 是否显示导出按钮。移动端关掉：`@tauri-apps/plugin-dialog` 的 `save`
     * 在 Android 上没有原生实现，点下去只会抛错——不如不显示。
     */
    exportable?: boolean
  }>(),
  { exportable: true },
)

const store = usePingStore()
const message = useMessage()

/** 折线宽度：跟着容器走一次即可，不做 ResizeObserver（重型且没必要）。 */
const rootEl = ref<HTMLElement | null>(null)

async function refresh() {
  const width = rootEl.value?.clientWidth ? Math.min(520, rootEl.value.clientWidth - 240) : 160
  await store.loadTrend(Math.max(80, width))
}

onMounted(refresh)

/** 导出可用率报表（扩展名决定格式，与后端一致）。 */
async function onExport(kind: 'csv' | 'html') {
  if (!props.exportable) return
  const path = await save({
    defaultPath: `uptime-${store.prefix || 'report'}.${kind}`.replace(/[^\w.\-]/g, '_'),
    filters:
      kind === 'csv'
        ? [{ name: 'CSV 文件', extensions: ['csv'] }]
        : [{ name: 'HTML 报告', extensions: ['html'] }],
  })
  if (!path) return
  const msg = await store.exportTrend(path)
  if (msg) message.success(msg)
  else message.error(store.status)
}

async function onClear() {
  await store.dropTrend()
  message.info(store.status)
}
</script>

<template>
  <div ref="rootEl" class="trend-panel">
    <div class="trend-panel__head">
      <span v-if="store.trend">
        时间跨度 <b>{{ store.trend.spanHours.toFixed(1) }}</b> 小时 ·
        <b>{{ store.trend.points }}</b> 个时间点 · 累计 <b>{{ store.trend.rounds }}</b> 轮 ·
        保留 {{ store.trend.retentionDays }} 天
      </span>
      <span v-else>趋势历史</span>
      <div class="trend-panel__actions">
        <NButton size="tiny" :loading="store.trendBusy" @click="refresh">刷新</NButton>
        <NButton
          v-if="exportable"
          size="tiny"
          :disabled="!store.trend || store.trend.rows.length === 0"
          @click="onExport('csv')"
        >
          导出 CSV
        </NButton>
        <NButton
          v-if="exportable"
          size="tiny"
          :disabled="!store.trend || store.trend.rows.length === 0"
          @click="onExport('html')"
        >
          导出 HTML
        </NButton>
        <NButton
          size="tiny"
          quaternary
          :disabled="!store.trend || store.trend.points === 0"
          @click="onClear"
        >
          清空
        </NButton>
      </div>
    </div>

    <!-- 两条边界必须写清楚，否则这张表很容易被读错 -->
    <div class="trend-panel__note">
      可用率 = 在线轮数 ÷ 观测轮数。<b>从未在线的主机，RTT 显示为「-」而不是 0</b>——
      0 会被读成「极快」。曲线断开的那些点表示该轮<b>没测到</b>这台主机，不是「离线」。
      历史由本进程累积；需要跨会话长期保留，请用 <code>cping-cli --history</code>
      （两端共用同一套时序核心）。
    </div>

    <NAlert v-if="store.trend?.emptyHint" type="info" :bordered="false" style="margin-bottom: 12px">
      {{ store.trend.emptyHint }}
    </NAlert>

    <NEmpty v-if="!store.trend || store.trend.rows.length === 0" description="还没有可用率数据">
      <template #extra>
        <div style="font-size: 12px; color: #6b7280; max-width: 420px; line-height: 1.6">
          先做一次扫描；打开右上角的「持续监控」连续跑几轮，这里就会出现每台主机的
          可用率、延迟与抖动，以及一条在线率曲线。
        </div>
      </template>
    </NEmpty>

    <div v-else class="trend-panel__grid">
      <div v-for="r in store.trend.rows" :key="r.ip" class="trend-panel__card">
        <div class="trend-panel__card-head">
          <code class="cping-mono">{{ r.ip }}</code>
          <NTag
            size="tiny"
            :bordered="false"
            :type="r.uptimePercent >= 99 ? 'success' : r.uptimePercent >= 90 ? 'warning' : 'error'"
          >
            {{ r.uptimeText }}
          </NTag>
        </div>
        <!--
          v-html：内容是核心里的纯函数产出的 SVG（无 <script>、无外链）。
          资产面板同样只展示核心给的文本，不在这里拼标记。
        -->
        <div class="trend-panel__spark" v-html="r.sparkline" />
        <div class="trend-panel__nums">
          <NStatistic size="small" label="在线/观测" :value="`${r.online}/${r.samples}`" />
          <NStatistic size="small" label="平均 RTT (ms)" :value="r.avgRttText" />
          <NStatistic size="small" label="最差 RTT (ms)" :value="r.worstRttText" />
          <NStatistic size="small" label="抖动 (ms)" :value="r.jitterText" />
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.trend-panel {
  height: 100%;
  overflow: auto;
  font-size: 12.5px;
}
.trend-panel__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  flex-wrap: wrap;
}
.trend-panel__actions {
  display: flex;
  align-items: center;
  gap: 8px;
}
.trend-panel__note {
  margin: 8px 0 12px;
  padding: 8px 10px;
  background: #f6f7f9;
  border: 1px solid #e5e7eb;
  border-radius: 8px;
  color: #4b5563;
  line-height: 1.7;
}
.trend-panel__grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));
  gap: 10px;
}
.trend-panel__card {
  border: 1px solid #eef0f3;
  border-radius: 8px;
  padding: 8px 10px;
  background: #fff;
}
.trend-panel__card-head {
  display: flex;
  align-items: center;
  gap: 8px;
}
.trend-panel__spark {
  margin: 6px 0 4px;
  min-height: 28px;
}
.trend-panel__nums {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 4px;
}
</style>
