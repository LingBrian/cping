<script setup lang="ts">
import { computed } from 'vue'
import { NButton, NModal, NSpace, useMessage } from 'naive-ui'
import { t } from '../i18n'
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
  message.info(deleteFile ? t('msg.discarded') : t('msg.ignored'))
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
      <div style="font-size: 15px; font-weight: 600">{{ t('restore.title') }}</div>

      <template v-if="readable">
        <div style="font-size: 13px; color: #374151; margin-top: 12px; line-height: 1.9">
          <div>{{ t('restore.scanTime', pending?.scanTime) }}</div>
          <div>{{ t('restore.prefix', pending?.prefix) }}</div>
          <div>{{ t('restore.rows', pending?.rows, pending?.total) }}</div>
          <div>{{ t('restore.elapsed', elapsed) }}</div>
        </div>
      </template>
      <template v-else>
        <!-- 读不出来就如实说读不出来，不提供「恢复」这条路。
             下面那行 error 是后端给的原文，原样透传。 -->
        <div style="font-size: 13px; color: #b91c1c; margin-top: 12px; line-height: 1.9">
          <div>{{ t('restore.unreadable') }}</div>
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
        {{ t('restore.file', pending?.path) }}
      </div>

      <NSpace justify="end" style="margin-top: 16px">
        <NButton @click="onDismiss(true)">{{ t('btn.discard') }}</NButton>
        <NButton @click="onDismiss(false)">{{ t('btn.notNow') }}</NButton>
        <NButton v-if="readable" type="primary" @click="onRestore">{{ t('btn.restore') }}</NButton>
      </NSpace>
    </div>
  </NModal>
</template>
