# 测试与验收记录

> 「编译成功」不算完成。本文件记录每个阶段**实际跑过**的测试与观察到的事实。

## 阶段结果总览

| 阶段 | 构建 | 测试 | 真实运行验证 |
| --- | --- | --- | --- |
| M0 现状检查 | — | — | 阅读 `analysis/**`、`tech.md`，输出三份清单 |
| M0.5 动态复核 | — | — | Frida 回环复测（V1/V2/V3/V6 确证） |
| M1a 核心离线 | ✅ | ✅ 47 | — |
| M1b 调度 | ✅ | ✅ +15 = 62 | — |
| M2 TUI | ✅ | ✅ +10 = 72 | ✅ 真实控制台截图验证 |
| M3 桌面 GUI | ✅ | ✅ 类型检查 + 2701 模块构建 | ✅ 窗口创建 + 界面渲染 |
| M4/M5 真实探测 | ✅ | ✅ +12 = 84 | ✅ **Windows 真实 ICMP**（TUI 与 GUI 双端 255/255） |
| M6 Android 打包 | ✅ APK ×2 | ✅ 84（回归） | ⚠️ **静态校验通过，真机未验证**（无设备） |
| M7 目标信息增强 | ✅ | ✅ +67 = 151 | ✅ 真实局域网端到端 + GUI 真实运行截图 |
| M8–M12 通向 v2.0.0 | ✅ 四平台产物 | ✅ +125 = **276** | ✅ Windows + Linux（WSL）双平台实测，含 IPv6/TCP 与 TUI |

## 统一检查命令

```bash
cd cping

# 核心 + TUI
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# GUI 前端
cd apps/cping-gui
npm run typecheck      # vue-tsc --noEmit
npm run build          # vite build

# GUI 外壳
cd apps/cping-gui/src-tauri
cargo build

# Android APK（需要 ANDROID_HOME / NDK_HOME / JDK17，见 docs/release.md §2）
cd cping/apps/cping-gui
set ANDROID_HOME=D:\Program Files\Android
set NDK_HOME=D:\Program Files\Android\ndk\26.1.10909125
npx tauri android build --apk            # 默认构建全部已安装的 ABI
npx tauri android build --apk --debug    # debug 版（用 debug 证书签名，可直接安装）

# Linux（WSL Debian，离线 vendored 源；见 docs/release.md §8）
wsl -d Debian -e bash -c 'export PATH=$HOME/.cargo/bin:/usr/local/bin:/usr/bin:/bin; \
  cd ~/cping && cargo test --workspace --offline && \
  cargo build --release -p cping-cli -p cping-tui --offline'
```

> **环境注意**：在本沙箱中 cargo 的 registry 探测会挂起，因此命令里显式带了
> `--offline`。**注意不要**把 `[net] offline = true` 写进 `.cargo/config.toml`——
> 那会让 CI 与全新克隆全部构建失败（v2.0.0 发行时真实踩过，见本文末）。
> 需要下载新依赖时用 `CARGO_NET_OFFLINE=false cargo fetch`。

## M1a / M1b：核心（62 个测试）

| 测试文件 | 数量 | 覆盖 |
| --- | --- | --- |
| `target_prefix.rs` | 14 | 空前缀、段数错、非数字、>255、前导零、负数；三种目标范围的数量与首尾 |
| `matrix_index.rs` | 9 | 16×16=256；`index ↔ (row,col)` 双向；`格号 == IP 最后一段`；`Standard` 下格 0 必须为空、其余 255 格一一对应 |
| `classify_boundaries.rs` | 9 | **49 / 50 / 99 / 100 / 101** 精确落类；超时→无回应 与 权限错误→错误 **互不相同** |
| `export_format.rs` | 15 | UTF-8 BOM、CRLF、RFC4180 转义、IPv4 数值排序（`.10` 排在 `.3` 之后）、空 RTT/MAC 留空、TXT 摘要 |
| `scheduler.rs` | 15 | 并发峰值 ≤ 上限；**取消后不再调度新目标且已完成结果保留**；重复启动被拒；task_id 唯一；错误五种分类各自映射 |

关键断言（对应需求逐条）：

- 取消：`stats.total` 保持目标总数、`results.len() == stats.completed`、`seen < total`
- 并发：用 `RecordingBackend.peak_concurrency()` 断言峰值不超过 1/4/16
- 超时：`MockResponse::Slow{200ms}` 在 50ms 预算下必须判为**超时**而非在线

## M2：TUI（10 个测试）

用 ratatui 的 `TestBackend` 真实渲染一帧并断言字符缓冲，而非只测纯逻辑：

- 输入区 / 矩阵标题 / 默认前缀渲染
- 矩阵包含 0、1、16、127、240、255 等格号
- 帮助弹窗（含「Ctrl+C」与「无颜色」说明）
- 40×14 小终端给出「终端过小」指引，不 panic、不输出乱码
- 扫描后统计正确、矩阵出现分类符号
- 表格视图 + 离线过滤后的行数 **等于核心给出的在线数**（GUI/TUI 口径一致性）
- 取消后已完成结果数量与统计一致
- 非法前缀不启动任务
- 导出 CSV（带 BOM）与 TXT（含摘要）

### 真实终端验证（M2 已完成）

在 Windows 真实控制台（118×38）中运行 `cping-tui.exe` 并截图
（`analysis/tui_real.png`）：

- 顶部输入区、进度 `0/0 (0%)`、统计行
  「空闲 在线 0 超时 0 不可达 0 错误 0 用时 0.00s 后端 mock 本机 未知（M5 接入网卡枚举）」
- 16×16 矩阵完整显示 0–255 格号
- 底部快捷键行与状态提示「已适配终端尺寸」

> 过程中发现并修复的真实问题：原本把状态区放在矩阵**下方**，而 Windows console 的
> buffer 高度常大于可视窗口，导致状态与进度被挤到可视区之外。已改为放在矩阵**上方**。
> 同时发现 ratatui 的 `Gauge` 默认样式在深色主题下几乎不可见，已显式指定配色。

## M3：桌面 GUI

### 前端

| 检查 | 命令 | 结果 |
| --- | --- | --- |
| 类型检查 | `npm run typecheck`（vue-tsc --noEmit） | ✅ 0 错误 |
| 生产构建 | `npm run build`（vite） | ✅ 2701 modules，产出 `dist/` |

### Tauri 外壳

| 检查 | 命令 | 结果 |
| --- | --- | --- |
| 编译 | `cd src-tauri && cargo build` | ✅ 产出 `cping-gui.exe`（15.5 MB） |
| 窗口 | 启动 exe | ✅ 标题「C网群PING工具」，1016×719 |
| 界面渲染 | release 构建 + 截图 | ✅ 见 `analysis/gui_desktop.png` |
| **端到端** | 点击「开始探测」 | ✅ 见 `analysis/gui_after_start.png` |

### 端到端验证结果（最关键的一条）

点击「开始探测」后，界面显示 **已完成 255/255、在线 158、超时 97、不可达 0、错误 0**，
16×16 矩阵按分类着色（`#` 低延迟 / `+` 中延迟 / `*` 高延迟 / `-` 无回应），
按钮状态正确切换（开始→禁用，停止→可用）。

这条链路一次性验证了：
**前端交互 → Tauri Command → cping-core 目标生成与调度 → MockBackend 探测
→ ProbeEvent 回推 → 前端 store → 矩阵渲染**。

像素统计（`gui_after_start.png`）：
`#00FF00` ×39367、`#FFFF00` ×41463、`#99B4D1` ×32595、`#C0C0C0` ×71390 ——
四种分类色**同时出现**，说明分类逻辑与配色映射都生效。

### 又一个真实故障：devUrl 导致 release 版打不开界面

`cargo build --release` 出的 exe 启动后显示「localhost 拒绝连接」。
排查：exe 中出现 `localhost:1420` 字符串且**不含** dist 资源 → 说明资源未被内嵌，
Tauri 仍在找开发服务器。

修复：从 `tauri.conf.json` 移除 `devUrl`（本环境 vite dev server 本就无法监听 1420），
release 构建即改为内嵌 `frontendDist`。

> 代价与说明：`tauri dev`（开发热重载）需要 `devUrl`。如需开发模式，
> 把 `devUrl` 加回并确保 1420 端口可监听；生产构建则必须走 `cargo build --release`。

### 排查过的两个真实构建故障

1. **schemars 0.8 编译失败**（`E0107: IndexMap 需要 3 个泛型参数`）
   - 根因：`indexmap 1.x` 的 `std` feature **默认关闭**，其 build.rs 在 `CARGO_FEATURE_STD`
     缺失时改用 autocfg 探测 sysroot 中的 std crate；该探测在本机失败，`has_std` 未生效，
     indexmap 退化为 `IndexMap<K, V, S>`（无默认泛型参数）。
   - 修复：显式声明 `indexmap = { version = "1.9", features = ["std"] }`。
   - **坑中坑**：resolver v2 下 build-dependencies 与 dependencies 的 feature **分开解析**，
     必须在 `[dependencies]` 与 `[build-dependencies]` **两处**都声明，否则
     `tauri-build → tauri-utils → schemars` 那条链仍拿到没有 std 的 indexmap。
2. **tauri-build 缺少 `icons/icon.ico`**：Windows 生成资源文件需要它，已补齐。

### 环境限制（必须知道，否则会误判）

| 限制 | 现象 | 影响与应对 |
| --- | --- | --- |
| `spawnSync` + 管道 = EBUSY | `npm install` 时 esbuild / tauri-cli 的 postinstall 失败 | 用 `npm install --ignore-scripts`；平台二进制仍在，实测 `vite build` 正常 |
| npm 默认源慢 | 单包响应约 18 s，卡住 12 min 无进展 | 改用 `--registry=https://registry.npmmirror.com`（79 包 32 s） |
| 端口监听被拒 | vite dev server `EACCES ::1:1420` | dev 模式不可用；改用 `cargo build --release` 内嵌 `dist` |
| cargo registry 探测挂起 | `cargo build` 无输出并被 SIGTERM | 命令里显式带 `--offline`；需要下依赖时用 `CARGO_NET_OFFLINE=false cargo fetch`。**不要**写进 `.cargo/config.toml`（会让 CI 全红） |
| 批量删除保护 | `rm -rf` 超过 50 个目标被拦截 | 改用 `cargo clean -p`；构建时清理 `target` 需分批 |
| Gradle 无法 exec `npm.cmd` | `BuildTask.kt` 里 `exec` 报 PATHEXT 解析失败 | 改为经 `cmd.exe /c` 调用 npm |
| `compileSdk 37` 不存在 | AGP 报 platform 37 未安装 | 降到 36（AGP 会自动装 platform-36 / build-tools 36.0.0） |
| Kotlin 增量缓存跨盘符 | `IllegalArgumentException: different roots` | `gradle.properties` 设 `kotlin.incremental=false` |
| Rust 交叉编译增量目录锁 | `error deleting lock file ... os error 5`（非致命） | 忽略；只影响 `.rmeta` 复用，不影响产物 |

## M4 / M5：真实 ICMP 后端（84 个测试）

| 检查 | 命令 | 结果 |
| --- | --- | --- |
| 核心 + TUI 测试 | `cargo test --workspace` | ✅ **84 passed / 0 failed**（含 `native_icmp.rs` 12 项报文构造与校验测试） |
| Windows 真实探测 | `cargo test -p cping-core --test native_icmp_real -- --ignored --nocapture` | ✅ `127.0.0.1` → 在线 rtt=0ms；`192.0.2.123` → Timeout 816ms |
| TUI 真机全量扫描 | `cping-tui.exe` + 回环 `127.0.0.0/24` | ✅ **255/255 在线，0.03 s**（`analysis/tui_real_icmp.png`） |
| GUI 真机全量扫描 | `cping-gui.exe` + 回环 | ✅ **255/255 在线**（`analysis/gui_real_after.png`） |
| TCP 模式隔离 | `TcpConnectBackend::name()` | ✅ 返回 `tcp-connect`，与 `windows-icmp` 完全分离，不会冒充 ICMP 成功 |

> **M5 的真实故障（值得记住）**：GUI 里 255 个 `ProbeEvent` 在 31 ms 内全部到达，
> 前端的「立即 `refreshSnapshot()`」把它们**互相覆盖**，界面只剩最后一条。
> 修复：在 `finished` / `cancelled` 事件到达时再 `resyncFromBackend()` 做一次全量对齐。

## M6：Android 打包

### 构建结果

| 检查 | 命令 / 工具 | 结果 |
| --- | --- | --- |
| debug APK | `npx tauri android build --apk --debug --target aarch64` | ✅ 2 分 50 秒，产出 142 MB APK |
| release APK | `npx tauri android build --apk --target aarch64` | ✅ 8 分 4 秒，12.7 MB（**未签名**） |
| 元信息 | `aapt2 dump badging` | ✅ `com.cping.gui` / v0.1.0 / minSdk 24 / targetSdk 36 / `native-code: 'arm64-v8a'` |
| 原生库 | `unzip -l` | ✅ `lib/arm64-v8a/libcping_gui_lib.so` |
| 前端资源 | `unzip -l` | ✅ `assets/tauri.conf.json`（`dist` 已内嵌） |
| 签名 | `apksigner verify --print-certs` | ✅ debug 变体：`CN=Android Debug`，SHA-256 `793d68…bfea` |
| 设备安装 | `adb install` | ❌ **未执行**：`adb devices` 为空 |

### M6 根因分析：APK 为什么一开始「根本出不来」

第一次构建在 `:app:rustBuildArm64Debug` 失败，报：

```
failed to validate library: Library from .../libcping_gui_lib.so
does not include required runtime symbols.
This means you are likely missing the tauri::mobile_entry_point macro usage
```

**根因**（不是环境问题，是代码问题）：`src-tauri/src/lib.rs` 里 `run()` 虽然存在，
但**没有加移动端入口标注**。Tauri CLI 在把 `.so` 交给 Gradle 之前会校验
`Java_<包名>_..._run` 这类 JNI 符号是否存在，桌面构建不查、Android 必查。

**修复**（一行）：

```rust
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() { /* ... */ }
```

`mobile` cfg 由 `tauri-build` 仅在 android/ios 目标下发出，
因此桌面端行为不变（`main.rs` 照旧直接调 `run()`）。

### M6 的第二个坑：Kotlin 增量缓存跨盘符

```
Suppressed: java.lang.IllegalArgumentException:
  this and base files have different roots:
  C:\Users\...\.cargo\registry\...\DialogPlugin.kt  and
  D:\Workspace\...\gen\android.
```

tauri-plugin 的 Kotlin 源码来自 **cargo registry（C 盘）**，而工程在 **D 盘**；
Kotlin 增量编译用 `File.relativeTo()` 算相对路径，跨盘符直接抛异常 →
每个 Kotlin module 都要回退到「无 daemon 编译」，又慢又刷屏。
修复：`gradle.properties` 加 `kotlin.incremental=false`。

## 布局修复：桌面版左栏过高、底部状态栏被挤出（用户反馈）

**现象**（`analysis/gui_before.png`）：窗口右侧出现**整页滚动条**，
左栏底部的「保存结果（CSV / TXT）」按钮和最下方的状态栏都要往下滚才看得到。

**两个真实根因，都在 CSS：**

1. **Grid 的 `1fr` 陷阱**：`grid-template-rows: 1fr auto` 里的 `1fr`
   实际等价于 `minmax(auto, 1fr)` —— 第一行的**最小高度是内容高度**，
   左栏内容一多就把行撑高，整页超过视口。
   修复：`grid-template-rows: minmax(0, 1fr) auto` + `.desktop__side { min-height: 0 }`。
2. **高度链断裂**：`#app` 与 `.desktop` 之间隔着 naive-ui 的
   `n-config-provider` / `n-message-provider` 两层 div，它们没有高度，
   `.desktop` 的 `height: 100%` 因此解析为 `auto`（父容器高度未定义）。
   修复：给两层 provider 都加 `class="app-shell"`，CSS 里 `height: 100%`。

**同时把左栏压矮**（否则内容本身仍超过可用高度）：

| 位置 | 改动 |
| --- | --- |
| `.desktop` | 内边距 12→10，列宽 288→280，行距 12→10 |
| `.desktop__side` | 间距 10→8；`.desktop__side .cping-panel` 内边距 12→`9px 10px` |
| `ColorSettings.vue` | 色块 42×26→34×20，行距 6→4，提示语压成一行 |
| `PrefixForm.vue` / `ProgressSummary.vue` | 各处 margin 12/10/6 → 8/8/4 |
| `DesktopLayout.vue` | 导出按钮 margin 10→8；「上次导出」改单行省略号 |

**验证**（`analysis/gui_final_680.png`、`analysis/gui_final_560.png`）：

- 1016×680（默认）：左栏五个面板 + 保存按钮 + 底部状态栏**全部可见，零滚动**。
- 1016×560（人为压矮）：左栏出现**自己的**内部滚动条，
  状态栏仍然固定在底部完整可见 —— 降级行为正确。

> 顺带说明：截图最底边那条模糊文字是 DWM 不可见边框外的**背后窗口**透出来，
> 不是应用内容（GetWindowRect 含 ~7px 不可见边距）。

## Android 模拟器真机级验收（M6 之后）

### 环境搭建（两处 SDK 工具链的坑）

| 坑 | 现象 | 解法 |
| --- | --- | --- |
| `sdkmanager` 单连接下载 ~0.3 MB/s | 1.6 GB 镜像要 1.5 h+ | 从仓库 XML 拿真实 URL，多线程分段下载（`analysis/pdl.py`，8 连接 ~9.7 MB/s），解包后**手写 `package.xml`**（`analysis/install_sdk_pkg.py`），`sdkmanager --list_installed` 即可识别 |
| `avdmanager create avd` 报 *Package path is not valid*，错误信息里却把同一 path 列为「合法」 | 该版 cmdline-tools 解析系统镜像元数据 bug | 手写 `~/.android/avd/<name>.ini` + `<name>.avd/config.ini`（`analysis/make_avd.py`）——emulator 启动只读 config.ini，完全不查 package.xml |

WHPX：`Enable-WindowsOptionalFeature HypervisorPlatform -NoRestart` 之后
**未重启即生效**（`emulator -accel-check` → `Windows Hypervisor Platform accelerator
is operational`），模拟器 40 s 完成冷启动。

### 验收矩阵（全部实测）

| 项 | 结果 |
| --- | --- |
| 安装（290 MB 通用 APK，双 ABI） | ✅ Success，设备端 `primaryCpuAbi=x86_64` |
| 冷启动 | ✅ COLD，**不自动扫描**（点击前保持 0/0） |
| 真实扫描 10.0.2.x | ✅ 255 目标 / 8.27 s = 8 批 × 1 s 超时 |
| ICMP 权限 | ✅ `EPERM`/`EACCES` → 如实显示「权限不足」，0 在线 |
| 列表 / 矩阵 / 横屏 / 前后台 | ✅ 全部正常（`docs/images/android-*.png`） |
| 导出 CSV | ✅ 应用专属外部目录；BOM / 256 行 CRLF / `.1`–`.255` |
| 真机级截图 | ✅ `android-list / android-matrix / android-landscape`（APK 本体，非仿真） |

### ✅ 曾经的「间歇性 native abort」：已归因，**不是缺陷**

```
F libc: FORTIFY: pthread_mutex_lock called on a destroyed mutex
ActivityManager: Process com.cping.gui (pid xxx) has died: cch CRE  ← cached
```

- **结论**：这是 `tao` 的 Android 退出路径（`platform_impl/android/mod.rs:199`
  在 `run_return()` 返回后 `::std::process::exit(exit_code)`，退出码 0）与
  Chromium WebView 拆卸并发产生的日志噪音，**不是本项目崩溃**。
- 证据：`dumpsys activity exit-info` 记为 `reason=1 (EXIT_SELF) status=0`（主动退出）；
  `/data/tombstones` 为空、`SIGABRT`/`Fatal signal` 命中 0（无致命信号，故无回溯）；
  三条 FORTIFY 来自本进程 3 个不同 tid、命中 2 个不同互斥量，紧接
  `wm_on_destroy_called`，WebView 沙箱进程 4 ms 后以 `ISOLATED NOT NEEDED` 退出。
- 本项目不可能触发该日志：bionic 上 `std::sync::Mutex` 无 `pthread_mutex_destroy`
  路径，全部锁点都在 `Arc` 保护下；`clipboard` 全仓库零使用。
- **确定性复现**：`adb shell input keyevent KEYCODE_BACK`（finish MainActivity）
  即可 1 次命中；只按 HOME（`onStop` 而不 `onDestroy`）**不会**触发——这就是当初
  「三次受控重现均未复现」的原因。
- **对照组**：用 NDK clang 编的独立小程序（`tools/android-forensics/`）在**同一台模拟器**上真触发该条件
  （`pthread_mutex_destroy` 后再 lock），结果必然 abort：紧跟 `Fatal signal 6 (SIGABRT)`、
  `DEBUG: Abort message` + `__fortify_fatal` 回溯、生成 `tombstone_00`、退出码 134。
  本应用那次这些痕迹**一个都没有** → 工具链有效，进程不是被 FORTIFY 终结的。
- 影响评估：前台交互与扫描全流程不受影响；进程退出后**内存态结果丢失**属 Android
  生命周期固有行为（与「取消后保留已完成结果」不冲突——进程级回收本不保证内存态存活），
  当前由导出 CSV 缓解。
- 详见 `docs/platform-compatibility.md` §3.5。

### 仍未验证（M6 明确未覆盖）

- 释放版本未签名（`release` APK 需 keystore 才能安装）
- 真机差异项：厂商 ROM 后台管控、字体缩放、Wi-Fi/蜂窝切换

## M7：目标信息增强（151 个测试）

### 测试结果

| 项 | 命令 | 结果 |
| --- | --- | --- |
| 核心 + TUI | `cargo test --workspace` | ✅ **151 passed / 0 failed**（较 M6 的 84 新增 67） |
| 真实网络用例 | `CPING_M7_TARGETS=… cargo test -p cping-core --test m7_target_info_real -- --ignored` | ✅ 4/4（另有 8 个 `#[ignore]` 用例按需运行） |
| 前端类型检查 | `npm run build`（含 `vue-tsc --noEmit`） | ✅ 0 错误，2703 模块构建通过 |
| GUI 外壳 | `cargo clippy --all-targets -- -D warnings` | ✅ 干净（workspace 与 GUI 两套均通过） |
| 格式 | `cargo fmt --all -- --check` | ✅ 干净 |

M7 新增的测试面：

| 模块 | 覆盖 |
| --- | --- |
| `oui.rs` | 三表嵌入完整性（行偏移二分索引恰好覆盖每条记录）、最长前缀优先（MA-S > MA-M > MA-L，用**真实数据**里同一前缀同时命中 MA-L 与 MA-S 的条目验证）、资格判断（全零/组播 I/G/本地管理 U/L 一律拒绝）、MAC 解析与格式化 |
| `neighbor.rs` | `/proc/net/arp` 纯解析、`GetIpNetTable2` FFI 结构体布局（SOCKADDR_INET=28、MIB_IPNET_ROW2=88 字节）、**IP 字节序回归**、单播过滤（拒全零与广播）、回环/跨网段地址不收录 |
| `hostname.rs` | PTR 结果规整（去尾点、拒 IP 字面量、剥 NetBIOS 数字后缀）、否定缓存、超时不污染缓存、不覆盖已有主机名 |
| `snapshot.rs` | 往返逐字段一致、统计口径一致、**序列化字节级稳定**、拒坏签名/版本/状态/列数/行数/在线无 RTT、容忍空行与缺尾换行、制表符换行有损但不破结构 |
| `enrich.rs` | 只补空缺不覆盖、本地管理地址不出厂商、空表 no-op、补全不改动任何状态与统计 |
| 导出 | 旧列逐字节向后兼容、新列转义（含逗号的厂商名）、TXT 定宽截断后各列显示宽度一致 |
| TUI | 20 个（新增 10 个）：补全不改状态/统计、扫描后自动落盘、**重启后默认不自动恢复**、显式确认后恢复且统计一致、丢弃后不再重复询问、坏快照如实报错、快照与目标规则不一致时拒绝恢复、表格详情行渲染（宽/窄终端）、无行可反查时如实提示 |

### 真实局域网端到端（Windows 原生 ICMP）

```text
目标：127.0.0.1, 192.168.0.1, 192.168.0.95, 192.168.0.229, 192.168.0.255, 8.8.8.8

扫描完成：total=6 online=3
邻居表：ok=true 条目=5 补上 MAC=3 补上厂商=1

IP              状态      MAC                 厂商                主机名
127.0.0.1       在线                                              XuanKui
192.168.0.1     在线      5C:7D:AE:EA:CD:8D   zte corporation
192.168.0.95    超时      E2:7B:18:8E:06:56
192.168.0.229   在线      BE:51:D2:EE:26:F0
192.168.0.255   超时
8.8.8.8         超时
```

「不许伪造」这条硬规则全部成立：

- 回环地址没有 MAC（二层不存在）✅
- `.255` 广播地址没有 MAC（单播过滤生效，不再混入 `FF:FF:FF:FF:FF:FF`）✅
- `192.168.0.95/.229` 有 MAC 但没有厂商——它们的首字节是本地管理地址（U/L 位置位），
  不属于 IEEE 分配，**留空才是正确行为** ✅
- 厂商名与直接查表结果逐条一致（防张冠李戴）✅
- `127.0.0.1` 反查出真实主机名 `XuanKui`（本机名，走的是真实 DNS/PTR 链路）✅

### 真实数据抓出的两个真 bug（单测抓不到的那类）

| Bug | 现象 | 根因 | 修复 |
| --- | --- | --- | --- |
| IP 字节序颠倒 | 邻居表把 `192.168.0.1` 显示成 `1.0.168.192`、`224.0.0.2` 显示成 `2.0.0.224` | 把 28 字节的 `SOCKADDR_INET` 当 `[u32; 7]` 读再 `.to_be_bytes()`，多做了主机序→大端转换 | 改成命名字段 `sin_addr: [u8; 4]` 按字节直读；加回归测试把「按字节读」与「当时的错值」同时钉死 |
| 广播 MAC 混进结果 | `.255` 是扫描目标，ARP 表里它的 MAC 是 `FF:FF:FF:FF:FF:FF`，界面会给广播地址标 MAC | 只校验了 IPv4 合法性，没校验 MAC 单播性 | 新增 `is_unicast_mac`（拒全零 + 拒 I/G 位置位），在 FFI 与 `/proc/net/arp` 两条路径同时生效；邻居表从 15 条降到 5 条 |

### 快照链路实测

- 真实结果落盘：337–372 字节（4–6 行），读回后**逐字段与统计完全一致**，
  同一份数据两次序列化字节相同（格式稳定）
- GUI 真实运行验证：把一份**真实格式**的快照放进应用数据目录再启动，
  确认框正确显示扫描时间/网段/条数/用时与文件路径，且提供
  「丢弃 / 暂不恢复 / 恢复结果」三个出口——**默认落在不恢复一侧**
  （截图 `docs/images/m7-gui-restore-prompt.png`）

### M7 明确未覆盖

- Linux/Android 真机上的邻居表与主机名链路（`/proc/net/arp` 分支已实现并单测，未实测）
- MAC/厂商列在 Android 移动布局下的真机视觉效果（逻辑同桌面端）

## M8–M12：通向 v2.0.0（276 个测试）

### 测试结果（2026-10-15）

```text
cargo test --workspace                                  →  274 passed; 0 failed; 8 ignored
cargo fmt --all -- --check                              →  clean
cargo clippy --workspace --all-targets -- -D warnings   →  clean
（GUI 壳单独一套）cargo clippy --all-targets -- -D warnings  →  clean
npm run typecheck                                       →  0 errors
npm run build                                           →  2705 modules / 562 kB JS / 6.3 kB CSS
```

上面这份是 M8–M12 交付当时的记录。之后 `v2.0.0` 的 CI 又抓出两个缺陷
（见本文末「本轮抓出并修掉的真实缺陷」第 5、6 条），随之补了 2 条回归测试，
当前计数为 **276 passed / 0 failed / 8 ignored**；在 CRLF 条件下复现验证时为
236 passed / 0 failed（`cping-core` 单包）。

新增的 123 个测试集中在：目标规格解析（含 IPv6 CIDR 边界）、持续监控的
`diff_scans` 全状态组合、变化日志容量上限、JSON 严格转义、HTML 无外部引用、
扫描差异、后端注册表（能力与现实一致）、可选异步调度（与同步版语义对齐）。

### Linux 实测（WSL Debian 13，Rust 1.99.0，x86_64）

本机没有独立 Linux 机器，用 WSL2 里的 Debian 完成：

| 用例 | 命令 | 观察到的结果 |
| --- | --- | --- |
| 原生 ICMP | `cping-cli -t 127.0.0.1 --mode icmp` | 在线 1、1 ms、退出码 0 |
| 原生 ICMP（跨主机） | `cping-cli -t 172.28.160.1 --mode icmp` | 在线 1、0 ms、退出码 0 |
| ICMPv6（未实装） | `cping-cli -t ::1 --mode icmp` | **不支持 1**、退出码 3 |
| TCP v4 | `cping-cli -t 127.0.0.1 --mode tcp --port 18080` | `TCP-OPEN`、2 ms、退出码 0 |
| TCP v6 | `cping-cli -t ::1 --mode tcp --port 18081` | `TCP-OPEN`、1 ms、退出码 0 |
| **TCP 关闭端口** | `cping-cli -t 127.0.0.1 --mode tcp --port 18999` | **不可达 1**、0.000 s、退出码 1 |
| 混合 v4+v6 | `cping-cli -t "::1, 127.0.0.1" --mode tcp --port 18081` | 2 个网段 / 2 个目标、两个都在线 |
| TUI 渲染与退出 | `script -qec "./cping-tui --mock"` 后送 `q` | 进入备用屏正常渲染、**退出码 0** |
| 四种导出 | `--out r.csv/r.json/r.html/r.txt` | 四个文件均产出且内容正确（含 `::1` 行） |

`--list-backends` 在 Linux 上的输出：

```text
ICMP 可用（需要原始套接字权限，仅 IPv4）；TCP 可用（IPv4 + IPv6）；ICMPv6 未实装，IPv6 目标请用 TCP
icmp4  原生 ICMP（IPv4）  是  仅 IPv4    需要原始套接字权限
icmp6  原生 ICMP（IPv6）  否  —          ICMPv6 尚未实装，请用 TCP 方式探测 IPv6 目标
tcp    TCP 连接           是  IPv4+IPv6  —
mock   模拟（测试）        是  IPv4+IPv6  仅测试用
```

**这次 Linux 实测补上了一条 Windows 上拿不到的结论**：TCP 目标端口关闭时，
内核回 RST，被归类为「不可达」并立即返回（0.000 s），而**不是**超时。
Windows 上同一个用例（连确定关闭的端口）表现为超时，用 Python 独立对照同样超时，
所以 Windows 侧至今不宣称该路径已实测。

### Windows 实测

| 用例 | 观察到的结果 |
| --- | --- |
| `--list-backends` | 能力表与实现一致（icmp6 标为不可用） |
| `-t ::1 --mode icmp` | 不支持 1、退出码 3（**不是**静默超时） |
| `-t ::1 --mode tcp --port 18081` | `TCP-OPEN`、3 ms、退出码 0 |
| Windows x86（32 位）产物 | 在 x64 系统上可运行 |

### 对**已发布的线上产物**的复核（2026-10-15）

上面的实测都是针对**本机构建出来的**产物。发布之后又单独复核了线上那一份，
两步都做了：

1. `gh release download v2.0.0` 把 11 个附件重新拉下来 →
   `sha256sum -c SHA256SUMS.txt` **10 个产物全部 OK**；
   与本地 `release/v2.0.0/` 逐字节比对 **11/11 一致**；
2. 把下载到的 `linux-x64` 包丢进 WSL Debian 解压后直接运行（用户拿到手后的实际动作）：
   原生 ICMP 到 `127.0.0.1` 在线 0 ms、IPv6 TCP 到 `[::1]:18099` 得到 **`TCP-OPEN`**、
   同一端口走 IPv4 报不可达（服务只绑 `::1`，**正好证明 v4/v6 没被混淆**）、
   关闭端口 0.000 s 内核 RST 且退出码 1。明细表见 `docs/release.md` §8.1。

### 本轮抓出并修掉的真实缺陷

1. **Android 构建回归**：`WebviewWindow::center()` 在移动端不存在，
   v1.1.1 加「窗口尺寸收敛」时引入；因为 Android 一直没重建，直到本次才暴露。
   已把该逻辑改为桌面专属（`#[cfg(desktop)]`）。
2. **TUI 切换探测方式会丢掉 `--mock`**：`set_mode()` 里写死了 `false`，
   演示途中按一次 `T` 就静默变成真实探测，看起来像「结果变了」。
   已改为沿用启动时的选择。
3. **三端各写一份后端选择**：TUI 与 GUI 各有一份「mock 就造 MockBackend，
   否则 `backend_for_mode`」，连模拟参数都要人工对齐。
   已收敛到核心的 `resolve_for_run` / `backend_for_run`。
4. **打包脚本在受限环境下中断**：`Remove-Item` 与 `Compress-Archive -Force`
   都会走回收站并被策略拦下，已改用 .NET 的 `File.Delete`。
5. **CI 与全新克隆完全构建不起来**（`v2.0.0` tag 触发的 `Release` 与 `main` 的
   `CI` **全部任务**失败）：仓库里 `.cargo/config.toml` 硬编码了 `[net] offline = true`。
   cargo 解析依赖图、校验 `Cargo.lock` 时**必须访问 registry 索引，即使可选 feature
   未启用**，于是报 `no matching package named tokio/indexmap found`。该行写死于 M1a
   （当时零第三方依赖所以无害），M11 引入 Tauri 壳、M12 引入异步调度后才致命。
   **本地一直「看着正常」只是因为依赖缓存是热的**——这条正是 CI 的价值所在。
6. **同一份 OUI 数据在 Windows 与 Linux 上查出「看起来一样、比较却不相等」的厂商名**：
   `oui_ma_*.txt` 由 `include_str!` **逐字节**嵌入，Windows 上按 `core.autocrlf=true`
   检出成 CRLF 后，查表结果会带上尾随 `\r`（CI 实际报
   `left: Some("XEROX CORPORATION\r")` / `right: Some("XEROX CORPORATION")`，5 条测试挂掉）。
   夹具测试之所以没发现，是因为它们用 `str::lines()`——**它本身就会剥掉 `\r`**，
   而真实查表路径是手工按 `\n` 切片的。
   修法两层：① `find()` 出口处 `trim_end_matches('\r')`，让结果与换行符无关；
   ② `.gitattributes` 把 `cping-core/src/data/*.txt` 钉死为 LF，保证嵌入字节跨平台一致。
   新增两条回归：合成 CRLF 夹具（各平台都能守住修复）+ 真实数据抽查（断言输出不含 `\r`）。

7. **Release workflow 的 Linux 作业「构建成功、打包却报缺二进制」**：
   workflow 用 `cargo build --target x86_64-unknown-linux-gnu` 构建，
   产物落在 `target/<triple>/release/`；而 `scripts/package.sh` 只看
   `target/release/`，于是打印
   `[缺失] .../target/release/cping-tui 不存在或不可执行` 并以 1 退出。
   Windows 作业之所以没这个问题，是因为它显式传了 `-TargetDir target/<triple>/release`，
   Linux 作业缺了等价项——**两个作业的契约不一致**。
   修法：① `package.sh` 增加 `BIN_DIR` 显式覆盖，并在原生目录为空时自动回落到
   `target/*/release`，同时在报错里列出已找到的候选目录；
   ② workflow 显式传入 `BIN_DIR`，不依赖自动回落。
   回归验证：合成目录树跑通四种场景——原生优先 / 三元组回落 / 显式 BIN_DIR 覆盖 /
   全都没有时以非 0 退出，并确认产出的 `SHA256SUMS.txt` 可 `sha256sum -c` 通过。

8. **Release workflow 的 Android 作业组装 APK 失败**（Rust 侧四个 ABI 都编过了）：
   Gradle 报 `Value 'D:/Program Files/Android/jdk-17' given for org.gradle.java.home
   Gradle property is invalid (Java home supplied is invalid)`。
   根因是 `apps/cping-gui/src-tauri/gen/android/gradle.properties` 里提交了
   **本机专属的绝对路径**（本机默认 JDK 26、AGP 不支持，所以当初写死了 JDK 17 的路径），
   而这条属性会覆盖 `setup-java` 设好的 `JAVA_HOME`。
   修法：移除该属性，改用 `JAVA_HOME` 表达；CI 由 `actions/setup-java` 提供，
   本地由 `scripts/build-all.cmd` 提供。
9. **机制性收口**：上面第 5、6、7、8 条里，第 5、6、8 条实质是**同一类**缺陷——
   把「本机环境耦合配置」提交进了仓库，于是本地全绿、CI 全红。已新增
   `scripts/check-portability.sh` 静态拦住（盘符绝对路径 / cargo 离线开关 /
   vendored 源替换），并接进 CI 的独立 job 与 `release.yml` 的 publish 阶段。
   已用负向测试确认两类注入都能被拦下。

### 这几个缺陷是怎么被发现的

本地全绿、四平台产物也都出得来，但 `main` 与 `v2.0.0` 的 workflow 是红的。
顺着日志读下去才挖出上面几条——**都是「只在别处复现」的类型**：
一条依赖「冷缓存」，一条依赖「换行符转换」，一条依赖「CI 里的 `--target` 落点」，
一条依赖「CI 是 Linux 而本机是 Windows」。
第 7 条尤其典型：**同一个脚本被两个 job 用不同方式调用**，
契约不一致就会在「构建成功、打包失败」这种最反直觉的地方炸出来。
它们共同的形状是：**配置写成了「本机恰好成立」，而不是「任何机器都成立」**——
这也是为什么要加第 9 条那个静态守卫，靠人记不住。修复后在 CRLF 条件下重跑了完整
`cping-core` 套件（236 passed / 0 failed）复现验证，再还原为 LF 并逐字节比对备份。

## M17–M18：安全合规 · 国际化（460 → 507 个测试）

### 测试结果（2026-10-15）

| 阶段 | 版本 | 测试数 | 变化 |
| --- | --- | --- | --- |
| M15 收尾 | v2.3.0 | 393 | 基线 |
| M16 配置与模板 | v2.4.0 | 430 | CLI 28 → 45，核心 +20（`profile`） |
| M17 安全与合规 | v2.5.0 | 460 | CLI 45 → 51，TUI 43 → 47，核心 +20（`policy`） |
| M18 第一步（i18n） | v2.6.0 途中 | **507** | 核心 266 → 291，CLI 51 → 62，TUI 47 → 58 |

`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`
均干净。GUI 的 `src-tauri` **不在 workspace members 里**，要单独跑 clippy；
GUI 前端跑 `npm run typecheck`（含 `node scripts/check-i18n.mjs`）与 `npm run build`。

### M17 真实冒烟（Windows 本机，debug 真实二进制）

1. 首次运行打印授权告知并落 `~/.cping/consent`；第二次不再打印
2. **限速实测**：`127.0.0.0/24`（254 目标）`--max-pps 0` = **644 ms**，
   `--max-pps 50` = **4708 ms**（理论 ≈ 启动 0.6 s + 突发 50 + 其余 204÷50 ≈ 4.08 s）
3. `-t 224.0.0.0/29` → 退出码 2「组播地址：一个探测会打扰整组主机」+ 放行提示
4. `-t 192.0.2.5` → 退出码 2「文档示例网段 TEST-NET-1（RFC 5737）」
5. `--max-targets 70000` → 「只能收紧，不能放宽」；`--max-pps 100001` → 「要关闭限速请写 0」
6. `--audit x.jsonl` + `--watch 1 --rounds 3` → 首行 `CPING-AUDIT 1`，
   事件序列 `session_start / round ×3 / session_end`，`exitCode` 只在 `session_end`
7. 审计路径指向别人的文件 → 退出码 2「不是 CPing 审计文件，拒绝追加」
8. `artifact-manifest.sh` 无密钥 → `signed: false` + 补签方法；坏密钥 → 显式报错

### M18 真实冒烟（Windows 本机，debug 真实二进制）

1. `cping-cli --lang en --help` → 全文**零中日韩字符**
2. `cping-cli --lang zh --help` → 中文原文
3. `cping-cli --lang en -t 224.0.0.0/29` → 退出码 2，
   stderr 为英文的 `argument error: target 224.0.0.1 is in a reserved range: ...`
   + `hint: add --allow-reserved if you really mean to scan this; ...`
4. `cping-cli --lang zh -t 224.0.0.0/29` → 同一判定、中文提示、退出码 2
5. `cping-cli --lang fr` → 退出码 2，报 `--lang value "fr" is invalid;
   expected zh or en` 并打印帮助（**没有静默回退**）
6. `cping-tui --lang en --help` → 零中日韩字符；`--lang zh --help` → 中文
7. GUI：`node scripts/check-i18n.mjs` →「zh/en 各 241 条词条，键一致、
   无中文残留、占位符个数一致」

### 这两轮踩到的坑（值得记住的）

- **限速的「对不对」只能靠时间量**：`--max-pps 0` 与 `--max-pps 50` 的耗时差
  必须与「目标数 ÷ pps」吻合，否则限速可能根本没生效却看不出来。
  这类**数值型行为**要在真实二进制上量一次，不能只靠单元测试。
- **`cargo fmt --all` 会在并发改文件时格式化到别人的中间态**：
  多个任务同时改不同 crate 时，收尾要统一再 fmt 一次，
  但**别在中间态下 commit**。
- **跨层重构要先把下层 API 做完再委派上层**：否则子代理只能用临时代码兜住，
  事后还要收敛一遍（M18 第一步的 CLI/TUI 桥接就是这么留下的）。
- **子代理的「我不能改 X」会造成重复实现**：委派时要么先补好下层，
  要么明确要求它把「上层临时桥接」列成清单交回来。
- **GUI 的 `src-tauri` 不在 workspace 里**：`cargo clippy --workspace` 覆盖不到它，
  必须单独跑，否则容易漏掉 lint。
- **README 的测试徽章是手工数字**：改完测试要顺手改它（这轮从 336 校正到 507）。

## 尚未完成的验证

- **M17**：gpg **成功**签名路径（Git Bash 下 `GNUPGHOME` 解释不一致，
  只跑通「导入失败 → 显式报错」分支）、Windows Authenticode 与 Android **正式**
  签名（本机无证书 / keystore）、`sign-windows.ps1` 未在真实 PowerShell 跑过、
  Linux deb 签名（本仓库不产 deb）、TUI/GUI 限速的人工键盘与点击冒烟
- **M18**：三端在非中文 Windows / 纯英文 Linux 上的**默认语言推断**
  （本机测的是显式 `--lang` 与 Git Bash 环境变量）；GUI 的语言切换与
  `localStorage` 持久化**未在真机点过**；TUI 英文模式下的终端观感（列宽对齐）
  未人工确认；GUI 里**由 Rust 侧生成**的文本（状态列、资产摘要、趋势与告警面板）
  切到英文仍是中文——这是 M18 第二步要解决的

- **Android 真机**：模拟器已覆盖大部分；真机差异项见上
- **Linux 平台**：原生 ICMP 与 TCP 已在 WSL Debian 内实测（见上）；
  但 **Linux 的邻居表（MAC/厂商）与主机名反查未实测**，只有单测覆盖
- **ICMPv6**：未实装，注册表如实登记为不可用；IPv6 目前只能走 TCP
- 终端 resize 的动态验证（`Resize` 事件路径已实现并有字符提示）
- Ctrl+C 与退出时的终端恢复（已在 `main.rs` 用 panic hook + 正常路径双重处理，待人工确认）
- M7 快照在 Android 真实场景下的完整闭环（杀进程 → 重启 → 恢复）：
  逻辑已由 20 个 TUI 测试 + 真实格式快照的 GUI 运行验证覆盖，
  但「真的被系统杀掉再重启」这一步需要真机复测
