#!/usr/bin/env bash
# ===========================================================================
# CPing Android release APK 签名（M17）
#
# 用法：
#   scripts/sign-android.sh <apk 或 apk 目录>
#
# 行为：
#   · 提供了 keystore（CPING_ANDROID_KEYSTORE_BASE64 + 三个密码/别名变量）时，
#     用 apksigner 做**正式**签名（v2/v3 方案 + v1 兼容），并立即 verify。
#   · 没提供时**不做任何伪装**：只校验并明确报告「当前是 debug 签名，不可上架」，
#     同时给出补签命令。
#
# 所需密钥（GitHub Secrets）：
#   CPING_ANDROID_KEYSTORE_BASE64     keystore 的 base64
#   CPING_ANDROID_KEYSTORE_PASSWORD   keystore 口令
#   CPING_ANDROID_KEY_ALIAS           密钥别名
#   CPING_ANDROID_KEY_PASSWORD        密钥口令
#
# 为什么必须显式区分：debug 密钥是公开的（androiddebugkey / android），
# 任何人都能用它签出一个「看起来正常」的 APK。把 debug 签名的产物当正式版
# 发出去，等于把「这个包是我们发的」这条保证彻底作废。
# ===========================================================================
set -euo pipefail

TARGET="${1:-}"
if [[ -z "$TARGET" ]]; then
  echo "用法: scripts/sign-android.sh <apk 或 apk 目录>" >&2
  exit 2
fi
if [[ ! -e "$TARGET" ]]; then
  echo "路径不存在：$TARGET" >&2
  exit 2
fi

mapfile -t APKS < <(
  if [[ -d "$TARGET" ]]; then
    find "$TARGET" -maxdepth 2 -name '*.apk' | sort
  else
    printf '%s\n' "$TARGET"
  fi
)
if [[ ${#APKS[@]} -eq 0 ]]; then
  echo "没有找到任何 .apk（$TARGET）" >&2
  exit 1
fi

find_apksigner() {
  if command -v apksigner >/dev/null 2>&1; then
    command -v apksigner
    return 0
  fi
  # Android SDK 的位置由 ANDROID_HOME / ANDROID_SDK_ROOT 给出
  local sdk="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}"
  [[ -z "$sdk" ]] && return 1
  find "$sdk/build-tools" -name 'apksigner*' -type f 2>/dev/null | sort -V | tail -1
}

APKSIGNER="$(find_apksigner || true)"
if [[ -z "$APKSIGNER" ]]; then
  echo "找不到 apksigner（需要 Android SDK build-tools）" >&2
  echo "当前产物**未做签名校验**，请在有 SDK 的环境里重跑本脚本。" >&2
  exit 1
fi

have_key() {
  [[ -n "${CPING_ANDROID_KEYSTORE_BASE64:-}" \
    && -n "${CPING_ANDROID_KEYSTORE_PASSWORD:-}" \
    && -n "${CPING_ANDROID_KEY_ALIAS:-}" \
    && -n "${CPING_ANDROID_KEY_PASSWORD:-}" ]]
}

if have_key; then
  KS="$(mktemp -d)/cping.keystore"
  trap 'rm -f "$KS"' EXIT
  # 密钥从 Secrets 解码到临时目录，绝不落进仓库或产物
  printf '%s' "$CPING_ANDROID_KEYSTORE_BASE64" | base64 -d > "$KS"
  for apk in "${APKS[@]}"; do
    echo "正式签名：$apk"
    "$APKSIGNER" sign \
      --ks "$KS" \
      --ks-pass "pass:$CPING_ANDROID_KEYSTORE_PASSWORD" \
      --ks-key-alias "$CPING_ANDROID_KEY_ALIAS" \
      --key-pass "pass:$CPING_ANDROID_KEY_PASSWORD" \
      --v1-signing-enabled true --v2-signing-enabled true --v3-signing-enabled true \
      "$apk"
    echo "校验：$apk"
    "$APKSIGNER" verify --verbose --print-certs "$apk"
  done
  echo
  echo "全部 APK 已用正式密钥签名并通过校验。"
  exit 0
fi

# ---- 没有密钥 -----------------------------------------------------------
echo "未提供 keystore（CPING_ANDROID_KEYSTORE_BASE64 等四个变量），"
echo "当前 APK 用的是 **Android Debug 密钥**签名——可以侧载试用，**不可上架**。"
echo
for apk in "${APKS[@]}"; do
  echo "==== $apk ===="
  # verify 失败不直接退出：这里的目的就是**如实报告**现状
  "$APKSIGNER" verify --verbose --print-certs "$apk" || true
  echo
done
echo "补签方法（在有 keystore 的机器上）："
echo "  export CPING_ANDROID_KEYSTORE_BASE64=\"\$(base64 -w0 your.keystore)\""
echo "  export CPING_ANDROID_KEYSTORE_PASSWORD=... CPING_ANDROID_KEY_ALIAS=... CPING_ANDROID_KEY_PASSWORD=..."
echo "  scripts/sign-android.sh <apk 目录>"
echo
echo "注意：keystore 一旦丢失就**无法**给已上架的应用发更新（同包名不接受换密钥）。"
echo "请把 keystore 与口令离线备份之外，另存一份到受控的密钥管理服务。"
