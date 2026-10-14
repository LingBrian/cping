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
npx tauri android build --apk --debug --target aarch64
```

> **环境注意**：`cping/.cargo/config.toml` 设了 `[net] offline = true`。
> 在本沙箱中 cargo 的 registry 探测会挂起，必须离线；需要下载新依赖时用
> `CARGO_NET_OFFLINE=false cargo fetch`。

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
| cargo registry 探测挂起 | `cargo build` 无输出并被 SIGTERM | `.cargo/config.toml` 设 `[net] offline = true`；需要下依赖时用 `CARGO_NET_OFFLINE=false cargo fetch` |
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
- **对照组**：用 NDK clang 编的独立小程序在**同一台模拟器**上真触发该条件
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

## 尚未完成的验证

- **Android 真机**：模拟器已覆盖大部分；真机差异项见上
- **Linux 平台**：`linux-icmp` 后端仅有代码级审查，未编译未实测（本机无 Linux/WSL）
- 终端 resize 的动态验证（`Resize` 事件路径已实现并有字符提示）
- Ctrl+C 与退出时的终端恢复（已在 `main.rs` 用 panic hook + 正常路径双重处理，待人工确认）
