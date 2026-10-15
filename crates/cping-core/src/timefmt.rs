//! 时间格式化（M15 抽出来的公共件）。
//!
//! 这套换算原先在 CLI / TUI / GUI 里各写了一遍（`civil_from_days` 一共三份拷贝），
//! M15 的告警事件需要给时间戳配可读文本，第四份拷贝显然不该再出现——
//! 于是下沉到核心，三端改为调用这里。
//!
//! ## 时区：**如实标注 UTC，不假装本地时间**
//!
//! 核心是零第三方依赖的，拿不到可靠的时区偏移（Windows 与 Unix 的取法完全不同，
//! 且都要求平台 API）。与其猜一个偏移，不如把事实写进字符串：本函数产出
//! `1970-01-01 00:00:00 UTC` 这种带后缀的形式。要显示本地时间由界面层决定，
//! 而不是由这里默默算错。

/// Howard Hinnant 的 `civil_from_days`：1970-01-01 起的天数 → `(年, 月, 日)`。
///
/// 这是公历的标准算法，对所有整数输入都成立（含闰年边界与负天数）。
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Unix 秒 → `YYYY-MM-DD HH:MM:SS UTC`。
pub fn format_unix_local(unix_secs: u64) -> String {
    let days = unix_secs / 86_400;
    let rem = unix_secs % 86_400;
    let (y, m, d) = civil_from_days(days as i64);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// 当前 Unix 秒。取不到时返回 0 —— 由调用方按「时间未知」处理，
/// **不要**在这里伪造一个看起来合理的数值。
pub fn unix_secs_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 当前 Unix 毫秒（时序点与告警事件的时间戳）。
pub fn unix_millis_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_from_days_matches_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(1), (1970, 1, 2));
        assert_eq!(civil_from_days(364), (1970, 12, 31));
        assert_eq!(civil_from_days(365), (1971, 1, 1));
        // 2026-10-15 = 20741 天（用 python 独立核算，不靠心算）
        assert_eq!(civil_from_days(20_741), (2026, 10, 15));
        assert_eq!(civil_from_days(20_740), (2026, 10, 14));
        // 闰年边界
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(11_017), (2000, 3, 1));
    }

    #[test]
    fn formatted_text_is_marked_as_utc_not_local() {
        assert_eq!(format_unix_local(0), "1970-01-01 00:00:00 UTC");
        assert!(format_unix_local(1_700_000_000).ends_with(" UTC"));
        assert_eq!(format_unix_local(1_700_000_000), "2023-11-14 22:13:20 UTC");
    }

    #[test]
    fn clock_helpers_return_plausible_values() {
        // 只断言「不是 0 且落在合理区间」，不跟真实时钟较劲
        assert!(unix_secs_now() > 1_600_000_000);
        assert!(unix_millis_now() > 1_600_000_000_000);
    }
}
