<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import {
  NAlert,
  NButton,
  NEmpty,
  NInputNumber,
  NSwitch,
  NTag,
  NTooltip,
  useMessage,
} from 'naive-ui'
import { usePingStore } from '../stores/ping'
import { t } from '../i18n'

const store = usePingStore()
const message = useMessage()

const rootEl = ref<HTMLElement | null>(null)

onMounted(() => {
  void store.loadAlerts()
})

/** 阈值表单：初始值来自后端，改完点「应用」才提交（避免边输入边打 IPC）。 */
const latencyMs = ref<number | null>(null)
const uptimePct = ref<number | null>(null)
const cooldownSecs = ref<number | null>(null)

watch(
  () => store.alerts,
  (a) => {
    if (!a) return
    latencyMs.value = a.latencyThresholdMs
    uptimePct.value = a.uptimeThresholdPct
    cooldownSecs.value = a.cooldownSecs
  },
  { immediate: true },
)

const dirty = computed(() => {
  const a = store.alerts
  if (!a) return false
  return (
    latencyMs.value !== a.latencyThresholdMs ||
    uptimePct.value !== a.uptimeThresholdPct ||
    cooldownSecs.value !== a.cooldownSecs
  )
})

/** 类别开关：切一下立刻生效（后端只改这一项，冷却记录与基线都保留）。 */
async function onToggleKind(kind: string, on: boolean) {
  const a = store.alerts
  if (!a) return
  const kinds = a.rules.filter((r) => r.enabled).map((r) => r.kind)
  const next = on ? [...kinds, kind] : kinds.filter((k) => k !== kind)
  const ok = await store.updateAlertRules({ kinds: next })
  if (!ok) message.error(store.status)
}

async function applyThresholds() {
  const ok = await store.updateAlertRules({
    latencyMs: latencyMs.value ?? undefined,
    uptimePct: uptimePct.value ?? undefined,
    cooldownSecs: cooldownSecs.value ?? undefined,
  })
  // 后端给的是可直接展示的中文原因（比如「可用率阈值必须在 0-100 之间」）
  if (ok) message.success(store.status)
  else message.error(store.status)
}

function severityType(sev: string): 'default' | 'info' | 'warning' | 'error' {
  if (sev === 'critical') return 'error'
  if (sev === 'warning') return 'warning'
  return 'info'
}

/** 值 → 可读文本：边沿类规则没有值，写「-」而不是 0。 */
function valueText(v: number | null, threshold: number | null, kind: string): string {
  if (v === null) return '-'
  if (kind === 'uptime_below') return t('alert.valueUptime', v.toFixed(1), threshold)
  return t('alert.valueLatency', v.toFixed(0), threshold)
}
</script>

<template>
  <div ref="rootEl" class="alert-panel">
    <div class="alert-panel__head">
      <span v-if="store.alerts">
        <b>{{ store.alerts.enabled ? t('alert.enabled') : t('alert.disabled') }}</b> ·
        {{ t('alert.activeRules', store.alerts.activeRules) }} ·
        {{ t('alert.total', store.alerts.total) }} ·
        {{ t('alert.suppressedCooldown', store.alerts.suppressedByCooldown) }} ·
        {{ t('alert.suppressedMute', store.alerts.suppressedByMute) }}
      </span>
      <span v-else>{{ t('alert.titleShort') }}</span>
      <div class="alert-panel__actions">
        <NSwitch
          :value="store.alerts?.enabled ?? false"
          size="small"
          @update:value="(v: boolean) => store.setAlertsEnabled(v)"
        >
          <template #checked>{{ t('alert.on') }}</template>
          <template #unchecked>{{ t('alert.off') }}</template>
        </NSwitch>
        <NSwitch
          :value="store.alerts?.muted ?? false"
          size="small"
          :disabled="!store.alerts?.enabled"
          @update:value="(v: boolean) => store.setAlertMuted(v)"
        >
          <template #checked>{{ t('alert.muted') }}</template>
          <template #unchecked>{{ t('alert.unmuted') }}</template>
        </NSwitch>
        <NButton size="tiny" :loading="store.alertsBusy" @click="store.loadAlerts()">{{ t('btn.refresh') }}</NButton>
        <NButton
          size="tiny"
          quaternary
          :disabled="!store.alerts || store.alerts.total === 0"
          @click="store.dropAlerts()"
        >
          {{ t('btn.clearRecords') }}
        </NButton>
      </div>
    </div>

    <!--
      这几条边界是本项目里最容易被读错的地方，必须写在界面上而不是只写在代码注释里。
      文案里的 <b> 是我们自己的静态标记（无用户输入），故用 v-html 保留强调。
    -->
    <div class="alert-panel__note" v-html="t('alert.note')" />

    <NAlert v-if="store.alerts?.emptyHint" type="info" :bordered="false" style="margin-bottom: 12px">
      {{ store.alerts.emptyHint }}
    </NAlert>

    <div class="alert-panel__rules">
      <div v-for="r in store.alerts?.rules ?? []" :key="r.kind" class="alert-panel__rule">
        <NSwitch
          size="small"
          :value="r.enabled"
          :disabled="!store.alerts?.enabled"
          @update:value="(v: boolean) => onToggleKind(r.kind, v)"
        />
        <span class="alert-panel__rule-label">{{ r.label }}</span>
        <NTooltip v-if="r.usesThreshold" trigger="hover">
          <template #trigger>
            <code class="cping-mono alert-panel__rule-th">
              {{ r.kind === 'uptime_below' ? `${r.threshold}%` : `${r.threshold} ms` }}
            </code>
          </template>
          {{ t('alert.thresholdTip') }}
        </NTooltip>
        <span v-else class="alert-panel__rule-th alert-panel__rule-th--none">{{ t('alert.noThreshold') }}</span>
        <span class="alert-panel__rule-cd">{{ t('alert.cooldown', r.cooldownSecs) }}</span>
      </div>
    </div>

    <div class="alert-panel__form">
      <label>
        {{ t('alert.latencyThreshold') }}
        <NInputNumber v-model:value="latencyMs" size="tiny" :min="0" :max="60000" style="width: 110px" />
      </label>
      <label>
        {{ t('alert.uptimeThreshold') }}
        <NInputNumber v-model:value="uptimePct" size="tiny" :min="0" :max="100" style="width: 110px" />
      </label>
      <label>
        {{ t('alert.cooldownLabel') }}
        <NInputNumber v-model:value="cooldownSecs" size="tiny" :min="0" :max="86400" style="width: 110px" />
      </label>
      <NButton size="tiny" type="primary" :disabled="!dirty" @click="applyThresholds">
        {{ t('btn.apply') }}
      </NButton>
      <span v-if="dirty" class="alert-panel__dirty">{{ t('alert.dirty') }}</span>
    </div>

    <NEmpty v-if="!store.alerts || store.alerts.events.length === 0" :description="t('alert.empty')">
      <template #extra>
        <div style="font-size: 12px; color: #6b7280; max-width: 460px; line-height: 1.6">
          {{ t('alert.emptyHint') }}
        </div>
      </template>
    </NEmpty>

    <div v-else class="alert-panel__list">
      <div v-for="e in store.alerts.events" :key="`${e.atMs}-${e.ip}-${e.kind}`" class="alert-panel__row">
        <span class="alert-panel__time cping-mono">{{ e.time }}</span>
        <NTag size="tiny" :bordered="false" :type="severityType(e.severity)">
          {{ e.kindLabel }}
        </NTag>
        <code class="cping-mono alert-panel__ip">{{ e.ip }}</code>
        <span class="alert-panel__msg">{{ e.message }}</span>
        <span class="alert-panel__val cping-mono">{{ valueText(e.value, e.threshold, e.kind) }}</span>
        <span class="alert-panel__round">{{ t('round.n', e.round) }}</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.alert-panel {
  height: 100%;
  overflow: auto;
  font-size: 12.5px;
}
.alert-panel__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  flex-wrap: wrap;
}
.alert-panel__actions {
  display: flex;
  align-items: center;
  gap: 10px;
}
.alert-panel__note {
  margin: 8px 0 12px;
  padding: 8px 10px;
  background: #f6f7f9;
  border: 1px solid #e5e7eb;
  border-radius: 8px;
  color: #4b5563;
  line-height: 1.7;
}
.alert-panel__rules {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
  gap: 8px;
  margin-bottom: 12px;
}
.alert-panel__rule {
  display: flex;
  align-items: center;
  gap: 8px;
  border: 1px solid #eef0f3;
  border-radius: 8px;
  padding: 6px 10px;
  background: #fff;
}
.alert-panel__rule-label {
  flex: 1;
}
.alert-panel__rule-th {
  color: #2563eb;
}
.alert-panel__rule-th--none {
  color: #9ca3af;
}
.alert-panel__rule-cd {
  color: #9ca3af;
  font-size: 11.5px;
}
.alert-panel__form {
  display: flex;
  align-items: center;
  gap: 14px;
  flex-wrap: wrap;
  margin-bottom: 12px;
}
.alert-panel__form label {
  display: flex;
  align-items: center;
  gap: 6px;
  color: #4b5563;
}
.alert-panel__dirty {
  color: #d97706;
}
.alert-panel__list {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.alert-panel__row {
  display: grid;
  grid-template-columns: 150px 64px 120px 1fr 150px 70px;
  align-items: center;
  gap: 8px;
  border: 1px solid #eef0f3;
  border-radius: 6px;
  padding: 4px 8px;
  background: #fff;
}
.alert-panel__time,
.alert-panel__round {
  color: #9ca3af;
  font-size: 11.5px;
}
.alert-panel__msg {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.alert-panel__val {
  color: #4b5563;
  font-size: 11.5px;
}
@media (max-width: 900px) {
  .alert-panel__row {
    grid-template-columns: 1fr;
    gap: 2px;
  }
}
</style>
