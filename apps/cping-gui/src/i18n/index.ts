import { ref } from 'vue'
import { zh } from './zh'
import { en } from './en'

/**
 * 自研轻量 i18n。
 *
 * 为什么不用 vue-i18n：本项目只需要「查表 + 顺序占位符」，引入一个完整的
 * i18n 运行时（以及它的插件安装、SFC 预编译、复数规则）不划算。这里总共
 * 不到 40 行，且类型系统已经能保证「en 不缺键」。
 */

export type Lang = 'zh' | 'en'

/** 字典的键集合由 `zh` 派生；`en` 声明成 `Dict`，少键/多键都会编译报错。 */
export type Dict = Record<keyof typeof zh, string>

/** 语言持久化用的 localStorage 键。不翻译、不跟随语言变化，否则切换后会读不到。 */
const STORAGE_KEY = 'cping.gui.lang'

const dicts: Record<Lang, Dict> = { zh, en }

/**
 * 首次启动的默认语言：先看 localStorage 里有没有上次的选择；
 * 没有就跟随系统/浏览器语言（`zh` 开头算中文，其余一律英文）；都拿不到就中文。
 *
 * 这里的 try/catch 是必要的：某些隐私模式或旧 WebView 里访问
 * `localStorage` / `navigator` 会直接抛异常，不能让界面因此起不来。
 */
function detectLang(): Lang {
  try {
    const saved = localStorage.getItem(STORAGE_KEY)
    if (saved === 'zh' || saved === 'en') return saved
  } catch {
    // 读不到存储就落到系统语言判断
  }
  try {
    const nav = navigator.language || ''
    return nav.toLowerCase().startsWith('zh') ? 'zh' : 'en'
  } catch {
    return 'zh'
  }
}

/**
 * 当前语言。用 `ref` 而不是普通变量，是为了让模板里的 `t(...)` 变成响应式依赖：
 * 切换语言时 Vue 会自动重新渲染所有读到过它的组件。
 */
export const lang = ref<Lang>(detectLang())

export function currentLang(): Lang {
  return lang.value
}

/** 切换语言并持久化。写存储失败不影响本次会话（只是下次启动回到默认判断）。 */
export function setLang(next: Lang): void {
  lang.value = next
  try {
    localStorage.setItem(STORAGE_KEY, next)
  } catch {
    // 存储不可用：本次会话仍然生效
  }
}

/**
 * 取词条并做顺序替换。
 *
 * 语义与 Rust 核心层保持一致：
 * - 占位符是 `{}`，按出现顺序依次替换；
 * - 参数不足时，多出来的 `{}` **原样保留**（而不是替换成空串），
 *   这样漏参在界面上是可见的，不会静默丢信息；
 * - 参数多余则忽略；
 * - 键缺失时退到中文，再退到键名本身，绝不返回 undefined。
 *
 * 参数类型放宽到 `null | undefined` 是为了适配模板里的可选链取值
 * （例如 `pending?.scanTime` 的静态类型是 `string | undefined`）。
 * 真值为 null/undefined 时替换成空串——把字面量 "null" / "undefined"
 * 摆到用户面前没有任何意义；「缺参数」这种情况由「保留 {}」这条规则负责提醒。
 */
export function t(key: string, ...args: (string | number | null | undefined)[]): string {
  const table = dicts[lang.value] as Record<string, string> | undefined
  const fallback = dicts.zh as Record<string, string>
  const raw = table?.[key] ?? fallback[key] ?? key
  if (args.length === 0) return raw
  let i = 0
  return raw.replace(/\{\}/g, (placeholder) => {
    if (i >= args.length) return placeholder
    const value = args[i]
    i += 1
    return value == null ? '' : String(value)
  })
}
