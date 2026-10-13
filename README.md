# CPing（跨平台重写）

原软件是 2004 年的 Windows 小工具「C网群PING工具」（Delphi + Synapse，ASPack 加壳）。
本项目按 `../tech.md` 的指南，用现代跨平台架构重写：**业务逻辑一套，界面多端**。

> 逆向结论（哪些是原版确证行为、哪些是新项目决定）见 [`docs/reverse-findings.md`](docs/reverse-findings.md)。
> 分阶段计划与验收见 [`docs/plan.md`](docs/plan.md) 与 [`docs/testing.md`](docs/testing.md)。

## 结构

```
cping/
├── crates/cping-core/        共享核心（零第三方依赖）
│   └── src/{target,result,classify,config,export,error,probe,scheduler,local_ip}.rs
├── apps/cping-tui/           ratatui + crossterm 交互式终端界面
├── apps/cping-gui/           Tauri 2 + Vue 3 + TypeScript 图形界面
│   ├── src/                  Vue 前端（桌面 / Android 两套布局）
│   └── src-tauri/            Tauri 容器（独立 workspace）
└── docs/                     逆向结论、平台兼容性、测试记录
```

**核心不重复实现**：GUI 经 Tauri Commands、TUI 直接调用，都走同一个 `cping-core`。
目标生成、探测调度、结果模型、分类、统计、导出**只有一份实现**。

## 构建与测试

```bash
# 核心 + TUI（秒级）
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p cping-tui

# 桌面 GUI
cd apps/cping-gui
npm install
npm run typecheck
npm run build
cd src-tauri && cargo build          # 或回到 apps/cping-gui 执行 npm run tauri:build

# Android
cd apps/cping-gui && npm run tauri:android:build
```

> `.cargo/config.toml` 设了 `[net] offline = true`：在受限环境里 cargo 的 registry
> 探测会挂起。需要新增依赖时先 `CARGO_NET_OFFLINE=false cargo fetch`。

## TUI 快捷键

| 键 | 功能 |
| --- | --- |
| `Enter` | 开始探测（编辑态下为确认输入） |
| `i` | 进入/退出网段前缀编辑 |
| `Esc` | 取消探测 / 关闭帮助 |
| `g` / `t` | 矩阵 / 表格视图 |
| `f` | 过滤不在线目标 |
| `s` / `S` | 导出 CSV / TXT |
| `↑ ↓ PgUp PgDn Home End` | 表格滚动 |
| `?` / `F1` | 帮助 |
| `q` | 退出（运行中需再按一次确认） |
| `Ctrl+C` | 退出并恢复终端 |

状态符号：`#` 低延迟 · `+` 中延迟 · `*` 高延迟 · `-` 无回应 · `!` 错误 · `.` 未探测。
**颜色只是辅助**，无颜色终端下形状仍然可区分。

## 当前进度

| 阶段 | 状态 |
| --- | --- |
| M0 现状检查 | ✅ |
| M0.5 动态复核 | ✅（V1/V2/V3/V6 确证，V4/V5 未能确证，已如实记录） |
| M1a 核心离线部分 | ✅ 47 测试 |
| M1b 调度 + 模拟后端 | ✅ 62 测试 |
| M2 TUI | ✅ 72 测试 + 真实控制台截图验证 |
| M3 桌面 GUI | ✅ 类型检查 + 构建 + **端到端点击验证**（255/255，在线 158） |
| M4 Android GUI | 未开始（本机缺 Android SDK/NDK） |
| M5 真实 ICMP | 未开始 |
| M6 打包发布 | 未开始 |

## 构建环境注意事项（否则会踩坑）

| 现象 | 原因 | 处理 |
| --- | --- | --- |
| `cargo build` 无输出后被 SIGTERM | 沙箱内 registry 探测挂起 | 已设 `.cargo/config.toml` 的 `[net] offline = true`；需下依赖时 `CARGO_NET_OFFLINE=false cargo fetch` |
| `npm install` 报 `spawnSync EBUSY` | esbuild / tauri-cli 的 postinstall 在本机无法 spawn | 加 `--ignore-scripts`（平台二进制仍在，`vite build` 正常） |
| npm 装得极慢/卡住 | 默认源响应慢 | 换 `--registry=https://registry.npmmirror.com` |
| GUI 启动后显示「localhost 拒绝连接」 | debug 构建会去找 `devUrl` | 用 `cargo build --release`（已移除 `devUrl`，资源内嵌） |
| `schemars` 编译失败（`E0107`） | `indexmap 1.x` 的 `std` feature 默认关闭 | 已在 `[dependencies]` 与 `[build-dependencies]` **两处**声明 `indexmap/std` |

## 已知限制

- **探测后端目前是 `MockBackend`**（模拟数据），真实 ICMP 在 M5 接入。
- 本机地址与 MAC 尚未接入平台枚举，界面显示「未知」，**不伪造**。
- 目标范围默认 `.1`–`.255`（对齐原版）；`.0` 不参与。
- 仅可用于你拥有或获授权管理的网络；并发与目标数均有上限。
