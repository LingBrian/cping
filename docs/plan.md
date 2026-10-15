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
| **cargo 网络** | 沙箱内 `cargo build` 会因 registry 探测卡死被 SIGTERM | **按需临时离线**：`cargo build --offline` / `CARGO_NET_OFFLINE=true cargo build`。⚠️ **不要**落盘到 `.cargo/config.toml`（v2.0.0 发行时因此让 CI 全红，详见「路线图扩展」后的 M11 实施记录） |
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
| **M7** | 目标信息增强（v1.1.0） | OUI 厂商识别、ARP/MAC 获取、主机名反查、**结果快照与恢复** | ✅ 完成（151 测试 + 真实局域网实测 + GUI 真实运行截图） |

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

**实现记录（2026-10-14，v1.1.0）**：

- OUI 数据量比原计划大一个量级：不是「约 3 万条」而是 **54,190 条**（MA-L 40,325 /
  MA-M 6,633 / MA-S 7,232，共 1.68 MB）。三张表按 9/7/6 个十六进制位最长前缀优先匹配，
  编译期嵌入、运行时零 I/O。数据来自 IEEE 官方 CSV，由 `tools/oui-gen/gen_oui.py`
  确定性生成（附 `SOURCE.md`），可复现。
- 快照落盘时机与原计划不同：不是「退到后台」，而是「**每轮扫描结束 + 每次信息变化**」。
  原因是 Android 上销毁 Activity 会**直接退出进程**（`tao` 调 `process::exit`），
  退出钩子根本没机会跑；「扫描一结束就落盘」才是真正可靠的时机。
  「退后台」只在桌面端作为额外保险（窗口失焦时再落一次盘）。
- 快照指纹机制：同一份结果只询问一次（扫描时间/网段/总数/条数/用时 任一变化
  才视为新结果），避免用户每次启动都被同一个弹窗打扰。
- 真实数据抓出并修复两个单测抓不到的 bug：`SOCKADDR_INET` 按整型读取导致 IP
  字节序颠倒；广播 MAC 随 `.255` 目标混进结果表。
- 实测记录见 `docs/testing.md` §M7。

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

---

## 路线图扩展：v2.0.0 之后的十余个阶段（2026-10-15 规划）

> v1.0.0 之后共规划 **13 个阶段（M8–M20）**，其中 M8–M12 通向 **v2.0.0**，
> M13–M20 通向 **v3.0.0**。每阶段一个次版本号（破坏性变更升主版本）。

### 全量阶段总览

| 阶段 | 版本 | 主题 | 一句话 |
| --- | --- | --- | --- |
| M8 | v1.2.0 | 持续监控 | 定时重扫 + 上下线事件流 + 变化高亮 |
| M9 | v1.3.0 | 探测扩展 | TCP 端口探测 + CIDR + 多网段 |
| M10 | v1.4.0 | 数据与集成 | JSON / HTML / 扫描 diff / 无头 CLI |
| M11 | v1.5.0 | 平台与分发 | Linux 实测 + 打包 + GitHub Actions |
| M12 | **v2.0.0** | 架构升级 | IPv6 + 可选异步 + 后端插件化 |
| M13 | v2.1.0 | 资产识别 | 端口指纹、服务横幅、设备类型推断 |
| M14 | v2.2.0 | 历史与趋势 | 时序落盘、可用率报表、离线率曲线 |
| M15 | **v2.3.0** ✅ | 告警与通知 | 阈值告警、Webhook / 邮件 / 系统通知 |
| M16 | **v2.4.0** ✅ | 配置与模板 | 扫描模板、配置导入导出、CLI 配置文件 |
| M17 | **v2.5.0** ✅ | 安全与合规 | 签名制品、权限最小化、审计日志、限速 |
| M18 | v2.6.0 | 国际化与无障碍 | i18n（中/英）、键盘全导航、屏幕阅读器 |
| M19 | v2.7.0 | 规模化性能 | 万级目标、流式结果、内存预算与背压 |
| M20 | **v3.0.0** | 生态与开放 | 插件 SDK、REST/WS API、脚本引擎 |

### M13 — v2.1.0 资产识别

新功能：

1. **端口指纹**：对在线主机并行探测常见端口集合（22/80/443/445/3389…），
   输出「开放端口」列表；每端口超时独立、可配置。
2. **服务横幅抓取**：可选的轻量 banner grab（只读前若干字节，不做协议交互），
   结果原样保存并标注来源，绝不猜测。
3. **设备类型推断**：基于「厂商 + 开放端口组合」给出一条**推断**结论，
   界面明确标注为「推断」而非事实。

验收要点：端口扫描与 ICMP 结果分列，不混入在线判定；指纹规则表纯数据驱动、有单测；
未识别的组合留空而非臆造。

### M14 — v2.2.0 历史与趋势

新功能：

1. **时序落盘**：每轮扫描追加一条精简记录（时间戳 + 每目标状态/延迟档），
   环形缓冲 + 分级降采样，默认保留 7 天。
2. **可用率报表**：按主机统计在线率、平均/最差延迟、抖动，导出 CSV/HTML。
3. **离线曲线**：GUI 内嵌轻量折线（纯 SVG，不引图表库），TUI 用字符柱状图。

验收要点：存储格式版本化、坏数据不阻塞读取；时间跨度与保留策略有明确测试；
报表数值与原始记录可逐条对账。

### M15 — v2.3.0 告警与通知

新功能：

1. **阈值告警规则**：新增主机 / 主机离线 / 延迟超过阈值 / 在线率低于阈值。
2. **通知通道**：系统通知（Windows Toast / Linux notify-send）、Webhook（HTTP POST JSON）、
   可选 SMTP 邮件；通道失败必须可见（不静默吞掉）。
3. **抑制与去重**：同一事件在冷却窗口内只通知一次；告警状态在快照中持久化。

验收要点：网络不可用时通知失败不得阻塞扫描主流程；规则匹配是纯函数且有全边界测试；
用户可一键静音所有告警。

### M16 — v2.4.0 配置与模板

新功能：

1. **扫描模板**：把「网段 + 范围 + 超时 + 并发 + 后端 + 端口」存为命名模板，一键调用。
2. **配置导入导出**：`cping.toml` 完整往返（含未知键保留），带 schema 版本号与迁移。
3. **CLI 配置文件**：`cping-cli --config prod.toml` 直接消费模板，便于 CI 复用。

验收要点：旧版本配置文件必须仍能读取（向前兼容有测试）；模板切换不静默丢弃用户改动。

### M17 — v2.5.0 安全与合规

新功能：

1. **制品签名**：Windows exe 代码签名（可选，无证书则至少出 SHA-256 + sigstore 式自签清单）、
   Android release APK 正式签名、Linux 提供 deb 签名说明。
2. **权限最小化与限速**：全局速率上限（pps）、目标数上限、禁止扫描保留网段的白名单机制。
3. **审计日志**：记录每次扫描的发起者/时间/参数/结果摘要，落盘可导出，用于自证合规。

验收要点：默认速率上限存在且可配置；审计日志不含敏感内容（不含主机名以外的隐私）；
文档明确「仅用于获授权网络」并在首次启动时提示。

### M18 — v2.6.0 国际化与无障碍

新功能：

1. **i18n**：中/英双语，前端 `vue-i18n` 风格的自研轻量字典（避免引重依赖），
   核心层错误信息改为错误码 + 参数，由界面本地化。
2. **键盘全导航**：GUI 全功能可纯键盘操作，焦点可见；TUI 已有，补齐 GUI。
3. **无障碍**：高对比主题、字体缩放（150%/200%）、ARIA 标注、色盲友好备选配色。

验收要点：无颜色/无鼠标环境下所有功能可达；本地化键无遗漏（有测试断言字典键一致）。

### M19 — v2.7.0 规模化性能

新功能：

1. **万级目标**：`/16` 级目标（65536）分片流式扫描，结果不整表驻留 UI。
2. **背压与内存预算**：事件通道带上限，UI 慢时合并进度事件（丢弃中间态保终态）。
3. **基准**：`cargo bench` 风格的基准用例（自研计时，不引 criterion），
   把「1 万目标 / 32 并发 / 1s 超时」作为回归基线。

验收要点：内存峰值有上限断言；UI 不卡死（长任务期间仍可取消）；性能数据写入文档。

### M20 — v3.0.0 生态与开放（主版本）

新功能：

1. **插件 SDK**：稳定 ABI/API 的 `cping-plugin` crate，第三方可实现探测后端并动态加载。
2. **本地 API**：可选启用的 REST + WebSocket 服务（仅监听回环，默认关闭），
   供看板/脚本消费实时事件流。
3. **脚本引擎**：极小的规则脚本（用于判定与告警条件），不引 JS 运行时，走受限表达式求值。

验收要点：API 默认关闭且仅回环；插件崩溃不得带崩主程序（隔离与超时）；
主版本迁移说明完整（2.x → 3.0 的破坏性变更逐条列出）。

### 里程碑原则（不变）

- 每阶段先在本文件追加详细计划（文件清单 + 验收条件），再动代码。
- 「实测过」与「未实测」在文档里严格分开表述；绝不把计划写成事实。
- 破坏性变更（M12 / M20）升主版本并提前在 CHANGELOG 标注迁移说明。
- 三个阶段一条硬线：**核心逻辑只在 `cping-core`，三端不重复实现。**

---

## 阶段报告：M8–M12 实施记录（2026-10-15，通向 v2.0.0）

> 本节只记录**已实际完成并验证**的内容；计划与事实严格分列。

### M8 — v1.2.0 持续监控

- 新增 `cping-core::monitor`：`MonitorConfig`（间隔/启用/最小变化阈值）、
  `ChangeKind`（`Appeared` / `Disappeared` / `RttClassChanged` / `StatusChanged`）、
  `Change` 记录、`diff_scans(prev, next) -> Vec<Change>` 纯函数、
  `ChangeLog` 环形缓冲（容量上限，保证长跑不增长内存）。
- 新增 `cping-core::history`：`HistoryBuffer` 保存每轮轮次的精简快照（供 M14 复用）。
- TUI：`M` 切换监控、`[`/`]` 调整间隔、`e` 查看变化事件流、变化行高亮。
- GUI：侧栏「持续监控」开关 + 间隔选择（30s/60s/5min/自定义）、
  「变化」页签列出事件流、刚变化的主机在表格/矩阵中高亮并自动淡出。
- 验收：`diff_scans` 全边界单测；`ChangeLog` 容量上限断言；取消语义不回归（沿用 M1b 测试）。

### M9 — v1.3.0 探测扩展

- 新增 `cping-core::spec`：`TargetSpec` 解析器，支持单网段（`192.168.1`）、
  CIDR（`192.168.1.0/24`，主机位非 0 时**报错并给出正确网段**）、
  多段用逗号/空格/换行分隔，统一产出 `Vec<IpAddr>` + `Vec<Segment>`（每段的显示名与起始下标）。
- 新增 `ProbeMode`：`Icmp` / `Tcp { port: u16 }`。**一次任务只有一种方式**，
  因此 ICMP 的「在线」与 TCP 的「开放」不会混进同一列统计；
  `ProbeMode::COMMON_PORTS` 提供 8 个常用端口供界面轮换。
- **实现时收敛的一处设计（与本节初版计划的差异）**：**没有**给 `ProbeStatus` 新增
  `TcpOpen` 变体，也**没有**给 `ScanStats` 加 `tcp_open` 字段。TCP 开放复用
  `Online { rtt_ms }`，「TCP-OPEN / TCP-超时 / TCP-关闭」由 `ProbeMode` 在**显示层**派生。
  好处是统计口径只有一套（在线就是在线、离线就是离线），代价是「这条结果是 ICMP 还是 TCP」
  必须靠导出里的「探测方式」列区分——该列已随 CSV/TXT 一并产出。
- `MAX_TARGETS` 由 1024 提升到 **65536**；矩阵视图另有
  `MATRIX_VIEW_MAX = 256` 的独立上限，两者互不代表。
- Android 默认后端为 TCP（`ProbeMode::Tcp { port: 80 }`）；桌面端默认 ICMP，可随时切换。
- 验收：`TargetSpec` 解析全用例（含拒绝主机位非 0 的 CIDR、拒绝超上限的巨型网段）；
  TCP 后端以本地监听端口实测。

### M10 — v1.4.0 数据与集成

- 新增 `cping-core::json`：零依赖 JSON 序列化（严格转义、稳定键序），
  导出结构含 `meta` / `config` / `targets` / `stats` / `changes`。
- 新增 `cping-core::html`：单文件自包含 HTML 报告（内联矩阵 + 表格 + 统计卡片，
  无外部资源、无 JS）。
- 新增 `cping-core::diff`：两次扫描对比（新增/消失/延迟变化/状态变化），
  导出为 CSV / JSON / HTML 三种形式。
- 新增二进制 `apps/cping-cli`：无头模式，退出码语义
  （`0` 全部在线 / `1` 有离线 / `2` 参数错误 / `3` 权限或后端不可用），
  支持 `--json` / `--csv` / `--html` / `--quiet` / `--watch`。
- 验收：JSON 转义全覆盖（引号/反斜杠/控制字符/非 ASCII）；HTML 无外部引用断言；
  CLI 退出码实测。

### M11 — v1.5.0 平台与分发

- Linux：在 WSL Debian 13 内装 Rust 1.99.0，构建并**实测** `cping-tui` 与 `cping-cli`：
  原生 ICMP（`SOCK_DGRAM`）实测通过；TCP v4/v6 实测通过；
  TUI 在真实伪终端内渲染并进入备用屏，按 `q` **优雅退出（码 0）**。
  结论写入 `docs/platform-compatibility.md`。
- **离线构建链路**：WSL 内没有外网，因此先在 Windows 侧 `cargo vendor` 生成依赖快照，
  再同步进 WSL 并由 `.cargo/config.toml` 指向本地 vendored 源。
  CI 环境不需要这一手，但它让「完全断网也能复现 Linux 产物」成立。
- 打包：`scripts/package.sh`（tar.gz + 内附 `README.txt`）与
  `scripts/package.ps1`（zip + 内附 `README.txt`），各自产出 `SHA256SUMS`；
  两处的版本、平台、用法、退出码、权限说明保持同一套文案，不各写一份。
- CI/CD：`.github/workflows/ci.yml`（fmt / clippy / test / 前端 typecheck 与构建 /
  Android 交叉编译检查，并断言核心依赖树只有一行）与
  `.github/workflows/release.yml`（打 tag 自动构建 Windows x64+x86 / Linux / Android 产物，
  合并校验和后创建 Release）。
- **实现时收敛的一处（v2.0.0 发布时才真正跑通）**：CI 与打包链路此前**从未绿过**，
  被三个「本地看不出、CI 才炸」的缺陷挡住，全部在 `v2.0.0` 发行过程中定位并修掉：
  1. 仓库内 `.cargo/config.toml` 硬编码 `[net] offline = true`（M1a 时期零依赖所以无害，
     M11 引入 Tauri 壳、M12 引入异步调度后致命）；
  2. `scripts/package.sh` 只看 `target/release/`，而 Linux 作业用 `--target <triple>`
     构建、产物在 `target/<triple>/release/`——**同一个脚本被两个 job 用不同方式调用，
     契约不一致**；
  3. `gen/android/gradle.properties` 提交了本机专属的 JDK 绝对路径，
     覆盖了 `setup-java` 设好的 `JAVA_HOME`。
  为此新增 `scripts/check-portability.sh`（静态拦住盘符绝对路径 / cargo 离线开关 /
  vendored 源替换），接进 CI 独立 job 与 release 的 publish 阶段。
  详见 `docs/testing.md`「本轮抓出并修掉的真实缺陷」与 `docs/release.md` §8.4。
- 验收：打包脚本产出可解压可运行的包并实测冒烟；Windows x86 产物在 x64 系统上运行正常；
  CI 全部任务绿（2026-10-15 达成，含新增的 portability job）。

### M12 — v2.0.0 架构升级（主版本）

- **IPv6**：目标模型**不新造类型**，直接用 std 的 `IpAddr`——v4/v6 在同一个
  `Vec<IpAddr>` 里混排，排序口径 `spec::addr_sort_key`（`(u8, u128)`）只定义一次，
  导出 / 差异 / 监控三处不再各写一份。新增 `TargetSpec::has_ipv6()` 与
  `matrix_compatible()`；矩阵视图对 v6 自动退化为表格
  （v6 没有「最后一格 = 格号」的语义，硬套只会得到一张骗人的图）。
- **后端插件化**：`BackendRegistry`（内置 `icmp4` / `icmp6` / `tcp` / `mock`）提供
  `BackendCaps`（可用性 + 支持哪些地址族 + 是否需要权限 + 说明）与 `resolve()`。
  三端的后端选择（含 `--mock` 分支）由 `resolve_for_run` / `backend_for_run` 单点给出，
  界面不再自己推「现在用的是哪个后端」——推错就会出现「界面写 icmp4、实际跑 mock」。
- **可选异步**：新增 `async` feature（默认关），提供 `AsyncScheduler`
  （信号量封顶并发 + `spawn_blocking`），语义与同步版逐条对齐；
  默认构建仍零第三方依赖（`cargo tree -p cping-core -e normal` 只有它自己一行）。
- **实现时收敛的一处边界（与本节初版计划的差异）**：**ICMPv6 未实装**。
  计划里的 Windows `Icmp6SendEcho2` 与 `neighbor` 的 IPv6 邻居表都没有做；
  注册表把 `icmp6` 明确登记为 `available: false` 并给出人话说明——
  宁可说「不支持」，也不做「能编译但扫不出结果」的假支持。
  IPv6 的完整可用路径是 **TCP**（`SocketAddr::new` 原生支持 v6），
  已在 Windows 与 Linux 上实测（`::1` → `TCP-OPEN`）。
- **破坏性变更**：`ProbeBackend::probe` 与目标模型全面 `IpAddr` 化（约 20 个文件）；
  `cell_of_addr` 返回 `Option<usize>`（v6 → `None`）；`matrix_slots` 泛型化为
  `AsTargetAddr`。逐条迁移说明写入 CHANGELOG 的 `[2.0.0]` 段。
- 验收：默认 feature 组合下 `cargo tree` 无第三方运行时依赖；
  v4 与 v6 走同一套 `classify`（阈值不分族）；注册表有文档化的稳定性承诺
  （v2.x 内只新增、不改签名）。

---

## 阶段报告：M13–M20 实施记录（通向 v3.0.0）

> 与 M8–M12 同一套纪律：每阶段先写计划（上文的 M13–M20 各节），再动代码；
> 「实测过」与「未实测」严格分开；核心逻辑只在 `cping-core`。

### M13 — v2.1.0 资产识别（实施记录，2026-10-15）

**实现文件**

- `crates/cping-core/src/fingerprint.rs`（新增，约 700 行含测试）
  - 纯数据表 `PORT_SERVICES`（40 条端口 → 服务名，有「升序且唯一」的守卫测试）
  - `PortScanConfig`：端口列表 / 单端口超时 / 单主机并发 / 是否抓横幅，
    `parse_ports` 支持 `22,80,443-445` 写法并拒绝越界与超跨度区间
  - `probe_port` / `grab_banner` / `sanitize_banner` / `scan_ports`
  - `infer_device`：规则按特异性排序的**纯函数**（RDP → 网络设备 → SMB → SSH →
    打印 → VNC → 数据库 → Web 兜底），未识别返回 `None`
  - `fingerprint_hosts`：三端共享的批量入口
- `crates/cping-core/src/export.rs`：`ScanReport` 新增 `fingerprints` 字段与
  `with_fingerprints()`；CSV 追加 `开放端口`/`设备推断` 两列（**只在末尾**）、
  TXT 追加独立小节；`html.rs` 追加独立分区
- `apps/cping-cli`：`--fingerprint` / `--fp-ports` / `--no-banner` / `--fp-timeout`
- `apps/cping-tui`：`F` 键 + 独立面板（`fingerprint_lines()` 与界面同源）
- `apps/cping-gui`：`fingerprint_hosts_cmd` / `clear_fingerprints` / `fingerprint_state`
  + 「资产」页签 `FingerprintPanel.vue` + 侧栏按钮

**实现时收敛的两处设计（与本节初版计划的差异）**

1. 计划里写「端口指纹结果」如何承载没说清。实现时定为：**绝不进 `ProbeResult`**，
   而是与 `results` 并列的独立数组。理由是「在线」判定只能有一个来源；
   一旦塞进结果模型，任何一个消费方都可能把「端口全关」读成「离线」。
2. TUI 的指纹探测放在**后台线程**（计划里没提）。单主机 30 端口 × 800 ms
   最坏要几秒，同步做会把界面卡住——卡界面比慢几秒更糟。

**验收对照**

| 验收要点 | 落地情况 |
| --- | --- |
| 端口扫描与 ICMP 结果分列，不混入在线判定 | CSV 只在末尾追加两列（前 9 列逐字节断言不变）；TXT/HTML 独立小节且标题写明边界；JSON 独立数组 |
| 指纹规则表纯数据驱动、有单测 | `PORT_SERVICES` / `NETWORK_VENDOR_KEYS` 为常量数组；表结构有守卫测试；`infer_device` 为纯函数，8 条规则各有测试 |
| 未识别的组合留空而非臆造 | `infer_device` 返回 `Option<DeviceGuess>`，`None` 时 CSV/TXT/HTML 均为空、JSON 为 `null`，有专门测试 |

**测试与实测**

- 单元/集成测试：+33（`fingerprint.rs` 19 + `fingerprint_export.rs` 7 +
  CLI 参数 3 + TUI 4），工作区合计 **309 passed / 0 failed**
- 真实冒烟（Windows 本机回环）：起一个「连接后主动问候」的服务，
  `cping-cli -t 127.0.0.1 --fingerprint --fp-ports 18555,1` →
  识别 `18555` 开放、`1` 未开放，横幅 `SSH-2.0-CPingSmokeTest` 原样抓取；
  `--no-banner` 下同一端口横幅为空；TXT/CSV/JSON/HTML 四种导出均含指纹数据
- 未实测：跨主机（非回环）的指纹与横幅；IPv6 目标的端口指纹；
  Android 上的指纹（TCP 可用，但未在真机跑过）

### M14 — v2.2.0 历史与趋势（实施记录，2026-10-15）

**实现文件**

- `crates/cping-core/src/timeseries.rs`（新增，约 1090 行含测试）
  - `HostAgg`（只存**可累加**的量：`samples` / `online` / `rtt_sum` / `rtt_max`）、
    `SeriesPoint`（`at_ms` / `span_ms` / `rounds` / `hosts`）、`TimeSeries`
  - `append` / `append_results`：时间倒退的点**拒绝**（否则序列乱序、曲线错位）
  - `prune`：按保留期裁剪，基准是**最新点**而不是「现在」
  - `downsample`：从**最旧的一半**两两合并（同 IP 才合并），返回合并数
  - `to_text` / `from_text`：行式文本格式，零依赖；版本不符报错、坏行跳过并计数
  - `report()` → `HostReport`（观测/在线轮数、可用率、平均/最差 RTT、抖动、点数）
  - `sparkline_svg` / `char_bar_chart`：两个纯函数，GUI / TUI 各用其一
  - `report_csv` / `report_html`：报表导出（CSV 带 BOM，与主导出同口径）
- `apps/cping-cli`：`--history` / `--report` / `--keep-days`
- `apps/cping-tui`：`U` 键 + 趋势面板（`trend_lines()` 与界面同源）
- `apps/cping-gui`：`trend_state` / `clear_trend` / `export_uptime_report`
  + 「趋势」页签 `TrendPanel.vue`（桌面与移动端共用，移动端关掉导出按钮）

**实现时收敛的三处设计（与本节初版计划的差异）**

1. `span_ms` 的语义改过一次。初版定义为「与上一个点的时间差（向后看）」，
   合并与裁剪都算错——合并后的窗口长度必须覆盖两点区间。最终定为
   **前向窗口 `[at, at+span)`**：`span` 表示「这个点往后覆盖多久」，
   上一个点的窗口延伸到下一个点的起点；最新点的 `span` 是 0
   （还没有后继，长度未知，不能凭空给一个数）。
2. 「抖动」的算法明确为「相邻点**平均 RTT** 之差的平均绝对值」，不是 RTT 逐样本
   标准差。理由是前者对单点内的降采样不敏感，报表可逐条对账回原始记录。
3. TUI 不落盘，只累积在进程内；跨会话的长期历史由 `cping-cli --history` 负责。
   两者共用同一套核心（`TimeSeries`），不存在两份口径。

**顺手修掉的一个真 bug**

`HostReport::worst_rtt_ms` 最初写成「`rtt_max == 0` 则为 `None`」。而回环地址的
RTT 本来就是 0 ms ——拿 0 当「没测到」的哨兵，会把一台**在线且极快**的主机在
报表里显示成「-」。改为以 `online == 0` 判定，并补了一条专门的回归测试
（`online_with_zero_rtt_still_reports_a_worst_rtt`）。

**验收对照**

| 验收要点 | 落地情况 |
| --- | --- |
| 存储格式版本化 | 文件头 `CPING-TS <version>`；版本不符 `from_text` 返回 `Err` 并说明原因，有测试 |
| 坏数据不阻塞读取 | 单行解析失败只跳过并计数（`from_text` 返回 `skipped`），CLI 据此打印警告后继续；有测试 |
| 时间跨度与保留策略有明确测试 | `span_of_a_point_is_its_window_towards_the_next_one`、`prune_uses_latest_point_as_reference_not_wall_clock` |
| 报表数值与原始记录可逐条对账 | `downsample_preserves_totals_exactly`（合并前后 `samples`/`online`/`rtt_sum`/`rounds` 四者守恒）、`report_matches_raw_records_host_by_host`（期望值手算给出） |
| 曲线不把「没测到」画成「离线」 | `uptime_curve_marks_missing_points_as_none`、`sparkline_breaks_the_line_on_missing_points`、`char_bar_chart_is_fixed_width_and_marks_gaps`（三处一致） |

**测试与实测**

- 单元/集成测试：+27（`timeseries.rs` 17 + CLI 参数 4 + TUI 5 + 核心零值边界 1），
  工作区合计 **336 passed / 0 failed**
- 真实冒烟（Windows 本机回环，两段会话）：
  1. `cping-cli -t 127.0.0.1 -n 3 -w 1 --history net.ts --report uptime.html`
     → 每轮各追加 1 点（`point <at_ms> <span_ms> 1` + `host ...` + `end`），
     结束后写出 HTML 报表，退出码 0
  2. 另起一次会话，`-n 2 -q` 续写**同一个** `net.ts` → 累计 5 个时间点；
     `--report uptime.csv` 显示 `127.0.0.1,5,5,100.0,...`
- `cargo fmt --check` / `cargo clippy --workspace --all-targets` 两套干净；
  前端 `vue-tsc --noEmit` 0 错误、`npm run build` 通过
- 未实测：跨天/跨周的长期保留与降采样（降采样只在单测里跑过，没有真实跑满
  2016 个点）；Android 上的趋势页（与桌面同一份 Vue 代码，未在真机跑过）；
  GUI 趋势页的人工目视确认（只做了构建与类型检查）

### M15 — v2.3.0 告警与通知（实施记录，2026-10-15）

**实现文件**

- `crates/cping-core/src/alerts.rs`（新增，约 1200 行含测试）
  - `AlertKind`（`Appeared` / `Disappeared` / `LatencyAbove` / `UptimeBelow`）
    带 `key()` / `label_zh()` / `parse_key()` / `uses_threshold()`；`Severity` 三档
  - `AlertRule`（`kind` / `enabled` / `threshold` / `cooldown_secs`）、
    `AlertConfig`（`rules` / `muted` / `capacity`）、`AlertContext`
  - **纯函数** `match_rule(rule, ctx) -> Option<AlertEvent>`
  - `AlertEngine`：`evaluate` / `prime_with` / `reset` / `apply_config` /
    `to_text` / `from_text` / `save_to_path` / `load_from_path` /
    `suppressed_by_cooldown` / `suppressed_by_mute` / `log_mut`
  - `AlertLog`：环形缓冲 + `total_seen`
- `crates/cping-core/src/notify.rs`（新增，约 1400 行含测试）
  - `deliver(events, cfg) -> Vec<DeliveryOutcome>`、`notifications_available()`
  - 五个通道：`console` / `system`（Windows Toast、Linux `notify-send`）/
    `webhook`（仅 `http://`）/ `command` / `smtp`（明文）
  - `expand_template`（整词替换）、`events_to_json`、`base64`、
    `encode_powershell`、`ps_quote`、`xml_escape`、`truncate`
  - `AlertSink`：后台线程 + `sync_channel(64)`，队列满丢弃并计数
- `crates/cping-core/src/timefmt.rs`（新增）：时间算法下沉，消除三份拷贝
- `apps/cping-cli`：告警与通知全部参数 + `--notify-list`；
  `execute()` 拆成 `run_rounds()` + `finish_alerts()`
- `apps/cping-tui`：`A` 告警面板（首次按即启用）、`!` 静音、状态栏告警摘要
- `apps/cping-gui`：`alert_state` / `set_alerts_enabled` / `set_alert_muted` /
  `update_alert_rules` / `clear_alerts` + `alert-event` 事件 + 「告警」页签
  `AlertPanel.vue`（桌面与移动端共用）

**实现时定死的三条边界（本节初版计划只写了「阈值告警」，没说清这几处）**

1. **第一轮不发 `Appeared`**。第一轮只建立基线（`primed` 抑制）。不这么做的话，
   扫一个 /24 会一次刷出 255 条「新增主机」——那不是告警，是噪音。
   纯函数层保留「`was_online == None` 且在线 → 新增」的语义，
   「第一轮不刷屏」由引擎的 `primed` 负责，两层各有测试。
2. **阈值用严格不等号**。`LatencyAbove` 用 `>`、`UptimeBelow` 用 `<`。
   恰好等于阈值不告警——`>=` / `<=` 会让「阈值 200ms」在 200.0ms 上报一次，
   用户按字面理解会觉得是误报。
3. **静音不是暂停**。静音时规则照常求值、`last_fired` 照常推进，只是不返回事件。
   因此解除静音**不会**补报一堆积压告警。这是「静音」与「暂停」的本质区别，
   也是这一版里最容易被实现错的一处（朴素做法是直接 `return` 掉早退，
   那样一解除静音就会炸出一堆历史告警）。

**通知通道的两条原则**

- **通道失败必须可见**：每个通道单独汇报成功/失败与原因，一个通道挂了不影响其它
  通道；失败还会在 `--quiet` 下照样打到 stderr，并给一句「告警本身已记录」的提示。
- **不静默降级**：`webhook` 只支持 `http://`。`https://` 会被明确拒绝并提示改用
  `--notify-cmd` 调 curl——把告警明文发出去比发不出去更糟。
- **不阻塞主流程**：投递在后台线程，队列满时丢弃并计数（`dropped`），
  由 `finish_alerts()` 如实报出「有 N 批未投递」。

**验收对照**

| 验收要点 | 落地情况 |
| --- | --- |
| 规则匹配与状态无耦合 | `match_rule` 是纯函数，`AlertContext` 携带全部输入；`appeared_only_fires_for_a_genuinely_new_host` 等直接在纯函数层断言 |
| 第一轮不刷屏 | `first_round_does_not_report_every_online_host_as_new`（核心 256 台 + TUI 256 台两条路径各一条） |
| 阈值边界 | `threshold_uses_a_strict_inequality`（等于阈值不告警） |
| 冷却按主机、按类型分别生效 | `cooldown_suppresses_a_level_rule_within_the_window`、`cooldown_is_per_host_and_per_kind` |
| 静音不改变状态 | `mute_silences_events_but_keeps_the_dedup_state_advancing`（核心）+ TUI 同名场景 |
| 投递不阻塞、失败可见 | `AlertSink` 的 `dropped()` 计数 + CLI `finish_alerts()` 的通道结果表；`--notify-list` 列出能力 |
| 状态文件版本化 | `state_version_mismatch_is_rejected_and_bad_lines_are_counted`；`state_roundtrip_is_byte_stable` |

**测试与实测**

- 单元/集成测试：+57（`alerts.rs` 27 + `notify.rs` 22 + CLI 参数 5 + TUI 3），
  工作区合计 **393 passed / 0 failed**
- 真实冒烟（Windows 本机，非模拟后端）：
  1. 目标 `1.1.1.1`（实测 271 ms），`--alert-latency 200` 触发 1 条；
     **不给 `--notify`、只给 `--notify-webhook` 与 `--notify-cmd`** →
     `console` / `webhook` / `command` 三条通道同时生效，
     webhook 收到 200，两处 JSON 逐字段一致
  2. `--notify webhook --notify-webhook https://…` → 通道明确失败并给出改用
     `--notify-cmd` 的提示
  3. 四类误用（缺地址 / 缺 `--notify-cmd` / 地址没被 `--notify` 选上 /
     SMTP 缺收件人）全部退出码 2；`--notify telegram` 退出码 2
  4. `--notify-list` 正确列出五通道能力（Windows 上 `system` 可用）
  5. 告警状态跨会话：会话 B 被冷却挡下、会话 C 静音，计数与文件内容一致
- `cargo fmt --check` / `cargo clippy --workspace --all-targets -- -D warnings`
  两套工作区干净；GUI（独立工程）另跑一遍同样干净
- 前端 `vue-tsc --noEmit` 0 错误、`npm run build` 通过
- 未实测：TUI 的 `A` / `!` 未做人工键盘冒烟（单元测试直接驱动 `on_key` 与求值路径）；
  GUI 告警页未做真机点击冒烟（仅编译 + 类型检查 + 构建通过）；
  `system` 通道只在 Windows 上跑过（Linux 的 `notify-send` 未在真机验证）；
  `smtp` 通道未接真实中继（只覆盖了构造与错误路径）


### M16 — v2.4.0 配置与模板（实施记录，2026-10-15）

**实现文件**

- `crates/cping-core/src/profile.rs`（新增，约 1140 行含 20 条测试）
  - `CONFIG_SCHEMA_VERSION = 2`、`SCHEMA_VERSION_KEY`、`DEFAULT_PROFILE_KEY`、
    `PROFILES_SECTION`
  - `ScanProfile`（`name` / `spec` / `range` / `timeout_ms` / `concurrency` /
    `mode` / `note` / `extra`）带 `validate()` / `probe()` / `overrides()` /
    `describe()` / `target_count()`
  - `ProbeOverrides`：模板**只**覆盖探测参数（不含导出路径/通知通道，见下）
  - `UnknownEntry`（`section` / `key` / `raw` / `path()`）
  - `ConfigDocument`：`schema_version` / `default_profile` / `profiles` /
    `app` / `unknown`；`to_toml` / `from_toml` / `load_from_path` /
    `save_to_path` / `select_profile` / `upsert_profile` / `remove_profile` /
    `profile_names` / `validate`
  - `migrate()`：v1 → v2；`schema_version` 高于支持值时**拒绝读取**
  - `APP_SECTIONS` / `APP_KEYS`：应用配置的已知面，`[probe]` 等小节的读写
    **委托**给 `AppConfig::to_toml` / `from_toml`（唯一实现）
- `crates/cping-core/src/target.rs`：新增 `TargetRange::parse_key` / `label_zh`
- `crates/cping-core/src/lib.rs`：导出 `profile` 模块与相关类型
- `apps/cping-cli/src/args.rs`：`--config` / `--profile` / `--list-profiles` /
  `--dump-config`，以及 `apply_config()`（三层合并 + `config_notes`）
- `apps/cping-cli/src/main.rs`：`print_config_notes()` / `print_profiles()` /
  `write_effective_config()` 与两个早退分支
- `cping.example.toml`（新增）：三份模板 + 全部应用配置 + 逐条说明

**实现时定死的几处边界（本节初版计划只写了「配置与模板」，没说清这几处）**

1. **优先级是「命令行 → 模板 → `[probe]`」，不是「模板优先」**。
   模板盖掉用户在命令行上手敲的值，是最难查的一类问题（「我明明写了
   `--timeout 5000`」）。因此模板只填空缺，且**每一层提供了哪些字段都打印出来**。
   被盖掉的配置值也一并说明；**值相同不算冲突**——同一件事说两遍会把真正
   重要的说明淹掉。
2. **`--profile` 点名的模板不存在就报错**，并列出所有现有模板名，绝不悄悄
   退回 `default_profile`。「我明明选了 prod 怎么扫的是开发网段」比报错危险得多。
3. **模板只管「扫什么、怎么扫」**。导出路径、`--history`、通知通道、告警规则
   **刻意不进模板**：否则 CI 里换个模板会连通知一起换掉，排查起来是场噩梦。
   `--dump-config` 的输出末尾会明写「哪些项刻意未写入」。
4. **目标规格只能落进模板**。`[probe].prefix` 是「单个 C 类前三段」，
   装不下 CIDR 与多网段；硬塞会丢信息。所以 `ScanProfile` 保留 `spec` 原文，
   `probe()` 里 `prefix: None`，`--dump-config` 也把目标写进 `[[profiles]]`。
5. **注释不保留，未知键保留**。受限子集解析器不建 AST，保留注释需要一个
   完整的格式保持型 TOML 实现；未知键则必须保留——悄悄吃掉用户不认识的项
   是不可接受的。这条取舍写在生成文件的第一行注释里。
6. **`schema_version` 只向前兼容一个方向**：低版本读入时自动迁移并把动作
   打印出来；高版本**拒绝读取**。猜着读会吃掉用户配置里我们不认识的项。
7. **`TargetRange::parse_key` 写错返回 `None`，不回落成 `Standard`**。
   配置里写错一个范围却静默按 255 个目标去扫，是「跑完才发现扫错了」的事故。
8. **`--list-profiles` / `--dump-config` 走早退分支，位置在 `validate()` 之前**。
   `validate()` 第一条就是「必须有目标」，而列模板/导出配置恰恰不需要目标；
   放在后面会永远执行不到（与 `--list-backends` / `--notify-list` 同一处理）。

**范围决定：本版只接 CLI，TUI / GUI 暂不消费模板**

- 依据：M16 计划原文第 3 条写的是「**CLI** 配置文件」，验收要点也只涉及
  「旧配置仍可读」「模板切换不静默丢弃用户改动」两条，均已满足。
- 核心 API（`ConfigDocument` / `ScanProfile` / `ProbeOverrides`）已就绪，
  三端接的是**线**而不是规则，后续接入不需要再动 `cping-core`。
- TUI 的目标模型是「单个 C 类前三段 + 矩阵视图」（`parse_prefix` 决定矩阵能否
  渲染），与模板支持的 CIDR / 多网段不是同一个模型；硬接会出现「选了模板但
  矩阵是空的」。GUI 侧则需要先设计模板选择器的界面位置。
  两者并入 M18（i18n 与无障碍）统一动界面，避免同一块 UI 改两遍。

**验收对照**

| 验收要点 | 落地情况 |
| --- | --- |
| 旧版本配置文件仍能读取 | `v1_config_without_schema_version_is_migrated_and_reported`（核心）+ `m16_v1_config_is_migrated_and_still_readable`（CLI）；迁移动作逐条打印 |
| 配置完整往返（含未知键保留） | `round_trip_is_byte_stable`、`unknown_keys_are_preserved_verbatim`、`m16_dumped_config_keeps_unknown_keys` |
| schema 版本与迁移 | `a_newer_schema_version_is_refused_not_guessed`、`migration_is_deterministic` |
| 模板切换不静默丢弃用户改动 | 模板只填空缺；`m16_cli_values_always_win_over_the_profile`、`m16_same_value_is_not_reported_as_a_conflict`；`配置来源：…` 逐行打印 |
| 模板不存在不静默退回 | `m16_unknown_profile_is_an_error_listing_what_exists` |
| `[profiles]` 单方括号被拒绝 | `profiles_must_be_written_as_an_array_of_tables`（核心） |
| 参数漂移守卫 | `app_keys_cover_everything_app_config_writes`（`APP_KEYS` 与 `AppConfig::to_toml` 对表） |
| `--dump-config` 可复现 | `m16_dumped_config_round_trips_through_the_reader`（导出 → 读回 → 再合并，结果一致） |

**测试与实测**

- 单元/集成测试：+37（`profile.rs` 20 + CLI 参数 13 + CLI 导出侧 4），
  工作区合计 **430 passed / 0 failed**（CLI 自身 28 → 45）
- `cargo fmt --check` / `cargo clippy --workspace --all-targets -- -D warnings`
  工作区干净；GUI（独立工程）另跑一遍同样干净
- 真实冒烟（Windows 本机）：
  1. `--config prod.toml --list-profiles` 正确列出 2 份模板，标出
     `default_profile`，目标数量分别 1 / 2
  2. 不给 `--profile` → 用 `default_profile`；给 `--profile dmz` →
     `hosts_only` 2 个目标 / `TCP:445` / 2000 ms / 并发 8 全部生效
  3. `--profile nope` → 退出码 2 并列出「现有模板：lan, dmz」；
     `--profile` 缺 `--config`、`--list-profiles` 缺 `--config` 同样退出码 2
  4. 命令行 `-t` / `--range` / `--timeout` 分别盖掉模板对应字段，
     `配置来源：` 逐条说明；模板仍提供未被覆盖的 `concurrency` / `mode`
  5. v1 旧配置（`schema_version = 1`，含陌生的 `[my_plugin]` 小节）读入时
     自动迁移为 `default` 模板，并报告「另有 2 个本程序不认识的键，已原样保留」
  6. `--dump-config` 写出的文件读回后，目标/超时/并发与首次完全一致；
     `[my_plugin]` 小节与模板 `note` 均保留；覆盖原有规格时明确提示
  7. 无目标的 `--dump-config` 只写 `[probe]` 并提示目标需每次用 `--target`
  8. `schema_version = 9` → 退出码 2「拒绝读取」；`[profiles]` 单方括号 →
     退出码 2「模板必须写成数组表 [[profiles]]」
  9. 配置文件缺失 / 写出路径不可写 → 均退出码 2，**不静默按默认值跑**
- 未实测：TUI / GUI 侧的模板消费（本版未接，见上「范围决定」）；
  Linux / Android 上的 `--config`（与平台无关的纯文件读写，但未在真机跑过）

### M17 — v2.5.0 安全与合规（实施记录，2026-10-15）

**实现文件**

- `crates/cping-core/src/policy.rs`（新增，约 1000 行含 20 条测试）
  - `ScanPolicy { max_pps, max_targets, allow_reserved }` + `validate()` /
    `check_targets()` / `describe()`
  - `reserved_reason(ip) -> Option<&'static str>`（纯函数，给**原因**而不是布尔）
  - `Bucket`（纯算术令牌桶）+ `RateLimiter`（带时钟、可取消的等待）
  - `AuditEvent` / `AuditRecord` / `AuditLog`（JSONL 追加写，首行带格式标识）
  - `consent` 子模块：`ConsentState` / `consent_path()` / `load()` /
    `acknowledge()` / `notice_text()`
  - `operator_name()` / `machine_name()`（取不到就空串，**不猜、不填占位符**）
- `crates/cping-core/src/error.rs`：新增 `CoreError::ReservedTarget { ip, reason }`
  ——单独立变体而不是塞进 `InvalidConfig`，因为三端都要能把「这条提示」与
  「配置写错了」区分开，前者要给出**可执行的下一步**
- `crates/cping-core/src/scheduler.rs`：`ProbeScheduler` 持有 `ScanPolicy`
  （`set_policy` / `policy`）；`Shared` 带 `Arc<RateLimiter>`；
  `worker_loop` 在「已拿到目标、确认没取消」之后、「真正发包」之前限速
- `apps/cping-cli`：`--max-pps` / `--max-targets` / `--allow-reserved` /
  `--audit`，`print_authorization_notice()`，`Session` 带 `AuditLog`，
  会话开始 / 每轮 / 会话结束各落一条审计
- `apps/cping-tui`：同样三个策略参数；`App.policy` + `apply_policy()`
  （装配失败写状态栏）；授权提示在进入备用屏幕**之前**打印
- `apps/cping-gui`：**刻意**不调 `set_policy`，用无条件默认值（见下）
- `scripts/artifact-manifest.sh` / `scripts/sign-windows.ps1` /
  `scripts/sign-android.sh`（新增）；`release.yml` 三处接入
- `docs/signing.md`（新增）

**实现时定死的几处边界**

1. **默认就是「有限速」**（500 包/秒）。合规不能指望用户自觉去找开关。
   想关必须显式写 `--max-pps 0`，而且关掉这件事会出现在参数摘要里
   ——「限速被关掉」需要摆在明面上，默认值则不（否则每行都是噪音）。
2. **保留网段拦的是什么，不拦什么，都是刻意的**。拦「会打扰别人」（组播、
   广播）、「没有意义」（`0.0.0.0/8`）、「一定会失败」（文档示例网段）；
   **不拦**私有网段与回环——那正是本工具的正常用途，拦掉等于把工具废掉。
   自测用的 `127.0.0.1` 因此仍然可用。
3. **只报第一个命中的地址**。列 255 条不会让人更快修好配置，只会把真正
   那一句淹掉。
4. **越界一律报错，不夹取**。`--max-pps 100000` 是上限；`--max-targets`
   **只能比 65536 更严**（那是结果槽位的预分配长度，参数突破不了）。报错
   里直接写清「想关限速请写 0」「只能收紧」。
5. **限速绝不阻塞取消**。`RateLimiter::wait` 每轮醒来都问一次「是否已取消」，
   单次睡眠上限 50 毫秒——限速变成取消不掉的等待，用户就只能杀进程。
6. **策略不进配置文件**。`--max-pps` / `--max-targets` / `--allow-reserved`
   只走命令行：一份从别处拷来的模板能把限速改成「不限速」，等于留了个
   绕过合规的暗门。
7. **审计不放隐私**。只有「谁、何时、扫什么规格、什么参数、结果计数、退出码」；
   逐主机明细、MAC、厂商、反查出的主机名一律不写。审计要能自证「我扫了什么」，
   而不是变成一份资产清单。
8. **审计文件认标识**。首行不是 `CPING-AUDIT <版本>` 就拒绝追加——往别人的
   文件里塞东西比写不进去危险得多。写入失败必须打到 stderr。
9. **首次告知不阻塞**。CLI 会被 CI 与计划任务调用，卡在等人按回车等于把工具
   弄坏。提示一次、记一次（按版本号判重），`--quiet` 下改走 stderr。
10. **不伪造签名**。这是本阶段最重要的一条：没有密钥时 `signed: false` +
    打印补签命令；**不用自签证书冒充正式签名**，**不把 SHA-256 说成签名**。
    Android 未配置 keystore 时如实报告「debug 签名，可侧载不可上架」。

**范围决定：GUI 的策略设置随 M18 接入**

`apps/cping-gui` 里**刻意**不调 `set_policy`，用 `ScanPolicy::default()`：
桌面端用户最不可能去读文档，所以「默认限速 + 拒绝保留网段」必须是无条件的
默认值。界面里可调的限速值与 M16 的模板选择器一起，随 M18 的界面工作接入
（同一块设置区改两遍不值得）。

**验收对照**

| 验收要点 | 落地情况 |
| --- | --- |
| 默认速率上限存在且可配置 | `policy_defaults_are_the_documented_ones` + `m17_policy_defaults_are_limited_and_guarded`（默认 500）；`--max-pps` / TUI 同名参数；`limiter_is_a_no_op_when_unlimited` 反证「关掉才不限速」 |
| 速率真的被限住 | `bucket_never_exceeds_the_rate_over_a_long_run`（整数断言，不靠 sleep）；`bucket_wait_is_rounded_up_never_down`；实测见下 |
| 限速不阻塞取消 | `limiter_can_be_cancelled_while_waiting` |
| 目标数上限 | `check_targets_enforces_count_and_reserved`、`policy_rejects_out_of_range_values_instead_of_clamping`（只能收紧） |
| 保留网段白名单机制 | `multicast_broadcast_and_reserved_are_rejected_with_a_reason`、`reserved_boundaries_do_not_bleed`、`private_and_loopback_are_deliberately_allowed`；`--allow-reserved` 放行有测试 |
| 审计日志可导出、不含敏感内容 | `audit_line_is_single_line_json_with_the_fields_we_promised`（逐字段点名）、`audit_log_refuses_a_file_it_did_not_write`；字段集合里没有 MAC/厂商/主机名 |
| 审计行不会被注入 | `audit_line_escapes_hostile_target_specs`（带引号/反斜杠/换行的目标规格仍产出单行合法 JSON） |
| 首次启动提示 | `consent_needs_notice_until_the_version_is_reached`、`consent_notice_text_says_what_it_must_say`；CLI 实测见下 |
| 文档明确「仅用于获授权网络」 | 三端共用 `consent::notice_text()`；README 与 `docs/signing.md` 同步 |
| 签名不伪造 | `artifact-manifest.sh` 的 `signed` 字段；密钥导入失败**显式报错**且不改成 `true`（实测见下） |
| 平台签名脚本可执行 | `bash -n` 语法检查 + 无密钥分支实跑；Android / Windows 的正式签名分支需要密钥，**未实测**（见下） |

**测试与实测**

- 单元/集成测试：+30（`policy.rs` 20 + CLI 6 + TUI 4），工作区合计
  **460 passed / 0 failed**（CLI 45 → 51，TUI 43 → 47）
- `cargo fmt --check` / `cargo clippy --workspace --all-targets -- -D warnings`
  干净；GUI（独立工程）另跑一遍同样干净（首次因 Windows 文件锁报
  `os error 5`，重试即通过）
- 真实冒烟（Windows 本机）：
  1. **首次运行**打印授权提示并把状态落到 `~/.cping/consent`；**第二次运行不再打印**
  2. **限速实测**：目标 `127.0.0.0/24`（254 个回环地址）
     - `--max-pps 0`（不限速）：**644 ms**
     - `--max-pps 50`：**4708 ms**，与理论值（启动 ~0.6 s + 突发 50 +
       其余 204 个 ÷ 50 pps ≈ 4.08 s）吻合
  3. `-t 224.0.0.0/29` → 退出码 2，提示「组播地址：一个探测会打扰整组主机」
     并给出 `--allow-reserved`；加放行后真的发出（结果如实为离线，无应答）
  4. `-t 192.0.2.5` → 退出码 2，提示「文档示例网段 TEST-NET-1（RFC 5737）」
  5. `--max-targets 3` + 6 个目标 → 退出码 2；`--max-targets 70000` →
     「只能收紧，不能放宽」；`--max-pps 100001` → 「要关闭限速请写 0」
  6. `--audit x.jsonl` + `--watch 1 --rounds 3` → 文件首行 `CPING-AUDIT 1`，
     事件序列 `session_start / round ×3 / session_end`，轮次 0/1/2/3/3，
     `exitCode` 只出现在 `session_end`
  7. 审计路径指向别人的文件 → 退出码 2「不是 CPing 审计文件，拒绝追加」；
     追加到已有审计文件不重写表头、不丢已有记录
  8. `artifact-manifest.sh` 在无密钥时产出 `signed: false` 并打印补签方法；
     故意给一个坏密钥 → **显式报错**且 `signed` 仍为 `false`
  9. 三个新脚本均过 `bash -n`
- **未实测**（如实登记）：
  - gpg **成功**签名路径：本机 gpg 在 Git Bash 下把 `GNUPGHOME` 路径解释错，
    只跑通了「导入失败 → 显式报错 → signed 保持 false」这一支；
    成功分支需在 Linux CI 上验证
  - Windows Authenticode 正式签名（需要代码签名证书）、Android 正式签名
    （需要 keystore）：脚本的无密钥分支可跑，**有密钥分支未实测**
  - `scripts/sign-windows.ps1` 的语法未在真实 PowerShell 里实跑过
  - Linux deb 签名：本仓库当前不产出 deb，`docs/signing.md` 只给流程说明
  - TUI / GUI 的限速人工冒烟：TUI 用单元测试驱动了策略装配与状态栏提示，
    未做真实键盘操作下的限速观察；GUI 沿用默认策略，未做界面观察
