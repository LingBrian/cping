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
| M18 | **v2.6.0** ✅ | 国际化与无障碍 | i18n（中/英）、键盘全导航、屏幕阅读器 |
| M19 | **v2.7.0** ✅ | 规模化性能 | 万级目标、流式结果、内存预算与背压 |
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

**本阶段拆三步**（与 M18 同一套做法：每步一个可独立验收的单元，合并成 v2.7.0）。

#### 先摸清的三件事（动代码前的实测，不是猜测）

1. **三端统计是 O(n²)**。TUI 的 `App::recompute_stats` 与 GUI 的 `ProbeEvent::Result`
   分支都是同一行：`results.iter().flatten().cloned().collect()` 之后整表重算。
   每来一条结果就拍平一次整表——65536 目标要拍平 65536 次、累计克隆约 21 亿条。
   基准实测（见第一步）：8192 目标已经要 **2773 ms**，按二次关系外推到 65536 是 **177 秒**；
   增量累加在同一台机器上是 **0.079 ms**。这不是「优化」，是「不做就跑不动」。
2. **每条结果发两条通道消息**。`Result` 里已经带了完整状态，`Progress` 只多带一个
   `completed` 计数——而基准实测**进度合并比 = 1.0×**，即「合并」根本没合并。
   GUI 收到 `Progress` 直接 `{}` 丢弃（`commands.rs`）；TUI 的 `Progress` 分支在
   `Result` 分支已经重算过统计的前提下再判一次，实际是冗余路径。
3. **事件通道是无界的**（`mpsc::channel`）。生产者快、消费者慢（界面在重绘、
   或统计还在 O(n²)）时，队列会一直涨到 OOM。生产环境里这是最难查的一类崩溃。
   顺带确认了一件**不是**问题的事：`index: u16` 与 `MAX_TARGETS = 65536` 恰好吻合
   （`0..=65535`），不是溢出隐患——但这个上限是硬的，本阶段不提高它。

#### 第一步：增量统计口径与基准工程

- 核心：`ScanStats::set_total` / `push_status` / `for_total`——增量累加，O(1)，
  与既有批量口径（`from_results_with_total` / `from_statuses`）**逐字段相同**，
  由 `incremental_stats_match_the_batch_ones` 守着。
- 三端接线：TUI `recompute_stats`、GUI 的 `Result` 分支改走增量；落盘/报表仍走批量
  （两条路必须对得上，否则「界面显示完成 500、报表里 498」会同时存在且无人知对错）。
- 基准：`crates/cping-core/benches/scan.rs`，`harness = false`（于是**不需要 criterion**，
  核心 crate 的 dev-dependencies 保持为空）；基线 `docs/perf-baseline.txt` + `--check`。
- 验收：增量与批量逐字段相同（测试）；基准能抓到算法性回归（容差 2.5×，
  故意放得很宽——机器差异不会误报，而「增量被改回全量」会差 2 万倍）。

#### 第二步：事件通道背压与 `Progress` 冗余

- `Progress` 从「每条一报」改为**节流上报**（距上次 ≥ 某间隔，或进度跨过 1% 才发）。
  终态（`Finished` / `Cancelled`）**永远带完整统计**，进度事件只是「长时间没有结果、
  但确实在推进」时的兜底——它不再承担「让界面知道完成数」的职责，那由 `Result` 承担。
- 事件通道改**有界**（`sync_channel`，沿用 `notify.rs` 已有的「有界队列 + 丢弃计数」范式）。
  满时策略按重要性分层：**先丢进度，再丢中间态，结果与终态永不丢**。
  结果也满时**阻塞在发送侧**（真背压）——丢结果比卡住更糟。
- 丢弃计数要**可观测**（`dropped_progress` 之类），让「丢了」是能被看见的，不是静默的。
- 验收：65536 目标下进度事件条数远小于结果条数（实测合并比）；结果事件零丢失；
  慢消费者下队列长度有界。

#### 第三步：万级目标端到端与内存预算

- 65536 目标在离线 mock 下真实跑通端到端（不是外推），量内存峰值。
- 内存预算常量 + 断言：单条 `ProbeResult` 的宽度、整表规模、事件队列上限，
  三者相乘要落在预算内（实测值：`sizeof(ProbeResult) = 120 B`、`sizeof(ProbeEvent) = 144 B`、
  65536 整表 = 7.50 MiB）。
- TUI 在 65536 目标下仍可取消（内存后端断言，真实终端观感如实登记为未实测）。
- 验收：内存峰值有上限断言；长任务期间仍可取消；性能数据写入文档。

#### 本阶段定死的边界（不是遗漏）

- **不提高 `MAX_TARGETS`（65536）**：`index: u16` 与它绑定，提高要先加宽类型，
  那是 M20 主版本的事。
- **不引 criterion / 不引任何新依赖**：核心 crate 的「零第三方运行时依赖」是承诺，
  dev-dependencies 也保持为空；基准自己写计时。
- **不做分片真并发扫描**（多进程/多机）：那是 M27 分布式采集的范围。
- **基准数字不写死进代码**：只进 `docs/perf-baseline.txt` 与文档，代码里不嵌性能常量
  （否则换机器就红）。

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

## 路线图扩展：v3.0.0 之后的八个阶段（2026-10-15 规划）

> v1.0.0 之后规划的 M8–M20 通向 **v3.0.0**；本节把路线图再往前推 **8 个阶段
> （M21–M28），通向 v4.0.0**。主题上的转折点在这里：M8–M20 是「把一个单机工具
> 做完整」，M21–M28 是「**把一个工具做成一个平台**」——探测协议、协作、可观测性、
> 分布式采集，最终在 v4.0.0 收敛为「服务端 + 多租户 + 冻结的 API 契约」。

### 全量阶段总览（v3.0.0 之后）

| 阶段 | 版本 | 主题 | 一句话 |
| --- | --- | --- | --- |
| M21 | v3.1.0 | 协议扩展 | SNMP v1/v2c/v3 只读、DNS 解析与校验、逐跳路径探测 |
| M22 | v3.2.0 | 拓扑与可视化 | 拓扑自动成图、机架视图、SVG / PNG 导出 |
| M23 | v3.3.0 | 团队与协作 | 配置档案导入导出、只读分享、变更留痕 |
| M24 | v3.4.0 | 可观测性集成 | Prometheus 指标、Grafana 面板、值班系统载荷 |
| M25 | v3.5.0 | 基线与异常 | 基线学习、EWMA 平滑、偏离基线的异常提示 |
| M26 | v3.6.0 | 移动与远程 | 移动端原生体验、远程只读视图、离线缓存 |
| M27 | v3.7.0 | 分布式采集 | 探针节点、中心聚合、断网续传 |
| M28 | **v4.0.0** | 平台化 | 服务端 + 多租户 + 插件市场 + 冻结的 API v2 契约（主版本） |

### M21 — v3.1.0 协议扩展

新功能：

1. **SNMP 只读探测**：v1 / v2c / v3（**只做只读**，不做 set）。取
   `sysDescr` / `sysUpTime` / `ifTable` 摘要；v3 支持 authPriv，但凭据**只接受
   用户显式配置**，不提供任何内置默认社区串。
2. **DNS 解析与校验**：目标写成主机名时统一走一次解析，并允许把「预期地址」
   一并声明——解析结果与预期不一致时报「地址不符」告警，而不是静默采用。
3. **逐跳路径探测**：traceroute 式 TTL 递增探测，输出每跳地址与往返延迟；
   权限不足（无法构造原始套接字）时**明确报错**，不用 ICMP 近似值顶替。

验收要点：SNMP v3 凭据在默认路径下**不落盘明文**；DNS 不符必须告警不得静默采用；
每一跳都必须能区分「未响应」（`* * *`）与「已到达」，不得把二者画成一样。

### M22 — v3.2.0 拓扑与可视化

新功能：

1. **拓扑自动成图**：按「同网段 / 网关 / 路由跳数」推断层级，生成树状视图；
   推断结果在界面上**显式标注为推断**，且提供一键切回列表。
2. **机架与机房视图**：手工摆放（拖拽）并持久化；自动布局只作为初始建议，
   用户摆放后**不再被重扫覆盖**。
3. **导出**：SVG 矢量导出（可嵌进 M10 的 HTML 报表）与 PNG 位图导出。

验收要点：手工布局与自动布局分开存储；节点数超过阈值时自动降级为列表并提示
（一张画满 4096 个点的图对谁都没用）；导出的 SVG 不引外部资源、可离线打开。

### M23 — v3.3.0 团队与协作

新功能：

1. **配置档案共享**：把「模板 + 策略 + 采集配置」汇出为单文件档案，带格式版本
   与内容校验和；导入时**逐项列出差异并逐项确认**。
2. **只读分享**：把一次扫描结果导出为自包含 HTML（复用 M10 的渲染器），
   附内容校验和，可离线打开与转发。
3. **变更留痕**：谁在什么时候改了哪个模板 / 策略，落本地变更日志
   （与 M17 审计日志同一套 JSONL 格式，但写不同文件、不同格式标识）。

验收要点：导入档案时**安全类字段（限速、目标上限、保留网段）必须显式确认**，
不能被档案静默改写（M17 定下的「策略只走命令行」在这里要有一致的对应规则）；
分享页不含本机路径、账号名、MAC 等隐私。

### M24 — v3.4.0 可观测性集成

新功能：

1. **Prometheus 指标端点**：`/metrics`（**仅回环、默认关闭**），暴露在线数、
   延迟分位（p50/p95/p99）、告警计数、轮次耗时。
2. **Grafana 面板模板**：仓库内提供一份 JSON 面板定义，随版本更新。
3. **值班系统载荷**：在 M15 的通知通道之上补 PagerDuty / Slack / Teams 的
   专用载荷格式（不是新的通道，是同一通道的不同序列化）。

验收要点：指标端点与 M20 的 REST API **共用同一套回环 + 认证 + 开关**，
不许各写一套；新增载荷复用 M15 的冷却与去重，不得另起一套计数；
指标里不含目标 IP 之外的任何隐私字段（与 M17 审计日志同一条纪律）。

### M25 — v3.5.0 基线与异常

新功能：

1. **基线学习**：按「目标 × 时段」统计延迟与在线率的正常区间；冷启动期
   显式标注「基线未成型」，**此期间不产出任何异常结论**。
2. **EWMA 平滑**：抑制单次抖动造成的误报；平滑系数可配置且有上下界。
3. **异常提示**：偏离基线到可配置倍数才提示；提示永远是「偏离基线」这一
   可核对的陈述，而不是「主机有问题」这种结论。

验收要点：基线数据可清除、可导出、可禁用；算法是纯函数且有全边界测试
（空序列、单点、全相同、全不同、含缺测）；关掉基线功能后行为与 M15 完全一致。

### M26 — v3.6.0 移动与远程

新功能：

1. **移动端原生体验**：Android 通知权限与「后台扫描白名单」引导、
   深色模式跟随系统、返回键 / 手势与现有导航一致。
2. **远程只读视图**：连到自建服务端（M28）或本机 API（M20），
   **只读**查看结果与历史，不能在移动端改扫描策略。
3. **离线缓存**：断网时可看最近一次快照，界面显式标注「数据截至 …」。

验收要点：远程视图在协议层就没有写接口（不是靠界面把按钮藏起来）；
离线数据的陈旧程度必须可见，不得把旧数据当实时数据展示。

### M27 — v3.7.0 分布式采集

新功能：

1. **探针节点**：一个极小的采集进程（复用 `cping-core`），按中心下发的
   任务扫描并回报；探针**只连出站**，不需要被公网可达。
2. **中心聚合**：多探针结果合并，每条结果标注来源探针；同一目标的多个
   探针结论**并列展示**，不互相覆盖。
3. **断网续传**：本地有界队列 + 恢复后按序补报；队列溢出必须**可见**
   （丢了多少条要能答出来），不得静默丢弃。

验收要点：探针与中心之间是**探针主动拉任务、推结果**（不要求入站端口）；
凭据是一次性令牌且可吊销；聚合结果不得掩盖探针之间的分歧
（两个探针一个说在线一个说离线，界面必须同时显示两条）。

### M28 — v4.0.0 平台化（主版本）

新功能：

1. **服务端**：集中式目标清单、调度计划与结果归档；提供自托管部署方式与
   最小权限的默认配置。
2. **多租户与权限**：组织 / 团队 / 只读成员三类角色；跨租户数据隔离
   **有测试保证**，不是靠查询条件「记得加 where」。
3. **插件市场**：M20 的插件 SDK 之上补「发现与安装」；本地目录优先，
   远端仓库**可选且需显式信任**，默认不联网取插件。
4. **冻结的 API v2 契约**：REST / WS 契约冻结、版本协商、弃用策略，
   随仓库附 OpenAPI 描述文件与兼容性测试。

验收要点：3.x → 4.0 的破坏性变更逐条列出并附迁移说明；API 契约有兼容性测试
（旧客户端对新增字段必须无感）；**桌面端与服务端能各自完全离线运行**——
服务端是增强项而不是必需依赖，这条是 v4.0.0 的底线。

### 里程碑原则（v3.0.0 之后的追加两条）

前四条与上一节相同（先写计划再动代码 / 实测与未实测分开 / 破坏性变更升主版本并
写迁移说明 / 核心逻辑只在 `cping-core`），另追加两条：

- **推断类功能必须自证是推断**：「拓扑层级」「设备类型」「异常」「基线」一律
  显式标注来源与置信度，并必须提供关掉它的开关。平台化最容易犯的错，就是把
  猜测当成事实展示，然后被当成事实使用。
- **新增监听默认关闭 + 仅回环 + 独立开关**：API、指标端点、探针入站口一律如此，
  文档必须写清默认值。**默认安全优先于默认方便**（这条是 M17 纪律的延续）。

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

### M18 — v2.6.0 国际化与无障碍（第一步实施记录，2026-10-15）

> 本阶段按「机制 → 界面 → 无障碍」三步走。**本步只做完了 i18n 的机制与三端
> 界面自有文案**；核心层自由文本、GUI 的后端文案、无障碍与键盘导航
> 见文末「本步未做」。

**实现文件**

- `crates/cping-core/src/i18n.rs`（新增，约 900 行含 14 条测试）
  - `Lang { Zh, En }`：`parse`（`zh_CN.UTF-8` / `en-US` 都认；`C` / `POSIX` /
    空串返回 `None`）、`from_env()`、`code()` / `label()` / `describe()`
  - `Key`（83 键）+ `TABLE` + `ordinal` + `text` + `render_template` / `render`
  - `detect_from_values()`：`from_env` 的**纯函数内核**。拆出来是为了能测——
    直接读写进程环境变量的测试会互相污染（cargo test 默认多线程），
    而「优先级顺序」恰恰是最容易写错的逻辑
- `crates/cping-core/src/error.rs`：`CoreError::code()` + `localized(lang)`；
  `ProbeError::localized(lang)`；`Display` 改为 `localized(Lang::Zh)` 的转发
- `crates/cping-core/src/policy.rs`：
  - `ReservedKind`（10 个类别）+ `reserved_kind()` + `reason(lang)`；
    `reserved_reason()` 保留为中文快捷方式
  - `ScanPolicy::validate_in` / `check_targets_in` / `describe_in`
  - `consent::notice_text_in` / `notice_lines_in`
- 14 个枚举加 `label_in(lang)`：`alerts`（`AlertKind` / `Severity`）、
  `classify`（`LatencyClass` / `ResultClass`）、`diff`（`DiffKind`）、
  `fingerprint`（`Confidence`）、`monitor`（`ChangeKind`）、
  `policy`（`AuditEvent`）、`registry`（`BackendId`，以及
  `BackendRegistry::summary_in`）、`result`（`ProbeStatus`）、
  `target`（`TargetRange`）
- `apps/cping-cli/src/i18n.rs`（新增，148 键）+ `help_zh.txt` / `help_en.txt`
  （帮助正文用 `include_str!` 编译期内联）+ `args.rs` / `main.rs` 接线
- `apps/cping-tui/src/i18n.rs`（新增，225 键）+ `app.rs` / `ui.rs` / `main.rs` 接线
- `apps/cping-gui/src/i18n/{index,zh,en}.ts`（**241 键**）+
  `components/LanguageSettings.vue` + `scripts/check-i18n.mjs`

**定死的边界（下阶段别推翻）**

1. **核心层产出键，界面产出句子**。核心层只给「哪一个键、哪些参数」，
   渲染在界面侧。否则三端只能各自复刻一遍核心层的判定逻辑才知道该说什么，
   那就是三份会各自漂移的实现。
2. **键是枚举不是字符串**。字符串键写错了只会在运行期暴露，而且表现为
   「界面上显示成键名」而不是崩溃——这种错没人会当成 bug 来报。
3. **「翻译好不好」测不了，「漏译」能测**。四条测试把最常犯的错变成红灯：
   `english_has_no_cjk`（英文条目含中日韩字符）、`placeholders_match`
   （中英 `{}` 个数不等）、`table_covers_every_key`（字典漏键）、
   `enum_labels_are_localized_both_ways`（从枚举一路走到渲染结果，
   抓「映射到了错的条目」）。其中 `chinese_entries_are_not_empty_and_not_ascii_only`
   抓的是反方向——把英文粘到了中文栏上。
4. **中文只有一个来源**。`label_zh()` 变成 `label_in(Lang::Zh)` 的转发，
   不保留原先各枚举里的中文 `match`——两份中文迟早会漂移，而漂移了没人会发现。
5. **错误码与错误句子分开**。`code()` 给脚本与查表用（**不要靠匹配消息文本
   判断错误类型**，那句话随时会因为改措辞而变化），`localized(lang)` 给人读。
6. **`Display` 就是 `localized(Lang::Zh)`**，不是另写一份。两处各自 `match`
   的结果是改了其中一处、另一处悄悄留在旧措辞上。
7. **底层细节原样透传**。`Export("No such file (os error 2)")` 里的 `detail`
   来自操作系统：本地化的只是外面那层框。翻了反而让用户搜不到。
8. **数据契约不翻译**。CSV / JSON 的列名与字段名、快照与时序的文件格式、
   审计字段名一律保持原样——同一份报表在中英文机器上必须一致，
   否则下游脚本会「在同事的机器上」失效。TUI / GUI 的状态符号
   （`#` `+` `*` `-` `!` `.`）同理：符号不是文字。
9. **`C` / `POSIX` 不是英文**。它表示「没有本地化偏好」，
   当成英文等于让服务器的默认 locale 决定用户看到什么。
10. **`--lang` 非法值报错**，不静默忽略。TUI 是唯一的例外（交互式程序，
    启动后还能在状态栏看到告警并修正），沿用该程序既有的
    「参数告警不退出」惯例。
11. **连分隔符也跟着语言走**。`BackendRegistry::summary_in` 中文用全角分号、
    英文用 `"; "`——把中文标点留在英文句子里，读起来是「一眼假」的机器翻译味。

**范围决定：本步做到哪**

**做完的**：i18n 的全部机制 + 核心层「枚举标签」类文案 + 三端**界面自有文案**。

**刻意没做的**（不是遗漏，是边界）：

- **核心层「自由文本」类文案仍未本地化**：`TargetSpec::summary()`、
  `Change::summary()`、`AlertEvent::message`、`HostFingerprint` 的端口 / 设备摘要、
  `BackendCaps::note`、`notifications_available()` 的说明、
  `ScanProfile::describe()`、`native_icmp_hint()`、`ConfigDocument` 的 notes、
  `TimeSeries` / `AlertEngine` 的读写错误、`PortScanConfig::parse_ports` 的错误文本。
  它们要么参数多、要么嵌在 `Result<_, String>` 里，**改造面比标签大一个量级**，
  且需要先定下「参数化模板」的形态，留到本阶段第二步。
- **GUI 的后端文案因此仍是中文**：GUI 的「状态」列、「资产」摘要、趋势与告警
  面板里那些**由 Rust 侧生成的**文字，走的就是上面那批自由文本。
  第二步要把 `lang` 一路传进 Tauri 命令，并让核心层按语言产出。
- **CLI / TUI 各自的「键 → 展示名」桥接函数尚未收敛到 `label_in`**：
  两端的桥接目前**输出与核心层一致**（都过了各自的测试），但翻译存了三份。
  收敛需要删除字典条目并重排 `ordinal`（CLI 148 键 / TUI 225 键），
  是纯机械但高风险的改动，与第二步一起做。
  TUI 的 `status_label` 还多一段 TUI 特有的 `TCP-OPEN` 显示逻辑，
  收敛时要把它一并考虑（该逻辑或许应当下沉到核心层）。
- **无障碍与键盘全导航**：本阶段第三步（高对比主题、字体缩放、ARIA 标注、
  色盲友好备选配色、GUI 纯键盘可达）。

**测试与实测**

- 测试 **460 → 507 passed / 0 failed**（核心 266 → 291，CLI 51 → 62，TUI 47 → 58）
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -- -D warnings`
  / GUI `npm run typecheck` / `npm run build` 均干净
- GUI 词典校验：`node scripts/check-i18n.mjs` →「zh/en 各 241 条词条，
  键一致、无中文残留、占位符个数一致」
- **真实冒烟（Windows 本机，debug 构建的真实二进制）**：
  1. `cping-cli --lang en --help` → 全文**零中日韩字符**，标题为
     「CPing headless command line -- batch PING with exportable results
     and scriptable exit codes」
  2. `cping-cli --lang zh --help` → 中文原文
  3. `cping-cli --lang en -t 224.0.0.0/29` → 退出码 2，stderr 为
     「argument error: target 224.0.0.1 is in a reserved range: multicast
     address: one probe disturbs the entire group」+「hint: add
     --allow-reserved if you really mean to scan this; such addresses are
     either meaningless or disturb others.」
  4. `cping-cli --lang zh -t 224.0.0.0/29` → 同一判定、中文提示、退出码 2
  5. `cping-cli --lang fr` → 退出码 2，报「--lang value "fr" is invalid;
     expected zh or en」并打印帮助（**没有静默回退成某种语言**）
  6. `cping-tui --lang en --help` → 零中日韩字符；
     `cping-tui --lang zh --help` → 中文

**未实测（如实登记）**

- 三端在**非中文 Windows / 纯英文 Linux** 上的默认语言推断
  （本机测的是显式 `--lang` 与 Git Bash 的环境变量）
- GUI 的语言切换与 `localStorage` 持久化**未在真机 / 浏览器里点过**，
  只跑了 typecheck + build + 词典校验脚本
- TUI 在英文模式下的**实际终端观感**（列宽、对齐）未人工确认，
  只由单元测试覆盖数据路径
- GUI 的 241 键之外，仍有「后端生成的中文」会露在英文界面下
  （见上「范围决定」）——**这是本步已知的、有意的未完成项**，
  不是漏测

### M18 — v2.6.0 国际化与无障碍（第二步实施记录，2026-10-15）

> 本步把第一步「刻意没做」的两件事做了：**核心层的自由文本本地化**、
> **GUI 后端文案的语言贯通**；顺带把第一步欠下的**三端桥接收敛**做掉，
> 并补上 M16 / M17 一直没接的界面。无障碍与键盘全导航仍是第三步。
>
> 版本号**仍然停在 2.5.0**：M18 三步合成一个 v2.6.0，第三步（无障碍）
> 落地时才升版本号。这是与第一步一致的取舍，不是漏改。

**实现文件**

- `crates/cping-core/src/i18n.rs`：键 **83 → 151**（新增 68 条自由文本键）；
  新增 `ASCII_TOKEN_KEYS` 白名单 + `table_order_matches_ordinal`；
  新增总闸测试 `free_text_is_localized_both_ways`
- `crates/cping-core/src/spec.rs`：`TargetSpec::summary_in`、`Segment::describe_in`、
  `ProbeMode::status_label_in` / `validate_in`
- `crates/cping-core/src/monitor.rs`：`Change::summary_in`
- `crates/cping-core/src/diff.rs`：`ScanDiff::summary_in`
- `crates/cping-core/src/alerts.rs`：`match_rule_in`、`AlertEngine::evaluate_in`、
  `AlertEngine::from_text_in` / `save_to_path_in` / `load_from_path_in`
- `crates/cping-core/src/fingerprint.rs`：`PortScanConfig::parse_ports_in`；
  **`DeviceGuess.kind` 由 `&'static str` 改为枚举 `DeviceKind`（8 变体）**；
  **`DeviceGuess.basis` 由 `Vec<String>` 改为 `Vec<BasisSignal>`**
  （`Vendor` / `Ports` 两种信号）；`DeviceGuess::summary_in` / `basis_text_in`、
  `HostFingerprint::device_summary_in`
- `crates/cping-core/src/registry.rs`：**`BackendCaps.note` 由 `&'static str`
  改为枚举 `CapsNote`**（5 变体，含 `None`）；`BackendCaps::describe_in`
- `crates/cping-core/src/profile.rs`：`ScanProfile::describe_in`
- `crates/cping-core/src/native.rs`：`native_icmp_hint_in`（平台分支改用 `cfg!`
  而不是 `#[cfg]`，四种分支都编译进二进制，开发机上也能测到非本平台那一句）
- `crates/cping-core/src/notify.rs`：`notifications_available_in`
- `crates/cping-core/src/timeseries.rs`：`TimeSeries::from_text_in`
- `crates/cping-core/src/export.rs`：设备指数字段改用 `kind.label_zh()` /
  `basis.render_zh()`——**导出固定中文**，见「定死的边界」第 5 条
- `apps/cping-cli/src/main.rs` / `args.rs`：接通上述全部 `_in`；
  `--notify-list`、`--fp-ports`、回环扫描表格的状态名与设备摘要
- `apps/cping-tui/src/app.rs` / `ui.rs` / `i18n.rs` / `input.rs`：
  **M16 模板选择面板（`P`）**、**M17 策略面板（`G`）**、
  字典 **225 → 217 键**（删掉 35 个纯转发的桥接条目、新增 27 个面板键）、
  `--config` 参数、指纹面板改用 `device_summary_in`
- `apps/cping-gui/src-tauri/src/i18n.rs`（新增，**37 键**）、`commands.rs`
  （命令收 `lang`、改用核心 `_in`）、`lib.rs`
- `apps/cping-gui/src/services/ping.ts`（`invokeT` 注入语言，33 处 typed +
  3 处 bare invoke）、`src/stores/ping.ts`（`refreshLocalizedViews`）、
  `src/components/LanguageSettings.vue`

**定死的边界（下阶段别推翻）**

1. **只有散文进字典，标点不进**。`{} → {} ms`、` · `、`{}({})` 这种纯符号模板
   留在代码里：`→` 在两种语言里都是 `→`。把它们塞进字典，字典会被一批
   「中英一字不差」的条目撑爆，真正需要复核的条目反而被淹掉。
2. **技术记号要有白名单，而不是放宽检查**。`TCP-OPEN`、`IPv4+IPv6` 两种语言
   下相同才是对的。「中文条目必须含中日韩字符」那条检查抓的正是「把英文粘到
   中文栏」，**放宽它就等于放弃这条检查**。所以给记号一个显式白名单，
   并用 `ascii_token_allowlist_is_exact` 保证它不会腐烂。
3. **枚举化优先于加一个 `xxx_en: &'static str`**。并列字段迟早漂移：
   改了 `kind` 忘了改 `kind_en`，编译不会报错。`DeviceKind` / `CapsNote`
   换掉 `&'static str` 之后，漏翻是**编译期错误**。
4. **结构化信号优先于存句子**。`DeviceGuess.basis` 从中文句子数组改成
   `Vec<BasisSignal>`：信号是数据，渲染成哪种语言是界面的事。
   存句子的写法让同一份依据在导出、GUI、TUI 三处各被复制一遍，且都没法翻。
5. **导出固定中文，界面跟着语言走**。CSV / JSON 列名与字段名、报表里的
   目标摘要、审计记录里的 `targetSpec` 一律中文——**数据契约**，
   同一份报表在中英文机器上必须逐字节可比。`lang` 只影响界面。
   这一条在 `export.rs` 里有注释写明，避免下一个人「顺手也翻了」。
6. **用户自己写的数据不翻译**。模板的 `note`、快照里的主机名、厂商名、
   banner 一律原样。自由文本双语测试里的样本刻意用 ASCII 的 `note`，
   就是为了让这条测试检查「**我们写的词**有没有翻」，
   而不是在检查「用户写什么语言」。
7. **`TABLE` 顺序即 `ordinal` 顺序**。`text()` 是 `TABLE[ordinal(key)]`，
   按下标取值。错位的表现不是崩溃而是**所有消息变成邻居那一条**
   （`ICMP 需要权限`显示成`厂商 {}`）。第一步的 `table_covers_every_key`
   查不出这个——它只看序号集合是不是一个排列。三端字典各加一条
   `table_order_matches_ordinal` 补上。
8. **判定与措辞严格分开**。`match_rule_in` / `evaluate_in` 只改 `message`，
   冷却、静音、基线推进、阈值比较一字不改。措辞是最常改的东西；
   判定一旦跟着改，「上周还能报出来的告警这周不报了」没人查得出来。
9. **告警正文跟语言走不影响落盘**。告警状态文件里不存 `message`
   （只存 `kind` / `ip` / 时间戳），所以 `evaluate_in(lang)` 是安全的。
   **改这一处前必须确认落盘字段没变**——这是「语言只影响界面」的边界情形。
10. **平台分支用 `cfg!` 不用 `#[cfg]`**。`native_icmp_hint_in` 四种平台的句子
    都编译进二进制：`#[cfg]` 写法下非本平台的分支根本编不进来，
    漏翻要等发布到那个平台才发现。
11. **GUI 命令的 `lang` 是 `Option<String>`**。旧前端不发这个字段时退化为
    跟随环境变量，而不是报错——IPC 契约只增不改。
12. **TUI 的模板合并优先级必须与 CLI 一致**：命令行显式参数 > 模板 > `[probe]`。
    模板只填空缺，**绝不覆盖用户显式给的值**；命令行压住模板字段时
    仍记录「正在用的模板」（其余字段照常生效）。

**范围决定：本步做到哪**

**做完的**：核心层自由文本（含告警正文、设备推断、后端能力、平台提示、
通知通道、解析错误、落盘错误）；三端全部接通；三端桥接收敛；
M16 模板选择器与 M17 策略面板接入 TUI；GUI 命令层语言贯通；
路线图延伸至 v4.0.0（M21–M28，见本文件「路线图扩展：v3.0.0 之后的八个阶段」）。

**刻意没做的**（不是遗漏，是边界）：

- **无障碍与键盘全导航**：本阶段第三步（高对比主题、字体缩放、ARIA 标注、
  色盲友好备选配色、GUI 纯键盘可达）。**这是本步已知的、有意的未完成项。**
- **GUI 没有真机点击过**：本机没有可用的桌面会话，Tauri 窗口一个都没起过；
  只跑了 `cargo test`（10 条）+ `npm run typecheck` + `npm run build` +
  词典校验脚本。切换语言的实际观感、`invokeT` 多传 `lang` 在真机上的行为
  都靠静态推断（已核对 tauri 2.12.3 的 `CommandItem::deserialize_option`
  在缺键时走 `visit_none()`、多余键被忽略）。
- **`AppConfig::validate()` 的内层细节仍是中文**（外层「配置无效：…」已翻）。
  核心没有 `validate_in`，要改得连带 `ProbeConfig` / `ColorConfig` 一起动，
  与本步的「自由文本」范围不同，登记为后续项。
- **TUI 的模板只能「选」不能「增删改」**：模板在配置文件里定义。
  这是刻意的边界（界面增删改需要一套表单与冲突处理，属于另一个阶段）。
- **TUI 没有 `--profile`**：与 CLI 口径一致，启动恒走 `default_profile`。
- **M16 / M17 的 GUI 界面仍未接**：TUI 已接；GUI 的设置页要动 Vue 组件与
  Tauri 命令两侧，与本步的 i18n 改动耦合，留到无障碍那一步一起做。

**测试与实测**

- 测试 **507 → 525 passed / 0 failed**（工作区）+ GUI Rust 侧 **0 → 10 passed**，
  合计 **535**。其中核心层 291 → 294（新增自由文本双语总闸、TABLE 顺序）、
  CLI 62、TUI 58 → 73（新增 15 条：模板加载与合并优先级、策略校验与不夹取、
  面板行为）、GUI 10
- `cargo fmt --all -- --check` / `cargo clippy --workspace --all-targets -D warnings`
  / GUI 独立工程 clippy / `npm run typecheck` / `node scripts/check-i18n.mjs`
  / `npm run build` 全部干净
- **真实冒烟（Windows 本机，debug 构建的真实二进制）**：
  1. `cping-cli --lang en --list-backends` → 后端表**全英文**，
     包括此前一直是中文的 `note` 列
     （`ICMPv6 is not implemented yet; probe IPv6 targets over TCP instead`）
  2. `cping-cli --lang zh --list-backends` → 中文对照，同一份能力判定
  3. `cping-cli --lang en --notify-list` → 六个通道的说明**全英文**
     （修复前仍是中文）
  4. `cping-cli --lang en -t 127.0.0.1 --fingerprint --fp-ports 70000`
     → `port "70000" is out of range 1-65535`；`443-445x`
     → `port range end "445x" is not a number`（修复前仍是中文）
  5. `cping-cli --lang en -t 192.0.2.5` → `documentation range TEST-NET-1
     (RFC 5737); never routed on the public Internet`
  6. `cping-cli --lang en -t 127.0.0.1,127.0.0.2,127.0.0.3,127.0.0.4 -c 4
     --timeout 300 --fingerprint --fp-ports 80,443` →
     `round 1 · ICMP · 4 segments / 4 targets`、状态列 `online`、
     设备推断 `web service (inferred, confidence: low; basis:
     TCP 80(http) open; TCP 443(https) open)`；
     **整段输出含中日韩字符的行数 = 0**
  7. 同上 `--lang zh` → 中文对照，依据用顿号分隔（`开放、`）而非英文分号
  8. `cping-cli --lang en --config <含模板的 toml> --list-profiles`
     → 模板表头与字段全英文；剩下的 2 处中文是**用户写的 `note`**
     （`办公网段` / `DMZ，走 SMB 探测`）——正是「用户数据不翻译」的预期行为

**未实测（如实登记）**

- **GUI 未真机点击**（见上「刻意没做的」第 2 条）
- **TUI 的两个新面板未在真实终端手工验证**：渲染只由 `TestBackend`
  内存后端的单元测试断言文本数据路径；实际终端里的排版、颜色、
  宽字符对齐、面板遮挡关系没有人工确认
- **TUI 模板与策略面板的键盘交互**只由单元测试覆盖（按键映射 + 状态变化），
  没有真人在终端里敲过 `P` / `G`
- **非中文 Windows / 纯英文 Linux 上的默认语言推断仍未实测**
  （本机测的是显式 `--lang` 与 Git Bash 的环境变量）

### M18 — v2.6.0 国际化与无障碍（第三步实施记录，2026-10-15）

> 本步把 M18 的最后一块做完：**无障碍与键盘全导航**（高对比主题、字体缩放、
> ARIA 标注、色盲友好备选配色、GUI 纯键盘可达），并补上第二步欠下的
> **M16 / M17 的 GUI 界面**。M18 三步到此收口，版本号升到 **v2.6.0**。
>
> 这一步最值钱的地方不是「加了三套颜色」，而是**把配色变成了能算出对错的性质**：
> 对比度用 WCAG 公式算、色觉可区分度用色觉模拟后的色距算，
> 于是「配色被改坏了」会在测试里失败，而不是等色盲用户来报告。

**实现文件**

- `crates/cping-core/src/a11y.rs`（新增，约 900 行含 19 条测试）：
  `Rgb` / `relative_luminance` / `contrast_ratio`（WCAG 2.x，**先线性化 sRGB**）、
  `ColorVision`（4 种）+ `simulate`（Viénot/Brettel 矩阵）+ `color_distance`（redmean）、
  `Theme`（3 套）+ `Palette` + `Palette::audit` + `A11yIssue`、
  `ensure_contrast`、`FontScale`（3 档）+ `MIN_TARGET_PX`、`A11ySettings` + `flag_is_on`
- `crates/cping-core/src/classify.rs`：`ResultClass::symbol()` —— 符号的唯一来源
- `crates/cping-core/src/html.rs`：类别色改走 CSS 变量（浅底 / 深底两套）、
  `REPORT_BG_LIGHT` / `REPORT_BG_DARK` + `EXPORT_CONTRAST_TARGET`（8% 余量）、
  统计卡片与类别列改用类名
- `crates/cping-core/src/i18n.rs`：键 **151 → 163**（主题名 / 字号档 / 视觉类型 / 两条审计问题）
- `crates/cping-core/src/lib.rs`：新增模块与 re-export
- `apps/cping-tui/src/app.rs` / `ui.rs` / `i18n.rs`：`Options` 加 `theme` / `font_scale`；
  新增 `ui::Look`（**颜色的唯一出口**）；全部硬编码 `Color::*` 收敛；
  符号与「未探测」字符各归其位；字典 217 → 219
- `apps/cping-gui/src-tauri/`：新增 `a11y_theme` / `set_font_scale`（+ `ThemeDto`）、
  M16/M17 的 `list_profiles` / `apply_profile` / `scan_policy` / `set_scan_policy`；
  Rust 侧字典 37 → 45 键
- `apps/cping-gui/src/`：新增 `A11ySettings.vue` / `ProfileSettings.vue` /
  `PolicySettings.vue`；`styles.css`（语义令牌 + `:focus-visible` + 减少动效）；
  12 个组件的 ARIA 与键盘可达性；i18n 字典 241 → 291 键
- `scripts/check-symbols.mjs`（新增）+ `.github/workflows/ci.yml`（接进 portability job）

**定死的边界（下阶段别推翻）**

1. **三套主题的目标不同，不是「同一套颜色的三档亮度」**：`default` 保持原版视觉
   （兼容性优先）；`high-contrast` 对比度优先（**≥ 7:1**，AAA 正文）；
   `color-blind` 用 Okabe-Ito 配色，**色觉可区分优先**（色距 ≥ 60）。
   把三个目标混成一个，结果一定是两头都不突出。
2. **高对比 ≠ 色盲友好，这是实测结论不是感觉**：传统高对比配色
   （纯绿 `#00FF00` + 纯黄 `#FFFF00`）在红色盲下几乎同色——实测色距 **37.9**，
   而阈值是 60。所以高对比主题**不承诺**色觉可区分，并把这条写成断言
   （`high_contrast_does_not_imply_color_blind_safe`，它同时守住「默认主题的
   类别色没有被顺手优化」）。
3. **「无色」去掉的是颜色，不是字形属性**。加粗 / 反显 / 弱化必须保留——
   无色之后「焦点在哪」只能靠反显、「这是次要信息」只能靠弱化。
   TUI 的输入区焦点从「黄底黑字」改成**反显**，正是这个道理。
4. **`NO_COLOR` 是「存在且非空」**（no-color.org）。空串不算。
5. **显式选了无障碍主题时，主题色优先于用户自定义色**：那两个主题的价值就在
   「达标」，允许用户配置覆盖等于把无障碍开关变成摆设。
6. **符号只有一个来源**（核心 `ResultClass::symbol()`），由
   `scripts/check-symbols.mjs` 在 CI 里守住。报表里 `low` 用中点 `·` 是
   **登记过的例外**（打印观感），例外之外任何差异都失败。
7. **导出侧给对比度留余量**（8%）：`ensure_contrast` 返回「第一个达标的点」，
   不留余量会贴着 4.5 过线（实测 4.54 / 4.56 / 4.57），
   四舍五入或换渲染引擎就可能掉下去。
8. **矩阵格不进 Tab 序**（`ResultGrid`）：255 个停靠点会让键盘用户寸步难行。
   格子没有点击行为，纯展示——这是**有意不给**，不是漏了。
9. **GUI 的主题色由核心给出**并注入 CSS 变量，**前端不做任何颜色运算**：
   否则就是第四份配色，迟早与核心漂移。
10. **模板「只填空缺」在三端口径一致**：GUI 把「我已填哪些」交给后端，
    合并只在一处判定；被模板顶掉的字段必须在界面上说明来源。

**范围决定：本步做到哪**

**做完的**：核心 a11y（对比度与色觉达标都是可测性质）；三套主题在三端落地；
字体缩放；无色模式全覆盖；键盘可达与 ARIA；M16/M17 接入 GUI；
符号与配色收敛到单一来源；报表可读性缺陷修复；静态守卫接进 CI；版本升 v2.6.0。

**刻意没做的**（不是遗漏，是边界）：

- **TUI 的主题与无色没有真人在终端里看过**：渲染只由 `TestBackend` 内存后端
  断言文本与样式数据，实际终端里的观感（颜色实际呈现、宽字符对齐）未人工确认。
- **GUI 没有真机点击过**：本机没有可用的桌面会话。`set_zoom` 的成功路径
  只做到「编译期签名核对（`tauri-2.12.3` 源码）+ 失败路径 Err 透传」。
- **字号与主题不落盘**：字号是窗口的 zoom 状态，前端没有反读接口，
  持久化会造出「界面显示标准、实际 150%」这种自相矛盾的状态。
- **组件内的中性灰没有全量令牌化**：只令牌化**主要语义色**
  （背景 / 文字 / 边框 / 面板 / 弱化 / 强调 / 次要底色）。
  局部徽章配色保留原值，理由是它们在中性灰上本来就够读，
  全量替换会铺开几十处 diff 且与「默认主题视觉完全一致」直接冲突。
  边界写在 `styles.css` 里。
- **naive-ui 抽屉关闭按钮的 `aria-label` 未本地化**（库内置英文）。
  它至少有标注，不属于「只有符号没有标注」。
- **`AppConfig::validate()` 的内层细节仍是中文**（第二步登记过，本步未动）。

**测试与实测**

- 测试：工作区 **525 → 552 passed / 0 failed**（核心 +22 = a11y 19 + 报表 3，
  TUI +5）；GUI 独立工程 **25 → 30 passed**；合计 **582**
- `cargo fmt --check` / 工作区 `clippy -D warnings` / GUI 独立工程 clippy /
  `npm run typecheck` / `node scripts/check-i18n.mjs`（291 键）/ `npm run build`
  全部干净
- **静态守卫的负向测试**：往 `CLASS_SYMBOLS` 注入一个错符号 →
  `check-symbols.mjs` 以 1 退出并指出差异；还原后通过
- **真实冒烟（Windows 本机，debug 二进制）**：
  1. `cping-tui --help` → 列出 `--theme` / `--font-scale`（本机 shell 环境推断为英文）
  2. `env -u LANG -u LC_MESSAGES -u LC_CTYPE LC_ALL=C cping-tui --help` → **中文**，
     即「`C` / `POSIX` 是『无偏好』而不是英文」这条边界**在真实环境里成立**
     （第一步时它登记为未实测）
  3. `cping-tui --lang zh --help` → 中文对照
  4. `cping-cli -t 127.0.0.1,127.0.0.2,127.0.0.3 -c 3 --out x.html` → 导出成功
  5. **对导出的报表做真实计算**（算出来的，不是看出来的）：无内联
     `style="color:`；浅色纸面 `#00FF00 → #008200`（4.69）、
     `#FFFF00 → #757500`（4.58）；深色纸面保持原色（12.95 / 16.55）；
     十项全部 ≥ 4.5

**未实测（如实登记）**

- TUI 三套主题与无色模式的**真实终端观感**（只由内存后端断言数据路径）
- TUI 的 `--theme` / `--font-scale` 在真终端里的敲击体验
- **GUI 真机点击**：主题切换、字号缩放、ARIA 在屏幕阅读器下的实际朗读效果
- `set_zoom` 的**成功路径**（签名已核对，本机无桌面会话，没真跑过改字号）
- 非中文 Windows / 纯英文 Linux 上的默认语言推断
  （本机测的是 Git Bash 默认环境与 `LC_ALL=C` 两种）

### M19 — v2.7.0 规模化性能（第一步实施记录，2026-10-15）

#### 做了什么

- 核心 `ScanStats` 增加增量口径：`set_total` / `push_status` / `for_total`。
  口径必须与批量路径（`from_results_with_total` / `from_statuses`）**逐字段相同**。
- 三端接线：TUI 的 `App::recompute_stats` **删除**，改在 `ProbeEvent::Result`
  分支按条累加；GUI 的 `Result` 分支同样改为 `push_status`。落盘与报表仍走批量口径。
- 基准工程 `crates/cping-core/benches/scan.rs`（`harness = false`，不引 criterion）
  + 基线 `docs/perf-baseline.txt` + `--write-baseline` / `--check`。
- CI 新增 `perf` job：只跑基准里**机器无关的结构性断言**，不加 `--check`
  （`--check` 是拿本机基线比绝对值，换机器会误报，只作本地工具）。

#### 实测数字（Windows 本机，bench profile）

| 目标数 | 批量重算（旧写法） | 增量累加（本步） | 加速比 |
| --- | --- | --- | --- |
| 1024 | 17.03 ms | 0.004 ms | 4367× |
| 2048 | 101.89 ms | 0.008 ms | 12737× |
| 4096 | 318.85 ms | 0.017 ms | 18538× |
| 8192 | 1276.17 ms | 0.036 ms | 35548× |
| 65536 | 81675 ms（按 O(n²) 外推，**非实测**） | 0.036 ms | 2275061× |

增量那一列是**取 9 次测量的最小值**（单次微秒级路径的噪声大于信号，见「踩坑」）。

调度器（10000 目标 / 32 并发 / 1 s 超时）：

- **不限速**：5.7–6.6 ms，约 150–174 万目标/秒；结果事件 10000 条**一条不少**；
  堆峰值 3.22 MiB
- **默认策略（500 pps）**：19.0 s，实测/理论 **0.95×** ——
  这 19 秒是**限速的时长，不是调度器的能力**。两个数都记，输出里写明哪个是哪个
- **65536 目标 + 300 ms 后取消**：取消延迟 3.2–4.3 ms，取消前完成 650 条，
  终态 `Cancelled`。这是「限速与背压都不能变成取消不掉的等待」的回归基线

内存（`size_of`）：`ProbeResult` **120 B**、`Option<ProbeResult>` **也是 120 B**
（niche 优化，`Option` 不额外占宽——所以 M19 第三要省的不是 `Option`，
而是**克隆**与**整表拍平**）、`ProbeEvent` 144 B、65536 整表 7.50 MiB。

#### 过程中最值得记的一条：`Progress` 不可信

写 TUI 接线时顺手加了一句不变式断言「本地完成数 ≥ `Progress.completed`」，
16 个测试立刻红灯，且**恰好差 1**。隔离到核心层复现后拿到硬结论：

1. **结果事件一条不少**：5/5 轮 255 条全到，`Started` / 终态各一条。
2. **`Progress` 会领先于它自己的 `Result`**：`Result` 与 `Progress` 由多个 worker
   线程并发入队，虽然同一个 worker 是先发 `Result` 再发 `Progress`，但跨线程的
   入队顺序不保证。实测超前 **0–42 次、最多超前 3 条**。
3. **`Progress` 尾部会丢**：通道按「块」缓冲，最后一个未填满的块要等填满（或发送端
   析构）才可见；而终态是**协调线程**用另一个 `Sender` 发的，可能先被取走。
   结果是收到的最后一条 `Progress` 可能明显小于目标数（实测 **232 / 255**）。

所以那句不变式是**错的**——删掉，改成注释 + 一条常驻测试
（`result_events_are_complete_but_progress_is_neither_ordered_nor_complete`：
只断言完整性，**刻意不碰顺序**，因为断言「超前几次」会随机红灯）。
这条结论直接决定了 M19 第二步的做法：**进度的权威来源只能是 `Result`**，
`Progress` 降级成粗粒度心跳。

#### 踩坑

- **基准路径**：`BASELINE_PATH = "docs/perf-baseline.txt"` 写不住——`cargo bench`
  会把基准进程的工作目录设成**包目录**，实际落到 `crates/cping-core/docs/`
  （不存在），报 `os error 3 系统找不到指定的路径`。改用编译期
  `CARGO_MANIFEST_DIR` 往上退两级。
- **基线精度**：`{:.1}` 会把微秒级的 `incremental_ms` 写成 `0.0`，
  最重要的那个数字反而没信息量。改自适应精度（`< 1.0` 用 6 位小数）。
- **亚毫秒指标不能按绝对值比对**：同机连续两次跑出 0.064 / 0.375 ms（差 5.8×），
  `--check` 误报「回归」。改成取 9 次最小值 + 比对时对「两边都 < 1 ms」的指标
  标注跳过；真正的判定交给代码里那条**机器无关的结构性断言**
  （批量必须比增量慢 100× 以上，实测余量 185–355×）。
- **clippy 参数位置**：`cargo clippy --all-targets --message-format short -- -D warnings`
  里 `--message-format` 放在 `--` **之后**会被当成 clippy 自己的参数，
  报 `Unrecognized option: 'message-format'`。必须放在 `--` 之前。
- GUI 侧反复出现的 `warning: cping-gui (build script) generated 1 warning` 是
  **tauri 构建脚本**发出的（清理后单独重编可确认），与手写代码无关。

#### 测试与实测

- 工作区 **552 → 559 passed**（+7：核心 `result.rs` 4 条增量口径 + 核心
  `tests/scheduler.rs` 1 条事件完整性 + TUI 2 条增量对账）；GUI 仍 **30 passed**，
  合计 **589**
- 基准：`cargo bench -p cping-core --bench scan` 全绿；`--write-baseline` 生成基线；
  `--check` 退出码 0
- `cargo fmt --all -- --check`、工作区 clippy、GUI clippy、`vue-tsc --noEmit`、
  `check-i18n`、`check-symbols`、`check-portability` 全干净
- **真实冒烟**：`cping-cli -t 127.0.0.0/29 --timeout 500` → 6 目标 / 6 在线 / 0 错误，
  0.012 s，退出码 0

#### 未实测（如实登记）

- 基准数字只有**一台机器**（本机 Windows）的口径；Linux / macOS 上未跑过
- TUI 的增量路径由 `TestBackend` + 模拟后端断言（**真实跑调度器**），
  但没有在真终端里以万级目标看过观感
- GUI 的增量路径没有真机点击验证（本机无桌面会话）
- 65536 目标下的端到端内存峰值与界面响应属于**第三步**，本步只做到 10000 目标

### M19 — v2.7.0 规模化性能（第二步实施记录，2026-10-15）

第二步的起点是第一步实测出来的那条契约：**`Progress` 既不保序也不完整**。
既然它既不保序（会领先于自己的 `Result`）、尾部又会丢，那它就不能再承担
「让界面知道完成数」这个职责——**完成数的权威来源只能是 `Result`**。
想通这一点之后，第二步要做的就不是「让进度更准」，而是「让进度更便宜」。

#### 做法

- **`Progress` 降级为心跳**：按时间节流（距上次发出 ≥
  `PROGRESS_MIN_INTERVAL_MS = 200` ms 才发一条），中间态直接合并掉只留最新值。
  200 ms 的依据是比人眼能分辨的进度条跳动（约 100 ms）略慢一点，
  于是「看起来在动」不受影响，而事件数从**上万条降到个位数**。
  多个 worker 同时判定「该发了」时用 **CAS 抢名额**——否则并发一高就会冒出
  好几条同刻的进度事件，节流等于没做。
- **通道改有界**：`mpsc::sync_channel(EVENT_QUEUE_CAPACITY = 4096)`。
  分层丢弃策略：**先丢进度，结果与终态永不丢**；结果也满时**阻塞在发送侧**
  （真背压）——丢结果比卡住更糟。放弃无界通道的理由很直接：无界通道在
  「UI 慢、生产者快」时会一路上涨到 OOM，那是生产环境里最难查的一类崩溃。
- **丢弃可观测**：新增 `EventStats { progress_sent, progress_merged,
  progress_dropped }` + `merge_ratio()`，挂在 `SchedulerHandle::event_stats()`
  与 `RunOutcome.events` 上。**静默丢弃比丢弃本身更糟**——「丢了」必须是能看见的。
  （`run_to_completion` 会消费掉句柄，所以摘要里也放一份。）
- **`Drop` 顺序修正（这条是真死锁，不是洁癖）**：通道改有界之后，worker 可能
  正阻塞在 `send(Result)` 上等队列腾位。此时若直接 `join`，就没人在取事件，
  而 `Receiver` 还活着**不会**让 `send` 返回 `Err`——直接死锁。所以句柄的
  `events` 字段包成 `Option`，`Drop` 里**先 `drop(events.take())` 再 join**。
- **先紧后松地同步改 `asynchronous.rs`**：两套实现共用同一组常量与同名的
  `claim_progress_slot()` / `event_stats()`，避免「只优化了同步版」。
- **前端不再靠进度事件推进**：`ping.ts` 的 `Result` 分支自己把 `completed` +1
  （它本来就知道每条结果的归属），`Progress` 分支只做**兜底校正**
  （取较大值并夹到 `total`）。

#### 实测（Windows 本机，bench profile）

| 场景 | 第二步之前 | 第二步之后 |
| --- | --- | --- |
| 10000 目标不限速（5.7–6.6 ms） | 10000 条进度事件（合并比 1.0×） | **0 条**（合并比 ∞——整个扫描比 200 ms 节流窗口还短） |
| 10000 目标默认限速（19.0 s） | 10000 条 | **94 条**（合并比 **106×**） |
| 事件队列上限 | 无上限 | **4096 条 × 144 B = 576 KiB** |

队列上限 576 KiB 相对 65536 目标的整表 7.50 MiB 是小头——这条由
代码里的结构性断言守着（队列上限必须远小于整表，否则「有界」等于没有上限）。

#### 踩坑

- **有界通道 + `join` 顺序 = 真实死锁**。这不是推演出来的：写完
  `dropping_a_handle_mid_scan_does_not_deadlock` 后**故意把
  `drop(self.events.take())` 注释掉**做负向验证，测试在 15 s 看门狗上超时并
  报「句柄在扫描中途被丢弃时死锁了」。改回来才过——证明修复是必要的、
  测试是有牙的（不是「加了行代码也没坏」那种摆设测试）。
- **`--check` 的跳过规则一开始写太宽**：原本是「两边都 < 1.0 就跳过」，
  结果把 `progress_sent`（10000 → 0）这种**最重要的计数类指标**也一起跳过了，
  等于把本步最该看见的收益藏起来。改成**只跳过以 `_ms` 结尾的时长**，
  计数类（事件条数、字节数）照常比对。
- **基线为 0 的「越小越好」指标算不出倍率**（除零）。`progress_sent` 的理想值
  就是 0，如实打印「跳过（基线为 0，算不出倍率）」而不是假装通过；
  真正的判定交给结构性断言（`progress_sent <= 10`、`merge_ratio > 100`）。
- **`assert!(PROGRESS_MIN_INTERVAL_MS >= 100)` 触发 `clippy::assertions_on_constants`**：
  常量断言要用 `const _: () = assert!(...)` 做成**编译期**断言，运行期的
  `assert!` 在 clippy 眼里是恒真的死代码。
- **CI 此前从未编译过 `async` feature**（它默认关闭）。第二步两套实现都改了，
  顺带在 `core` job 里补上 `--features async` 的 clippy + test——
  否则会出现「改了异步版却没人编译」。

#### 测试与实测

- 工作区 **559 → 563 passed**（+4）；核心 lib tests 320（默认）/ **327（async）**；
  GUI 仍 **30 passed**，合计 **593**
- 新增 4 条核心测试：
  - `progress_events_are_merged_instead_of_one_per_result`（合并比 ≥ 20×）
  - `a_slow_consumer_never_loses_results_and_stays_bounded`（慢消费者下
    65536 个结果**一条不少**，且队列有界）
  - `dropping_a_handle_mid_scan_does_not_deadlock`（15 s 看门狗，已负向验证）
  - `event_queue_capacity_is_a_real_bound`（`const _: () = assert!(...)` 编译期断言）
  - 异步版另加 `dropping_an_async_handle_mid_scan_does_not_deadlock`
- 基准：`cargo bench -p cping-core --bench scan` 全绿（含新的结构性断言）；
  `--write-baseline` 重生成基线；`--check` 退出码 0（20 项）
- `cargo fmt --all -- --check`、工作区 clippy、**async clippy**、GUI clippy、
  `vue-tsc --noEmit`、`check-i18n`、`check-symbols`、`check-portability` 全干净

#### 未实测（如实登记）

- 进度节流在**真实万级扫描**下的观感（本步只有 bench 的墙钟口径与模拟后端）
- TUI / GUI 的进度推进改造**没有真机观感确认**（本机无桌面会话、无万级真机扫描）
- `progress_dropped` 在实测里**始终为 0**（背压没被真正触发过——限速把生产
  速度压得足够慢）。它由单测覆盖，但没有在真机上丢过一条
- 65536 目标的端到端内存峰值属于**第三步**

### M19 — v2.7.0 规模化性能（第三步实施记录，2026-10-15）

第三步的原始计划是三件事：65536 目标真实跑一遍量内存峰值、定内存预算常量并断言、
TUI 在万级目标下仍可取消。做下来多了一件事——**在量内存的过程中发现了终态路径上
一份本不该存在的整表副本**，顺手修掉了，它才是这一步最大的一笔内存收益。

> M19 三步到此收口，版本号升到 **v2.7.0**。

#### 做法

- **终态路径不再克隆整表**（计划外，实测中发现）：协调线程在发终态事件前写着
  `slots.iter().flatten().cloned().collect()` 再 `ScanStats::from_results_with_total
  (&collected, total)`——**克隆一整份结果表，唯一的作用就是被数一遍**。
  65536 目标下这是 7.5 MiB，而且它与结果表**同时存活**，等于把峰值直接翻倍。
  现在持锁用第一步做好的**增量口径**逐条累加：O(1)/条、零分配，口径逐字段相同
  （由 `terminal_stats_match_a_batch_recompute_of_the_returned_results` 守着）。
  同步版与异步版一起改。
- **内存预算模块 `cping_core::budget`**：五个结构性常量（结果整表、结果快照、
  目标表、事件队列上限、每 worker 临时），全部由**类型的编译期宽度 × 明确的计数
  上限**相乘得到——**没有一个来自测量**，所以可以做成 `const fn`、可以做编译期断言、
  换台机器也不会红。两个入口：`scan_memory_budget_bytes`（流式路径，TUI/GUI 用）
  与 `run_to_completion_budget_bytes`（多一份结果副本，CLI/测试用）。
- **预算不是靠猜的，是靠断言钉住的**：三条编译期 `const _: () = assert!(...)`
  （队列上限必须远小于结果整表、预算必须盖住结构性开销之和），
  加六条单元测试（单调性、夹取、两条预算的关系、登记值对账、文档承诺的量级）。
- **基准新增 `[2d]`**：65536 目标**真实跑完**（不是外推）+ 实测峰值必须落在预算内
  的结构性断言。这条断言机器无关，可以进 CI。
- **TUI 万级取消测试**：`10.0.0.0/16` → 65534 个目标（CIDR 排除网络号与广播地址），
  贴着 `MAX_TARGETS` 硬上限；中途取消必须**立刻**停下，并用 ratatui 内存后端
  渲染一帧证明「界面在万级目标下还能画出来」。

#### 实测

**内存（这步的主要收获）**：

| 场景 | 第二步之后 | 第三步之后 |
| --- | --- | --- |
| 10000 目标（不限速）堆峰值 | 3.80 MiB | **1.94 MiB**（−49%） |
| 65536 目标端到端堆峰值 | 未测 | **9.37 MiB** |
| 流式预算（默认并发 32） | 无 | **11.12 MiB** |
| 流式预算（满并发 256） | 无 | **25.12 MiB** |

10000 目标那一行的 −2 MiB 正是「终态克隆」那一刀：1.2 MiB 的结果表副本加上它带出的
分配开销。**这一步没有动任何算法，峰值就掉了一半**——这就是「按结构性开销算账」
比「看着不大」有用的地方。

**端到端（65536 目标真实跑完，不是外推）**：

| 口径 | 数字 |
| --- | --- |
| 模拟后端（bench，release） | 25.8 ms · **248 万目标/秒** · 65536 条结果一条不少 |
| 真实 ICMP（CLI，debug，`127.0.0.0/16` 全回环） | **42.4 s** · 65534 目标 · 在线 58365 · 超时 7169 · 错误 0 |
| 导出 JSON 核对 | 65534 条 · `total == completed == 65534, pending 0` · 16.90 MiB |

真实 ICMP 那一行的 7169 个超时是**真实观测**：Windows 并不会为 `127.0.0.0/8` 的每个
地址都回 ICMP，所以这不是缺陷而是「回环地址段的行为」——但它顺便证明了一件重要的
事：**超时路径在万级规模下也是走通的**，不是只跑通了全部在线那种顺风局。

#### 踩坑

- **`size_of::<IpAddr>()` 是 17，不是 16**。它是 `V4`/`V6` 两个变体的枚举，判别式
  占了第 17 个字节（两个变体都是 `align = 1`，所以不补齐）。我写预算时按 16 算，
  被刚写好的 `every_term_matches_its_registered_width` 当场抓住——
  目标表是 1.06 MiB 不是 1.00 MiB。**这就是把宽度写成「可断言的登记值」的价值**：
  算错会红，不会悄悄低报。
- **`Ord::clamp` 还不能用在 `const fn` 里**（`Ord` 尚未 const 稳定，
  rust-lang/rust#143874）。预算函数要能在编译期被断言调用，所以夹取只能手写
  `if/else`。
- **并发度要夹到 `MAX_CONCURRENCY` 而不是 `MAX_TARGETS`**：若按后者算，
  `64 KiB × 65536` 在 32 位平台（Android armv7）上会溢出 `usize`。
  按配置校验保证的上限算，既正确又不会溢出。
- **CIDR 展开比「减一」少一个**：`10.0.0.0/16` 是 65534 个目标（网络号与广播地址
  **都**排除），我第一版测试写 65535，红了。

#### 测试与实测

- 工作区 **563 → 571 passed**（+8：`budget` 6 条 + 核心 `tests/scheduler.rs` 1 条
  终态口径守卫 + TUI 1 条万级取消）；核心 lib 326（默认）/ **333（async）**；
  GUI 30；合计 **601**
- 基准 `cargo bench -p cping-core --bench scan` 全绿（含 `[2d]` 的预算断言）；
  `--write-baseline` 重生成（新增 5 个键：65536 端到端的耗时/结果数/峰值 + 两条预算）；
  `--check` 退出码 0
- `cargo fmt --all -- --check`、工作区 clippy、**async clippy**、GUI clippy、
  `check-portability`、`check-symbols` 全干净（`vue-tsc` / `check-i18n` 本步未改前端）
- **真实冒烟**（两轮）：
  1. `cping-cli -t 127.0.0.0/24 --max-pps 0 --concurrency 64 --timeout 300`
     → 254 目标全在线，1.1 s，退出码 0
  2. `cping-cli -t 127.0.0.0/16 --max-pps 0 --concurrency 256 --timeout 300 --json …`
     → 上表那 42.4 s 的一行

#### 未实测（如实登记）

- 预算里的 `PER_WORKER_SCRATCH_BYTES = 64 KiB` 是**全表唯一估出来的数**。
  实测峰值只用到预算的 84%（9.37 / 11.12 MiB），说明这个估计**偏保守但不离谱**；
  不过它没有被独立测过（没有单独量过「一路 worker 到底占多少堆」）。
- 内存峰值的读数来自**计数全局分配器**（`benches/scan.rs`），所以它数的是
  **堆**、不含线程栈与 mmap；这条与预算的口径一致，但不是 RSS。
- TUI 万级取消是**内存后端 + 模拟后端**的断言；真实终端下万级目标的观感、
  以及 GUI 侧的观感仍未人工确认（本机无桌面会话）。
- 65536 目标的端到端只有**本机 Windows** 的口径；Linux / macOS / Android 未跑过。
- `MAX_TARGETS` 仍是 65536 硬上限（`index: u16` 与它绑定），本阶段**刻意不提高**
  ——那是 M20 主版本的事。
