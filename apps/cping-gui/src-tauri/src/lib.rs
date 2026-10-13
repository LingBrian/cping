//! CPing GUI 的 Tauri 容器。
//!
//! 这一层只做三件事：把前端请求转成对 `cping-core` 的调用、把核心事件推给前端、
//! 处理平台相关的文件保存路径。**不在这里重新实现任何探测/分类/统计逻辑。**

mod commands;

use commands::AppState;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::parse_prefix_cmd,
            commands::start_scan,
            commands::cancel_scan,
            commands::snapshot,
            commands::export_results,
            commands::export_to_app_dir,
            commands::load_config,
            commands::save_config,
            commands::get_local_addresses,
        ])
        .run(tauri::generate_context!())
        .expect("启动 CPing 失败");
}
