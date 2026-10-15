//! M19 规模化性能基准（`harness = false`，自研计时，**不引 criterion**）。
//!
//! ## 为什么自己写
//!
//! 核心 crate 的承诺是「零第三方运行时依赖」。criterion 会带进二十来个
//! crate，为了量几个数不值得。`harness = false` 的 bench 目标就是一个普通
//! 可执行文件，`cargo bench` 会用 bench profile（≈ release）编译并运行它。
//!
//! ## 量什么
//!
//! 1. **统计口径**：三端原先「每收到一条结果就把整表拍平再全量统计」是 O(n²)，
//!    这里与增量累加并排量，用数字说明差距（不是感觉）。
//! 2. **调度器吞吐**：10000 目标 / 32 并发 / 1s 超时（M19 的回归基线），
//!    用 `MockBackend` 离线跑，量事件数与墙钟时间。
//! 3. **内存峰值**：装一个计数用全局分配器，量扫描期间的真实堆峰值。
//! 4. **事件流**：进度合并比（发出了多少条进度事件）、通道上限。
//!
//! ## 「实测」与「外推」严格分开
//!
//! O(n²) 那条在 65536 规模上真跑要几分钟，本基准**只量到 8192**，
//! 65536 的数字由二次关系外推得出，输出里明确标注 `(外推)`。
//! 其余数字都是实测。

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicUsize, Ordering};

// ---------------------------------------------------------------------------
// 计数用全局分配器：量真实堆峰值
// ---------------------------------------------------------------------------

/// 只在被测阶段计数，避免初始化与输出本身的噪声混进来。
static TRACK: AtomicBool = AtomicBool::new(false);
static CUR: AtomicIsize = AtomicIsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        if !ptr.is_null() && TRACK.load(Ordering::Relaxed) {
            let size = layout.size() as isize;
            let cur = CUR.fetch_add(size, Ordering::Relaxed) + size;
            if cur > 0 {
                PEAK.fetch_max(cur as usize, Ordering::Relaxed);
            }
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if TRACK.load(Ordering::Relaxed) {
            CUR.fetch_sub(layout.size() as isize, Ordering::Relaxed);
        }
        System.dealloc(ptr, layout)
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = System.realloc(ptr, layout, new_size);
        if !new_ptr.is_null() && TRACK.load(Ordering::Relaxed) {
            let delta = new_size as isize - layout.size() as isize;
            let cur = CUR.fetch_add(delta, Ordering::Relaxed) + delta;
            if cur > 0 {
                PEAK.fetch_max(cur as usize, Ordering::Relaxed);
            }
        }
        new_ptr
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

// 内存读数不抽成 `phase_memory(f)` 那样的高阶函数，而是由调用方手动
// 开/关计数（见 `run_scan`）。原因：读数必须在**被测对象仍存活时**取，
// 而 `run_scan` 里的结果表挂在 `handle` 上——若把那几步塞进闭包再返回，
// 表可能已被释放，量到的就不是峰值。少一层抽象换来读数准确，划算。

// ---------------------------------------------------------------------------
// 被测对象
// ---------------------------------------------------------------------------

use cping_core::classify::Thresholds;
use cping_core::policy::{DEFAULT_MAX_PPS, MAX_PPS_LIMIT};
use cping_core::result::{ProbeResult, ProbeStatus, ScanStats};
use cping_core::scheduler::ProbeScheduler;
use cping_core::{MockBackend, MockPolicy, ProbeConfig};
use std::net::{IpAddr, Ipv4Addr};
use std::time::Instant;

/// 造一批结果：分布接近真实网段（约三成在线，其余超时/不可达）。
fn make_results(n: usize) -> Vec<ProbeResult> {
    (0..n)
        .map(|i| {
            let ip = IpAddr::V4(Ipv4Addr::new(
                10,
                ((i / 65_536) % 256) as u8,
                ((i / 256) % 256) as u8,
                (i % 256) as u8,
            ));
            let status = match i % 10 {
                0..=2 => ProbeStatus::Online {
                    rtt_ms: (i % 120) as u32,
                },
                3 => ProbeStatus::Unreachable,
                4 => ProbeStatus::Error("mock failure".into()),
                _ => ProbeStatus::Timeout,
            };
            ProbeResult::new(i as u16, ip, status)
        })
        .collect()
}

/// **今天三端的写法**（原样搬运）：每收到一条结果就拍平整表再全量统计。
///
/// TUI 的 `recompute_stats`、GUI 的 `ProbeEvent::Result` 分支都是这三行。
fn consume_like_today(results: &[ProbeResult]) -> ScanStats {
    let total = results.len();
    let mut slots: Vec<Option<ProbeResult>> = vec![None; total];
    let mut stats = ScanStats::from_results_with_total(&[], total);
    for r in results {
        slots[r.index as usize] = Some(r.clone());
        let done: Vec<ProbeResult> = slots.iter().flatten().cloned().collect();
        stats = ScanStats::from_results_with_total(&done, total);
    }
    stats
}

/// **M19 之后的写法**：增量累加，O(1)。
fn consume_incremental(results: &[ProbeResult]) -> ScanStats {
    let mut stats = ScanStats::for_total(results.len());
    for r in results {
        stats.push_status(&r.status);
    }
    stats
}

// ---------------------------------------------------------------------------
// 输出与基线
// ---------------------------------------------------------------------------

/// 收集到的度量，供 `--check` 与基线文件比对。
struct Report {
    rows: Vec<(String, f64)>,
}

impl Report {
    fn new() -> Self {
        Self { rows: Vec::new() }
    }

    fn record(&mut self, key: &str, value: f64) {
        self.rows.push((key.to_string(), value));
    }

    fn value(&self, key: &str) -> Option<f64> {
        self.rows.iter().find(|(k, _)| k == key).map(|(_, v)| *v)
    }
}

/// 基线文件：一行一个 `键 = 值`，`#` 开头是注释。
///
/// 刻意用「行文本」而不是 JSON：核心 crate 不引 serde，
/// 为了读一个基线文件去加依赖是不划算的。
///
/// **路径按工作区根解析，不按当前目录**：`cargo bench` 会把基准进程的工作目录
/// 设成包目录（`crates/cping-core`），直接用 `docs/...` 会落到
/// `crates/cping-core/docs/...`（不存在）。用编译期的 `CARGO_MANIFEST_DIR`
/// 往上退两级，才能稳定落到仓库根的 `docs/`（实测过：写错路径时报
/// `os error 3 系统找不到指定的路径`）。
fn baseline_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("docs")
        .join("perf-baseline.txt")
}

/// 回归容差。
///
/// 放得很宽（2.5×）是有意的：不同机器的绝对性能差两三倍很正常，
/// 而**算法性回归**（例如增量路径被改回全量重算）会差 60 倍以上。
/// 这样既不会因机器差异误报，又能抓住真正该抓的东西。
const REGRESSION_TOLERANCE: f64 = 2.5;

fn load_baseline() -> Vec<(String, f64)> {
    let Ok(text) = std::fs::read_to_string(baseline_path()) else {
        return Vec::new();
    };
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let (k, v) = l.split_once('=')?;
            Some((k.trim().to_string(), v.trim().parse::<f64>().ok()?))
        })
        .collect()
}

/// 数字显示：微秒级的值不能用一位小数（会被写成 `0.0`）。
fn fmt_num(v: f64) -> String {
    if v.abs() < 1.0 {
        format!("{v:.6}")
    } else if v.abs() < 1000.0 {
        format!("{v:.3}")
    } else {
        format!("{v:.1}")
    }
}

fn check_against_baseline(report: &Report) -> bool {
    let path = baseline_path();
    let baseline = load_baseline();
    if baseline.is_empty() {
        println!(
            "\n[check] 找不到基线 {}，跳过比对（用 --write-baseline 生成）",
            path.display()
        );
        return true;
    }
    println!(
        "\n[check] 与 {} 比对（容差 {REGRESSION_TOLERANCE}×）",
        path.display()
    );
    let mut ok = true;
    for (key, base) in &baseline {
        let Some(now) = report.value(key) else {
            continue;
        };
        // **亚毫秒指标不比对**：这类数字（增量累加的微秒级耗时）在共享机器上
        // 噪声大于信号——实测同一台机器连续两次跑出 0.064 ms 与 0.375 ms，
        // 按容差会误报「回归 5.8×」。它们仍然写进基线当**证据**，但判定交给
        // 代码里那条机器无关的**结构性断言**（批量必须比增量慢 100× 以上）。
        if *base < 1.0 && now < 1.0 {
            println!(
                "  {key:44} 基线 {:>14}  本次 {:>14}  跳过（亚毫秒，噪声大于信号）",
                fmt_num(*base),
                fmt_num(now)
            );
            continue;
        }
        // 约定：metric 名以 `.per_sec` 结尾的是「越大越好」，其余是「越小越好」。
        let higher_is_better = key.ends_with(".per_sec");
        let ratio = if higher_is_better {
            if now <= 0.0 {
                f64::INFINITY
            } else {
                base / now
            }
        } else if *base <= 0.0 {
            1.0
        } else {
            now / base
        };
        let verdict = if ratio > REGRESSION_TOLERANCE {
            ok = false;
            "回归"
        } else {
            "ok"
        };
        println!(
            "  {key:44} 基线 {:>14}  本次 {:>14}  倍率 {ratio:>6.2}  {verdict}",
            fmt_num(*base),
            fmt_num(now)
        );
    }
    ok
}

fn write_baseline(report: &Report) {
    let mut out = String::from(
        "# CPing 性能基线（M19）\n\
         # 由 `cargo bench -p cping-core --bench scan -- --write-baseline` 生成。\n\
         # 格式：键 = 值。键名以 .per_sec 结尾表示「越大越好」，其余表示「越小越好」。\n\
         # 数字随机器变化，比对时用 2.5× 容差，只抓算法性回归。\n",
    );
    for (key, value) in &report.rows {
        // 自适应精度：增量累加是**微秒级**的，固定一位小数会把它写成 `0.0`，
        // 于是基线里最重要的那个数字反而没信息量（实测踩过）。见 `fmt_num`。
        out.push_str(&format!("{key} = {}\n", fmt_num(*value)));
    }
    let path = baseline_path();
    match std::fs::write(&path, out) {
        Ok(()) => println!("\n[baseline] 已写入 {}", path.display()),
        Err(e) => println!("\n[baseline] 写入 {} 失败：{e}", path.display()),
    }
}

fn human(bytes: usize) -> String {
    const KB: f64 = 1024.0;
    let b = bytes as f64;
    if b < KB {
        format!("{bytes} B")
    } else if b < KB * KB {
        format!("{:.1} KiB", b / KB)
    } else {
        format!("{:.2} MiB", b / (KB * KB))
    }
}

// ---------------------------------------------------------------------------
// 各段基准
// ---------------------------------------------------------------------------

/// 1. 统计口径：批量重算 vs 增量累加。
fn bench_stats(report: &mut Report) {
    println!("\n=== 1. 统计口径：批量重算（今天三端的写法） vs 增量累加 ===");
    println!(
        "{:>8}  {:>14}  {:>14}  {:>10}",
        "目标数", "批量重算", "增量累加", "加速比"
    );

    // 批量那边是 O(n²)：65536 真跑要几分钟，所以只量到 8192。
    const MEASURED: [usize; 4] = [1024, 2048, 4096, 8192];
    let mut last: Option<(usize, f64, f64)> = None;

    for n in MEASURED {
        let results = make_results(n);

        let t0 = Instant::now();
        let batch = consume_like_today(&results);
        let batch_ms = t0.elapsed().as_secs_f64() * 1000.0;

        // 增量路径是**微秒级**的，单次测量完全被调度噪声淹没（实测同一台机器
        // 两次跑出 0.064 ms 与 0.375 ms，差 5.8×——那不是回归，是噪声）。
        // 所以取 9 次里的**最小值**：最小值最接近「这条路径本身有多快」，
        // 也是基准界的常规做法（噪声只会让单次变慢，不会让它变快）。
        let mut incremental = ScanStats::for_total(results.len());
        let mut incremental_ms = f64::INFINITY;
        for _ in 0..9 {
            let t = Instant::now();
            incremental = consume_incremental(&results);
            incremental_ms = incremental_ms.min(t.elapsed().as_secs_f64() * 1000.0);
        }

        assert_eq!(
            batch, incremental,
            "两条路径的统计必须逐字段相同（否则基准本身没意义）"
        );

        let speedup = batch_ms / incremental_ms.max(0.000_001);
        println!("{n:>8}  {batch_ms:>11.2} ms  {incremental_ms:>11.3} ms  {speedup:>9.1}×");

        // **机器无关的结构性断言**：批量是 O(n²)、增量是 O(1)，两者之比必须随 n
        // 显著拉开。这条不依赖机器快慢，所以可以进 CI；`--check` 的基线比对依赖
        // 机器速度，只作本地参考（见 `REGRESSION_TOLERANCE` 的说明）。
        // 阈值取 100×：实测 4096 是 9201×、8192 是 35013×，余量足够大到
        // 「换台慢机器也不会红」，而又足以在「增量路径被改回全量重算」时立刻报警。
        if n >= 4096 {
            assert!(
                batch_ms > incremental_ms * 100.0,
                "{n} 目标下批量重算只比增量慢 {speedup:.1}×，\
                 远低于结构性预期（100×）——增量路径可能被改回了全量重算"
            );
        }

        if n == 1024 {
            report.record("stats.1024.incremental_ms", incremental_ms);
        }
        last = Some((n, batch_ms, incremental_ms));
    }

    if let Some((n, batch_ms, incremental_ms)) = last {
        report.record("stats.8192.batch_ms", batch_ms);
        report.record("stats.8192.incremental_ms", incremental_ms);

        // 二次外推：批量是 O(n²)，n 翻 8 倍 → 时间 64 倍
        let scale = (65_536.0 / n as f64).powi(2);
        let extrapolated = batch_ms * scale;
        println!(
            "{:>8}  {:>11.0} ms  {:>11.3} ms  {:>9.0}×   (外推：批量按 O(n²) 放大 {scale:.0}×，非实测)",
            65_536, extrapolated, incremental_ms, extrapolated / incremental_ms.max(0.000_001)
        );
        report.record("stats.65536.incremental_ms", incremental_ms);
        report.record("stats.65536.batch_extrapolated_ms", extrapolated);
    }
}

/// 2. 调度器：10000 目标 / 32 并发 / 1s 超时（M19 回归基线）。
///
/// **分两次量**，因为 M17 的默认策略把全局限速压在 500 pps：
/// 用默认策略量到的是「限速的时长」，不是调度器的能力。两者都有用，
/// 所以都记下来，并且在输出里说清哪个是哪个。
fn bench_scheduler(report: &mut Report) {
    const TARGETS: usize = 10_000;
    const CONCURRENCY: usize = 32;
    const TIMEOUT_MS: u64 = 1000;

    println!("\n=== 2. 调度器：{TARGETS} 目标 / {CONCURRENCY} 并发 / {TIMEOUT_MS} ms 超时 ===");

    let config = ProbeConfig {
        prefix: None,
        range: cping_core::TargetRange::Standard,
        timeout_ms: TIMEOUT_MS,
        concurrency: CONCURRENCY,
        thresholds: Thresholds::default(),
        mode: cping_core::ProbeMode::Icmp,
    };

    let targets: Vec<IpAddr> = (0..TARGETS)
        .map(|i| {
            IpAddr::V4(Ipv4Addr::new(
                10,
                ((i / 65_536) % 256) as u8,
                ((i / 256) % 256) as u8,
                (i % 256) as u8,
            ))
        })
        .collect();

    // ---- 2a. 不限速（策略放宽到硬天花板）：调度器本身能跑多快 ----
    let fast = ProbeScheduler::new(MockBackend::new(MockPolicy::Realistic {
        online_percent: 30,
        base_rtt_ms: 1,
        span_ms: 40,
    }));

    let free_policy = cping_core::ScanPolicy {
        max_pps: cping_core::policy::MAX_PPS_LIMIT,
        ..cping_core::ScanPolicy::default()
    };
    let mut fast = fast;
    fast.set_policy(free_policy).expect("策略校验失败");
    let run = run_scan(&fast, &config, &targets, None);

    println!("  [2a] 不限速（策略放宽到 {MAX_PPS_LIMIT} pps）");
    println!("       墙钟时间         {:.1} ms", run.elapsed_s * 1000.0);
    println!(
        "       吞吐             {:.0} 目标/秒",
        TARGETS as f64 / run.elapsed_s
    );
    println!("       结果事件         {} 条", run.results);
    println!(
        "       进度事件         {} 条（合并比 {:.1}×）",
        run.progress,
        TARGETS as f64 / run.progress.max(1) as f64
    );
    println!(
        "       Started/终态      {} / {}",
        run.started, run.terminal
    );
    println!("       堆峰值           {}", human(run.peak_bytes));
    assert_eq!(run.results, TARGETS, "结果事件不得丢");
    assert_eq!(run.started, 1, "Started 只应有一条");
    assert_eq!(run.terminal, 1, "终态只应有一条");

    report.record("scheduler.10000.free.elapsed_ms", run.elapsed_s * 1000.0);
    report.record(
        "scheduler.10000.free.per_sec",
        TARGETS as f64 / run.elapsed_s.max(0.000_001),
    );
    report.record("scheduler.10000.free.result_events", run.results as f64);
    report.record("scheduler.10000.free.progress_events", run.progress as f64);
    report.record("scheduler.10000.free.peak_bytes", run.peak_bytes as f64);
    report.record(
        "scheduler.10000.free.retained_bytes",
        run.retained_bytes as f64,
    );

    // ---- 2b. 默认策略（500 pps）：量的是限速是否被遵守 ----
    let throttled = ProbeScheduler::new(MockBackend::new(MockPolicy::Realistic {
        online_percent: 30,
        base_rtt_ms: 1,
        span_ms: 40,
    }));
    let run = run_scan(&throttled, &config, &targets, None);
    let expected_s = TARGETS as f64 / cping_core::policy::DEFAULT_MAX_PPS as f64;
    println!(
        "  [2b] 默认策略（限速 {DEFAULT_MAX_PPS} pps，理论 {:.1} s）",
        expected_s
    );
    println!("       墙钟时间         {:.1} ms", run.elapsed_s * 1000.0);
    println!(
        "       实测 / 理论       {:.2}×",
        run.elapsed_s / expected_s
    );
    println!("       结果事件         {} 条", run.results);
    assert_eq!(run.results, TARGETS);
    println!("       （这 19 秒是**限速**的时长，不是调度器的能力）");

    report.record(
        "scheduler.10000.throttled.elapsed_ms",
        run.elapsed_s * 1000.0,
    );

    // ---- 2c. 限速期间取消：取消延迟 ----
    let cancel_targets: Vec<IpAddr> = mock_targets(65_536);
    let canceller = ProbeScheduler::new(MockBackend::new(MockPolicy::Realistic {
        online_percent: 30,
        base_rtt_ms: 1,
        span_ms: 40,
    }));
    let cancel_run = run_scan(&canceller, &config, &cancel_targets, Some(300));
    println!("  [2c] 65536 目标 + 默认限速，300 ms 后请求取消");
    println!(
        "       取消延迟         {:.1} ms",
        cancel_run.cancel_latency_ms
    );
    println!("       取消前完成       {} 条", cancel_run.results);
    println!(
        "       终态             {}",
        if cancel_run.cancelled {
            "Cancelled"
        } else {
            "Finished（异常！应被取消）"
        }
    );
    assert!(cancel_run.cancelled, "长任务期间取消必须生效");
    assert!(
        cancel_run.cancel_latency_ms < 500.0,
        "取消延迟必须远小于一轮探测（实测 {:.1} ms）——\
         限速与背压都不能变成取消不掉的等待",
        cancel_run.cancel_latency_ms
    );
    let cancelled = cancel_run.results;
    report.record("scheduler.cancel.latency_ms", cancel_run.cancel_latency_ms);
    report.record("scheduler.cancel.completed_before", cancelled as f64);
}

/// 造 n 个目标地址。
fn mock_targets(n: usize) -> Vec<IpAddr> {
    (0..n)
        .map(|i| {
            IpAddr::V4(Ipv4Addr::new(
                10,
                ((i / 65_536) % 256) as u8,
                ((i / 256) % 256) as u8,
                (i % 256) as u8,
            ))
        })
        .collect()
}

/// 一次扫描的观测结果。
struct ScanRun {
    elapsed_s: f64,
    cancel_latency_ms: f64,
    results: usize,
    progress: usize,
    started: usize,
    terminal: usize,
    cancelled: bool,
    peak_bytes: usize,
    retained_bytes: usize,
}

/// 跑一次扫描并消费掉全部事件。
///
/// `cancel_after_ms` 为 `Some` 时另起一个线程在指定延迟后请求取消，
/// 并记录「请求取消 → 收到终态」的延迟。
fn run_scan(
    scheduler: &ProbeScheduler,
    config: &ProbeConfig,
    targets: &[IpAddr],
    cancel_after_ms: Option<u64>,
) -> ScanRun {
    // 本阶段的内存读数：清零后只统计这一段
    CUR.store(0, Ordering::SeqCst);
    PEAK.store(0, Ordering::SeqCst);
    TRACK.store(true, Ordering::SeqCst);

    let t0 = Instant::now();
    let handle = scheduler
        .start(config, targets.to_vec())
        .expect("启动调度失败");

    let cancel_at = cancel_after_ms.map(|ms| {
        let token = handle.cancel_token();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(ms));
            let at = Instant::now();
            token.cancel();
            at
        })
    });

    let mut results = 0usize;
    let mut progress = 0usize;
    let mut started = 0usize;
    let mut terminal = 0usize;
    let mut cancelled = false;
    let mut cancel_latency_ms = 0.0;

    while let Some(event) = handle.recv() {
        match event {
            cping_core::ProbeEvent::Started { .. } => started += 1,
            cping_core::ProbeEvent::Result { .. } => results += 1,
            cping_core::ProbeEvent::Progress { .. } => progress += 1,
            cping_core::ProbeEvent::Finished { .. } => {
                terminal += 1;
                break;
            }
            cping_core::ProbeEvent::Cancelled { .. } => {
                terminal += 1;
                cancelled = true;
                break;
            }
        }
    }
    let elapsed = t0.elapsed();

    // 内存读数要在 `handle`（结果表还在它手里）存活时取
    let peak_bytes = PEAK.load(Ordering::SeqCst);
    let retained_bytes = CUR.load(Ordering::SeqCst).max(0) as usize;
    drop(handle);

    if let Some(t) = cancel_at {
        let requested_at = t.join().expect("取消线程 panic");
        cancel_latency_ms = requested_at.elapsed().as_secs_f64() * 1000.0;
    }

    TRACK.store(false, Ordering::SeqCst);

    ScanRun {
        elapsed_s: elapsed.as_secs_f64(),
        cancel_latency_ms,
        results,
        progress,
        started,
        terminal,
        cancelled,
        peak_bytes,
        retained_bytes,
    }
}

/// 3. 内存：结果集规模估算。
fn bench_memory(report: &mut Report) {
    println!("\n=== 3. 内存：单条结果与整表规模 ===");
    let one = std::mem::size_of::<ProbeResult>();
    let opt = std::mem::size_of::<Option<ProbeResult>>();
    let event = std::mem::size_of::<cping_core::ProbeEvent>();

    println!("   sizeof(ProbeResult)          {one} B");
    println!("   sizeof(Option<ProbeResult>)  {opt} B");
    println!("   sizeof(ProbeEvent)           {event} B");
    println!("   整表（65536，无上限）         {}", human(opt * 65_536));

    report.record("memory.size_of_probe_result", one as f64);
    report.record("memory.size_of_probe_event", event as f64);
    report.record("memory.full_table_65536_bytes", (opt * 65_536) as f64);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let check = args.iter().any(|a| a == "--check");
    let write_baseline_flag = args.iter().any(|a| a == "--write-baseline");

    println!("CPing M19 基准（自研计时，不引 criterion）");
    println!(
        "profile: {}",
        if cfg!(debug_assertions) {
            "debug（数字仅供参考，请用 cargo bench）"
        } else {
            "bench/release"
        }
    );

    let mut report = Report::new();
    bench_stats(&mut report);
    bench_scheduler(&mut report);
    bench_memory(&mut report);
    if write_baseline_flag {
        write_baseline(&report);
    } else if check && !check_against_baseline(&report) {
        std::process::exit(1);
    }

    println!("\n完成。");
}
