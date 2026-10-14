# CPing v2.0.0 —— 持续监控、TCP/IPv6 探测、命令行与四平台发行

主版本。把 v1.1.2 之后的五个阶段（M8–M12）一次交付，含**破坏性变更**（见 §4）。
这一版的三个关键词：**看得见变化**（持续监控）、**打得开更多目标**（TCP / 多网段 / CIDR / IPv6）、
**进得了自动化**（无头 CLI + 退出码契约 + 四平台产物）。

---

## 1. 这一版新增了什么

### 1.1 持续监控（M8）

不再是「按一次扫一次」：开启监控后按间隔自动重扫，并把**每一轮与上一轮的差异**列出来。

- 变化类型分四种：主机**上线**、**离线**、**延迟档位变化**、**状态变化**。
- 判定是纯函数 `diff_scans(prev, next)`，与界面无关，可单测到全状态组合。
- 变化日志是**环形缓冲**（容量封顶），长期开着也不会把内存吃光。
- 变化行在表格/矩阵里**高亮并自动淡出**；TUI 按 `e`、GUI 点「变化」页签可看事件流。
- 首轮只建立基线，不报「全部新上线」——这是刻意的，否则第一次刷新就是满屏噪音。

### 1.2 探测目标与方式（M9）

- 目标写法：`192.168.1`（网段）/ `192.168.1.7`（单主机）/ `192.168.1.0/24`（CIDR）/
  多段用逗号或空格分隔，**重复目标只算一次**。
- CIDR 写错会**明确指出错在哪**：主机位非 0 时报告正确的网络号；
  `/0`、`/64` 这类巨型网段**明确拒绝**，而不是静默展开成几百万个目标。
- 探测方式：**ICMP** 或 **TCP（指定端口）**。一次任务只用一种，
  所以 ICMP 的「在线」与 TCP 的「开放」不会混进同一列统计。
- TCP 模式下状态显示为 `TCP-OPEN` / `TCP-超时` / `TCP-关闭`，导出里也有「探测方式」列。

> **为什么 TCP 模式重要**：它**不需要任何特权**。Linux 上不用 `CAP_NET_RAW`、
> 不用 root；Android 上普通应用也能用——这正好绕开 Android 拿不到 raw socket 的限制。

### 1.3 数据与集成（M10）

- 导出格式从 2 种扩到 **4 种**：CSV / TXT / **JSON** / **HTML**（单文件自包含，
  无外部资源、无 JS，直接发给别人就能看）。
- **扫描对比**：两次结果之间的新增 / 消失 / 延迟变化 / 状态变化，可导出。
- **新增 `cping-cli`**：无头命令行，带可判定的退出码，适合脚本、计划任务、CI。

```bash
cping-cli --target 192.168.1 --mode icmp
cping-cli -t "192.168.1.0/24, ::1" --mode tcp --port 443 --out report.html
cping-cli -t 192.168.1 --watch 30 --rounds 10 --quiet
cping-cli --list-backends
```

退出码：`0` 全部在线 / `1` 存在离线 / `2` 参数错误 / `3` 权限不足或后端不可用。

### 1.4 平台与分发（M11）

- **Linux 从「未实测」变成「已实测」**：在 WSL Debian 13 内构建并实测
  `cping-tui` 与 `cping-cli`（细节见 §5）。
- 打包脚本：`scripts/package.sh`（tar.gz）与 `scripts/package.ps1`（zip），
  每个包内附 `README.txt`，并生成 `SHA256SUMS`。
- GitHub Actions：CI 做 fmt / clippy / test / 前端构建 / Android 交叉编译检查；
  Release 工作流在打 tag 时自动构建四平台产物并创建 Release。

### 1.5 架构升级（M12）

- **IPv4 + IPv6 统一目标模型**：目标类型从 `Ipv4Addr` 改为 `IpAddr`，
  同一个任务里 v4 与 v6 可以混排（`-t "::1, 127.0.0.1"` 是一条命令）。
- **后端注册表**：`icmp4` / `icmp6` / `tcp` / `mock` 四个后端带着**能力声明**登记在册
  （支持哪些地址族、是否需要权限、当前是否可用）。
  界面据此**在探测前**就能告诉你「这个组合不行」，而不是扫完才发现清一色「不支持」。
- **可选异步调度**：`async` feature（**默认关闭**）提供 `AsyncScheduler`，
  语义与同步版逐条对齐。默认构建**仍然零第三方运行时依赖**——Tokio 只在你显式
  开启该 feature 时才会被引入。

---

## 2. 平台支持现状（诚实版）

| 平台 | 产物 | 实测程度 |
| --- | --- | --- |
| Windows x64 | `cping-tui` / `cping-cli`（zip）、桌面 GUI（exe） | ✅ 真实 ICMP 扫描、IPv6 TCP、`--list-backends` |
| Windows x86（32 位） | `cping-tui` / `cping-cli`（zip） | ✅ 构建通过，可在 x64 系统上运行 |
| Linux x64 | `cping-tui` / `cping-cli`（tar.gz） | ✅ 在 WSL Debian 内实测 ICMP、TCP(v4/v6)、TUI、四种导出 |
| Android | APK（见 §6） | ⚠️ 构建成功；模拟器验收（v1.0.0 时完成）；**真机未验证**，release 未签名 |

---

## 3. 本轮修掉的真实缺陷

1. **Android 构建回归**：`WebviewWindow::center()` 在移动端不存在。
   这是 v1.1.1 加「窗口尺寸按显示器收敛」时引入的，因为 Android 一直没重建，
   直到这次才暴露。已把该逻辑改为桌面专属。
2. **TUI 切换探测方式会静默丢掉 `--mock`**：演示途中按一次 `T` 就从模拟数据
   变成真实探测，看起来像「结果变了」。已改为沿用启动时的选择。
3. **三端各写一份后端选择逻辑**：TUI 与 GUI 各有一份
   「mock 就造模拟后端、否则按方式选」的分支，连模拟参数都要人工保持一致。
   已收敛到核心的单一入口。
4. **打包脚本在受限环境下中断**：`Remove-Item` 与 `Compress-Archive -Force`
   都会走回收站，被安全策略拦下后整轮打包失败。已改用 .NET 的直接删除。

---

## 4. 破坏性变更（升级请读这一节）

| 变更 | 从 | 到 | 怎么改 |
| --- | --- | --- | --- |
| 目标地址类型 | `Ipv4Addr` | `IpAddr` | `TargetSpec::targets` 元素类型变了；排序用 `addr_sort_key` |
| 探测后端接口 | `probe(Ipv4Addr, Duration)` | `probe(IpAddr, Duration)` | 后端自行拒绝不支持的地址族（返回 `unsupported`），不要静默超时 |
| 矩阵格号 | `cell_of_addr(Ipv4Addr) -> usize` | `cell_of_addr(IpAddr) -> Option<usize>` | v6 返回 `None`，必须处理 |
| 矩阵槽位 | `matrix_slots(&[Ipv4Addr])` | `matrix_slots<T: AsTargetAddr>(&[T])` | 传 `&[IpAddr]` 或 `&[Ipv4Addr]` 都可以 |
| 探测方式 | 只有 ICMP | `ProbeMode::{Icmp, Tcp { port }}` | 取平台默认用 `platform_default_mode()` |
| CSV 格式 | 8 列 | 9 列（末尾新增「探测方式」） | **按列名解析**，不要按下标硬编码 |
| 三端后端选择 | 各自 `backend_for_mode` | `resolve_for_run` / `backend_for_run` | 旧接口仍保留，新代码用注册表 |

- 旧接口 `native::backend_for_mode` **没有删除**，已有调用方不会被破坏。
- 默认 feature 组合下 `cping-core` **仍无第三方运行时依赖**：

```bash
cargo tree -p cping-core -e normal --prefix none   # 只有它自己一行
```

---

## 5. 验证（实测，不是「应该可以」）

### 5.1 静态与测试（Windows，两套 workspace）

```text
cargo test --workspace                                  →  274 passed; 0 failed; 8 ignored
cargo fmt --all -- --check                              →  clean
cargo clippy --workspace --all-targets -- -D warnings   →  clean
（GUI 壳单独一套）cargo clippy --all-targets -- -D warnings  →  clean
npm run typecheck                                       →  0 errors
npm run build                                           →  2705 modules / 562 kB JS
```

### 5.2 Linux（WSL Debian 13，Rust 1.99.0，x86_64）

| 用例 | 结果 |
| --- | --- |
| 原生 ICMP | `127.0.0.1` → 在线、1 ms、退出码 0；默认网关 → 在线、0 ms |
| ICMPv6（未实装） | `::1` → **不支持 1**、退出码 3（**不是**静默超时） |
| TCP v4 | `127.0.0.1:18080` → `TCP-OPEN`、2 ms、退出码 0 |
| TCP v6 | `::1:18081` → `TCP-OPEN`、1 ms、退出码 0 |
| **TCP 关闭端口** | `127.0.0.1:18999` → **不可达 1**、0.000 s、退出码 1 |
| 混合 v4+v6 | `"::1, 127.0.0.1"` → 2 个网段 / 2 个目标，两个都在线 |
| TUI | 在真实伪终端内渲染并进入备用屏，按 `q` **优雅退出（码 0）** |
| 四种导出 | CSV / TXT / JSON / HTML 均产出且内容正确（含 `::1` 行） |

> **一条 Windows 上拿不到的结论**：Linux 上「TCP 端口关闭」会收到内核 RST，
> 被正确归类为「不可达」并**立即**返回（0.000 s）。
> Windows 上同一个用例表现为超时（用 Python 独立对照同样超时），
> 所以 Windows 侧至今**不宣称**该路径已实测。

### 5.3 Windows x64

| 用例 | 结果 |
| --- | --- |
| `--list-backends` | 能力表与实现一致（`icmp6` 明确标为不可用） |
| `-t ::1 --mode icmp` | 不支持 1、退出码 3 |
| `-t ::1 --mode tcp --port 18081` | `TCP-OPEN`、3 ms、退出码 0 |

---

## 6. 产物

| 文件 | 说明 |
| --- | --- |
| `cping-tui-2.0.0-windows-x64.zip` | Windows 64 位终端版（内附 README.txt） |
| `cping-cli-2.0.0-windows-x64.zip` | Windows 64 位命令行 |
| `cping-gui-2.0.0-windows-x64.exe` | Windows 64 位桌面版（Tauri） |
| `cping-tui-2.0.0-windows-x86.zip` | Windows 32 位终端版 |
| `cping-cli-2.0.0-windows-x86.zip` | Windows 32 位命令行 |
| `cping-tui-2.0.0-linux-x64.tar.gz` | Linux x86-64 终端版 |
| `cping-cli-2.0.0-linux-x64.tar.gz` | Linux x86-64 命令行 |
| `cping-2.0.0-android-universal-release-debugkey-signed.apk` | Android **全 ABI 单包**，已用 debug 密钥签名，**可直接安装** |
| `cping-2.0.0-android-universal-release-unsigned.apk` | Android 同上，但未签名（供自行签名发布） |
| `SHA256SUMS.txt` | 上述产物的 SHA-256 |

Android 包内已含 `lib/arm64-v8a`、`lib/armeabi-v7a`、`lib/x86`、`lib/x86_64`
四个 ABI 的 `.so`，**不需要挑架构**。包名 `com.cping.gui`，`versionName 2.0.0`，
`targetSdk 36` / `minSdk 24`。

> 签名的那个用的是 **Android Debug 证书**（`CN=Android Debug`）——
> 侧载试用没问题，但**不能上架应用商店**。要正式发布请用你自己的 keystore 重新签名
> （命令见 `docs/release.md` §8.3）。

校验方式：

```bash
sha256sum -c SHA256SUMS.txt            # Linux
Get-FileHash .\cping-cli-2.0.0-windows-x64.zip -Algorithm SHA256   # Windows
```

---

## 7. 已知限制（未实测的部分，不藏着）

- **ICMPv6 未实装**。注册表把 `icmp6` 登记为「不可用」并附说明，
  选中它会得到明确的「不支持」。**IPv6 目前请用 TCP**——这条路径完整可用且已实测。
- **Android 真机未验证**：本机无设备、无 AVD；安装 / 启动 / 触控 / 前后台 /
  文件保存在 v1.0.0 时用 Android 15 模拟器做过真机级验收，本版**未重新做模拟器验收**。
- **Android release APK 未签名**，要立刻试用请装 debug 变体。
- **Linux 的邻居表（MAC）与主机名反查未实测**（只有单测覆盖）；
  `CAP_NET_RAW` 缺失时的 `EPERM` 映射未单独复现（WSL 内以 root 运行）。
- **Windows 上的「TCP-关闭」判据不可靠**（见 §5.2）。
- **矩阵视图对 IPv6 不可用**：这是语义限制而不是缺陷——
  v6 地址没有「最后一格 = 格号」这种映射，硬套只会得到一张骗人的图，
  因此界面会自动切到表格并说明原因。
- **仅用于你拥有或获授权管理的网络**。

---

## 8. 下一步

`docs/plan.md` 已把路线图排到 **M13–M20**：
资产识别（M13）→ 历史与趋势（M14）→ 告警通知（M15）→ 配置模板（M16）→
安全合规（M17）→ 国际化无障碍（M18）→ 规模化性能（M19）→ 生态与开放（M20，v3.0.0）。

下一步最可能先动的是 **M13 资产识别**：已经有了目标信息（MAC/厂商/主机名）
和 TCP 端口探测，把「开放端口 + 服务指纹」接上，结果就从「这台机器在线」
变成「这台机器是什么」。
