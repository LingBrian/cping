<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { NButton, NInput, NInputNumber, useMessage } from 'naive-ui'
import { usePingStore } from '../stores/ping'
import * as svc from '../services/ping'

const store = usePingStore()
const message = useMessage()
const localPrefix = ref(store.prefix)
const prefixError = ref<string | null>(null)

watch(
  () => store.prefix,
  (v) => {
    localPrefix.value = v
  },
)

/** 交给 Rust 核心校验：前端不做第二套解析规则 */
async function validate() {
  prefixError.value = null
  try {
    await svc.parsePrefix(localPrefix.value)
    store.prefix = localPrefix.value
    return true
  } catch (e) {
    prefixError.value = String(e)
    return false
  }
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

const canStart = computed(() => !store.running)
const canStop = computed(() => store.running)
</script>

<template>
  <div class="cping-panel">
    <label class="cping-label">IP 地址段（C 类网，只需输入前三位）</label>
    <NInput
      v-model:value="localPrefix"
      placeholder="192.168.1"
      :status="prefixError ? 'error' : undefined"
      @blur="validate"
      @keyup.enter="onStart"
    />
    <div v-if="prefixError" style="color: #d03050; font-size: 12px; margin-top: 6px">
      {{ prefixError }}
    </div>

    <div style="display: grid; grid-template-columns: 1fr 1fr; gap: 8px; margin-top: 12px">
      <NButton type="primary" :disabled="!canStart" @click="onStart">开始探测</NButton>
      <NButton :disabled="!canStop" @click="onStop">停止</NButton>
    </div>

    <div style="display: grid; grid-template-columns: 1fr 1fr; gap: 8px; margin-top: 10px">
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
