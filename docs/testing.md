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
| M3 桌面 GUI | 见下 | 见下 | 见下 |

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

## 尚未完成的验证

- GUI（Tauri）构建与运行实测 —— 见 `docs/plan.md` 阶段报告
- Android 真机：安装、启动、触控、停止、前后台、文件保存、网络变化
- 真实 ICMP：Windows / Linux / Android 三平台能力与权限
- 终端 resize 的动态验证（自动化困难，`Resize` 事件路径已实现并有字符提示）
- Ctrl+C 与退出时的终端恢复（已在 main.rs 用 panic hook + 正常路径双重处理，待人工确认）
