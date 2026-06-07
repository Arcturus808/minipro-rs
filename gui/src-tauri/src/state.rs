use std::sync::Mutex;

use minipro_core::{
    device::ProgrammerInfo, DatabasePaths, MiniproHandle,
};

/// Shared application state managed by Tauri.
pub struct AppState {
    /// The active USB programmer handle, if any.
    pub handle: Mutex<Option<MiniproHandle>>,
    /// Resolved database paths for infoic.xml / logicic.xml / algorithms.xml.
    pub db_paths: Mutex<Option<DatabasePaths>>,
    /// Cached programmer info for the UI status bar.
    pub programmer_info: Mutex<Option<ProgrammerInfo>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            handle: Mutex::new(None),
            db_paths: Mutex::new(None),
            programmer_info: Mutex::new(None),
        }
    }
}
