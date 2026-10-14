<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { NButton, NInput, NInputNumber, NRadioButton, NRadioGroup, NSelect, useMessage } from 'naive-ui'
import { usePingStore } from '../stores/ping'
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
    store.prefix = localSpec.value
  } else {
    specError.value = store.specError || '目标无法解析'
  }
  return ok
}

async function onStart() {
  if (store.running) {
    message.warning('已有任务在运行，请先停止')
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
    <label class="cping-label">目标（网段 / 单主机 / CIDR，可多个用逗号分隔）</label>
    <NInput
      v-model:value="localSpec"
      placeholder="192.168.1 或 192.168.1.0/24, 10.0.0.1"
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
      本次范围：{{ store.specSummary }}
      <template v-if="store.segments.length > 1">
        （{{ store.segments.length }} 段）
      </template>
    </div>

    <!-- M9：探测方式。ICMP 需要原始套接字权限；TCP 只看端口是否开放。
         两者语义不同，所以一次任务只允许一种，绝不同时统计。 -->
    <div style="margin-top: 10px">
      <label class="cping-label">探测方式</label>
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
          placeholder="端口"
        />
      </div>
      <div style="font-size: 11px; color: #6b7280; margin-top: 4px">
        <template v-if="isTcp">
          TCP 连接成功只说明端口开放，不代表 ICMP 可达；状态列会标成 TCP-OPEN。
        </template>
        <template v-else>
          ICMP 需要管理员/root 权限；普通用户下会被记成「权限不足」而不是「离线」。
        </template>
      </div>
      <!-- M12：v6 目标 + ICMP 是必然失败组合，提前说清楚，别让用户扫完才发现 -->
      <div v-if="!isTcp && store.matrixCompatible === false" style="font-size: 11px; color: #b45309; margin-top: 4px">
        当前目标含 IPv6 地址，而 ICMPv6 尚未实装；请切到 TCP 方式。
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
        本构建已启用异步调度（async feature）
      </div>
    </div>

    <div style="display: grid; grid-template-columns: 1fr 1fr; gap: 8px; margin-top: 10px">
      <NButton type="primary" :disabled="!canStart" @click="onStart">开始探测</NButton>
      <NButton :disabled="!canStop" @click="onStop">停止</NButton>
    </div>

    <div style="display: grid; grid-template-columns: 1fr 1fr; gap: 8px; margin-top: 8px">
      <div>
        <label class="cping-label">超时 (ms)</label>
        <NInputNumber v-model:value="store.timeoutMs" :min="50" :max="60000" size="small" />
      </div>
      <div>
        <label class="cping-label">并发上限</label>
        <NInputNumber v-model:value="store.concurrency" :min="1" :max="256" size="small" />
      </div>
    </div>
  </div>
</template>
