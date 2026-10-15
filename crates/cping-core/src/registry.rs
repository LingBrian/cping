//! 后端注册表（M12）：把「探测方式」映射到可用的 [`ProbeBackend`]，并如实报告能力。
//!
//! ## 为什么需要它
//!
//! M1b 定下的 [`ProbeBackend`] 抽象已经能容纳多种实现，但选择逻辑散落在各端
//! （`backend_for_mode` + `platform_icmp_backend` + 平台 `cfg`），于是每加一种
//! 能力都要改三个地方。注册表把这件事收敛成一处：
//!
//! - **唯一的选择入口**：`BackendRegistry::resolve(mode, family)`；
//! - **唯一的能力来源**：`BackendCaps` 明确写出「支持哪些地址族」，
//!   界面据此在探测前提示，而不是等扫完才发现清一色「不支持」；
//! - **可测试**：注册表是纯数据 + 工厂函数，不需要真实网络。
//!
//! ## 稳定性承诺
//!
//! 注册表本身（[`BackendRegistry`] / [`BackendCaps`] / [`BackendId`]）在
//! v2.x 内保持**源码兼容**：只做新增，不改签名。新增后端 = 新增一个
//! `BackendId` 变体 + 一处工厂分支；调用方代码不需要改动。
//!
//! ## 当前内置后端
//!
//! | id | 方式 | IPv4 | IPv6 | 说明 |
//! | --- | --- | --- | --- | --- |
//! | `icmp4` | ICMP | ✅ | ❌ | 平台原生 ICMP（Windows `IcmpSendEcho` / Linux `SOCK_DGRAM`） |
//! | `icmp6` | ICMP | ❌ | ❌ | **占位**：ICMPv6 尚未实装，登记为「不可用」而不是假装支持 |
//! | `tcp` | TCP | ✅ | ✅ | TCP 握手，无特权即可运行；IPv6 的完整路径 |
//! | `mock` | — | ✅ | ✅ | 仅测试用，不出现在选择列表里 |

use crate::i18n::{self, Key, Lang};
use crate::probe::ProbeBackend;
use crate::spec::ProbeMode;
use std::fmt;
use std::net::IpAddr;

/// 后端标识。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackendId {
    /// 平台原生 ICMPv4。
    Icmp4,
    /// ICMPv6。**尚未实装**：登记在册以便界面能如实显示「不可用」。
    Icmp6,
    /// TCP 连接探测。
    Tcp,
    /// 模拟后端（测试用）。
    Mock,
}

impl BackendId {
    /// 稳定的机器可读标识。
    pub const fn key(self) -> &'static str {
        match self {
            BackendId::Icmp4 => "icmp4",
            BackendId::Icmp6 => "icmp6",
            BackendId::Tcp => "tcp",
            BackendId::Mock => "mock",
        }
    }

    /// 中文标签（等价于 `label_in(Lang::Zh)`）。
    ///
    /// 核心层内部的报表与导出仍在用它——**产物内容是数据契约**：
    /// 同一份 CSV / HTML 在中英文机器上必须一致；只有界面跟着语言变。
    pub fn label_zh(self) -> &'static str {
        self.label_in(Lang::Zh)
    }

    /// 按语言渲染的标签。
    pub fn label_in(self, lang: Lang) -> &'static str {
        i18n::text(
            lang,
            match self {
                BackendId::Icmp4 => Key::LabelBackendIcmp4,
                BackendId::Icmp6 => Key::LabelBackendIcmp6,
                BackendId::Tcp => Key::LabelBackendTcp,
                BackendId::Mock => Key::LabelBackendMock,
            },
        )
    }

    pub const ALL: [BackendId; 4] = [
        BackendId::Icmp4,
        BackendId::Icmp6,
        BackendId::Tcp,
        BackendId::Mock,
    ];
}

/// 一个后端的能力描述。
///
/// 「能力」是**事实陈述**，不是愿望：界面只应把 `available == true` 的后端
/// 放进可选列表，并在用户选了不支持的地址族时提前给出提示。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendCaps {
    pub id: BackendId,
    /// 该后端在当前平台是否可用（编译进来 + 有权限路径）。
    pub available: bool,
    /// 是否支持 IPv4 目标。
    pub ipv4: bool,
    /// 是否支持 IPv6 目标。
    pub ipv6: bool,
    /// 是否需要提权（原始套接字）。
    pub needs_privilege: bool,
    /// 不可用时的人话说明。
    pub note: CapsNote,
}

/// 后端「为什么不可用」的说明。
///
/// M18 之前这里是 `&'static str` 的中文常量，英文界面下原样露出中文。
/// 改成枚举之后有两个好处：一是**漏翻变成编译期问题**（match 是穷尽的），
/// 二是调用方没法再随手往里塞一句没翻译的字符串。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CapsNote {
    /// 可用，没有要说的。
    #[default]
    None,
    /// 该平台没有原生 ICMP 实现。
    PlatformIcmpMissing,
    /// ICMPv6 未实装。
    Icmp6Missing,
    /// Mock 后端。
    TestOnly,
    /// 查不到这个后端（注册表里没有）。
    UnknownBackend,
}

impl CapsNote {
    /// 有没有话要说。空说明不该被拼进展示文本。
    pub const fn is_empty(self) -> bool {
        matches!(self, CapsNote::None)
    }

    /// 对应的字典键；[`CapsNote::None`] 没有键。
    pub const fn key(self) -> Option<Key> {
        match self {
            CapsNote::None => None,
            CapsNote::PlatformIcmpMissing => Some(Key::CapsNotePlatformIcmpMissing),
            CapsNote::Icmp6Missing => Some(Key::CapsNoteIcmp6Missing),
            CapsNote::TestOnly => Some(Key::CapsNoteTestOnly),
            CapsNote::UnknownBackend => Some(Key::CapsNoteUnknownBackend),
        }
    }

    /// 按语言渲染；`None` 渲染成空串（不是 `"None"`）。
    pub fn label_in(self, lang: Lang) -> &'static str {
        match self.key() {
            Some(k) => i18n::text(lang, k),
            None => "",
        }
    }

    /// 中文（等价于 `label_in(Lang::Zh)`）。
    pub fn label_zh(self) -> &'static str {
        self.label_in(Lang::Zh)
    }
}

impl fmt::Display for CapsNote {
    /// `Display` 就是中文渲染——两处各自 `match` 的结果是改了其一、
    /// 另一处悄悄留在旧措辞上。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label_in(Lang::Zh))
    }
}

impl BackendCaps {
    /// 该后端能否处理这个目标地址。
    pub fn supports(&self, ip: IpAddr) -> bool {
        self.available && if ip.is_ipv4() { self.ipv4 } else { self.ipv6 }
    }

    /// 一行展示文本，例如 `原生 ICMP（IPv4）· 需要权限 · IPv4`。
    pub fn describe(&self) -> String {
        self.describe_in(Lang::Zh)
    }

    /// 按语言渲染的一行展示文本。
    ///
    /// 分隔符 ` · ` 在两种语言下一致，留在代码里——它是标点不是散文。
    pub fn describe_in(&self, lang: Lang) -> String {
        let mut parts = vec![self.id.label_in(lang).to_string()];
        if self.needs_privilege {
            parts.push(i18n::text(lang, Key::CapsNeedsPrivilege).to_string());
        }
        match (self.ipv4, self.ipv6) {
            (true, true) => parts.push(i18n::text(lang, Key::CapsIpv4And6).to_string()),
            (true, false) => parts.push(i18n::text(lang, Key::CapsIpv4Only).to_string()),
            (false, true) => parts.push(i18n::text(lang, Key::CapsIpv6Only).to_string()),
            (false, false) => {}
        }
        if !self.available && !self.note.is_empty() {
            parts.push(self.note.label_in(lang).to_string());
        }
        parts.join(" · ")
    }
}

/// 后端注册表。
///
/// 结构上刻意保持「无状态」：不缓存实例，`resolve` 每次返回新的后端。
/// 原因是后端可能持有套接字或可变计数（`MockBackend` 就记峰值并发），
/// 复用同一个实例会让两次扫描互相污染。
#[derive(Debug, Clone, Copy, Default)]
pub struct BackendRegistry;

impl BackendRegistry {
    pub const fn new() -> Self {
        Self
    }

    /// 当前平台下全部内置后端的能力（**含不可用的**，界面自行过滤）。
    ///
    /// 顺序固定：icmp4 → icmp6 → tcp → mock。
    pub fn caps(&self) -> [BackendCaps; 4] {
        let icmp4_available = cfg!(any(windows, target_os = "linux", target_os = "android"));
        [
            BackendCaps {
                id: BackendId::Icmp4,
                available: icmp4_available,
                ipv4: icmp4_available,
                ipv6: false,
                needs_privilege: true,
                note: if icmp4_available {
                    CapsNote::None
                } else {
                    CapsNote::PlatformIcmpMissing
                },
            },
            BackendCaps {
                id: BackendId::Icmp6,
                // M12 的诚实边界：IPv6 的 TCP 路径完整可用，ICMPv6 未实装。
                available: false,
                ipv4: false,
                ipv6: false,
                needs_privilege: true,
                note: CapsNote::Icmp6Missing,
            },
            BackendCaps {
                id: BackendId::Tcp,
                available: true,
                ipv4: true,
                ipv6: true,
                needs_privilege: false,
                note: CapsNote::None,
            },
            BackendCaps {
                id: BackendId::Mock,
                available: true,
                ipv4: true,
                ipv6: true,
                needs_privilege: false,
                note: CapsNote::TestOnly,
            },
        ]
    }

    /// 某个后端的能力描述。
    pub fn caps_of(&self, id: BackendId) -> BackendCaps {
        self.caps()
            .into_iter()
            .find(|c| c.id == id)
            .unwrap_or(BackendCaps {
                id,
                available: false,
                ipv4: false,
                ipv6: false,
                needs_privilege: false,
                note: CapsNote::UnknownBackend,
            })
    }

    /// 为指定探测方式挑选后端。
    ///
    /// 返回的是「有能力处理**该方式**」的后端，**与目标地址族无关**：
    /// 一次扫描里 v4/v6 目标可能并存，由后端自己在 `probe` 里对不支持的族
    /// 返回 [`crate::ProbeError::unsupported`]。这样「选了 ICMP 但目标是 v6」
    /// 会得到明确的「不支持」而不是静默超时。
    pub fn resolve(&self, mode: ProbeMode) -> (BackendId, Box<dyn ProbeBackend>) {
        match mode {
            ProbeMode::Icmp => (BackendId::Icmp4, crate::native::platform_icmp_backend()),
            ProbeMode::Tcp { port } => (
                BackendId::Tcp,
                Box::new(crate::native::TcpConnectBackend::new(port)),
            ),
        }
    }

    /// 界面可选的探测方式（只列可用后端对应的方式）。
    ///
    /// 当前平台上，ICMP 与 TCP 恒定可用（`icmp4` 在不支持的平台上会退化为
    /// 「不支持」错误，但仍然列出——把它藏起来会让用户以为程序坏了）。
    pub fn selectable_modes(&self) -> Vec<ProbeMode> {
        vec![ProbeMode::Icmp, ProbeMode::Tcp { port: 80 }]
    }

    /// 面向界面的一行能力摘要，例如：
    /// `ICMP 需要权限（仅 IPv4）；IPv6 目标请用 TCP`。
    /// 中文摘要（等价于 `summary_in(Lang::Zh)`）。
    pub fn summary_zh(&self) -> String {
        self.summary_in(Lang::Zh)
    }

    /// 按语言渲染的能力摘要。
    ///
    /// 连**分隔符**也要跟着语言走：中文用全角分号，英文用 `"; "`。
    /// 把中文标点留在英文句子里，读起来是「一眼假」的机器翻译味。
    pub fn summary_in(&self, lang: Lang) -> String {
        let icmp = self.caps_of(BackendId::Icmp4);
        let tcp = self.caps_of(BackendId::Tcp);
        let mut parts = Vec::new();
        if icmp.available {
            parts.push(if icmp.needs_privilege {
                i18n::text(lang, Key::RegistryIcmpAvailablePrivileged).to_string()
            } else {
                i18n::text(lang, Key::RegistryIcmpAvailable).to_string()
            });
        } else {
            parts.push(i18n::text(lang, Key::RegistryIcmpUnavailable).to_string());
        }
        if tcp.available && tcp.ipv6 {
            parts.push(i18n::text(lang, Key::RegistryTcpAvailable).to_string());
        }
        let icmp6 = self.caps_of(BackendId::Icmp6);
        if !icmp6.available {
            parts.push(i18n::text(lang, Key::RegistryIcmp6Missing).to_string());
        }
        parts.join(i18n::text(lang, Key::RegistrySeparator))
    }
}

/// 便捷函数：与 [`crate::native::backend_for_mode`] 等价，但走注册表。
///
/// 保留 `native::backend_for_mode` 是为了不破坏既有调用方；新代码应优先用注册表。
pub fn backend_for(mode: ProbeMode) -> Box<dyn ProbeBackend> {
    BackendRegistry::new().resolve(mode).1
}

/// 演示 / 调试用的模拟分布参数。
///
/// 放在核心而不是各端，是为了让 TUI、GUI、CLI 的 `--mock` 演示效果**完全一致**：
/// 同一个目标地址在任何一端都得到同一个结果，截图与实测才可比。
pub const MOCK_ONLINE_PERCENT: u8 = 62;
pub const MOCK_BASE_RTT_MS: u32 = 6;
pub const MOCK_SPAN_MS: u32 = 140;

/// 与 [`backend_for_run`] 相同，但**同时给出选中的 [`BackendId`]**。
///
/// 界面要在状态栏 / 帮助里写「当前用的是哪个后端」，直接复用这里的选择结果，
/// 而不是自己按 `mock` / 方式再推一遍——推错就会出现「界面写 icmp4、
/// 实际跑的是 mock」这种自相矛盾。
pub fn resolve_for_run(mode: ProbeMode, mock: bool) -> (BackendId, Box<dyn ProbeBackend>) {
    if mock {
        (
            BackendId::Mock,
            Box::new(crate::probe::MockBackend::new(
                crate::probe::MockPolicy::Realistic {
                    online_percent: MOCK_ONLINE_PERCENT,
                    base_rtt_ms: MOCK_BASE_RTT_MS,
                    span_ms: MOCK_SPAN_MS,
                },
            )),
        )
    } else {
        BackendRegistry::new().resolve(mode)
    }
}

/// 一次扫描真正使用的后端选择入口——**三端共用这一处**。
///
/// - `mock == true`：返回确定性模拟后端，用于演示 / UI 调试，不发真实报文；
/// - `mock == false`：按注册表解析探测方式对应的真实后端。
///
/// 之所以把 `mock` 也收进核心：此前 TUI 与 GUI 各写了一份
/// 「mock 就造 MockBackend，否则 backend_for_mode」的分支，两处的模拟参数
/// 还必须手动保持一致。收敛到这里之后，`--mock` 的语义只有一个定义点。
///
/// 注意 `mock` **必须显式开启**——绝不因为探测失败就悄悄退回模拟数据，
/// 那会把「没扫到」伪装成「扫到了」。
pub fn backend_for_run(mode: ProbeMode, mock: bool) -> Box<dyn ProbeBackend> {
    resolve_for_run(mode, mock).1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backends_keys_are_stable() {
        assert_eq!(BackendId::Icmp4.key(), "icmp4");
        assert_eq!(BackendId::Icmp6.key(), "icmp6");
        assert_eq!(BackendId::Tcp.key(), "tcp");
        assert_eq!(BackendId::Mock.key(), "mock");
        assert_eq!(BackendId::ALL.len(), 4);
    }

    #[test]
    fn caps_are_consistent_with_reality() {
        let reg = BackendRegistry::new();
        let tcp = reg.caps_of(BackendId::Tcp);
        assert!(tcp.available);
        assert!(tcp.ipv4 && tcp.ipv6, "TCP 同时支持 v4/v6");
        assert!(!tcp.needs_privilege, "TCP 不需要特权");

        let icmp4 = reg.caps_of(BackendId::Icmp4);
        assert!(icmp4.available, "三个目标平台都有原生 ICMP");
        assert!(icmp4.ipv4);
        assert!(!icmp4.ipv6, "ICMPv4 后端不处理 v6");
        assert!(icmp4.needs_privilege);

        // 这一条是 M12 的诚实边界：没实装就登记为不可用。
        let icmp6 = reg.caps_of(BackendId::Icmp6);
        assert!(!icmp6.available);
        assert!(!icmp6.ipv6);
        assert!(icmp6.note.label_zh().contains("未实装"));
    }

    #[test]
    fn supports_reflects_address_family() {
        let reg = BackendRegistry::new();
        let v4: IpAddr = "192.168.1.1".parse().unwrap();
        let v6: IpAddr = "::1".parse().unwrap();

        assert!(reg.caps_of(BackendId::Tcp).supports(v4));
        assert!(reg.caps_of(BackendId::Tcp).supports(v6));
        assert!(reg.caps_of(BackendId::Icmp4).supports(v4));
        assert!(!reg.caps_of(BackendId::Icmp4).supports(v6));
        // 不可用的后端对任何地址都返回 false
        assert!(!reg.caps_of(BackendId::Icmp6).supports(v6));
    }

    #[test]
    fn resolve_picks_the_backend_matching_the_mode() {
        let reg = BackendRegistry::new();
        let (id, backend) = reg.resolve(ProbeMode::Icmp);
        assert_eq!(id, BackendId::Icmp4);
        assert_eq!(
            backend.name(),
            crate::native::platform_icmp_backend().name()
        );

        let (id, backend) = reg.resolve(ProbeMode::Tcp { port: 445 });
        assert_eq!(id, BackendId::Tcp);
        assert_eq!(backend.name(), "tcp-connect");
    }

    #[test]
    fn selectable_modes_are_never_empty() {
        let modes = BackendRegistry::new().selectable_modes();
        assert_eq!(modes.len(), 2);
        assert!(modes.contains(&ProbeMode::Icmp));
        assert!(modes.iter().any(|m| m.is_tcp()));
    }

    #[test]
    fn summary_mentions_the_icmpv6_gap() {
        let text = BackendRegistry::new().summary_zh();
        assert!(text.contains("ICMP"), "{text}");
        assert!(text.contains("ICMPv6 未实装"), "{text}");
        assert!(text.contains("IPv6"), "{text}");
    }

    #[test]
    fn describe_marks_privilege_and_families() {
        let reg = BackendRegistry::new();
        let icmp = reg.caps_of(BackendId::Icmp4).describe();
        assert!(icmp.contains("需要权限"), "{icmp}");
        assert!(icmp.contains("仅 IPv4"), "{icmp}");
        let tcp = reg.caps_of(BackendId::Tcp).describe();
        assert!(tcp.contains("IPv4+IPv6"), "{tcp}");
    }

    #[test]
    fn backend_for_run_returns_a_real_backend_unless_asked_for_mock() {
        // 默认必须给真实后端：绝不能因为探测失败就悄悄换成模拟数据。
        let real = backend_for_run(ProbeMode::Tcp { port: 445 }, false);
        assert_eq!(real.name(), "tcp-connect");

        let real_icmp = backend_for_run(ProbeMode::Icmp, false);
        assert_eq!(
            real_icmp.name(),
            crate::native::platform_icmp_backend().name(),
            "ICMP 方式应解析到平台原生后端"
        );

        let mocked = backend_for_run(ProbeMode::Tcp { port: 445 }, true);
        assert_eq!(mocked.name(), "mock", "mock 分支必须返回模拟后端");
        // 方式不影响 mock：换方式仍是同一个模拟后端
        assert_eq!(backend_for_run(ProbeMode::Icmp, true).name(), "mock");
    }

    #[test]
    fn mock_parameters_live_in_core_so_three_ends_agree() {
        // 这些常量是 TUI / GUI / CLI `--mock` 演示结果的唯一来源：
        // 三端不再各写一份 62 / 6 / 140，改这里即三端同改。
        assert_eq!(MOCK_ONLINE_PERCENT, 62);
        assert_eq!(MOCK_BASE_RTT_MS, 6);
        assert_eq!(MOCK_SPAN_MS, 140);
    }

    #[test]
    fn mock_backend_is_reproducible_and_mode_independent() {
        use std::time::Duration;

        // 「可重复」是模拟后端对演示的基本承诺：同地址两次探测结果必须一致，
        // 且换探测方式不该改变结果——否则同一个网段在不同端的演示会对不上。
        let icmp = backend_for_run(ProbeMode::Icmp, true);
        let tcp = backend_for_run(ProbeMode::Tcp { port: 80 }, true);
        let target: IpAddr = "192.168.1.7".parse().unwrap();
        let win = Duration::from_millis(50);

        let a = format!("{:?}", icmp.probe(target, win));
        let b = format!("{:?}", icmp.probe(target, win));
        let c = format!("{:?}", tcp.probe(target, win));

        assert_eq!(a, b, "同一地址的模拟结果必须可重复");
        assert_eq!(a, c, "mock 与探测方式无关");
    }
}
