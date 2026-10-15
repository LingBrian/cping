<script setup lang="ts">
import { onMounted } from 'vue'
import { NButton, NEmpty } from 'naive-ui'
import { t } from '../i18n'
import { usePingStore } from '../stores/ping'

const store = usePingStore()

onMounted(() => {
  // 打开面板时刷新一次；之后每次终态校准（resyncFromBackend）会自动带上最新流水。
  void store.loadChanges()
})

function kindClass(kind: string): string {
  return `change-kind change-kind--${kind.replace(/_/g, '-')}`
}
</script>

<template>
  <div class="change-panel">
    <div class="change-panel__head">
      <span>
        {{ t('change.head', store.changeTotal, store.round) }}
      </span>
      <span v-if="store.diffSummary" class="change-panel__diff">{{ store.diffSummary }}</span>
      <NButton size="tiny" quaternary @click="store.loadChanges()">{{ t('btn.refresh') }}</NButton>
    </div>

    <!-- 空态要解释清楚「为什么没有变化」，否则用户会以为功能坏了 -->
    <div v-if="store.changeFeed.length === 0" class="change-panel__empty">
      <NEmpty :description="t('change.empty')">
        <template #extra>
          <div style="font-size: 12px; color: #6b7280; max-width: 320px; line-height: 1.6">
            {{ t('change.emptyHint') }}
          </div>
        </template>
      </NEmpty>
    </div>

    <!-- 变化列表会整块换掉（每轮结束时刷新），加 `aria-live="polite"` 让读屏器
         把新出现的变化念出来——否则纯键盘/读屏用户不看这一页就完全不知道
         「刚才那一轮有主机上线了」。容器级而不是逐条：一次播报一整批，
         比 255 条各播一次要好受得多。 -->
    <ul v-else class="change-panel__list" aria-live="polite">
      <li v-for="(c, i) in store.changeFeed" :key="`${c.round}-${c.index}-${i}`" class="change-panel__item">
        <span class="change-panel__round">{{ t('round.n', c.round) }}</span>
        <span :class="kindClass(c.kind)">{{ c.kindLabel }}</span>
        <span class="change-panel__ip cping-mono">{{ c.ip }}</span>
        <span class="change-panel__summary">{{ c.summary }}</span>
      </li>
    </ul>
  </div>
</template>
