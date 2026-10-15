# CPing v2.7.0 —— 规模化性能：万级目标不卡、结果一条不丢、内存有预算

次版本（minor）。把 **M19 一个阶段的三步**一次交付，主题只有一个：
让「一万个目标」从**能用**变成**用得住**。

| 步 | 做了什么 | 为什么非做不可 |
| --- | --- | --- |
| 一 | 统计从 O(n²) 改成 **O(1) 增量累加**，并建了可回归的基准工程 | 万级目标下，界面会被「统计」本身拖死 |
| 二 | 事件通道改**有界** + 进度事件**降级为心跳** | 无界通道在「界面慢、生产者快」时会一路涨到 OOM |
| 三 | **内存预算**（由类型宽度推出，不是测量值）+ **65536 目标真实跑通** | 时间可以外推，**内存不能** |

**没有破坏性变更**：核心 API 只有新增（`ScanStats` 的增量方法、`budget` 模块、
`EventStats`、`SchedulerHandle::event_stats()`）。唯一新增的公开字段是
`RunOutcome::events`，而 `RunOutcome` 只在核心内部构造，调用方只读。

---

## 1. 这一版改了什么

### 1.1 统计：O(n²) → O(1)

三端此前都是「**每收到一条结果就把整张表拍平，再全量统计一遍**」。
8192 个目标要拍平 8192 次、累计克隆约 1.3 亿条结果——实测这一步就要 **1276 ms**，
而按二次关系外推到 65536 目标是 **82 秒**。

现在改成按条增量累加：同一套数字、O(1)/条、零分配。同一台机器上
8192 目标从 **1276 ms** 降到 **0.036 ms**。

- 新增 `ScanStats::set_total` / `push_status` / `for_total`，与既有批量口径
  （`from_results_with_total` / `from_statuses`）**逐字段相同**，由测试守着；
- 三端接线（TUI 删掉 `recompute_stats`、GUI 改 `push_status`）；
- **落盘与报表仍走批量口径**，两条路必须对得上——否则会出现「界面显示完成 500、
  导出的报表里 498」这种没人知道该信哪个的局面。

### 1.2 事件通道：有界 + 背压 + 心跳

- 事件通道由**无界**改**有界**（`mpsc::sync_channel`，容量 4096 条 / 峰值 **576 KiB**）。
- 满时策略**分层**：先丢进度，**结果与终态永不丢**；结果也满时**阻塞在发送侧**
  （真背压）——丢结果比卡住更糟。
- `Progress` 从「每条结果一报」改为**按时间节流**（200 ms，多 worker 同时判定时用
  CAS 抢名额）。它携带的完成数在 `Result` 里都有，所以它只是「长时间没结果、
  但确实在推进」时的心跳。
- 新增 `EventStats { progress_sent, progress_merged, progress_dropped }`，
  **静默丢弃比丢弃本身更糟**——「丢了」必须是能看见、能断言的数。

实测 10000 目标：不限速时进度事件 **10000 → 0 条**；默认限速时 **10000 → 94 条**
（合并比 **106×**）。

### 1.3 内存：预算 + 65536 目标端到端

- 新增 `cping-core::budget`：五个结构性常量（结果整表 / 结果快照 / 目标表 /
  事件队列上限 / 每 worker 临时）**全部由类型的编译期宽度 × 明确的计数上限相乘得到**
  ——**没有一个来自测量**，所以是 `const fn`、可做**编译期断言**、换机器不会红。
  两个入口：`scan_memory_budget_bytes`（流式）与 `run_to_completion_budget_bytes`。
  默认并发（32）下 **11.12 MiB**，满并发（256）下 **25.12 MiB**。
- 基准新增 `[2d]`：65536 目标**真实跑完**（不再按 8192 外推），并断言
  实测堆峰值必须落在预算内。这条断言机器无关，已接进 CI。

---

## 2. 平台支持现状（诚实版）

| 平台 | 产物 | 实测程度 |
| --- | --- | --- |
| Windows x64 | `cping-tui` / `cping-cli`（zip）、桌面 GUI（exe） | ✅ 真实 ICMP 万级扫描（65534 目标）、`--list-backends` |
| Windows x86（32 位） | `cping-tui` / `cping-cli`（zip） | ✅ 构建通过，可在 x64 系统上运行 |
| Linux x64 | `cping-tui` / `cping-cli`（tar.gz）、GUI（deb / AppImage） | ⚠️ 本次由 CI 构建；**万级性能结论取自 Windows 一侧**，未在 Linux 上复测 |
| Android | APK（全 ABI 单包） | ⚠️ 由 CI 构建；**未签名**（未配置 keystore），真机未验证 |

> 本版的性能数字**全部来自 Windows 本机**（见 §5）。Linux 与 Android 只保证
> 「构建通过、可运行」，**不宣称**已达到同样的吞吐——没有测过的事不说。

---

## 3. 修掉的真实缺陷

1. **三端统计是 O(n²)**，万级目标下界面会被统计本身拖死（见 §1.1）。
2. **有界事件通道 + `SchedulerHandle::Drop` 的 `join` 顺序会导致真实死锁**：
   通道改有界后，worker 可能正阻塞在 `send(Result)` 上等队列腾位置；此时若直接
   `join` 协调线程，就没人在取事件，而 `Receiver` 还活着**不会**让 `send` 返回
   `Err` —— 死锁。修法：`Drop` 里**先丢掉接收端再 join**。
   这条**做过负向验证**：故意注掉那行后，15 s 看门狗超时报「句柄在扫描中途被丢弃时
   死锁了」。同步版与异步版各有一条测试守着。
3. **终态路径会凭空克隆一整份结果表，只为把它数一遍**：协调线程发终态事件时用
   `flatten().cloned().collect()` 再全量统计——克隆的唯一作用是数一遍。65536 目标下
   那是 7.5 MiB，且与结果表**同时存活**，等于把峰值翻倍。改成持锁增量累加后，
   10000 目标的堆峰值 **3.80 MiB → 1.94 MiB（−49%）**，**这一步没有动任何算法**。
4. **`Progress` 既不保序也不完整**（实测出来的事件契约）：它会**领先于自己的
   `Result`**（255 目标 / 32 并发下超前 0–42 次、最多 3 条），尾部也会丢
   （实测 232/255）。所以完成数的权威来源只能是 `Result`——GUI 的进度推进据此改了。

---

## 4. 破坏性变更

**没有。** 本版只新增公开 API，不改既有签名与语义。

唯一的理论边界：`RunOutcome` 新增了公开字段 `events`。`RunOutcome` 只在
`cping-core` 内部构造，外部只读取它，因此不影响任何调用方；
若你曾用结构体字面量自己造过 `RunOutcome`（都不该这么做），补一个字段即可。

---

## 5. 验证（实测，不是「应该可以」）

### 5.1 静态与测试（Windows）

```text
cargo test --workspace                                  →  571 passed（核心 lib 326 / async 333）
cargo test（GUI 壳，独立 workspace）                     →  30 passed
cargo fmt --all -- --check                              →  clean
cargo clippy --workspace --all-targets -- -D warnings   →  clean
cargo clippy -p cping-core --all-targets --features async -- -D warnings  →  clean
（GUI 壳单独一套）cargo clippy --all-targets            →  clean
bash scripts/check-portability.sh                       →  通过
node scripts/check-symbols.mjs                          →  通过
```

合计 **601 个测试通过**。

### 5.2 基准（Windows 本机；`cargo bench -p cping-core --bench scan`）

| 指标 | 值 |
| --- | --- |
| 8192 目标 · 批量统计 | **1276 ms** |
| 8192 目标 · 增量统计 | **0.036 ms**（35548×） |
| 10000 目标 · 进度事件（不限速） | 10000 → **0 条** |
| 10000 目标 · 进度事件（默认限速） | 10000 → **94 条**（合并比 106×） |
| 10000 目标 · 堆峰值 | 3.80 MiB → **1.94 MiB**（−49%） |
| **65536 目标 · 端到端真实跑完** | **25.8 ms / 248 万目标/秒 / 堆峰值 9.37 MiB**（预算 11.12 MiB 以内） |
| 事件队列上限 | **576 KiB**（远小于结果整表 7.50 MiB） |

> 「堆峰值」是**计数全局分配器**的口径：只计堆分配，**不含线程栈与 mmap**，
> **不等于 RSS**。这一点在 `docs/perf-baseline.txt` 里写明了。

### 5.3 真实 ICMP 万级端到端（Windows）

```bash
cping-cli -t 127.0.0.0/16 --max-pps 0 --concurrency 256 --timeout 300 --json out.json
```

| 项 | 结果 |
| --- | --- |
| 目标数 | **65534**（`/16` 排除网络号与广播地址，贴着 `MAX_TARGETS` 硬上限） |
| 用时 | **42.4 s** |
| 在线 / 超时 / 错误 | **58365 / 7169 / 0** |
| 导出 JSON | **65534 条**，`total == completed == 65534`、`pending 0` |

- 目标全部是 `127.0.0.0/8` 内的本地回环地址，**不出本机**。
- 那 **7169 个超时是真实观测**（Windows 并不为 `127.0.0.0/8` 的每个地址回 ICMP），
  顺带证明**超时路径在万级规模下也走通**，不是只跑通了「全在线」那种顺风局。
- TUI 侧另有 `10.0.0.0/16`（65534 目标）的**取消测试**：中途取消必须立刻停下，
  并用终端内存后端渲染一帧，证明万级下界面仍能画出来。

---

## 6. 产物

| 文件 | 说明 |
| --- | --- |
| `cping-tui-2.7.0-windows-x64.zip` | Windows 64 位终端版（内附 `README.txt`） |
| `cping-cli-2.7.0-windows-x64.zip` | Windows 64 位命令行 |
| `cping-gui-2.7.0-windows-x64.exe` | Windows 64 位桌面版（Tauri） |
| `cping-tui-2.7.0-windows-x86.zip` | Windows 32 位终端版 |
| `cping-cli-2.7.0-windows-x86.zip` | Windows 32 位命令行 |
| `cping-tui-2.7.0-linux-x64.tar.gz` | Linux x86-64 终端版 |
| `cping-cli-2.7.0-linux-x64.tar.gz` | Linux x86-64 命令行 |
| `cping-gui-2.7.0-linux-gui-x64.deb` / `.AppImage` | Linux x86-64 桌面版 |
| `cping-2.7.0-android-universal-release-*.apk` | Android **全 ABI 单包**（`arm64-v8a` / `armeabi-v7a` / `x86_64` / `x86`），**不需要挑架构** |
| `SHA256SUMS.txt` | 上述产物的 SHA-256（权威清单，以它为准） |
| `MANIFEST.json` | 制品清单：每个产物的 sha256 / 字节数 / 构建来源 |

校验方式：

```bash
sha256sum -c SHA256SUMS.txt                                          # Linux / WSL
Get-FileHash .\cping-cli-2.7.0-windows-x64.zip -Algorithm SHA256     # Windows
```

> **签名状态：本版未做代码签名。** 仓库未配置 Windows 证书与 Android keystore，
> 因此 Windows 产物**未做 Authenticode 签名**（首次运行会弹「未知发布者」），
> Android 包是 gradle 构建的**原始产物（`release-unsigned.apk`，未签名）——
> 不可直接安装**；要侧载需自行用 debug 或正式密钥签名（`scripts/sign-android.sh`）。
> v2.0.0 附带的「debug 证书已签名」包是当时本机人工补签的，本次自动链路
> 没有密钥就**不做也不声称**任何签名。这两件事在 `MANIFEST.json` 里如实标注，
> **不用自签证书冒充正式签名**。

---

## 7. 已知限制（未实测的部分，不藏着）

- **性能数字只有 Windows 一侧的口径**；Linux 与 Android 未复测万级吞吐。
- **内存预算是上界估算，不是实测复现**：五个常量里的「每 worker 临时 64 KiB」
  是全表**唯一估出来的数**（其余四个都由类型宽度精确推出）。
- **`progress_dropped` 在实测里始终为 0**：限速把生产速度压得足够慢，
  背压没被真正触发过。它有单测覆盖，但**没有在真机上丢过一条**。
- **TUI / GUI 的万级观感未人工确认**（本机无桌面会话），只有内存后端渲染与单测。
- **Android 真机未验证**；release 包未签名。
- **ICMPv6 未实装**（IPv6 请走 TCP，那条路径完整可用）。
- **仅用于你拥有或获授权管理的网络。**

---

## 8. 下一步

`docs/plan.md` 已把 **M20（v3.0.0）** 的详细计划补好，三步走：

1. **插件契约与隔离** —— 关键决定是走**进程外插件**（stdio + 行分隔 JSON）
   而不是 `dlopen`：进程边界是唯一真能做到「插件崩溃不带崩主程序」的边界，
   而且协议即 ABI（Rust 没有稳定 ABI）。
2. **本地只读 API** —— 新 crate 承载依赖，核心仍然零第三方运行时依赖；
   只读、默认关闭、仅回环。
3. **受限表达式脚本** —— 纯求值无副作用、求值步数有上限、读不到时钟与环境
   （否则告警不可复现）。

完整路线图见 `docs/plan.md`。
