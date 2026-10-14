#!/usr/bin/env bash
# ===========================================================================
# CPing Linux 打包脚本（M11）
#
# 用法：
#   scripts/package.sh <version> <platform-tag> [bin...]
#
# 例：
#   scripts/package.sh 2.0.0 linux-x64 cping-tui cping-cli
#
# 二进制目录的解析顺序（见 resolve_bin_dir）：
#   1. 环境变量 BIN_DIR（显式指定，CI 里推荐直接给全路径）
#   2. $CARGO_TARGET_DIR/release —— 原生构建（不带 --target）的落点
#   3. $CARGO_TARGET_DIR/*/release —— 带 --target <triple> 构建时的落点
# 之所以要第 3 条：workflow 用 `--target x86_64-unknown-linux-gnu` 构建，
# 产物落在 target/<triple>/release/，而脚本原先只看 target/release，
# 于是「构建明明成功、打包却报缺二进制」。
#
# 产出（默认写入 release/<version>/）：
#   cping-tui-<version>-<tag>.tar.gz     随包附 README.txt
#   cping-cli-<version>-<tag>.tar.gz
#   SHA256SUMS.txt                        覆盖本次产出的全部压缩包
#
# 设计约束：
#   - 只打包**实际存在**的二进制；缺哪个就明确报缺哪个，不静默跳过。
#   - 二进制不存在时以非 0 退出，让 CI 立刻失败，而不是产出空包。
#   - 校验和清单可被 `sha256sum -c SHA256SUMS.txt` 直接校验。
# ===========================================================================
set -euo pipefail

VERSION="${1:-}"
TAG="${2:-}"
shift 2 2>/dev/null || true
BINS=("$@")

if [[ -z "$VERSION" || -z "$TAG" ]]; then
  echo "用法: scripts/package.sh <version> <platform-tag> [bin...]" >&2
  echo "例:   scripts/package.sh 2.0.0 linux-x64 cping-tui cping-cli" >&2
  exit 2
fi

if [[ ${#BINS[@]} -eq 0 ]]; then
  BINS=(cping-tui cping-cli)
fi

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_BASE="${CARGO_TARGET_DIR:-$REPO/target}"
OUT_DIR="${OUT_DIR:-$REPO/release/$VERSION}"
STAGE_ROOT="$(mktemp -d)"
trap 'rm -rf "$STAGE_ROOT"' EXIT

# 某目录里是否**全部**目标二进制都在
all_bins_present() {
  local dir="$1" bin
  for bin in "${BINS[@]}"; do
    [[ -x "$dir/$bin" ]] || return 1
  done
  return 0
}

# 找到实际存放二进制的 release 目录
resolve_bin_dir() {
  if [[ -n "${BIN_DIR:-}" ]]; then
    echo "$BIN_DIR"
    return
  fi
  local native="$TARGET_BASE/release"
  all_bins_present "$native" && { echo "$native"; return; }
  local cand
  for cand in "$TARGET_BASE"/*/release; do
    [[ -d "$cand" ]] || continue
    all_bins_present "$cand" && { echo "$cand"; return; }
  done
  echo "$native" # 都没找到，回落到原生路径，好让下面的报错指到最可能的位置
}

TARGET_DIR="$(resolve_bin_dir)"

mkdir -p "$OUT_DIR"

echo "==== CPing 打包 ===="
echo "  版本       : $VERSION"
echo "  平台标签   : $TAG"
echo "  二进制目录 : $TARGET_DIR"
echo "  输出目录   : $OUT_DIR"
echo

missing=0
for bin in "${BINS[@]}"; do
  if [[ ! -x "$TARGET_DIR/$bin" ]]; then
    echo "[缺失] $TARGET_DIR/$bin 不存在或不可执行" >&2
    missing=1
  fi
done
if [[ $missing -ne 0 ]]; then
  echo >&2
  echo "先构建：cargo build --release -p cping-tui -p cping-cli" >&2
  echo "（若用了 --target <triple>，可用 BIN_DIR=<...>/<triple>/release 指定）" >&2
  echo "已找到的候选目录：" >&2
  ls -d "$TARGET_BASE"/release "$TARGET_BASE"/*/release 2>/dev/null >&2 || true
  exit 1
fi

# ---------------------------------------------------------------------------
# 统一模板：打包时把版本、平台与用法写进包内 README.txt，
# 这样离线拿到压缩包的人不用回仓库就知道怎么跑。
# ---------------------------------------------------------------------------
write_readme() {
  local bin="$1" dest="$2"
  cat > "$dest/README.txt" <<EOF
CPing $VERSION（$TAG）
========================================

本包内含二进制：$bin

运行
----
  ./$bin --help

退出码（cping-cli 无头模式）
----------------------------
  0  全部目标在线
  1  存在离线/超时目标
  2  参数错误
  3  权限不足或后端不可用

权限说明
--------
ICMP 探测需要原始套接字权限：
  - Linux ：需要 CAP_NET_RAW，或直接以 root 运行；
            普通用户下程序会如实报告「权限不足」，不会记成「离线」。
  - 无权限时可用 TCP 方式（--mode tcp --port 80），无特权即可运行。

校验
----
  sha256sum -c SHA256SUMS.txt
EOF
}

cd "$TARGET_DIR"
for bin in "${BINS[@]}"; do
  stage="$STAGE_ROOT/$bin-$VERSION-$TAG"
  mkdir -p "$stage"
  cp "$TARGET_DIR/$bin" "$stage/$bin"
  chmod 755 "$stage/$bin"
  write_readme "$bin" "$stage"

  archive="$OUT_DIR/$bin-$VERSION-$TAG.tar.gz"
  rm -f "$archive"
  tar -czf "$archive" -C "$STAGE_ROOT" "$bin-$VERSION-$TAG"
  echo "[产出] $archive  ($(du -h "$archive" | cut -f1))"
done

# ---------------------------------------------------------------------------
# 校验和清单：只覆盖本次产出的包。在输出目录内计算，写相对文件名，
# 便于把整目录拷走之后原地 `sha256sum -c`。
# ---------------------------------------------------------------------------
SUMS="$OUT_DIR/SHA256SUMS.txt"
: > "$SUMS"
cd "$OUT_DIR"
for bin in "${BINS[@]}"; do
  printf '%s *%s\n' "$(sha256sum "$bin-$VERSION-$TAG.tar.gz" | cut -d' ' -f1)" \
    "$bin-$VERSION-$TAG.tar.gz" >> "$SUMS"
done

echo
echo "==== 校验和 ($SUMS) ===="
cat "$SUMS"
echo
echo "自检：cd \"$OUT_DIR\" && sha256sum -c SHA256SUMS.txt"
