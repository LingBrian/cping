<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { NButton, NInputNumber, NSwitch } from 'naive-ui'
import { usePingStore } from '../stores/ping'
import { t } from '../i18n'

/**
 * M17 / M18：安全策略（限速 / 目标数上限 / 保留网段守卫）。
 *
 * 三条刻意的取舍：
 *
 * 1. **默认值仍然是安全的**。界面把策略做成可调的，但后端在没人设置过时给的是
 *    限速 500 包/秒 + 拒绝保留网段——「可调」不该等于「默认放松」。
 * 2. **越界的值不夹取**。校验在后端（[`ScanPolicy::validate_in`]），失败时
 *    后端**什么都不改**并给出已本地化的原因；界面把原因原样显示，不自己改成
 *    一个「合法」的值。「我明明设了 1000 怎么还是 500」比报错难查得多。
 * 3. **草稿只在点「应用」时提交**。边打字边改策略会在输入过程中产生一堆中间态
 *    （速率刚清零、上限还没填），既没意义也容易误伤。
 */
const store = usePingStore()

const pps = ref<number | null>(null)
const targets = ref<number | null>(null)
const allowReserved = ref(false)
const localError = ref('')

/** 后端给了新的「当前生效策略」就回填草稿（首次加载、应用成功后各一次）。 */
watch(
  () => store.policy,
  (p) => {
    if (!p) return
    pps.value = p.maxPps
    targets.value = p.maxTargets
    allowReserved.value = p.allowReserved
    localError.value = ''
  },
  { immediate: true },
)

/**
 * 上限提示。数字全部来自**后端返回的 DTO**（核心的常量），
 * 界面不自己记一份——两边各记一套迟早漂移。
 */
const limits = computed(() => {
  const p = store.policy
  return p ? t('policy.limits', p.maxPpsLimit, p.maxTargetsLimit) : ''
})

async function onApply() {
  localError.value = ''
  if (!store.policy) return
  if (pps.value == null || targets.value == null) {
    localError.value = t('policy.needNumbers')
    return
  }
  const ok = await store.updatePolicy(pps.value, targets.value, allowReserved.value)
  // 失败时**保留草稿**（用户能接着改），只把后端给的原因摆出来；
  // 成功时 watch 会拿新的生效值回填，两边不会不一致。
  if (!ok) localError.value = store.status
}

onMounted(() => {
  void store.loadPolicy()
})
</script>

<template>
  <div class="cping-panel">
    <div style="font-weight: 600; font-size: 13px; margin-bottom: 6px">
      {{ t('policy.title') }}
    </div>

    <div style="font-size: 11px; color: #6b7280">
      {{ store.policy ? t('policy.current', store.policy.describe) : t('policy.loading') }}
    </div>

    <div
      style="display: flex; align-items: center; justify-content: space-between; margin-top: 8px"
    >
      <span style="font-size: 12px">{{ t('policy.pps') }}</span>
      <!-- 刻意**不设 `:max`**：越界的值要让后端拒绝并给出原因，而不是被输入框
           悄悄夹到上限——「我明明设了 200000 怎么还是 100000」正是最不能接受的那类问题。
           `:min="0"` 保留：负数是 `u32` 根本装不下的类型问题，不是策略问题。 -->
      <NInputNumber v-model:value="pps" size="small" style="width: 132px" :min="0" />
    </div>

    <div
      style="display: flex; align-items: center; justify-content: space-between; margin-top: 6px"
    >
      <span style="font-size: 12px">{{ t('policy.targets') }}</span>
      <!-- 同理不设 `:max`，也不设 `:min="1"`：0 是合法输入，由后端给出
           「必须大于 0」的说明才有意义。 -->
      <NInputNumber v-model:value="targets" size="small" style="width: 132px" :min="0" />
    </div>

    <div
      style="display: flex; align-items: center; justify-content: space-between; margin-top: 6px"
    >
      <span style="font-size: 12px">{{ t('policy.allowReserved') }}</span>
      <NSwitch v-model:value="allowReserved" size="small" />
    </div>

    <!-- 安全开关**必须有显式警示**：打开它会去探测组播 / 文档网段这类地址，
         可能打扰同网段的其他人。不能无声开启。 -->
    <div v-if="allowReserved" style="font-size: 11px; color: #b45309; margin-top: 6px">
      {{ t('policy.warnReserved') }}
    </div>

    <div style="font-size: 11px; color: #9ca3af; margin-top: 4px">{{ limits }}</div>
    <div style="font-size: 11px; color: #9ca3af; margin-top: 2px">{{ t('policy.hint') }}</div>
    <div v-if="localError" style="font-size: 11px; color: #d03050; margin-top: 4px">
      {{ localError }}
    </div>

    <NButton
      style="width: 100%; margin-top: 8px"
      size="small"
      :disabled="!store.policy"
      :loading="store.policyBusy"
      @click="onApply"
    >
      {{ t('btn.apply') }}
    </NButton>
  </div>
</template>
