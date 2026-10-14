# CPing v1.0.0 — 首个稳定版

对 2004 年 Delphi 版「C网群PING工具」的跨平台重写：**业务逻辑一套（`cping-core`），界面多端**（TUI / 桌面 GUI / Android）。

## 下载

| 文件 | 平台 | 说明 |
| --- | --- | --- |
| `cping-tui-v1.0.0-windows-x64.exe` | Windows x64 | 终端界面，单文件，无需安装 |
| `cping-gui-v1.0.0-windows-x64.exe` | Windows x64 | 图形界面，单文件（Tauri，资源已内嵌） |
| `cping-arm64-v8a-debug.apk` | Android 7.0+（arm64） | **debug 签名，可直接安装** |
| `cping-arm64-v8a-release-unsigned.apk` | Android 7.0+（arm64） | release 未签名，需自行签名后安装 |
| `SHA256SUMS.txt` | — | 全部产物校验和 |

Linux 产物计划在 v1.5.0（M11）随 CI/CD 一起提供。

## 亮点

- **共享核心 `cping-core`**：目标生成、并发调度、分类、统计、导出只有一份实现，三端共用；运行时零第三方依赖
- **真实 ICMP**：Windows `IcmpSendEcho`（普通权限可用）、Linux/Android `SOCK_DGRAM`（回退 `SOCK_RAW`）；TCP 连通性模式独立标注、不冒充 ICMP
- **TUI**：矩阵/表格双视图、离线过滤、CSV/TXT 导出、无颜色终端回退（符号可区分）
- **桌面 GUI**：矩阵/表格、时延配色自定义、Excel 友好的 CSV（UTF-8 BOM + CRLF）
- **Android**：列表优先的触控布局，导出到应用可写目录

## 质量

- `cargo test --workspace` **84 通过 / 0 失败**；`clippy -D warnings`、`cargo fmt --check` 通过
- Windows 真实 ICMP 端到端实测：TUI 与 GUI 均 255/255 目标扫描通过（回环网段）
- 原版行为逆向结论经 Frida 动态复核确证（详见 `docs/reverse-findings.md`）

## 已知限制

- Linux / Android 的原生 ICMP 未在真系统实测（拿不到权限时如实报「权限不足」）
- Android 未真机验证；release APK 未签名
- 本机地址 / MAC 显示「未知」（枚举尚未接入，不伪造）

完整变更见 [CHANGELOG.md](https://github.com/LingBrian/cping/blob/main/CHANGELOG.md)。

## 路线图

v1.1.0 厂商识别/MAC/主机名 → v1.2.0 持续监控 → v1.3.0 TCP 端口探测/多网段 →
v1.4.0 JSON/HTML 导出与 diff → v1.5.0 Linux 打包与 CI → v2.0.0 IPv6/异步调度。
详见 [`docs/plan.md`](https://github.com/LingBrian/cping/blob/main/docs/plan.md)。

## 使用声明

仅用于你拥有或获授权管理的网络。
