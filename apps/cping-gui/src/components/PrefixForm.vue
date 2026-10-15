<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { NButton, NInput, NInputNumber, NRadioButton, NRadioGroup, NSelect, useMessage } from 'naive-ui'
import { usePingStore } from '../stores/ping'
import { t } from '../i18n'
import { COMMON_TCP_PORTS, type ProbeModeKey } from '../shared/types'

const store = usePingStore()
const message = useMessage()

/**
 * 目标可以是单主机（192.168.1.10）、网段（192.168.1）、CIDR（192.168.1.0/24），
 * 也可以逗号/空格分隔混写。校验一律交给 Rust 核心（`parse_spec`），
 * 前端不实现第二套解析规则——否则两边迟早会不一致。
 */
const localSpec = ref(store.prefix)
const specError = ref<string | null>(null)

watch(
  () => store.prefix,
  (v) => {
    localSpec.value = v
  },
)

const modeOptions = [
  { key: 'icmp' as ProbeModeKey, label: 'ICMP' },
  { key: 'tcp' as ProbeModeKey, label: 'TCP' },
]

const portOptions = COMMON_TCP_PORTS.map((p) => ({ label: `:${p}`, value: p }))

const isTcp = computed(() => store.mode === 'tcp')

/** 校验成功才把输入提交到 store（store.prefix 是实际会扫的值）。 */
async function validate(): Promise<boolean> {
  specError.value = null
  const ok = await store.validateSpec(localSpec.value)
  if (ok) {
    // M16：走 `setTargetSpec` 而不是直接赋值——它顺带记账「这是用户亲手填的」，
    // 模板从此不会覆盖它（「只填空缺」）。直接赋值会让模板把用户的输入顶掉。
    store.setTargetSpec(localSpec.value)
  } else {
    specError.value = store.specError || t('status.specUnparsable')
  }
  return ok
}

async function onStart() {
  if (store.running) {
    message.warning(t('msg.taskRunning'))
    return
  }
  if (!(await validate())) return
  await store.start()
}

async function onStop() {
  await store.cancel()
}

function onMode(key: ProbeModeKey) {
  store.setMode(key)
}

const canStart = computed(() => !store.running)
const canStop = computed(() => store.running)
</script>

<template>
  <div class="cping-panel">
    <label class="cping-label">{{ t('label.target') }}</label>
    <NInput
      v-model:value="localSpec"
      :placeholder="t('placeholder.target')"
      :status="specError ? 'error' : undefined"
      @blur="validate"
      @keyup.enter="onStart"
    />
    <div v-if="specError" style="color: #d03050; font-size: 12px; margin-top: 6px">
      {{ specError }}
    </div>
    <div
      v-else-if="store.specSummary"
      style="font-size: 11px; color: #6b7280; margin-top: 6px"
      :title="store.specSummary"
    >
      {{ t('label.specRange', store.specSummary) }}
      <template v-if="store.segments.length > 1">
        {{ t('label.segments', store.segments.length) }}
      </template>
    </div>

    <!-- M9：探测方式。ICMP 需要原始套接字权限；TCP 只看端口是否开放。
         两者语义不同，所以一次任务只允许一种，绝不同时统计。 -->
    <div style="margin-top: 10px">
      <label class="cping-label">{{ t('label.mode') }}</label>
      <div style="display: flex; gap: 8px; align-items: center">
        <NRadioGroup :value="store.mode" size="small" @update:value="onMode">
          <NRadioButton v-for="m in modeOptions" :key="m.key" :value="m.key">
            {{ m.label }}
          </NRadioButton>
        </NRadioGroup>
        <NSelect
          v-if="isTcp"
          v-model:value="store.tcpPort"
          size="small"
          style="width: 96px"
          :options="portOptions"
        />
        <NInputNumber
          v-if="isTcp"
          v-model:value="store.tcpPort"
          size="small"
          style="width: 110px"
          :min="1"
          :max="65535"
          :placeholder="t('label.port')"
        />
      </div>
      <div style="font-size: 11px; color: #6b7280; margin-top: 4px">
        <template v-if="isTcp">
          {{ t('hint.tcp') }}
        </template>
        <template v-else>
          {{ t('hint.icmp') }}
        </template>
      </div>
      <!-- M12：v6 目标 + ICMP 是必然失败组合，提前说清楚，别让用户扫完才发现 -->
      <div v-if="!isTcp && store.matrixCompatible === false" style="font-size: 11px; color: #b45309; margin-top: 4px">
        {{ t('hint.ipv6Icmp') }}
      </div>
      <div
        v-if="store.backendSummary"
        style="font-size: 11px; color: #9ca3af; margin-top: 4px"
        :title="store.backendCapDetails || store.backendSummary"
      >
        {{ store.backendSummary }}
      </div>
      <!-- M12：默认构建**不带**异步调度，只有显式开启 async-scheduler 才提示，避免噪音 -->
      <div v-if="store.asyncEnabled" style="font-size: 11px; color: #9ca3af; margin-top: 4px">
        {{ t('hint.async') }}
      </div>
    </div>

    <div style="display: grid; grid-template-columns: 1fr 1fr; gap: 8px; margin-top: 10px">
      <NButton type="primary" :disabled="!canStart" @click="onStart">{{ t('btn.start') }}</NButton>
      <NButton :disabled="!canStop" @click="onStop">{{ t('btn.stop') }}</NButton>
    </div>

    <div style="display: grid; grid-template-columns: 1fr 1fr; gap: 8px; margin-top: 8px">
      <div>
        <label class="cping-label">{{ t('label.timeout') }}</label>
        <!-- 用 setter 而不是 v-model：这两项是「模板只填空缺」的判据之一，
             用户手改过就要记账（模板不能再覆盖）。 -->
        <NInputNumber
          :value="store.timeoutMs"
          :min="50"
          :max="60000"
          size="small"
          @update:value="(v: number | null) => v && store.setTimeoutMs(v)"
        />
      </div>
      <div>
        <label class="cping-label">{{ t('label.concurrency') }}</label>
        <NInputNumber
          :value="store.concurrency"
          :min="1"
          :max="256"
          size="small"
          @update:value="(v: number | null) => v && store.setConcurrency(v)"
        />
      </div>
    </div>
  </div>
</template>
