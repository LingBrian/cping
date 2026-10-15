//! GUI **命令层自己**的字典：Tauri 命令在失败 / 回报结果时说的那些话。
//!
//! # 为什么这里要有一份字典，而不是复用核心层的
//!
//! 核心层的字典说的是「**逻辑**发生了什么」（`目标 224.0.0.1 属于保留网段`）；
//! 这里说的是「**这个界面**在说什么」（`尚无扫描结果可保存`）。
//! 两者是不同的键空间：
//!
//! - 核心层的消息是**结论**，CLI / TUI / GUI 三端都会原样转述，所以它必须在核心；
//! - 下面这些是 GUI 独有的编排消息——「导出对话框还没选路径」「快照前缀解析不了」
//!   这类话在无界面的 CLI 里根本不存在。把它们塞进核心字典会让另外两端背上一批
//!   永远用不到、且没人复核的条目，而核心字典越大，「中英不一致」越难发现。
//!
//! 所以划界是：**核心产出结论，GUI 产出编排**。两者机制完全相同（见下），
//! 只是各自持有自己的键。
//!
//! # 机制（与 `crates/cping-core/src/i18n.rs` 同构）
//!
//! 1. **键是枚举**：`t("typo")` 只在运行期显示成键名本身，`Key::Typo` 直接编译不过。
//! 2. **序号 + 表格双关卡**：`ordinal` 是穷尽的 `match`（漏登记 → 编译失败），
//!    `TABLE` 必须严格按序号升序（[`tests::table_order_matches_ordinal`] 守着）。
//! 3. **漏译是可测的**：英文条目里出现中日韩字符即失败，中英 `{}` 个数不等即失败。
//!
//! # 一条必须记住的坑
//!
//! [`text`] 是 `TABLE[ordinal(key)]`——**按下标取值，不是按 key 查找**。
//! 往 `TABLE` 中间插一条会让它后面所有键整体错位，而且**不会崩、不会报错**，
//! 只会静默显示成邻居那句消息。新增键一律追加到 `Key`、`ordinal`、`TABLE`
//! **三处的末尾**。
//!
//! # 刻意不做的事
//!
//! - **不重复定义 `Lang`**。语言枚举属于「支持哪几种语言」这件事，是产品级决定，
//!   三端必须一致，所以只有 `cping_core::Lang` 一份。
//! - **技术记号不进字典**。`CSV` / `TXT` / `JSON` 两种语言下一样，写进字典只会
//!   给 [`tests::chinese_entries_are_not_empty_and_not_ascii_only`] 添麻烦
//!   （它要求中文条目至少含一个中日韩字符）。只有 `HTML 报告` 这类**散文**才进来。
//! - **导出内容一律中文**。CSV / JSON 的列名、字段名、状态文本是**数据契约**：
//!   同一份报表在中英文机器上必须一致，否则下游脚本会按语言分支。
//!   所以 `target_desc` / `local_ip` 这些进报表的字段仍然走中文渲染，
//!   只有「界面提示」跟着 `lang` 走。

use cping_core::Lang;

// ---------------------------------------------------------------------------
// 键
// ---------------------------------------------------------------------------

/// GUI 命令层的消息键。
///
/// 新增变体后必须同时做三件事，否则测试会失败：
/// 在 [`ordinal`] 里登记序号、在 [`code_of`] 里给出稳定码、在 [`TABLE`] 里给出中英条目。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    // —— 邻居表补全 ——
    /// 参数：无
    UnknownReason,
    /// 参数：`原因`
    EnrichReadFailed,
    /// 参数：`条目数`
    EnrichNoFill,
    /// 参数：`条目数`、`新填 MAC 数`、`新填厂商数`
    EnrichFilled,
    EnrichNoResults,

    // —— 端口指纹 ——
    FingerprintNoOnlineHosts,

    // —— 导出 ——
    ExportNoResults,
    ExportBadExtension,
    /// `HTML 报告`——唯一需要翻译的导出格式名。
    ExportFormatHtml,
    /// 参数：`条数`、`格式名`、`路径`
    ExportDone,
    /// 参数：`路径`、`原因`
    WriteFailed,

    // —— 差异报告 ——
    DiffNoBaseline,
    /// 参数：`实际格式名`
    DiffOnlyCsv,
    DiffNeedCsv,
    /// 参数：`条目数`、`路径`
    DiffExportDone,

    // —— 趋势与可用率报表 ——
    TrendEmptyHint,
    TrendNoData,
    UptimeReportOnlyCsvHtml,
    /// 参数：`主机数`、`轮次`、`路径`
    UptimeReportDone,

    // —— 告警 ——
    AlertEmptyPrimed,
    AlertEmptyDisabled,
    /// 参数：`实际值`
    AlertBadUptimeThreshold,
    /// 参数：`支持的类别列表`、`实际收到的类别`
    AlertUnsupportedKinds,

    // —— 目录与路径（这些几乎都是 OS 错误的**外框**）——
    /// 参数：`原因`
    CannotDetermineSaveDir,
    /// 参数：`目录`、`原因`
    CannotCreateDir,
    /// 参数：`原因`
    CannotDetermineConfigDir,
    /// 参数：`原因`
    CannotCreateConfigDir,
    /// 参数：`原因`
    WriteConfigFailed,
    /// 参数：`原因`
    CannotDetermineAppDataDir,

    // —— 结果补全的下标与并发 ——
    /// 参数：`下标`
    IndexNoResult,
    TaskSwitched,

    // —— 快照 ——
    SnapshotNoPath,
    SnapshotNoResults,
    /// 参数：`原因`
    SnapshotDeleteFailed,
    /// 参数：`网段`、`原因`
    SnapshotBadPrefix,
    /// 参数：`快照目标数`、`网段`、`重算出的目标数`
    SnapshotTargetMismatch,
    /// 参数：`下标`、`目标总数`
    SnapshotIndexOutOfRange,
}

/// 键 → 稳定序号。
///
/// 这个 `match` 是**穷尽**的：新增 [`Key`] 变体却忘了在这里登记，编译就会失败。
/// 登记了却忘了加进 [`TABLE`]，[`tests::table_covers_every_key`] 会因为序号不连续而失败。
pub const fn ordinal(key: Key) -> usize {
    match key {
        Key::UnknownReason => 0,
        Key::EnrichReadFailed => 1,
        Key::EnrichNoFill => 2,
        Key::EnrichFilled => 3,
        Key::EnrichNoResults => 4,
        Key::FingerprintNoOnlineHosts => 5,
        Key::ExportNoResults => 6,
        Key::ExportBadExtension => 7,
        Key::ExportFormatHtml => 8,
        Key::ExportDone => 9,
        Key::WriteFailed => 10,
        Key::DiffNoBaseline => 11,
        Key::DiffOnlyCsv => 12,
        Key::DiffNeedCsv => 13,
        Key::DiffExportDone => 14,
        Key::TrendEmptyHint => 15,
        Key::TrendNoData => 16,
        Key::UptimeReportOnlyCsvHtml => 17,
        Key::UptimeReportDone => 18,
        Key::AlertEmptyPrimed => 19,
        Key::AlertEmptyDisabled => 20,
        Key::AlertBadUptimeThreshold => 21,
        Key::AlertUnsupportedKinds => 22,
        Key::CannotDetermineSaveDir => 23,
        Key::CannotCreateDir => 24,
        Key::CannotDetermineConfigDir => 25,
        Key::CannotCreateConfigDir => 26,
        Key::WriteConfigFailed => 27,
        Key::CannotDetermineAppDataDir => 28,
        Key::IndexNoResult => 29,
        Key::TaskSwitched => 30,
        Key::SnapshotNoPath => 31,
        Key::SnapshotNoResults => 32,
        Key::SnapshotDeleteFailed => 33,
        Key::SnapshotBadPrefix => 34,
        Key::SnapshotTargetMismatch => 35,
        Key::SnapshotIndexOutOfRange => 36,
    }
}

/// 字典里的键总数。
pub const KEY_COUNT: usize = 37;

/// 键的**稳定机器码**（小写下划线）。
///
/// 存在的理由与 `Lang::code()`、`CoreError::code()` 一样：日志、审计记录、外部脚本
/// 需要一个不随枚举重命名而变、也不用链接 Rust 枚举就能指代「某一句话」的抓手。
/// [`key_from_code`] 是它的逆运算。
const fn code_of(key: Key) -> &'static str {
    match key {
        Key::UnknownReason => "unknown_reason",
        Key::EnrichReadFailed => "enrich_read_failed",
        Key::EnrichNoFill => "enrich_no_fill",
        Key::EnrichFilled => "enrich_filled",
        Key::EnrichNoResults => "enrich_no_results",
        Key::FingerprintNoOnlineHosts => "fingerprint_no_online_hosts",
        Key::ExportNoResults => "export_no_results",
        Key::ExportBadExtension => "export_bad_extension",
        Key::ExportFormatHtml => "export_format_html",
        Key::ExportDone => "export_done",
        Key::WriteFailed => "write_failed",
        Key::DiffNoBaseline => "diff_no_baseline",
        Key::DiffOnlyCsv => "diff_only_csv",
        Key::DiffNeedCsv => "diff_need_csv",
        Key::DiffExportDone => "diff_export_done",
        Key::TrendEmptyHint => "trend_empty_hint",
        Key::TrendNoData => "trend_no_data",
        Key::UptimeReportOnlyCsvHtml => "uptime_report_only_csv_html",
        Key::UptimeReportDone => "uptime_report_done",
        Key::AlertEmptyPrimed => "alert_empty_primed",
        Key::AlertEmptyDisabled => "alert_empty_disabled",
        Key::AlertBadUptimeThreshold => "alert_bad_uptime_threshold",
        Key::AlertUnsupportedKinds => "alert_unsupported_kinds",
        Key::CannotDetermineSaveDir => "cannot_determine_save_dir",
        Key::CannotCreateDir => "cannot_create_dir",
        Key::CannotDetermineConfigDir => "cannot_determine_config_dir",
        Key::CannotCreateConfigDir => "cannot_create_config_dir",
        Key::WriteConfigFailed => "write_config_failed",
        Key::CannotDetermineAppDataDir => "cannot_determine_app_data_dir",
        Key::IndexNoResult => "index_no_result",
        Key::TaskSwitched => "task_switched",
        Key::SnapshotNoPath => "snapshot_no_path",
        Key::SnapshotNoResults => "snapshot_no_results",
        Key::SnapshotDeleteFailed => "snapshot_delete_failed",
        Key::SnapshotBadPrefix => "snapshot_bad_prefix",
        Key::SnapshotTargetMismatch => "snapshot_target_mismatch",
        Key::SnapshotIndexOutOfRange => "snapshot_index_out_of_range",
    }
}

/// 字典条目。
#[derive(Debug, Clone, Copy)]
pub struct Entry {
    pub key: Key,
    pub zh: &'static str,
    pub en: &'static str,
}

/// 全部条目。
///
/// **顺序必须与 [`ordinal`] 严格一致**（[`text`] 按下标取值），
/// 新增条目一律追加到末尾（见模块文档的「坑」一节）。
///
/// 条目里的 `{}` 是顺序占位符，个数与顺序必须与另一种语言一致
/// （[`tests::placeholders_match`] 守着）。不引入具名占位符，是为了让两种语言的
/// 语序可以完全不同——英文的参数顺序碰巧与中文相同只是巧合，不能依赖。
pub static TABLE: &[Entry] = &[
    // —— 邻居表补全 ——
    Entry {
        key: Key::UnknownReason,
        zh: "未知原因",
        en: "unknown reason",
    },
    Entry {
        key: Key::EnrichReadFailed,
        zh: "读取邻居表失败：{}",
        en: "failed to read the neighbour table: {}",
    },
    Entry {
        key: Key::EnrichNoFill,
        zh: "邻居表 {} 条，没有可回填的目标（ARP 表只记录近期通信过的地址）",
        en: "the neighbour table has {} entries and nothing to fill in (the ARP table only keeps recently contacted addresses)",
    },
    Entry {
        key: Key::EnrichFilled,
        zh: "邻居表 {} 条，新填 MAC {} 条、厂商 {} 条",
        en: "the neighbour table has {} entries; filled in {} MAC and {} vendor",
    },
    Entry {
        key: Key::EnrichNoResults,
        zh: "尚无扫描结果可补全",
        en: "no scan results to enrich yet",
    },
    // —— 端口指纹 ——
    Entry {
        key: Key::FingerprintNoOnlineHosts,
        zh: "没有在线主机可做端口指纹（先扫描，或先选中在线行）",
        en: "no online host to fingerprint (run a scan first, or select online rows first)",
    },
    // —— 导出 ——
    Entry {
        key: Key::ExportNoResults,
        zh: "尚无已完成的结果可导出",
        en: "no completed results to export yet",
    },
    Entry {
        key: Key::ExportBadExtension,
        zh: "仅支持 .csv / .txt / .json / .html 扩展名",
        en: "only .csv / .txt / .json / .html extensions are supported",
    },
    Entry {
        key: Key::ExportFormatHtml,
        zh: "HTML 报告",
        en: "HTML report",
    },
    Entry {
        key: Key::ExportDone,
        zh: "已导出 {} 条结果（{}）→ {}",
        en: "exported {} results ({}) → {}",
    },
    Entry {
        key: Key::WriteFailed,
        zh: "写入 {} 失败：{}",
        en: "failed to write {}: {}",
    },
    // —— 差异报告 ——
    Entry {
        key: Key::DiffNoBaseline,
        zh: "还没有可对比的上一轮结果（至少完成两轮扫描）",
        en: "no previous round to compare against yet (at least two scan rounds are required)",
    },
    Entry {
        key: Key::DiffOnlyCsv,
        zh: "差异报告目前只支持 .csv，收到 {}（差异也含在 JSON/HTML 报告的 changes 段里）",
        en: "the diff report only supports .csv so far, got {} (the diff is also part of the changes section of the JSON/HTML report)",
    },
    Entry {
        key: Key::DiffNeedCsv,
        zh: "请用 .csv 扩展名",
        en: "please use a .csv extension",
    },
    Entry {
        key: Key::DiffExportDone,
        zh: "已导出差异报告（{} 项）→ {}",
        en: "exported the diff report ({} entries) → {}",
    },
    // —— 趋势与可用率报表 ——
    Entry {
        key: Key::TrendEmptyHint,
        zh: "还没有历史数据：先扫描一轮，开持续监控可累积更多轮次。",
        en: "no history yet: run one scan; enabling continuous monitoring accumulates more rounds.",
    },
    Entry {
        key: Key::TrendNoData,
        zh: "还没有历史数据可导出（先完成至少一轮扫描）",
        en: "no history to export yet (finish at least one scan round first)",
    },
    Entry {
        key: Key::UptimeReportOnlyCsvHtml,
        zh: "可用率报表只支持 .csv / .html",
        en: "the uptime report only supports .csv / .html",
    },
    Entry {
        key: Key::UptimeReportDone,
        zh: "已导出可用率报表（{} 台主机 · {} 轮）→ {}",
        en: "exported the uptime report ({} hosts · {} rounds) → {}",
    },
    // —— 告警 ——
    Entry {
        key: Key::AlertEmptyPrimed,
        zh: "还没有告警。第一轮只建立基线：新增主机规则从第二轮起才生效（否则扫个 /24 会一次刷出 255 条）。",
        en: "no alerts yet. The first round only builds the baseline: the new-host rule takes effect from the second round (otherwise scanning a /24 would fire 255 alerts at once).",
    },
    Entry {
        key: Key::AlertEmptyDisabled,
        zh: "告警尚未启用。打开后按默认规则（新增主机 / 离线 / 延迟>200ms / 可用率<90%）监测。",
        en: "alerts are not enabled yet. Once enabled they watch with the default rules (new host / offline / latency > 200 ms / uptime < 90%).",
    },
    Entry {
        key: Key::AlertBadUptimeThreshold,
        zh: "可用率阈值必须在 0-100 之间，得到 {}",
        en: "the uptime threshold must be between 0 and 100, got {}",
    },
    Entry {
        key: Key::AlertUnsupportedKinds,
        zh: "告警类别只支持 {}，得到 {}",
        en: "the only supported alert kinds are {}, got {}",
    },
    // —— 目录与路径 ——
    Entry {
        key: Key::CannotDetermineSaveDir,
        zh: "无法确定保存目录：{}",
        en: "cannot determine the save directory: {}",
    },
    Entry {
        key: Key::CannotCreateDir,
        zh: "无法创建目录 {}：{}",
        en: "cannot create the directory {}: {}",
    },
    Entry {
        key: Key::CannotDetermineConfigDir,
        zh: "无法确定配置目录：{}",
        en: "cannot determine the config directory: {}",
    },
    Entry {
        key: Key::CannotCreateConfigDir,
        zh: "无法创建配置目录：{}",
        en: "cannot create the config directory: {}",
    },
    Entry {
        key: Key::WriteConfigFailed,
        zh: "写入配置失败：{}",
        en: "failed to write the config: {}",
    },
    Entry {
        key: Key::CannotDetermineAppDataDir,
        zh: "无法确定应用数据目录：{}",
        en: "cannot determine the app data directory: {}",
    },
    // —— 下标与并发 ——
    Entry {
        key: Key::IndexNoResult,
        zh: "下标 {} 尚无结果",
        en: "index {} has no result yet",
    },
    Entry {
        key: Key::TaskSwitched,
        zh: "扫描任务已切换，本次主机名未写回",
        en: "the scan task has changed; this hostname was not written back",
    },
    // —— 快照 ——
    Entry {
        key: Key::SnapshotNoPath,
        zh: "未指定保存路径",
        en: "no save path specified",
    },
    Entry {
        key: Key::SnapshotNoResults,
        zh: "尚无扫描结果可保存",
        en: "no scan results to save yet",
    },
    Entry {
        key: Key::SnapshotDeleteFailed,
        zh: "删除快照失败：{}",
        en: "failed to delete the snapshot: {}",
    },
    Entry {
        key: Key::SnapshotBadPrefix,
        zh: "快照中的网段 {} 无法解析：{}",
        en: "cannot parse the prefix {} stored in the snapshot: {}",
    },
    Entry {
        key: Key::SnapshotTargetMismatch,
        zh: "快照与当前目标规则不一致（快照记录 {} 个目标，按网段 {} 算出 {} 个）",
        en: "the snapshot does not match the current target rules (the snapshot records {} targets but prefix {} yields {})",
    },
    Entry {
        key: Key::SnapshotIndexOutOfRange,
        zh: "快照中的下标 {} 超出目标范围（0..{}）",
        en: "index {} in the snapshot is out of the target range (0..{})",
    },
];

/// 取某个键在指定语言下的模板。
pub fn text(lang: Lang, key: Key) -> &'static str {
    let idx = ordinal(key);
    // `TABLE` 与 `ordinal` 的一致性由 `table_order_matches_ordinal` 守着；
    // 真跑到这里说明有人绕过了那道测试，此时**返回中文兜底**也好过 panic——
    // 界面显示一句中文，比整个程序崩掉好。
    if let Some(e) = TABLE.get(idx) {
        return match lang {
            Lang::Zh => e.zh,
            Lang::En => e.en,
        };
    }
    match lang {
        Lang::Zh => "（未登记的消息键）",
        Lang::En => "(unregistered message key)",
    }
}

/// 渲染带参数的模板。
///
/// 转发核心层的 [`cping_core::i18n::render_template`]：占位符语义
/// （`{}` 顺序替换、参数不足**原样保留 `{}`**、多余参数忽略）三端必须完全一致，
/// 否则同一句漏参在 CLI 上显示 `{}`、在 GUI 上显示空串，排查时会被误导。
pub fn render(lang: Lang, key: Key, args: &[&str]) -> String {
    cping_core::i18n::render_template(text(lang, key), args)
}

/// 由稳定机器码反查键；未知码返回 `None`。
///
/// `None` 而不是「返回中文兜底」：调用方拿到 `None` 时通常应该**记日志并跳过**，
/// 而不是在界面上显示一句猜出来的话。这与 [`text`] 的兜底策略不同是有意的——
/// `text` 拿到的一定是一个合法枚举值，这里拿到的是一段来路不明的字符串。
pub fn key_from_code(code: &str) -> Option<Key> {
    TABLE.iter().map(|e| e.key).find(|k| code_of(*k) == code)
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// 中日韩字符与全角标点的范围。英文条目里出现这些，说明是漏译或
    /// 「先复制中文再改」时漏改的残留。
    fn has_cjk(s: &str) -> Option<char> {
        s.chars().find(|c| {
            let u = *c as u32;
            (0x3000..=0x303F).contains(&u)      // CJK 标点
                || (0x3040..=0x30FF).contains(&u) // 假名
                || (0x3400..=0x4DBF).contains(&u) // 扩展 A
                || (0x4E00..=0x9FFF).contains(&u) // 基本区
                || (0xF900..=0xFAFF).contains(&u) // 兼容表意
                || (0xFF00..=0xFFEF).contains(&u) // 全角
        })
    }

    fn placeholder_count(s: &str) -> usize {
        s.matches("{}").count()
    }

    #[test]
    fn table_covers_every_key() {
        // 双射：TABLE 的条目数 == 键总数，且每个序号恰好出现一次。
        assert_eq!(
            TABLE.len(),
            KEY_COUNT,
            "TABLE 条目数（{}）与 KEY_COUNT（{}）不一致",
            TABLE.len(),
            KEY_COUNT
        );
        let mut seen = HashSet::new();
        for e in TABLE {
            let o = ordinal(e.key);
            assert!(o < KEY_COUNT, "{:?} 的序号 {o} 越界", e.key);
            assert!(seen.insert(o), "{:?} 的序号 {o} 与其它键重复", e.key);
        }
        assert_eq!(seen.len(), KEY_COUNT, "有键没有出现在 TABLE 里");
    }

    #[test]
    fn table_order_matches_ordinal() {
        // `text()` 是 `TABLE[ordinal(key)]`——按下标取值，不是按 key 查找。
        // 所以 TABLE 的**顺序必须与 ordinal 一致**，而这一点
        // `table_covers_every_key` 是查不出来的：它只看序号集合是不是
        // `0..KEY_COUNT` 的一个排列，把任意两条互换位置它照样通过。
        //
        // 错位的后果是静默的：不崩、不报错，只是所有消息都变成了邻居那一条。
        for (i, e) in TABLE.iter().enumerate() {
            assert_eq!(
                ordinal(e.key),
                i,
                "TABLE 第 {i} 条是 {:?}（序号 {}）——TABLE 必须严格按 ordinal 升序排列",
                e.key,
                ordinal(e.key)
            );
        }
    }

    #[test]
    fn placeholders_match() {
        // 中英条目的 `{}` 个数必须一致：不一致意味着某种语言下会显示出
        // 悬空的 `{}`，或者有个参数被静默丢掉。
        for e in TABLE {
            assert_eq!(
                placeholder_count(e.zh),
                placeholder_count(e.en),
                "{:?} 的占位符个数不一致：zh={:?} en={:?}",
                e.key,
                e.zh,
                e.en
            );
        }
    }

    #[test]
    fn english_has_no_cjk() {
        for e in TABLE {
            if let Some(c) = has_cjk(e.en) {
                panic!("{:?} 的英文条目里出现了中日韩字符 {c:?}：{:?}", e.key, e.en);
            }
        }
    }

    #[test]
    fn chinese_entries_are_not_empty_and_not_ascii_only() {
        // 反向检查：中文条目至少含一个中日韩字符。
        // 「英文条目漏译成了中文」由上一个测试抓，「中文条目被误填成英文」
        // 由这个抓——例如复制粘贴时把 en 的内容粘到了 zh 上。
        for e in TABLE {
            assert!(!e.zh.trim().is_empty(), "{:?} 的中文条目为空", e.key);
            assert!(!e.en.trim().is_empty(), "{:?} 的英文条目为空", e.key);
            assert!(
                has_cjk(e.zh).is_some(),
                "{:?} 的中文条目里没有任何中日韩字符，疑似误填成英文：{:?}",
                e.key,
                e.zh
            );
        }
    }

    /// **总闸**：`Lang::En` 下逐键渲染，结果里不许出现中日韩字符。
    ///
    /// 只查 `TABLE` 的 `en` 字段是不够的——那条保证的是**条目**干净，
    /// 保证不了 [`text`] 把 `Lang::En` 映射到了正确的列（把 `zh` 的 `match`
    /// 分支写错、或哪个条目两列填反，表本身仍然自洽）。
    /// 这条从 `(lang, key)` 出发走到最终字符串，才是「英文模式下不再露中文」的证明。
    #[test]
    fn every_key_renders_without_cjk_in_english() {
        for e in TABLE {
            let s = text(Lang::En, e.key);
            if let Some(c) = has_cjk(s) {
                panic!("{:?} 在 Lang::En 下渲染出了中日韩字符 {c:?}：{s:?}", e.key);
            }
            // 中文侧同样走一遍 `text`，确认 `Lang::Zh` 分支没有接到英文列。
            assert_eq!(
                text(Lang::Zh, e.key),
                e.zh,
                "{:?} 的 Lang::Zh 渲染与表中条目不一致",
                e.key
            );
            assert_eq!(
                text(Lang::En, e.key),
                e.en,
                "{:?} 的 Lang::En 渲染与表中条目不一致",
                e.key
            );
        }
    }

    #[test]
    fn render_substitutes_in_order() {
        let s = render(Lang::Zh, Key::ExportDone, &["255", "CSV", "out.csv"]);
        assert_eq!(s, "已导出 255 条结果（CSV）→ out.csv");
        let s = render(Lang::En, Key::WriteFailed, &["out.csv", "denied"]);
        assert_eq!(s, "failed to write out.csv: denied");
    }

    #[test]
    fn render_without_args_uses_the_template_verbatim() {
        assert_eq!(
            render(Lang::Zh, Key::SnapshotNoResults, &[]),
            "尚无扫描结果可保存"
        );
    }

    #[test]
    fn render_keeps_placeholder_when_args_run_short() {
        // 少给一个参数：`{}` 必须**留在原地**，而不是变成空串。
        let s = render(Lang::Zh, Key::ExportDone, &["255"]);
        assert_eq!(s, "已导出 255 条结果（{}）→ {}");
        // 多给参数则忽略，不 panic。
        let s = render(Lang::Zh, Key::SnapshotNoResults, &["多余"]);
        assert_eq!(s, "尚无扫描结果可保存");
    }

    #[test]
    fn key_codes_are_unique_and_round_trip() {
        // 机器码必须唯一：两条消息共用一个码，`key_from_code` 就会静默返回
        // 「表里靠前的那条」，日志与审计记录从此对不上话。
        let mut seen = HashSet::new();
        for e in TABLE {
            let code = code_of(e.key);
            assert!(!code.is_empty(), "{:?} 的机器码为空", e.key);
            assert!(
                code.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "{:?} 的机器码 {code:?} 不是小写下划线形式",
                e.key
            );
            assert!(seen.insert(code), "机器码 {code:?} 被多个键共用");
        }
        // 逆运算必须闭合，否则「按码查键」这条通路就是个摆设。
        for e in TABLE {
            assert_eq!(key_from_code(code_of(e.key)), Some(e.key));
        }
        assert_eq!(key_from_code("no_such_code"), None);
        assert_eq!(key_from_code(""), None);
    }
}
