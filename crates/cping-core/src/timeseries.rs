//! 长期时序与可用率报表（M14，v2.2.0）。
//!
//! 与 [`crate::history`] 的分工：
//! - `history.rs` 是**进程内**的轮次缓冲（固定容量、不落盘），用于变化高亮与最近趋势；
//! - 本模块是**可落盘、可跨会话**的长期序列：追加精简样本、按保留期裁剪、
//!   超出规模时**分级降采样**，并据此产出可用率报表。
//!
//! 三条设计约束（都是被踩过的坑）：
//!
//! 1. **格式版本化**：文件头带 `version`，读取时先校验。跨版本必须显式拒绝而不是
//!    「尽力猜」，否则一份被误读的历史会静默污染报表。
//! 2. **坏数据不阻塞读取**：单行解析失败只跳过该行并计数，返回给调用方；
//!    一行人可读的坏行不该让整份历史不可用。
//! 3. **降采样不丢账**：合并只做「同类项相加」，`samples` / `online` / `rtt_sum`
//!    在合并前后必须守恒——报表数值因此可以逐条对账回原始记录。

use crate::classify::Thresholds;
use crate::history::RoundRecord;
use crate::result::{ProbeResult, ProbeStatus};
use std::collections::BTreeMap;
use std::net::IpAddr;

/// 序列文件格式版本。结构变更时递增，并在读取端显式判断。
pub const SERIES_FORMAT_VERSION: u32 = 1;

/// 文件头魔数。
pub const SERIES_SIGNATURE: &str = "CPING-TS";

/// 默认保留期：7 天。
pub const DEFAULT_RETENTION_SECS: u64 = 7 * 24 * 3600;

/// 默认保留的（降采样后）点数上限。
pub const DEFAULT_MAX_POINTS: usize = 2016;

/// 单主机在一个时间点内的聚合样本。
///
/// 刻意只存**可累加**的量：计数、求和、最大值。这样降采样就只是相加，
/// 不存在「平均值再平均」这类会算错的加权问题。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostAgg {
    pub index: u16,
    pub ip: IpAddr,
    /// 该点内的观测轮数。
    pub samples: u32,
    /// 其中在线的轮数。
    pub online: u32,
    /// 在线轮次的 RTT 求和（用于算平均）。
    pub rtt_sum: u64,
    /// 观测到的最差 RTT。
    pub rtt_max: u32,
}

impl HostAgg {
    fn online_ratio(&self) -> f64 {
        if self.samples == 0 {
            0.0
        } else {
            self.online as f64 / self.samples as f64
        }
    }

    fn average_rtt(&self) -> Option<f64> {
        if self.online == 0 {
            None
        } else {
            Some(self.rtt_sum as f64 / self.online as f64)
        }
    }

    /// 合并另一份聚合（降采样用）。要求同一台主机。
    fn merge(&mut self, other: &HostAgg) {
        self.samples += other.samples;
        self.online += other.online;
        self.rtt_sum += other.rtt_sum;
        self.rtt_max = self.rtt_max.max(other.rtt_max);
        // index 取较小的：同一 IP 在不同轮次里下标可能因目标规格变化而不同，
        // 报表按下标排序时以最早出现的为准，避免顺序抖动。
        self.index = self.index.min(other.index);
    }
}

/// 序列中的一个时间点。
///
/// `span_ms` 是它覆盖的时间跨度：原始点是一轮（跨度 = 与上一点的时间差），
/// 降采样后的点覆盖多轮。`rounds` 记录合并了几轮——报表要据此说明「这是几轮的账」。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeriesPoint {
    /// 该点覆盖区间的**起点**（Unix 毫秒）。
    pub at_ms: u64,
    /// 覆盖的时间跨度（毫秒）。
    pub span_ms: u64,
    /// 合并的轮数。
    pub rounds: u32,
    pub hosts: Vec<HostAgg>,
}

impl SeriesPoint {
    /// 从一轮扫描结果构造（未经降采样）。
    pub fn from_results(at_ms: u64, span_ms: u64, results: &[ProbeResult]) -> Self {
        let mut hosts: Vec<HostAgg> = Vec::with_capacity(results.len());
        for r in results {
            let online = r.status.is_online();
            hosts.push(HostAgg {
                index: r.index,
                ip: r.ip,
                samples: 1,
                online: if online { 1 } else { 0 },
                rtt_sum: r.status.rtt_ms().map(u64::from).unwrap_or(0),
                rtt_max: r.status.rtt_ms().unwrap_or(0),
            });
        }
        hosts.sort_by_key(|h| (h.index, h.ip));
        Self {
            at_ms,
            span_ms,
            rounds: 1,
            hosts,
        }
    }

    /// 从 [`RoundRecord`] 构造（复用进程内缓冲）。
    pub fn from_record(record: &RoundRecord, span_ms: u64) -> Self {
        let mut hosts: Vec<HostAgg> = record
            .samples
            .iter()
            .map(|s| HostAgg {
                index: s.index,
                ip: s.ip,
                samples: 1,
                online: if s.online { 1 } else { 0 },
                rtt_sum: s.rtt_ms.map(u64::from).unwrap_or(0),
                rtt_max: s.rtt_ms.unwrap_or(0),
            })
            .collect();
        hosts.sort_by_key(|h| (h.index, h.ip));
        Self {
            at_ms: record.timestamp_ms,
            span_ms,
            rounds: 1,
            hosts,
        }
    }

    /// 该点的结束时刻。
    pub fn end_ms(&self) -> u64 {
        self.at_ms.saturating_add(self.span_ms)
    }

    /// 该点内的整体在线比例（各主机轮次加权）。
    pub fn online_ratio(&self) -> f64 {
        let samples: u64 = self.hosts.iter().map(|h| h.samples as u64).sum();
        let online: u64 = self.hosts.iter().map(|h| h.online as u64).sum();
        if samples == 0 {
            0.0
        } else {
            online as f64 / samples as f64
        }
    }

    /// 合并另一个时间点（要求 `other` 在时间上紧随其后）。
    pub fn merge(&mut self, other: &SeriesPoint) {
        let end = other.end_ms();
        self.span_ms = end.saturating_sub(self.at_ms);
        self.rounds += other.rounds;
        let mut map: BTreeMap<IpAddr, HostAgg> = self.hosts.drain(..).map(|h| (h.ip, h)).collect();
        for h in &other.hosts {
            match map.get_mut(&h.ip) {
                Some(existing) => existing.merge(h),
                None => {
                    map.insert(h.ip, h.clone());
                }
            }
        }
        self.hosts = map.into_values().collect();
        self.hosts.sort_by_key(|h| (h.index, h.ip));
    }
}

/// 可落盘的长期序列。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeSeries {
    pub version: u32,
    /// 保留期（秒）：早于 `最新点时间 - retention` 的点会被裁剪。
    pub retention_secs: u64,
    /// 按时间升序。
    pub points: Vec<SeriesPoint>,
    /// 降采样后的点数上限。
    pub max_points: usize,
}

impl Default for TimeSeries {
    fn default() -> Self {
        Self::new()
    }
}

impl TimeSeries {
    pub fn new() -> Self {
        Self {
            version: SERIES_FORMAT_VERSION,
            retention_secs: DEFAULT_RETENTION_SECS,
            points: Vec::new(),
            max_points: DEFAULT_MAX_POINTS,
        }
    }

    pub fn len(&self) -> usize {
        self.points.len()
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    pub fn latest(&self) -> Option<&SeriesPoint> {
        self.points.last()
    }

    /// 时间跨度（毫秒）：最早点起点 → 最晚点终点。
    pub fn span_ms(&self) -> u64 {
        match (self.points.first(), self.points.last()) {
            (Some(a), Some(b)) => b.end_ms().saturating_sub(a.at_ms),
            _ => 0,
        }
    }

    /// 累计观测轮数（含降采样合并掉的）。
    pub fn total_rounds(&self) -> u64 {
        self.points.iter().map(|p| p.rounds as u64).sum()
    }

    /// 追加一轮。返回是否真的新增（时间倒退的点会被拒绝，避免序列乱序）。
    ///
    /// `span_ms` 一律由本方法维护：**上一个点的窗口延伸到本点起点**
    /// （即 `span` 表示「这个点代表的时间窗往后覆盖多久」）。这样合并时
    /// `end_ms()` 才等于「窗口真正结束的时刻」，裁剪与曲线的时间轴才不会错位。
    pub fn append(&mut self, mut point: SeriesPoint) -> bool {
        if let Some(last) = self.points.last() {
            if point.at_ms < last.at_ms {
                return false;
            }
            let gap = point.at_ms - last.at_ms;
            if let Some(prev) = self.points.last_mut() {
                prev.span_ms = gap;
            }
        }
        point.span_ms = 0;
        self.points.push(point);
        self.compact_if_needed();
        true
    }

    /// 从一轮扫描结果追加。
    pub fn append_results(&mut self, at_ms: u64, results: &[ProbeResult]) -> bool {
        self.append(SeriesPoint::from_results(at_ms, 0, results))
    }

    /// 按保留期裁剪（相对**最新点**的时间，而不是「现在」）。
    ///
    /// 用最新点做基准是有意的：一份历史文件隔几天再打开时，若按「现在」裁剪，
    /// 整份文件会被一次清空——那是数据丢失，不是保留策略。
    pub fn prune(&mut self) -> usize {
        let Some(last) = self.points.last() else {
            return 0;
        };
        let cutoff = last.end_ms().saturating_sub(self.retention_secs * 1000);
        let before = self.points.len();
        self.points.retain(|p| p.end_ms() > cutoff);
        before - self.points.len()
    }

    /// 分级降采样：点数超过 `max_points` 时，把**最旧的一半**两两合并。
    ///
    /// 之所以「从旧的一端开始合并」：近期的分辨率最有价值，历史趋势粗一点没关系。
    /// 合并只做同类项相加，`samples` / `online` / `rtt_sum` 守恒。
    pub fn downsample(&mut self) -> usize {
        let mut merges = 0usize;
        while self.points.len() > self.max_points && self.points.len() >= 2 {
            let half = (self.points.len() / 2).max(1);
            let drained: Vec<SeriesPoint> = self.points.drain(..).collect();
            let mut merged: Vec<SeriesPoint> = Vec::with_capacity(half + 1);
            let mut iter = drained.into_iter().peekable();
            while let Some(mut cur) = iter.next() {
                // 在前半段里成对合并
                if merged.len() < half && iter.peek().is_some() {
                    let next = iter.next().expect("peek 已确认有值");
                    cur.merge(&next);
                    merges += 1;
                }
                merged.push(cur);
            }
            self.points = merged;
        }
        merges
    }

    fn compact_if_needed(&mut self) {
        self.prune();
        if self.points.len() > self.max_points {
            self.downsample();
        }
    }

    /// 全部出现过的主机 IP（按出现顺序稳定：先按下标，再按 IP）。
    pub fn hosts(&self) -> Vec<IpAddr> {
        let mut seen: Vec<(u16, IpAddr)> = Vec::new();
        for p in &self.points {
            for h in &p.hosts {
                if !seen.iter().any(|(_, ip)| *ip == h.ip) {
                    seen.push((h.index, h.ip));
                }
            }
        }
        seen.sort();
        seen.into_iter().map(|(_, ip)| ip).collect()
    }

    /// 逐台主机汇总成可用率报表。
    pub fn report(&self) -> Vec<HostReport> {
        let mut out: Vec<HostReport> = Vec::new();
        for ip in self.hosts() {
            let mut samples = 0u64;
            let mut online = 0u64;
            let mut rtt_sum: u64 = 0;
            let mut rtt_max: u32 = 0;
            let mut points_with_sample = 0usize;
            // 抖动：相邻**在线**轮次的平均 RTT 之差的平均绝对值
            let mut per_point_avg: Vec<f64> = Vec::new();
            for p in &self.points {
                if let Some(h) = p.hosts.iter().find(|h| h.ip == ip) {
                    samples += h.samples as u64;
                    online += h.online as u64;
                    rtt_sum += h.rtt_sum;
                    rtt_max = rtt_max.max(h.rtt_max);
                    points_with_sample += 1;
                    if let Some(avg) = h.average_rtt() {
                        per_point_avg.push(avg);
                    }
                }
            }
            let jitter = if per_point_avg.len() < 2 {
                0.0
            } else {
                let mut sum = 0.0;
                for w in per_point_avg.windows(2) {
                    sum += (w[1] - w[0]).abs();
                }
                sum / (per_point_avg.len() - 1) as f64
            };
            out.push(HostReport {
                ip,
                samples,
                online,
                uptime_ratio: if samples == 0 {
                    0.0
                } else {
                    online as f64 / samples as f64
                },
                avg_rtt_ms: if online == 0 {
                    None
                } else {
                    Some(rtt_sum as f64 / online as f64)
                },
                // 用 `online == 0` 判「从未在线」，**不是** `rtt_max == 0`：
                // 回环地址的 RTT 就是 0ms，拿 0 当哨兵会把「在线且极快」误报成「没测到」。
                worst_rtt_ms: if online == 0 { None } else { Some(rtt_max) },
                jitter_ms: jitter,
                points: points_with_sample,
            });
        }
        out
    }

    /// 某主机的在线率曲线（每个点一格；该点没有该主机时为 `None`）。
    pub fn uptime_curve(&self, ip: IpAddr) -> Vec<Option<f64>> {
        self.points
            .iter()
            .map(|p| {
                p.hosts
                    .iter()
                    .find(|h| h.ip == ip)
                    .map(|h| h.online_ratio())
            })
            .collect()
    }

    /// 「主机 → 累计可用率（0.0..=1.0）」的一次性映射。
    ///
    /// 给 M15 的告警引擎用：`UptimeBelow` 规则要对**每台主机**判一次，
    /// 若在规则里逐台扫全部时间点就是 O(点数 × 主机数)。这里一次扫完。
    /// 从没被观测过的主机不会出现在结果里（调用方据此按 `None` 处理，
    /// 也就是「没有历史数据，不判可用率」——不外推、不臆造）。
    pub fn uptime_map(&self) -> BTreeMap<IpAddr, f64> {
        let mut online: BTreeMap<IpAddr, u64> = BTreeMap::new();
        let mut samples: BTreeMap<IpAddr, u64> = BTreeMap::new();
        for p in &self.points {
            for h in &p.hosts {
                *online.entry(h.ip).or_insert(0) += h.online as u64;
                *samples.entry(h.ip).or_insert(0) += h.samples as u64;
            }
        }
        online
            .into_iter()
            .map(|(ip, on)| {
                let total = samples.get(&ip).copied().unwrap_or(0);
                let ratio = if total == 0 {
                    0.0
                } else {
                    on as f64 / total as f64
                };
                (ip, ratio)
            })
            .collect()
    }

    // ------------------------------------------------------------------
    // 落盘（行式文本，零依赖）
    // ------------------------------------------------------------------

    /// 序列化为文本。格式：
    ///
    /// ```text
    /// CPING-TS 1
    /// retention 604800
    /// max-points 2016
    /// point <at_ms> <span_ms> <rounds>
    /// host <index> <ip> <samples> <online> <rtt_sum> <rtt_max>
    /// end
    /// ```
    pub fn to_text(&self) -> String {
        let mut out = String::with_capacity(64 + self.points.len() * 32);
        out.push_str(&format!("{SERIES_SIGNATURE} {}\n", self.version));
        out.push_str(&format!("retention {}\n", self.retention_secs));
        out.push_str(&format!("max-points {}\n", self.max_points));
        for p in &self.points {
            out.push_str(&format!("point {} {} {}\n", p.at_ms, p.span_ms, p.rounds));
            for h in &p.hosts {
                out.push_str(&format!(
                    "host {} {} {} {} {} {}\n",
                    h.index, h.ip, h.samples, h.online, h.rtt_sum, h.rtt_max
                ));
            }
            out.push_str("end\n");
        }
        out
    }

    /// 从文本解析。返回 `(序列, 跳过的坏行数)`。
    ///
    /// **坏行只跳过、不报错**：一份历史里的个别坏行不该让整份数据不可用。
    /// 但**版本不符会报错**：格式变了就必须显式拒绝，不能猜着读。
    pub fn from_text(text: &str) -> Result<(Self, usize), String> {
        let mut series = TimeSeries::new();
        let mut skipped = 0usize;
        let mut lines = text.lines();
        let mut current: Option<SeriesPoint> = None;

        match lines.next() {
            Some(first) => {
                let mut parts = first.split_whitespace();
                let sig = parts.next().unwrap_or("");
                if sig != SERIES_SIGNATURE {
                    return Err(format!("不是 CPing 时序文件（头 {sig:?}）"));
                }
                match parts.next().and_then(|v| v.parse::<u32>().ok()) {
                    Some(v) if v == SERIES_FORMAT_VERSION => {}
                    Some(v) => {
                        return Err(format!(
                            "时序文件版本 {v} 与当前 {SERIES_FORMAT_VERSION} 不符，拒绝读取"
                        ))
                    }
                    None => return Err("时序文件缺少版本号".into()),
                }
            }
            None => return Err("时序文件为空".into()),
        }

        for line in lines {
            let line = line.trim_end_matches('\r');
            if line.is_empty() {
                continue;
            }
            let mut it = line.split_whitespace();
            match it.next() {
                Some("retention") => match it.next().and_then(|v| v.parse::<u64>().ok()) {
                    Some(v) => series.retention_secs = v,
                    None => skipped += 1,
                },
                Some("max-points") => match it.next().and_then(|v| v.parse::<usize>().ok()) {
                    Some(v) => series.max_points = v.max(1),
                    None => skipped += 1,
                },
                Some("point") => {
                    // 上一个点没写 end 也要收下，但计入跳过
                    if let Some(p) = current.take() {
                        series.points.push(p);
                        skipped += 1;
                    }
                    let at = it.next().and_then(|v| v.parse::<u64>().ok());
                    let span = it.next().and_then(|v| v.parse::<u64>().ok());
                    let rounds = it.next().and_then(|v| v.parse::<u32>().ok());
                    match (at, span, rounds) {
                        (Some(at_ms), Some(span_ms), Some(rounds)) => {
                            current = Some(SeriesPoint {
                                at_ms,
                                span_ms,
                                rounds,
                                hosts: Vec::new(),
                            });
                        }
                        _ => skipped += 1,
                    }
                }
                Some("host") => {
                    let Some(point) = current.as_mut() else {
                        skipped += 1;
                        continue;
                    };
                    let index = it.next().and_then(|v| v.parse::<u16>().ok());
                    let ip = it.next().and_then(|v| v.parse::<IpAddr>().ok());
                    let samples = it.next().and_then(|v| v.parse::<u32>().ok());
                    let online = it.next().and_then(|v| v.parse::<u32>().ok());
                    let rtt_sum = it.next().and_then(|v| v.parse::<u64>().ok());
                    let rtt_max = it.next().and_then(|v| v.parse::<u32>().ok());
                    match (index, ip, samples, online, rtt_sum, rtt_max) {
                        (
                            Some(index),
                            Some(ip),
                            Some(samples),
                            Some(online),
                            Some(rtt_sum),
                            Some(rtt_max),
                        ) => {
                            point.hosts.push(HostAgg {
                                index,
                                ip,
                                samples,
                                online,
                                rtt_sum,
                                rtt_max,
                            });
                        }
                        _ => skipped += 1,
                    }
                }
                Some("end") => {
                    if let Some(p) = current.take() {
                        series.points.push(p);
                    } else {
                        skipped += 1;
                    }
                }
                _ => skipped += 1,
            }
        }
        if let Some(p) = current.take() {
            series.points.push(p);
        }
        series.version = SERIES_FORMAT_VERSION;
        Ok((series, skipped))
    }

    pub fn save_to_path(&self, path: &str) -> Result<(), String> {
        std::fs::write(path, self.to_text()).map_err(|e| format!("写入 {path} 失败：{e}"))
    }

    pub fn load_from_path(path: &str) -> Result<(Self, usize), String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("读取 {path} 失败：{e}"))?;
        Self::from_text(&text)
    }
}

/// 单台主机的可用率报表行。
#[derive(Debug, Clone, PartialEq)]
pub struct HostReport {
    pub ip: IpAddr,
    /// 观测轮数（含降采样合并的）。
    pub samples: u64,
    pub online: u64,
    /// `online / samples`，`0.0..=1.0`。
    pub uptime_ratio: f64,
    /// 在线轮次的平均 RTT；从未在线为 `None`（**不写 0**）。
    pub avg_rtt_ms: Option<f64>,
    /// 最差 RTT；从未在线为 `None`。
    pub worst_rtt_ms: Option<u32>,
    /// 抖动：相邻点的平均 RTT 之差的平均绝对值。
    pub jitter_ms: f64,
    /// 出现过该主机的时间点数。
    pub points: usize,
}

impl HostReport {
    pub fn uptime_percent(&self) -> f64 {
        self.uptime_ratio * 100.0
    }

    pub fn uptime_text(&self) -> String {
        format!("{:.1}%", self.uptime_percent())
    }

    pub fn avg_rtt_text(&self) -> String {
        match self.avg_rtt_ms {
            Some(v) => format!("{v:.1}"),
            None => String::from("-"),
        }
    }

    pub fn worst_rtt_text(&self) -> String {
        match self.worst_rtt_ms {
            Some(v) => v.to_string(),
            None => String::from("-"),
        }
    }

    pub fn jitter_text(&self) -> String {
        format!("{:.1}", self.jitter_ms)
    }
}

// ---------------------------------------------------------------------------
// 图表：纯函数，三端共用（GUI 用 SVG，TUI 用字符柱）
// ---------------------------------------------------------------------------

/// 在线率曲线的 SVG（纯折线，不引图表库）。
///
/// `values` 里 `None` 表示该点没有这台主机的样本——画成**断线**而不是 0，
/// 因为「没测到」和「测到离线」是两件事。
pub fn sparkline_svg(values: &[Option<f64>], width: u32, height: u32) -> String {
    let w = width.max(16) as f64;
    let h = height.max(12) as f64;
    let pad = 2.0;
    let n = values.len();
    let usable_h = h - pad * 2.0;
    let mut out = String::new();
    out.push_str(&format!(
        "<svg class=\"spark\" viewBox=\"0 0 {w} {h}\" width=\"{w}\" height=\"{h}\" role=\"img\" aria-label=\"在线率曲线\">"
    ));
    // 背景 + 三条参考线（100% / 50% / 0%）
    for frac in [0.0f64, 0.5, 1.0] {
        let y = pad + usable_h * (1.0 - frac);
        out.push_str(&format!(
            "<line x1=\"{pad}\" y1=\"{y:.2}\" x2=\"{:.2}\" y2=\"{y:.2}\" stroke=\"#e5e7eb\" stroke-width=\"1\"/>",
            w - pad
        ));
    }
    if n == 0 {
        out.push_str(&format!(
            "<text x=\"{:.2}\" y=\"{:.2}\" font-size=\"9\" fill=\"#9ca3af\">无数据</text>",
            pad + 2.0,
            h / 2.0
        ));
        out.push_str("</svg>");
        return out;
    }
    let step = if n <= 1 {
        0.0
    } else {
        (w - pad * 2.0) / (n - 1) as f64
    };
    // 连续段：遇到 None 就断开
    let mut segment: Vec<(f64, f64)> = Vec::new();
    let flush = |seg: &mut Vec<(f64, f64)>, out: &mut String| {
        if seg.len() >= 2 {
            let pts = seg
                .iter()
                .map(|(x, y)| format!("{x:.2},{y:.2}"))
                .collect::<Vec<_>>()
                .join(" ");
            out.push_str(&format!(
                "<polyline points=\"{pts}\" fill=\"none\" stroke=\"#2563eb\" stroke-width=\"1.6\"/>"
            ));
        } else if seg.len() == 1 {
            let (x, y) = seg[0];
            out.push_str(&format!(
                "<circle cx=\"{x:.2}\" cy=\"{y:.2}\" r=\"1.4\" fill=\"#2563eb\"/>"
            ));
        }
        seg.clear();
    };
    for (i, v) in values.iter().enumerate() {
        match v {
            Some(ratio) => {
                let x = pad + step * i as f64;
                let y = pad + usable_h * (1.0 - ratio.clamp(0.0, 1.0));
                segment.push((x, y));
            }
            None => flush(&mut segment, &mut out),
        }
    }
    flush(&mut segment, &mut out);
    out.push_str("</svg>");
    out
}

/// 字符柱状图（TUI 用）：每个字符一格，`None` 用 `·`，按比例给 8 档高度。
pub fn char_bar_chart(values: &[Option<f64>], width: usize) -> Vec<String> {
    const LEVELS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let w = width.max(1);
    // 超过宽度时按块**取平均**（None 不参与平均，整块全 None 才算 None）
    let cells: Vec<Option<f64>> = if values.len() <= w {
        values.to_vec()
    } else {
        let chunk = values.len().div_ceil(w);
        values
            .chunks(chunk)
            .map(|c| {
                let vals: Vec<f64> = c.iter().filter_map(|v| *v).collect();
                if vals.is_empty() {
                    None
                } else {
                    Some(vals.iter().sum::<f64>() / vals.len() as f64)
                }
            })
            .collect()
    };
    let line: String = cells
        .iter()
        .map(|v| match v {
            None => '·',
            Some(r) => {
                let idx = (r.clamp(0.0, 1.0) * 7.0).round() as usize;
                LEVELS[idx.min(7)]
            }
        })
        .collect();
    vec![line]
}

// ---------------------------------------------------------------------------
// 报表导出（CSV / HTML）
// ---------------------------------------------------------------------------

/// 把可用率报表渲染成 CSV（含 BOM，与主导出同口径）。
pub fn report_csv(report: &[HostReport]) -> Vec<u8> {
    const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];
    let mut text = String::new();
    text.push_str(
        "IP地址,观测轮数,在线轮数,可用率(%),平均RTT(ms),最差RTT(ms),抖动(ms),时间点数\r\n",
    );
    for r in report {
        text.push_str(&format!(
            "{},{},{},{:.1},{},{},{:.1},{}\r\n",
            r.ip,
            r.samples,
            r.online,
            r.uptime_percent(),
            r.avg_rtt_text(),
            r.worst_rtt_text(),
            r.jitter_ms,
            r.points
        ));
    }
    let mut bytes = Vec::with_capacity(BOM.len() + text.len());
    bytes.extend_from_slice(&BOM);
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

/// 把可用率报表渲染成自包含 HTML（含每台主机一条 SVG 曲线）。
pub fn report_html(series: &TimeSeries, report: &[HostReport]) -> String {
    let mut html = String::with_capacity(8 * 1024);
    html.push_str("<!DOCTYPE html>\n<html lang=\"zh-CN\">\n<head>\n<meta charset=\"utf-8\">\n");
    html.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    html.push_str("<title>CPing 可用率报表</title>\n");
    html.push_str(
        "<style>body{font-family:-apple-system,\"Segoe UI\",\"Microsoft YaHei\",sans-serif;\
         margin:0;padding:24px;background:#f6f7f9;color:#1b1d21}\
         section{background:#fff;border:1px solid #e5e7eb;border-radius:10px;padding:16px;margin-bottom:16px}\
         table{border-collapse:collapse;font-size:13px;width:100%}\
         th,td{border-bottom:1px solid #eef0f3;padding:5px 8px;text-align:left;white-space:nowrap}\
         th{background:#fafbfc}h2{font-size:15px;margin:0 0 12px;color:#374151}\
         .spark{display:block}code{font-family:ui-monospace,Consolas,monospace}</style>\n");
    html.push_str("</head>\n<body>\n");
    html.push_str("<section>\n<h2>CPing 可用率报表</h2>\n<dl style=\"font-size:13px;margin:0\">\n");
    html.push_str(&format!(
        "<dt>时间跨度</dt><dd>{:.1} 小时</dd>\n",
        series.span_ms() as f64 / 3_600_000.0
    ));
    html.push_str(&format!(
        "<dt>时间点 / 累计轮数</dt><dd>{} 点 · {} 轮</dd>\n",
        series.len(),
        series.total_rounds()
    ));
    html.push_str(&format!(
        "<dt>保留期</dt><dd>{} 天</dd>\n",
        series.retention_secs / 86_400
    ));
    html.push_str("</dl>\n</section>\n");

    html.push_str("<section>\n<h2>汇总</h2>\n<div style=\"overflow:auto\">\n<table>\n<thead><tr>");
    for h in [
        "IP地址",
        "观测轮数",
        "在线轮数",
        "可用率",
        "平均RTT(ms)",
        "最差RTT(ms)",
        "抖动(ms)",
        "在线率曲线",
    ] {
        html.push_str(&format!("<th>{h}</th>"));
    }
    html.push_str("</tr></thead>\n<tbody>\n");
    for r in report {
        let curve = series.uptime_curve(r.ip);
        html.push_str(&format!(
            "<tr><td><code>{}</code></td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{:.1}</td><td>{}</td></tr>\n",
            r.ip,
            r.samples,
            r.online,
            r.uptime_text(),
            r.avg_rtt_text(),
            r.worst_rtt_text(),
            r.jitter_ms,
            sparkline_svg(&curve, 160, 28)
        ));
    }
    html.push_str("</tbody>\n</table>\n</div>\n</section>\n");

    if report.is_empty() {
        html.push_str("<section>还没有历史数据。开启持续监控并保留几轮后再看。</section>\n");
    }
    html.push_str("</body>\n</html>\n");
    html
}

/// 把一组扫描结果折成「该轮相对阈值的延迟档」，用于极简历史记录。
pub fn latency_bucket(status: &ProbeStatus, thresholds: &Thresholds) -> Option<&'static str> {
    crate::classify::classify_status(status, thresholds).map(|c| c.key())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn online(ip: [u8; 4], idx: u16, rtt: u32) -> ProbeResult {
        ProbeResult::new(
            idx,
            IpAddr::V4(Ipv4Addr::new(ip[0], ip[1], ip[2], ip[3])),
            ProbeStatus::Online { rtt_ms: rtt },
        )
    }

    fn offline(ip: [u8; 4], idx: u16) -> ProbeResult {
        ProbeResult::new(
            idx,
            IpAddr::V4(Ipv4Addr::new(ip[0], ip[1], ip[2], ip[3])),
            ProbeStatus::Timeout,
        )
    }

    fn series_with(rounds: usize) -> TimeSeries {
        let mut s = TimeSeries::new();
        for i in 0..rounds {
            let at = 1_700_000_000_000 + (i as u64) * 30_000;
            let results = vec![
                online([192, 168, 1, 1], 0, 10 + i as u32),
                if i % 2 == 0 {
                    online([192, 168, 1, 2], 1, 20)
                } else {
                    offline([192, 168, 1, 2], 1)
                },
            ];
            s.append_results(at, &results);
        }
        s
    }

    #[test]
    fn append_rejects_time_travel() {
        let mut s = TimeSeries::new();
        assert!(s.append_results(1000, &[online([10, 0, 0, 1], 0, 1)]));
        assert!(
            !s.append_results(500, &[online([10, 0, 0, 1], 0, 1)]),
            "时间倒退的点必须被拒绝，否则序列乱序后曲线会错"
        );
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn span_of_a_point_is_its_window_towards_the_next_one() {
        let mut s = TimeSeries::new();
        s.append_results(1_000, &[online([10, 0, 0, 1], 0, 1)]);
        s.append_results(31_000, &[online([10, 0, 0, 1], 0, 1)]);
        assert_eq!(
            s.points[0].span_ms, 30_000,
            "上一个点的窗口延伸到下一点起点"
        );
        assert_eq!(
            s.points[1].span_ms, 0,
            "最新点还没有后继，窗口长度未知（不能凭空给一个数）"
        );
        assert_eq!(s.points[0].end_ms(), 31_000);
    }

    #[test]
    fn prune_uses_latest_point_as_reference_not_wall_clock() {
        let mut s = series_with(10);
        // 最新点 = 起点 + 9*30s；保留期设为 60 秒 → 只留最近两个点
        s.retention_secs = 60;
        let removed = s.prune();
        assert!(removed > 0);
        assert!(s.len() >= 2, "至少要留下保留期内的点");
        assert!(s.span_ms() <= s.retention_secs * 1000 + 1);
    }

    #[test]
    fn downsample_preserves_totals_exactly() {
        let mut s = series_with(64);
        let samples_before: u64 = s
            .points
            .iter()
            .flat_map(|p| &p.hosts)
            .map(|h| h.samples as u64)
            .sum();
        let online_before: u64 = s
            .points
            .iter()
            .flat_map(|p| &p.hosts)
            .map(|h| h.online as u64)
            .sum();
        let rtt_before: u64 = s
            .points
            .iter()
            .flat_map(|p| &p.hosts)
            .map(|h| h.rtt_sum)
            .sum();
        let rounds_before = s.total_rounds();

        s.max_points = 8;
        let merges = s.downsample();
        assert!(merges > 0, "点数超上限时必须真的合并");
        assert!(s.len() <= 8);

        let samples_after: u64 = s
            .points
            .iter()
            .flat_map(|p| &p.hosts)
            .map(|h| h.samples as u64)
            .sum();
        let online_after: u64 = s
            .points
            .iter()
            .flat_map(|p| &p.hosts)
            .map(|h| h.online as u64)
            .sum();
        let rtt_after: u64 = s
            .points
            .iter()
            .flat_map(|p| &p.hosts)
            .map(|h| h.rtt_sum)
            .sum();
        assert_eq!(samples_before, samples_after, "样本数必须守恒");
        assert_eq!(online_before, online_after, "在线数必须守恒");
        assert_eq!(rtt_before, rtt_after, "RTT 求和必须守恒");
        assert_eq!(rounds_before, s.total_rounds(), "轮数必须守恒");
    }

    #[test]
    fn downsample_keeps_worst_rtt_and_merges_the_same_host_only() {
        let mut s = TimeSeries::new();
        s.append_results(1_000, &[online([10, 0, 0, 1], 0, 5)]);
        s.append_results(2_000, &[online([10, 0, 0, 1], 0, 90)]);
        s.append_results(3_000, &[online([10, 0, 0, 2], 1, 7)]);
        s.append_results(4_000, &[online([10, 0, 0, 2], 1, 8)]);
        s.max_points = 2;
        s.downsample();
        assert_eq!(s.points.len(), 2);
        let first = &s.points[0];
        assert_eq!(first.hosts.len(), 1, "不同主机不得被合并成同一条");
        let h1 = &first.hosts[0];
        assert_eq!(h1.ip.to_string(), "10.0.0.1");
        assert_eq!(h1.samples, 2);
        assert_eq!(h1.online, 2);
        assert_eq!(h1.rtt_sum, 95);
        assert_eq!(h1.rtt_max, 90, "最差 RTT 取最大值");
        assert_eq!(first.rounds, 2);
        assert_eq!(
            first.span_ms, 2_000,
            "合并后的窗口 = 两点窗口之和（[1000, 3000)）"
        );

        let second = &s.points[1];
        assert_eq!(second.hosts.len(), 1);
        assert_eq!(second.hosts[0].ip.to_string(), "10.0.0.2");
        assert_eq!(second.hosts[0].samples, 2);
    }

    #[test]
    fn text_roundtrip_is_byte_stable() {
        let s = series_with(5);
        let text = s.to_text();
        let (back, skipped) = TimeSeries::from_text(&text).unwrap();
        assert_eq!(skipped, 0);
        assert_eq!(back.points, s.points);
        assert_eq!(back.retention_secs, s.retention_secs);
        assert_eq!(back.max_points, s.max_points);
        assert_eq!(back.to_text(), text, "写出 → 读回 → 再写出必须逐字节一致");
    }

    #[test]
    fn bad_lines_are_skipped_and_counted_not_fatal() {
        let mut text = series_with(2).to_text();
        text.push_str("这行是垃圾\n");
        text.push_str("host x y z\n");
        text.push_str("point 1 2 3\n");
        let (s, skipped) = TimeSeries::from_text(&text).unwrap();
        assert!(
            skipped >= 2,
            "坏行必须计数返回，让调用方能如实说明：{skipped}"
        );
        assert_eq!(s.len(), 3, "合法点仍然全部读到（含未闭合的最后一点）");
    }

    #[test]
    fn version_mismatch_is_rejected_explicitly() {
        let err = TimeSeries::from_text("CPING-TS 99\n").unwrap_err();
        assert!(err.contains("版本"), "{err}");
        let err2 = TimeSeries::from_text("NOT-CPING 1\n").unwrap_err();
        assert!(err2.contains("不是 CPing"), "{err2}");
        assert!(TimeSeries::from_text("").is_err(), "空文件要报错");
    }

    #[test]
    fn report_matches_raw_records_host_by_host() {
        let s = series_with(10);
        let report = s.report();
        let r1 = report
            .iter()
            .find(|r| r.ip.to_string() == "192.168.1.1")
            .unwrap();
        assert_eq!(r1.samples, 10);
        assert_eq!(r1.online, 10);
        assert!((r1.uptime_percent() - 100.0).abs() < 1e-9);
        // RTT: 10..19 的平均 = 14.5
        assert!((r1.avg_rtt_ms.unwrap() - 14.5).abs() < 1e-9);
        assert_eq!(r1.worst_rtt_ms, Some(19));
        // 抖动：每步 +1，平均绝对差 = 1.0
        assert!((r1.jitter_ms - 1.0).abs() < 1e-9);

        let r2 = report
            .iter()
            .find(|r| r.ip.to_string() == "192.168.1.2")
            .unwrap();
        assert_eq!(r2.samples, 10);
        assert_eq!(r2.online, 5, "偶数轮在线、奇数轮离线");
        assert!((r2.uptime_percent() - 50.0).abs() < 1e-9);
        assert_eq!(r2.worst_rtt_ms, Some(20));
    }

    #[test]
    fn never_online_host_reports_dash_not_zero() {
        let mut s = TimeSeries::new();
        for i in 0..3 {
            s.append_results(1_000 + i * 1_000, &[offline([10, 0, 0, 9], 0)]);
        }
        let r = &s.report()[0];
        assert_eq!(r.online, 0);
        assert_eq!(r.avg_rtt_ms, None, "从未在线必须是 None（不写 0 冒充）");
        assert_eq!(r.avg_rtt_text(), "-");
        assert_eq!(r.worst_rtt_ms, None);
        assert_eq!(r.uptime_text(), "0.0%");
    }

    #[test]
    fn online_with_zero_rtt_still_reports_a_worst_rtt() {
        // 回环地址的 RTT 就是 0ms。把 0 当「没测到」的哨兵，会让一台「在线且极快」
        // 的主机在报表里显示成「-」，是实打实的误报。
        let mut s = TimeSeries::new();
        s.append_results(1_000, &[online([127, 0, 0, 1], 0, 0)]);
        let r = &s.report()[0];
        assert_eq!(r.online, 1);
        assert_eq!(r.avg_rtt_ms, Some(0.0));
        assert_eq!(r.worst_rtt_ms, Some(0), "在线但 RTT=0 也必须给出数值");
        assert_eq!(r.worst_rtt_text(), "0");
        assert_eq!(r.uptime_text(), "100.0%");
    }

    #[test]
    fn uptime_curve_marks_missing_points_as_none() {
        let mut s = TimeSeries::new();
        s.append_results(1_000, &[online([10, 0, 0, 1], 0, 3)]);
        s.append_results(2_000, &[online([10, 0, 0, 2], 1, 4)]); // 只测了 .2
        let curve = s.uptime_curve("10.0.0.1".parse().unwrap());
        assert_eq!(curve, vec![Some(1.0), None], "没测到 ≠ 离线");
    }

    #[test]
    fn sparkline_breaks_the_line_on_missing_points() {
        let svg = sparkline_svg(&[Some(1.0), None, Some(0.5), Some(0.0)], 160, 28);
        assert!(svg.starts_with("<svg"));
        assert!(svg.ends_with("</svg>"));
        assert!(svg.contains("polyline"), "连续段应画成折线");
        assert!(!svg.contains("<script"), "报表必须无脚本");
        // 空数据也要给出可读图形
        let empty = sparkline_svg(&[], 160, 28);
        assert!(empty.contains("无数据"));
        // 单点画成圆点而不是无法渲染的折线
        let one = sparkline_svg(&[Some(1.0)], 160, 28);
        assert!(one.contains("<circle"));
    }

    #[test]
    fn char_bar_chart_is_fixed_width_and_marks_gaps() {
        let chart = char_bar_chart(&[Some(0.0), Some(1.0), None], 10);
        assert_eq!(chart.len(), 1);
        assert_eq!(chart[0].chars().count(), 3);
        assert!(chart[0].contains('·'), "缺口用 · 表示");
        assert_ne!(
            chart[0].chars().next().unwrap(),
            chart[0].chars().nth(1).unwrap()
        );

        // 超宽时按块聚合到指定宽度
        let many: Vec<Option<f64>> = (0..50).map(|i| Some(i as f64 / 49.0)).collect();
        let narrow = char_bar_chart(&many, 10);
        assert_eq!(narrow[0].chars().count(), 10);
    }

    #[test]
    fn csv_and_html_reports_render_without_placeholders_for_missing() {
        let s = series_with(4);
        let report = s.report();
        let csv = report_csv(&report);
        let text = String::from_utf8(csv[3..].to_vec()).unwrap();
        assert!(text.starts_with(
            "IP地址,观测轮数,在线轮数,可用率(%),平均RTT(ms),最差RTT(ms),抖动(ms),时间点数\r\n"
        ));
        assert!(text.contains("192.168.1.1"));
        assert!(text.contains("100.0"));

        let html = report_html(&s, &report);
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("可用率报表"));
        assert!(html.contains("spark"));
        assert!(!html.contains("<script"));

        // 空报表也要能渲染
        let empty = report_html(&TimeSeries::new(), &[]);
        assert!(empty.contains("还没有历史数据"));
    }

    #[test]
    fn latency_bucket_uses_the_shared_classifier() {
        let t = Thresholds::default();
        assert_eq!(
            latency_bucket(&ProbeStatus::Online { rtt_ms: 5 }, &t),
            Some("low")
        );
        assert_eq!(latency_bucket(&ProbeStatus::Timeout, &t), Some("offline"));
        assert_eq!(latency_bucket(&ProbeStatus::Pending, &t), None);
    }

    #[test]
    fn from_record_reuses_in_memory_history() {
        let record = RoundRecord::from_results(
            3,
            1_700_000_000_000,
            120,
            &[online([10, 0, 0, 1], 0, 8), offline([10, 0, 0, 2], 1)],
        );
        let point = SeriesPoint::from_record(&record, 30_000);
        assert_eq!(point.at_ms, 1_700_000_000_000);
        assert_eq!(point.span_ms, 30_000);
        assert_eq!(point.hosts.len(), 2);
        assert!((point.online_ratio() - 0.5).abs() < 1e-9);
    }
}
