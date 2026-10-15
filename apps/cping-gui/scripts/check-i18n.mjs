#!/usr/bin/env node
/**
 * 零依赖的 i18n 校验脚本（用 `node` 直接跑，不引任何包）。
 *
 * 为什么是这样一个脚本，而不是在 TypeScript 里做：
 * `en: Dict` 的类型标注已经能保证「en 的键和 zh 完全一致」，但类型系统管不了
 * 下面这些「语义」问题，只能拿文本检查：
 *   1. zh / en 的键集合完全一致（双向：en 不能多键，也不能少键）；
 *      类型系统只管 en 缺不缺键，管不了 en 有没有多余的键。
 *   2. 英文条目的值里不含中日韩字符（把中文原文粘过去、或漏翻的表现）。
 *   3. 中文条目的值里至少含一个中日韩字符（把英文粘到 zh 上的表现）。
 *   4. 每条中英条目的 `{}` 占位符个数相同（否则英文译文会多吃/少吃一个参数）。
 *
 * 本来想直接 import `.ts`，但 Node 默认不认识 TypeScript，要么上 loader
 * （等于引依赖），要么改构建。这里选最省事也最稳的路：把 zh.ts / en.ts 当成
 * 纯文本，用正则逐行解析。为此两个字典文件遵守统一的书写约定——
 * 每条一行、键值都用单引号、值内不出现单引号（详见文件头注释）。
 */
import { readFileSync } from 'node:fs'

const ZH_URL = new URL('../src/i18n/zh.ts', import.meta.url)
const EN_URL = new URL('../src/i18n/en.ts', import.meta.url)

/** 一行词条：`  'key': 'value',` */
const ENTRY_RE = /^\s*'([^']+)'\s*:\s*'([^']*)'\s*,?\s*$/

/**
 * 中日韩相关码点区间（与需求一致）：
 * 0x3000–0x303F 中文标点、0x3040–0x30FF 日文假名、0x3400–0x4DBF 扩展 A、
 * 0x4E00–0x9FFF 基本汉字、0xF900–0xFAFF 兼容汉字、0xFF00–0xFFEF 全角字符。
 */
const CJK_RE = /[\u3000-\u303F\u3040-\u30FF\u3400-\u4DBF\u4E00-\u9FFF\uF900-\uFAFF\uFF00-\uFFEF]/

const PLACEHOLDER_RE = /\{\}/g

/**
 * 解析字典文件文本，返回 Map<键, 值>。
 * 同文件内出现重复键直接算错误：对象字面量里后者会覆盖前者，
 * 会让「键对照」悄悄失真。
 */
function parseDict(text, file) {
  const out = new Map()
  const dupes = []
  for (const line of text.split(/\r?\n/)) {
    const m = ENTRY_RE.exec(line)
    if (!m) continue
    const [, key, value] = m
    if (out.has(key)) dupes.push(key)
    out.set(key, value)
  }
  if (out.size === 0) {
    throw new Error(`${file}: 一条词条都没解析出来，检查文件格式是否仍为「每行一个 '键': '值',」`)
  }
  return { entries: out, dupes }
}

function countPlaceholders(value) {
  const found = value.match(PLACEHOLDER_RE)
  return found ? found.length : 0
}

function main() {
  const errors = []

  let zh
  let en
  try {
    zh = parseDict(readFileSync(ZH_URL, 'utf8'), ZH_URL.pathname)
    en = parseDict(readFileSync(EN_URL, 'utf8'), EN_URL.pathname)
  } catch (e) {
    console.error(`[i18n] 读取字典失败：${e.message}`)
    process.exit(1)
  }

  for (const k of zh.dupes) errors.push(`zh.ts 存在重复键：'${k}'`)
  for (const k of en.dupes) errors.push(`en.ts 存在重复键：'${k}'`)

  // 1) 键集合必须完全一致（双向）
  for (const k of zh.entries.keys()) {
    if (!en.entries.has(k)) errors.push(`en.ts 缺少键：'${k}'`)
  }
  for (const k of en.entries.keys()) {
    if (!zh.entries.has(k)) errors.push(`en.ts 多出键：'${k}'（zh.ts 里没有）`)
  }

  // 只对两边都有的键做后续检查
  for (const [key, zhValue] of zh.entries) {
    const enValue = en.entries.get(key)
    if (enValue === undefined) continue

    // 4) 占位符个数一致
    const zhN = countPlaceholders(zhValue)
    const enN = countPlaceholders(enValue)
    if (zhN !== enN) {
      errors.push(`'${key}' 的 {} 个数不一致：zh=${zhN}，en=${enN}`)
    }

    // 2) 英文值不得含中日韩字符
    if (CJK_RE.test(enValue)) {
      errors.push(`'${key}' 的英文值含中日韩字符：${enValue}`)
    }

    // 3) 中文值至少含一个中日韩字符
    if (!CJK_RE.test(zhValue)) {
      errors.push(`'${key}' 的中文值不含任何中日韩字符（疑似把英文粘到了 zh）：${zhValue}`)
    }
  }

  if (errors.length > 0) {
    console.error(`[i18n] 校验失败，共 ${errors.length} 个问题：`)
    for (const line of errors) console.error(`  - ${line}`)
    process.exit(1)
  }

  console.log(
    `[i18n] 校验通过：zh/en 各 ${zh.entries.size} 条词条，键一致、无中文残留、占位符个数一致。`,
  )
}

main()
