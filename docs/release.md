# 构建产物与发布说明（M6）

> 本文件记录**实际产出过的**产物、构建命令与已知限制。
> 「编译成功」不等于完成——每个产物都标注了它的验证程度。

## 1. 产物清单（v1.0.0 时期；**v2.0.0 见 §8**）

| 目标 | 产物 | 构建命令 | 验证程度 |
| --- | --- | --- | --- |
| Windows GUI | `apps/cping-gui/src-tauri/target/release/cping-gui.exe`（11.1 MB） | 在 `src-tauri` 内 `cargo build --release` | ✅ 真实运行 + 截图 + 端到端探测（255/255） |
| Windows TUI | `target/release/cping-tui.exe`（0.66 MB） | `cargo build --release -p cping-tui` | ✅ 真实控制台运行 + 截图 + 真实 ICMP（255/255） |
| Android debug APK | `release/android/app-arm64-v8a-debug.apk`（142 MB，debug 符号未裁剪） | `npx tauri android build --apk --debug --target aarch64` | ✅ **构建成功 + 已用 debug 证书签名 + aapt2 校验通过**；⚠️ 未真机安装（本机无设备） |
| Android release APK | `release/android/app-arm64-v8a-release-unsigned.apk`（12.7 MB） | `npx tauri android build --apk --target aarch64` | ✅ **构建成功 + aapt2 校验通过**；⚠️ **未签名**，需 keystore 才能安装 |
| 共享核心 | `crates/cping-core`（库） | `cargo test --workspace` | ✅ **84 个测试通过**，0 失败（含 12 个 ICMP 报文构造测试） |
| Linux GUI / TUI | —— | 需要 Linux 环境 | ❌ 本机未构建（见 §4） |

原始构建输出（未归集）：

```
apps/cping-gui/src-tauri/gen/android/app/build/outputs/apk/universal/
├── debug/app-universal-debug.apk
└── release/app-universal-release-unsigned.apk
```

一键构建：`scripts\build-all.cmd [all|tui|gui|android|shortcuts]`

## 2. Android 工具链（全部装在 `D:\Program Files\Android`）

| 组件 | 版本 / 路径 |
| --- | --- |
| JDK | 17.0.13（Microsoft OpenJDK）→ `jdk-17` |
| cmdline-tools | `cmdline-tools\latest`（sdkmanager / avdmanager） |
| platform-tools | adb 37.0.1 |
| platforms | `android-34`、`android-35`、`android-36` |
| build-tools | `34.0.0`、`36.0.0` |
| NDK | `26.1.10909125`（r26b） |
| licences | `licenses\android-sdk-license` 等已写入 |

环境变量（用户级，已 `setx`）：`ANDROID_HOME`、`NDK_HOME`。
**`JAVA_HOME` 有意不改**：系统默认是 JDK 26（Ghidra 需要 21+），
Android 构建改由 `gen/android/gradle.properties` 的 `org.gradle.java.home` 指定 JDK 17。

Rust 目标：`aarch64-linux-android`、`armv7-linux-androideabi`、
`i686-linux-android`、`x86_64-linux-android` 均已 `rustup target add`。

### 2.1 国内镜像配置（否则构建极慢或失败）

| 位置 | 配置 |
| --- | --- |
| `gen/android/gradle/wrapper/gradle-wrapper.properties` | `distributionUrl` → 腾讯镜像 |
| `gen/android/build.gradle.kts` | 阿里云 maven（google / public / gradle-plugin）优先 |
| npm | `--registry=https://registry.npmmirror.com`（华为云镜像单包要 ~18s） |
| cargo | `.cargo/config.toml` 的 `[net] offline = true` + 需要时 `CARGO_NET_OFFLINE=false cargo fetch` |

### 2.2 构建参数（已验证可用）

| 位置 | 值 | 原因 |
| --- | --- | --- |
| `app/build.gradle.kts` | `compileSdk = 36` / `targetSdk = 36` / `minSdk = 24` | 本机装的是 android-36；原模板的 37 本机不存在 |
| `gradle.properties` | `org.gradle.configuration-cache=false` | 需禁用配置缓存，否则 Rust 构建任务不执行 |
| `gradle.properties` | `kotlin.incremental=false` | tauri-plugin 的 Kotlin 源码在 C 盘（cargo registry），工程在 D 盘，增量缓存 `relativeTo()` 跨盘符抛 `IllegalArgumentException` |
| `buildSrc/.../BuildTask.kt` | 经 `cmd.exe` 调用 npm | 直接 `exec npm.cmd` 因 PATHEXT 解析失败 |

## 3. 桌面快捷方式

`scripts/create-desktop-shortcuts.py` 用 **pylnk3 纯 Python** 生成 `.lnk`
（本机安全策略会拦截 COM 实例化，故不能用 `WScript.Shell`）。

已创建 4 个：`CPing 桌面版`、`CPing 终端版`、`Android SDK Manager`、`Android adb`。

## 4. 已知限制（未完成的部分，如实记录）

1. **Android 真机未验证**：`adb devices` 为空，本机没有连接设备，
   也没有创建 AVD（需额外下载 system-image 且依赖虚拟化）。
   因此 Android 侧的「安装、启动、触控、停止、前后台切换、文件保存、网络变化」
   **全部未验证**。→ 安装命令已备好：`adb install -r release/android/app-arm64-v8a-debug.apk`
2. **Android 的 ICMP 能力未验证**：`PosixIcmpBackend` 已实现且按
   `EPERM/EACCES → PermissionDenied` 如实上报，但没有真机确认
   Android 普通应用是否真的拿不到 `CAP_NET_RAW`。
   若确认拿不到，按既定策略改用**独立的 TCP 连通性模式**（`TcpConnectBackend`，名字与统计口径都独立）。
3. **release APK 未签名**：`app-arm64-v8a-release-unsigned.apk` 无法直接安装。
   发布前需生成 keystore 并在 `app/build.gradle.kts` 配 `signingConfig`，
   或改产 AAB。**要立刻试用请用 debug APK**（已用 debug 证书签名）。
4. ~~**Linux 未构建**~~ → **v2.0.0 已补齐**：在 WSL2 的 Debian 13 内构建并实测
   原生 ICMP、TCP（v4/v6）、TUI 渲染与退出、四种导出格式。详见 §8。
5. **Windows 安装包未生成**：`tauri.conf.json` 的 `bundle.active = false`，
   需要 `npm run tauri:build` 配合 WiX/NSIS 才能产出 MSI/EXE 安装包。
6. ~~**仅产出 arm64-v8a**~~ → **v2.0.0 起默认构建全部已安装的 ABI**；
   只想要某一个 ABI 时用 `--target aarch64` 显式指定。

## 5. 发布前检查清单

- [ ] 替换 `src-tauri/icons/` 的占位图标为正式设计稿
- [ ] 在真实 Android 设备上完成 `docs/platform-compatibility.md` §5 的验收项
- [x] ~~在 Linux 上构建并实测 `linux-icmp`（注意 `CAP_NET_RAW`）~~ →
      **v2.0.0 已完成**（WSL Debian 13；原生 ICMP / TCP / TUI 全部实测，见 §8）
- [ ] 生成 Android keystore 并配置 release 签名（当前 release APK 未签名）
- [ ] 复核 `docs/platform-compatibility.md` 的平台结论
- [ ] 确认默认并发（32）与超时（1000ms）在真实网络下的表现

## 6. v1.1.0 产物（M7，2026-10-14）

| 产物 | 体积 | 构建命令 | 验证程度 |
| --- | --- | --- | --- |
| `cping-tui-v1.1.0-windows-x64.exe` | 2.4 MB | `cargo build --release -p cping-tui` | ✅ `--help` 冒烟通过 |
| `cping-gui-v1.1.0-windows-x64.exe` | 12.4 MB | 在 `src-tauri` 内 `cargo build --release` | ✅ 启动创建主窗口（冒烟）+ debug 版真实运行截图与快照恢复验证 |

- Android APK 本版未重新构建：M7 的邻居表 / 快照链路在 Android 已实现并单测，
  尚未真机复测；v1.0.0 的 APK 不含 M7 功能
- 发布说明：`docs/release-notes-v1.1.0.md`；SHA256 见 Release 附件

## 7. v1.1.1 / v1.1.2 补丁（GUI 布局与滚动，2026-10-14）

| 版本 | 修了什么 | 产物 | 验证 |
| --- | --- | --- | --- |
| v1.1.1 | 默认窗口 1240×840、表格 `table-layout: fixed`、启动按显示器收敛尺寸 | TUI + GUI exe + SHA256 | PrintWindow 抓窗口自检（侧栏/矩阵/表格三项） |
| v1.1.2 | 「表单显示」滚轮失效：补齐 naive-ui Tabs 断掉的高度链 | TUI + GUI exe + SHA256 | 255 行快照 + 注入滚轮消息：修复前 0 像素变化，修复后正常滚动 |

- 两版都未改核心逻辑；Android APK 未重新构建
- 发布说明：`docs/release-notes-v1.1.1.md`、`docs/release-notes-v1.1.2.md`

## 8. v2.0.0 产物（M8–M12，2026-10-15）

四平台一次性发行，**全部产物在同一台机器上构建**，归集在 `release/v2.0.0/`。

| 产物 | 体积 | 构建方式 | 验证程度 |
| --- | --- | --- | --- |
| `cping-cli-2.0.0-windows-x64.zip` | 0.75 MB | `cargo build --release -p cping-cli -p cping-tui` + `scripts/package.ps1` | ✅ 真实 ICMP、IPv6 TCP、`--list-backends`、退出码实测 |
| `cping-tui-2.0.0-windows-x64.zip` | 0.95 MB | 同上 | ✅ `--help` 冒烟；核心逻辑与 CLI 是同一份 |
| `cping-gui-2.0.0-windows-x64.exe` | 12.6 MB | `npm run build` + `src-tauri` 内 `cargo build --release` | ✅ 启动创建主窗口（冒烟） |
| `cping-cli-2.0.0-windows-x86.zip` | 0.74 MB | 同上 + `--target i686-pc-windows-msvc` | ✅ 在 x64 系统上可运行 |
| `cping-tui-2.0.0-windows-x86.zip` | 0.93 MB | 同上 | ✅ 同上 |
| `cping-cli-2.0.0-linux-x64.tar.gz` | 0.84 MB | WSL Debian 内 `cargo build --release` + `scripts/package.sh` | ✅ ICMP / TCP v4+v6 / 关闭端口 / 四种导出实测 |
| `cping-tui-2.0.0-linux-x64.tar.gz` | 1.07 MB | 同上 | ✅ 伪终端内渲染并进入备用屏，按 `q` 优雅退出（码 0） |
| `cping-2.0.0-android-universal-release-unsigned.apk` | 50.9 MB | `npx tauri android build --apk` | ⚠️ 构建与 aapt2 校验通过；**未签名**，不能直接安装 |
| `cping-2.0.0-android-universal-release-debugkey-signed.apk` | 50.9 MB | 上面的包 + `apksigner`（debug 密钥） | ✅ 已签名可安装；**证书是 Android Debug**，仅供侧载试用，不可上架；真机未验证 |
| `SHA256SUMS.txt` | — | 汇总上述全部产物 | 可 `sha256sum -c` 直接校验 |

> Android 的 APK 是 **universal 单包全 ABI**：`lib/arm64-v8a`、`lib/armeabi-v7a`、
> `lib/x86`、`lib/x86_64` 四个 `.so` 都在同一个包里，用户不需要判断该装哪个。
> 包名 `com.cping.gui`，`versionName 2.0.0` / `versionCode 2000000`，
> `targetSdk 36` / `minSdk 24`（用 aapt2 校验确认）。

### 8.1 校验

```bash
# Linux / macOS / WSL
cd release/v2.0.0 && sha256sum -c SHA256SUMS.txt
```

```powershell
# Windows
Get-ChildItem release\v2.0.0 -File -Exclude SHA256SUMS.txt |
  ForEach-Object { "$((Get-FileHash $_ -Algorithm SHA256).Hash.ToLower()) *$($_.Name)" }
```

### 8.2 Linux 构建链路（WSL 内无外网）

WSL 里访问不到 crates.io，因此先在 Windows 侧落一份依赖快照，再同步进 WSL：

```bash
cargo vendor vendor-offline        # Windows 侧执行
```

WSL 内的 `.cargo/config.toml` 指向它（**只加在 WSL 本地，绝不提交进仓库**。
把 `[net] offline = true` 或这段 `replace-with` 落盘到仓库会让 CI 与任何全新克隆
全部构建失败——详见 §8.4 第 1 条）：

```toml
[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "vendor-offline"
```

`vendor-offline/` 已在 `.gitignore` 中（体积大，且用同一条命令随时可重建）。

### 8.3 Android 构建与签名

```bash
cd apps/cping-gui
npx tauri android build --apk           # 默认构建全部已安装的 ABI，产出 universal 包
npx tauri android build --apk --debug   # debug 变体（debug 证书已签名，可直接安装）
```

给未签名的 release 包补签名：

```bash
apksigner sign --ks ~/.android/debug.keystore \
  --ks-pass pass:android --key-pass pass:android \
  --ks-key-alias androiddebugkey \
  --out <输出>.apk <输入>-unsigned.apk
```

### 8.4 发布渠道与 CI 状态

- **GitHub Release 已发布**（非草稿、非预览）：
  <https://github.com/LingBrian/cping/releases/tag/v2.0.0>
  11 个附件 = 上表 10 个产物 + `SHA256SUMS.txt`，字节数与本地产物逐一致。
- `.github/workflows/release.yml` 支持两种进入方式：
  - 打 tag（`v*`）自动触发；
  - **手动触发（`workflow_dispatch`）**，可传 `tag` 输入用于补产物或预演。
- `publish` job 有**幂等保护**：若该 tag 下已有 Release（例如人工用本机实测过的产物
  发布过），则跳过自动创建，避免 `draft: true` 把一个已公开的 Release 打回草稿。

#### 发行时踩到的两个「只在别人机器上复现」的缺陷

本地四平台产物都出得来、测试全绿，但 `main` 与 `v2.0.0` 的 workflow 全红。
两条根因都不是逻辑错误，而是**环境耦合**（完整记录见 `docs/testing.md` 文末）：

1. **`.cargo/config.toml` 里硬编码了 `[net] offline = true`** → 所有 Rust 任务报
   `no matching package named tokio / indexmap found`。cargo 解析依赖图与校验
   `Cargo.lock` 时**必须访问 registry 索引，即使可选 feature 未启用**；本地一直
   「看着正常」只是因为依赖缓存是热的。**不要把这个开关落盘到仓库。**
2. **OUI 数据表 CRLF** → Windows 全新克隆下厂商名带尾随 `\r`，5 条测试挂掉。
   修法：`find()` 出口剥掉 `\r` + `.gitattributes` 把
   `cping-core/src/data/*.txt` 钉死为 LF。

修完后 CI 五个任务（core / gui frontend / gui shell / windows / android）**首次全绿**，
测试数 276。发布前请确认这条：

```bash
gh run list --workflow=ci.yml --limit 1    # 期望 completed / success
```

> 上面 10 个产物是在**修复前**的提交上构建的。两处修复都不改变编译进二进制的
> 内容（一处是构建配置、一处是数据文件的换行符，而本机构建时该文件本来就是 LF），
> 因此产物有效、无需重建；`SHA256SUMS.txt` 仍与线上附件逐字节对应。

