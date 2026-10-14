//! 轮次历史（M8 引入，M14 的可用率报表在此之上构建）。
//!
//! 只保存**精简样本**：每个目标一行 `(ip, 是否在线, 状态标识, RTT)`。
//! 不保存完整 `ProbeResult`（含 MAC/厂商/主机名），因为历史缓冲区要长期驻留内存，
//! 而目标信息在每轮扫描后都会重新补全。
//!
//! 容量固定，超出后丢弃最旧轮次 —— 保证「跑一整晚」不会把内存吃光。

use crate::result::{ProbeResult, ScanStats};
use std::collections::VecDeque;
use std::net::IpAddr;

/// 默认保留轮数。
pub const DEFAULT_ROUND_CAPACITY: usize = 720;

/// 单主机在某一轮的精简样本。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostSample {
    pub index: u16,
    pub ip: IpAddr,
    pub online: bool,
    /// [`crate::result::ProbeStatus::key`] 的返回值。
    pub status_key: &'static str,
    pub rtt_ms: Option<u32>,
}

impl HostSample {
    pub fn from_result(r: &ProbeResult) -> Self {
        Self {
            index: r.index,
            ip: r.ip,
            online: r.status.is_online(),
            status_key: r.status.key(),
            rtt_ms: r.status.rtt_ms(),
        }
    }
}

/// 一轮扫描的历史记录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoundRecord {
    /// 轮次编号（1 基）。
    pub round: u64,
    /// 该轮提交时刻（Unix 毫秒，由调用方给出）。
    pub timestamp_ms: u64,
    pub elapsed_ms: u64,
    pub stats: ScanStats,
    pub samples: Vec<HostSample>,
}

impl RoundRecord {
    pub fn from_results(
        round: u64,
        timestamp_ms: u64,
        elapsed_ms: u64,
        results: &[ProbeResult],
    ) -> Self {
        Self {
            round,
            timestamp_ms,
            elapsed_ms,
            stats: ScanStats::from_results(results),
            samples: results.iter().map(HostSample::from_result).collect(),
        }
    }

    pub fn bandwidth_len(&self) -> usize {
        self.samples.len()
    }
}

/// 某主机在历史窗口内的可用率。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Uptime {
    /// 被观测到的轮数（只统计该主机出现过的轮次）。
    pub samples: usize,
    pub online: usize,
    /// `online / samples`，`0.0..=1.0`；`samples == 0` 时为 0。
    pub ratio: f64,
}

impl Uptime {
    pub fn percent(&self) -> f64 {
        self.ratio * 100.0
    }

    /// 百分比文本，保留一位小数。
    pub fn percent_text(&self) -> String {
        format!("{:.1}%", self.percent())
    }
}

/// 固定容量的轮次历史。
#[derive(Debug, Clone)]
pub struct HistoryBuffer {
    capacity: usize,
    rounds: VecDeque<RoundRecord>,
}

impl Default for HistoryBuffer {
    fn default() -> Self {
        Self::new(DEFAULT_ROUND_CAPACITY)
    }
}

impl HistoryBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            rounds: VecDeque::new(),
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.rounds.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rounds.is_empty()
    }

    /// 追加一轮；超出容量时丢弃最旧的一轮。
    pub fn push(&mut self, record: RoundRecord) {
        if self.rounds.len() == self.capacity {
            self.rounds.pop_front();
        }
        self.rounds.push_back(record);
    }

    /// 从一组扫描结果直接追加。
    pub fn push_results(
        &mut self,
        round: u64,
        timestamp_ms: u64,
        elapsed_ms: u64,
        results: &[ProbeResult],
    ) -> &RoundRecord {
        self.push(RoundRecord::from_results(
            round,
            timestamp_ms,
            elapsed_ms,
            results,
        ));
        self.rounds.back().expect("just pushed")
    }

    pub fn latest(&self) -> Option<&RoundRecord> {
        self.rounds.back()
    }

    pub fn earliest(&self) -> Option<&RoundRecord> {
        self.rounds.front()
    }

    /// 时间升序迭代。
    pub fn iter(&self) -> impl Iterator<Item = &RoundRecord> {
        self.rounds.iter()
    }

    pub fn clear(&mut self) {
        self.rounds.clear();
    }

    /// 单主机可用率：只统计它真正出现过的轮次。
    pub fn uptime(&self, ip: IpAddr) -> Uptime {
        let mut samples = 0usize;
        let mut online = 0usize;
        for round in &self.rounds {
            if let Some(s) = round.samples.iter().find(|s| s.ip == ip) {
                samples += 1;
                if s.online {
                    online += 1;
                }
            }
        }
        Uptime {
            samples,
            online,
            ratio: if samples == 0 {
                0.0
            } else {
                online as f64 / samples as f64
            },
        }
    }

    /// 单主机平均延迟（只统计在线轮次）；无样本返回 `None`。
    pub fn average_rtt(&self, ip: IpAddr) -> Option<f64> {
        let mut sum = 0u64;
        let mut n = 0u64;
        for round in &self.rounds {
            if let Some(s) = round.samples.iter().find(|s| s.ip == ip) {
                if let Some(rtt) = s.rtt_ms {
                    sum += rtt as u64;
                    n += 1;
                }
            }
        }
        (n > 0).then(|| sum as f64 / n as f64)
    }

    /// 单主机最差延迟；无在线样本返回 `None`。
    pub fn worst_rtt(&self, ip: IpAddr) -> Option<u32> {
        self.rounds
            .iter()
            .filter_map(|r| r.samples.iter().find(|s| s.ip == ip))
            .filter_map(|s| s.rtt_ms)
            .max()
    }

    /// 全部轮次的总体在线率（所有目标所有轮次汇总）。
    pub fn overall_uptime(&self) -> Uptime {
        let mut samples = 0usize;
        let mut online = 0usize;
        for round in &self.rounds {
            for s in &round.samples {
                samples += 1;
                if s.online {
                    online += 1;
                }
            }
        }
        Uptime {
            samples,
            online,
            ratio: if samples == 0 {
                0.0
            } else {
                online as f64 / samples as f64
            },
        }
    }

    /// 各主机在此窗口内出现过的所有 IP（按数值升序）。
    pub fn known_hosts(&self) -> Vec<IpAddr> {
        let mut set: Vec<IpAddr> = Vec::new();
        for round in &self.rounds {
            for s in &round.samples {
                if !set.contains(&s.ip) {
                    set.push(s.ip);
                }
            }
        }
        // 排序口径与导出/差异一致：先 IPv4（按数值）、后 IPv6（按数值）。
        set.sort_by_key(|ip| crate::spec::addr_sort_key(*ip));
        set
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result::ProbeStatus;

    fn res(index: u16, host: u8, status: ProbeStatus) -> ProbeResult {
        ProbeResult::new(
            index,
            IpAddr::V4(std::net::Ipv4Addr::new(10, 0, 0, host)),
            status,
        )
    }

    fn round_of(hosts: &[(u8, ProbeStatus)]) -> Vec<ProbeResult> {
        hosts
            .iter()
            .enumerate()
            .map(|(i, (h, s))| res(i as u16, *h, s.clone()))
            .collect()
    }

    #[test]
    fn buffer_is_bounded() {
        let mut buf = HistoryBuffer::new(3);
        for round in 1..=10u64 {
            buf.push_results(
                round,
                round * 1000,
                5,
                &round_of(&[(1, ProbeStatus::Timeout)]),
            );
        }
        assert_eq!(buf.len(), 3);
        assert_eq!(buf.latest().unwrap().round, 10);
        assert_eq!(buf.earliest().unwrap().round, 8);
    }

    #[test]
    fn uptime_counts_only_rounds_where_the_host_appeared() {
        let mut buf = HistoryBuffer::new(10);
        buf.push_results(
            1,
            0,
            1,
            &round_of(&[(1, ProbeStatus::Online { rtt_ms: 4 })]),
        );
        buf.push_results(2, 1, 1, &round_of(&[(1, ProbeStatus::Timeout)]));
        // 第二轮里主机 2 才出现
        buf.push_results(
            3,
            2,
            1,
            &round_of(&[
                (1, ProbeStatus::Online { rtt_ms: 4 }),
                (2, ProbeStatus::Online { rtt_ms: 9 }),
            ]),
        );

        let up1 = buf.uptime(IpAddr::V4(std::net::Ipv4Addr::new(10, 0, 0, 1)));
        assert_eq!(up1.samples, 3);
        assert_eq!(up1.online, 2);
        assert!((up1.percent() - 66.666_667).abs() < 1e-4);

        let up2 = buf.uptime(IpAddr::V4(std::net::Ipv4Addr::new(10, 0, 0, 2)));
        assert_eq!(up2.samples, 1, "只算它出现过的轮次");
        assert_eq!(up2.percent_text(), "100.0%");

        let unknown = buf.uptime(IpAddr::V4(std::net::Ipv4Addr::new(10, 0, 0, 99)));
        assert_eq!(unknown.samples, 0);
        assert_eq!(unknown.ratio, 0.0);
    }

    #[test]
    fn rtt_aggregates_ignore_offline_rounds() {
        let mut buf = HistoryBuffer::new(10);
        buf.push_results(
            1,
            0,
            1,
            &round_of(&[(1, ProbeStatus::Online { rtt_ms: 10 })]),
        );
        buf.push_results(2, 1, 1, &round_of(&[(1, ProbeStatus::Timeout)]));
        buf.push_results(
            3,
            2,
            1,
            &round_of(&[(1, ProbeStatus::Online { rtt_ms: 30 })]),
        );

        let ip = IpAddr::V4(std::net::Ipv4Addr::new(10, 0, 0, 1));
        assert_eq!(buf.average_rtt(ip), Some(20.0));
        assert_eq!(buf.worst_rtt(ip), Some(30));
        assert_eq!(
            buf.average_rtt(IpAddr::V4(std::net::Ipv4Addr::new(10, 0, 0, 5))),
            None
        );
    }

    #[test]
    fn known_hosts_are_unique_and_sorted_by_numeric_value() {
        let mut buf = HistoryBuffer::new(4);
        buf.push_results(
            1,
            0,
            1,
            &round_of(&[
                (20, ProbeStatus::Timeout),
                (3, ProbeStatus::Timeout),
                (3, ProbeStatus::Timeout),
            ]),
        );
        buf.push_results(2, 1, 1, &round_of(&[(9, ProbeStatus::Timeout)]));
        assert_eq!(
            buf.known_hosts(),
            vec![
                IpAddr::V4(std::net::Ipv4Addr::new(10, 0, 0, 3)),
                IpAddr::V4(std::net::Ipv4Addr::new(10, 0, 0, 9)),
                IpAddr::V4(std::net::Ipv4Addr::new(10, 0, 0, 20))
            ]
        );
    }

    #[test]
    fn overall_uptime_aggregates_every_sample() {
        let mut buf = HistoryBuffer::new(4);
        buf.push_results(
            1,
            0,
            1,
            &round_of(&[
                (1, ProbeStatus::Online { rtt_ms: 1 }),
                (2, ProbeStatus::Timeout),
            ]),
        );
        assert_eq!(buf.overall_uptime().samples, 2);
        assert_eq!(buf.overall_uptime().online, 1);
        assert_eq!(buf.overall_uptime().percent_text(), "50.0%");
    }

    #[test]
    fn round_record_keeps_stats_consistent_with_core() {
        let results = round_of(&[
            (1, ProbeStatus::Online { rtt_ms: 1 }),
            (2, ProbeStatus::Timeout),
            (3, ProbeStatus::Unreachable),
        ]);
        let record = RoundRecord::from_results(7, 1234, 42, &results);
        assert_eq!(record.round, 7);
        assert_eq!(record.elapsed_ms, 42);
        assert_eq!(record.stats.total, 3);
        assert_eq!(record.stats.online, 1);
        assert_eq!(record.stats.timeout, 1);
        assert_eq!(record.stats.unreachable, 1);
        assert_eq!(record.bandwidth_len(), 3);
    }

    #[test]
    fn clear_empties_the_buffer() {
        let mut buf = HistoryBuffer::default();
        buf.push_results(1, 0, 1, &round_of(&[(1, ProbeStatus::Timeout)]));
        assert!(!buf.is_empty());
        buf.clear();
        assert!(buf.is_empty());
        assert!(buf.latest().is_none());
    }
}
