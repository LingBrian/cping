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

### 新增

- **国际化（i18n）：中 / 英双语**（M18 第一步：机制 + 三端界面文案）
  - 核心层新增 `cping-core::i18n`：`Lang`（`zh` / `en`）+ `Key` + 双语字典 +
    `render_template`。**键是枚举、不是字符串**——`t("typo")` 会在运行期静默
    显示成键名本身，而 `Key::Typo` 直接编译不过
  - **「漏译」是红灯，不是等用户截图**：`english_has_no_cjk`（英文条目里出现
    中日韩字符即失败）、`placeholders_match`（中英条目的 `{}` 个数必须相同）、
    `table_covers_every_key`（`TABLE` 与 `ordinal` 必须是双射）、
    `enum_labels_are_localized_both_ways`（从枚举一路走到渲染结果，
    抓「映射到了错的条目」）
  - **错误有两份表示**：稳定的错误码（`CoreError::code()` / `ProbeError::kind()`）
    与渲染后的句子（`localized(lang)`）。`Display` 就等于 `localized(Lang::Zh)`
    ——**共用一处实现**，不会出现「改了中文忘了改另一处」
  - 核心层 14 个枚举（结果类别、探测状态、变更种类、告警种类与严重级、后端、
    目标范围、审计事件、设备置信度……）统一加 `label_in(lang)`，
    `label_zh()` 变成它的转发——**中文只有一个来源**，
    不再散落在 14 个各自的 `match` 里各自漂移
  - 三端各自的字典（键空间是分开的：核心层说「逻辑发生了什么」，
    界面说「这个界面在说什么」）：CLI 148 键、TUI 225 键、GUI 241 键。
    GUI 用 TypeScript 类型（`Dict = Record<keyof typeof zh, string>`）
    + 零依赖校验脚本 `scripts/check-i18n.mjs` 双保险
  - **语言来源**：`--lang`（CLI / TUI）或设置面板（GUI）→ `CPING_LANG` →
    `LC_ALL` → `LC_MESSAGES` → `LANG` → 默认中文。
    `C` / `POSIX` 视为「没有偏好」而**不是**「请用英文」
    ——服务器的默认 locale 不该决定用户看到什么
  - `--lang` 取值非法**报错**（CLI 退出码 2）而不是静默忽略：
    「静默忽略用户显式给的参数」是本项目最不能接受的一类 bug。
    唯一的例外是 TUI：它是交互式程序，启动后用户还能在状态栏看到告警并修正，
    沿用该程序既有的「参数告警不退出」惯例
  - **刻意不翻译的东西**：CSV / JSON 的列名与字段名（**数据契约**——同一份报表
    在中英文机器上必须一致，下游脚本依赖它）、快照与时序的文件格式、
    TUI / GUI 的状态符号（`#` `+` `*` `-` `!` `.`）、命令行参数名、
    来自操作系统的原始错误详情（`No such file (os error 2)` 翻了反而更难搜）、
    **以及代码注释**

- **国际化（i18n）第二步：核心层的「自由文本」也本地化了**（M18 第二步）
  - 第一步只翻了**枚举标签**（一个词）；这一步翻的是**句子**——
    带参数、带语序的完整陈述。它们此前是散在 8 个模块里的 `format!`，
    英文界面下原样露出中文：目标规格摘要、变化事件摘要、探测方式状态名、
    设备类型推断（名字 + 依据）、后端能力说明、模板摘要、平台 ICMP 提示、
    通知通道说明、端口列表解析错误、时序与告警状态文件的读写错误、
    **告警正文**
  - 字典键 **83 → 151**（新增 68 条）。划界原则写进了代码：
    **只有散文进字典，标点不进**。`{} → {} ms` 这种纯符号模板留在代码里
    （`→` 在两种语言里都是 `→`），否则字典会被一堆「中英一字不差」的条目
    撑爆，真正需要复核的条目反而被淹掉
  - **技术记号有显式白名单**（`i18n::ASCII_TOKEN_KEYS`）：`TCP-OPEN`、
    `IPv4+IPv6` 两种语言下相同才是对的，但「中文条目必须含中日韩字符」
    那条检查会把它们当成漏译。与其放宽检查（放宽之后就再也抓不到真正的误填），
    不如给它们一个**能说出理由的白名单**，并加一条
    `ascii_token_allowlist_is_exact` 保证它不会腐烂：多一条、少一条都失败
  - **两个枚举被换成类型**（漏翻从运行期变成编译期问题）：
    `BackendCaps.note`（`&'static str` → `CapsNote`）、
    `DeviceGuess.kind`（`&'static str` → `DeviceKind`，8 个变体）。
    `DeviceGuess.basis` 从 `Vec<String>`（中文句子）改成 `Vec<BasisSignal>`
    ——**信号是数据，渲染成哪种语言是界面的事**
  - **自由文本的双语总闸**：`free_text_is_localized_both_ways` 从**真实输入**
    出发（真实的 `TargetSpec`、真实的 `DeviceGuess`、真实的解析失败），
    一路走到最终字符串，断言英文渲染里没有中日韩字符。只查字典表是不够的
    ——`summary_in()` 忘了把 `lang` 传下去，表还是干净的，界面照样露中文
  - **告警正文跟着语言走，但不影响落盘**：`AlertEngine::evaluate_in(lang)`、
    `match_rule_in(rule, ctx, lang)`。判定、冷却、静音、基线推进**一字不改**；
    告警状态文件里本来就不存 `message`（只存 `kind` / `ip` / 时间戳），
    所以落盘内容不受影响
  - **导出仍然是中文，且是刻意的**：CSV / JSON 的列名与字段名、报表里的
    目标摘要、审计记录里的 `targetSpec` 一律固定中文——它们是**数据契约**，
    同一份报表在中英文机器上必须逐字节可比。语言偏好只影响**界面**
  - **GUI 的 Rust 侧也接上了**：新增 `apps/cping-gui/src-tauri/src/i18n.rs`
    （GUI 命令自己那 37 条消息，机制与核心层同构）；所有产出展示文本的
    Tauri 命令收 `lang`（`Option<String>`，缺省时跟随环境变量以保持向后兼容）；
    前端统一走 `invokeT()` 注入当前语言；切换语言后重拉后端能力 / 快照 / 变化流水
    （趋势与告警仅在已加载过时重拉；扫描进行中不碰快照，避免与事件流竞争）
  - **CLI 与 TUI 的重复翻译收敛到核心层**：删掉只做转发的桥接条目，
    改为直接调 `label_in(lang)` / `describe_in(lang)` 等。
    TUI 字典 **225 → 217 键**；CLI 输出里的状态名、设备摘要、模板摘要
    全部改走核心
  - **补上了 M16 / M17 欠的界面**（原计划「并入 M18 统一动界面」）：
    TUI 新增模板选择面板（`P`）与策略设置面板（`G`）——启动加载配置文件
    （读不了只写状态栏、**绝不 panic**），合并优先级与 CLI 一致
    （**命令行显式参数 > 模板 > `[probe]`**，模板只填空缺）；策略面板可改
    速率上限 / 目标数上限 / 是否允许保留网段，取值非法**可见且不生效**
    （不夹取、不静默），`allow_reserved` 打开时有显式警示

- **`TABLE` 顺序错位会被静默吞掉——新增一条专门的测试守住**
  - 给 `Key` 追加变体时顺手把新条目插在了 `TABLE` 中间，结果后面所有键
    整体错位：`ICMP 需要权限`显示成了`厂商 {}`，模板摘要显示成了目标标签。
    **不崩、不报错**，只是所有消息都变成了邻居那一条
  - 根因：`text()` 是 `TABLE[ordinal(key)]`——按下标取值。
    原有的 `table_covers_every_key` 只检查序号集合是不是
    `0..KEY_COUNT` 的一个排列，**把任意两条互换位置它照样通过**
  - 修法：新增 `table_order_matches_ordinal`（`TABLE[i]` 的序号必须等于 `i`），
    三端各自的字典都加了一条

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

- **Release workflow 的 Linux 作业打包失败**：构建明明成功，
  却打印 `[缺失] .../target/release/cping-tui 不存在或不可执行` 并以 1 退出
  - 根因：workflow 用 `cargo build --target x86_64-unknown-linux-gnu` 构建，
    产物落在 `target/<triple>/release/`；而 `scripts/package.sh` 只看
    `target/release/`。Windows 作业显式传了 `-TargetDir target/<triple>/release`，
    Linux 作业缺了等价项——**两个作业对同一个脚本的契约不一致**
  - 修法：① `package.sh` 新增 `BIN_DIR` 显式覆盖，并在原生目录为空时自动回落到
    `target/*/release`，报错时列出已找到的候选目录；
    ② `release.yml` 的 Linux 作业显式传入 `BIN_DIR`
  - 验证：合成目录树跑通四种场景（原生优先 / 三元组回落 / 显式覆盖 /
    全部缺失时非 0 退出），产出清单可 `sha256sum -c` 通过

- **Release workflow 的 Android 作业组装 APK 失败**：Rust 侧四个 ABI 都编过了，
  Gradle 报 `Value 'D:/Program Files/Android/jdk-17' given for org.gradle.java.home
  Gradle property is invalid (Java home supplied is invalid)`
  - 根因：`apps/cping-gui/src-tauri/gen/android/gradle.properties` 里提交了
    **本机专属的绝对路径** `org.gradle.java.home=D:/Program Files/Android/jdk-17`
    （本机系统默认 JDK 26、AGP 不支持，所以当初写死了 JDK 17 的路径）。
    CI 是 Linux，该路径不存在，而这条属性会**覆盖** `setup-java` 设好的 `JAVA_HOME`
  - 修法：移除该属性，改用 `JAVA_HOME` 表达（CI 由 `actions/setup-java` 自动设置；
    本地由 `scripts/build-all.cmd` 设置），原位置留注释说明「不要提交绝对路径」

### 新增

- **可移植性守卫 `scripts/check-portability.sh`**：静态检查构建相关文件里
  有没有「本机环境耦合配置」，并接入 CI 的独立 job 与 `release.yml` 的 publish 阶段。
  拦住三类：`Cargo.toml` / `*.toml` / workflow / `package.sh|ps1` / `gradle.properties` /
  `build.gradle.kts` / `tauri.conf.json` 里的 **Windows 盘符绝对路径**、
  `.cargo/config.toml` 里的 **`offline = true`** 与 **`replace-with = "vendored-sources"`**。
  动因：v2.0.0 发布期间连踩三次同一类缺陷（本地全绿、CI 全红），
  光靠人工检查挡不住。已用负向测试确认两类注入都会被拦下。

## [2.5.0] - 2026-10-15

M17 安全与合规：默认限速、保留网段守卫、目标数上限、审计日志、首次授权告知、
制品签名与清单。**规则全在 `cping-core::policy`，三端只做接线。**

### 新增

- **`cping-core::policy`**（新模块，含 20 条测试）：
  - `ScanPolicy`：`max_pps` / `max_targets` / `allow_reserved`，越界值一律**报错
    而不夹取**
  - `reserved_reason(ip)`：保留网段判定，返回**给人看的原因**
  - `Bucket` / `RateLimiter`：令牌桶限速
  - `AuditRecord` / `AuditLog`：审计日志（JSONL 追加写）
  - `consent`：授权确认状态的读取/落盘与三端共用文案
- **默认限速 500 包/秒**（`--max-pps`，写 `0` 才关）。默认值就是「有限速」——
  合规不能指望用户自觉。调度器里所有 worker 共用同一个令牌桶
- **保留网段守卫**（默认拒绝，`--allow-reserved` 放行）。拦的是三类：
  - **会打扰别人**：组播 `224/4`（一个探测打到整组）、保留段 `240/4`（含
    `255.255.255.255` 广播）
  - **没有意义**：`0.0.0.0/8`（「本网络」）、IPv6 `::`
  - **一定会失败**：文档示例网段（`192.0.2/24`、`198.51.100/24`、`203.0.113/24`、
    `198.18/15`、`2001:db8::/32`）
  - **刻意不拦**：私有网段（`10/8`、`172.16/12`、`192.168/16`、`fc00::/7`）、
    回环（`127/8`、`::1`）、链路本地、CGNAT——那正是这类工具的正常用途
  - 命中时给出**具体原因**与放行方式，而不是笼统的「参数错误」
- **目标数上限**（`--max-targets`）：只能比结构上限 `65536` **更严**，
  参数突破不了结果槽位的预分配长度
- **审计日志**（`--audit <路径>`）：JSONL，首行 `CPING-AUDIT 1`，
  记录 `session_start` / `round` / `session_end`，内容为「谁、何时、扫什么规格、
  什么参数、结果**计数**、退出码」。
  - **不记**逐主机明细、MAC、厂商、反查出的主机名——审计要能自证
    「我扫了什么」，不是变成一份资产清单
  - 打开时校验首行标识，**不是本程序写的文件拒绝追加**（往别人的文件里塞东西
    比写不进去危险得多）；写入失败**必须可见**
- **首次启动告知**（`consent`）：首行提示「仅用于获授权网络」，状态落在
  `<家目录>/.cping/consent`，**按版本号判重**（文案升版会重新提示一次）。
  **不做阻塞式确认**——CLI 会被 CI 调用，卡在等人回车等于把工具弄坏。
  `--quiet` 下改走 stderr：静默模式是「别刷屏」，不是「免告知」
- **制品清单 `MANIFEST.json`**（`scripts/artifact-manifest.sh`）：每个产物的
  sha256 / 字节数 + 构建来源（版本、提交、rustc）；提供 `CPING_SIGNING_KEY`
  时对清单做 gpg 分离签名
- **Windows Authenticode 签名**（`scripts/sign-windows.ps1`）：`/fd SHA256`
  + 时间戳 + `/pa`，签完立即 `Get-AuthenticodeSignature` 断言 `Valid`
- **Android APK 签名与校验**（`scripts/sign-android.sh`）：`apksigner` 同时开
  v1/v2/v3，签完立即 `verify --print-certs`
- `docs/signing.md`：三平台签名现状（如实登记）、校验步骤、补签方法
- CLI / TUI 新参数：`--max-pps` / `--max-targets` / `--allow-reserved`，
  CLI 另加 `--audit`

### 变更

- 调度器持有 `ScanPolicy`（用 `set_policy` 装配）。挂在调度器上而不是塞进
  `ProbeConfig`：限速是**本机合规策略**，不该被一份从别处拷来的模板改掉
- CLI 摘要只在**限速被改动**时才显示策略；默认值写进每一行只会变成噪音，
  而「限速被关掉」需要摆在明面上
- TUI 策略来自命令行，装配失败写进**状态栏**（不 panic、不静默）；
  切换探测方式时**重新装配**（否则切一次就把限速丢了）

### 说明（明确不做的事）

- **不伪造签名**：没有密钥时 `MANIFEST.json` 写 `"signed": false` 并打印补签
  命令。**绝不用自签证书冒充正式签名**（自签证书在用户机器上照样弹
  「未知发布者」，冒充只会让人误判发布状态），也**绝不把 SHA-256 说成签名**
  ——sha256 只能证明「文件没被改动」，不能证明「是谁发布的」
- Android 未配置 keystore 时，脚本如实报告「当前是 Android Debug 密钥签名，
  可侧载、不可上架」。debug 口令是公开的，把这种包当正式版发出去等于让
  「这个包是我们发的」这条保证作废
- GUI 侧的策略设置随 M18 的界面工作接入；桌面端**用无条件默认值**，
  因为桌面用户最不可能去读文档

## [2.4.0] - 2026-10-15

M16 配置与模板：命名扫描模板、配置完整往返（含未知键保留）、schema 版本与迁移、
CLI 直接消费模板。**规则全在 `cping-core::profile`，CLI 只做接线。**

### 新增

- **配置文件 `cping.toml`**（`cping-core::profile`）：
  - 顶层 `schema_version` + `default_profile`
  - `[[profiles]]` 命名模板：`name` / `spec` / `range` / `timeout_ms` /
    `concurrency` / `mode` / `tcp_port` / `note`
  - `[probe]` / `[colors]` / `[ui]` / `[monitor]` 应用配置（借 `AppConfig`
    现有读写实现，不另起一份）
- **完整往返**：`ConfigDocument::to_toml` / `from_toml` 逐字节稳定；
  **不认识的键原样保留**（连陌生小节一起，写回位置也稳定）；
  注释**不保留**——受限子集解析器不建 AST，这一点在文件头明写
- **schema 版本与迁移**：`schema_version = 1`（只有 `[probe]` 的老写法）读入时
  自动升级到 v2，把 `[probe].prefix` 提升为名为 `default` 的模板并设为默认，
  **迁移动作逐条打印**；`schema_version` 高于本程序支持值时**拒绝读取**
  （猜着读会吃掉用户不认识的项）
- **模板校验**：名字非空、不含会破坏 TOML 结构的 `[ ] = "`；
  `spec` 必须**真能 `parse_spec`**——模板最大的价值是「一键调用」，
  存进去一个写错的规格等于把这个错误固化下来、每次调用都踩一次
- **合并优先级（高 → 低）：命令行 → 模板 → 配置 `[probe]`**，
  且**每一层提供了哪些字段都逐行打印**（`配置来源：…`）。
  模板只填空缺，绝不覆盖用户在命令行上手敲的值；被盖掉的配置值也一并说明。
  **值相同不算冲突**——同一件事说两遍只会把真正重要的说明淹掉
- **`--profile` 点名的模板不存在就报错退出**，并列出所有现有模板名，
  不会悄悄退回 `default_profile`
- CLI 新参数：
  - `--config <路径>` / `--profile <名>`
  - `--list-profiles`：摊开每份模板的**可核对项**（目标规格、解析出的目标数量、
    range / 超时 / 并发 / 探测方式、note），并标出 `default_profile`；
    列表一格写「目标规格无法解析」而**不猜数字**
  - `--dump-config <路径>`：把**这次实际生效**的参数固化成配置文件。
    同名模板就地更新（保住 `note` 与陌生的键），并设为 `default_profile`，
    保证「`--config <路径>` 单独一条就能复现」；覆盖了原有规格会明确提示。
    只写「扫什么、怎么扫」——导出路径 / `--history` / 通知通道 / 告警规则
    **刻意不写**，否则换个模板会把通知一起换掉
- 仓库新增 `cping.example.toml`（三份模板 + 全部应用配置 + 逐条说明）

### 变更

- `TargetRange::parse_key` / `label_zh`：名字写错返回 `None`，
  **不回落成 `Standard`**——配置里写错一个范围却静默按 255 个目标去扫，
  是那种「跑完才发现扫错了」的事故
- CLI 摘要（`describe()`）带上配置文件与模板名：模板决定扫哪个网段，
  不写出来的话，同一份命令行换个默认模板就会扫出完全不同的结果
- 帮助文本新增 `--config / --profile / --list-profiles / --dump-config` 四项
  与「关于 `--config` / `--profile`」整段说明

### 修正

- **CLI 会话横幅从未打印过**：`run_rounds()` 里的 `self.round == 1` 判断发生在
  循环自增**之前**（`round` 从 0 开始），条件永不成立，`CPing <版本> · <平台> ·
  <参数摘要>` 那一行一次也没出现过。改为 `self.round == 0`。
  横幅里带着 `describe()`（本次用的是哪份模板），不该静默消失

## [2.3.0] - 2026-10-15

M15 告警与通知：阈值告警、去重冷却、静音、五通道通知。**核心逻辑全部在
`cping-core`（`alerts.rs` / `notify.rs` / `timefmt.rs`），CLI / TUI / GUI 三端只做接线。**

### 新增

- **告警规则**（`cping-core::alerts`）四类，全部是**纯函数** `match_rule(rule, ctx) -> Option<AlertEvent>`：
  - 边沿类：`Appeared`（首次观测到且在线）、`Disappeared`（上一轮在线、本轮离线）
  - 电平类：`LatencyAbove`（延迟超阈）、`UptimeBelow`（累计可用率低于阈值）
  - 严重度三档（提示 / 警告 / 严重），事件排序按「严重度 → 主机序号」，
    不让「哪条更该先看」交给运气
- **三条定死的边界**（都有回归测试盯着）：
  1. **第一轮不发 `Appeared`**：第一轮只建立基线。不这么做的话，扫一个 /24
     会一次刷出 255 条「新增主机」，等于把告警功能做成噪音源
  2. **阈值用严格不等号**：`>` 与 `<`，**恰好等于阈值不告警**
  3. **静音不改变判定**：静音时规则照常求值、`last_fired` 照常推进，只是不返回事件。
     因此**解除静音不会补报一堆积压告警**（这是「静音」与「暂停」的本质区别）
- **去重与冷却**：同一 `(类型, 主机)` 在冷却窗口（默认 300 秒）内只报一次；
  冷却记录可按主机、按类型分别生效；`suppressed_by_cooldown` / `suppressed_by_mute`
  两个计数器把「被挡下多少条」如实记出来，而不是静默吞掉
- **通知五通道**（`cping-core::notify`），**通道失败必须可见**：
  - `console`：标准输出（任何平台可用）
  - `system`：Windows Toast（PowerShell `-EncodedCommand`，UTF-16LE + Base64，
    避免命令行中文编码炸掉）/ Linux `notify-send`
  - `webhook`：HTTP POST JSON，**仅 `http://`**——本构建不含 TLS，
    `https://` 会被**明确拒绝**并提示改用 `--notify-cmd`，绝不静默降级成明文
  - `command`：执行外部命令，事件 JSON 从 stdin 喂入（接 curl / msmtp 等自带 TLS 的工具）
  - `smtp`：**明文** SMTP 中继（不支持 TLS 与认证），仅适用于内网
  - 模板占位符 `{title}` / `{body}` / `{json}` / `{count}`，**整词替换**
- **非阻塞投递**：`AlertSink` 后台线程 + `sync_channel(64)`。队列满时**丢弃并计数**
  ——告警投递永远不许拖住扫描主流程，但丢了多少必须说出来
- **状态落盘**（`AlertEngine::to_text` / `from_text`）：首行 `CPING-ALERTS <版本>`，
  版本不符显式拒绝；坏行跳过并计数，不让整份状态作废
- CLI：`--alert` / `--alert-kinds` / `--alert-latency` / `--alert-uptime` /
  `--alert-cooldown` / `--alert-state` / `--mute-alerts`，
  以及 `--notify` / `--notify-webhook` / `--notify-cmd` / `--notify-smtp` /
  `--notify-mail-from` / `--notify-mail-to` / `--notify-list`
- TUI：`A` 键打开告警面板（**首次按 `A` 就地启用告警**，不必重启加参数），
  `!` 切静音；状态栏显示告警摘要
- GUI：新增「告警」页签（桌面端另加侧栏按钮，移动端加入底部页签）：
  规则开关、阈值/冷却表单、静音开关、事件列表；扫描结束时后端推
  `alert-event` 事件，前端据此就地提示并刷新

### 变更

- **时间算法下沉到 `cping-core::timefmt`**：`civil_from_days` 此前在 CLI / TUI / GUI
  各有一份拷贝（三份都可能各自漂移），现在只剩一处，三端改为转发。
  `format_unix_local` / `unix_secs_now` / `unix_millis_now` 同源
- `--notify` 与 `--notify-*` 的关系收敛成两条可解释的规则：
  1. 给了 `--notify` → **就是**这几条通道（多一个不给、少一个不补）
  2. 没给 `--notify` → `console`，外加任何「给了参数」的通道
  - 第 2 条的意思是 `--notify-cmd …` 单独出现时**一定**生效。静默忽略用户
    显式给出的参数是本项目里最不能接受的一类 bug
- **通道误用一律报错**，不静默降级：点名了通道却没给必填参数（`--notify webhook`
  缺地址、`--notify command` 缺 `--notify-cmd`）、给了参数却没在 `--notify` 里
  选上它、SMTP 没写收件人——三类都是退出码 2
- `AlertLog::clear()` 连 `total_seen` 一起归零：界面上点了「清空」却还写着
  「累计 5 条」，用户只会以为按钮坏了
- GUI 的「清空告警记录」**只丢事件**，不动 `seen` 基线与冷却记录——
  动了基线，下一轮就会把整段在线主机当成「新增」重报一遍

### 修正

- **告警阈值与分类阈值容易被混为一谈**：`AlertEngine::evaluate` 的签名上挂着
  `Thresholds`（分类用的低/中/高延迟档位）却完全不读，容易被误以为
  「告警阈值 = 分类阈值」。保留参数以维持与三端调用点的一致，但补上文档写清：
  告警阈值在 `AlertRule::threshold`，与 `classify::Thresholds` **无关**

### 验证

- 测试：**393 passed**（较 v2.2.0 的 336 增加 57 条：`alerts.rs` 内 27 条 +
  `notify.rs` 内 22 条 + CLI 参数 5 条 + TUI 3 条）
- **真实冒烟（本机，非模拟）**：
  - 目标 `1.1.1.1`（实测 271 ms），`--alert-latency 200` 触发 1 条告警；
    **不给 `--notify`、只给 `--notify-webhook` 与 `--notify-cmd`**，
    三条通道同时生效：`console` 打印、`webhook` POST 到本机 HTTP 服务收到 200、
    `command` 从 stdin 收到 JSON。两处收到的 JSON 逐字段一致：
    `kind/kindLabel/severity/ip/index/atMs/time/round/value/threshold/message`
  - `--notify webhook` 但地址写成 `https://` → 通道**明确失败**并报出原因，
    退出码与提示引导用户改用 `--notify-cmd`
  - 四类误用全部以退出码 2 拒绝（缺地址 / 缺 `--notify-cmd` / 地址没被 `--notify` 选上 /
    SMTP 缺收件人）；`--notify telegram` 退出码 2
  - `--notify-list` 正确列出本平台五通道能力（Windows 上 `system` 可用）
  - 告警状态跨会话：会话 B 被冷却挡下、会话 C 静音，状态文件内容与计数一致
- `cargo fmt --check` / `cargo clippy --workspace --all-targets -- -D warnings`
  两套工作区干净（GUI 是独立工程，另跑一遍同样干净）
- 前端 `vue-tsc --noEmit` 0 错误、`npm run build` 通过
- 未实测：TUI 的 `A` / `!` 交互未做人工键盘冒烟（由单元测试直接驱动 `on_key`
  与告警求值路径覆盖）；GUI 未做真机点击冒烟（仅编译 + 类型检查 + 构建通过）

## [2.2.0] - 2026-10-15

M14 历史与趋势：长期时序、可用率报表、离线/在线率曲线。**核心逻辑全部在
`cping-core`（`timeseries.rs`），CLI / TUI / GUI 三端只做接线。**

### 新增

- **长期时序**：每轮扫描追加一条精简记录（时间戳 + 每目标的观测数/在线数/
  RTT 求和/最差 RTT），可跨会话累积
  - 存储格式**版本化**：文件头 `CPING-TS <version>`，版本不符时**显式拒绝读取**
    （「尽力猜着读」会静默污染报表）；个别坏行只跳过并计数，不让整份历史作废
  - 环形保留：默认保留 7 天（`--keep-days` 可调），裁剪基准是**最新点**而不是
    「现在」——否则隔几天打开一份历史文件会被一次清空，那是数据丢失
  - 分级降采样：点数超过上限时把**最旧的一半**两两合并（近期分辨率更有价值），
    合并只做同类项相加，`samples`/`online`/`rtt_sum` **逐条守恒**
- **可用率报表**：按主机统计观测轮数、在线轮数、可用率、平均/最差 RTT、
  抖动（相邻点平均 RTT 之差的平均绝对值），导出 CSV / HTML
  - **从未在线的主机 RTT 记为空而不是 0**：0 会被读成「极快」，这是实打实的误报。
    回归测试专门盯住这条（回环地址 RTT 本来就是 0，拿 0 当哨兵就会误判）
- **曲线**：核心提供两个纯函数，三端各取所需
  - `sparkline_svg`：GUI 用，纯 SVG 折线（不引图表库），**无 `<script>`**
  - `char_bar_chart`：TUI 用，8 档字符柱（`▁▂▃▄▅▆▇█`）
  - 两处一致：某轮**没测到**某台主机 → 画成断线 / `·`，**不是** 0、也不是「离线」
- CLI：`--history <路径>`（跨会话累积，每轮立即落盘）、`--report <路径>`
  （`.csv` / `.html`）、`--keep-days <天>`
- TUI：`U` 键打开「历史与趋势」面板，每台主机一行字符柱状在线率曲线
- GUI：新增「趋势」页签（含侧栏按钮），每台主机一张卡片：可用率标签 +
  SVG 折线 + 在线/观测、平均/最差 RTT、抖动；可导出 CSV / HTML、可清空

### 变更

- 时序点的时间戳取**本轮开扫时刻**，而不是「扫描结束的现在」——
  一轮本身可能跑几秒，用结束时刻会让曲线的时间轴一点点被拉长
- `--keep-days` 在未同时指定 `--history` 时报错（此前会静默忽略）：
  「我明明设了 30 天，怎么还在按 7 天裁」这种困惑必须用报错挡掉
- 移动端趋势页不显示导出按钮：`plugin-dialog` 的 `save` 在 Android 上没有
  原生实现，点下去只会抛错——不如不显示

### 验证

- 测试：336 passed（+27：`timeseries.rs` 内 17 条 + CLI 参数 4 条 + TUI 5 条 +
  核心零值边界 1 条）
- 真实冒烟（本机回环）：`-n 3 -w 1 --history net.ts --report uptime.html`
  落盘 3 个时间点、每点带 `span_ms`；**第二次会话续写**同一文件后累计 5 轮，
  CSV 报表显示 `127.0.0.1,5,5,100.0,...`；HTML 报表正常渲染
- `cargo fmt --check` / `cargo clippy --all-targets -- -D warnings` 两套工作区干净
- 前端 `vue-tsc --noEmit` 0 错误、`npm run build` 通过

## [2.1.0] - 2026-10-15

M13 资产识别：端口指纹、服务横幅、设备类型推断。**核心逻辑全部在 `cping-core`
（`fingerprint.rs`），CLI / TUI / GUI 三端只做接线，不重复实现。**

### 新增

- **端口指纹**：对在线主机并行 TCP 连接一组常见端口（默认 28 个，可用
  `--fp-ports` 覆盖，上限 512），输出开放端口列表与各自的握手耗时
  - 每个端口**独立超时**（默认 800 ms，`--fp-timeout` 可调）
  - 单主机内部按 `max_concurrent` 并行，主机之间串行——对整网段同时开
    端口扫描会打满上行，也会在对端留下明显的同源并发扫描痕迹
- **服务横幅**：连接开放端口后**只读**对方主动送出的前 256 字节，
  **不发送任何协议请求**；结果原样保存并清洗为可打印文本（`--no-banner` 关闭）
- **设备类型推断**：基于「厂商 + 开放端口组合」给出一条**推断**结论，
  带置信度（低/中，**没有「高」**——TCP 指纹给不出确定结论，写了就是撒谎）
  与逐条依据；未识别的组合返回空，**绝不臆造**
- CLI：`--fingerprint`（别名 `--fp`）、`--fp-ports`（支持 `22,80,443-445` 写法）、
  `--no-banner`、`--fp-timeout`
- TUI：`F` 键对选中主机做指纹（无选中行时对全部在线主机），结果在独立面板展示
- GUI：新增「资产」页签与侧栏按钮，可勾选是否抓横幅、一键清空

### 变更

- 导出新增**末尾两列**（CSV）/ **独立小节**（TXT）/ **独立数组**（JSON）/
  **独立分区**（HTML）：
  - CSV 第 10/11 列 `开放端口`、`设备推断`；前 9 列**逐字节不变**
  - JSON 顶层新增 `fingerprints` 数组，其中 `device.is_inference` 恒为 `true`，
    让机器消费者也无法把推断当成事实
  - TXT / HTML 的小节标题写明「附加信息，与在线判定相互独立」
- 指纹结果**不进入 `ProbeResult`**：端口扫描是独立的一趟探测，
  混进结果模型会让「在线」出现第二个来源

### 关于「分列」这条边界

三端与四种导出格式一致遵守：**端口全关不会把主机判成离线，端口开放也不会
把离线主机变成在线**。CSV 里指纹只出现在末尾附加列，TXT/HTML 里只出现在
标题写明「与在线判定相互独立」的独立小节。有测试逐字节断言「加上指纹后
既有列不变」。

### 验证

- 测试：309 passed（+33：`fingerprint.rs` 内 19 条 + 导出契约 7 条 +
  CLI 参数 3 条 + TUI 4 条）
- 真实冒烟（本机回环）：起一个连接后主动问候的服务，`--fingerprint
  --fp-ports 18555,1` 正确识别 `18555` 开放、`1` 未开放，抓到横幅
  `SSH-2.0-CPingSmokeTest`；`--no-banner` 下同一端口横幅为空
- `cargo fmt --check` / `cargo clippy --all-targets -- -D warnings` 两套工作区干净
- 前端 `vue-tsc --noEmit` 0 错误、`npm run build` 通过

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

[2.1.0]: https://github.com/LingBrian/cping/releases/tag/v2.1.0
[2.0.0]: https://github.com/LingBrian/cping/releases/tag/v2.0.0
[1.1.2]: https://github.com/LingBrian/cping/releases/tag/v1.1.2
[1.1.1]: https://github.com/LingBrian/cping/releases/tag/v1.1.1
[1.1.0]: https://github.com/LingBrian/cping/releases/tag/v1.1.0
[1.0.0]: https://github.com/LingBrian/cping/releases/tag/v1.0.0
