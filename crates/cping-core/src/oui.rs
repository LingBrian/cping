//! 厂商识别（M7）：MAC 前缀 → 厂商名，数据来自 IEEE 官方注册表并**编译期嵌入**。
//!
//! 零第三方依赖、零网络请求、零额外索引文件：
//! - 数据表是三段纯文本（`include_str!` 嵌入，见 [`crate::data`] 旁的 `SOURCE.md`），
//!   每行 `<十六进制前缀>\t<厂商名>`，按前缀升序；
//! - 首次查询时**懒建一次**「每行起始偏移」数组，之后二分查找；
//! - 命中后直接切片返回 `&'static str`，**不分配**。
//!
//! 优先级严格按 IEEE 规则：**MA-S(36bit) > MA-M(28bit) > MA-L(24bit)**，
//! 因为 28/36 位分配比 24 位更具体，可能覆盖同一 24 位空间。
//!
//! ## 换行符无关性
//!
//! 数据文件是 `include_str!` 按字节嵌入的，在 `core.autocrlf=true` 的机器上
//! 可能被检出为 CRLF。查表结果因此**显式剥掉行尾 `\r`**，使厂商名与换行符无关——
//! 否则同一个 MAC 在 Linux 与 Windows 上会得到「看起来一样、比较却不相等」的结果。
//!
//! ## 刻意不做的猜测
//!
//! - 全零地址（未取到）、组播位（I/G=1）、本地管理位（U/L=1）的地址
//!   **一律不查表**：这些地址不属于 IEEE 全局唯一分配，查出来的一定是错的。
//! - 查不到就返回 `None`，由界面显示为空，而不是回退成「未知厂商」之类的占位。

use std::sync::OnceLock;

/// IEEE MA-L 注册表（24 bit，约 4 万条）。
pub static MA_L: &str = include_str!("data/oui_ma_l.txt");
/// IEEE MA-M 注册表（28 bit）。
pub static MA_M: &str = include_str!("data/oui_ma_m.txt");
/// IEEE MA-S 注册表（36 bit）。
pub static MA_S: &str = include_str!("data/oui_ma_s.txt");

static MA_L_IDX: OnceLock<Vec<u32>> = OnceLock::new();
static MA_M_IDX: OnceLock<Vec<u32>> = OnceLock::new();
static MA_S_IDX: OnceLock<Vec<u32>> = OnceLock::new();

/// 一张注册表的引用：文本 + 前缀位宽 + 懒建索引的存放位置。
#[derive(Clone, Copy)]
pub(crate) struct OuiTable {
    text: &'static str,
    /// 参与匹配的十六进制位数：MA-L=6、MA-M=7、MA-S=9。
    nibbles: usize,
    idx: &'static OnceLock<Vec<u32>>,
}

impl OuiTable {
    pub(crate) const fn new(
        text: &'static str,
        nibbles: usize,
        idx: &'static OnceLock<Vec<u32>>,
    ) -> Self {
        Self { text, nibbles, idx }
    }

    fn offsets(&self) -> &'static [u32] {
        self.idx.get_or_init(|| build_offsets(self.text))
    }

    /// 用本表匹配；本表无命中返回 `None`（不代表全局未命中）。
    pub(crate) fn lookup(&self, mac: [u8; 6]) -> Option<&'static str> {
        let (buf, len) = prefix_hex(mac, self.nibbles);
        find(self.text, self.offsets(), &buf[..len])
    }
}

/// 内置的三张表，**顺序即优先级**。
pub(crate) fn builtin_tables() -> [OuiTable; 3] {
    [
        OuiTable::new(MA_S, 9, &MA_S_IDX),
        OuiTable::new(MA_M, 7, &MA_M_IDX),
        OuiTable::new(MA_L, 6, &MA_L_IDX),
    ]
}

/// 计算每行起始偏移（末行以 `\n` 结束，不会多出一行）。
fn build_offsets(table: &str) -> Vec<u32> {
    let bytes = table.as_bytes();
    let mut v = Vec::with_capacity(bytes.len() / 32 + 1);
    v.push(0u32);
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'\n' && i + 1 < bytes.len() {
            v.push((i + 1) as u32);
        }
    }
    v
}

/// 在「按行升序」的文本表里二分查找 `key`，命中返回该行的厂商名切片。
///
/// 这是本模块唯一的检索原语，刻意做成纯函数（只依赖入参），便于用小型
/// 夹具表做边界测试。
pub(crate) fn find<'a>(table: &'a str, offsets: &[u32], key: &[u8]) -> Option<&'a str> {
    if offsets.is_empty() || key.is_empty() {
        return None;
    }
    let bytes = table.as_bytes();
    let (mut lo, mut hi) = (0usize, offsets.len());
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let start = offsets[mid] as usize;
        if start >= bytes.len() {
            return None; // 索引与文本不一致，宁可返回未命中也不误报
        }
        let rest = &table[start..];
        let key_len = rest.as_bytes().iter().position(|&b| b == b'\t')?;
        let rec_key = &rest.as_bytes()[..key_len];
        match rec_key.cmp(key) {
            std::cmp::Ordering::Equal => {
                let after = &rest[key_len + 1..];
                let end = after.find('\n').unwrap_or(after.len());
                // CRLF 兼容（必须）：数据文件是 `include_str!` **按字节**嵌入的，
                // 若检出时被转成 CRLF，行尾会是 `\r\n`，按 `\n` 切出来的厂商名
                // 就会残留一个不可见的 `\r`。剥掉它，让结果与换行符无关。
                // （CI 上 Windows 全新克隆真实踩到过：返回 "XEROX CORPORATION\r"。）
                return Some(after[..end].trim_end_matches('\r'));
            }
            std::cmp::Ordering::Less => lo = mid + 1,
            std::cmp::Ordering::Greater => hi = mid,
        }
    }
    None
}

/// 取 MAC 前 `nibbles` 个十六进制半字节，返回定长缓冲与有效长度。
fn prefix_hex(mac: [u8; 6], nibbles: usize) -> ([u8; 9], usize) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let n = nibbles.min(9);
    let mut out = [0u8; 9];
    for (i, slot) in out.iter_mut().enumerate().take(n) {
        let byte = mac[i / 2];
        let nib = if i % 2 == 0 { byte >> 4 } else { byte & 0x0F };
        *slot = HEX[nib as usize];
    }
    (out, n)
}

/// 该地址是否**有资格**查 OUI。
///
/// 组播位与本地管理位的地址不由 IEEE 分配，查表结果一定是错的，
/// 因此这里直接判定为「不可查」，而不是返回一个看似合理的结果。
pub fn is_lookup_eligible(mac: [u8; 6]) -> bool {
    if mac == [0u8; 6] {
        return false; // 未取到 MAC 时的占位
    }
    if mac[0] & 0x01 != 0 {
        return false; // I/G 位：组播 / 广播
    }
    if mac[0] & 0x02 != 0 {
        return false; // U/L 位：本地管理地址，非 IEEE 注册
    }
    true
}

/// 按给定优先级顺序解析厂商名。
pub(crate) fn resolve(mac: [u8; 6], tables: &[OuiTable]) -> Option<&'static str> {
    if !is_lookup_eligible(mac) {
        return None;
    }
    tables.iter().find_map(|t| t.lookup(mac))
}

/// **对外入口**：MAC → 厂商名；查不到（含不可查地址）返回 `None`。
pub fn vendor_of(mac: [u8; 6]) -> Option<&'static str> {
    resolve(mac, &builtin_tables())
}

/// 格式化 MAC 为 `AA:BB:CC:DD:EE:FF`（仅用于展示与导出，统一大小写）。
pub fn mac_to_string(mac: [u8; 6]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut s = String::with_capacity(17);
    for (i, b) in mac.iter().enumerate() {
        if i > 0 {
            s.push(':');
        }
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0F) as usize] as char);
    }
    s
}

/// 解析 `AA:BB:CC:DD:EE:FF` / `AABBCCDDEEFF`（也接受 `-` 分隔），失败返回 `None`。
pub fn parse_mac(s: &str) -> Option<[u8; 6]> {
    let hex: String = s
        .chars()
        .filter(|c| !matches!(c, ':' | '-' | '.' | ' '))
        .collect();
    if hex.len() != 12 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0u8; 6];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

/// 三张表的条目数（`MA-L, MA-M, MA-S`），供界面/文档展示，不做缓存。
pub fn entry_counts() -> (usize, usize, usize) {
    let count = |t: &str| t.as_bytes().iter().filter(|b| **b == b'\n').count();
    (count(MA_L), count(MA_M), count(MA_S))
}

/// 数据表来源与生成日期（来自 `data/SOURCE.md` 的同一批数据）。
pub const DATA_SOURCE: &str = "IEEE MA-L/MA-M/MA-S public registries (standards-oui.ieee.org)";

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- 纯函数 find 的边界测试：用小型夹具表 ---

    const FIXTURE: &str = "0000AA\tAAA Corp\n0000BB\tBBB Inc\n0000CC\tCCC Ltd\n";

    fn fixture_offsets() -> Vec<u32> {
        build_offsets(FIXTURE)
    }

    /// 由 `0000AA` / `C85CE27` / `8C1F64AFA` 这类变长前缀补零构造一个 MAC。
    /// 末字节固定为非零，避免整串落进「全零占位地址」那条资格排除分支。
    fn mac_from_prefix_hex(prefix: &str) -> [u8; 6] {
        let nib = |i: usize| -> u8 {
            prefix
                .as_bytes()
                .get(i)
                .map(|b| (*b as char).to_digit(16).expect("前缀应为十六进制") as u8)
                .unwrap_or(0)
        };
        let mut out = [0u8; 6];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = (nib(i * 2) << 4) | nib(i * 2 + 1);
        }
        out[5] = 0x11;
        out
    }

    #[test]
    fn find_hits_first_middle_last() {
        let offs = fixture_offsets();
        assert_eq!(find(FIXTURE, &offs, b"0000AA"), Some("AAA Corp"));
        assert_eq!(find(FIXTURE, &offs, b"0000BB"), Some("BBB Inc"));
        assert_eq!(find(FIXTURE, &offs, b"0000CC"), Some("CCC Ltd"));
    }

    #[test]
    fn find_misses_below_and_above_range() {
        let offs = fixture_offsets();
        assert_eq!(find(FIXTURE, &offs, b"000000"), None);
        assert_eq!(find(FIXTURE, &offs, b"FFFF00"), None);
    }

    #[test]
    fn find_misses_between_records() {
        let offs = fixture_offsets();
        // 排序介于 0000AA 与 0000BB 之间，但不存在该记录
        assert_eq!(find(FIXTURE, &offs, b"0000AB"), None);
    }

    #[test]
    fn find_handles_degenerate_inputs() {
        assert_eq!(find("", &[], b"0000AA"), None);
        assert_eq!(find(FIXTURE, &fixture_offsets(), b""), None);
        // 单条记录的表
        let one = "123456\tOnly Corp\n";
        assert_eq!(find(one, &build_offsets(one), b"123456"), Some("Only Corp"));
        assert_eq!(find(one, &build_offsets(one), b"123455"), None);
        // 末行没有换行结尾也要能取到
        let no_trailing = "123456\tNo Newline";
        assert_eq!(
            find(no_trailing, &build_offsets(no_trailing), b"123456"),
            Some("No Newline")
        );
    }

    #[test]
    fn find_is_crlf_safe() {
        // 数据文件在 Windows 上可能按 CRLF 检出（core.autocrlf=true），而
        // include_str! 是逐字节嵌入的。无论行尾是 LF 还是 CRLF，取到的
        // 厂商名都必须完全一致，不能带上不可见的 `\r`。
        let crlf = "0000AA\tAAA Corp\r\n0000BB\tBBB Inc\r\n0000CC\tCCC Ltd\r\n";
        let offs = build_offsets(crlf);
        assert_eq!(find(crlf, &offs, b"0000AA"), Some("AAA Corp"));
        assert_eq!(find(crlf, &offs, b"0000BB"), Some("BBB Inc"));
        assert_eq!(find(crlf, &offs, b"0000CC"), Some("CCC Ltd"));
        // 与同一份数据的 LF 版本结果逐字节相同
        assert_eq!(
            find(crlf, &offs, b"0000BB"),
            find(FIXTURE, &fixture_offsets(), b"0000BB")
        );

        // 末行是 CRLF 但不带尾随换行
        let crlf_tail = "0000AA\tNo Tail Switch\r\n0000BB\tLast Corp\r";
        let offs = build_offsets(crlf_tail);
        assert_eq!(find(crlf_tail, &offs, b"0000AA"), Some("No Tail Switch"));
        assert_eq!(find(crlf_tail, &offs, b"0000BB"), Some("Last Corp"));

        // 偏移数量不受行尾影响（CRLF 只多加一个字节，不另起一行）
        assert_eq!(build_offsets(crlf).len(), 3);
    }

    #[test]
    fn find_rejects_malformed_record() {
        // 记录里没有制表符 → 视为表损坏，返回 None 而不是 panic
        let broken = "123456NoTab\n";
        assert_eq!(find(broken, &build_offsets(broken), b"123456"), None);
    }

    #[test]
    fn offsets_cover_every_record_exactly_once() {
        let offs = fixture_offsets();
        // "0000AA\tAAA Corp\n" = 16 字节，"0000BB\tBBB Inc\n" = 15 字节
        assert_eq!(offs, vec![0, 16, 31]);
        assert_eq!(offs.len(), FIXTURE.lines().count());
        // 真实表：偏移数量必须等于条目数
        let (l, m, s) = entry_counts();
        assert_eq!(build_offsets(MA_L).len(), l);
        assert_eq!(build_offsets(MA_M).len(), m);
        assert_eq!(build_offsets(MA_S).len(), s);
    }

    // --- 前缀提取 ---

    #[test]
    fn prefix_hex_takes_leading_nibbles() {
        let mac = [0xAB, 0xCD, 0xEF, 0x01, 0x23, 0x45];
        let (b, n) = prefix_hex(mac, 6);
        assert_eq!(&b[..n], b"ABCDEF");
        let (b, n) = prefix_hex(mac, 7);
        assert_eq!(&b[..n], b"ABCDEF0");
        let (b, n) = prefix_hex(mac, 9);
        assert_eq!(&b[..n], b"ABCDEF012");
        // 越界请求被夹到 9 位，不会 panic
        let (_, n) = prefix_hex(mac, 99);
        assert_eq!(n, 9);
    }

    // --- 可查资格 ---

    #[test]
    fn ineligible_addresses_are_not_looked_up() {
        assert!(!is_lookup_eligible([0, 0, 0, 0, 0, 0])); // 占位
        assert!(!is_lookup_eligible([0x01, 0, 0, 0, 0, 0])); // 组播
        assert!(!is_lookup_eligible([0xFF; 6])); // 广播
        assert!(!is_lookup_eligible([0x02, 0, 0, 0, 0, 0])); // 本地管理
        assert!(is_lookup_eligible([0x00, 0x1A, 0x2B, 0, 0, 0]));
    }

    #[test]
    fn multicast_mac_returns_none_even_if_prefix_exists() {
        // 01:00:5E 是组播段，绝不应返回厂商名
        assert_eq!(vendor_of([0x01, 0x00, 0x5E, 0x00, 0x00, 0x01]), None);
    }

    // --- 优先级：MA-S > MA-M > MA-L ---

    #[test]
    fn longest_prefix_wins_over_shorter() {
        static IDX1: OnceLock<Vec<u32>> = OnceLock::new();
        static IDX2: OnceLock<Vec<u32>> = OnceLock::new();
        // 注意首字节用 0xA8：组播位(0x01)与本地管理位(0x02)都为 0，
        // 才是有资格查 OUI 的全局唯一地址（用 0xAA 会被资格检查挡掉）。
        let wide: &'static str = "A8BBCC\tWide Vendor\n";
        let mid: &'static str = "A8BBCCD\tMid Vendor\n";
        let narrow: &'static str = "A8BBCCDEF\tNarrow Vendor\n";
        let tables = [
            OuiTable::new(narrow, 9, &IDX1),
            OuiTable::new(mid, 7, &IDX1),
            OuiTable::new(wide, 6, &IDX2),
        ];
        // 36 位命中
        assert_eq!(
            resolve([0xA8, 0xBB, 0xCC, 0xDE, 0xF0, 0x00], &tables),
            Some("Narrow Vendor")
        );
        // 只有 28 位命中（36 位不匹配）→ 退到 MA-M
        assert_eq!(
            resolve([0xA8, 0xBB, 0xCC, 0xD9, 0x99, 0x00], &tables),
            Some("Mid Vendor")
        );
        // 只有 24 位命中 → 退到 MA-L
        assert_eq!(
            resolve([0xA8, 0xBB, 0xCC, 0x11, 0x22, 0x00], &tables),
            Some("Wide Vendor")
        );
        // 完全不在表内
        assert_eq!(resolve([0xAB, 0x00, 0x00, 0x00, 0x00, 0x00], &tables), None);
    }

    // --- 真实数据（IEEE 官方表） ---

    #[test]
    fn real_tables_are_populated_and_sorted() {
        let (l, m, s) = entry_counts();
        assert!(l > 30_000, "MA-L 条目数偏少: {l}");
        assert!(m > 5_000, "MA-M 条目数偏少: {m}");
        assert!(s > 5_000, "MA-S 条目数偏少: {s}");
        for table in [MA_L, MA_M, MA_S] {
            let mut prev: Option<&str> = None;
            for line in table.lines() {
                let (key, vendor) = line.split_once('\t').expect("每条记录都应有制表符");
                assert!(!vendor.is_empty(), "厂商名不应为空");
                assert!(
                    key.bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_lowercase()),
                    "前缀应为大写十六进制: {key}"
                );
                if let Some(p) = prev {
                    assert!(p < key, "表必须严格升序: {p} 之后出现 {key}");
                }
                prev = Some(key);
            }
        }
    }

    #[test]
    fn real_lookup_hits_known_assignments() {
        // MA-L 真实条目
        let l = vendor_of([0x00, 0x00, 0x0C, 0x00, 0x00, 0x00]);
        assert_eq!(l, Some("Cisco Systems, Inc"));

        // MA-M 样本 C8:5C:E2:7*
        let m = vendor_of([0xC8, 0x5C, 0xE2, 0x70, 0x00, 0x00]);
        assert_eq!(m, Some("SYNERGY SYSTEMS AND SOLUTIONS"));

        // 真实数据里的优先级用例：8C:1F:64 在 MA-L 里是
        // "IEEE Registration Authority"（它把子块再分配给 MA-S/MA-M），
        // 而 8C:1F:64:AF:A 在 MA-S 里是 "DATA ELECTRONIC DEVICES, INC"。
        // 更具体的那条必须胜出——这正是「最长前缀优先」在真实数据上的体现。
        assert_eq!(
            find(
                MA_L,
                MA_L_IDX.get_or_init(|| build_offsets(MA_L)),
                b"8C1F64"
            ),
            Some("IEEE Registration Authority"),
            "前提：MA-L 中 8C1F64 属于 IEEE 注册管理机构"
        );
        assert_eq!(
            vendor_of([0x8C, 0x1F, 0x64, 0xAF, 0xA0, 0x00]),
            Some("DATA ELECTRONIC DEVICES, INC"),
            "MA-S(36bit) 必须覆盖同一空间的 MA-L(24bit)"
        );
    }

    #[test]
    fn real_lookup_misses_unassigned_prefix() {
        // 本地管理地址：不做 OUI 查询
        assert_eq!(vendor_of([0x02, 0xAA, 0xBB, 0x00, 0x00, 0x00]), None);
        // 00:08:33 是 MA-L 表中真实存在的空缺（由数据统计得出）
        assert_eq!(
            vendor_of([0x00, 0x08, 0x33, 0x11, 0x22, 0x33]),
            None,
            "未分配前缀必须返回 None，不得回退成占位文本"
        );
        // 注意：00:00:00 并非空缺（XEROX 持有），这里顺带把数据事实固定下来
        assert_eq!(
            vendor_of([0x00, 0x00, 0x00, 0x11, 0x22, 0x33]),
            Some("XEROX CORPORATION")
        );
    }

    #[test]
    fn real_data_lookups_never_leak_carriage_returns() {
        // 平台无关的守卫：无论数据文件是按 LF 还是 CRLF 检出，走**完整查询路径**
        // （资格检查 + 三表最长前缀优先）拿到的厂商名都不应残留 `\r`。
        // 这里对真实数据抽查，因此该断言在 Windows 与 Linux 上含义完全一致。
        let tables = builtin_tables();
        let mut checked = 0usize;
        for table in [MA_S, MA_M, MA_L] {
            let lines: Vec<&str> = table.lines().collect();
            let sample = lines.iter().take(50).chain(lines.iter().rev().take(10));
            for line in sample {
                let key = line.split_once('\t').expect("每条记录都应有制表符").0;
                if let Some(vendor) = resolve(mac_from_prefix_hex(key), &tables) {
                    assert!(
                        !vendor.contains('\r'),
                        "厂商名残留 CR: {vendor:?}（前缀 {key}）"
                    );
                    checked += 1;
                }
            }
        }
        assert!(checked > 0, "抽查应至少命中一条记录");
    }

    // --- 格式化 / 解析 ---

    #[test]
    fn mac_formatting_roundtrip() {
        let mac = [0xA4, 0x5E, 0x60, 0x12, 0x34, 0xAB];
        assert_eq!(mac_to_string(mac), "A4:5E:60:12:34:AB");
        assert_eq!(parse_mac("A4:5E:60:12:34:AB"), Some(mac));
        assert_eq!(parse_mac("a4-5e-60-12-34-ab"), Some(mac));
        assert_eq!(parse_mac("a45e6012 34ab"), Some(mac));
        assert_eq!(parse_mac("a45e601234"), None); // 位数不足
        assert_eq!(parse_mac("a45e601234abcd"), None); // 位数过多
        assert_eq!(parse_mac("ZZ:5E:60:12:34:AB"), None); // 非十六进制
        assert_eq!(parse_mac(""), None);
    }

    #[test]
    fn formatted_real_mac_looks_up_correctly() {
        let mac = parse_mac("8C:1F:64:AF:A1:02").unwrap();
        assert_eq!(mac, [0x8C, 0x1F, 0x64, 0xAF, 0xA1, 0x02]);
        assert_eq!(
            vendor_of(mac),
            Some("DATA ELECTRONIC DEVICES, INC"),
            "截取 9 位前缀后仍应命中 MA-S"
        );
    }
}
