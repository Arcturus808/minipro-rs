use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
// force rebuild
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
            // Initialize shared application state and pre-load device names
            let state = std::sync::Arc::new(state::AppState::default());
            {
                let mut guard = state.db_paths.lock().unwrap();
                // First try standard search paths (CWD, exe dir, MINIPRO_HOME, %PROGRAMDATA%)
                let mut db_paths = minipro_core::database::DatabasePaths::resolve(None, None, None).ok();

                // If not found, try Tauri bundled resources (for installed builds)
                if db_paths.is_none() {
                    if let (Ok(infoic_res), Ok(logicic_res)) = (
                        app.path().resolve("infoic.xml", tauri::path::BaseDirectory::Resource),
                        app.path().resolve("logicic.xml", tauri::path::BaseDirectory::Resource)
                    ) {
                        if infoic_res.exists() && logicic_res.exists() {
                            db_paths = Some(minipro_core::database::DatabasePaths {
                                infoic: infoic_res,
                                logicic: logicic_res,
                                algorithms: None,
                            });
                        }
                    }
                }

                *guard = db_paths;
            }
            if let Err(e) = state.load_device_names() {
                eprintln!("Warning: failed to load device names: {}", e);
            }
            app.manage(state);

            // Dynamically size the main window based on the primary monitor.
            // Uses 90% of screen width and 85% of height, clamped to reasonable bounds.
            if let Ok(Some(monitor)) = app.primary_monitor() {
                let scale = monitor.scale_factor();
                let screen_w = (monitor.size().width as f64 / scale) as u32;
                let screen_h = (monitor.size().height as f64 / scale) as u32;

                let win_w = ((screen_w as f64 * 0.90) as u32).clamp(1280, 1600);
                let win_h = ((screen_h as f64 * 0.85) as u32).clamp(768, 1000);

                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize {
                        width: win_w as f64,
                        height: win_h as f64,
                    }));
                    let _ = window.center();
                }
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_programmer_info,
            commands::force_reconnect,
            commands::get_programmer_details,
            commands::search_devices,
            commands::get_device_info,
            commands::select_device,
            commands::deselect_device,
            commands::do_read,
            commands::read_chip_to_bytes,
            commands::save_bytes_to_file,
            commands::open_folder,
            commands::do_write,
            commands::do_verify,
            commands::do_erase,
            commands::do_blank_check,
            commands::do_chip_id,
            commands::do_logic_test,
            commands::read_file_bytes,
            commands::check_database,
            commands::check_overcurrent,
            commands::read_calibration,
            commands::run_hardware_check,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

mod commands;
mod state;
