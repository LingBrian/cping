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
| M15 | v2.3.0 | 告警与通知 | 阈值告警、Webhook / 邮件 / 系统通知 |
| M16 | v2.4.0 | 配置与模板 | 扫描模板、配置导入导出、CLI 配置文件 |
| M17 | v2.5.0 | 安全与合规 | 签名制品、权限最小化、审计日志、限速 |
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
- 验收：打包脚本产出可解压可运行的包并实测冒烟；Windows x86 产物在 x64 系统上运行正常。

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
