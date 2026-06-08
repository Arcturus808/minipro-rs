use std::sync::{atomic::AtomicBool, atomic::Ordering, Arc, Mutex};

use minipro_core::{
    database::{list_devices, DatabasePaths},
    device::{Device, ProgrammerInfo},
    MiniproHandle,
};

/// Shared application state managed by Tauri.
pub struct AppState {
    /// The active USB programmer handle, if any.
    pub handle: Mutex<Option<MiniproHandle>>,
    /// Resolved database paths for infoic.xml / logicic.xml / algorithms.xml.
    pub db_paths: Mutex<Option<DatabasePaths>>,
    /// Cached programmer info for the UI status bar.
    pub programmer_info: Mutex<Option<ProgrammerInfo>>,
    /// Currently selected device (set when user picks one from the list).
    pub selected_device: Mutex<Option<Arc<Device>>>,
    /// Guards against concurrent operations.
    pub is_running: AtomicBool,
    /// Pre-loaded list of all device names (loaded once at startup).
    pub all_device_names: Mutex<Vec<String>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            handle: Mutex::new(None),
            db_paths: Mutex::new(None),
            programmer_info: Mutex::new(None),
            selected_device: Mutex::new(None),
            is_running: AtomicBool::new(false),
            all_device_names: Mutex::new(Vec::new()),
        }
    }
}

impl AppState {
    /// Attempt to acquire the running lock. Returns true if acquired.
    pub fn try_acquire(&self) -> bool {
        !self.is_running.swap(true, Ordering::SeqCst)
    }

    /// Release the running lock.
    pub fn release(&self) {
        self.is_running.store(false, Ordering::SeqCst);
    }

    /// Take the programmer handle out of state.
    pub fn take_handle(&self) -> Result<MiniproHandle, String> {
        let mut guard = self.handle.lock().map_err(|e| e.to_string())?;
        guard.take().ok_or_else(|| "No programmer connected".into())
    }

    /// Store the programmer handle back into state.
    pub fn store_handle(&self, handle: MiniproHandle) -> Result<(), String> {
        let mut guard = self.handle.lock().map_err(|e| e.to_string())?;
        *guard = Some(handle);
        Ok(())
    }

    /// Get a clone of the selected device.
    pub fn get_device(&self) -> Result<Arc<Device>, String> {
        let guard = self.selected_device.lock().map_err(|e| e.to_string())?;
        guard.clone().ok_or_else(|| "No device selected".into())
    }

    /// Set the selected device.
    pub fn set_device(&self, device: Option<Arc<Device>>) -> Result<(), String> {
        let mut guard = self.selected_device.lock().map_err(|e| e.to_string())?;
        *guard = device;
        Ok(())
    }

    /// Load all device names from the database (called once at startup).
    pub fn load_device_names(&self) -> Result<(), String> {
        let guard = self.db_paths.lock().map_err(|e| e.to_string())?;
        let db = guard.as_ref().ok_or("Database not loaded")?;
        let names = list_devices(db, None).map_err(|e| e.to_string())?;
        drop(guard);
        let mut guard = self.all_device_names.lock().map_err(|e| e.to_string())?;
        *guard = names;
        Ok(())
    }

    /// Search the pre-loaded device names by substring.
    pub fn search_device_names(&self, query: &str) -> Result<Vec<String>, String> {
        let guard = self.all_device_names.lock().map_err(|e| e.to_string())?;
        let filter = query.to_ascii_lowercase();
        let mut results: Vec<String> = guard
            .iter()
            .filter(|name| name.to_ascii_lowercase().contains(&filter))
            .cloned()
            .collect();
        results.truncate(200);
        Ok(results)
    }
}
