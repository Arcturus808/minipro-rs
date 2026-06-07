use minipro_core::{
    database::{find_device_any, list_devices, DatabasePaths},
    device::{ChipType, Device, PackageDetails, Voltages},
    MiniproHandle,
};
use serde::Serialize;
use tauri::State;

use crate::state::AppState;

// ── Data transfer objects for the frontend ──────────────────────────────────

#[derive(Serialize)]
pub struct ProgrammerInfoDto {
    model: String,
    firmware: String,
    serial_number: String,
    hardware_version: String,
}

#[derive(Serialize)]
pub struct DeviceInfoDto {
    name: String,
    chip_type: String,
    pin_count: u8,
    package_type: String,
    voltages: VoltagesDto,
}

#[derive(Serialize)]
pub struct VoltagesDto {
    vpp: u8,
    vdd: u8,
    vcc: u8,
}

impl From<&Voltages> for VoltagesDto {
    fn from(v: &Voltages) -> Self {
        Self {
            vpp: v.vpp,
            vdd: v.vdd,
            vcc: v.vcc,
        }
    }
}

// ── Helper: resolve or reuse database paths ─────────────────────────────────

fn get_db_paths(state: &State<AppState>) -> Result<DatabasePaths, String> {
    {
        let guard = state.db_paths.lock().map_err(|e| e.to_string())?;
        if let Some(ref paths) = *guard {
            return Ok(DatabasePaths {
                infoic: paths.infoic.clone(),
                logicic: paths.logicic.clone(),
                algorithms: paths.algorithms.clone(),
            });
        }
    }

    let paths = DatabasePaths::resolve(None, None, None)
        .map_err(|e| format!("Failed to locate chip database: {}", e))?;

    {
        let mut guard = state.db_paths.lock().map_err(|e| e.to_string())?;
        *guard = Some(DatabasePaths {
            infoic: paths.infoic.clone(),
            logicic: paths.logicic.clone(),
            algorithms: paths.algorithms.clone(),
        });
    }

    Ok(paths)
}

// ── Tauri commands ──────────────────────────────────────────────────────────

/// Open the programmer and return its info.
#[tauri::command]
pub fn get_programmer_info(state: State<AppState>) -> Result<ProgrammerInfoDto, String> {
    // Try to reuse cached info first
    {
        let guard = state.programmer_info.lock().map_err(|e| e.to_string())?;
        if let Some(ref info) = *guard {
            return Ok(ProgrammerInfoDto {
                model: info.model.to_string(),
                firmware: info.firmware_str.clone(),
                serial_number: info.serial_number.clone(),
                hardware_version: format!("{:02x}", info.hardware_version),
            });
        }
    }

    let handle = MiniproHandle::open().map_err(|e| e.to_string())?;
    let info = handle.info.clone();

    {
        let mut guard = state.programmer_info.lock().map_err(|e| e.to_string())?;
        *guard = Some(info.clone());
    }
    {
        let mut guard = state.handle.lock().map_err(|e| e.to_string())?;
        *guard = Some(handle);
    }

    Ok(ProgrammerInfoDto {
        model: info.model.to_string(),
        firmware: info.firmware_str,
        serial_number: info.serial_number,
        hardware_version: format!("{:02x}", info.hardware_version),
    })
}

/// Search devices by optional query string.
#[tauri::command]
pub fn search_devices(query: String, state: State<AppState>) -> Result<Vec<String>, String> {
    let db = get_db_paths(&state)?;
    let filter = if query.trim().is_empty() {
        None
    } else {
        Some(query.trim())
    };

    let results = list_devices(&db, filter).map_err(|e| e.to_string())?;
    Ok(results)
}

/// Get detailed info for a single device (no programmer required).
#[tauri::command]
pub fn get_device_info(name: String, state: State<AppState>) -> Result<DeviceInfoDto, String> {
    let db = get_db_paths(&state)?;

    // Try without model constraint first (find_device_any searches both infoic and logicic)
    let dev = find_device_any(&db, &name).map_err(|e| e.to_string())?;

    Ok(device_to_dto(&dev))
}

// ── Internal helpers ────────────────────────────────────────────────────────

fn device_to_dto(dev: &Device) -> DeviceInfoDto {
    let chip_type_str = ChipType::try_from(dev.chip_type)
        .map(|t| format!("{:?}", t))
        .unwrap_or_else(|_| format!("Unknown({})", dev.chip_type));

    DeviceInfoDto {
        name: dev.name.clone(),
        chip_type: chip_type_str,
        pin_count: dev.package_details.pin_count,
        package_type: package_type_name(&dev.package_details),
        voltages: VoltagesDto::from(&dev.voltages),
    }
}

fn package_type_name(pkg: &PackageDetails) -> String {
    if pkg.plcc {
        format!("PLCC{}", pkg.pin_count)
    } else {
        format!("DIP{}", pkg.pin_count)
    }
}
