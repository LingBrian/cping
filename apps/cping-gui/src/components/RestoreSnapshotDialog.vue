<script setup lang="ts">
import { computed } from 'vue'
import { NButton, NModal, NSpace, useMessage } from 'naive-ui'
import { usePingStore } from '../stores/ping'

/**
 * 「发现上次未处理完的结果」确认框。
 *
 * 存在的理由：Android 上销毁 Activity 会**带动整个进程退出**（tao 直接
 * `process::exit`，见 docs/platform-compatibility.md §3.5），内存里的结果会丢。
 * 所以后端在每轮扫描结束时落盘一份，这里负责问用户要不要恢复。
 *
 * 硬性约束：**用户不表态就不恢复**（默认按 Esc/点遮罩都等于「不恢复」）。
 */
const store = usePingStore()
const message = useMessage()

const pending = computed(() => store.pendingRestore)
const show = computed(() => pending.value !== null)
const readable = computed(() => pending.value?.ok === true)

const elapsed = computed(() => {
  const ms = pending.value?.elapsedMs ?? 0
  return `${(ms / 1000).toFixed(2)} s`
})

async function onRestore() {
  const ok = await store.restoreSnapshot()
  if (ok) message.success(store.status)
  else message.error(store.status)
}

async function onDismiss(deleteFile: boolean) {
  await store.dismissSnapshot(deleteFile)
  message.info(deleteFile ? '已丢弃上次的结果' : '已忽略，下次启动不再提示')
}
</script>

<template>
  <NModal
    :show="show"
    :mask-closable="true"
    :close-on-esc="true"
    transform-origin="center"
    @mask-click="onDismiss(false)"
    @esc="onDismiss(false)"
  >
    <div class="cping-panel" style="width: min(520px, 92vw); padding: 18px">
      <div style="font-size: 15px; font-weight: 600">发现上次未处理完的扫描结果</div>

      <template v-if="readable">
        <div style="font-size: 13px; color: #374151; margin-top: 12px; line-height: 1.9">
          <div>扫描时间：{{ pending?.scanTime }}</div>
          <div>网段：{{ pending?.prefix }}</div>
          <div>结果：{{ pending?.rows }} 条 / 共 {{ pending?.total }} 个目标</div>
          <div>用时：{{ elapsed }}</div>
        </div>
      </template>
      <template v-else>
        <!-- 读不出来就如实说读不出来，不提供「恢复」这条路 -->
        <div style="font-size: 13px; color: #b91c1c; margin-top: 12px; line-height: 1.9">
          <div>这份快照无法读取，不能恢复。</div>
          <div>{{ pending?.error }}</div>
        </div>
      </template>

      <div
        style="
          font-size: 11px;
          color: #6b7280;
          margin-top: 10px;
          word-break: break-all;
        "
      >
        文件：{{ pending?.path }}
      </div>

      <NSpace justify="end" style="margin-top: 16px">
        <NButton @click="onDismiss(true)">丢弃</NButton>
        <NButton @click="onDismiss(false)">暂不恢复</NButton>
        <NButton v-if="readable" type="primary" @click="onRestore">恢复结果</NButton>
      </NSpace>
    </div>
  </NModal>
</template>
