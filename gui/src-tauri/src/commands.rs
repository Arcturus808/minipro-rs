use std::path::Path;
use std::sync::Arc;

use minipro_core::{
    batch::{patch_serial, SerialChecksum, SerialConfig, SerialEndian, SerialFormat},
    database::{find_device, find_device_any, DatabasePaths},
    device::{ChipType, Device, PackageDetails, Voltages},
    operations::{blank_check, check_chip_id, erase_chip, firmware_update, hardware_check, logic_ic_test, normalize_chip_id, read_chip, read_file, verify_chip, verify_chip_bytes, write_chip, write_chip_bytes, write_file, OpStats, SizeMismatch},
    MiniproHandle,
};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, State, Window};

use crate::state::AppState;

/// Check if an error indicates the programmer was physically disconnected.
/// If so, clear cached state so the UI badge updates on next check.
fn handle_usb_error(state: &AppState, err: &str) {
    let usb_errors = ["STALL", "NoDevice", "LIBUSB_ERROR_NO_DEVICE",
        "LIBUSB_ERROR_IO", "LIBUSB_ERROR_PIPE", "DeviceNotFound",
        "endpoint", "USB error", "No programmer connected"];
    if usb_errors.iter().any(|&keyword| err.contains(keyword)) {
        state.clear_programmer();
        log::warn!("USB error detected, clearing cached programmer state: {}", err);
    }
}

/// Emit a log message to the frontend terminal.
fn emit_log(window: &Window, level: &str, message: &str) {
    let _ = window.emit("app-log", serde_json::json!({
        "level": level,
        "message": message,
    }));
}

// ── Data transfer objects ───────────────────────────────────────────────────

/// Serial number configuration for batch programming (sent from frontend).
#[derive(Deserialize, Clone, Debug)]
pub struct SerialConfigDto {
    pub start: u64,
    pub address: usize,
    pub width: usize,
    pub format: String,
    pub endian: String,
    pub step: u64,
    pub checksum: String,
}

impl TryFrom<&SerialConfigDto> for SerialConfig {
    type Error = String;
    fn try_from(dto: &SerialConfigDto) -> Result<Self, Self::Error> {
        Ok(SerialConfig {
            start: dto.start,
            address: dto.address,
            width: dto.width,
            format: SerialFormat::parse(&dto.format).map_err(|e| e.to_string())?,
            endian: SerialEndian::parse(&dto.endian).map_err(|e| e.to_string())?,
            step: dto.step,
            checksum: SerialChecksum::parse(&dto.checksum).map_err(|e| e.to_string())?,
        })
    }
}

#[derive(Serialize)]
pub struct ProgrammerInfoDto {
    model: String,
    firmware: String,
    serial_number: String,
    hardware_version: String,
}

#[derive(Serialize)]
pub struct HardwareCheckResultDto {
    supported: bool,
    pass: bool,
    message: String,
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

#[derive(Serialize, Clone)]
pub struct FuseFieldDto {
    name: String,
    display_name: String,
    mask: u16,
    default_value: u16,
}

#[derive(Serialize)]
#[serde(tag = "type")]
pub enum ChipConfigDto {
    Mcu { fuses: Vec<FuseFieldDto>, locks: Vec<FuseFieldDto> },
    Pld {},
}

#[derive(Serialize)]
pub struct DeviceInfoDto {
    name: String,
    manufacturer: String,
    chip_type: String,
    pin_count: u8,
    package_type: String,
    voltages: VoltagesDto,
    code_memory_size: u32,
    data_memory_size: u32,
    can_erase: bool,
    has_chip_id: bool,
    config: Option<ChipConfigDto>,
    /// True for AVR-family devices where fuse bit=0 means programmed.
    invert_fuse_bits: bool,
}

#[derive(Serialize)]
pub struct VoltagesDto {
    vpp: String,
    vdd: String,
    vcc: String,
}

impl From<&Voltages> for VoltagesDto {
    fn from(v: &Voltages) -> Self {
        static VPP_TABLE: &[&str] = &[
            "9.0", "9.5", "10.0", "11.0", "11.5", "12.0", "12.5", "13.0", "13.5", "14.0", "14.5",
            "15.5", "16.0", "16.5", "17.0", "18.0",
        ];
        static VCC_TABLE: &[&str] = &[
            "1.9", "2.7", "3.0", "3.3", "3.6", "3.9", "4.1", "4.5", "4.8", "5.0", "5.3", "5.5", "6.0",
            "6.3", "6.5", "7.0",
        ];
        Self {
            vpp: VPP_TABLE.get(v.vpp as usize).unwrap_or(&"?").to_string(),
            vdd: VCC_TABLE.get(v.vdd as usize).unwrap_or(&"?").to_string(),
            vcc: VCC_TABLE.get(v.vcc as usize).unwrap_or(&"?").to_string(),
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
    #[serde(default)]
    pub skip_blank: bool,
    #[serde(default = "default_true")]
    pub check_device_id: bool,
    #[serde(default)]
    pub vpp: Option<String>,
    #[serde(default)]
    pub vcc: Option<String>,
    #[serde(default)]
    pub vdd: Option<String>,
    #[serde(default = "default_icsp_mode")]
    pub icsp_mode: String,
    #[serde(default = "default_page")]
    pub page: String,
    #[serde(default = "default_format")]
    pub format: String,
    #[serde(default = "default_size_mismatch")]
    pub size_mismatch: String,
}

fn default_icsp_mode() -> String { "zif".into() }
fn default_page() -> String { "code".into() }
fn default_format() -> String { "auto".into() }
fn default_size_mismatch() -> String { "error".into() }
fn default_true() -> bool { true }

/// Apply voltage overrides from GUI options to a device.
fn apply_voltage_overrides(device: &mut Device, options: &OperationOptions) -> Result<(), String> {
    // VPP voltage table (index 0..15 → volts), from tl866iiplus.c
    static VPP_TABLE: &[&str] = &[
        "9.0", "9.5", "10.0", "11.0", "11.5", "12.0", "12.5", "13.0", "13.5", "14.0", "14.5",
        "15.5", "16.0", "16.5", "17.0", "18.0",
    ];
    // VCC / VDD voltage table (index 0..15 → volts), from tl866iiplus.c
    static VCC_TABLE: &[&str] = &[
        "1.9", "2.7", "3.0", "3.3", "3.6", "3.9", "4.1", "4.5", "4.8", "5.0", "5.3", "5.5", "6.0",
        "6.3", "6.5", "7.0",
    ];

    if let Some(ref v) = options.vpp {
        let idx = VPP_TABLE
            .iter()
            .position(|&t| t == v)
            .ok_or_else(|| format!("invalid vpp voltage '{v}'; valid values: {}", VPP_TABLE.join(", ")))?;
        device.voltages.vpp = idx as u8;
    }
    if let Some(ref v) = options.vdd {
        let idx = VCC_TABLE
            .iter()
            .position(|&t| t == v)
            .ok_or_else(|| format!("invalid vdd voltage '{v}'; valid values: {}", VCC_TABLE.join(", ")))?;
        device.voltages.vdd = idx as u8;
    }
    if let Some(ref v) = options.vcc {
        let idx = VCC_TABLE
            .iter()
            .position(|&t| t == v)
            .ok_or_else(|| format!("invalid vcc voltage '{v}'; valid values: {}", VCC_TABLE.join(", ")))?;
        device.voltages.vcc = idx as u8;
    }
    Ok(())
}

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

/// Force-close any existing handle and re-open the programmer.
/// Use this after unplugging/replugging the device.
/// Retries up to 5 times with increasing delays because Windows USB
/// enumeration can be stale immediately after a hot-plug event.
#[tauri::command]
pub async fn force_reconnect(state: State<'_, Arc<AppState>>) -> Result<ProgrammerInfoDto, String> {
    // Explicitly drop any stale handle so the USB device can be re-claimed
    {
        let mut handle_guard = state.handle.lock().map_err(|e| e.to_string())?;
        *handle_guard = None;
    }
    {
        let mut info_guard = state.programmer_info.lock().map_err(|e| e.to_string())?;
        *info_guard = None;
    }

    // Retry with increasing delays — Windows USB enumeration can lag behind
    // the Device Manager display by several seconds after hot-plug.
    let mut last_err = String::new();
    for attempt in 1..=5 {
        tokio::time::sleep(std::time::Duration::from_millis(500 * attempt)).await;

        let result = tokio::task::spawn_blocking(move || {
            let handle = MiniproHandle::open().map_err(|e| e.to_string())?;
            let info = handle.info.clone();
            Ok::<(minipro_core::device::ProgrammerInfo, MiniproHandle), String>((info, handle))
        }).await;

        match result {
            Ok(Ok((info, handle))) => {
                {
                    let mut guard = state.programmer_info.lock().map_err(|e| e.to_string())?;
                    *guard = Some(info.clone());
                }
                {
                    let mut guard = state.handle.lock().map_err(|e| e.to_string())?;
                    *guard = Some(handle);
                }
                return Ok(ProgrammerInfoDto {
                    model: info.model.to_string(),
                    firmware: info.firmware_str,
                    serial_number: info.serial_number,
                    hardware_version: format!("{:02x}", info.hardware_version),
                });
            }
            Ok(Err(e)) => {
                last_err = e;
                eprintln!("force_reconnect attempt {} failed: {}", attempt, last_err);
            }
            Err(e) => {
                last_err = format!("Task panicked: {}", e);
                eprintln!("force_reconnect attempt {} panicked", attempt);
            }
        }
    }

    Err(format!("No programmer detected after 5 reconnect attempts. Last error: {}", last_err))
}

#[derive(Serialize, Clone)]
pub struct DeviceSearchResultDto {
    name: String,
    manufacturer: String,
}

/// Search devices by optional query string.
#[tauri::command]
pub async fn search_devices(query: String, state: State<'_, Arc<AppState>>) -> Result<Vec<DeviceSearchResultDto>, String> {
    let filter = query.trim().to_ascii_lowercase();
    if filter.is_empty() {
        return Ok(vec![]);
    }
    // Use pre-loaded device names (loaded once at startup) for instant search
    let items = state.search_device_names(&filter)?;
    Ok(items.into_iter().map(|item| DeviceSearchResultDto {
        name: item.name,
        manufacturer: item.manufacturer,
    }).collect())
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

        log::info!(
            "do_read: device={} page={:#02x} code_size={} data_size={}",
            device.name,
            page,
            device.code_memory_size,
            device.data_memory_size
        );

        let result = (|| {
            let code_size = handle.protocol.effective_code_size(&device) as usize;
            let size = match page {
                0x00 => code_size,
                0x01 => device.data_memory_size as usize,
                _ => code_size,
            };
            if size == 0 {
                return Err(format!(
                    "Device '{}' has 0 bytes for the selected page (code={}, data={}). Try a different page.",
                    device.name, device.code_memory_size, device.data_memory_size
                ));
            }

            handle.icsp = options_clone.icsp_mode != "zif";
            handle.begin_transaction(device.clone()).map_err(|e| e.to_string())?;

            if options_clone.check_device_id {
                match check_chip_id(&mut handle) {
                    Ok(()) => {
                        emit_log(&window_clone, "info", "Chip ID check passed");
                    }
                    Err(e) => {
                        emit_log(&window_clone, "error", &format!("Chip ID check failed: {}", e));
                        return Err(e.to_string());
                    }
                }
            }

            let stats = read_chip(
                &mut handle,
                Path::new(&path_clone),
                page,
                &options_clone.format,
                false, // chip ID already checked above
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

            Ok::<OpStats, String>(stats)
        })();

        let _ = handle.end_transaction();
        let _ = state_task.store_handle(handle);
        if let Err(ref e) = result {
            handle_usb_error(&state_task, e);
        }
        result
    })
    .await;

    state_clone.release();

    match result {
        Ok(Ok(stats)) => Ok(stats.into()),
        Ok(Err(e)) => Err(e),
        Err(e) => Err(format!("Task panicked: {}", e)),
    }
}

#[derive(Serialize)]
pub struct ChipBytesDto {
    base64: String,
    stats: OpStatsDto,
}

/// Read chip memory to a temporary file, then return the bytes as base64.
/// The caller can display the bytes in a hex viewer without saving to disk.
#[tauri::command]
pub async fn read_chip_to_bytes(
    options: OperationOptions,
    window: Window,
    state: State<'_, Arc<AppState>>,
) -> Result<ChipBytesDto, String> {
    let state_clone = (*state).clone();
    if !state_clone.try_acquire() {
        return Err("Another operation is already running".into());
    }

    let state_task = state_clone.clone();
    let window_clone = window.clone();
    let options_clone = options.clone();

    let result = tokio::task::spawn_blocking(move || {
        let mut handle = state_task.take_handle()?;
        let device = state_task.get_device()?;
        let page = parse_page(&options_clone.page)?;
        let op_name = "read".to_string();

        let result = (|| {
            let code_size = handle.protocol.effective_code_size(&device) as usize;
            let size = match page {
                0x00 => code_size,
                0x01 => device.data_memory_size as usize,
                _ => code_size,
            };
            if size == 0 {
                return Err(format!(
                    "Device '{}' has 0 bytes for the selected page (code={}, data={}). Try a different page.",
                    device.name, device.code_memory_size, device.data_memory_size
                ));
            }

            // Create a temp file for the read operation
            let temp_dir = std::env::temp_dir();
            let temp_path = temp_dir.join(format!("minipro_read_{}.bin", std::process::id()));
            let _temp_path_str = temp_path.to_string_lossy().to_string();

            handle.icsp = options_clone.icsp_mode != "zif";
            handle.begin_transaction(device.clone()).map_err(|e| e.to_string())?;

            let stats = read_chip(
                &mut handle,
                &temp_path,
                page,
                "bin", // always read raw binary for the hex viewer
                options_clone.check_device_id,
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

            // Read the temp file bytes
            let bytes = std::fs::read(&temp_path)
                .map_err(|e| format!("Failed to read temp file: {}", e))?;

            // Clean up temp file
            let _ = std::fs::remove_file(&temp_path);

            let base64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes);
            Ok::<(String, OpStats), String>((base64, stats))
        })();

        let _ = handle.end_transaction();
        let _ = state_task.store_handle(handle);
        if let Err(ref e) = result {
            handle_usb_error(&state_task, e);
        }
        result
    })
    .await;

    state_clone.release();

    match result {
        Ok(Ok((base64, stats))) => Ok(ChipBytesDto {
            base64,
            stats: stats.into(),
        }),
        Ok(Err(e)) => Err(e),
        Err(e) => Err(format!("Task panicked: {}", e)),
    }
}

/// Write raw bytes (base64 encoded) to a file on disk.
#[tauri::command]
pub async fn save_bytes_to_file(path: String, base64Data: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let bytes = base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            &base64Data,
        )
        .map_err(|e| format!("Failed to decode base64: {}", e))?;

        std::fs::write(&path, &bytes)
            .map_err(|e| format!("Failed to write file: {}", e))
    })
    .await
    .map_err(|e| format!("Task panicked: {}", e))?
}

/// Write a buffer to disk in the specified file format.
///
/// `format` is one of `"bin"`, `"ihex"`, `"srec"`, or `"jedec"`.
/// `"auto"` is treated as `"bin"`.  For `"jedec"`, `deviceName` is optional.
#[tauri::command]
pub async fn save_buffer_to_file(
    path: String,
    base64Data: String,
    format: String,
    deviceName: Option<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let bytes = base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            &base64Data,
        )
        .map_err(|e| format!("Failed to decode base64: {}", e))?;

        let path_ref = std::path::Path::new(&path);
        let effective_fmt = if format == "auto" || format == "bin" {
            "bin"
        } else {
            &format
        };

        write_file(path_ref, effective_fmt, &bytes, deviceName.as_deref())
            .map_err(|e| format!("Failed to write file: {}", e))
    })
    .await
    .map_err(|e| format!("Task panicked: {}", e))?
}

/// Open the folder containing the given file path in the system file manager.
#[tauri::command]
pub fn open_folder(path: String) -> Result<(), String> {
    let parent = std::path::Path::new(&path)
        .parent()
        .ok_or("Path has no parent directory")?;

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(parent)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(parent)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(parent)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }
    Ok(())
}

/// Check whether a file exists at the given path.
#[tauri::command]
pub fn file_exists(path: String) -> Result<bool, String> {
    Ok(std::path::Path::new(&path).exists())
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
        let device_arc = state_task.get_device()?;
        let mut device = (*device_arc).clone();
        apply_voltage_overrides(&mut device, &options_clone).map_err(|e| e.to_string())?;
        let device = Arc::new(device);
        let page = parse_page(&options_clone.page)?;
        let size_mismatch = parse_size_mismatch(&options_clone.size_mismatch)?;
        let op_name = "write".to_string();

        let result = (|| {
            handle.icsp = options_clone.icsp_mode != "zif";
            handle.begin_transaction(device.clone()).map_err(|e| e.to_string())?;

            if options_clone.check_device_id {
                match check_chip_id(&mut handle) {
                    Ok(()) => {
                        emit_log(&window_clone, "info", "Chip ID check passed");
                    }
                    Err(e) => {
                        emit_log(&window_clone, "error", &format!("Chip ID check failed: {}", e));
                        return Err(e.to_string());
                    }
                }
            }

            if !options_clone.skip_erase {
                erase_chip(&mut handle, false).map_err(|e| e.to_string())?;
                handle.end_transaction().map_err(|e| e.to_string())?;
                handle.begin_transaction(device).map_err(|e| e.to_string())?;
            }

            let stats = write_chip(
                &mut handle,
                Path::new(&path_clone),
                page,
                &options_clone.format,
                size_mismatch,
                options_clone.skip_blank,
                false, // chip ID already checked above
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
                    false, // chip ID already checked above
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

            Ok::<OpStats, String>(stats)
        })();

        let _ = handle.end_transaction();
        let _ = state_task.store_handle(handle);
        if let Err(ref e) = result {
            handle_usb_error(&state_task, e);
        }
        result
    })
    .await;

    state_clone.release();

    match result {
        Ok(Ok(stats)) => Ok(stats.into()),
        Ok(Err(e)) => Err(e),
        Err(e) => Err(format!("Task panicked: {}", e)),
    }
}

/// Write file to chip memory — single chip within a batch run.
/// Same as `do_write` but emits batch-specific log messages with the chip number.
/// The frontend manages the batch loop and calls this once per chip.
/// If `serialConfig` is provided, the firmware is read into a buffer, patched
/// with the serial number, and written/verified via the bytes-based path.
#[tauri::command]
pub async fn do_batch_write_chip(
    path: String,
    chipNumber: u32,
    options: OperationOptions,
    serialConfig: Option<SerialConfigDto>,
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
    let serial_dto = serialConfig.clone();

    let result = tokio::task::spawn_blocking(move || {
        let mut handle = state_task.take_handle()?;
        let device_arc = state_task.get_device()?;
        let mut device = (*device_arc).clone();
        apply_voltage_overrides(&mut device, &options_clone).map_err(|e| e.to_string())?;
        let device = Arc::new(device);
        let page = parse_page(&options_clone.page)?;
        let size_mismatch = parse_size_mismatch(&options_clone.size_mismatch)?;
        let op_name = format!("write (chip {})", chipNumber);

        emit_log(&window_clone, "info", &format!("── Chip {} ──", chipNumber));

        // Parse serial config if provided
        let serial_cfg = if let Some(ref dto) = serial_dto {
            Some(SerialConfig::try_from(dto)?)
        } else {
            None
        };

        let result = (|| {
            handle.icsp = options_clone.icsp_mode != "zif";
            handle.begin_transaction(device.clone()).map_err(|e| e.to_string())?;

            if options_clone.check_device_id {
                match check_chip_id(&mut handle) {
                    Ok(()) => {
                        emit_log(&window_clone, "info", "Chip ID check passed");
                    }
                    Err(e) => {
                        emit_log(&window_clone, "error", &format!("Chip ID check failed: {}", e));
                        return Err(e.to_string());
                    }
                }
            }

            if !options_clone.skip_erase {
                emit_log(&window_clone, "info", &format!("Chip {}: erasing...", chipNumber));
                erase_chip(&mut handle, false).map_err(|e| e.to_string())?;
                handle.end_transaction().map_err(|e| e.to_string())?;
                handle.begin_transaction(device.clone()).map_err(|e| e.to_string())?;
            }

            // ── If serial injection: read file, patch, write bytes, verify bytes ──
            if let Some(ref sc) = serial_cfg {
                let dev = handle.device().map_err(|e| e.to_string())?;
                let code_size = handle.protocol.effective_code_size(dev) as usize;
                let size = match page {
                    0x00 => code_size,
                    0x01 => dev.data_memory_size as usize,
                    _ => code_size,
                };
                let mut buf = read_file(
                    Path::new(&path_clone),
                    &options_clone.format,
                    size,
                    dev.blank_value as u8,
                )
                .map_err(|e| e.to_string())?;

                let serial_value = sc.value_for_chip(chipNumber as usize);
                patch_serial(&mut buf, sc, chipNumber as usize).map_err(|e| e.to_string())?;
                emit_log(
                    &window_clone,
                    "info",
                    &format!("Chip {}: serial = 0x{:0>width$X}", chipNumber, serial_value, width = sc.width * 2),
                );

                let write_window = window_clone.clone();
                let stats = write_chip_bytes(
                    &mut handle,
                    buf.clone(),
                    page,
                    size_mismatch,
                    options_clone.skip_blank,
                    false,
                    Some(&mut |done, total| {
                        let _ = write_window.emit(
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
                    let _ = verify_chip_bytes(
                        &mut handle,
                        buf,
                        page,
                        false,
                        Some(&mut |done, total| {
                            let _ = verify_window.emit(
                                "progress",
                                ProgressPayload {
                                    done,
                                    total,
                                    operation: format!("verify (chip {})", chipNumber),
                                },
                            );
                        }),
                    )
                    .map_err(|e| e.to_string())?;
                }

                emit_log(&window_clone, "info", &format!("Chip {}: PASS", chipNumber));
                return Ok::<OpStats, String>(stats);
            }

            // ── No serial injection: use file-based write + verify (original path) ──
            let stats = write_chip(
                &mut handle,
                Path::new(&path_clone),
                page,
                &options_clone.format,
                size_mismatch,
                options_clone.skip_blank,
                false,
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
                    false,
                    Some(&mut |done, total| {
                        let _ = verify_window.emit(
                            "progress",
                            ProgressPayload {
                                done,
                                total,
                                operation: format!("verify (chip {})", chipNumber),
                            },
                        );
                    }),
                )
                .map_err(|e| e.to_string())?;
            }

            emit_log(&window_clone, "info", &format!("Chip {}: PASS", chipNumber));
            Ok::<OpStats, String>(stats)
        })();

        let _ = handle.end_transaction();
        let _ = state_task.store_handle(handle);
        if let Err(ref e) = result {
            handle_usb_error(&state_task, e);
        }
        result
    })
    .await;

    state_clone.release();

    match result {
        Ok(Ok(stats)) => Ok(stats.into()),
        Ok(Err(e)) => Err(e),
        Err(e) => Err(format!("Task panicked: {}", e)),
    }
}

/// Write the hex buffer (base64-encoded) to the chip.
#[tauri::command]
pub async fn do_write_bytes(
    base64Data: String,
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
    let options_clone = options.clone();

    let result = tokio::task::spawn_blocking(move || {
        let bytes = base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            &base64Data,
        )
        .map_err(|e| format!("Failed to decode base64: {}", e))?;

        let mut handle = state_task.take_handle()?;
        let device_arc = state_task.get_device()?;
        let mut device = (*device_arc).clone();
        apply_voltage_overrides(&mut device, &options_clone).map_err(|e| e.to_string())?;
        let device = Arc::new(device);
        let page = parse_page(&options_clone.page)?;
        let size_mismatch = parse_size_mismatch(&options_clone.size_mismatch)?;
        let op_name = "write".to_string();

        let result = (|| {
            handle.icsp = options_clone.icsp_mode != "zif";
            handle.begin_transaction(device.clone()).map_err(|e| e.to_string())?;

            if options_clone.check_device_id {
                match check_chip_id(&mut handle) {
                    Ok(()) => {
                        emit_log(&window_clone, "info", "Chip ID check passed");
                    }
                    Err(e) => {
                        emit_log(&window_clone, "error", &format!("Chip ID check failed: {}", e));
                        return Err(e.to_string());
                    }
                }
            }

            if !options_clone.skip_erase {
                erase_chip(&mut handle, false).map_err(|e| e.to_string())?;
                handle.end_transaction().map_err(|e| e.to_string())?;
                handle.begin_transaction(device).map_err(|e| e.to_string())?;
            }

            let verify_bytes = bytes.clone();
            let stats = write_chip_bytes(
                &mut handle,
                bytes,
                page,
                size_mismatch,
                options_clone.skip_blank,
                false, // chip ID already checked above
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
                let _ = verify_chip_bytes(
                    &mut handle,
                    verify_bytes,
                    page,
                    false, // chip ID already checked above
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

            Ok::<OpStats, String>(stats)
        })();

        let _ = handle.end_transaction();
        let _ = state_task.store_handle(handle);
        if let Err(ref e) = result {
            handle_usb_error(&state_task, e);
        }
        result
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

        let result = (|| {
            handle.icsp = options_clone.icsp_mode != "zif";
            handle.begin_transaction(device).map_err(|e| e.to_string())?;

            if options_clone.check_device_id {
                match check_chip_id(&mut handle) {
                    Ok(()) => {
                        emit_log(&window_clone, "info", "Chip ID check passed");
                    }
                    Err(e) => {
                        emit_log(&window_clone, "error", &format!("Chip ID check failed: {}", e));
                        return Err(e.to_string());
                    }
                }
            }

            verify_chip(
                &mut handle,
                Path::new(&path_clone),
                page,
                &options_clone.format,
                false, // chip ID already checked above
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

            Ok::<(), String>(())
        })();

        let _ = handle.end_transaction();
        let _ = state_task.store_handle(handle);
        if let Err(ref e) = result {
            handle_usb_error(&state_task, e);
        }
        result
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
pub async fn do_erase(icspMode: String, checkDeviceId: bool, window: Window, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let state_clone = (*state).clone();
    if !state_clone.try_acquire() {
        return Err("Another operation is already running".into());
    }

    let state_task = state_clone.clone();
    let window_clone = window.clone();
    let result = tokio::task::spawn_blocking(move || {
        let mut handle = state_task.take_handle()?;
        let device = state_task.get_device()?;

        let result = (|| {
            handle.icsp = icspMode != "zif";
            handle.begin_transaction(device).map_err(|e| e.to_string())?;

            if checkDeviceId {
                match check_chip_id(&mut handle) {
                    Ok(()) => {
                        emit_log(&window_clone, "info", "Chip ID check passed");
                    }
                    Err(e) => {
                        emit_log(&window_clone, "error", &format!("Chip ID check failed: {}", e));
                        return Err(e.to_string());
                    }
                }
            }

            erase_chip(&mut handle, false).map_err(|e| e.to_string())?;
            Ok::<(), String>(())
        })();

        let _ = handle.end_transaction();
        let _ = state_task.store_handle(handle);
        if let Err(ref e) = result {
            handle_usb_error(&state_task, e);
        }
        result
    })
    .await;

    state_clone.release();

    match result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(e),
        Err(e) => Err(format!("Task panicked: {}", e)),
    }
}

#[derive(Serialize)]
pub struct BlankCheckResultDto {
    is_blank: bool,
    address: u32,
}

/// Blank-check the chip.
/// Returns Ok(is_blank=true) if blank, Ok(is_blank=false, address) if not blank.
#[tauri::command]
pub async fn do_blank_check(icspMode: String, state: State<'_, Arc<AppState>>) -> Result<BlankCheckResultDto, String> {
    let state_clone = (*state).clone();
    if !state_clone.try_acquire() {
        return Err("Another operation is already running".into());
    }

    let state_task = state_clone.clone();
    let result = tokio::task::spawn_blocking(move || {
        let mut handle = state_task.take_handle()?;
        let device = state_task.get_device()?;

        let result = (|| {
            handle.icsp = icspMode != "zif";
            handle.begin_transaction(device).map_err(|e| e.to_string())?;
            blank_check(&mut handle).map_err(|e| e.to_string())?;
            Ok::<BlankCheckResultDto, String>(BlankCheckResultDto { is_blank: true, address: 0 })
        })();

        let _ = handle.end_transaction();
        let _ = state_task.store_handle(handle);
        result
    })
    .await;

    state_clone.release();

    match result {
        Ok(Ok(dto)) => Ok(dto),
        Ok(Err(e)) => {
            // Parse the NotBlank error to extract the address
            if let Some(addr_str) = e.strip_prefix("Chip is not blank at 0x") {
                if let Ok(addr) = u32::from_str_radix(addr_str.trim(), 16) {
                    return Ok(BlankCheckResultDto { is_blank: false, address: addr });
                }
            }
            Err(e)
        }
        Err(e) => Err(format!("Task panicked: {}", e)),
    }
}

#[derive(Serialize)]
pub struct ChipIdResultDto {
    id: String,
    expected: String,
    is_match: bool,
    is_variant: bool,
    base_name: String,
}

/// Read the chip ID.
#[tauri::command]
pub async fn do_chip_id(icspMode: String, state: State<'_, Arc<AppState>>) -> Result<ChipIdResultDto, String> {
    let state_clone = (*state).clone();
    if !state_clone.try_acquire() {
        return Err("Another operation is already running".into());
    }

    let state_task = state_clone.clone();
    let result = tokio::task::spawn_blocking(move || {
        let mut handle = state_task.take_handle()?;
        let device = state_task.get_device()?;

        let result = (|| {
            handle.icsp = icspMode != "zif";
            handle.begin_transaction(device.clone()).map_err(|e| e.to_string())?;
            let (_id_type, chip_id) = handle.protocol.get_chip_id(&handle.usb).map_err(|e| e.to_string())?;
            // Package variants (e.g. @DIP8) often have copied chip_id values from the base
            // chip that don't match what the firmware returns for that variant's protocol.
            // Treat them as "no expected value" to avoid false mismatch warnings.
            let is_variant = device.name.contains('@');
            let base_name = if let Some(at) = device.name.find('@') {
                device.name[..at].to_string()
            } else {
                device.name.clone()
            };
            let expected = device.chip_id;
            let bytes = if is_variant { 4 } else { device.chip_id_bytes_count.max(1).min(4) };
            let mask = match bytes {
                1 => 0xFFu32,
                2 => 0xFFFF,
                3 => 0xFFFFFF,
                _ => 0xFFFFFFFF,
            };
            let masked_id = chip_id & mask;
            let masked_expected = expected & mask;
            let id_str = format!("0x{:0width$x}", masked_id, width = (bytes * 2) as usize);
            let expected_str = format!("0x{:0width$x}", masked_expected, width = (bytes * 2) as usize);
            // Use normalized comparison to handle byte-position differences across protocols
            let norm_id = normalize_chip_id(chip_id);
            let norm_expected = normalize_chip_id(expected);
            // For variants, treat as a match so we don't show a generic mismatch error,
            // but the frontend will show a contextual message instead.
            let is_match = expected == 0 || norm_id == norm_expected || is_variant;
            Ok::<ChipIdResultDto, String>(ChipIdResultDto { id: id_str, expected: expected_str, is_match, is_variant, base_name })
        })();

        let _ = handle.end_transaction();
        let _ = state_task.store_handle(handle);
        if let Err(ref e) = result {
            handle_usb_error(&state_task, e);
        }
        result
    })
    .await;

    state_clone.release();

    match result {
        Ok(Ok(dto)) => Ok(dto),
        Ok(Err(e)) => Err(e),
        Err(e) => Err(format!("Task panicked: {}", e)),
    }
}

/// Test a logic IC against its built-in test vectors.
/// Returns the test result table as a string.
#[tauri::command]
pub async fn do_logic_test(icspMode: String, state: State<'_, Arc<AppState>>) -> Result<String, String> {
    let state_clone = (*state).clone();
    if !state_clone.try_acquire() {
        return Err("Another operation is already running".into());
    }

    let state_task = state_clone.clone();
    let result = tokio::task::spawn_blocking(move || {
        let mut handle = state_task.take_handle()?;
        let device = state_task.get_device()?;

        let result = (|| {
            handle.icsp = icspMode != "zif";
            handle.begin_transaction(device).map_err(|e| e.to_string())?;
            let mut output = Vec::new();
            let test_result = logic_ic_test(&mut handle, &mut output);
            let mut text = String::from_utf8_lossy(&output).into_owned();
            if let Err(ref e) = test_result {
                if !text.is_empty() && !text.ends_with('\n') {
                    text.push('\n');
                }
                text.push_str(&format!("[ERROR] {}", e));
            }
            Ok::<String, String>(text)
        })();

        let _ = handle.end_transaction();
        let _ = state_task.store_handle(handle);
        if let Err(ref e) = result {
            handle_usb_error(&state_task, e);
        }
        result
    })
    .await;

    state_clone.release();

    match result {
        Ok(Ok(text)) => Ok(text),
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
            let device = state_task.get_device()?;
            let result = handle.protocol.get_ovc_status(&handle.usb, &device).map_err(|e| e.to_string());
            let _ = state_task.store_handle(handle);
            if let Err(ref e) = result {
                handle_usb_error(&state_task, e);
            }
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
            if let Err(ref e) = result {
                handle_usb_error(&state_task, e);
            }
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

#[derive(Serialize, Deserialize, Clone)]
pub struct FuseValueDto {
    name: String,
    value: u8,
}

#[derive(Serialize)]
pub struct ConfigDataDto {
    cfg_fuses: Vec<FuseValueDto>,
    lock_bits: Vec<FuseValueDto>,
    user_fuses: Vec<u8>,
    calibration: Vec<u8>,
}

/// Read all fuse / lock / user / calibration data from the chip.
#[tauri::command]
pub async fn read_fuses(icspMode: String, state: State<'_, Arc<AppState>>) -> Result<ConfigDataDto, String> {
    let state_clone = (*state).clone();
    if !state_clone.try_acquire() {
        return Err("Another operation is already running".into());
    }

    let state_task = state_clone.clone();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::task::spawn_blocking(move || {
            let mut handle = state_task.take_handle()?;

            let device = state_task.get_device()?;
            let result = (|| {
                handle.icsp = icspMode != "zif";
                handle.begin_transaction(device).map_err(|e| e.to_string())?;

                // Read named CFG fuses + LOCK bits
                let named = minipro_core::operations::read_fuses(&mut handle).map_err(|e| e.to_string())?;

                let dev = handle.device().map_err(|e| e.to_string())?;
                let fuse_len = if let Some(minipro_core::device::ChipConfig::Mcu(ref cfg)) = dev.config { cfg.fuses.len() } else { 0 };

                // Read chip calibration bytes (OSCCAL word for PIC devices)
                let calibration = minipro_core::operations::read_chip_calibration(&mut handle)
                    .map_err(|e| e.to_string())?;

                Ok::<ConfigDataDto, String>(ConfigDataDto {
                    cfg_fuses: named.iter().take(fuse_len)
                        .map(|v| FuseValueDto { name: v.name.clone(), value: v.value })
                        .collect(),
                    lock_bits: named.iter().skip(fuse_len)
                        .map(|v| FuseValueDto { name: v.name.clone(), value: v.value })
                        .collect(),
                    user_fuses: vec![],  // TODO: TL866A user fuse read hangs firmware
                    calibration,
                })
            })();

            let _ = handle.end_transaction();
            let _ = state_task.store_handle(handle);
            if let Err(ref e) = result {
                handle_usb_error(&state_task, e);
            }
            result
        }),
    )
    .await;

    state_clone.release();

    match result {
        Ok(Ok(Ok(dto))) => Ok(dto),
        Ok(Ok(Err(e))) => Err(e),
        Ok(Err(e)) => Err(format!("Task panicked: {}", e)),
        Err(_) => Err("Operation timed out".into()),
    }
}

/// Write fuse / lock bytes to the chip.
#[tauri::command]
pub async fn write_fuses(cfgFuses: Vec<FuseValueDto>, lockBits: Vec<FuseValueDto>, icspMode: String, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let state_clone = (*state).clone();
    if !state_clone.try_acquire() {
        return Err("Another operation is already running".into());
    }

    let state_task = state_clone.clone();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::task::spawn_blocking(move || {
            let mut handle = state_task.take_handle()?;

            let device = state_task.get_device()?;
            let result = (|| {
                handle.icsp = icspMode != "zif";
                handle.begin_transaction(device.clone()).map_err(|e| e.to_string())?;

                // Write CFG + LOCK via high-level function
                let mut all: Vec<minipro_core::operations::FuseValue> = cfgFuses.iter()
                    .map(|d| minipro_core::operations::FuseValue { name: d.name.clone(), value: d.value })
                    .collect();
                all.extend(lockBits.iter()
                    .map(|d| minipro_core::operations::FuseValue { name: d.name.clone(), value: d.value }));
                minipro_core::operations::write_fuses(&mut handle, &all).map_err(|e| e.to_string())?;

                Ok::<(), String>(())
            })();

            let _ = handle.end_transaction();
            let _ = state_task.store_handle(handle);
            if let Err(ref e) = result {
                handle_usb_error(&state_task, e);
            }
            result
        }),
    )
    .await;

    state_clone.release();

    match result {
        Ok(Ok(Ok(()))) => Ok(()),
        Ok(Ok(Err(e))) => Err(e),
        Ok(Err(e)) => Err(format!("Task panicked: {}", e)),
        Err(_) => Err("Operation timed out".into()),
    }
}

#[derive(Serialize)]
pub struct LockStatusDto {
    is_protected: bool,
    lock_byte: u8,
}

/// Quick check whether the chip's lock bits indicate read/write protection.
#[tauri::command]
pub async fn check_lock_protection(icspMode: String, state: State<'_, Arc<AppState>>) -> Result<LockStatusDto, String> {
    let state_clone = (*state).clone();
    if !state_clone.try_acquire() {
        return Err("Another operation is already running".into());
    }

    let state_task = state_clone.clone();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::task::spawn_blocking(move || {
            let mut handle = state_task.take_handle()?;

            let device = state_task.get_device()?;
            let result = (|| {
                handle.icsp = icspMode != "zif";
                handle.begin_transaction(device).map_err(|e| e.to_string())?;

                let lock_count = if let Some(minipro_core::device::ChipConfig::Mcu(ref cfg)) = handle.device().map_err(|e| e.to_string())?.config {
                    cfg.locks.len() as u8
                } else { 0 };

                let lock_byte = if lock_count > 0 {
                    handle.protocol.read_fuses(
                        &handle.usb,
                        handle.device().map_err(|e| e.to_string())?,
                        minipro_core::operations::MP_FUSE_LOCK,
                        lock_count as usize,
                        lock_count,
                    ).map(|b| b.first().copied().unwrap_or(0xff)).unwrap_or(0xff)
                } else {
                    0xff
                };

                // AVR: any bit cleared means some lock protection is active
                let is_protected = lock_byte != 0xff;

                Ok::<LockStatusDto, String>(LockStatusDto { is_protected, lock_byte })
            })();

            let _ = handle.end_transaction();
            let _ = state_task.store_handle(handle);
            result
        }),
    )
    .await;

    state_clone.release();

    match result {
        Ok(Ok(Ok(status))) => Ok(status),
        Ok(Ok(Err(e))) => Err(e),
        Ok(Err(e)) => Err(format!("Task panicked: {}", e)),
        Err(_) => Err("Operation timed out".into()),
    }
}

/// Run the programmer's built-in hardware self-test.
#[tauri::command]
pub async fn run_hardware_check(state: State<'_, Arc<AppState>>) -> Result<HardwareCheckResultDto, String> {
    let state_clone = (*state).clone();
    if !state_clone.try_acquire() {
        return Err("Another operation is already running".into());
    }

    let state_task = state_clone.clone();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::task::spawn_blocking(move || {
            let mut handle = state_task.take_handle()?;
            let result = hardware_check(&mut handle).map_err(|e| e.to_string());
            let _ = state_task.store_handle(handle);
            if let Err(ref e) = result {
                handle_usb_error(&state_task, e);
            }
            result
        }),
    )
    .await;

    state_clone.release();

    match result {
        Ok(Ok(Ok(()))) => Ok(HardwareCheckResultDto {
            supported: true,
            pass: true,
            message: "PASS".into(),
        }),
        Ok(Ok(Err(e))) => {
            if e.contains("UnsupportedOperation") || e.contains("not supported") {
                Ok(HardwareCheckResultDto {
                    supported: false,
                    pass: false,
                    message: "Not supported on this programmer model".into(),
                })
            } else {
                Err(e)
            }
        }
        Ok(Err(e)) => Err(format!("Task panicked: {}", e)),
        Err(_) => Err("Operation timed out".into()),
    }
}

/// Update programmer firmware from an update.dat / updateII.dat / updateT76.dat file.
#[tauri::command]
pub async fn do_firmware_update(
    path: String,
    state: State<'_, Arc<AppState>>,
    window: tauri::Window,
) -> Result<(), String> {
    let state_clone = (*state).clone();
    if !state_clone.try_acquire() {
        return Err("Another operation is already running".into());
    }
    let _guard = scopeguard::guard((), |_| state_clone.release());

    let fw_data = tokio::task::spawn_blocking(move || {
        std::fs::read(&path).map_err(|e| format!("cannot read firmware file: {}", e))
    })
    .await
    .map_err(|e| format!("Task panicked: {}", e))??;

    let window_clone = window.clone();
    let updated_info = {
        let mut guard = state.handle.lock().map_err(|e| e.to_string())?;
        let handle = guard.as_mut().ok_or("No programmer connected")?;
        let mut output = Vec::new();
        let result = firmware_update(handle, &fw_data, &mut output, Some(&mut |done, total| {
            let _ = window_clone.emit(
                "progress",
                ProgressPayload {
                    done,
                    total,
                    operation: "firmware_update".to_string(),
                },
            );
        }));
        let text = String::from_utf8_lossy(&output).into_owned();
        if !text.is_empty() {
            // Emit each line as a separate log entry
            for line in text.lines() {
                if !line.is_empty() {
                    let _ = window_clone.emit(
                        "app-log",
                        serde_json::json!({ "level": "info", "message": line }),
                    );
                }
            }
        }
        result.map_err(|e| e.to_string())?;
        handle.info.clone()
    };

    // Programmer reconnects in bootloader then normal mode during update.
    // Refresh our cached info so the UI shows the new firmware version.
    {
        let mut guard = state.programmer_info.lock().map_err(|e| e.to_string())?;
        *guard = Some(updated_info);
    }

    Ok(())
}

/// Trim trailing blank bytes from a buffer.
fn trim_trailing_blanks(mut bytes: Vec<u8>, blank: u8) -> Vec<u8> {
    let last = bytes.iter().rposition(|&b| b != blank);
    if let Some(idx) = last {
        bytes.truncate(idx + 1);
    } else {
        bytes.clear();
    }
    bytes
}

/// Read a file on disk and return as base64 for efficient IPC transfer.
/// Automatically detects and parses Intel HEX / SREC / JEDEC files.
/// Parsed text-format files are trimmed of trailing blank bytes for cleaner display.
#[tauri::command]
pub async fn read_file_bytes(path: String, target_size: Option<u32>, blank_value: Option<u8>) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let p = Path::new(&path);
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
        let is_text_format = matches!(ext.to_lowercase().as_str(), "hex" | "srec" | "mot" | "jed");

        let bytes = if is_text_format {
            let size = target_size.unwrap_or(65536) as usize;
            let blank = blank_value.unwrap_or(0xFF);
            let buf = read_file(p, "auto", size, blank).map_err(|e| format!("Cannot parse file: {}", e))?;
            trim_trailing_blanks(buf, blank)
        } else {
            std::fs::read(p).map_err(|e| format!("Cannot read file: {}", e))?
        };

        Ok(base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes))
    })
    .await
    .map_err(|e| format!("Task panicked: {}", e))?
}

/// Compare a base64-encoded buffer (the hex viewer's current data) against a
/// reference file on disk. Returns a structured `DiffResult` as JSON.
///
/// The reference file is read as raw binary (no format parsing). For text
/// formats (.hex, .srec, .jed), the file is parsed and trimmed of trailing
/// blank bytes, matching `read_file_bytes` behavior.
#[tauri::command]
pub async fn do_smart_diff(
    base64Data: String,
    referencePath: String,
    eraseValue: Option<u8>,
) -> Result<minipro_core::DiffResult, String> {
    tokio::task::spawn_blocking(move || {
        let buf_a = base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            &base64Data,
        )
        .map_err(|e| format!("Invalid base64 data: {}", e))?;

        let p = Path::new(&referencePath);
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
        let is_text_format = matches!(ext.to_lowercase().as_str(), "hex" | "srec" | "mot" | "jed");

        let buf_b = if is_text_format {
            let blank = eraseValue.unwrap_or(0xFF);
            let size = 65536usize;
            let buf = read_file(p, "auto", size, blank)
                .map_err(|e| format!("Cannot parse reference file: {}", e))?;
            trim_trailing_blanks(buf, blank)
        } else {
            std::fs::read(p).map_err(|e| format!("Cannot read reference file: {}", e))?
        };

        let erase = eraseValue.unwrap_or(0xFF);
        Ok(minipro_core::smart_diff(&buf_a, &buf_b, erase))
    })
    .await
    .map_err(|e| format!("Task panicked: {}", e))?
}

/// Return the dynamic window size that would be computed for the primary monitor.
#[tauri::command]
pub async fn get_dynamic_window_size(app: tauri::AppHandle) -> Result<(u32, u32), String> {
    let monitor = app.primary_monitor().map_err(|e| e.to_string())?
        .ok_or("No primary monitor found")?;
    let scale = monitor.scale_factor();
    let screen_w = (monitor.size().width as f64 / scale) as u32;
    let screen_h = (monitor.size().height as f64 / scale) as u32;

    let win_w = ((screen_w as f64 * 0.90) as u32).clamp(1280, 1600);
    let win_h = ((screen_h as f64 * 0.85) as u32).clamp(768, 1000);
    Ok((win_w, win_h))
}

// ── Internal helpers ──────────────────────────────────────────────────────

fn fuse_display_name(name: &str) -> String {
    match name.to_lowercase().as_str() {
        "lfuse" => "Low Fuse".to_string(),
        "hfuse" => "High Fuse".to_string(),
        "efuse" => "Extended Fuse".to_string(),
        "fuse" => "Fuse".to_string(),
        "lock" => "Lock Bits".to_string(),
        other => other.to_string(),
    }
}

fn device_to_dto(dev: &Device) -> DeviceInfoDto {
    let chip_type_str = ChipType::try_from(dev.chip_type)
        .map(|t| format!("{:?}", t))
        .unwrap_or_else(|_| format!("Unknown({})", dev.chip_type));

    let config = dev.config.as_ref().map(|cfg| match cfg {
        minipro_core::device::ChipConfig::Mcu(fuse_cfg) => ChipConfigDto::Mcu {
            fuses: fuse_cfg.fuses.iter().map(|f| FuseFieldDto {
                name: f.name.clone(),
                display_name: fuse_display_name(&f.name),
                mask: f.mask,
                default_value: f.default,
            }).collect(),
            locks: fuse_cfg.locks.iter().map(|f| FuseFieldDto {
                name: f.name.clone(),
                display_name: fuse_display_name(&f.name),
                mask: f.mask,
                default_value: f.default,
            }).collect(),
        },
        minipro_core::device::ChipConfig::Pld(_) => ChipConfigDto::Pld {},
    });

    // Detect AVR-family devices (fuse bit=0 means programmed).
    let name_upper = dev.name.to_uppercase();
    let invert_fuse_bits = name_upper.starts_with("AT")
        && (name_upper.contains("TINY") || name_upper.contains("MEGA") || name_upper.contains("90S") || name_upper.contains("90C") || name_upper.contains("SAMD") || name_upper.contains("XMEGA"));

    DeviceInfoDto {
        name: dev.name.clone(),
        manufacturer: dev.manufacturer.clone(),
        chip_type: chip_type_str,
        pin_count: dev.package_details.pin_count,
        package_type: package_type_name(&dev.package_details),
        voltages: VoltagesDto::from(&dev.voltages),
        code_memory_size: dev.code_memory_size,
        data_memory_size: dev.data_memory_size,
        can_erase: dev.flags.can_erase,
        has_chip_id: dev.flags.has_chip_id,
        config,
        invert_fuse_bits,
    }
}

fn package_type_name(pkg: &PackageDetails) -> String {
    if pkg.plcc {
        format!("PLCC{}", pkg.pin_count)
    } else {
        format!("DIP{}", pkg.pin_count)
    }
}
