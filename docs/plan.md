# CPing 跨平台重构 · 分阶段计划

> 版本：v1 · 2026-10-13 · 严格遵循「先提交计划 → 再改代码 → 跑测试 → 报告实际结果」

---

## 零、环境与工具链体检（M0 实测结果）

| 项 | 实测 | 影响 |
| --- | --- | --- |
| Rust | `rustc 1.97.1` / `cargo 1.97.1`（stable-x86_64-pc-windows-msvc） | 可用 |
| 已装 target | **仅** `x86_64-pc-windows-msvc` | 无 Linux / Android target |
| Node / npm | `22.22.2` / `10.9.7` | 可用 |
| Tauri CLI | **未安装**（`cargo tauri` 报 no such command） | M3 前需装 |
| Android SDK / NDK | **缺失**（`ANDROID_HOME` 空，无 `$LOCALAPPDATA/Android/Sdk`，无 `C:/Android`） | **M4 硬阻塞** |
| JDK | `26.0.1` | Android Gradle 需 JDK 17–21，**可能不兼容 AGP**，需单独处理 |
| Git | 原工作区不是仓库；已在 `cping/` 内 `git init` | 已解决 |
| **cargo 网络** | 沙箱内 `cargo build` 会因 registry 探测卡死被 SIGTERM | **必须在 `cping/.cargo/config.toml` 设 `[net] offline = true`**（已加） |
| Linux 环境 | 无 WSL2 / 无容器探测到 | Linux GUI/TUI 构建**无法在本机验证** |

**迁移方案结论**：无迁移。当前目录不存在任何既有代码工程（仅 `CPing.exe` + `analysis/` + `tech.md`），
因此**不需要破坏性重构**。新增内容一律放进新目录，逆向产物原样保留。

---

## 阶段总览

| 阶段 | 内容 | 产出 | 当前状态 |
| --- | --- | --- | --- |
| **M0** | 现状检查、三份清单、环境体检 | `docs/reverse-findings.md`、`docs/plan.md` | ✅ 完成 |
| **M0.5** | 动态复核 V1–V6 | `docs/reverse-findings.md` §5 + `analysis/m05*` | ✅ 完成（V1/V2/V3/V6 确证；V4/V5 未能确证） |
| **M1a** | `cping-core` 纯离线部分 | `crates/cping-core` + 47 个测试 | ✅ 完成（fmt/clippy 零警告，47/47 通过） |
| **M1b** | `cping-core` 调度：`ProbeBackend` + `MockBackend` + 并发/超时/取消/事件 | `probe.rs`、`scheduler.rs` | ✅ 完成（62 测试） |
| **M2** | TUI（ratatui + crossterm），先接 MockBackend | `apps/cping-tui` | ✅ 完成（72 测试 + 真实控制台截图） |
| **M3** | 桌面 GUI（Tauri 2 + Vue 3），先接 MockBackend | `apps/cping-gui` | ✅ 完成（构建 + 运行截图） |
| **M4** | Android GUI（`MobileLayout.vue` + 平台保存 + 真机验证） | 同上 | ⚠️ 构建链路打通、APK 产出；**真机验证未做**（无设备） |
| **M5** | 真实 ICMP 后端分平台接入 | `probe` 后端实现 | ✅ 完成（Windows 实测 255/255；Linux/Android 未实测） |
| **M6** | 打包与发布文档 | `docs/platform-compatibility.md`、`docs/testing.md`、`docs/release.md` | ✅ 完成（APK ×2 + 文档；真机/签名/Linux 列为已知限制） |

> M0.5 与 M1a 可并行；M1a 不依赖任何网络或平台能力，风险最低，故作为**第一阶段编码任务**。

---

## 第一阶段（M1a）详细计划

### A. 目标

建立 Cargo Workspace 与 `cping-core` 的**纯离线可测**部分：地址解析、目标生成、结果模型、分类、统计、导出。
**不包含**任何网络 I/O、不包含 GUI/TUI、不引入 Tauri 依赖。全部逻辑用单元 + 整合测试覆盖。

### B. 准备新增的文件

| # | 文件 | 理由 |
| --- | --- | --- |
| 1 | `Cargo.toml`（workspace 根） | 统一管理 `crates/*`、`apps/*`；`resolver = "2"`；workspace 级依赖与 lint |
| 2 | `rust-toolchain.toml` | 锁定 stable 通道与 `rustfmt`/`clippy` 组件，保证多机一致 |
| 3 | `.gitignore` | 忽略 `target/`、`node_modules/`、`dist/`；**不忽略** `analysis/` 与 `docs/` |
| 4 | `crates/cping-core/Cargo.toml` | 核心 crate 清单；本阶段仅需 `serde`、`thiserror`、`uuid` |
| 5 | `crates/cping-core/src/lib.rs` | 模块装配与对外 re-export，定义公共 API 面 |
| 6 | `crates/cping-core/src/error.rs` | 统一 `ProbeError` / `CoreError`，**必须区分**超时、不可达、权限不足、内部错误 |
| 7 | `crates/cping-core/src/target.rs` | 前缀解析（`Ipv4Prefix`）+ 目标生成策略 + **矩阵索引映射**（16×16） |
| 8 | `crates/cping-core/src/result.rs` | `ProbeStatus` / `ProbeResult` / `ProbeEvent` / `TaskStats` / `task_id` |
| 9 | `crates/cping-core/src/classify.rs` | `classify_rtt()` + 可配置阈值，边界语义对齐原版 `<50 / 50–100 / >100` |
| 10 | `crates/cping-core/src/config.rs` | `ProbeConfig` / `ColorConfig` + TOML 序列化 |
| 11 | `crates/cping-core/src/export.rs` | CSV（UTF-8 BOM / CRLF / RFC 4180 转义）与 TXT 定宽导出 |
| 12 | `crates/cping-core/src/local_ip.rs` | 本机地址**列表**抽象（本阶段仅定义接口 + 占位实现，M5 补平台实现） |
| 13 | `tests/target_prefix.rs` | 前缀解析与目标范围测试 |
| 14 | `tests/classify_boundaries.rs` | 分类边界测试（49/50/99/100/101 + 超时/不可达/权限） |
| 15 | `tests/matrix_index.rs` | 目标 ↔ 16×16 矩阵索引双向映射测试 |
| 16 | `tests/export_format.rs` | CSV/TXT 字段、转义、换行、空字段、排序测试 |

**本阶段明确不创建**：`scheduler.rs`、`probe.rs`（留到 M1b）；`apps/**`（留到 M2/M3）；任何 Vue/TS 文件。

### C. 关键设计约束（写入代码前先定死）

1. `parse_prefix("192.168.1") -> Ok(Ipv4Prefix)`；拒绝空串、段数 ≠ 3、非数字、> 255、含前导 `+`/空格等。
2. `build_targets(prefix, TargetRange::Standard)` 默认产出 **255** 个：`.1`–`.255`。
   另备 `Full`（`.0`–`.255`）与 `HostsOnly`（`.1`–`.254`）仅供测试对照；
   **`.0/.255` 的默认处理在 M0.5 动态验证前不写死为「原版行为」**，只作为新项目默认值记录。
3. 矩阵索引：`index = row * 16 + col`，`targets[index]` ↔ `(col,row)`；对 255 个目标的越界格（第 256 格）显式定义为空。
   该映射独立成函数并双向测试，**不散落在 UI 里**。
4. `classify_rtt(rtt)` 返回枚举；`None`/错误另路返回 `Offline`/`Error`，**不用 0ms 代表失败**。
5. 统计（完成数、在线数、耗时）只在核心计算，UI 不得自行累加。
6. 导出函数签名为 `export(results: &[ProbeResult], format: ExportFormat) -> Result<Vec<u8>, CoreError>`，
   返回字节缓冲，由调用方决定落盘方式（GUI 用 Tauri 文件 API，TUI 用 `std::fs`）。

### D. 测试命令

```bash
# 静态检查
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings

# 构建
cargo build --workspace

# 测试（本阶段的核心验收）
cargo test -p cping-core
cargo test --workspace -- --nocapture
```

### E. 验收条件（M1a）

| # | 验收项 | 通过标准 |
| --- | --- | --- |
| 1 | 构建 | `cargo build --workspace` 零错误、零警告（clippy `-D warnings` 通过） |
| 2 | 前缀解析 | 覆盖空/段数错/非数字/超 255/前后空格/多小数点等用例，全部按预期报错或通过 |
| 3 | 目标范围 | `Standard` 恰为 255 个、首 `.1` 尾 `.255`；三种策略数量与首尾均被断言 |
| 4 | 矩阵索引 | 全部 255 个目标 ↔ (row,col) 双向一致；第 256 格行为被显式断言 |
| 5 | 分类边界 | `49/50/99/100/101` 五值落类明确；超时/不可达/权限错误**分类互不相同** |
| 6 | 导出 | CSV 含 BOM+CRLF；含逗号/引号/换行的字段转义正确；空 MAC 写空；按 IPv4 数值升序 |
| 7 | 统计一致性 | 同一组 `ProbeResult` 的统计结果在多次调用与不同入口下完全一致 |
| 8 | 无回归 | `analysis/**`、`CPing.exe`、`tech.md` 的哈希/内容未被改动 |

### F. 本阶段不做的验收（避免误判完成）

- 不接任何真实网络探测 → **不验证 ICMP**（属 M5）。
- 不产出 GUI/TUI 界面 → **不验证交互**（属 M2/M3）。
- 不验证 Android → 属 M4，当前受 SDK/NDK 缺失阻塞。

---

## 需要用户拍板的开放项

| # | 问题 | 备选 |
| --- | --- | --- |
| Q1 | 新工程放哪 | ① 当前工作区根（推荐）② 新子目录 `cping/` ③ 另建独立仓库 |
| Q2 | 是否 `git init` | ① 初始化并分阶段提交（推荐，便于回滚）② 不初始化 |
| Q3 | 先做 M0.5（REA 动态复核）还是直接进 M1a 编码 | ① 先 M0.5 补齐 `.0/.255` 与矩阵映射（推荐，消除最大歧义）② 直接 M1a |
| Q4 | 「无回应」颜色是否允许用户修改 | ① 允许（推荐，UX 更好）② 照抄原版禁用 |
| Q5 | Android 若普通权限拿不到 ICMP | ① 显式声明限制 + 独立的 TCP 模式（推荐）② 只声明限制、不提供 TCP 模式 |
| Q6 | 默认超时与并发 | 建议 `timeout=1000ms`、`concurrency=32`，可否？ |


---

## 阶段报告：M3（桌面 GUI）

### 构建结果

| 项 | 命令 | 结果 |
| --- | --- | --- |
| 前端类型检查 | `npm run typecheck` | ✅ 0 错误 |
| 前端生产构建 | `npm run build` | ✅ 2701 modules → `dist/` |
| Tauri 外壳 | `cd src-tauri && cargo build` | ✅ `cping-gui.exe`（15.5 MB） |
| 窗口创建 | 启动 exe | ✅ 标题「C网群PING工具」1016×719 |
| 界面渲染 | release 构建 + 截图 | ✅ `analysis/gui_desktop.png` |

### 过程中修掉的两个真实构建故障

1. **`schemars` 0.8 编译失败**（`E0107`：`IndexMap` 需要 3 个泛型参数）
   - 根因：`indexmap 1.x` 的 `std` feature 默认关闭 → build.rs 走 autocfg 的 sysroot 探测
     → 本机探测失败 → `has_std` 未生效 → `IndexMap<K, V, S>` 失去默认参数。
   - 修复：显式 `indexmap = { version = "1.9", features = ["std"] }`，
     **且必须同时写在 `[dependencies]` 与 `[build-dependencies]`**（resolver v2 分开解析 feature）。
2. **`tauri-build` 缺 `icons/icon.ico`** → 已补齐多尺寸图标。

### 已知限制

- **探测后端仍是 `MockBackend`**：界面跑通的是模拟数据，真实 ICMP 在 M5。
- **本机 dev 模式不可用**：vite dev server 监听 1420 被拒（`EACCES`），
  因此只能走 `cargo build --release`（内嵌 `dist`）。
- **npm install 需要 `--ignore-scripts` + 淘宝源**：postinstall 的 `spawnSync` 在本环境 EBUSY；
  平台二进制仍在，`vite build` 实测正常。
- **Android 未验证**：本机缺 Android SDK / NDK。

### 下一步（M4 / M5）

1. M4：准备 Android 工具链（SDK/NDK + `rustup target add`），真机验证移动布局、
   触控、前后台、文件保存、网络切换。
2. M5：为 Windows / Linux / Android 分别实现 `ProbeBackend`，实测权限与错误码，
   并把结果登记到 `docs/platform-compatibility.md`。

---

## 路线图：v1.0.0 之后的开发阶段（2026-10-14 规划）

> 每个阶段一个次版本号（破坏性变更升主版本）；沿用同一流程：
> 先计划 → 再实现 → 测试全绿 → 实测记录 → 更新 CHANGELOG 与平台文档。
> 新功能一律落在 `cping-core`，三端（TUI / 桌面 GUI / Android）同步暴露，不单独实现。

### 总览

| 阶段 | 版本 | 主题 | 核心新功能 |
| --- | --- | --- | --- |
| **M7** | v1.1.0 | 目标信息增强 | 厂商识别（离线 OUI 库）、ARP/MAC 获取、主机名反查 |
| **M8** | v1.2.0 | 持续监控模式 | 定时自动重扫、上下线事件流与高亮、扫描间隔可配 |
| **M9** | v1.3.0 | 探测能力扩展 | 自定义端口 TCP 探测、多网段批量扫描、CIDR 目标 |
| **M10** | v1.4.0 | 数据与集成 | JSON/HTML 导出、扫描对比 diff、无头 CLI 模式 |
| **M11** | v1.5.0 | 平台与分发 | Linux 实测与 deb/AppImage 打包、GitHub Actions 三平台 CI |
| **M12** | v2.0.0 | 架构升级 | IPv6 支持（新矩阵布局）、可选 Tokio 异步调度、后端插件化 |

### M7 — v1.1.0 目标信息增强

新功能：

1. **厂商识别**：内置离线 OUI 数据库（IEEE MA-L/MA-M/MA-S 子集，约 3 万条，
   编译期嵌入或随包携带），从 MAC 前 3 字节查出厂商名。
2. **MAC 地址获取**：Windows 用 `GetIpNetTable2`（ARP 缓存），Linux/Android 读
   `/proc/net/arp`；拿不到就显示空，不伪造。
3. **主机名反查**：DNS 反解（`getnameinfo`）+ Windows NetBIOS 后缀查询（可选）。
4. **结果快照与恢复**（源自 Android 生命周期取证结论，见
   `platform-compatibility.md` §3.5）：Activity 被销毁会带动进程退出，内存态结果必然丢失。
   因此改为在「退到后台」时把当前结果快照落盘（应用专属外部目录，无需任何权限），
   下次启动时提示「检测到上次未保存的扫描结果（N 个目标 / 时间），是否恢复」；
   **默认不自动恢复**，避免旧数据被误当成新扫描结果。

验收要点：OUI 查询纯函数化并全覆盖单测；MAC/主机名缺失时界面与导出仍结构完整；
导出列扩展后 CSV/TXT 向后兼容（新列追加在末尾）；快照恢复必须显式确认，
且恢复前后 `total/online/elapsed` 等统计与快照时刻完全一致。

### M8 — v1.2.0 持续监控模式

新功能：

1. **定时重扫**：扫描完成后按配置间隔（30s/1min/5min/自定义）自动重扫。
2. **变化事件流**：核心层输出 `HostUp` / `HostDown` / `RttChanged` 事件；
   GUI 列表对刚变化的主机高亮数秒。
3. **通知**：桌面端可选系统通知（新增主机时）；TUI 状态栏显示变化计数。

验收要点：重扫复用同一 `taskId` 语义（新一轮次可辨识）；取消语义不回归；
连续运行 1 小时内存无泄漏（测试断言事件数组不无限增长）。

### M9 — v1.3.0 探测能力扩展

新功能：

1. **自定义端口 TCP 探测**：指定端口列表做连通性探测，结果明确标注 `TCP-OPEN`，
   不与 ICMP 在线混计（延续 Q5 决策）；Android 上作为 ICMP 不可用时的主探测方式。
2. **多网段批量扫描**：一次任务排队多个 C 段（如 `192.168.1,192.168.31`）。
3. **CIDR 目标**：支持 `/24`–`/30` CIDR 输入（矩阵视图仅对 ≤256 目标的任务可用）。

验收要点：目标上限保护不变（单任务 ≤65535）；CIDR 解析拒绝主机位为 0/全 1 的歧义输入
（或显式定义语义并有测试）；GUI 输入框与 TUI 编辑器均做校验。

### M10 — v1.4.0 数据与集成

新功能：

1. **JSON 导出**：完整结构化导出（含配置快照与统计），便于程序消费。
2. **HTML 报告**：单文件自包含 HTML（内联矩阵快照 + 表格），方便归档分享。
3. **扫描对比**：保存上次结果后一键 diff（新增 / 消失 / 延迟变化），导出 diff 报告。
4. **无头 CLI**：`cping-tui --scan 192.168.1 --json --quiet` 直接输出结果、退出码
   反映在线数，供脚本与 CI 使用（TUI 交互模式不受影响）。

验收要点：导出字节缓冲统一走 `cping-core::export`；diff 是纯函数并有边界测试；
CLI 模式不初始化终端 UI。

### M11 — v1.5.0 平台与分发

新功能：

1. **Linux 实测**：在真实 Linux（或无沙箱环境）实测 `SOCK_DGRAM`/`SOCK_RAW`
   ICMP、`CAP_NET_RAW` 缺失时的错误映射，更新平台兼容性文档。
2. **打包**：deb / AppImage（GUI）、Linux TUI 二进制；Windows NSIS 安装包恢复。
3. **CI/CD**：GitHub Actions——push 跑 `fmt + clippy + test + typecheck`；
   打 tag 自动构建三平台产物并创建 GitHub Release 草稿。

验收要点：CI 全绿是合并门槛；release 产物带版本号与校验和（SHA-256 清单）。

### M12 — v2.0.0 架构升级（主版本，含破坏性变更）

新功能：

1. **IPv6 支持**：目标模型支持 v6 前缀；矩阵视图为 v6 设计新布局（v6 地址空间
   无法 16×16 一格一 IP，改为分页矩阵/列表为主）；ICMPv6 后端按平台接入。
2. **可选异步调度**：`cping-core` 增加 `tokio` feature（默认仍零依赖 + std 线程），
   高并发场景（数千目标）切换异步实现，API 不变。
3. **后端插件化**：`ProbeBackend` 注册表 + 运行时选择（icmp / tcp / mock / 未来
   的 snmp 等），GUI 设置页可选后端。

验收要点：默认构建仍不引 Tokio；IPv6 与 IPv4 的结果模型统一、分类阈值一致；
插件注册表有文档化的 trait 稳定性承诺。

### 里程碑原则（不变）

- 每阶段先在本文件追加详细计划（文件清单 + 验收条件），再动代码。
- 「实测过」与「未实测」在文档里严格分开表述；绝不把计划写成事实。
- 破坏性变更（M12）升主版本并提前在 CHANGELOG 标注迁移说明。
