//! 单次扫描的**内存预算**（M19 第三步）。
//!
//! 这一步要回答的问题是「万级目标会不会把内存撑爆」。答案不能是
//! 「看起来不大」—— 得是一个**能断言的上界**，而且最好是不随机器变化的上界。
//!
//! 所以这里的每一个常量都由**类型的编译期宽度 × 明确的计数上限**相乘得到，
//! 没有一个来自测量，也没有一个依赖运行时配置之外的输入：
//!
//! | 项 | 来源 | 65536 目标下的量 |
//! | --- | --- | --- |
//! | 结果整表 | `size_of::<Option<ProbeResult>>() × MAX_TARGETS` | 7.50 MiB |
//! | 结果快照 | `size_of::<ProbeResult>() × MAX_TARGETS` | 7.50 MiB |
//! | 目标表 | `size_of::<IpAddr>() × MAX_TARGETS` | 1.06 MiB |
//! | 事件队列上限 | `size_of::<ProbeEvent>() × EVENT_QUEUE_CAPACITY` | 576 KiB |
//! | 每 worker 临时 | 保守估计（见下） | 每路 64 KiB |
//!
//! 编写这一步时被 `every_term_matches_its_registered_width` 抓到的一处**估算错误**：
//! `IpAddr` 是 **17 B** 而不是 16 B —— 它是 `V4`/`V6` 两个变体的枚举，判别式占了
//! 第 17 个字节（两个变体都是 `align = 1`，所以不补齐）。所以目标表不是 1.00 MiB
//! 而是 1.06 MiB。这正是把宽度写成**可断言的登记值**的价值：算错会红，不会悄悄低报。
//!
//! 于是「预算」是这些**结构性开销之和**——任何一项被改宽（比如 `ProbeResult`
//! 里加了个字段、队列容量调大），预算会自动跟着涨，不需要人去改数字。
//!
//! 相关的实测峰值由基准的 `[2d]` 段给出并断言落在预算内（`benches/scan.rs`）。
//!
//! **为什么不把线程栈算进来**：线程栈由操作系统 mmap，不走全局分配器，
//! 所以它既进不了基准的堆峰值读数，也不该混进这份堆预算里（否则预算会虚高，
//! 断言就失去意义）。它由 `PER_WORKER_SCRATCH_BYTES` 之外的机制约束
//! （并发上限见 [`MAX_CONCURRENCY`](crate::MAX_CONCURRENCY)）。

use crate::result::{ProbeEvent, ProbeResult};
use crate::scheduler::EVENT_QUEUE_CAPACITY;
use crate::target::MAX_TARGETS;
use std::net::IpAddr;

/// 结果整表的结构性开销。
///
/// 三端与核心用的都是 `Vec<Option<ProbeResult>>`（未完成的槽位是 `None`），
/// 所以按 `Option<ProbeResult>` 算。实测两者都是 120 B（`ProbeStatus` 的分支
/// 留出了 niche），但**按更宽的那个算**——万一哪天 niche 没了，预算只会更保守，
/// 不会因此低估。
pub const RESULTS_TABLE_BYTES: usize = std::mem::size_of::<Option<ProbeResult>>() * MAX_TARGETS;

/// 结果**快照**的结构性开销（`RunOutcome::results` 那一份）。
///
/// [`SchedulerHandle::run_to_completion`](crate::SchedulerHandle::run_to_completion)
/// 会把结果表拍平成 `Vec<ProbeResult>` 交给调用方：那是一份与结果表**同时存在**
/// 的副本，所以要单独计入预算。
pub const RESULTS_SNAPSHOT_BYTES: usize = std::mem::size_of::<ProbeResult>() * MAX_TARGETS;

/// 目标表的结构性开销（`Vec<IpAddr>`，会一路复制到调度器里）。
pub const TARGETS_VEC_BYTES: usize = std::mem::size_of::<IpAddr>() * MAX_TARGETS;

/// 事件队列上限的结构性开销（M19 第二步把通道改成了有界）。
///
/// 这是全表里**唯一**一个「不随目标数增长」的项——它正是背压的意义所在：
/// 生产者再快，这条通道最多也只占这么多。
pub const EVENT_QUEUE_BYTES: usize = std::mem::size_of::<ProbeEvent>() * EVENT_QUEUE_CAPACITY;

/// 每个 worker 的堆上临时开销（保守估计）。
///
/// 这是全表里**唯一一个「估」出来的数**，所以要说明白它估的是什么：
/// worker 循环里每条结果的少量临时值、探测后端自己的每路状态与缓冲。
/// 取 64 KiB 是一个明显偏宽的量级（真实占用是 KB 级），它**不是**用来精确
/// 预测的，而是用来保证预算不会小到「多开一路就超」。线程栈不计入（见模块说明）。
pub const PER_WORKER_SCRATCH_BYTES: usize = 64 * 1024;

/// 「流式」路径的内存预算：`start()` + 逐事件消费（TUI / GUI / 基准的 `run_scan`）。
///
/// 这条路径**不取结果快照**，峰值就是「结果表 + 事件队列 + 目标表 + 每路临时」。
///
/// 并发度夹到 [`MAX_CONCURRENCY`](crate::MAX_CONCURRENCY)：配置校验保证真实并发
/// 不可能超过它，所以按它算就是上界。**这同时也是一个可移植性考虑**——
/// 若不夹而按 `MAX_TARGETS` 算，32 位平台上 `64 KiB × 65536` 会溢出 `usize`。
pub const fn scan_memory_budget_bytes(concurrency: usize) -> usize {
    // 这里手写夹取而不是 `clamp`：`Ord::clamp` 还没在 `const fn` 里稳定
    // （见 rust-lang/rust#143874），而本函数要能在**编译期**被断言调用。
    let workers = if concurrency < 1 {
        1
    } else if concurrency > crate::config::MAX_CONCURRENCY {
        crate::config::MAX_CONCURRENCY
    } else {
        concurrency
    };
    RESULTS_TABLE_BYTES + EVENT_QUEUE_BYTES + TARGETS_VEC_BYTES + PER_WORKER_SCRATCH_BYTES * workers
}

/// 「取快照」路径的内存预算：`run_to_completion()`（CLI 与测试场景）。
///
/// 比流式预算多一份结果副本——这也是**为什么它要单独列出来**：
/// 只跑流式路径的人不该被快照的 7.5 MiB 算进自己的账上。
pub const fn run_to_completion_budget_bytes(concurrency: usize) -> usize {
    scan_memory_budget_bytes(concurrency) + RESULTS_SNAPSHOT_BYTES
}

// ---------------------------------------------------------------------------
// 结构性断言（编译期）
// ---------------------------------------------------------------------------
//
// 这些不依赖机器、不依赖 `--release`，所以可以放在库里当场挡人：
// 任何一条挂了都是**编译不过**，而不是等到某次跑的断言才红。

// 事件队列上限必须远小于结果整表——否则「有界」就失去意义：
// 上限比结果本身还大，等于没有上限。
const _: () = assert!(EVENT_QUEUE_BYTES < RESULTS_TABLE_BYTES);

// 预算必须**至少**盖住结构性开销之和，否则它就不是上界。
const _: () = assert!(
    scan_memory_budget_bytes(1)
        >= RESULTS_TABLE_BYTES + EVENT_QUEUE_BYTES + TARGETS_VEC_BYTES + PER_WORKER_SCRATCH_BYTES
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DEFAULT_CONCURRENCY, MAX_CONCURRENCY};

    /// 预算必须覆盖三块结构性开销——这是「预算」二字的底线。
    #[test]
    fn budget_covers_every_structural_term() {
        let budget = scan_memory_budget_bytes(DEFAULT_CONCURRENCY);
        assert!(
            budget >= RESULTS_TABLE_BYTES + EVENT_QUEUE_BYTES + TARGETS_VEC_BYTES,
            "预算 {budget} 连结构性开销之和都没盖住"
        );
    }

    /// 并发越多，每路临时开销越多，预算必须单调不减。
    #[test]
    fn budget_grows_with_concurrency() {
        let mut prev = 0usize;
        for c in [1, 2, 8, 32, 128, MAX_CONCURRENCY] {
            let now = scan_memory_budget_bytes(c);
            assert!(now >= prev, "并发 {c} 时预算反而变小了");
            prev = now;
        }
        assert!(
            scan_memory_budget_bytes(2) > scan_memory_budget_bytes(1),
            "两路并发应当比一路多一份临时开销"
        );
    }

    /// 并发度取 0 或大得离谱时必须被夹住，不能算出个负数或爆掉。
    ///
    /// 预算函数**不能依赖调用方守规矩**：它算错只会静默给出一个偏小的上界，
    /// 而偏小的上界比没有上界更糟（会让人以为验过了）。
    #[test]
    fn budget_clamps_absurd_concurrency() {
        assert_eq!(
            scan_memory_budget_bytes(0),
            scan_memory_budget_bytes(1),
            "并发 0 应按 1 路算"
        );
        assert_eq!(
            scan_memory_budget_bytes(usize::MAX),
            scan_memory_budget_bytes(MAX_CONCURRENCY),
            "并发超过配置上限后没有意义，应夹到 MAX_CONCURRENCY"
        );
        // 夹住之后乘法两个因子都有限，32 位平台上也不会溢出
        assert!(scan_memory_budget_bytes(usize::MAX) > 0);
    }

    /// 取快照的预算 = 流式预算 + 正好一份结果副本。
    #[test]
    fn snapshot_budget_is_streaming_plus_one_table() {
        for c in [1, 8, 32, MAX_CONCURRENCY] {
            assert_eq!(
                run_to_completion_budget_bytes(c),
                scan_memory_budget_bytes(c) + RESULTS_SNAPSHOT_BYTES,
                "并发 {c} 时两条预算的关系不对"
            );
        }
    }

    /// 默认与满并发下的预算落在**写进文档**的口径内。
    ///
    /// 这条比对的是「自己承诺的量级」，不是机器性能——所以换机器不会红，
    /// 但谁把某一项的宽度改胖一倍，这里会当场红。
    #[test]
    fn budget_stays_within_the_documented_bound() {
        let budget = scan_memory_budget_bytes(DEFAULT_CONCURRENCY);
        // 文档承诺：默认并发（32 路）下 < 16 MiB（实测口径 11.13 MiB）。
        assert!(
            budget < 16 * 1024 * 1024,
            "默认并发下预算 {budget} B 超出了文档承诺的 16 MiB"
        );
        // 文档承诺：满并发（256 路 × 64 KiB = 16 MiB 临时）下 < 64 MiB（实测口径 25.12 MiB）。
        let max_budget = scan_memory_budget_bytes(MAX_CONCURRENCY);
        assert!(
            max_budget < 64 * 1024 * 1024,
            "满并发下预算 {max_budget} B 超出了文档承诺的 64 MiB"
        );
    }

    /// 逐项把**登记值**对一遍：任何一项被悄悄改宽，这里会红。
    ///
    /// 这里写死的 120 / 144 / 16 是登记在 `docs/perf-baseline.txt` 与本文档表格里的
    /// 类型宽度。**换 rustc 导致布局变化时，正确地做法是同时更新这里的数字与那张表**
    /// ——不是把断言删掉：这三项正是预算成立的前提。
    #[test]
    fn every_term_matches_its_registered_width() {
        assert_eq!(
            std::mem::size_of::<IpAddr>(),
            17,
            "登记值：IpAddr 宽 17 B（16 B 地址 + 1 B 判别式），不是 16"
        );
        assert_eq!(TARGETS_VEC_BYTES, 17 * MAX_TARGETS);
        assert_eq!(
            RESULTS_TABLE_BYTES,
            120 * MAX_TARGETS,
            "登记值：Option<ProbeResult> 宽 120 B"
        );
        assert_eq!(RESULTS_SNAPSHOT_BYTES, 120 * MAX_TARGETS);
        assert_eq!(
            EVENT_QUEUE_BYTES,
            144 * EVENT_QUEUE_CAPACITY,
            "登记值：ProbeEvent 宽 144 B"
        );
        assert_eq!(PER_WORKER_SCRATCH_BYTES, 64 * 1024);
    }
}
