use std::path::Path;
use std::sync::Arc;

use minipro_core::{
    database::{find_device, find_device_any, DatabasePaths},
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
pub struct ProgrammerDetailsDto {
    model: String,
    status: String,
    firmware: String,
    firmware_raw: u32,
    device_code: String,
    serial_number: String,
    hardware_version: String,
    hardware_version_raw: u8,
}

#[derive(Serialize)]
pub struct OvercurrentDto {
    ovc_flag: u8,
    address: u32,
    safe: bool,
}

#[derive(Serialize)]
pub struct CalibrationDto {
    bytes: Vec<u8>,
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

#[derive(Deserialize, Clone)]
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

fn get_db_paths(state: &Arc<AppState>) -> Result<DatabasePaths, String> {
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
pub async fn get_programmer_info(state: State<'_, Arc<AppState>>) -> Result<ProgrammerInfoDto, String> {
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

    let (info, handle) = tokio::task::spawn_blocking(move || {
        let handle = MiniproHandle::open().map_err(|e| e.to_string())?;
        let info = handle.info.clone();
        Ok::<(minipro_core::device::ProgrammerInfo, MiniproHandle), String>((info, handle))
    }).await.map_err(|e| format!("Task panicked: {}", e))??;

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
pub async fn search_devices(query: String, state: State<'_, Arc<AppState>>) -> Result<Vec<String>, String> {
    let filter = query.trim().to_ascii_lowercase();
    if filter.is_empty() {
        return Ok(vec![]);
    }
    // Use pre-loaded device names (loaded once at startup) for instant search
    state.search_device_names(&filter)
}

/// Get detailed info for a single device (no programmer required).
#[tauri::command]
pub async fn get_device_info(name: String, state: State<'_, Arc<AppState>>) -> Result<DeviceInfoDto, String> {
    let db = get_db_paths(&state)?;
    let name_clone = name.clone();

    tokio::task::spawn_blocking(move || {
        let dev = find_device_any(&db, &name_clone).map_err(|e| e.to_string())?;
        Ok::<DeviceInfoDto, String>(device_to_dto(&dev))
    })
    .await
    .map_err(|e| format!("Task panicked: {}", e))?
}

/// Select a device, resolving it for the connected programmer model if available.
#[tauri::command]
pub async fn select_device(name: String, state: State<'_, Arc<AppState>>) -> Result<DeviceInfoDto, String> {
    let db = get_db_paths(&state)?;

    let model = {
        let guard = state.programmer_info.lock().map_err(|e| e.to_string())?;
        guard.as_ref().map(|info| info.model)
    };

    let name_clone = name.clone();
    let (dto, device) = tokio::task::spawn_blocking(move || {
        let dev = if let Some(m) = model {
            find_device(&db, &name_clone, m)
                .or_else(|_| find_device_any(&db, &name_clone))
                .map_err(|e| e.to_string())?
        } else {
            find_device_any(&db, &name_clone).map_err(|e| e.to_string())?
        };
        Ok::<(DeviceInfoDto, Device), String>((device_to_dto(&dev), dev))
    })
    .await
    .map_err(|e| format!("Task panicked: {}", e))??;

    state.set_device(Some(std::sync::Arc::new(device)))?;

    Ok(dto)
}

/// Deselect the current device.
#[tauri::command]
pub async fn deselect_device(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.set_device(None)
}

// ── Chip operations ───────────────────────────────────────────────────────

/// Read chip memory to a file.
#[tauri::command]
pub async fn do_read(
    path: String,
    options: OperationOptions,
    window: Window,
    state: State<'_, Arc<AppState>>,
) -> Result<OpStatsDto, String> {
    let state_clone = (*state).clone();
    if !state_clone.try_acquire() {
        return Err("Another operation is already running".into());
    }

    let state_task = state_clone.clone();
    let window_clone = window.clone();
    let path_clone = path.clone();
    let options_clone = options.clone();

    let result = tokio::task::spawn_blocking(move || {
        let mut handle = state_task.take_handle()?;
        let device = state_task.get_device()?;
        let page = parse_page(&options_clone.page)?;
        let op_name = "read".to_string();

        handle.begin_transaction(device).map_err(|e| e.to_string())?;

        let stats = read_chip(
            &mut handle,
            Path::new(&path_clone),
            page,
            &options_clone.format,
            Some(&mut |done, total| {
                let _ = window_clone.emit(
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
        let _ = state_task.store_handle(handle);
        Ok::<OpStats, String>(stats)
    })
    .await;

    state_clone.release();

    match result {
        Ok(Ok(stats)) => Ok(stats.into()),
        Ok(Err(e)) => Err(e),
        Err(e) => Err(format!("Task panicked: {}", e)),
    }
}

/// Write file to chip memory.
#[tauri::command]
pub async fn do_write(
    path: String,
    options: OperationOptions,
    window: Window,
    state: State<'_, Arc<AppState>>,
) -> Result<OpStatsDto, String> {
    let state_clone = (*state).clone();
    if !state_clone.try_acquire() {
        return Err("Another operation is already running".into());
    }

    let state_task = state_clone.clone();
    let window_clone = window.clone();
    let path_clone = path.clone();
    let options_clone = options.clone();

    let result = tokio::task::spawn_blocking(move || {
        let mut handle = state_task.take_handle()?;
        let device = state_task.get_device()?;
        let page = parse_page(&options_clone.page)?;
        let size_mismatch = parse_size_mismatch(&options_clone.size_mismatch)?;
        let op_name = "write".to_string();

        if !options_clone.skip_erase {
            erase_chip(&mut handle).map_err(|e| e.to_string())?;
            let device_arc = std::sync::Arc::new(handle.device().map_err(|e| e.to_string())?.clone());
            handle.end_transaction().map_err(|e| e.to_string())?;
            handle.begin_transaction(device_arc).map_err(|e| e.to_string())?;
        } else {
            handle.begin_transaction(device).map_err(|e| e.to_string())?;
        }

        let stats = write_chip(
            &mut handle,
            Path::new(&path_clone),
            page,
            &options_clone.format,
            size_mismatch,
            Some(&mut |done, total| {
                let _ = window_clone.emit(
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

        if !options_clone.skip_verify {
            let verify_window = window_clone.clone();
            let _ = verify_chip(
                &mut handle,
                Path::new(&path_clone),
                page,
                &options_clone.format,
                Some(&mut |done, total| {
                    let _ = verify_window.emit(
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
        let _ = state_task.store_handle(handle);
        Ok::<OpStats, String>(stats)
    })
    .await;

    state_clone.release();

    match result {
        Ok(Ok(stats)) => Ok(stats.into()),
        Ok(Err(e)) => Err(e),
        Err(e) => Err(format!("Task panicked: {}", e)),
    }
}

/// Verify chip memory against a file.
#[tauri::command]
pub async fn do_verify(
    path: String,
    options: OperationOptions,
    window: Window,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let state_clone = (*state).clone();
    if !state_clone.try_acquire() {
        return Err("Another operation is already running".into());
    }

    let state_task = state_clone.clone();
    let window_clone = window.clone();
    let path_clone = path.clone();
    let options_clone = options.clone();

    let result = tokio::task::spawn_blocking(move || {
        let mut handle = state_task.take_handle()?;
        let device = state_task.get_device()?;
        let page = parse_page(&options_clone.page)?;

        handle.begin_transaction(device).map_err(|e| e.to_string())?;

        verify_chip(
            &mut handle,
            Path::new(&path_clone),
            page,
            &options_clone.format,
            Some(&mut |done, total| {
                let _ = window_clone.emit(
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
        let _ = state_task.store_handle(handle);
        Ok::<(), String>(())
    })
    .await;

    state_clone.release();

    match result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(e),
        Err(e) => Err(format!("Task panicked: {}", e)),
    }
}

/// Erase the chip.
#[tauri::command]
pub async fn do_erase(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let state_clone = (*state).clone();
    if !state_clone.try_acquire() {
        return Err("Another operation is already running".into());
    }

    let state_task = state_clone.clone();
    let result = tokio::task::spawn_blocking(move || {
        let mut handle = state_task.take_handle()?;
        let device = state_task.get_device()?;
        handle.begin_transaction(device).map_err(|e| e.to_string())?;
        erase_chip(&mut handle).map_err(|e| e.to_string())?;
        let _ = handle.end_transaction();
        let _ = state_task.store_handle(handle);
        Ok::<(), String>(())
    })
    .await;

    state_clone.release();

    match result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(e),
        Err(e) => Err(format!("Task panicked: {}", e)),
    }
}

/// Blank-check the chip.
#[tauri::command]
pub async fn do_blank_check(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let state_clone = (*state).clone();
    if !state_clone.try_acquire() {
        return Err("Another operation is already running".into());
    }

    let state_task = state_clone.clone();
    let result = tokio::task::spawn_blocking(move || {
        let mut handle = state_task.take_handle()?;
        let device = state_task.get_device()?;
        handle.begin_transaction(device).map_err(|e| e.to_string())?;
        blank_check(&mut handle).map_err(|e| e.to_string())?;
        let _ = handle.end_transaction();
        let _ = state_task.store_handle(handle);
        Ok::<(), String>(())
    })
    .await;

    state_clone.release();

    match result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(e),
        Err(e) => Err(format!("Task panicked: {}", e)),
    }
}

/// Read the chip ID.
#[tauri::command]
pub async fn do_chip_id(state: State<'_, Arc<AppState>>) -> Result<String, String> {
    let state_clone = (*state).clone();
    if !state_clone.try_acquire() {
        return Err("Another operation is already running".into());
    }

    let state_task = state_clone.clone();
    let result = tokio::task::spawn_blocking(move || {
        let mut handle = state_task.take_handle()?;
        let device = state_task.get_device()?;
        handle.begin_transaction(device).map_err(|e| e.to_string())?;
        let (_id_type, chip_id) = handle.protocol.get_chip_id(&handle.usb).map_err(|e| e.to_string())?;
        let _ = handle.end_transaction();
        let _ = state_task.store_handle(handle);
        Ok::<String, String>(format!("{:#010x}", chip_id))
    })
    .await;

    state_clone.release();

    match result {
        Ok(Ok(id)) => Ok(id),
        Ok(Err(e)) => Err(e),
        Err(e) => Err(format!("Task panicked: {}", e)),
    }
}

/// Check whether the chip database files can be located.
#[tauri::command]
pub async fn check_database(state: State<'_, Arc<AppState>>) -> Result<bool, String> {
    match get_db_paths(&state) {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),
    }
}

/// Return expanded programmer details (no USB reconnection required).
#[tauri::command]
pub async fn get_programmer_details(state: State<'_, Arc<AppState>>) -> Result<ProgrammerDetailsDto, String> {
    let guard = state.programmer_info.lock().map_err(|e| e.to_string())?;
    let info = guard.as_ref().ok_or("No programmer connected")?;

    Ok(ProgrammerDetailsDto {
        model: format!("{:?}", info.model),
        status: format!("{:?}", info.status),
        firmware: info.firmware_str.clone(),
        firmware_raw: info.firmware,
        device_code: info.device_code.clone(),
        serial_number: info.serial_number.clone(),
        hardware_version: format!("{:02x}", info.hardware_version),
        hardware_version_raw: info.hardware_version,
    })
}

/// Check the programmer's over-current protection status.
#[tauri::command]
pub async fn check_overcurrent(state: State<'_, Arc<AppState>>) -> Result<OvercurrentDto, String> {
    let state_clone = (*state).clone();
    if !state_clone.try_acquire() {
        return Err("Another operation is already running".into());
    }

    let state_task = state_clone.clone();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::task::spawn_blocking(move || {
            let handle = state_task.take_handle()?;
            let result = handle.protocol.get_ovc_status(&handle.usb).map_err(|e| e.to_string());
            let _ = state_task.store_handle(handle);
            result
        }),
    )
    .await;

    state_clone.release();

    match result {
        Ok(Ok(Ok((wstatus, ovc)))) => Ok(OvercurrentDto {
            ovc_flag: ovc,
            address: wstatus.address,
            safe: ovc == 0,
        }),
        Ok(Ok(Err(e))) => Err(e),
        Ok(Err(e)) => Err(format!("Task panicked: {}", e)),
        Err(_) => Err("Operation timed out".into()),
    }
}

/// Read the programmer's internal RC calibration bytes.
#[tauri::command]
pub async fn read_calibration(state: State<'_, Arc<AppState>>) -> Result<CalibrationDto, String> {
    let state_clone = (*state).clone();
    if !state_clone.try_acquire() {
        return Err("Another operation is already running".into());
    }

    let state_task = state_clone.clone();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::task::spawn_blocking(move || {
            let handle = state_task.take_handle()?;
            let result = handle.protocol.read_calibration(&handle.usb, 4).map_err(|e| e.to_string());
            let _ = state_task.store_handle(handle);
            result
        }),
    )
    .await;

    state_clone.release();

    match result {
        Ok(Ok(Ok(bytes))) => Ok(CalibrationDto { bytes }),
        Ok(Ok(Err(e))) => Err(e),
        Ok(Err(e)) => Err(format!("Task panicked: {}", e)),
        Err(_) => Err("Operation timed out".into()),
    }
}

/// Read a file on disk and return as base64 for efficient IPC transfer.
#[tauri::command]
pub async fn read_file_bytes(path: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        std::fs::read(&path)
            .map(|bytes| base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes))
            .map_err(|e| format!("Cannot read file: {}", e))
    })
    .await
    .map_err(|e| format!("Task panicked: {}", e))?
}

// ── Internal helpers ──────────────────────────────────────────────────────

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
