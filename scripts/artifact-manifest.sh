#!/usr/bin/env bash
# ===========================================================================
# CPing 制品清单与（可选）签名（M17）
#
# 用法：
#   scripts/artifact-manifest.sh <version> <dist-dir>
#
# 例：
#   scripts/artifact-manifest.sh 2.5.0 release/2.5.0
#
# 产出（写入 <dist-dir>）：
#   MANIFEST.json        每个产物的 sha256 / 字节数 + 构建来源（版本/提交/工具链）
#   MANIFEST.json.asc    仅在提供了签名密钥时生成（gpg 分离签名）
#
# 关于「签名」这件事，本脚本的立场是**不伪造**：
#   · 没有密钥就签不出签名。这不是可以糊弄过去的步骤。
#   · 所以未提供密钥时，脚本会写 `"signed": false` 并**打印补签方法**，
#     而不是生成一个看起来像签名的东西，也不把 sha256 说成签名。
#     sha256 只能证明「文件没被改动」，不能证明「是谁发布的」。
#   · 提供 `CPING_SIGNING_KEY`（ASCII-armored 私钥）后有 gpg 就真签。
#
# 各平台的原生签名（Windows Authenticode / Android APK / Linux deb）见
# docs/signing.md 与同目录下的 sign-windows.ps1 / sign-android.sh。
# ===========================================================================
set -euo pipefail

VERSION="${1:-}"
DIST="${2:-}"

if [[ -z "$VERSION" || -z "$DIST" ]]; then
  echo "用法: scripts/artifact-manifest.sh <version> <dist-dir>" >&2
  exit 2
fi
if [[ ! -d "$DIST" ]]; then
  echo "产物目录不存在：$DIST" >&2
  exit 2
fi
# 版本号会直接进 JSON；限定字符集，避免拼出非法 JSON
if [[ ! "$VERSION" =~ ^[0-9A-Za-z._+-]+$ ]]; then
  echo "版本号含不安全字符：$VERSION（只允许 0-9 A-Z a-z . _ + -）" >&2
  exit 2
fi

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$DIST"

# JSON 字符串转义：反斜杠、双引号、控制字符
je() {
  printf '%s' "$1" \
    | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g' \
    | tr -d '\000-\037'
}

COMMIT="$(git -C "$REPO" rev-parse HEAD 2>/dev/null || echo '')"
RUSTC="$(rustc --version 2>/dev/null || echo '')"
GENERATED_AT="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"

# ---------------------------------------------------------------------------
# 可选：用 gpg 对清单本身做分离签名
# ---------------------------------------------------------------------------
SIGNED="false"
SIGN_TOOL=""
GNUPGHOME_TMP=""

cleanup() { [[ -n "$GNUPGHOME_TMP" ]] && rm -rf "$GNUPGHOME_TMP"; }
trap cleanup EXIT

if [[ -n "${CPING_SIGNING_KEY:-}" ]]; then
  if ! command -v gpg >/dev/null 2>&1; then
    echo "警告：提供了 CPING_SIGNING_KEY 但找不到 gpg，本次不签名" >&2
  else
    GNUPGHOME_TMP="$(mktemp -d)"
    chmod 700 "$GNUPGHOME_TMP"
    export GNUPGHOME="$GNUPGHOME_TMP"
    # 私钥从环境变量读入，绝不落盘到仓库里。
    # 导入失败必须**显式报错**：静默降级成「未签名」会让发布者以为签过了。
    if printf '%s\n' "$CPING_SIGNING_KEY" | gpg --batch --quiet --import 2>"$GNUPGHOME_TMP/import.log"; then
      SIGNED="true"
      SIGN_TOOL="gpg"
    else
      echo "错误：签名密钥导入 gpg 失败，本次不会生成签名。gpg 输出：" >&2
      cat "$GNUPGHOME_TMP/import.log" >&2
      echo "（清单仍会生成，但 signed 字段为 false —— 不要把它当成已签名发布。）" >&2
    fi
  fi
fi

# ---------------------------------------------------------------------------
# 收集产物：只认本次发布目录下的顶层文件，把清单自身排除在外
# ---------------------------------------------------------------------------
mapfile -t FILES < <(find . -maxdepth 1 -type f \
  ! -name 'MANIFEST.json' ! -name 'MANIFEST.json.asc' ! -name 'SHA256SUMS.txt' \
  -printf '%f\n' | sort)

if [[ ${#FILES[@]} -eq 0 ]]; then
  echo "产物目录 $DIST 里没有任何可清单化的文件" >&2
  exit 1
fi

{
  echo '{'
  echo '  "format": "CPING-MANIFEST",'
  echo '  "formatVersion": 1,'
  printf '  "version": "%s",\n' "$(je "$VERSION")"
  printf '  "generatedAt": "%s",\n' "$GENERATED_AT"
  printf '  "commit": "%s",\n' "$(je "$COMMIT")"
  printf '  "rustc": "%s",\n' "$(je "$RUSTC")"
  printf '  "signed": %s,\n' "$SIGNED"
  printf '  "signatureTool": "%s",\n' "$(je "$SIGN_TOOL")"
  echo '  "artifacts": ['
  for i in "${!FILES[@]}"; do
    f="${FILES[$i]}"
    size="$(wc -c < "$f" | tr -d ' ')"
    sum="$(sha256sum "$f" | cut -d' ' -f1)"
    sep=','
    [[ $((i + 1)) -eq ${#FILES[@]} ]] && sep=''
    printf '    {"file": "%s", "size": %s, "sha256": "%s"}%s\n' \
      "$(je "$f")" "$size" "$sum" "$sep"
  done
  echo '  ]'
  echo '}'
} > MANIFEST.json

# 清单本身的自检：必须是配平的（本项目核心层的 JSON 只渲染不解析，
# 这里用最简单可靠的方式——能被 python3 读就说明结构没坏；没有 python3 就跳过）
if command -v python3 >/dev/null 2>&1; then
  python3 -c 'import json,sys; json.load(open(sys.argv[1], encoding="utf-8"))' MANIFEST.json \
    || { echo "MANIFEST.json 不是合法 JSON" >&2; exit 1; }
fi

if [[ "$SIGNED" == "true" ]]; then
  gpg --batch --quiet --armor --detach-sign --output MANIFEST.json.asc MANIFEST.json
  echo "已用 gpg 对 MANIFEST.json 签名 → MANIFEST.json.asc"
  echo "校验：gpg --verify MANIFEST.json.asc MANIFEST.json"
else
  echo "MANIFEST.json 已生成（**未签名**：没有提供 CPING_SIGNING_KEY）"
  echo
  echo "注意：sha256 只能证明文件没被改动，不能证明是谁发布的。"
  echo "要补一个可验证的发布者身份，任选其一："
  echo "  1) gpg 分离签名（本脚本内置）："
  echo "       export CPING_SIGNING_KEY=\"\$(cat your-private-key.asc)\""
  echo "       scripts/artifact-manifest.sh $VERSION $DIST"
  echo "  2) sigstore/cosign（无自建密钥，走透明日志）："
  echo "       cosign sign-blob --yes MANIFEST.json > MANIFEST.json.sig"
  echo "       cosign verify-blob --signature MANIFEST.json.sig MANIFEST.json \\"
  echo "         --certificate-identity-regexp 'github.com/.*' \\"
  echo "         --certificate-oidc-issuer https://token.actions.githubusercontent.com"
  echo "  平台原生签名（Windows exe / Android APK / Linux deb）见 docs/signing.md"
fi

echo
echo "清单内容："
cat MANIFEST.json
