#!/usr/bin/env node
/**
 * 静态守卫：**结果状态符号只能有一个来源**。
 *
 * 为什么需要它：符号是「不依赖颜色」的那条信息通道。颜色可以被关掉
 * （`--no-color` / `NO_COLOR`），符号不能没有。而它此前有三份拷贝：
 *
 *   1. 核心  `crates/cping-core/src/classify.rs` 的 `ResultClass::symbol()`
 *   2. 前端  `apps/cping-gui/src/shared/types.ts` 的 `CLASS_SYMBOLS`
 *   3. 报表  `crates/cping-core/src/html.rs` 的 `class_symbol()`
 *
 * 三份里任何一份被改，用户就会在三个界面看到同一个状态对应三种符号。
 * 这份脚本把它们对起来，不一致就以 1 退出。
 *
 * 报表里的 `-`、`+`、`*`、`!` 与核心一致，只有 `low` 用的是 `·`：
 * 那是打印观感的选择（矩阵格里一个中点比井号干净）。**它是登记过的例外**，
 * 不是漏改——例外之外任何差异都会失败。
 */
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..')
const CLASSES = ['low', 'mid', 'high', 'offline', 'error']

/** 登记过的例外：`报表的 low 用中点`（见文件头说明）。 */
const REPORT_EXCEPTIONS = { low: '·' }

function read(rel) {
  return readFileSync(join(ROOT, rel), 'utf8')
}

/** 截出某个函数（或对象字面量）的正文：**按大括号配平**。
 *
 * 不能简单地找 `\n}` 结尾——impl 块内的函数结束在 `\n    }`，
 * 那样会一直取到整个 impl，把 `key()` / `color_key()` 里返回的
 * `"low"` / `"mid"` 也当成符号抓进来（后匹配覆盖先匹配，
 * 结果是「脚本看着通过了，其实比的是另一组字符串」）。
 */
function body(text, startMarker, label) {
  const start = text.indexOf(startMarker)
  if (start < 0) {
    fail(`找不到 ${label}（标记：${startMarker}）`)
  }
  const open = text.indexOf('{', start)
  if (open < 0) {
    fail(`找不到 ${label} 的函数体`)
  }
  let depth = 0
  for (let i = open; i < text.length; i++) {
    const ch = text[i]
    if (ch === '{') depth++
    else if (ch === '}') {
      depth--
      if (depth === 0) return text.slice(open, i + 1)
    }
  }
  fail(`找不到 ${label} 的结尾`)
}

/** `ResultClass::Low => "#"` 与 `Some(ResultClass::Low) => "#"` 两种形状。 */
function fromRust(text) {
  const out = {}
  const re = /ResultClass::(Low|Mid|High|Offline|Error)\)?\s*=>\s*"([^"]*)"/g
  let m
  while ((m = re.exec(text)) !== null) {
    out[m[1].toLowerCase()] = m[2]
  }
  return out
}

/** `low: '#',` 形状（TS）。 */
function fromTs(text) {
  const out = {}
  const re = /(low|mid|high|offline|error)\s*:\s*'([^']*)'/g
  let m
  while ((m = re.exec(text)) !== null) {
    out[m[1]] = m[2]
  }
  return out
}

const problems = []

function fail(msg) {
  console.error(`[symbols] ${msg}`)
  process.exit(1)
}

function check(map, name, allowExceptions) {
  for (const key of CLASSES) {
    if (!(key in map)) {
      problems.push(`${name} 缺少 ${key} 的符号`)
      continue
    }
    if (allowExceptions && REPORT_EXCEPTIONS[key] === map[key]) {
      continue
    }
    if (map[key] !== CORE[key]) {
      problems.push(
        `${name} 的 ${key} 是 ${JSON.stringify(map[key])}，核心是 ${JSON.stringify(CORE[key])}`,
      )
    }
  }
}

const CORE = fromRust(
  body(
    read('crates/cping-core/src/classify.rs'),
    'pub const fn symbol(self) -> &\'static str {',
    'ResultClass::symbol',
  ),
)

for (const key of CLASSES) {
  if (typeof CORE[key] !== 'string' || CORE[key] === '') {
    fail(`核心的 ${key} 没有符号`)
  }
}
const uniq = new Set(CLASSES.map((k) => CORE[k]))
if (uniq.size !== CLASSES.length) {
  fail(`核心符号有重复：${JSON.stringify(CORE)}`)
}

check(fromTs(body(read('apps/cping-gui/src/shared/types.ts'), 'CLASS_SYMBOLS', 'CLASS_SYMBOLS')), 'GUI 的 CLASS_SYMBOLS', false)
check(
  fromRust(body(read('crates/cping-core/src/html.rs'), 'fn class_symbol(', 'html::class_symbol')),
  '报表的 class_symbol',
  true,
)

if (problems.length > 0) {
  console.error('[symbols] 符号不一致（核心是唯一来源，见 scripts/check-symbols.mjs 的说明）：')
  for (const p of problems) console.error(`  - ${p}`)
  process.exit(1)
}

console.log(
  `[symbols] 校验通过：核心 ${JSON.stringify(CORE)}；GUI 与核心一致；` +
    `报表仅 ${JSON.stringify(REPORT_EXCEPTIONS)} 为登记例外。`,
)
