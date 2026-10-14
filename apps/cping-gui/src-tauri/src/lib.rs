//! CPing GUI 的 Tauri 容器。
//!
//! 这一层只做三件事：把前端请求转成对 `cping-core` 的调用、把核心事件推给前端、
//! 处理平台相关的文件保存路径。**不在这里重新实现任何探测/分类/统计逻辑。**

mod commands;

use commands::AppState;
// `app_handle()` 来自 `Manager` trait，不导入会看不到它
use tauri::Manager;

// 桌面端由 `main.rs` 直接调用 `run()`；
// Android/iOS 下必须由该宏生成 JNI 入口符号（`Java_..._run`），
// 否则 Tauri CLI 会在校验 .so 时报「does not include required runtime symbols」。
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(AppState::new())
        // 配置里的尺寸是「理想值」：分辨率不够的机器上直接照搬会把窗口顶出屏幕，
        // 反而逼出滚动条。开窗前先按当前显示器收敛一次。
        .setup(|app| {
            // 窗口尺寸收敛是**桌面专属**逻辑：Android/iOS 的窗口由系统全屏管理，
            // 而且 `WebviewWindow::center()` 在移动端根本不存在（编译期就失败）。
            #[cfg(desktop)]
            if let Some(window) = app.get_webview_window("main") {
                fit_window_to_monitor(&window);
            }
            #[cfg(not(desktop))]
            let _ = app;
            Ok(())
        })
        // 窗口失焦/关闭时顺手落盘一次结果快照。
        // 这不是「优雅退出」的保证，只是多一层保险：真正的落盘时机在每次扫描结束时（见 commands）。
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::Focused(false) | tauri::WindowEvent::CloseRequested { .. } => {
                commands::autosave_on_background(window.app_handle());
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::parse_prefix_cmd,
            commands::parse_spec_cmd,
            commands::default_probe_mode,
            commands::matrix_view_max,
            commands::backend_caps,
            commands::backend_summary,
            commands::async_scheduler_available,
            commands::start_scan,
            commands::cancel_scan,
            commands::snapshot,
            commands::recent_changes,
            commands::export_results,
            commands::export_diff,
            commands::export_to_app_dir,
            commands::load_config,
            commands::save_config,
            commands::get_local_addresses,
            commands::enrich_results,
            commands::lookup_hostname_for,
            commands::save_snapshot,
            commands::save_snapshot_to_app_dir,
            commands::pending_snapshot,
            commands::dismiss_snapshot,
            commands::restore_snapshot,
        ])
        .run(tauri::generate_context!())
        .expect("启动 CPing 失败");
}

/// 期望尺寸（逻辑像素）。与 `tauri.conf.json` 里的 `width`/`height` 保持一致。
#[cfg(desktop)]
const PREFERRED_SIZE: (f64, f64) = (1240.0, 840.0);

/// 把开窗尺寸收敛到当前显示器的**可用区域**内（已扣除任务栏/停靠栏）。
///
/// 只在「屏幕装不下」时才改，且只缩小不放大：显示器够大时窗口尺寸完全由
/// `tauri.conf.json` 决定，不会因为这段逻辑出现启动瞬间的跳变。
///
/// 桌面专属：移动端没有「显示器可用区域」这回事，窗口也由系统管理。
#[cfg(desktop)]
fn fit_window_to_monitor(window: &tauri::WebviewWindow) {
    let (mut width, mut height) = PREFERRED_SIZE;

    if let Ok(Some(monitor)) = window.current_monitor() {
        let scale = monitor.scale_factor();
        let area = monitor.work_area().size;
        // 物理像素 → 逻辑像素；再留出边距，避免贴边或被 DPI 舍入顶出屏幕
        let avail_w = (area.width as f64 / scale - 48.0).max(320.0);
        let avail_h = (area.height as f64 / scale - 72.0).max(240.0);
        width = width.min(avail_w);
        height = height.min(avail_h);
    }

    let needs_resize = match window.inner_size() {
        Ok(size) => {
            let scale = window.scale_factor().unwrap_or(1.0);
            let cur_w = size.width as f64 / scale;
            let cur_h = size.height as f64 / scale;
            // `set_size` 设的就是内尺寸，两边口径一致；留 1px 容差吸收 DPI 舍入
            cur_w > width + 1.0 || cur_h > height + 1.0
        }
        Err(_) => false,
    };

    if needs_resize {
        let _ = window.set_size(tauri::LogicalSize::new(width, height));
    }
    let _ = window.center();
}
