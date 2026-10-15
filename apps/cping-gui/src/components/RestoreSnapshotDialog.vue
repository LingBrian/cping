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
  <!--
    对话框语义**必须自己补上**：这里的 NModal 没有指定 `preset`，走的是
    naive-ui 的「自定义内容」分支，它只提供遮罩 + 焦点陷阱（`trapFocus` /
    `autoFocus` 默认都是 true，所以打开时焦点会落进来、Tab 也出不去），
    **不会**渲染 `role="dialog"` / `aria-modal`——那两个属性只在 preset 为
    card/dialog/confirm 时才由 naive 加上（见 naive-ui 的 BodyWrapper）。

    - `role="dialog"` + `aria-modal="true"`：告诉读屏器「这是一个模态框，
      外面那些内容现在不算数」；
    - `aria-labelledby` 指向标题，读屏器先念标题再说内容；
    - `tabindex="-1"` 让容器本身可编程聚焦（焦点陷阱有时会把焦点放在容器上）；
    - Esc 关闭由 `:close-on-esc` + `@esc` 负责（下面已经写了）。
  -->
  <NModal
    :show="show"
    :mask-closable="true"
    :close-on-esc="true"
    transform-origin="center"
    @mask-click="onDismiss(false)"
    @esc="onDismiss(false)"
  >
    <div
      class="cping-panel"
      role="dialog"
      aria-modal="true"
      aria-labelledby="restore-snapshot-title"
      tabindex="-1"
      style="width: min(520px, 92vw); padding: 18px"
    >
      <div id="restore-snapshot-title" style="font-size: 15px; font-weight: 600">
        {{ t('restore.title') }}
      </div>

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
