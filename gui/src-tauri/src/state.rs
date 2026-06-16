use std::sync::{atomic::AtomicBool, atomic::Ordering, Arc, Mutex};

use minipro_core::{
    database::{list_devices, DatabasePaths, DeviceListItem},
    device::{Device, ProgrammerInfo},
    MiniproHandle,
};

/// Shared application state managed by Tauri.
pub struct AppState {
    /// The active USB programmer handle, if any.
    pub handle: Mutex<Option<MiniproHandle>>,
    /// Resolved database paths for infoic.xml / logicic.xml / algorithm.xml.
    pub db_paths: Mutex<Option<DatabasePaths>>,
    /// Cached programmer info for the UI status bar.
    pub programmer_info: Mutex<Option<ProgrammerInfo>>,
    /// Currently selected device (set when user picks one from the list).
    pub selected_device: Mutex<Option<Arc<Device>>>,
    /// Guards against concurrent operations.
    pub is_running: AtomicBool,
    /// Pre-loaded list of all device names + manufacturers (loaded once at startup).
    pub all_device_names: Mutex<Vec<DeviceListItem>>,
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

    /// Clear cached programmer state (handle + info).
    pub fn clear_programmer(&self) {
        if let Ok(mut guard) = self.handle.lock() {
            *guard = None;
        }
        if let Ok(mut guard) = self.programmer_info.lock() {
            *guard = None;
        }
    }

    /// Take the programmer handle out of state.
    /// Also clears cached programmer info if no handle is present.
    pub fn take_handle(&self) -> Result<MiniproHandle, String> {
        let mut guard = self.handle.lock().map_err(|e| e.to_string())?;
        match guard.take() {
            Some(h) => Ok(h),
            None => {
                // Handle is gone — the programmer was likely unplugged.
                // Clear cached info so the UI badge updates on next check.
                drop(guard);
                self.clear_programmer();
                Err("No programmer connected".into())
            }
        }
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
        let items = list_devices(db, None).map_err(|e| e.to_string())?;
        drop(guard);
        let mut guard = self.all_device_names.lock().map_err(|e| e.to_string())?;
        *guard = items;
        Ok(())
    }

    /// Search the pre-loaded device names by substring.
    pub fn search_device_names(&self, query: &str) -> Result<Vec<DeviceListItem>, String> {
        let guard = self.all_device_names.lock().map_err(|e| e.to_string())?;
        let filter = query.to_ascii_lowercase();
        let mut results: Vec<DeviceListItem> = guard
            .iter()
            .filter(|item| item.name.to_ascii_lowercase().contains(&filter))
            .cloned()
            .collect();
        results.truncate(200);
        Ok(results)
    }
}
