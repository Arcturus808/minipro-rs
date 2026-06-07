use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Debug)
                        .build(),
                )?;
            }
            // Initialize shared application state
            app.manage(state::AppState::default());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_programmer_info,
            commands::search_devices,
            commands::get_device_info,
            commands::select_device,
            commands::deselect_device,
            commands::do_read,
            commands::do_write,
            commands::do_verify,
            commands::do_erase,
            commands::do_blank_check,
            commands::do_chip_id,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

mod commands;
mod state;
