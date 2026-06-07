use std::sync::{atomic::AtomicBool, atomic::Ordering, Arc, Mutex};

use minipro_core::{
    device::{Device, ProgrammerInfo},
    DatabasePaths, MiniproHandle,
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
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            handle: Mutex::new(None),
            db_paths: Mutex::new(None),
            programmer_info: Mutex::new(None),
            selected_device: Mutex::new(None),
            is_running: AtomicBool::new(false),
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
}
