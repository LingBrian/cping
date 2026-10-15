//! # cping-core
//!
//! CPing 跨平台重写的**共享核心**。GUI（Tauri 2 + Vue 3）与 TUI（ratatui）
//! 都只能通过这里获取：地址解析、目标生成、结果模型、分类、统计与导出。
//!
//! 设计原则：
//! - 网络 I/O 与 UI **完全解耦**；本 crate 目前不含任何真实探测实现（M1b 通过
//!   `ProbeBackend` 抽象接入，M5 再补平台后端）。
//! - 分类与统计**只在这里算一次**，界面不得自行重算。
//! - 权限不足、超时、不可达、内部错误必须互相区分（见 [`error::ProbeError`]）。

pub mod classify;
pub mod config;
pub mod diff;
pub mod enrich;
pub mod error;
pub mod export;
pub mod fingerprint;
pub mod history;
pub mod hostname;
pub mod html;
pub mod json;
pub mod local_ip;
pub mod monitor;
pub mod native;
pub mod neighbor;
pub mod oui;
pub mod probe;
pub mod registry;
pub mod result;
pub mod scheduler;
pub mod snapshot;
pub mod spec;
pub mod target;

/// 可选异步调度（M12）。默认**不编译**，因此默认构建仍无第三方运行时依赖。
///
/// 开启方式：`cargo build -p cping-core --features async`。
/// 语义与同步版 [`scheduler::ProbeScheduler`] 一致，只是把「一个目标一个
/// std 线程」换成「固定并发度的异步任务」，用于数千目标的场景。
#[cfg(feature = "async")]
pub mod asynchronous;

pub use classify::{classify_rtt, classify_status, LatencyClass, ResultClass, Thresholds};
pub use config::{
    default_monitor_interval_ms, AppConfig, ColorConfig, ProbeConfig, ViewMode,
    DEFAULT_CONCURRENCY, DEFAULT_TIMEOUT_MS, MAX_CONCURRENCY, MAX_TIMEOUT_MS,
};
pub use diff::{diff_lines, diff_reports, DiffEntry, DiffKind, ScanDiff};
pub use enrich::{
    apply_neighbors, enrich_with_mac_and_vendor, fill_vendors_from_mac, lookup_hostname,
    EnrichReport,
};
pub use error::{CoreError, CoreResult, ProbeError};
pub use export::{
    csv_escape, export, export_csv, export_to_path, export_txt, ExportFormat, ExportMeta,
    ScanReport, CSV_BOM, CSV_HEADER, REPORT_FORMAT_VERSION,
};
pub use fingerprint::{
    fingerprint_host, fingerprint_hosts, grab_banner, infer_device, port_service_name, probe_port,
    sanitize_banner, scan_ports, vendor_is_network_equipment, Confidence, DeviceGuess,
    HostFingerprint, PortFinding, PortScanConfig, PortScanReport, BANNER_MAX_BYTES,
    DEFAULT_FINGERPRINT_PORTS, MAX_PORTS_PER_HOST,
};
pub use history::{HistoryBuffer, HostSample, RoundRecord, Uptime, DEFAULT_ROUND_CAPACITY};
pub use hostname::{
    normalize_ptr_name, reverse_lookup, reverse_lookup_default, DEFAULT_TIMEOUT as HOSTNAME_TIMEOUT,
};
pub use html::render_report as render_html_report;
pub use json::{escape as json_escape, is_balanced as json_is_balanced, Json};
pub use local_ip::{LocalAddress, LocalAddressProvider};
pub use monitor::{
    diff_scans, highlight_map, Change, ChangeKind, ChangeLog, HighlightState, MonitorConfig,
    MonitorState, DEFAULT_CHANGE_CAPACITY as MONITOR_CHANGE_CAPACITY,
    DEFAULT_INTERVAL_MS as MONITOR_DEFAULT_INTERVAL_MS, MAX_INTERVAL_MS as MONITOR_MAX_INTERVAL_MS,
    MIN_INTERVAL_MS as MONITOR_MIN_INTERVAL_MS,
};
#[cfg(windows)]
pub use native::ensure_winsock;
#[cfg(any(target_os = "linux", target_os = "android"))]
pub use native::PosixIcmpBackend;
#[cfg(windows)]
pub use native::WindowsIcmpBackend;
pub use native::{
    backend_for_mode, build_echo_request, icmp_checksum, native_icmp_hint, parse_echo_reply,
    parse_unreachable, platform_default_mode, platform_icmp_backend, verify_checksum,
    TcpConnectBackend, DEFAULT_PAYLOAD_LEN, ICMP_HEADER_LEN,
};
pub use neighbor::{mac_for, neighbor_table, parse_proc_net_arp, NeighborEntry};
pub use oui::{entry_counts, mac_to_string, parse_mac, vendor_of, DATA_SOURCE};
pub use probe::{
    MockBackend, MockPolicy, MockResponse, ProbeBackend, ProbeOutcome, RecordingBackend,
};
pub use registry::{
    backend_for, backend_for_run, resolve_for_run, BackendCaps, BackendId, BackendRegistry,
    MOCK_BASE_RTT_MS, MOCK_ONLINE_PERCENT, MOCK_SPAN_MS,
};
pub use result::{ProbeEvent, ProbeResult, ProbeStatus, ScanStats, TaskId};
pub use scheduler::{
    next_task_id, status_from_error, CancelToken, ProbeScheduler, RunOutcome, SchedulerHandle,
};
pub use snapshot::{
    load_from_path, save_to_path, Snapshot, FORMAT_VERSION as SNAPSHOT_VERSION,
    SIGNATURE as SNAPSHOT_SIGNATURE,
};
pub use spec::{
    addr_sort_key, parse_addr, parse_ipv4, parse_ipv6, parse_spec, spec_from_prefix, ProbeMode,
    Segment, TargetSpec, MATRIX_VIEW_MAX,
};
pub use target::{
    build_targets, cell_index, cell_of_addr, cell_of_host, cell_of_ip, cell_position, matrix_slots,
    parse_prefix, AsTargetAddr, Ipv4Prefix, TargetRange, MATRIX_CELLS, MATRIX_COLS, MATRIX_ROWS,
    MAX_TARGETS,
};

/// 版本号，供 UI 展示与调试。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
