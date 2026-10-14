# 平台兼容性与 ICMP 能力记录

> 状态：M0–M6。M5 已接入 Windows 真实 ICMP 并实测；M6 已产出 Android APK；
> M6 之后在 Android 模拟器（WHPX 硬件加速）完成真机级验收（见 §3.4 / §3.5）。
> 每一条结论都标了证据来源，**未实测的项一律标注「未验证」**。

## 1. 原则

1. **不把 TCP 连通性伪装成 ICMP 成功。** 任何非 ICMP 的探测必须使用独立的状态名与模式。
2. **不假设 Android 存在可用的系统 `ping` 命令。**
3. 拿不到的字段（如 MAC）**留空**，不伪造。
4. 只对用户拥有或获授权管理的网络执行探测；并发与目标数都有上限（核心默认并发 32 / 上限 256，目标上限 1024）。

## 2. 原版（CPing.exe）的实测行为

| 项 | 结论 | 证据 |
| --- | --- | --- |
| 套接字 | `socket(AF_INET, SOCK_RAW, IPPROTO_ICMP)`，每目标一个 | `analysis/dynamic_trace5.log` |
| 提权 | **无需管理员**（未开启 `IP_HDRINCL`） | 动态实测 |
| 载荷 | 固定 44 字节 | 同上 |
| 重试 | 不重试，单次采样 | 程序自带帮助文本 |
| 并发 | 255 线程 | `CreateThread`×255 |
| 超时 | **未能确定**（见下） | —— |

> 超时未能确证：DFM 无超时输入控件；未观察到 `select` / `WSAPoll` / `setsockopt(SO_RCVTIMEO)`；
> 在「目标无响应」场景下扫描 91 秒仍未结束（Frida 介入条件下）。
> 新项目因此自行定义超时（默认 1000 ms），并在 `ProbeConfig` 中可配。

## 3. 新项目的平台计划

| 平台 | 后端方案 | 关键风险 | 状态 |
| --- | --- | --- | --- |
| Windows | 手写 FFI：优先 `SOCK_DGRAM + IPPROTO_ICMP`，失败回退 `SOCK_RAW` | `SOCK_RAW` 需管理员；`SOCK_DGRAM` 普通用户可用 | ✅ **已实测通过**（见下） |
| Linux | 手写 FFI：`SOCK_DGRAM` → `SOCK_RAW` | 需要 `CAP_NET_RAW`（root 或 `setcap cap_net_raw+ep`） | 已实现，**未实测**（本机无 Linux） |
| Android | 同上（`PosixIcmpBackend`）；不可用时改用**独立 TCP 模式** | 普通应用无 `CAP_NET_RAW` | ✅ **模拟器实测**：errno 为 EPERM/EACCES，如实分类「权限不足」（§3.4）；⚠️ 后台回收偶发 abort（§3.5） |

### 3.2 Windows 实测结果（M5，本机执行）

后端 `windows-icmp`，`cargo test -p cping-core --test native_icmp_real -- --ignored --nocapture`：

| 场景 | 结果 | 判读 |
| --- | --- | --- |
| `127.0.0.1` | **在线，rtt = 0 ms** | 真的收到了 Echo Reply，`SOCK_DGRAM + IPPROTO_ICMP` 在**普通权限**下可用 |
| `192.0.2.123`（保留地址，无主机） | **`Timeout`，耗时 816 ms**（预算 800 ms） | 不存在的目标**没有被误判为在线**，超时预算生效 |
| `tcp-connect 127.0.0.1:9` | `Timeout`（`name()` = `tcp-connect`） | TCP 模式与 ICMP 名字/语义完全分离，不会互相冒充 |

> 结论：Windows 侧不需要管理员权限，也不需要 `icmp.dll` 的 `IcmpSendEcho`（那个通常要求提权）。
> 权限不足时的错误码映射（`WSAEACCES → PermissionDenied`）已实现但**在本机未触发**，
> 需在受限账户下复测。

### 3.1 Android 策略（Q5 已定）

若普通权限下无法做真实 ICMP：

- 在界面上**显式声明平台限制**，不使用任何"看起来成功"的替代；
- 另设**独立模式**「TCP 连通性测试」，其状态名与导出列都明确标注（如 `TCP-OPEN`），
  与 `在线(ICMP)` 区分开；
- 不进入同一套「在线主机数」统计口径。

### 3.3 Android 构建结果（M6，本机执行）

APK 已真实产出并通过静态校验，但**探测能力本身仍未在设备上运行过**：

| 项 | 结果 | 证据 |
| --- | --- | --- |
| debug APK | ✅ `app-arm64-v8a-debug.apk`，142 MB | `npx tauri android build --apk --debug --target aarch64` |
| release APK | ✅（未签名）`app-arm64-v8a-release-unsigned.apk`，12.7 MB | 同上（不带 `--debug`） |
| 包名 / 版本 | `com.cping.gui`，versionCode 1000，versionName 0.1.0 | `aapt2 dump badging` |
| SDK 区间 | compileSdk 36 / targetSdk 36 / minSdk 24 | 同上 |
| 原生库 | `lib/arm64-v8a/libcping_gui_lib.so`（`native-code: 'arm64-v8a'`） | `unzip -l` |
| 资源 | `assets/tauri.conf.json`（前端 `dist` 一并内嵌） | 同上 |
| 入口 | `com.cping.gui.MainActivity` | `aapt2 dump badging` |
| 签名 | debug APK 由 `CN=Android Debug` 证书签名（SHA-256 `793d68…bfea`） | `apksigner verify --print-certs` |
| 权限 | 仅 `android.permission.INTERNET` | `aapt2 dump badging` |

> **构建期踩的两个真实坑**（都已修，见 `docs/testing.md`）：
> 1. `lib.rs` 的 `run()` 缺 `#[cfg_attr(mobile, tauri::mobile_entry_point)]` →
>    Tauri CLI 校验 `.so` 报 *does not include required runtime symbols*，APK 根本不出。
> 2. `compileSdk = 37` 在本机 SDK 里不存在 → 改为 36。

### 3.4 Android 实测结果（M6 之后，模拟器真机级验收）

环境：Android Emulator 37.3.4 + **WHPX 硬件加速**、AVD `CPing_API35`
（Pixel 6 规格 1080×2400 @420dpi）、`system-images;android-35;google_apis;x86_64`（Android 15）。
APK 为 v1.0.0 双 ABI 通用 debug 包（`native-code: 'arm64-v8a' 'x86_64'`，290 MB），
设备端实际加载 `primaryCpuAbi=x86_64`。

| 项 | 结果 | 证据 |
| --- | --- | --- |
| 安装 | ✅ `adb install -r -t` Success | adb 输出 |
| 冷启动 | ✅ COLD，**不会自动开始扫描**（点击前一直保持 0/0） | `am start -W` + 截图 |
| 真实扫描 | ✅ 255 目标 / 8.27 s / **0 在线，全部「权限不足」** | 截图 `docs/images/android-list.png` |
| ICMP errno | `EPERM`/`EACCES`（由状态分类反证：`map_errno` 仅把这两个 errno 映射为 `PermissionDenied`） | 同上 |
| 状态分类 | ✅ **如实显示「权限不足」，未混同为离线/超时**（Q5 策略生效） | 同上 |
| 列表 / 矩阵 | ✅ 两视图均可用；矩阵 16 列行主序、编号 1–255，与原版一致 | `android-matrix.png` |
| 横竖屏 | ✅ 横屏布局正常渲染、竖屏恢复（`configChanges` 处理，不重建 Activity） | `android-landscape.png` |
| 前后台 | ✅ Home → 回到前台，状态保留 | 截图 |
| 导出 | ✅ 写入 `/storage/emulated/0/Android/data/com.cping.gui/files/Documents/`（应用专属外部目录，Android 10+ 分区存储**无需任何权限**） | toast + `adb pull` |
| 导出内容 | ✅ UTF-8 BOM、256 行全部 CRLF、表头+`.1`–`.255`、状态列如实写「权限不足」 | `analysis/emu_export.csv` |

**8.27 s / 255 目标的节奏**正好是 `255 / 并发32 ≈ 8 批 × 1 s 超时`——
即使 socket 创建立刻失败，探测仍按超时节拍走完，符合调度器设计。

### 3.5 退后台路径的 FORTIFY 日志：已归因，**不是本项目缺陷**

**现象**（最初记录为「间歇性 native abort」）：

```
F libc: FORTIFY: pthread_mutex_lock called on a destroyed mutex
ActivityManager: Process com.cping.gui (pid xxx) has died: cch CRE   ← cached（后台）
```

**归因结论**：这**不是崩溃**，而是 `tao` 的 Android 退出路径 + Chromium WebView
拆卸并发产生的日志噪音。证据链如下（2026-10-14 取证，可稳定复现）：

1. **退出原因是主动 `exit(0)`，不是致命信号**：
   `adb shell dumpsys activity exit-info com.cping.gui` 给出
   `process=com.cping.gui reason=1 (EXIT_SELF) subreason=0 (UNKNOWN) status=0`
   ——`EXIT_SELF` + `status=0` 表示进程自己调用了 `exit(0)`；
   系统回收（LMK）会记成 `EXIT_LOW_MEMORY`/`EXIT_OTHER_KILL`，被强停会记成
   `USER REQUESTED/FORCE STOP`，两者都不是本次的取值。
2. **没有任何致命信号**：`/data/tombstones` 目录**为空**（目录 mtime 停留在事件之前，
   未随事件更新）；`libc: Fatal signal`、`SIGABRT`、`tombstone` 关键字命中数 **0**。
   真发生 FORTIFY `abort()` 时必然出现 `Fatal signal 6 (SIGABRT)` 并留下 tombstone——
   **无 tombstone 本身就是「没有 abort」的证据**（此前文档把它误读成「无回溯所以无法归因」）。
3. **代码层面的触发点明确**：`tao-0.37.1/src/platform_impl/android/mod.rs:199`
   ```rust
   pub fn run<F>(mut self, event_handler: F) -> ! {
       let exit_code = self.run_return(event_handler);
       ::std::process::exit(exit_code);   // Android：退出码默认 0
   }
   ```
   Activity 被销毁 → `WindowEvent::Destroyed` → Tauri 置 `ControlFlow::ExitWithCode(0)`
   → tao 在 Android 上**直接 `process::exit(0)`**。这是平台上「Activity 没了就退出进程」
   的既有约定，不是异常。
4. **FORTIFY 行来自进程内的其它线程，且与 WebView 拆卸同时发生**：
   同一次事件里三条 FORTIFY 来自 **3 个不同 tid**、命中 **2 个不同互斥量地址**，
   紧接 `wm_on_destroy_called`（Activity 销毁），而 WebView 沙箱进程在 4 ms 后
   以 `subreason=17 (ISOLATED NOT NEEDED)` 退出——即 `exit(0)` 与 Chromium
   WebView 拆卸**并发**，被销毁的是平台侧（WebView）的互斥量。
5. **本项目代码不可能产生「销毁后加锁」**：bionic 上 `std::sync::Mutex` 是
   futex 实现，**没有 `pthread_mutex_destroy` 调用路径**；全仓库锁点只有
   `Arc<Mutex<Shared>>`（Tauri 托管状态）与调度器内部锁，`Arc` 生命周期长于
   所有相关线程，事件通道也已闭环。另外本项目**完全不使用剪贴板**
   （`grep -rn clipboard apps/` 为空），后台反复出现的
   `ClipboardService: Denying clipboard access to com.cping.gui` 同样来自
   WebView/Chromium 自身请求，非本应用逻辑。

**对照组（排除「工具链抓不到」的可能）**

仅有「无 tombstone」还不够——必须证明这套工具**本来抓得到**。用 NDK clang 编了一个
独立小程序（`tools/android-forensics/`，不参与 cargo 构建，仅手工编译运行）
在**同一台模拟器**上跑：

```c
/* 在已 destroy 的互斥量上 lock —— 复刻 FORTIFY 那条日志的真实触发条件 */
pthread_mutex_init(&m, NULL);
pthread_mutex_destroy(&m);
pthread_mutex_lock(&m);
```

实测（对照结果，与上面的应用日志形成鲜明差异）：

| 对照组（真触发） | 本应用退后台那次 |
| --- | --- |
| `F libc: FORTIFY: pthread_mutex_lock called on a destroyed mutex` | 同 |
| **紧跟** `F libc: Fatal signal 6 (SIGABRT) ...` | **没有**这一行 |
| `F DEBUG: Abort message: 'FORTIFY: ...'` + `__fortify_fatal` native 回溯 | **没有** |
| 生成 `/data/tombstones/tombstone_00` | tombstone 目录为空（mtime 未变） |
| 进程退出码 **134**（128+SIGABRT） | `EXIT_SELF` / `status=0` |

即：**真实的「锁已销毁互斥量」在 bionic 上必然 abort 并留下墓碑**，工具链是有效的；
本应用那次三条日志后**没有任何 abort 痕迹**，说明进程不是被 FORTIFY 终结的，
而是 `exit(0)` 已经启动、系统连同 WebView 一起拆卸时，平台侧线程碰到了
正在被销毁的互斥量，只留下了日志。结论：**日志真实、退出真实、崩溃不存在**。

**确定性复现步骤**（不再依赖「后台等 1 分钟」这种概率触发）：

```bash
adb logcat -c
adb shell am start -n com.cping.gui/.MainActivity
sleep 14
adb shell input keyevent KEYCODE_BACK     # finish MainActivity → onDestroy
adb logcat -b crash -d                    # 立即出现 3 条 FORTIFY
adb shell dumpsys activity exit-info com.cping.gui   # EXIT_SELF / status=0
```

此路径实测 1 次即命中。反过来，**只按 HOME 不销毁 Activity（仅 `onPause`/`onStop`）
不会触发**，这正是先前「三次受控重现均未复现」的原因：当时复现的是前台/息屏空闲，
而非 Activity 销毁。

**真正的影响**（非崩溃带来的）：进程退出后**内存态结果丢失**。这是 Android
生命周期的固有行为，当前由「导出 CSV」缓解；如需保留，应做「退前台时落盘快照 +
重启后恢复」，属于功能增强而非缺陷修复。

### 3.6 Linux 实测结果（M11，WSL Debian 13，2026-10-15）

本机没有独立的 Linux 机器，用 WSL2 里的 Debian 13 + Rust 1.99.0 完成构建与实测。

| 项 | 结果 |
| --- | --- |
| 构建 | ✅ `cargo test --workspace` 全绿；`cargo build --release` 产出 ELF x86-64（已 strip） |
| 原生 ICMP（有权限） | ✅ `127.0.0.1` 1 ms、默认网关 0 ms（WSL 内以 root 运行，具备 `CAP_NET_RAW`） |
| TCP v4 / v6 | ✅ `127.0.0.1:18080`、`::1:18081` 均 `TCP-OPEN` |
| **TCP 关闭端口** | ✅ 内核回 RST → 归类「不可达」、**0.000 s** 返回、退出码 1 |
| ICMPv6 | ⚠️ 未实装：`-t ::1 --mode icmp` → 「不支持」、退出码 3（如实上报，非静默超时） |
| TUI | ✅ 在真实伪终端内渲染、进入备用屏，按 `q` 优雅退出（码 0） |
| 导出 | ✅ CSV / TXT / JSON / HTML 四种均正确产出（含 IPv6 行） |

**与 Windows 的关键差异**：Linux 上「TCP 端口关闭」能可靠复现为 RST →「不可达」；
Windows 上同一用例表现为超时（用 Python 独立对照同样超时），
因此 Windows 侧不宣称该路径已实测。

**未测**：Linux 的邻居表（`/proc/net/arp`）与主机名反查链路只有单测覆盖；
`CAP_NET_RAW` 缺失时的 `EPERM` 映射未单独复现（WSL 内以 root 运行）。

## 4. 本机地址与 MAC

| 项 | 现状 |
| --- | --- |
| 本机地址 | `cping-core::local_ip` 只定义接口；平台实现留待 M5。当前返回「未知（M5 接入网卡枚举）」，**不猜地址** |
| MAC | 原版取 `GetIpNetTable`（ARP 邻居表）；回环场景下为空。新项目同样在取不到时留空 |
| 多网卡/VPN | 必须返回**列表**，由用户在设置中选择，而不是无条件取第一张网卡 |

## 5. 待实测清单（模拟器验收后更新）

- [x] Windows：无管理员权限下 raw ICMP 是否可用 —— ✅ 可用（`SOCK_DGRAM + IPPROTO_ICMP`）
- [ ] Windows：`IcmpSendEcho` 的错误码到状态映射（本机未触发权限错误）
- [ ] Linux：`CAP_NET_RAW` 缺失时的错误码是否为 `EPERM`（应映射为「权限不足」）——
      WSL 内以 root 运行，未单独复现无权限路径
- [x] Linux：原生 ICMP 与 TCP 的真实连通性 —— ✅ 实测通过（见 §3.6）
- [x] Linux：TCP 关闭端口是否被判为「不可达」而非超时 —— ✅ 是（RST，0.000 s）
- [ ] Linux：`/proc/net/arp` 邻居表与主机名反查 —— 仅单测覆盖，未实测
- [x] Android：`socket(AF_INET, SOCK_DGRAM/SOCK_RAW, IPPROTO_ICMP)` 的 errno ——
      ✅ `EPERM`/`EACCES`，状态如实分类为「权限不足」（模拟器实测）
- [ ] Android：`/system/bin/ping` 是否存在、普通应用能否执行（未测，不依赖）
- [ ] Android：Wi-Fi ↔ 蜂窝 ↔ VPN 切换时的行为与任务状态（模拟器无法覆盖）
- [x] Android：前后台切换 —— ✅ 正常；Activity 销毁后的进程退出 —— ✅
      已归因为 `tao` 主动 `exit(0)`，**非本项目缺陷**（见 §3.5，附确定性复现步骤）
- [x] Android：竖屏 / 横屏布局 —— ✅ 均正常；系统字体缩放未测
- [x] Android：文件保存 —— ✅ 落盘应用专属外部目录，内容/编码校验通过
- [x] Android：能否产出可安装的 APK —— ✅ 能（debug 变体已签名）

> **真机与模拟器的差异仍需真机补充验证**：真实 Wi-Fi 网段的扫描效果、
> 厂商 ROM 的后台管控（各厂商比 AOSP 激进得多）、字体缩放。模拟器上
> 「权限不足」的结论对真机同样成立（AOSP 权限模型一致）。
