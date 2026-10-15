<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { NButton, NSelect } from 'naive-ui'
import { usePingStore } from '../stores/ping'
import { t } from '../i18n'

/**
 * M16：扫描模板（配置文件里的 `[[profiles]]`）。
 *
 * 三条刻意的取舍：
 *
 * 1. **不在前端合并模板值**。界面只把「哪些项是我已经填好的」传给后端
 *    （`ProfileGaps`），由 Rust 侧的 `merge_profile` 一处判定「只填空缺」。
 *    合并规则写在界面里，迟早会与 CLI / TUI 的优先级漂移。
 * 2. **来源必须显式**。填充了哪些字段、哪些字段保留了用户的输入，都写在面板上。
 *    「模板悄悄换掉你的输入」是这类功能最难查的一类问题。
 * 3. **摘要不自拼**。`describe` 是后端按当前语言渲染好的成品（规格 · 目标数 ·
 *    range · 超时 · 并发 · 方式 · 说明），前端只展示——两份措辞必然漂移。
 */
const store = usePingStore()

/** 下拉里选中的模板名。默认落在后端标出的 `default_profile` 上（与 TUI 一致）。 */
const selected = ref<string | null>(null)

const options = computed(() =>
  store.profiles.map((p) => ({
    // 「哪一份是默认模板」标在标签上，省得用户去翻配置文件
    label: p.isDefault ? `${p.name} · ${t('profile.defaultMark')}` : p.name,
    value: p.name,
  })),
)

const current = computed(() => store.profiles.find((p) => p.name === selected.value) ?? null)

/**
 * 最近一次应用的结果，**仅当它对应的就是当前选中的那一份**时才展示：
 * 用户换了下拉选项之后，上一份模板的说明就成了过时的噪音。
 */
const applied = computed(() =>
  store.lastProfileApply && store.lastProfileApply.name === selected.value
    ? store.lastProfileApply
    : null,
)

async function onApply() {
  if (!selected.value) return
  await store.applyProfileByName(selected.value)
}

/** 换选中的模板。用显式函数而不是模板内联赋值，免得依赖模板对 ref 的自动解包。 */
function pick(name: string) {
  selected.value = name
}

onMounted(async () => {
  await store.loadProfiles()
  // 默认选中 `default_profile` 那一份；没有默认就选第一份，没有模板就留空
  const preferred = store.profiles.find((p) => p.isDefault) ?? store.profiles[0]
  if (preferred && !selected.value) selected.value = preferred.name
})
</script>

<template>
  <div class="cping-panel">
    <div style="font-weight: 600; font-size: 13px; margin-bottom: 6px">
      {{ t('profile.title') }}
    </div>

    <NSelect
      :value="selected"
      size="small"
      :options="options"
      :placeholder="t('profile.select')"
      :disabled="store.profiles.length === 0 || store.profileBusy"
      @update:value="pick"
    />

    <!-- 选中后的摘要：后端渲染好的成品 -->
    <div
      v-if="current"
      style="font-size: 11px; color: #6b7280; margin-top: 6px; word-break: break-all"
    >
      {{ current.describe }}
    </div>
    <div v-if="current" style="font-size: 11px; color: #9ca3af; margin-top: 2px">
      {{
        current.targets != null
          ? t('profile.targets', current.targets)
          : t('profile.targetsUnknown')
      }}
    </div>

    <!-- 空列表必须说明原因：文件不在 / 读不动 / 里面没有 [[profiles]] -->
    <div
      v-for="(n, i) in store.profilesNotes"
      :key="i"
      style="font-size: 11px; color: #b45309; margin-top: 6px"
    >
      {{ n }}
    </div>
    <div
      v-if="store.profilesPath"
      style="font-size: 11px; color: #9ca3af; margin-top: 4px; word-break: break-all"
    >
      {{ t('profile.path', store.profilesPath) }}
    </div>

    <NButton
      style="width: 100%; margin-top: 8px"
      size="small"
      :disabled="!selected || store.profileBusy"
      :loading="store.profileBusy"
      @click="onApply"
    >
      {{ t('btn.apply') }}
    </NButton>

    <!-- 来源说明：哪些字段来自模板、哪些**没有**覆盖用户已填的值。
         这一段不能省——它就是「不悄悄换掉用户输入」的兑现方式。 -->
    <template v-if="applied">
      <div
        v-if="applied.filled.length > 0"
        style="font-size: 11px; color: #4b5563; margin-top: 6px"
      >
        {{ t('profile.filledFrom', applied.name, store.fieldSummary(applied.filled)) }}
      </div>
      <div
        v-if="applied.skipped.length > 0"
        style="font-size: 11px; color: #b45309; margin-top: 4px"
      >
        {{ t('profile.skippedKept', store.fieldSummary(applied.skipped)) }}
      </div>
      <!-- 模板里的 range 本界面用不上（目标固定按 standard 展开）：说出来，别默默换规格 -->
      <div v-if="applied.rangeUnapplied" style="font-size: 11px; color: #b45309; margin-top: 4px">
        {{ t('profile.rangeUnapplied', applied.range) }}
      </div>
    </template>

    <div style="font-size: 11px; color: #9ca3af; margin-top: 6px">
      {{ t('profile.hint') }}
    </div>
  </div>
</template>
