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

（暂无）

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

[1.1.2]: https://github.com/LingBrian/cping/releases/tag/v1.1.2
[1.1.1]: https://github.com/LingBrian/cping/releases/tag/v1.1.1
[1.1.0]: https://github.com/LingBrian/cping/releases/tag/v1.1.0
[1.0.0]: https://github.com/LingBrian/cping/releases/tag/v1.0.0
