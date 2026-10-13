# 平台兼容性与 ICMP 能力记录

> 状态：M0–M3 阶段记录。**M5 接入真实探测时必须逐项实测并更新本文件。**

## 1. 原则

1. **不把 TCP 连通性伪装成 ICMP 成功。** 任何非 ICMP 的探测必须使用独立的状态名与模式。
2. **不假设 Android 存在可用的系统 `ping` 命令。**
3. 拿不到的字段（如 MAC）**留空**，不伪造。
4. 只对用户拥有或获授权管理的网络执行探测；并发与目标数都有上限（核心默认并发 32 / 上限 256，目标上限 1024）。

## 2. 原版（CPing.exe）的实测行为

| 项 | 结论 | 证据 |
| --- | --- | --- |
| 套接字 | `socket(AF_INET, SOCK_RAW, IPPROTO_ICMP)`，每目标一个 | `analysis/dynamic_trace5.log` |
| 提权 | **无需管理员**（未开启 `IP_HDRINCL`） | 动态实测 |
| 载荷 | 固定 44 字节 | 同上 |
| 重试 | 不重试，单次采样 | 程序自带帮助文本 |
| 并发 | 255 线程 | `CreateThread`×255 |
| 超时 | **未能确定**（见下） | —— |

> 超时未能确证：DFM 无超时输入控件；未观察到 `select` / `WSAPoll` / `setsockopt(SO_RCVTIMEO)`；
> 在「目标无响应」场景下扫描 91 秒仍未结束（Frida 介入条件下）。
> 新项目因此自行定义超时（默认 1000 ms），并在 `ProbeConfig` 中可配。

## 3. 新项目的平台计划

| 平台 | 后端方案 | 关键风险 | 状态 |
| --- | --- | --- | --- |
| Windows | 手写 FFI：优先 `SOCK_DGRAM + IPPROTO_ICMP`，失败回退 `SOCK_RAW` | `SOCK_RAW` 需管理员；`SOCK_DGRAM` 普通用户可用 | ✅ **已实测通过**（见下） |
| Linux | 手写 FFI：`SOCK_DGRAM` → `SOCK_RAW` | 需要 `CAP_NET_RAW`（root 或 `setcap cap_net_raw+ep`） | 已实现，**未实测**（本机无 Linux） |
| Android | 同上（`PosixIcmpBackend`）；不可用时改用**独立 TCP 模式** | 普通应用无 `CAP_NET_RAW` | 已实现，**待真机验证**（本机无设备） |

### 3.2 Windows 实测结果（M5，本机执行）

后端 `windows-icmp`，`cargo test -p cping-core --test native_icmp_real -- --ignored --nocapture`：

| 场景 | 结果 | 判读 |
| --- | --- | --- |
| `127.0.0.1` | **在线，rtt = 0 ms** | 真的收到了 Echo Reply，`SOCK_DGRAM + IPPROTO_ICMP` 在**普通权限**下可用 |
| `192.0.2.123`（保留地址，无主机） | **`Timeout`，耗时 816 ms**（预算 800 ms） | 不存在的目标**没有被误判为在线**，超时预算生效 |
| `tcp-connect 127.0.0.1:9` | `Timeout`（`name()` = `tcp-connect`） | TCP 模式与 ICMP 名字/语义完全分离，不会互相冒充 |

> 结论：Windows 侧不需要管理员权限，也不需要 `icmp.dll` 的 `IcmpSendEcho`（那个通常要求提权）。
> 权限不足时的错误码映射（`WSAEACCES → PermissionDenied`）已实现但**在本机未触发**，
> 需在受限账户下复测。

### 3.1 Android 策略（Q5 已定）

若普通权限下无法做真实 ICMP：

- 在界面上**显式声明平台限制**，不使用任何"看起来成功"的替代；
- 另设**独立模式**「TCP 连通性测试」，其状态名与导出列都明确标注（如 `TCP-OPEN`），
  与 `在线(ICMP)` 区分开；
- 不进入同一套「在线主机数」统计口径。

## 4. 本机地址与 MAC

| 项 | 现状 |
| --- | --- |
| 本机地址 | `cping-core::local_ip` 只定义接口；平台实现留待 M5。当前返回「未知（M5 接入网卡枚举）」，**不猜地址** |
| MAC | 原版取 `GetIpNetTable`（ARP 邻居表）；回环场景下为空。新项目同样在取不到时留空 |
| 多网卡/VPN | 必须返回**列表**，由用户在设置中选择，而不是无条件取第一张网卡 |

## 5. 待实测清单（M4 / M5）

- [ ] Windows：无管理员权限下 raw ICMP 是否可用；`IcmpSendEcho` 的错误码到状态映射
- [ ] Linux：`CAP_NET_RAW` 缺失时的错误码是否为 `EPERM`（应映射为「权限不足」）
- [ ] Android：`socket(AF_INET, SOCK_RAW, IPPROTO_ICMP)` 的 errno
- [ ] Android：`/system/bin/ping` 是否存在、普通应用能否执行
- [ ] Android：Wi-Fi ↔ 蜂窝 ↔ VPN 切换时的行为与任务状态
- [ ] Android：前后台切换、进程回收时的任务收尾
- [ ] Android：竖屏 / 横屏 / 系统字体缩放下的布局
- [ ] Android：文件保存（当前写入 `document_dir`，失败回退 `app_data_dir`）
