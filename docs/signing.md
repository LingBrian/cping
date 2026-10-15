# 制品签名与校验（M17 / v2.5.0）

本文说明 CPing 各平台产物的**签名现状**、**如何校验**、以及**如何补上正式签名**。

先说最要紧的一句：**本项目不伪造签名。**
没有密钥就签不出签名，这不是能糊弄过去的步骤。所以：

- 未提供密钥时，`MANIFEST.json` 里写的是 `"signed": false`，脚本会**明确打印
  「未签名」**并给出补签命令；
- **绝不**用自签证书冒充正式签名——自签证书在用户机器上照样弹「未知发布者」，
  冒充只会让人误判发布状态；
- **绝不**把 SHA-256 说成签名。SHA-256 只能证明「文件没被改动」，
  不能证明「是谁发布的」。这两件事必须分开讲。

---

## 1. 当前签名现状（如实登记）

| 平台 | 产物 | 当前状态 | 说明 |
| --- | --- | --- | --- |
| Windows | `cping-tui.exe` / `cping-cli.exe` | **未签名**（除非配置了证书 Secrets） | 见 §2 |
| Linux | `cping-*-linux-x64.tar.gz` | 提供 `SHA256SUMS.txt` + `MANIFEST.json` | tar.gz 本身无签名概念，见 §3 |
| Android | `*.apk` | **Android Debug 密钥**签名 | 可侧载试用，**不可上架**；见 §4 |
| 通用 | `MANIFEST.json` | 可选 gpg 分离签名 | 见 §3 |

> 「未签名」不是待办事项，而是一个**需要用户知情的状态**。它写在
> `MANIFEST.json` 的 `signed` 字段里，也写在本文里，不做模糊表述。

---

## 2. Windows：Authenticode 代码签名

脚本：`scripts/sign-windows.ps1`

```powershell
# 有证书：真签名 + 校验
$env:CPING_SIGN_CERT_PFX_BASE64 = [Convert]::ToBase64String([IO.File]::ReadAllBytes('cert.pfx'))
$env:CPING_SIGN_CERT_PASSWORD   = '...'
pwsh -File scripts/sign-windows.ps1 -Path target/release

# 无证书：只打印 SHA-256，并说明本次未签名
pwsh -File scripts/sign-windows.ps1 -Path target/release
```

需要安装 Windows SDK 的 **Signing Tools**（提供 `signtool.exe`）。

签名参数与理由：

| 参数 | 理由 |
| --- | --- |
| `/fd SHA256` | 摘要算法用 SHA-256；SHA-1 已被主流信任链淘汰 |
| `/tr <时间戳服务器> /td SHA256` | **打时间戳**。否则证书一过期，**已经发布出去**的签名也会立刻失效 |
| `/pa` | 走默认的验证策略（而不是只挑一条证书链），避免换证书后校验行为漂移 |

签名后脚本会对每个文件跑 `Get-AuthenticodeSignature` 并断言 `Status -eq Valid`
——**签了但校验不过**和没签一样，必须当场失败。

CI 接入：`release.yml` 的 `windows` 作业，Secrets 为
`CPING_SIGN_CERT_PFX_BASE64` / `CPING_SIGN_CERT_PASSWORD`。
PFX 只在 `RUNNER_TEMP` 落盘，`finally` 里删除，绝不进产物。

---

## 3. 通用：`SHA256SUMS.txt` 与 `MANIFEST.json`

### 3.1 校验压缩包完整性

```bash
cd <解压后的发布目录>
sha256sum -c SHA256SUMS.txt
```

能过说明**文件与你拿到的一致**。它**不**说明文件来自谁。

### 3.2 `MANIFEST.json`

`scripts/artifact-manifest.sh <version> <dist-dir>` 生成，形如：

```json
{
  "format": "CPING-MANIFEST",
  "formatVersion": 1,
  "version": "2.5.0",
  "generatedAt": "2026-10-15T06:24:04Z",
  "commit": "bd5334ee02b8f3a8ef83262fef0ada160f8e5825",
  "rustc": "rustc 1.97.1 (8bab26f4f 2026-07-14)",
  "signed": false,
  "signatureTool": "",
  "artifacts": [
    { "file": "cping-cli-2.5.0-linux-x64.tar.gz", "size": 1234567, "sha256": "..." }
  ]
}
```

`commit` 与 `rustc` 是**构建来源**：拿到产物后可以核对自己编译的结果是否同源。
`formatVersion` 用于以后换格式时能拒绝读旧文件（与项目其它落盘格式一致的做法）。

### 3.3 补上发布者身份（二选一）

**方式 A：gpg 分离签名**（脚本内置）

```bash
export CPING_SIGNING_KEY="$(cat your-private-key.asc)"
scripts/artifact-manifest.sh 2.5.0 release/2.5.0
# → MANIFEST.json.asc

# 用户侧校验
gpg --verify MANIFEST.json.asc MANIFEST.json
```

密钥从环境变量读入，导入到临时 `GNUPGHOME`，**不落盘到仓库**。
导入失败会显式报错并把 `signed` 留在 `false`——不静默降级成「看起来签过了」。

**方式 B：sigstore / cosign**（无自建密钥，走透明日志）

```bash
cosign sign-blob --yes MANIFEST.json > MANIFEST.json.sig

cosign verify-blob --signature MANIFEST.json.sig MANIFEST.json \
  --certificate-identity-regexp 'github.com/.*' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
```

CI 接入：`release.yml` 的 `publish` 作业读取 `CPING_SIGNING_KEY`。

---

## 4. Android：release APK 正式签名

脚本：`scripts/sign-android.sh <apk 或目录>`

```bash
export CPING_ANDROID_KEYSTORE_BASE64="$(base64 -w0 your.keystore)"
export CPING_ANDROID_KEYSTORE_PASSWORD=...
export CPING_ANDROID_KEY_ALIAS=...
export CPING_ANDROID_KEY_PASSWORD=...
scripts/sign-android.sh release/android
```

脚本用 `apksigner sign` 同时开启 **v1 / v2 / v3** 方案：

- **v1（JAR signing）**：Android 6 及更早只认它，关掉会让老设备装不上；
- **v2 / v3**：整包签名，防篡改能力更强，且是新设备的默认校验路径。

签完立即 `apksigner verify --verbose --print-certs` 并打印证书指纹——
「签了但校验不过」必须当场失败。

**没有 keystore 时**，脚本只做校验并明确报告：当前是 Android Debug 密钥
（`androiddebugkey` / `android`，口令**是公开的**）签的包，可侧载试用、
**不可上架**。把 debug 签名当正式版发出去，等于让「这个包是我们发的」
这条保证彻底作废。

### keystore 的保管

**keystore 一旦丢失，同包名就再也发不出更新**（Android 不接受换密钥）。
所以：keystore 与口令除了离线备份，另存一份到受控的密钥管理服务；
CI 里只放 base64 与口令的 Secrets。

---

## 5. Linux：deb 包签名说明

本仓库当前**不产出 deb**（Linux 产物是 `tar.gz`），因此这里只给出做 deb 时的
签名流程，供后续打包时照做。

```bash
# 1) 生成（或使用已有的）GPG 签名密钥
gpg --full-generate-key

# 2) 让 dpkg-buildpackage 用该密钥签名
export DEBSIGN_KEYID=<你的 key id>
dpkg-buildpackage -us -uc -b      # -us/-uc = 先不签源码包与 .changes
debsign -k "$DEBSIGN_KEYID" ../cping_*_amd64.changes

# 3) 用户侧校验
dpkg-sig --verify cping_*.deb
gpg --verify cping_*_amd64.changes
```

要点：

- **`.changes` 也要签**，只签 `.deb` 不足以证明「这一批文件是一起发布的」；
- 发布源（apt 仓库）需要 `Release` / `InRelease` 文件签名，否则 `apt` 会拒绝
  该源（或降级为警告，取决于策略）；
- 仓库公钥要随使用说明一起给出，并写清 `apt-key` 已废弃、应放到
  `/etc/apt/keyrings/` 并由 `signed-by=` 引用。

---

## 6. 校验清单（给最终用户）

```bash
# 1) 校验压缩包
sha256sum -c SHA256SUMS.txt

# 2) 校验清单（若发布了 .asc）
gpg --verify MANIFEST.json.asc MANIFEST.json

# 3) 核对清单里的 sha256 与目录内文件一致
python3 - <<'PY'
import json, hashlib, pathlib
m = json.load(open('MANIFEST.json'))
for a in m['artifacts']:
    p = pathlib.Path(a['file'])
    if not p.exists():
        print('缺失', p); continue
    h = hashlib.sha256(p.read_bytes()).hexdigest()
    print('OK  ' if h == a['sha256'] else '不符', p)
PY
```
