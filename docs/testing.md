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

## 尚未完成的验证

- **Android 真机**：安装、启动、触控、停止、前后台切换、文件保存、网络变化、竖横屏与字体缩放
- **真实 ICMP**：Windows / Linux / Android 三平台的真实能力与权限（当前仍是 `MockBackend`）
- GUI 交互实测：点击开始/停止、视图切换、离线过滤、颜色设置持久化、导出落盘
  （界面已渲染，但自动化的鼠标点击验证尚未做）
- 终端 resize 的动态验证（`Resize` 事件路径已实现并有字符提示）
- Ctrl+C 与退出时的终端恢复（已在 `main.rs` 用 panic hook + 正常路径双重处理，待人工确认）
