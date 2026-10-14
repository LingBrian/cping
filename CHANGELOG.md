# 更新日志（Changelog）

格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，
版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/)。

## [未发布]

### 文档

- **归因纠正**：Android 退后台出现的
  `F libc: FORTIFY: pthread_mutex_lock called on a destroyed mutex` 加
  `has died: cch CRE` 曾被记为「间歇性 native abort」。经取证
  （`dumpsys activity exit-info` 记为 `EXIT_SELF`/`status=0`、无 tombstone、
  无 `Fatal signal`，并用独立对照程序证明工具链能捕获真 abort）确认为
  `tao` 在 Android 上主动 `process::exit(0)` 与 Chromium WebView 拆卸并发
  产生的日志，**不是本项目缺陷**；同时给出确定性复现步骤（`KEYCODE_BACK`
  销毁 Activity 即可命中，仅按 HOME 不会）。详见
  `docs/platform-compatibility.md` §3.5。

## [1.0.0] - 2026-10-14

首个稳定版本。对 2004 年 Delphi 版「C网群PING工具」的跨平台重写：
业务逻辑一套（`cping-core`），界面多端（Windows/Linux TUI、Windows/Linux 桌面 GUI、Android GUI）。

### 新增

**共享核心 `cping-core`（零第三方运行时依赖）**

- C 类网段前缀解析与目标生成（默认 `.1`–`.255` 共 255 个目标，对齐原版；`.0` 不探测）
- 16×16 矩阵索引映射（`index = row * 16 + col`，格号即 IP 末段）
- 探测结果模型与四档时延分类（<50ms / 50–100ms / >100ms / 无回应），错误区分
  超时、不可达、权限不足、不支持、内部错误
- `ProbeBackend` 抽象 + std 线程并发调度器（可配置并发/超时，取消保留已完成结果）
- CSV（UTF-8 BOM + CRLF + RFC 4180 转义，按 IPv4 数值升序）与 TXT 定宽导出
- 平台原生 ICMP 后端：Windows（`IcmpSendEcho`，普通权限可用）、Linux/Android
  （`SOCK_DGRAM` ICMP，回退 `SOCK_RAW`）；独立的 TCP 连通性后端明确标注、不冒充 ICMP

**TUI（`cping-tui`，ratatui + crossterm，Windows/Linux）**

- 矩阵 / 表格双视图、离线过滤、网段前缀编辑、帮助面板
- CSV / TXT 导出、终端 resize 自适应、无颜色终端回退（符号始终可区分）
- Ctrl+C 安全退出并恢复终端状态

**桌面 GUI（`cping-gui`，Tauri 2 + Vue 3 + TypeScript，Windows/Linux）**

- 矩阵 / 表格双视图、离线过滤、时延配色自定义、CSV / TXT 导出
- 桌面布局与移动布局自动切换；移动布局为列表优先的触控界面（列表/矩阵页签、抽屉设置）
- Android 平台导出写入应用可写目录并回报完整路径

**Android**

- arm64-v8a APK 构建链路（debug 已签名 / release 未签名），minSdk 24 / targetSdk 36
- 通过 aapt2 / apksigner 产物校验；包名 `com.cping.gui`，权限仅 INTERNET

### 验证（实测）

- 84 个 Rust 测试全部通过（`cargo test --workspace`）；前端 `vue-tsc` 0 错误
- Windows 真实 ICMP 端到端：TUI 与 GUI 均完成 255/255 目标扫描（回环网段，`windows-icmp` 后端）
- 原版行为逆向结论经 Frida 动态复核确证（详见 `docs/reverse-findings.md`）

### 已知限制

- Linux / Android 的真实 ICMP 未在真机/真系统实测；拿不到权限时按「权限不足」如实上报
- Android 真机验证未做（无设备）；release APK 未签名
- 本机地址 / MAC 枚举未接入，界面显示「未知」，不伪造

[1.0.0]: https://github.com/LingBrian/cping/releases/tag/v1.0.0
