<div align="center">

# CPing

**跨平台局域网批量 PING 工具 —— 一套业务逻辑，三种界面**

Rust 核心 · TUI（ratatui）· 桌面 GUI（Tauri 2 + Vue 3）· Android

[![Release](https://img.shields.io/badge/release-v2.6.0-111827)](../../releases)
[![CI](https://github.com/LingBrian/cping/actions/workflows/ci.yml/badge.svg)](https://github.com/LingBrian/cping/actions/workflows/ci.yml)
[![Tests](https://img.shields.io/badge/tests-582%20passed-brightgreen)](docs/testing.md)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20Android-lightgrey)](docs/platform-compatibility.md)

现代重写版：原软件是 2004 年的 Windows 小工具「C网群PING工具」（Delphi + Synapse）。
本项目先对原版做逆向行为分析（含 Frida 动态复核确证），再按同等交互习惯用现代技术栈重写。

</div>

## 截图

**桌面 GUI（Windows，真实 ICMP 扫描回环网段 127.0.0.x，255/255 在线，用时 0.03s）**

| 矩阵视图 | 表格视图 |
| --- | --- |
| ![GUI 矩阵视图](docs/images/gui-matrix.png) | ![GUI 表格视图](docs/images/gui-table.png) |

**TUI 终端版（同一扫描，`windows-icmp` 后端）**

| TUI 矩阵视图 | 表格视图 |
| --- | --- |
| ![TUI 矩阵](docs/images/tui-matrix.png) | ![TUI 表格](docs/images/tui-table.png) |

**M7：目标信息与快照恢复**（扫描结果回答「这是谁」，不只是「在不在线」）

![启动时发现上次结果，默认不自动恢复](docs/images/m7-gui-restore-prompt.png)

**Android 移动布局**（真实 Android 运行时截图）

| 列表视图 | 矩阵视图 | 横屏 |
| --- | --- | --- |
| ![Android 列表](docs/images/android-list.png) | ![Android 矩阵](docs/images/android-matrix.png) | ![Android 横屏](docs/images/android-landscape.png) |

> Android 截图摄于 Android 15 模拟器（WHPX 硬件加速）中运行的 APK 本体——
> 与发布产物同一份 Rust 核心 + 前端，非浏览器仿真。截图中全部目标显示
> 「权限不足」是**正确行为**：Android 普通应用无 `CAP_NET_RAW`，无法创建
> raw ICMP 套接字，本工具如实分类而不假装离线（真机上同理）。
> Android 上要扫描局域网请切换到 **TCP 方式**——它不需要特权，
> 且 v2.0.0 起 Android 的默认探测方式就是 TCP（`TCP-OPEN` 与 ICMP 的「在线」分开标注）。

## 为什么重写

| | 原版（2004） | 本项目 |
| --- | --- | --- |
| 技术 | Delphi + Synapse，ASPack 加壳 | Rust + Tauri 2 / ratatui，无壳无混淆 |
| 平台 | 仅 Windows | Windows / Linux / Android |
| 探测 | WinSock ICMP | 平台原生 ICMP 后端抽象（可插拔） |
| 界面 | 单一固定布局 | 桌面矩阵/表格 + 移动列表优先布局 |
| 核心逻辑 | 界面与逻辑耦合 | `cping-core` 独立 crate，三端共用 |

原版确证行为（目标范围 `.1`–`.255`、矩阵行列映射、四档时延配色与符号等）见
[`docs/reverse-findings.md`](docs/reverse-findings.md)。

## 架构

```
cping/
├── crates/cping-core/        共享核心（零第三方运行时依赖）
│   ├── spec/target/result/classify     目标规格解析 · 生成 · 结果模型 · 分类
│   ├── probe/scheduler                 ProbeBackend 抽象 + std 线程并发调度
│   ├── registry                        M12：后端注册表（icmp4/icmp6/tcp/mock）与能力
│   ├── asynchronous                    M12：可选异步调度（`async` feature，默认关）
│   ├── monitor/history                 M8：轮次对比 · 变化日志 · 逐轮历史
│   ├── json/html/diff/export           M10：四种导出格式与两次扫描对比
│   ├── fingerprint                     M13：端口指纹 · 服务横幅 · 设备类型推断（纯数据驱动）
│   ├── timeseries                      M14：长期时序 · 降采样 · 可用率报表 · 折线与字符曲线
│   ├── alerts                          M15：告警规则（纯函数匹配）· 冷却去重 · 静音 · 状态落盘
│   ├── notify                          M15：五通道通知（console/system/webhook/command/smtp）
│   ├── timefmt                         M15：时间算法（三端共用，同一处实现）
│   ├── profile                         M16：配置文件 · 命名模板 · schema 版本与迁移 · 未知键保留
│   ├── policy                          M17：令牌桶限速 · 保留网段守卫 · 审计日志 · 首次授权告知
│   ├── i18n                            M18：中英双语字典 · 语言协商 · 漏译自动检查
│   ├── oui/neighbor/hostname/enrich    M7：厂商识别 · ARP 取 MAC · 反查主机名 · 信息补全
│   ├── snapshot                        结果快照落盘与恢复（严格解析，坏数据宁可报错）
│   ├── data/                           IEEE OUI 三表（54,190 条，编译期嵌入）
│   └── native.rs                       Windows/Linux/Android 原生 ICMP 与 TCP
├── apps/cping-tui/           ratatui + crossterm 交互式终端界面
├── apps/cping-cli/           M10/M16/M17：无头命令行（可判退出码）· 配置模板 · 安全策略与审计
├── apps/cping-gui/           Tauri 2 + Vue 3 + TypeScript
│   ├── src/                  桌面布局 + 移动布局（自动切换）
│   └── src-tauri/            Tauri 容器（唯一前后端通道）
├── scripts/                  M11：打包脚本（tar.gz / zip + SHA256SUMS）
│                             M17：制品清单与签名（manifest / Authenticode / apksigner）
├── tools/oui-gen/            IEEE 官方 CSV → 嵌入数据表的确定性生成器
└── tools/android-forensics/  Android 平台行为取证对照程序（不参与构建）
```

**逻辑只有一份**：GUI 经 Tauri Commands、TUI 与 CLI 直接调用，都走同一个 `cping-core`。
目标生成、调度、分类、统计、导出、后端选择不在任何界面里重复实现。

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
npm run typecheck && npm run build
cd src-tauri && cargo build --release

# 命令行（无头）
cargo run -p cping-cli -- --target 192.168.1 --mode tcp --port 80

# Android APK（默认构建全部已安装的 ABI；需 ANDROID_HOME / NDK_HOME / JDK 17，见 docs/release.md §2）
cd apps/cping-gui && npm run tauri:android:build
```

> 常见构建坑（cargo 离线沙箱、npm postinstall、Android 符号校验等）的排查表
> 见 [README 附表](#构建环境注意事项) 与 [`docs/testing.md`](docs/testing.md)。

## 使用

### 命令行（无头，适合脚本与计划任务）

```bash
cping-cli --target 192.168.1 --mode icmp
cping-cli -t "192.168.1.0/24, ::1" --mode tcp --port 443 --out report.html
cping-cli -t 192.168.1 --watch 30 --rounds 10 --quiet
cping-cli --list-backends          # 本平台有哪些后端可用、支持哪些地址族
cping-cli -t 192.168.1 --fingerprint --out assets.csv   # M13：端口指纹 + 设备推断
cping-cli -t 192.168.1 --fingerprint --fp-ports 22,80,443 --no-banner
# M14：把每轮追加到长期时序（跨会话累积），结束时出可用率报表
cping-cli -t 192.168.1 --watch 60 --history net.ts --report uptime.html
cping-cli -t 192.168.1 --watch 300 --history net.ts --keep-days 30 --report uptime.csv
# M15：告警与通知（阈值 + 新增/离线；五通道，失败一定可见）
cping-cli -t 192.168.1 --watch 30 --alert --alert-latency 200 --alert-state alerts.txt
cping-cli -t 192.168.1 --watch 30 --alert --notify console,system
cping-cli -t 192.168.1 --watch 30 --alert --notify webhook --notify-webhook http://10.0.0.5:9000/cping
cping-cli -t 192.168.1 --watch 30 --alert --notify-cmd "curl -s -d {json} https://example.com/h"
cping-cli --notify-list            # 本平台各通知通道的可用性与原因
# M16：配置文件与命名模板（CI / 巡检脚本复用同一份）
cping-cli --config prod.toml --list-profiles      # 看有哪些模板、各扫多少目标
cping-cli --config prod.toml --profile dmz --out dmz.csv
cping-cli --config prod.toml                      # 不给 --profile 就用 default_profile
cping-cli -t 10.0.0.0/24 -c 64 --dump-config nightly.toml   # 把生效参数固化下来
# M17：安全与合规（限速与保留网段守卫是默认行为，不是选配）
cping-cli -t 192.168.1 --max-pps 200 --max-targets 512      # 速率上限（pps）/ 目标数上限
cping-cli -t 192.168.1 --watch 30 --audit scan-audit.jsonl  # 追加审计记录（JSONL，可导出自证）
cping-cli -t 192.0.2.5 --allow-reserved                     # 显式放行保留网段（默认拒绝）
# M18：中英双语（不给 --lang 则跟随 CPING_LANG / LC_ALL / LC_MESSAGES / LANG）
cping-cli --lang en --help                                  # 英文帮助与英文报错
CPING_LANG=en cping-cli -t 192.168.1                        # 也可以用环境变量
```

退出码：`0` 全部在线 / `1` 存在离线 / `2` 参数错误 / `3` 权限不足或后端不可用。

> **双语（M18）**：三端都能说中英两种话。核心层产出的是**键 + 参数**
> （错误还额外有稳定的 `code()`，脚本判断错误类别时用它，
> **不要匹配消息文本**——那句话随时会因为改措辞而变化），句子在界面侧渲染。
> 覆盖范围已从「枚举标签」扩到**自由文本**：目标规格摘要、变更描述、
> **告警正文**、设备类型推断（名字与依据）、后端能力说明、平台 ICMP 提示、
> 通知通道说明、端口列表解析错误、时序与告警状态文件的读写错误——
> 三端（含 GUI 的 Rust 侧）都按当前语言产出。
> 字典漏译是**红灯而不是等用户截图**：英文条目里出现中日韩字符、
> 中英条目 `{}` 个数不等、`TABLE` 与键序号不是双射，任何一条都会让测试失败。
> **刻意不翻译**的是：CSV / JSON 的列名与字段名、快照与时序的文件格式、
> 状态符号（`#` `+` `*` `-` `!` `.`）、命令行参数名、操作系统的原始错误详情，
> 以及代码注释——前几项是**数据契约**，同一份报表在中英文机器上必须一致。
> `--lang` 给了不认识的值会**报错退出**（退出码 2），不会静默退回中文；
> 环境变量里的 `LC_ALL=C` / `POSIX` 表示「没有偏好」，
> 按默认中文处理，而**不是**当成「请用英文」。

> 一次任务**只用一种探测方式**：ICMP 的「在线」与 TCP 的「开放」（`TCP-OPEN`）
> 不会混进同一列统计。IPv6 目标目前请用 TCP（ICMPv6 未实装，如实登记为不可用）。
>
> `--fingerprint` 是**独立于在线判定的第二趟探测**：端口全关不会把主机改成离线，
> 端口开放也不会把离线主机变在线。横幅只读对方主动送出的前 256 字节，
> **不发送任何协议请求**；设备类型是「厂商 + 端口」的**推断**结论，识别不出就留空。
>
> `--history` 写出的时序文件**带格式版本号**，版本不符时显式拒绝读取；每轮都立即
> 落盘，Ctrl+C 也不丢已跑过的轮次。`--report` 的数值可直接对账回原始记录：
> 降采样只做同类项相加，观测轮数与 RTT 求和在合并前后守恒。
> **从未在线的主机 RTT 记为「-」而不是 0**——0 会被读成「极快」。
>
> `--alert` 的规则匹配是**纯函数**，三条边界写死：**第一轮不发「新增主机」**
> （否则扫个 /24 会一次刷出 255 条）；阈值用**严格不等号**（恰好等于阈值不告警）；
> **静音不是暂停**（规则照常求值、冷却照常推进，所以解除静音不会补报积压告警）。
> 通知**通道失败必须可见**：每通道单独汇报成功/失败与原因，投递在后台线程
> （队列满丢弃并计数），绝不阻塞扫描。`webhook` 只支持 `http://`——
> 本构建不含 TLS，`https://` 会被**明确拒绝**而不是静默明文发送；
> 需要 https 请用 `--notify-cmd` 调 curl 等自带 TLS 的工具。
>
> `--config` 把「扫什么、怎么扫」固化成一份份**命名模板**（`[[profiles]]`，
> **双方括号**；写成 `[profiles]` 会明确报错），模板段之外的 `[probe]` / `[colors]` /
> `[ui]` / `[monitor]` 是三端共用的应用配置。合并优先级**高 → 低**：
> **命令行 → 模板 → `[probe]`**——模板只填空缺，**绝不覆盖你在命令行上手敲的值**，
> 而且每次都会把「哪些字段来自哪一层、哪条命令行盖掉了配置」逐行打印出来。
> `--profile` 点名的模板不存在会**报错退出**（不会悄悄退回 `default_profile`）。
> 配置文件带 `schema_version`：老文件（v1，只有 `[probe]`）自动迁移并打印迁移动作，
> 比本程序更新的版本**拒绝读取**。**不认识的键原样保留，但注释不保留**
> （受限子集解析器不建 AST——这条写在生成文件的第一行里）。
> 可直接从 [`cping.example.toml`](cping.example.toml) 改起。
>
> **安全与合规（M17）**：**默认就限速**（`500` pps）且**默认拒绝保留 / 特殊用途网段**
> （组播、`0.0.0.0/8`、`240/4`、三个文档示例网段、`198.18/15`、IPv6 文档段与组播等），
> 被拦时会说明**是哪一类地址、为什么拦**，而不是笼统说「非法」。私有网段、回环、
> 链路本地与 CGNAT **不拦**——它们正是本工具的正当用途。`--allow-reserved` 可显式放行。
> 限速、目标数上限、是否放行保留网段这三项**只走命令行、刻意不进配置文件**：
> 一份从别处拷来的模板不该能把限速改成「不限速」。`--max-pps 0` 关闭限速。
> **首次运行会打印一次授权告知**（仅在告知版本变化时再提示，且不阻塞扫描），
> 确认后落盘到 `~/.cping/consent`。`--audit` 写出的 JSONL **只含时间、参数、计数
> 与操作者名，不含 MAC / 厂商 / 主机名结果**——自证合规用不着把邻居的隐私一起记下来。
> 制品签名的真实现状（哪些有签名、哪些只是 SHA-256）如实登记在
> [`docs/signing.md`](docs/signing.md)。

### TUI

```bash
cping-tui                # 交互式启动
cping-tui --prefix 192.168.1
cping-tui --max-pps 200 --max-targets 512   # M17：限速与目标数上限（取值不合法只警告不崩）
cping-tui --lang en                         # M18：英文界面（TUI 是交互式的，非法值只告警）
cping-tui --theme high-contrast             # M18：高对比主题（对比度 ≥ 7:1；另有 color-blind）
cping-tui --no-color                        # M18：完全不要颜色（保留加粗 / 反显 / 弱化）
cping-tui --font-scale 200                  # M18：按 200% 字号放宽「窗口太小」的判定
cping-tui --config cping.toml               # M16：从配置文件读模板（读不了只在状态栏提示，不崩）
```

> **`P` 打开模板面板、`G` 打开策略面板**（M16 / M17 的界面入口）。
> 模板的合并优先级与 CLI 一致：**命令行显式参数 > 模板 > 配置 `[probe]`**，
> 模板只填空缺、绝不覆盖你显式给的值。策略面板里越界的值**可见且不生效**，
> 不夹取也不静默忽略；打开「允许保留网段」时会有显式警示。

| 键 | 功能 | 键 | 功能 |
| --- | --- | --- | --- |
| `Enter` | 开始探测 | `g` / `t` | 矩阵 / 表格视图 |
| `i` | 编辑网段前缀 | `f` | 过滤不在线目标 |
| `Esc` | 取消 / 关闭帮助 | `s` / `S` | 导出 CSV / TXT |
| `m` | 补全 MAC / 厂商 | `n` | 反查当前行主机名 |
| `w` | 保存快照 | `l` | 从快照恢复 |
| `T` | 切换 ICMP ↔ TCP | `p` | 轮换常用 TCP 端口 |
| `F` | 端口指纹（选中主机；无选中则全部在线主机） | `M` | 开/关持续监控 | `e` | 查看变化事件流 |
| `U` | 历史与趋势（每台主机一条在线率字符柱曲线） | `[` / `]` | 缩短 / 延长重扫间隔 |
| `A` | 告警记录（**首次按即启用告警**） | `!` | 告警静音开关（不改判定） |
| `?` / `F1` | 帮助 | `q` | 退出（运行中需再按一次确认） |
| `Ctrl+C` | 强制退出并恢复终端 | | |

状态符号：`#` 低延迟 · `+` 中延迟 · `*` 高延迟 · `-` 无回应 · `!` 错误 · `.` 未探测。
**颜色不是唯一标识**：无颜色终端下符号仍然可区分。

趋势面板里的曲线用 8 档字符柱（`▁▂▃▄▅▆▇█`）表示每轮的在线率；`·` 表示该轮
**没测到**这台主机（不是「离线」）。TUI 的趋势只累积在本进程内——需要跨会话的
长期历史请用 `cping-cli --history`，两者共用同一套时序核心。

告警默认**关闭**（不问就发通知比不装还糟）。按 `A` 打开面板会**就地启用**
（不必重启加参数）；`!` 切静音——静音的只是通知，判定与冷却照常推进。

可选 `--snapshot-dir <目录>` 指定快照目录（默认当前目录）。

TUI 与 CLI 共用同一套安全策略（M17）：默认限速、默认拒绝保留网段，首次运行打印
同一个授权告知（**打印在进入备用屏幕之前**，否则确认文字会随备用屏幕一起被清掉）；
切换探测方式时会重建调度器并**重新装配策略**——不然切一次方式限速就丢了。

### 桌面 GUI

输入 C 类网段前三段 → 开始探测；矩阵/表格切换、仅在线过滤、时延配色自定义、
保存结果为 CSV（UTF-8 BOM，Excel 直接打开不乱码）或 TXT。

**目标信息（M7）**：扫描结束自动读本机邻居表（ARP）回填 MAC，并按 IEEE 注册数据
（54,190 条，离线内置）显示厂商；表格里每行可按需反查主机名（阻塞操作所以只在
点击时对单行发起）。查不到就留空——不猜、不填占位文本。

**结果快照**：每轮扫描结束自动落盘一份快照；下次启动若发现它会先问是否恢复，
**默认不恢复**（丢弃 / 暂不恢复 / 恢复结果三个出口）。存在的理由是 Android 上
进程会被系统随时回收（销毁 Activity 即退出进程），内存里的结果不可靠。

**趋势（M14）**：每轮扫描都会并入长期时序，「趋势」页签按主机显示可用率、在线/观测
轮数、平均与最差 RTT、抖动，以及一条内嵌 SVG 在线率折线（纯函数产出，**无脚本**）。
曲线断开处表示该轮**没测到**这台主机，不是「离线」。可导出 CSV / HTML 报表并清空。
移动端同一页签，但隐藏导出按钮（`plugin-dialog` 的 `save` 在 Android 上无原生实现）。

**告警（M15）**：「告警」页签可开关告警（默认关）、逐条启用/停用规则、改阈值与
冷却、一键静音、清空记录。事件表按「时间 / 类别 / 主机 / 说明 / 实测值与阈值 / 轮次」
列出。每轮扫描结束时后端推 `alert-event` 事件，界面据此提示并刷新。
「清空记录」**只丢事件**，基线与冷却记录保留——否则下一轮会把整段在线主机
当成「新增」重报一遍。

### Android

列表优先的触控布局：网段输入（数字键盘）→ 开始探测；列表/矩阵/变化/趋势/告警页签、
仅在线开关、导出到应用专属外部目录（回报完整路径，Android 10+ 分区存储无需权限）、
抽屉式设置。
已在 Android 15 模拟器完成真机级验收：安装/启动/扫描/导出/横竖屏/前后台全部通过；
普通权限下 raw ICMP 被拒（`EPERM`/`EACCES`），状态如实显示「权限不足」，
独立 TCP 连通性模式见路线图 M9。

## 路线图

| 版本 | 阶段 | 主题 |
| --- | --- | --- |
| **v1.0.0** ✅ | M0–M6 | 核心库 · TUI · 桌面 GUI · Android 构建链 · 真实 ICMP（Windows 实测） |
| **v1.1.0** ✅ | M7 | 目标信息增强：OUI 厂商识别（54,190 条离线库）、ARP/MAC、主机名反查、结果快照与恢复 |
| **v1.2.0–v1.5.0** ✅ | M8–M11 | 持续监控 · 探测扩展（TCP / 多网段 / CIDR）· JSON/HTML/diff 与无头 CLI · Linux 实测与打包、CI/CD |
| **v2.0.0** ✅ | M12 | 架构升级：IPv4+IPv6 统一目标模型、后端注册表、可选 Tokio 异步调度 |
| v2.1.0 ✅ | M13 | 资产识别：开放端口与服务指纹、设备类型推断 |
| **v2.2.0** ✅ | M14 | 历史与趋势：跨会话时序（版本化格式 + 降采样）、可用率报表、在线率曲线 |
| **v2.3.0** ✅ | M15 | 告警与通知：四类规则（纯函数匹配）、冷却去重与静音、五通道通知、跨会话状态 |
| **v2.4.0** ✅ | M16 | 配置与模板：命名扫描模板、schema 版本与迁移、未知键保留、CLI 直接消费模板 |
| **v2.5.0** ✅ | M17 | 安全与合规：默认限速与目标数上限、保留网段守卫、审计日志（JSONL）、首次授权告知、制品签名与清单 |
| **v2.6.0** ✅ | M18 | 国际化与无障碍：中英双语（界面与错误信息）、三套主题（高对比 / 色盲友好）、字体缩放、键盘全导航、ARIA 标注 |
| v2.7.0 | M19 | 规模化性能：万级目标分片流式扫描、背压与内存预算、基准用例 |
| v3.0.0 | M20 | 生态与开放：插件接口、脚本化任务、对外 API（主版本） |
| v3.1.0 | M21 | 协议扩展：SNMP 只读、DNS 解析与校验、逐跳路径探测 |
| v3.2.0 | M22 | 拓扑与可视化：自动成图（标注为推断）、机架布局、SVG/PNG 导出 |
| v3.3.0 | M23 | 团队与协作：配置档案往返、只读分享页、变更留痕 |
| v3.4.0 | M24 | 可观测性集成：Prometheus 指标、Grafana 面板、值班系统载荷 |
| v3.5.0 | M25 | 基线与异常：基线学习（冷启动期不产结论）、EWMA、偏离提示 |
| v3.6.0 | M26 | 移动与远程：移动端原生体验、远程只读视图、离线缓存 |
| v3.7.0 | M27 | 分布式采集：探针节点、中心聚合（分歧并列）、断网续传 |
| v4.0.0 | M28 | 平台化：服务端 + 多租户 + 插件市场 + 冻结的 API v2 契约（主版本） |

各阶段详细功能与验收条件见 [`docs/plan.md`](docs/plan.md)「路线图」章节。
v3.0.0 之后的主题转折（**从「单机工具」到「平台」**）与追加的两条原则
（推断类功能必须自证是推断、新增监听默认关闭且仅回环）见同一章节。

## 文档

| 文档 | 内容 |
| --- | --- |
| [`docs/reverse-findings.md`](docs/reverse-findings.md) | 原版逆向结论（静态 + Frida 动态复核），确证行为 / 待验证 / 设计决策三清单 |
| [`docs/plan.md`](docs/plan.md) | 分阶段计划、各阶段验收条件、路线图 |
| [`docs/platform-compatibility.md`](docs/platform-compatibility.md) | 各平台 ICMP 能力、权限要求与实测记录 |
| [`docs/testing.md`](docs/testing.md) | 每阶段真实构建/测试记录与踩坑 |
| [`docs/release.md`](docs/release.md) | 产物清单与发布说明 |
| [`docs/signing.md`](docs/signing.md) | 制品签名现状（三平台如实登记）、校验步骤、补签流程 |
| [`docs/release-notes-v2.0.0.md`](docs/release-notes-v2.0.0.md) | v2.0.0 发布说明 |
| [`CHANGELOG.md`](CHANGELOG.md) | 版本变更记录（含 v2.0.0 破坏性变更迁移表） |

## 构建环境注意事项

| 现象 | 原因 | 处理 |
| --- | --- | --- |
| `cargo build` 无输出后被 SIGTERM | 受限/无外网环境下 registry 探测挂起 | **按需**临时离线：`cargo build --offline` 或 `CARGO_NET_OFFLINE=true cargo build`。⚠️ **不要**把 `[net] offline = true` 写进 `.cargo/config.toml` —— cargo 解析依赖图时必须能访问索引，写死会让 CI 与全新克隆全部失败（v2.0.0 发行时真实踩过） |
| CI 报 `no matching package named ... found`，或 `Value 'D:/...' is invalid` | 把「本机恰好成立」的配置提交进了仓库 | 跑 `bash scripts/check-portability.sh`（已接进 CI）自查：盘符绝对路径 / cargo 离线开关 / vendored 源替换 |
| `npm install` 报 `spawnSync EBUSY` | esbuild/tauri-cli postinstall 无法 spawn | 加 `--ignore-scripts`（平台二进制仍在） |
| npm 装得极慢 | 默认源慢 | `--registry=https://registry.npmmirror.com` |
| GUI 启动显示「localhost 拒绝连接」 | debug 构建找 `devUrl` | 用 `cargo build --release`（资源已内嵌） |
| `schemars` 编译失败（`E0107`） | `indexmap 1.x` 的 `std` feature 默认关 | 在 `[dependencies]` 与 `[build-dependencies]` **两处**声明 `indexmap/std` |
| Android：`.so` 校验报 missing runtime symbols | `run()` 缺移动端入口宏 | 补 `#[cfg_attr(mobile, tauri::mobile_entry_point)]` |
| Android：Kotlin 报 different roots | tauri-plugin 源码与工程跨盘符 | `gradle.properties` 设 `kotlin.incremental=false` |
| Android：compileSdk 37 找不到 | 本机只装到 android-36 | `app/build.gradle.kts` 用 36 |
| `sdkmanager` 下载极慢（单连接 ~0.3 MB/s） | 无多线程下载 | 多线程分段拉取 zip 后解包 + 手写 `package.xml`（见 `docs/testing.md`） |
| `avdmanager` 报 *Package path is not valid*（却又把该路径列为合法） | 该版 cmdline-tools 解析系统镜像元数据有 bug | 手写 AVD 的 `config.ini`（emulator 只读它） |

## 已知限制与使用声明

- **Windows 与 Linux 均已实测真实 ICMP**（Windows 普通权限即可；Linux 在 WSL Debian
  内以 root 实测）；Android 已在模拟器实测（raw ICMP 被拒，如实显示「权限不足」）。
  拿不到权限时按「权限不足」如实上报，绝不把 TCP 连通性伪装成 ICMP 成功。
- **ICMPv6 未实装**：后端注册表把它登记为「不可用」并附人话说明，
  选中会得到明确的「不支持」，而不是静默超时。IPv6 的完整可用路径是 **TCP**。
- **Android 已在 Android 15 模拟器完成真机级验收**（安装/启动/扫描/导出/横竖屏/
  前后台）；真机上的厂商后台管控与字体缩放仍待验证。release APK 用 **Android Debug
  密钥**签名，可侧载试用，**不可上架**（同时提供未签名版供自行签名）。签名现状
  与自行签名步骤见 [`docs/signing.md`](docs/signing.md)。
- TUI 状态栏的本机地址尚未接入网卡枚举，显示「未知」，**不伪造**；
  MAC 与厂商已通过本机邻居表 + 离线 OUI 库接入。
- 目标范围默认 `.1`–`.255`（对齐原版），`.0` 不参与探测。
- **配置文件与模板**：`cping-cli` 与 `cping-tui` 都能消费（TUI 按 `P` 选模板，
  `--config` 指定文件，启动默认读 `./cping.toml`）。**GUI 的模板选择器尚未接入**
  ——它要同时动 Vue 组件与 Tauri 命令两侧，留到无障碍那一步一起做。
  TUI 只做「选择」，增删改模板请直接编辑配置文件。
- **配置文件里的注释不会被保留**：解析器是受限 TOML 子集，不建 AST；
  不认识的键会原样保留。写回前请自行备份带注释的原文件。
- **安全与合规策略（M17）**：默认速率上限 `500` pps、目标数上限 `65536`、默认拒绝
  保留 / 特殊用途网段；三项都能在命令行收紧，但**上限只能收紧不能放宽**，
  只有 `--max-pps 0` 会真正关闭限速。这三项刻意**不进配置文件**（防模板暗门）。
  审计日志（`--audit`）只记时间 / 参数 / 计数 / 操作者名，**不含** MAC、厂商、
  主机名等邻居隐私。
- **签名现状如实登记**：Windows exe 无证书时**不签**，只给 SHA-256 与
  `MANIFEST.json` 自证清单；官方构建的 Android release APK 目前仍是 **debug 密钥**
  签名；Linux 产物是未签名 tar.gz / zip + `SHA256SUMS`。**不把 SHA-256 说成签名，
  也不用自签证书冒充正式签名**——补签方法见 [`docs/signing.md`](docs/signing.md)。
- **双语的边界（M18 第一 / 二步）**：核心层的枚举标签与自由文本、三端界面文案
  都已双语。**刻意不翻译**的是数据契约与用户数据：CSV / JSON 的列名与字段名、
  报表里的目标摘要、审计记录里的 `targetSpec`（同一份报表在中英文机器上必须
  逐字节可比）、以及你自己写的模板 `note` / 主机名 / 厂商名 / banner。
  **仍只有中文**的一处是 `AppConfig::validate()` 的内层细节
  （外层「配置无效：…」已翻），登记为后续项。无障碍与 GUI 纯键盘导航
  是 M18 第三步，尚未开始。
- **语言协商顺序**：`--lang` → `CPING_LANG` → `LC_ALL` → `LC_MESSAGES` → `LANG`
  → 默认中文。`LC_ALL=C` / `POSIX` 表示「没有偏好」，按中文处理而**不是**英文。
  `--lang` 给不认识的值时 CLI 报错退出（退出码 2），TUI 只在状态栏告警
  （交互式程序，用户还能当场改）。
- **仅用于你拥有或获授权管理的网络**；并发、速率与目标数均有上限。

## License

[MIT](LICENSE)
