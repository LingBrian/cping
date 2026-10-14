#!/usr/bin/env bash
# ===========================================================================
# 可移植性守卫（CI + 本地都能跑）
#
# 为什么需要它：v2.0.0 发布期间连踩三次**同一类**缺陷——
# 把「本机环境的耦合配置」提交进了仓库，本地全绿、CI 全红：
#   1. `.cargo/config.toml` 里的 `[net] offline = true`
#      → CI 报 no matching package named tokio/indexmap found
#   2. `gen/android/gradle.properties` 里的
#      `org.gradle.java.home=D:/Program Files/Android/jdk-17`
#      → CI 报 Value 'D:/...' ... is invalid (Java home supplied is invalid)
#   3. （同类但另一面）`scripts/package.sh` 的二进制目录契约与 workflow 不一致
#
# 前两条都能被本脚本静态拦住，因此放进 CI 的必过项。
#
# 用法：
#   scripts/check-portability.sh          # 在仓库根或任意子目录执行
# 退出码：0 通过；1 发现不可移植的配置
# ===========================================================================
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO"

if ! git rev-parse --git-dir >/dev/null 2>&1; then
  echo "跳过：不在 git 仓库内，无法确定受检查的文件清单" >&2
  exit 0
fi

# 只检查**会真正影响构建**的文件。文档（docs/*.md）与 Windows 专用的
# 本地脚本（*.cmd / create-desktop-shortcuts.py）里出现盘符路径是正常的，
# 不在此列——这正是用清单而不是全仓库扫描的原因。
mapfile -t FILES < <(git ls-files -- \
  'Cargo.toml' '**/Cargo.toml' \
  '.cargo/config.toml' \
  '.github/workflows/*.yml' \
  'scripts/package.sh' 'scripts/package.ps1' \
  '**/gradle.properties' '**/build.gradle.kts' \
  '**/tauri.conf.json' '**/*.toml' \
  | grep -v '^vendor' || true)

if [[ ${#FILES[@]} -eq 0 ]]; then
  echo "跳过：没有匹配到需要检查的文件" >&2
  exit 0
fi

bad=0

echo "==== 可移植性检查（${#FILES[@]} 个构建相关文件） ===="

# --- 1. 盘符绝对路径 -------------------------------------------------------
# 只匹配「前面不是字母数字」的盘符（避免把 https:// 里的 `s:` 当成盘符），
# 且跳过注释行——注释里出现本机路径是在解释「为什么不能这么写」，不是配置本身。
for f in "${FILES[@]}"; do
  if hits="$(grep -nE '(^|[^A-Za-z0-9])[A-Za-z]:[\\/]' "$f" 2>/dev/null \
      | grep -vE '^[0-9]+:[[:space:]]*#' || true)"; then
    if [[ -n "$hits" ]]; then
      echo >&2
      echo "[不可移植] $f 含 Windows 盘符绝对路径：" >&2
      printf '%s\n' "$hits" | sed 's/^/    /' >&2
      echo "    → 改用相对路径，或环境变量（如 JAVA_HOME / CARGO_TARGET_DIR）；" >&2
      echo "      机器专属的设置请放用户级配置文件，不要提交。" >&2
      bad=1
    fi
  fi
done

# --- 2. cargo 离线开关 ------------------------------------------------------
if [[ -f .cargo/config.toml ]]; then
  if hits="$(grep -nE '^[[:space:]]*offline[[:space:]]*=[[:space:]]*true' .cargo/config.toml 2>/dev/null || true)"; then
    if [[ -n "$hits" ]]; then
      echo >&2
      echo "[不可移植] .cargo/config.toml 打开了 cargo 离线模式：" >&2
      printf '%s\n' "$hits" | sed 's/^/    /' >&2
      echo "    → cargo 解析依赖图与校验 Cargo.lock 时必须能访问 registry 索引，" >&2
      echo "      即使可选 feature 未启用；写死 offline 会让 CI 与全新克隆全部失败。" >&2
      echo "      需要离线时请在命令行按需开启：cargo build --offline" >&2
      bad=1
    fi
  fi
  if hits="$(grep -nE '^[[:space:]]*replace-with[[:space:]]*=[[:space:]]*"vendored-sources"' .cargo/config.toml 2>/dev/null || true)"; then
    if [[ -n "$hits" ]]; then
      echo >&2
      echo "[不可移植] .cargo/config.toml 指向了本地 vendored 快照：" >&2
      printf '%s\n' "$hits" | sed 's/^/    /' >&2
      echo "    → vendor-offline/ 按设计不入库，该配置只应存在于无外网机器的本地副本里。" >&2
      bad=1
    fi
  fi
fi

if [[ $bad -ne 0 ]]; then
  echo >&2
  echo "可移植性检查未通过。" >&2
  exit 1
fi

echo "通过：未发现盘符绝对路径 / cargo 离线开关 / vendored 源替换。"
