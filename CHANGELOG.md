# 更新日志（Changelog）

格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，
版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/)。

## [1.1.2] - 2026-10-14

### 修正

- **「表单显示」的结果无法用滚轮滚动查看**（255 行只能看到前 22 行左右，
  之后的内容既没有滚动条、滚轮也滚不动）
  - 根因：naive-ui Tabs 的高度链有两环是断的——`.n-tabs-pane-wrapper` 是
    `overflow: hidden` 的可收缩 flex 项（内容被静默裁掉但不提供滚动），
    `.n-tab-pane` 高度是 `auto`，于是内层 `height: 100%` 全部退化成 `auto`，
    表格容器撑到 255 行全高后被裁掉。「图形显示」只是恰好放得下才没暴露
  - 修复：在桌面布局里补上这两环（`flex: 1 1 auto; min-height: 0` +
    `height: 100%`），滚动容器的 `max-height: 100%` 恢复生效
  - 验证：恢复 255 行快照后向 WebView2 输入窗口注入滚轮消息，
    表格正常滚动、滚动条滑块同步移动、表头保持吸顶

## [1.1.1] - 2026-10-14

### 修正

- **GUI 布局**：初始窗口从 1000×680 放大到 1240×840，默认尺寸下侧栏、
  状态栏和七列表格全部可见，不再需要滚动条
  - 表格改用 `table-layout: fixed`：此前七列合计约 1050px，主区只有约 690px，
    必然撑出横向滚动条，右侧的厂商/主机名列等于看不见；现在列宽按比例
    填满容器，放不下的内容就地省略（完整值悬停可见），横向滚动条彻底消失
  - 启动时把窗口尺寸收敛到当前显示器的可用区域内（只缩小不放大）：
    分辨率不够的机器上不再把窗口顶出屏幕
  - 屏幕高度不足时进一步收紧侧栏间距（`max-height` 媒体查询）兜底

## [未发布]

### 修正

- **CI 与全新克隆无法构建：仓库内 `.cargo/config.toml` 硬编码了 `[net] offline = true`**
  - 现象：`v2.0.0` tag 触发的 `Release` workflow 与 `main` 的 `CI` workflow **全部任务失败**，
    报错为 `error: no matching package named tokio found` / `no matching package named
    indexmap found`，并附带 `note: offline mode (via --offline) can sometimes cause
    surprising resolution failures`
  - 根因：本 workspace 存在**可选依赖**（`cping-core` 的 `async` feature 引入 tokio、
    `cping-gui` 的 Tauri 壳引入 indexmap 等）。cargo 在解析依赖图、生成与校验
    `Cargo.lock` 时**必须访问 registry 索引，即使这些 feature 未启用**。该行写死于
    M1a 阶段，当时全仓库零第三方依赖所以无害；M11 引入 Tauri 壳、M12 引入异步调度后
    才成为致命阻塞。本地一直「看起来正常」只是因为依赖缓存是热的
  - 修复：移除该行，改为在文件内以注释说明「需要离线时按需用 `cargo build --offline` /
    `CARGO_NET_OFFLINE=true`，不要落盘」，并保留无外网环境的 vendored 快照做法
  - 验证：改动后本地 `cargo clippy --workspace --all-targets` 无 lint 警告、
    `cargo test --workspace` 全绿；随后重跑 CI 与 Release 两条 workflow 确认绿

- **OUI 厂商名在 Windows 上会带上尾随 `\r`**：同一份数据在 Linux 与 Windows 上查出
  「看起来一样、比较却不相等」的厂商名（CI 报 `left: Some("XEROX CORPORATION\r")`）
  - 根因：`oui_ma_*.txt` 由 `include_str!` **逐字节**嵌入，Windows 上按
    `core.autocrlf=true` 检出成 CRLF 后，`find()` 手工按 `\n` 切片，厂商名末尾就留下 `\r`
  - 修法两层：`find()` 出口 `trim_end_matches('\r')` 让结果与换行符无关；
    `.gitattributes` 把 `cping-core/src/data/*.txt` 钉死为 LF，保证嵌入字节跨平台一致
  - 附带解释「为什么单测没抓到」：夹具测试用的是 `str::lines()`，
    它**本身就会剥掉 `\r`**，走的不是真实查表路径
  - 新增回归测试 2 条：合成 CRLF 夹具（`find_is_crlf_safe`，各平台都守得住）+
    真实数据抽查（`real_data_lookups_never_leak_carriage_returns`，断言输出不含 `\r`）
  - 验证：把三份数据临时转成 CRLF 复现 CI 条件，完整 `cping-core` 套件
    236 passed / 0 failed；随后还原为 LF 并与备份逐字节比对一致

## [2.0.0] - 2026-10-15

主版本。把 v1.1.2 之后的五个阶段（M8–M12）一次交付：持续监控、探测扩展、
数据与集成、平台与分发、架构升级。**含破坏性变更**，迁移表见本节末。

### 新增

**共享核心 `cping-core`**

- **持续监控（M8）**：`monitor` 模块提供 `MonitorConfig`（间隔 / 启用）、
  `ChangeKind`（`Appeared` / `Disappeared` / `RttClassChanged` / `StatusChanged`）、
  `Change` 记录、纯函数 `diff_scans(prev, next)` 与环形缓冲 `ChangeLog`
  （容量封顶，长跑不涨内存）；`history` 模块按轮次保存精简快照。
- **目标规格解析（M9）**：`spec` 模块统一解析「网段前缀 / 单主机 / CIDR / 多段」，
  产出 `TargetSpec { targets, segments }`。CIDR 要求主机位为 0（写错时报错并给出正确网段），
  `/31`、`/127` 两端都可用，`/32`、`/128` 为单地址；展开数量受 `MAX_TARGETS = 65536` 约束，
  `/0`、`/64` 这类巨型网段**明确拒绝**而不是静默展开。
- **探测方式（M9）**：`ProbeMode::{Icmp, Tcp { port }}`。**一次任务只有一种方式**，
  因此 ICMP 的「在线」与 TCP 的「开放」不会混进同一列统计；
  TCP 模式下状态显示为 `TCP-OPEN` / `TCP-超时` / `TCP-关闭`，一眼可区分。
- **JSON 与 HTML 报告（M10）**：零依赖 JSON 序列化（严格转义、键序稳定）；
  单文件自包含 HTML 报告（内联矩阵 + 表格 + 统计卡片，无外部资源、无 JS）。
- **扫描差异（M10）**：`diff` 模块对比两次扫描（新增 / 消失 / 延迟档位变化 / 状态变化），
  可导出 CSV / JSON / HTML；`ScanReport` 是四种格式的唯一构造点。
- **后端注册表（M12）**：`BackendId`（`icmp4` / `icmp6` / `tcp` / `mock`）、
  `BackendCaps`（可用性 + 支持哪些地址族 + 是否需要权限 + 说明）、`BackendRegistry`。
  界面据此**在探测前**就能提示「这个组合不行」，而不是扫完才发现清一色「不支持」。
- **IPv6（M12）**：目标模型从 `Ipv4Addr` 统一为 `IpAddr`，同一任务里 v4 与 v6 可混排；
  排序口径 `addr_sort_key` 只定义一次，导出 / 差异 / 监控三处共用。
- **可选异步调度（M12）**：新增 `async` feature（**默认关闭**），提供 `AsyncScheduler`，
  用信号量封顶并发 + `spawn_blocking`，语义与同步版逐条对齐。默认构建仍**零第三方依赖**。

**命令行 `cping-cli`（M10，新增二进制）**

- 无头模式：`--target` / `--target-file` / `--range` / `--mode` / `--port` / `--timeout` /
  `--concurrency` / `--watch` / `--rounds` / `--out`（按扩展名选 csv/txt/json/html）/
  `--quiet` / `--enrich`。
- 退出码契约：`0` 全部在线 / `1` 存在离线 / `2` 参数错误 / `3` 权限不足或后端不可用。
- `--list-backends`：列出本平台全部后端（**含不可用的**）与能力，不猜测。
- 面向自动化，因此**不提供** `--mock`——无头命令里混进模拟数据会让退出码失去意义。

**TUI（`cping-tui`）**

- 持续监控：`M` 开关、`[` / `]` 调间隔、`e` 查看变化事件流，变化行高亮。
- 探测方式：`T` 切换 ICMP ↔ TCP、`p` 轮换常用端口（22/23/80/135/139/443/445/3389）。
- 帮助面板新增「探测后端（M12）」一节：当前后端能力、可选后端、IPv6 走 TCP、
  ICMPv6 的缺口，内容与 `cping-cli --list-backends` 同源。

**桌面 GUI 与 Android（`cping-gui`）**

- 目标规格输入支持网段 / 单主机 / CIDR / 多段；探测方式单选 + 端口下拉 + 自定义端口。
- 持续监控开关与间隔、变化页签、变化高亮并自动淡出、状态栏显示轮次与变化计数。
- 四种导出（CSV / TXT / JSON / HTML）与「导出本轮差异」。
- IPv6 使矩阵视图失效时**自动切表格**并说明原因（v6 没有「最后一格 = 格号」的语义）。

**分发（M11）**

- `scripts/package.sh`（Linux，产出 tar.gz）与 `scripts/package.ps1`（Windows，产出 zip），
  每个包内附 `README.txt`（版本、平台、用法、退出码、权限说明），并生成 `SHA256SUMS`。
- `.github/workflows/ci.yml`：fmt / clippy / test / 前端 typecheck 与构建 /
  Android 交叉编译检查，并断言 `cping-core` 的依赖树只有它自己一行。
- `.github/workflows/release.yml`：打 tag 自动构建 Windows(x64/x86) / Linux / Android 产物，
  合并校验和后创建 Release。

### 变更

- 三端的「选哪个后端」与「`--mock` 走哪个实现」收敛到核心的
  `resolve_for_run` / `backend_for_run`：此前 TUI 与 GUI 各写一份分支，
  连模拟分布参数都要手动保持一致。
- TUI 切换探测方式时**沿用**启动时的 `--mock` 选择；此前切一次方式会静默变成真实探测，
  看起来像「结果变了」。
- 矩阵视图的适用条件明确为「非空 && ≤256 个目标 && **无 IPv6**」，
  由 `TargetSpec::matrix_compatible()` 单点判定。
- 导出 CSV 追加「探测方式」列（**在末尾**，旧列位置不变），TXT 头部显示本次探测方式。
- 桌面 GUI 的窗口尺寸收敛逻辑改为桌面专属（`#[cfg(desktop)]`）：
  `WebviewWindow::center()` 在移动端不存在，此前 Android 构建会直接编译失败。

### 破坏性变更与迁移

| 变更 | 从 | 到 | 迁移方式 |
| --- | --- | --- | --- |
| 目标地址类型 | `Ipv4Addr` | `IpAddr` | `TargetSpec::targets` 元素类型改变；排序用 `addr_sort_key` |
| 探测后端接口 | `probe(Ipv4Addr, Duration)` | `probe(IpAddr, Duration)` | 后端需自行拒绝不支持的地址族（返回 `unsupported`） |
| 矩阵格号 | `cell_of_addr(Ipv4Addr) -> usize` | `cell_of_addr(IpAddr) -> Option<usize>` | v6 返回 `None`，调用方必须处理 |
| 矩阵槽位 | `matrix_slots(&[Ipv4Addr])` | `matrix_slots<T: AsTargetAddr>(&[T])` | 传 `&[IpAddr]` 或 `&[Ipv4Addr]` 均可 |
| 探测方式 | 只有 ICMP | `ProbeMode::{Icmp, Tcp { port }}` | 取平台默认用 `platform_default_mode()` |
| CSV 格式 | 8 列 | 9 列（末尾新增「探测方式」） | 按列名解析，不要按下标硬编码 |
| 三端后端选择 | 各自 `backend_for_mode` | `resolve_for_run` / `backend_for_run` | 旧接口仍保留，新代码用注册表 |

- `native::backend_for_mode` 仍保留（不破坏既有调用方），但新代码应改用
  `registry::backend_for_run` / `resolve_for_run`。
- 默认 feature 组合下 `cping-core` **仍无第三方运行时依赖**
  （`cargo tree -p cping-core -e normal` 只有它自己一行）；Tokio 仅随 `async` feature 引入。

### 验证（实测）

- **274 个 Rust 测试全部通过**（`cargo test --workspace`，较 v1.1.0 的 151 个新增 123 个），
  另有一批真实网络用例按 `#[ignore]` 单独运行；`cargo clippy -D warnings` 与
  `cargo fmt --check` 在 workspace 与 GUI 两套上均干净。
- **Windows x64**：`--list-backends` 输出的能力表与实现一致；
  `-t ::1 --mode icmp` → `不支持 1`、退出码 `3`（ICMPv6 未实装如实上报，**不是**静默超时）；
  `-t ::1 --mode tcp --port 18081` → `TCP-OPEN`、退出码 `0`。
- **Linux（WSL Debian 13，Rust 1.99.0，x86_64）**：
  - 原生 ICMP 实测通过（`127.0.0.1` 1 ms、默认网关 0 ms）；
  - TCP v4/v6 实测通过（`127.0.0.1:18080`、`::1:18081` 均 `TCP-OPEN`）；
  - **TCP 关闭端口实测**（`127.0.0.1:18999`）→ 内核回 RST，分类为「不可达」、
    退出码 `1`、用时 0.000 s——这条路径在 Windows 上无法可靠复现，Linux 上补上了；
  - 混合族一次任务：`::1, 127.0.0.1` → 「2 个网段 / 2 个目标」，两个都在线；
  - TUI 在真实伪终端内渲染并进入备用屏，按 `q` **优雅退出（码 0）**；
  - CSV / TXT / JSON / HTML 四种导出均实测产出且内容正确（含 `::1` 行）。
- **Windows x86（32 位）**：release 产物构建成功，并可在 x64 系统上运行。

### 已知限制

- **ICMPv6 未实装**：`icmp6` 后端在注册表里登记为 `available: false`，
  选中它会得到明确的「不支持」而不是静默超时。IPv6 的完整可用路径是 **TCP**。
- **Android 未真机验证**：本机无设备、无 AVD；安装 / 启动 / 触控 / 前后台 / 文件保存
  均未实测。release APK 未签名。
- **Linux 的邻居表（MAC）与主机名链路**未在真实 Linux 上实测，只有单测覆盖。
- **「TCP-关闭」在 Windows 上不可靠**：本机连确定关闭的端口也表现为超时
  （用 Python 独立对照同样超时），因此 Windows 侧不宣称该路径已实测。
- 矩阵视图对 IPv6 不可用（语义所限，非缺陷）。

## [1.1.0] - 2026-10-14

目标信息增强（M7）：让扫描结果回答「这是谁」，而不是只回答「它在不在线」。

### 新增

**共享核心 `cping-core`**

- **厂商识别（离线 OUI 库）**：内置 IEEE MA-L / MA-M / MA-S 三张注册表
  （共 **54,190 条**，1.68 MB，编译期 `include_str!` 嵌入，运行时零 I/O），
  按 9/7/6 个十六进制位**最长前缀优先**匹配；全零、组播（I/G）、本地管理（U/L）
  地址一律不查——查不到就留空，不猜。
  数据由 `tools/oui-gen/gen_oui.py` 从 IEEE 官方 CSV 确定性生成，
  附 `SOURCE.md` 记录来源与生成时间，可复现。
- **MAC 地址获取**：Windows 走 `GetIpNetTable2`（手写 FFI，28/88 字节结构体布局
  断言钉死），Linux/Android 解析 `/proc/net/arp`；只接受单播 MAC
  （拒绝全零与广播 `FF:FF:FF:FF:FF:FF`），仅补空缺、不覆盖已有值，可安全重复调用。
- **主机名反查**：`getnameinfo` + `NI_NAMEREQD`，独立线程 + 超时，
  **按需**单条发起（绝不批量阻塞）；结果规整（去尾点、拒 IP 字面量、剥 NetBIOS
  数字后缀），带否定缓存；查不到保持为空。
- **结果快照与恢复**：面向行的纯文本格式（签名 `CPING-SNAPSHOT` + 版本 1，
  固定 8 列），严格解析——签名/版本不认识、列数不符、行数不符、在线无 RTT
  一律报「快照损坏」，绝不猜测性修补；统计口径与实时扫描完全一致。

**TUI（`cping-tui`）**

- 表格视图详情行显示 MAC / 厂商 / 主机名（窄终端自动把 MAC 挪到详情行并按宽度截断）
- 新快捷键：`m` 补全 MAC/厂商、`n` 反查当前行主机名（后台线程，不卡界面）、
  `w` 保存快照、`l` 从快照恢复
- 状态栏新增「厂商 N / 主机名 N」；扫描结束自动补全一次并落盘快照
- 启动时发现上次结果会弹确认框，`y` 恢复 / `n` 丢弃，**默认不恢复**

**桌面 GUI 与 Android（`cping-gui`）**

- 表格新增「厂商」「主机名」列；主机名按行提供「反查」按钮（带 loading，不阻塞界面）
- 侧栏新增「补全 MAC / 厂商」「保存快照」；状态栏新增已识别 MAC / 厂商 / 主机名计数
- Android（无文件对话框）：快照写入应用可写目录并回报完整路径
- **每轮扫描结束自动落盘**一份快照（Android 上进程会被系统随时回收，不依赖退出钩子）
- 启动时发现上次未处理的结果弹确认框（丢弃 / 暂不恢复 / 恢复结果），**默认不恢复**；
  同一份结果只询问一次，不会每次启动都弹

**导出**

- CSV/TXT 新增「厂商」「主机名」两列（**追加在末尾**，旧列位置不变），
  厂商名含逗号时按 RFC 4180 转义；TXT 定宽排版按显示宽度截断（CJK 宽度感知）

### 变更

- Android「退后台日志噪音」正式改判：确认为 `tao` 主动 `process::exit(0)`，
  非本项目崩溃（见 [未发布] 之前已入库的归因文档）。由此把「结果可能随进程丢失」
  从缺陷转成了 M7 的功能需求（结果快照）。

### 验证（实测）

- 151 个 Rust 测试全部通过（`cargo test --workspace`，较 1.0.0 的 84 个新增 67 个）；
  另有 8 个真实网络/诊断用例按 `#[ignore]` 单独运行
- 真实局域网端到端（Windows 原生 ICMP）：6 目标扫描 → 邻居表 5 条 → 补 MAC 3 条 →
  厂商 1 条（`zte corporation`）；回环无 MAC、`.255` 广播无 MAC、本地管理地址无厂商；
  `127.0.0.1` 反查出真实主机名 `XuanKui`
- 真实数据**抓出并修复两个真 bug**：`SOCKADDR_INET` 按整型读取导致 IP 字节序颠倒
  （`192.168.0.1` 显示成 `1.0.168.192`），已改为按字节读取并加回归测试；
  广播 MAC 曾随 `.255` 目标混进结果表，已加单播过滤
- 真实结果快照往返：落盘 337–372 字节，读回后逐字段与统计完全一致，序列化字节级稳定
- GUI 真实运行验证：用真实格式快照触发「发现上次结果」确认框，渲染与文案符合预期
  （`docs/images/m7-gui-restore-prompt.png`）
- 质量门禁：`cargo fmt --check` 干净、`cargo clippy -D warnings` 干净（workspace 与 GUI 两套）、
  `vue-tsc` 0 错误

### 已知限制

- Linux / Android 的邻居表与主机名链路已实现并单测，但未在真实系统实测
- 快照只覆盖「本机扫描结果」：不含配色与视图设置（那些在配置文件里）
- ARP 只包含近期真正通信过的地址，扫完立即补全是正确时机；
  但隔很久再补，可能因为缓存过期而补不到（此时如实显示为空）

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

[2.0.0]: https://github.com/LingBrian/cping/releases/tag/v2.0.0
[1.1.2]: https://github.com/LingBrian/cping/releases/tag/v1.1.2
[1.1.1]: https://github.com/LingBrian/cping/releases/tag/v1.1.1
[1.1.0]: https://github.com/LingBrian/cping/releases/tag/v1.1.0
[1.0.0]: https://github.com/LingBrian/cping/releases/tag/v1.0.0
