use std::path::Path;

use minipro_core::{
    database::{find_device, find_device_any, list_devices, DatabasePaths},
    device::{ChipType, Device, PackageDetails, Voltages},
    operations::{blank_check, erase_chip, read_chip, verify_chip, write_chip, OpStats, SizeMismatch},
    MiniproHandle,
};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, State, Window};

use crate::state::AppState;

// ── Data transfer objects ───────────────────────────────────────────────────

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

#[derive(Serialize)]
pub struct OpStatsDto {
    bytes: usize,
    crc32: u32,
}

impl From<OpStats> for OpStatsDto {
    fn from(s: OpStats) -> Self {
        Self {
            bytes: s.bytes,
            crc32: s.crc32,
        }
    }
}

#[derive(Serialize, Clone)]
pub struct ProgressPayload {
    done: usize,
    total: usize,
    operation: String,
}

#[derive(Deserialize)]
pub struct OperationOptions {
    #[serde(default)]
    pub skip_erase: bool,
    #[serde(default)]
    pub skip_verify: bool,
    #[serde(default = "default_page")]
    pub page: String,
    #[serde(default = "default_format")]
    pub format: String,
    #[serde(default = "default_size_mismatch")]
    pub size_mismatch: String,
}

fn default_page() -> String { "code".into() }
fn default_format() -> String { "auto".into() }
fn default_size_mismatch() -> String { "error".into() }

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

// ── Helper: parse page string to protocol page type ─────────────────────────

fn parse_page(s: &str) -> Result<u8, String> {
    match s.to_ascii_lowercase().as_str() {
        "0" | "code" => Ok(0x00),
        "1" | "data" => Ok(0x01),
        "2" | "user" => Ok(0x02),
        other => Err(format!("unsupported page type '{}'", other)),
    }
}

fn parse_size_mismatch(s: &str) -> Result<SizeMismatch, String> {
    match s.to_ascii_lowercase().as_str() {
        "error" => Ok(SizeMismatch::Error),
        "warn" => Ok(SizeMismatch::Warn),
        "ignore" => Ok(SizeMismatch::Ignore),
        other => Err(format!("unknown size mismatch mode '{}'", other)),
    }
}

// ── Tauri commands ─────────────────────────────────────────────────────────

/// Open the programmer and return its info.
#[tauri::command]
pub fn get_programmer_info(state: State<AppState>) -> Result<ProgrammerInfoDto, String> {
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
    let dev = find_device_any(&db, &name).map_err(|e| e.to_string())?;
    Ok(device_to_dto(&dev))
}

/// Select a device, resolving it for the connected programmer model if available.
#[tauri::command]
pub fn select_device(name: String, state: State<AppState>) -> Result<DeviceInfoDto, String> {
    let db = get_db_paths(&state)?;

    // If a programmer is already connected, use its model for precise lookup.
    let model = {
        let guard = state.programmer_info.lock().map_err(|e| e.to_string())?;
        guard.as_ref().map(|info| info.model)
    };

    let dev = if let Some(m) = model {
        find_device(&db, &name, m)
            .or_else(|_| find_device_any(&db, &name))
            .map_err(|e| e.to_string())?
    } else {
        find_device_any(&db, &name).map_err(|e| e.to_string())?
    };

    state.set_device(Some(std::sync::Arc::new(dev.clone())))?;
    Ok(device_to_dto(&dev))
}

/// Deselect the current device.
#[tauri::command]
pub fn deselect_device(state: State<AppState>) -> Result<(), String> {
    state.set_device(None)
}

// ── Chip operations ───────────────────────────────────────────────────────

/// Read chip memory to a file.
#[tauri::command]
pub fn do_read(
    path: String,
    options: OperationOptions,
    window: Window,
    state: State<AppState>,
) -> Result<OpStatsDto, String> {
    if !state.try_acquire() {
        return Err("Another operation is already running".into());
    }

    let result = run_operation(&state, |handle, device| {
        let page = parse_page(&options.page)?;
        let op_name = "read".to_string();

        handle.begin_transaction(device).map_err(|e| e.to_string())?;

        let stats = read_chip(
            handle,
            Path::new(&path),
            page,
            &options.format,
            Some(&mut |done, total| {
                let _ = window.emit(
                    "progress",
                    ProgressPayload {
                        done,
                        total,
                        operation: op_name.clone(),
                    },
                );
            }),
        )
        .map_err(|e| e.to_string())?;

        let _ = handle.end_transaction();
        Ok(stats.into())
    });

    state.release();
    result
}

/// Write file to chip memory.
#[tauri::command]
pub fn do_write(
    path: String,
    options: OperationOptions,
    window: Window,
    state: State<AppState>,
) -> Result<OpStatsDto, String> {
    if !state.try_acquire() {
        return Err("Another operation is already running".into());
    }

    let result = run_operation(&state, |handle, device| {
        let page = parse_page(&options.page)?;
        let size_mismatch = parse_size_mismatch(&options.size_mismatch)?;
        let op_name = "write".to_string();

        // Auto-erase before write unless suppressed
        if !options.skip_erase {
            erase_chip(handle).map_err(|e| e.to_string())?;
            // Firmware requires transaction reset after erase
            let device_arc = std::sync::Arc::new(handle.device().map_err(|e| e.to_string())?.clone());
            handle.end_transaction().map_err(|e| e.to_string())?;
            handle.begin_transaction(device_arc).map_err(|e| e.to_string())?;
        } else {
            handle.begin_transaction(device).map_err(|e| e.to_string())?;
        }

        let stats = write_chip(
            handle,
            Path::new(&path),
            page,
            &options.format,
            size_mismatch,
            Some(&mut |done, total| {
                let _ = window.emit(
                    "progress",
                    ProgressPayload {
                        done,
                        total,
                        operation: op_name.clone(),
                    },
                );
            }),
        )
        .map_err(|e| e.to_string())?;

        // Verify after write unless suppressed
        if !options.skip_verify {
            let _verify_stats = verify_chip(
                handle,
                Path::new(&path),
                page,
                &options.format,
                Some(&mut |done, total| {
                    let _ = window.emit(
                        "progress",
                        ProgressPayload {
                            done,
                            total,
                            operation: "verify".to_string(),
                        },
                    );
                }),
            )
            .map_err(|e| e.to_string())?;
        }

        let _ = handle.end_transaction();
        Ok(stats.into())
    });

    state.release();
    result
}

/// Verify chip memory against a file.
#[tauri::command]
pub fn do_verify(
    path: String,
    options: OperationOptions,
    window: Window,
    state: State<AppState>,
) -> Result<(), String> {
    if !state.try_acquire() {
        return Err("Another operation is already running".into());
    }

    let result = run_operation(&state, |handle, device| {
        let page = parse_page(&options.page)?;

        handle.begin_transaction(device).map_err(|e| e.to_string())?;

        verify_chip(
            handle,
            Path::new(&path),
            page,
            &options.format,
            Some(&mut |done, total| {
                let _ = window.emit(
                    "progress",
                    ProgressPayload {
                        done,
                        total,
                        operation: "verify".to_string(),
                    },
                );
            }),
        )
        .map_err(|e| e.to_string())?;

        let _ = handle.end_transaction();
        Ok(())
    });

    state.release();
    result
}

/// Erase the chip.
#[tauri::command]
pub fn do_erase(state: State<AppState>) -> Result<(), String> {
    if !state.try_acquire() {
        return Err("Another operation is already running".into());
    }

    let result = run_operation(&state, |handle, device| {
        handle.begin_transaction(device).map_err(|e| e.to_string())?;
        erase_chip(handle).map_err(|e| e.to_string())?;
        let _ = handle.end_transaction();
        Ok(())
    });

    state.release();
    result
}

/// Blank-check the chip.
#[tauri::command]
pub fn do_blank_check(state: State<AppState>) -> Result<(), String> {
    if !state.try_acquire() {
        return Err("Another operation is already running".into());
    }

    let result = run_operation(&state, |handle, device| {
        handle.begin_transaction(device).map_err(|e| e.to_string())?;
        blank_check(handle).map_err(|e| e.to_string())?;
        let _ = handle.end_transaction();
        Ok(())
    });

    state.release();
    result
}

/// Read the chip ID.
#[tauri::command]
pub fn do_chip_id(state: State<AppState>) -> Result<String, String> {
    if !state.try_acquire() {
        return Err("Another operation is already running".into());
    }

    let result = run_operation(&state, |handle, device| {
        handle.begin_transaction(device).map_err(|e| e.to_string())?;
        let (_id_type, chip_id) = handle.protocol.get_chip_id(&handle.usb).map_err(|e| e.to_string())?;
        let _ = handle.end_transaction();
        Ok(format!("{:#010x}", chip_id))
    });

    state.release();
    result
}

// ── Internal helpers ──────────────────────────────────────────────────────

/// Generic operation wrapper: acquires handle and device, runs the closure,
/// and ensures the handle is always returned to state.
fn run_operation<T>(
    state: &State<AppState>,
    f: impl FnOnce(&mut MiniproHandle, std::sync::Arc<Device>) -> Result<T, String>,
) -> Result<T, String> {
    let mut handle = state.take_handle()?;
    let device = state.get_device()?;

    let result = f(&mut handle, device);

    // Always put the handle back, even on error
    let _ = state.store_handle(handle);
    result
}

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
