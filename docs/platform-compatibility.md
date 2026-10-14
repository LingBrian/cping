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

### 3.5 已发现的问题（开放）

**Android 后台回收路径出现一次 native abort**（间歇性，三次受控重现未复现）：

```
F libc: FORTIFY: pthread_mutex_lock called on a destroyed mutex
ActivityManager: Process com.cping.gui (pid xxx) has died: cch CRE   ← cached（后台）
```

- 发生时机：应用退到后台/息屏后约 1 分钟，被系统按 cached 进程处理时；
- 前台保持 60 s、息屏 90 s 两次受控重现均未复现，crash buffer 为空；
- 调度器自身的 mutex 全部在 `Arc` 保护下、事件通道生命周期闭环，未发现
  本项目代码存在「销毁后加锁」路径；无 tombstone（FORTIFY abort 未留下回溯），
  归因需在复现时用 `ndk-stack` 抓 native 栈，列为 **M7 待办**。

## 4. 本机地址与 MAC

| 项 | 现状 |
| --- | --- |
| 本机地址 | `cping-core::local_ip` 只定义接口；平台实现留待 M5。当前返回「未知（M5 接入网卡枚举）」，**不猜地址** |
| MAC | 原版取 `GetIpNetTable`（ARP 邻居表）；回环场景下为空。新项目同样在取不到时留空 |
| 多网卡/VPN | 必须返回**列表**，由用户在设置中选择，而不是无条件取第一张网卡 |

## 5. 待实测清单（模拟器验收后更新）

- [x] Windows：无管理员权限下 raw ICMP 是否可用 —— ✅ 可用（`SOCK_DGRAM + IPPROTO_ICMP`）
- [ ] Windows：`IcmpSendEcho` 的错误码到状态映射（本机未触发权限错误）
- [ ] Linux：`CAP_NET_RAW` 缺失时的错误码是否为 `EPERM`（应映射为「权限不足」）
- [x] Android：`socket(AF_INET, SOCK_DGRAM/SOCK_RAW, IPPROTO_ICMP)` 的 errno ——
      ✅ `EPERM`/`EACCES`，状态如实分类为「权限不足」（模拟器实测）
- [ ] Android：`/system/bin/ping` 是否存在、普通应用能否执行（未测，不依赖）
- [ ] Android：Wi-Fi ↔ 蜂窝 ↔ VPN 切换时的行为与任务状态（模拟器无法覆盖）
- [x] Android：前后台切换 —— ✅ 正常；进程回收 —— ⚠️ 出现一次 native abort（见 §3.5，待归因）
- [x] Android：竖屏 / 横屏布局 —— ✅ 均正常；系统字体缩放未测
- [x] Android：文件保存 —— ✅ 落盘应用专属外部目录，内容/编码校验通过
- [x] Android：能否产出可安装的 APK —— ✅ 能（debug 变体已签名）

> **真机与模拟器的差异仍需真机补充验证**：真实 Wi-Fi 网段的扫描效果、
> 厂商 ROM 的后台管控（各厂商比 AOSP 激进得多）、字体缩放。模拟器上
> 「权限不足」的结论对真机同样成立（AOSP 权限模型一致）。
