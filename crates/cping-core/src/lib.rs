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
pub mod error;
pub mod export;
pub mod local_ip;
pub mod result;
pub mod target;

pub use classify::{classify_rtt, classify_status, LatencyClass, ResultClass, Thresholds};
pub use config::{
    AppConfig, ColorConfig, ProbeConfig, ViewMode, DEFAULT_CONCURRENCY, DEFAULT_TIMEOUT_MS,
    MAX_CONCURRENCY, MAX_TIMEOUT_MS,
};
pub use error::{CoreError, CoreResult, ProbeError};
pub use export::{
    csv_escape, export, export_csv, export_to_path, export_txt, ExportFormat, ExportMeta,
};
pub use local_ip::{LocalAddress, LocalAddressProvider};
pub use result::{ProbeEvent, ProbeResult, ProbeStatus, ScanStats, TaskId};
pub use target::{
    build_targets, cell_index, cell_of_host, cell_of_ip, cell_position, matrix_slots, parse_prefix,
    Ipv4Prefix, TargetRange, MATRIX_CELLS, MATRIX_COLS, MATRIX_ROWS, MAX_TARGETS,
};

/// 版本号，供 UI 展示与调试。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
