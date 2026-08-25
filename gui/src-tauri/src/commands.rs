// Tauri v2 auto-converts top-level invoke keys to camelCase before matching
// to Rust parameter names, so all #[tauri::command] params must use
// camelCase (e.g. icspMode, not icsp_mode). This triggers Rust's
// non_snake_case lint, which we suppress file-wide rather than per-function.
#![allow(non_snake_case)]

use std::path::Path;
use std::sync::Arc;

use minipro_core::{
    batch::{patch_serial, SerialChecksum, SerialConfig, SerialEndian, SerialFormat},
    database::{find_device, find_device_any, get_pin_map, DatabasePaths},
    device::{ChipType, Device, PackageDetails, ProgrammerModel, Voltages},
    operations::{blank_check, check_chip_id, erase_chip, firmware_update, hardware_check, logic_auto_find, logic_ic_test, normalize_chip_id, pin_contact_check, read_chip, read_file, spi_autodetect_and_lookup, verify_chip, verify_chip_bytes, write_chip, write_chip_bytes, write_file, OpStats, SizeMismatch},
    MiniproHandle,
};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, State, Window};

use crate::state::AppState;

/// Check if an error indicates the programmer was physically disconnected
/// or is in a bad state.  If so, clear cached state so the UI badge updates.
fn handle_usb_error(state: &AppState, err: &str) {
    let usb_errors = [
        "STALL",
        "NoDevice",
        "LIBUSB_ERROR_NO_DEVICE",
        "LIBUSB_ERROR_IO",
        "LIBUSB_ERROR_PIPE",
        "DeviceNotFound",
        "endpoint",
        "USB error",
        "No programmer connected",
        "unknown error",   // nusb generic error when device is gone
        "timed out",       // our USB transfer timeout
        "cannot open it",  // open_programmer error when device can't be opened
        "cannot claim",    // interface claim failure
    ];
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

/// Emit pin-test results to the frontend for ZIF diagram highlighting.
fn emit_pin_test_result(window: &Window, dto: &PinTestResultDto) {
    let _ = window.emit("pin-test-result", serde_json::json!({
        "supported": dto.supported,
        "pass": dto.pass,
        "bad_pins": dto.bad_pins,
        "message": dto.message,
    }));
}

/// Run a pin contact check before an operation if enabled.
///
/// Silently returns `Ok(())` when:
/// - `pin_check` is false (user unchecked the box)
/// - Programmer model is not TL866II+/T48
/// - ICSP mode is active (pin test is ZIF-only)
/// - Device has `pin_map == 0` (no contact-test data in database)
///
/// On failure: emits bad-pin data to the frontend for diagram highlighting,
/// emits a log line, and returns `Err` so the calling operation aborts.
fn run_pin_check_if_enabled(
    handle: &mut MiniproHandle,
    enabled: bool,
    icsp_mode: &str,
    window: &Window,
    infoic_path: &std::path::Path,
) -> Result<(), String> {
    if !enabled {
        return Ok(());
    }
    if !matches!(
        handle.info.model,
        ProgrammerModel::Tl866iiPlus | ProgrammerModel::T48
    ) {
        return Ok(());
    }
    if icsp_mode != "zif" {
        return Ok(());
    }
    let device = handle.device().map_err(|e| e.to_string())?.clone();
    if device.pin_map & 0xFF == 0 {
        return Ok(());
    }

    emit_log(window, "info", "Running pin contact check...");
    let result = pin_contact_check(handle, infoic_path).map_err(|e| e.to_string())?;
    let pass = result.bad_pins.is_empty();
    let count = result.bad_pins.len();
    let dto = PinTestResultDto {
        supported: true,
        pass,
        bad_pins: result.bad_pins.clone(),
        message: if pass {
            "All pins OK".into()
        } else {
            format!("Bad contact on {} pin(s)", count)
        },
    };

    if pass {
        emit_log(window, "info", "Pin contact check passed");
    } else {
        let pin_list = result
            .bad_pins
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        emit_log(
            window,
            "warn",
            &format!("Pin contact check failed: bad contact on pin(s) {}. Operation aborted.", pin_list),
        );
        emit_pin_test_result(window, &dto);
        return Err(format!(
            "Pin contact check failed: bad contact on {} pin(s): {}",
            count, pin_list
        ));
    }
    Ok(())
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
    /// Config name from the XML `<config name="...">` attribute (e.g., "avr_11").
    /// Used by the frontend to look up fuse bit definitions.
    config_name: Option<String>,
    /// Raw pin_map value from the database (lower byte = index into `<maps>`).
    /// 0 means no contact-test data (use pin_count fallback for placement).
    pin_map: u32,
    /// True if chip has off_protect_before flag (needs unprotect before write).
    off_protect_before: bool,
    /// True if chip has protect_after flag (can be write-protected after write).
    protect_after: bool,
}

#[derive(Serialize)]
pub struct VoltagesDto {
    vpp: String,
    vdd: String,
    vcc: String,
}

impl VoltagesDto {
    /// Build a `VoltagesDto` from raw voltage values using the per-model
    /// voltage tables.  When `model` is `None` (no programmer connected),
    /// falls back to the TL866II+ tables — the most common model.
    ///
    /// For logic ICs, VPP and VDD are not applicable (returned as "—").
    fn from_voltages(v: &Voltages, model: Option<ProgrammerModel>, chip_type: u32, custom_protocol: bool) -> Self {
        let is_logic = chip_type == ChipType::Logic as u32;
        let model = model.unwrap_or(ProgrammerModel::Tl866iiPlus);
        let vcc_table = minipro_core::device::vcc_voltage_table(model, chip_type, custom_protocol);
        let vpp_table = minipro_core::device::vpp_voltage_table(model, chip_type, custom_protocol);

        let lookup = |code: u8, table: Option<&'static [(&'static str, u8)]>| -> String {
            match table {
                Some(t) => minipro_core::device::voltage_name(t, code)
                    .unwrap_or("?")
                    .to_string(),
                None => "—".to_string(),
            }
        };

        Self {
            vpp: if is_logic { "—".to_string() } else { lookup(v.vpp, vpp_table) },
            vdd: if is_logic { "—".to_string() } else { lookup(v.vdd, vcc_table) },
            vcc: lookup(v.vcc, vcc_table),
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
    /// Unprotect chip before write (if device supports off_protect_before).
    #[serde(default)]
    pub unprotect_before: bool,
    /// Re-protect chip after write (if device supports protect_after).
    #[serde(default)]
    pub protect_after_op: bool,
    /// Run pin contact check before the operation (matches XGPro "Pin Detect").
    #[serde(default = "default_true")]
    pub pin_check: bool,
}

fn default_icsp_mode() -> String { "zif".into() }

/// Set ICSP mode from the GUI's mode string.
/// "icsp" → ICSP with VCC (0x81), "icsp_no_vcc" → ICSP without VCC (0x80),
/// "zif" → ZIF socket mode (0x00).  Also auto-activates ICSP for ICSP-only chips.
fn set_icsp_from_mode(handle: &mut minipro_core::handle::MiniproHandle, mode: &str, device: &minipro_core::device::Device) {
    use minipro_core::device::{MP_ICSP_ONLY, MP_ZIF_ONLY};
    if device.flags.prog_support == MP_ICSP_ONLY {
        handle.set_icsp(true);
    } else if device.flags.prog_support == MP_ZIF_ONLY {
        handle.icsp = 0;
    } else {
        match mode {
            "icsp" => handle.set_icsp(true),
            "icsp_no_vcc" => handle.set_icsp(false),
            _ => handle.icsp = 0,
        }
    }
}
fn default_page() -> String { "code".into() }
fn default_format() -> String { "auto".into() }
fn default_size_mismatch() -> String { "error".into() }
fn default_true() -> bool { true }

/// Apply voltage overrides from GUI options to a device.
///
/// Uses the per-model voltage tables from `minipro_core::device` to look up
/// firmware codes from human-readable voltage strings.  When `model` is
/// `None`, falls back to TL866II+ tables.
fn apply_voltage_overrides(
    device: &mut Device,
    options: &OperationOptions,
    model: Option<ProgrammerModel>,
) -> Result<(), String> {
    let model = model.unwrap_or(ProgrammerModel::Tl866iiPlus);
    let vcc_table = minipro_core::device::vcc_voltage_table(model, device.chip_type, device.flags.custom_protocol);
    let vpp_table = minipro_core::device::vpp_voltage_table(model, device.chip_type, device.flags.custom_protocol);

    let valid_list = |table: Option<&[(&str, u8)]>| -> String {
        match table {
            Some(t) => t.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", "),
            None => "(no voltage overrides supported for this device)".to_string(),
        }
    };

    if let Some(ref v) = options.vpp {
        let table = vpp_table.ok_or_else(|| format!("VPP override not supported for this device; valid values: {}", valid_list(vpp_table)))?;
        let code = minipro_core::device::lookup_voltage(table, v)
            .ok_or_else(|| format!("invalid vpp voltage '{v}'; valid values: {}", valid_list(Some(table))))?;
        device.voltages.vpp = code;
    }
    if let Some(ref v) = options.vdd {
        let table = vcc_table.ok_or_else(|| format!("VDD override not supported for this device; valid values: {}", valid_list(vcc_table)))?;
        let code = minipro_core::device::lookup_voltage(table, v)
            .ok_or_else(|| format!("invalid vdd voltage '{v}'; valid values: {}", valid_list(Some(table))))?;
        device.voltages.vdd = code;
    }
    if let Some(ref v) = options.vcc {
        let table = vcc_table.ok_or_else(|| format!("VCC override not supported for this device; valid values: {}", valid_list(vcc_table)))?;
        let code = minipro_core::device::lookup_voltage(table, v)
            .ok_or_else(|| format!("invalid vcc voltage '{v}'; valid values: {}", valid_list(Some(table))))?;
        device.voltages.vcc = code;
    }
    Ok(())
}

// ── Voltage options for GUI dropdowns ───────────────────────────────────────

/// Valid voltage override values for the connected programmer and selected device.
///
/// `vcc` / `vpp` are `None` when overrides are not supported (e.g. custom
/// protocol on T56/T76, or VPP on logic ICs).  `is_logic` tells the GUI to
/// hide the VPP and VDD dropdowns entirely.
#[derive(Serialize)]
pub struct VoltageOptionsDto {
    vcc: Option<Vec<String>>,
    vpp: Option<Vec<String>>,
    is_logic: bool,
}

/// Return the valid voltage override options for the connected programmer
/// model and currently selected device.  When no device is selected, all
/// fields are `None`.  When no programmer is connected, falls back to
/// TL866II+ tables (matching `VoltagesDto::from_voltages` behavior).
#[tauri::command]
pub async fn get_voltage_options(state: State<'_, Arc<AppState>>) -> Result<VoltageOptionsDto, String> {
    let device = state.get_device();
    let model = {
        let guard = state.programmer_info.lock().map_err(|e| e.to_string())?;
        guard.as_ref().map(|info| info.model)
    };

    let device = match device {
        Ok(dev) => dev,
        Err(_) => return Ok(VoltageOptionsDto { vcc: None, vpp: None, is_logic: false }),
    };

    let model = model.unwrap_or(ProgrammerModel::Tl866iiPlus);
    let chip_type = device.chip_type;
    let custom_protocol = device.flags.custom_protocol;
    let is_logic = chip_type == ChipType::Logic as u32;

    let names = |table: Option<&'static [(&'static str, u8)]>| -> Option<Vec<String>> {
        table.map(|t| t.iter().map(|(n, _)| n.to_string()).collect())
    };

    let vcc = names(minipro_core::device::vcc_voltage_table(model, chip_type, custom_protocol));
    let vpp = names(minipro_core::device::vpp_voltage_table(model, chip_type, custom_protocol));

    Ok(VoltageOptionsDto { vcc, vpp, is_logic })
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
///
/// Retries a few times at startup because Windows USB enumeration can lag
/// behind the physical plug event by several seconds.
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

    // Retry a few times — the device may not be ready immediately after
    // a hot-plug or sleep/wake cycle.
    let delays = [0u64, 500, 1000, 1500];
    let mut last_err = String::new();

    // Clone the Arc<AppState> so the spawn_blocking closure can populate
    // the handle's db_paths (needed for T56/T76 algorithm lookup).
    let state_arc = (*state).clone();

    for (attempt, delay_ms) in delays.iter().enumerate() {
        if *delay_ms > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(*delay_ms)).await;
        }

        let state_for_task = state_arc.clone();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            tokio::task::spawn_blocking(move || {
                let mut handle = MiniproHandle::open().map_err(|e| e.to_string())?;

                // Populate db_paths on the handle so T56/T76 algorithm lookup
                // works (matches what lib.rs does at startup).
                {
                    let guard = state_for_task.db_paths.lock().map_err(|e| e.to_string())?;
                    if let Some(ref paths) = *guard {
                        handle.db_paths = Some(paths.clone());
                    }
                }

                let info = handle.info.clone();
                Ok::<(minipro_core::device::ProgrammerInfo, MiniproHandle), String>((info, handle))
            }),
        ).await;

        let (info, handle) = match result {
            Ok(Ok(Ok(v))) => v,
            Ok(Ok(Err(e))) => {
                last_err = e;
                eprintln!("get_programmer_info attempt {} failed: {}", attempt + 1, last_err);
                continue;
            }
            Ok(Err(e)) => {
                last_err = format!("Task panicked: {}", e);
                eprintln!("get_programmer_info attempt {} panicked", attempt + 1);
                continue;
            }
            Err(_) => {
                last_err = "Timed out waiting for USB device to respond".into();
                eprintln!("get_programmer_info attempt {} timed out", attempt + 1);
                continue;
            }
        };

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

    Err(last_err)
}

/// Force-close any existing handle and re-open the programmer.
/// Use this after unplugging/replugging the device.
///
/// Retries for up to ~15 seconds with increasing delays because Windows USB
/// enumeration can lag behind the physical replug by several seconds, and
/// the device may need time to re-initialise after a sleep/wake cycle or
/// hot-plug event.
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
    // the Device Manager display by several seconds after hot-plug, and a
    // sleep/wake Code 10 recovery may require 20-30 seconds before the
    // device is usable again.
    let delays = [500, 1000, 1500, 2000, 2000, 2000, 2000, 2000];
    let mut last_err = String::new();

    // Clone the Arc<AppState> so the spawn_blocking closure can populate
    // the handle's db_paths (needed for T56/T76 algorithm lookup after
    // reconnect).
    let state_arc = (*state).clone();

    for (attempt, delay_ms) in delays.iter().enumerate() {
        tokio::time::sleep(std::time::Duration::from_millis(*delay_ms)).await;

        // Wrap the blocking open in a timeout so a hung USB transfer
        // doesn't block the retry loop forever.  The orphaned task will
        // eventually finish or be cleaned up by tokio.
        let state_for_task = state_arc.clone();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            tokio::task::spawn_blocking(move || {
                let mut handle = MiniproHandle::open().map_err(|e| e.to_string())?;

                // Reset pin drivers to clear any stale transaction state
                // left over from before the unplug.  If the programmer wasn't
                // fully power-cycled (powered USB hub, capacitor), the firmware
                // may still have an active transaction with wrong pin config,
                // causing reads to return all 0xFF.  reset_state sends
                // CMD_RESET_PIN_DRIVERS which clears the ZIF socket.
                if let Err(e) = handle.protocol.reset_state(&handle.usb) {
                    eprintln!("force_reconnect: reset_state warning: {}", e);
                    // Non-fatal — continue anyway
                }

                // Populate db_paths on the handle so T56/T76 algorithm lookup
                // works after reconnect (matches what lib.rs does at startup).
                {
                    let guard = state_for_task.db_paths.lock().map_err(|e| e.to_string())?;
                    if let Some(ref paths) = *guard {
                        handle.db_paths = Some(paths.clone());
                    }
                }

                let info = handle.info.clone();
                Ok::<(minipro_core::device::ProgrammerInfo, MiniproHandle), String>((info, handle))
            }),
        ).await;

        match result {
            Ok(Ok(Ok((info, handle)))) => {
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
            Ok(Ok(Err(e))) => {
                last_err = e;
                eprintln!("force_reconnect attempt {} failed: {}", attempt + 1, last_err);
            }
            Ok(Err(e)) => {
                last_err = format!("Task panicked: {}", e);
                eprintln!("force_reconnect attempt {} panicked", attempt + 1);
            }
            Err(_) => {
                last_err = "Timed out waiting for USB device to respond".into();
                eprintln!("force_reconnect attempt {} timed out after 5s", attempt + 1);
            }
        }
    }

    Err(format!(
        "Could not reconnect after {} attempts ({}s). \
         If the programmer was connected when the computer went to sleep, \
         unplug the USB cable, wait 20-30 seconds, then plug it back in \
         and click the reconnect button again. \
         To prevent this in future, disable 'USB selective suspend' in \
         Windows Power Options. Last error: {}",
        delays.len(),
        delays.iter().sum::<u64>() / 1000,
        last_err
    ))
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
    let model = {
        let guard = state.programmer_info.lock().map_err(|e| e.to_string())?;
        guard.as_ref().map(|info| info.model)
    };

    tokio::task::spawn_blocking(move || {
        let dev = find_device_any(&db, &name_clone).map_err(|e| e.to_string())?;
        Ok::<DeviceInfoDto, String>(device_to_dto(&dev, model))
    })
    .await
    .map_err(|e| format!("Task panicked: {}", e))?
}

/// Get the pin-contact map for a device (ZIF pin numbers that must make contact).
/// Returns None when pin_map index is 0 (no contact-test data).
#[tauri::command]
pub async fn get_device_pin_map(pinMap: u32, state: State<'_, Arc<AppState>>) -> Result<Option<PinMapDto>, String> {
    let index = pinMap & 0xFF;
    if index == 0 {
        return Ok(None);
    }
    let db = get_db_paths(&state)?;

    tokio::task::spawn_blocking(move || {
        let pm = get_pin_map(&db.infoic, index).map_err(|e| e.to_string())?;
        Ok::<Option<PinMapDto>, String>(pm.map(|p| PinMapDto {
            gnd_table: p.gnd_table,
            mask: p.mask,
        }))
    })
    .await
    .map_err(|e| format!("Task panicked: {}", e))?
}

#[derive(Serialize, Clone)]
pub struct PinMapDto {
    /// ZIF pin numbers to drive as GND during contact test.
    pub gnd_table: Vec<u16>,
    /// ZIF pin numbers that must make electrical contact (chip footprint).
    pub mask: Vec<u16>,
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
        Ok::<(DeviceInfoDto, Device), String>((device_to_dto(&dev, model), dev))
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

            set_icsp_from_mode(&mut handle, &options_clone.icsp_mode, &device);
            handle.begin_transaction(device.clone()).map_err(|e| e.to_string())?;

            // Pin contact check (pre-operation gate)
            let db_paths = get_db_paths(&state_task)?;
            run_pin_check_if_enabled(
                &mut handle,
                options_clone.pin_check,
                &options_clone.icsp_mode,
                &window_clone,
                &db_paths.infoic,
            )?;

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

            set_icsp_from_mode(&mut handle, &options_clone.icsp_mode, &device);
            handle.begin_transaction(device.clone()).map_err(|e| e.to_string())?;

            // Pin contact check (pre-operation gate)
            let db_paths = get_db_paths(&state_task)?;
            run_pin_check_if_enabled(
                &mut handle,
                options_clone.pin_check,
                &options_clone.icsp_mode,
                &window_clone,
                &db_paths.infoic,
            )?;

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
    let model = {
        let guard = state_clone.programmer_info.lock().map_err(|e| e.to_string())?;
        guard.as_ref().map(|info| info.model)
    };

    let result = tokio::task::spawn_blocking(move || {
        let mut handle = state_task.take_handle()?;
        let device_arc = state_task.get_device()?;
        let mut device = (*device_arc).clone();
        apply_voltage_overrides(&mut device, &options_clone, model).map_err(|e| e.to_string())?;
        let device = Arc::new(device);
        let page = parse_page(&options_clone.page)?;
        let size_mismatch = parse_size_mismatch(&options_clone.size_mismatch)?;
        let op_name = "write".to_string();

        let result = (|| {
            set_icsp_from_mode(&mut handle, &options_clone.icsp_mode, &device);
            handle.begin_transaction(device.clone()).map_err(|e| e.to_string())?;

            // Pin contact check (pre-operation gate)
            let db_paths = get_db_paths(&state_task)?;
            run_pin_check_if_enabled(
                &mut handle,
                options_clone.pin_check,
                &options_clone.icsp_mode,
                &window_clone,
                &db_paths.infoic,
            )?;

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

            // ── Protect off (before erase/write) ──────────────────────────────
            // T76 + off_protect_before: auto-unprotect regardless of checkbox.
            // Non-T76: only if unprotect_before checkbox AND off_protect_before.
            let off_protect = device.flags.off_protect_before;
            let is_t76 = handle.info.model == minipro_core::device::ProgrammerModel::T76;
            if off_protect && (is_t76 || options_clone.unprotect_before) {
                emit_log(&window_clone, "info", "Protect off...");
                handle.protocol.protect_off(&handle.usb).map_err(|e| e.to_string())?;
                emit_log(&window_clone, "info", "Protect off...OK");
                if is_t76 {
                    handle.end_transaction().map_err(|e| e.to_string())?;
                    handle.begin_transaction(device.clone()).map_err(|e| e.to_string())?;
                }
            }

            if !options_clone.skip_erase {
                erase_chip(&mut handle, false).map_err(|e| e.to_string())?;
                handle.end_transaction().map_err(|e| e.to_string())?;
                handle.begin_transaction(device.clone()).map_err(|e| e.to_string())?;
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
                verify_chip(
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

            // ── Protect on (after write + verify) ─────────────────────────────
            if options_clone.protect_after_op && device.flags.protect_after {
                emit_log(&window_clone, "info", "Protect on...");
                handle.protocol.protect_on(&handle.usb).map_err(|e| e.to_string())?;
                emit_log(&window_clone, "info", "Protect on...OK");
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
    let model = {
        let guard = state_clone.programmer_info.lock().map_err(|e| e.to_string())?;
        guard.as_ref().map(|info| info.model)
    };

    let result = tokio::task::spawn_blocking(move || {
        let mut handle = state_task.take_handle()?;
        let device_arc = state_task.get_device()?;
        let mut device = (*device_arc).clone();
        apply_voltage_overrides(&mut device, &options_clone, model).map_err(|e| e.to_string())?;
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
            set_icsp_from_mode(&mut handle, &options_clone.icsp_mode, &device);
            handle.begin_transaction(device.clone()).map_err(|e| e.to_string())?;

            // Pin contact check (pre-operation gate)
            let db_paths = get_db_paths(&state_task)?;
            run_pin_check_if_enabled(
                &mut handle,
                options_clone.pin_check,
                &options_clone.icsp_mode,
                &window_clone,
                &db_paths.infoic,
            )?;

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

            // ── Protect off (before erase/write) ──────────────────────────────
            let off_protect = device.flags.off_protect_before;
            let is_t76 = handle.info.model == minipro_core::device::ProgrammerModel::T76;
            if off_protect && (is_t76 || options_clone.unprotect_before) {
                emit_log(&window_clone, "info", &format!("Chip {}: protect off...", chipNumber));
                handle.protocol.protect_off(&handle.usb).map_err(|e| e.to_string())?;
                emit_log(&window_clone, "info", &format!("Chip {}: protect off...OK", chipNumber));
                if is_t76 {
                    handle.end_transaction().map_err(|e| e.to_string())?;
                    handle.begin_transaction(device.clone()).map_err(|e| e.to_string())?;
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
                    verify_chip_bytes(
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

                // ── Protect on (after write + verify) ─────────────────────────
                if options_clone.protect_after_op && device.flags.protect_after {
                    emit_log(&window_clone, "info", &format!("Chip {}: protect on...", chipNumber));
                    handle.protocol.protect_on(&handle.usb).map_err(|e| e.to_string())?;
                    emit_log(&window_clone, "info", &format!("Chip {}: protect on...OK", chipNumber));
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
                verify_chip(
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

            // ── Protect on (after write + verify) ─────────────────────────────
            if options_clone.protect_after_op && device.flags.protect_after {
                emit_log(&window_clone, "info", &format!("Chip {}: protect on...", chipNumber));
                handle.protocol.protect_on(&handle.usb).map_err(|e| e.to_string())?;
                emit_log(&window_clone, "info", &format!("Chip {}: protect on...OK", chipNumber));
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
    let model = {
        let guard = state_clone.programmer_info.lock().map_err(|e| e.to_string())?;
        guard.as_ref().map(|info| info.model)
    };

    let result = tokio::task::spawn_blocking(move || {
        let bytes = base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            &base64Data,
        )
        .map_err(|e| format!("Failed to decode base64: {}", e))?;

        let mut handle = state_task.take_handle()?;
        let device_arc = state_task.get_device()?;
        let mut device = (*device_arc).clone();
        apply_voltage_overrides(&mut device, &options_clone, model).map_err(|e| e.to_string())?;
        let device = Arc::new(device);
        let page = parse_page(&options_clone.page)?;
        let size_mismatch = parse_size_mismatch(&options_clone.size_mismatch)?;
        let op_name = "write".to_string();

        let result = (|| {
            set_icsp_from_mode(&mut handle, &options_clone.icsp_mode, &device);
            handle.begin_transaction(device.clone()).map_err(|e| e.to_string())?;

            // Pin contact check (pre-operation gate)
            let db_paths = get_db_paths(&state_task)?;
            run_pin_check_if_enabled(
                &mut handle,
                options_clone.pin_check,
                &options_clone.icsp_mode,
                &window_clone,
                &db_paths.infoic,
            )?;

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

            // ── Protect off (before erase/write) ──────────────────────────────
            let off_protect = device.flags.off_protect_before;
            let is_t76 = handle.info.model == minipro_core::device::ProgrammerModel::T76;
            if off_protect && (is_t76 || options_clone.unprotect_before) {
                emit_log(&window_clone, "info", "Protect off...");
                handle.protocol.protect_off(&handle.usb).map_err(|e| e.to_string())?;
                emit_log(&window_clone, "info", "Protect off...OK");
                if is_t76 {
                    handle.end_transaction().map_err(|e| e.to_string())?;
                    handle.begin_transaction(device.clone()).map_err(|e| e.to_string())?;
                }
            }

            if !options_clone.skip_erase {
                erase_chip(&mut handle, false).map_err(|e| e.to_string())?;
                handle.end_transaction().map_err(|e| e.to_string())?;
                handle.begin_transaction(device.clone()).map_err(|e| e.to_string())?;
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
                verify_chip_bytes(
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

            // ── Protect on (after write + verify) ─────────────────────────────
            if options_clone.protect_after_op && device.flags.protect_after {
                emit_log(&window_clone, "info", "Protect on...");
                handle.protocol.protect_on(&handle.usb).map_err(|e| e.to_string())?;
                emit_log(&window_clone, "info", "Protect on...OK");
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
            set_icsp_from_mode(&mut handle, &options_clone.icsp_mode, &device);
            handle.begin_transaction(device).map_err(|e| e.to_string())?;

            // Pin contact check (pre-operation gate)
            let db_paths = get_db_paths(&state_task)?;
            run_pin_check_if_enabled(
                &mut handle,
                options_clone.pin_check,
                &options_clone.icsp_mode,
                &window_clone,
                &db_paths.infoic,
            )?;

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
pub async fn do_erase(icspMode: String, checkDeviceId: bool, pinCheck: bool, window: Window, state: State<'_, Arc<AppState>>) -> Result<(), String> {
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
            set_icsp_from_mode(&mut handle, &icspMode, &device);
            handle.begin_transaction(device).map_err(|e| e.to_string())?;

            // Pin contact check (pre-operation gate)
            let db_paths = get_db_paths(&state_task)?;
            run_pin_check_if_enabled(
                &mut handle,
                pinCheck,
                &icspMode,
                &window_clone,
                &db_paths.infoic,
            )?;

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
pub async fn do_blank_check(icspMode: String, pinCheck: bool, window: Window, state: State<'_, Arc<AppState>>) -> Result<BlankCheckResultDto, String> {
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
            set_icsp_from_mode(&mut handle, &icspMode, &device);
            handle.begin_transaction(device).map_err(|e| e.to_string())?;

            // Pin contact check (pre-operation gate)
            let db_paths = get_db_paths(&state_task)?;
            run_pin_check_if_enabled(
                &mut handle,
                pinCheck,
                &icspMode,
                &window_clone,
                &db_paths.infoic,
            )?;

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
pub async fn do_chip_id(icspMode: String, pinCheck: bool, window: Window, state: State<'_, Arc<AppState>>) -> Result<ChipIdResultDto, String> {
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
            set_icsp_from_mode(&mut handle, &icspMode, &device);
            handle.begin_transaction(device.clone()).map_err(|e| e.to_string())?;

            // Pin contact check (pre-operation gate)
            let db_paths = get_db_paths(&state_task)?;
            run_pin_check_if_enabled(
                &mut handle,
                pinCheck,
                &icspMode,
                &window_clone,
                &db_paths.infoic,
            )?;

            let (_id_type, chip_id) = handle.protocol.get_chip_id(&handle.usb, &device).map_err(|e| e.to_string())?;
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
            let bytes = if is_variant { 4 } else { device.chip_id_bytes_count.clamp(1, 4) };
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

/// Structured logic IC test result for the GUI.
#[derive(Serialize)]
pub struct LogicTestResultDto {
    pub pinCount: u16,
    pub vectorCount: u16,
    pub vectors: Vec<u8>,
    pub step1: Vec<u8>,
    pub step2: Vec<u8>,
    pub errors: u32,
    pub pass: bool,
}

/// Test a logic IC against its built-in test vectors.
/// Returns a structured result for the GUI grid rendering.
/// `vcc` is an optional VCC override (e.g. "3.3") for logic ICs.
#[tauri::command]
pub async fn do_logic_test(icspMode: String, vcc: Option<String>, state: State<'_, Arc<AppState>>) -> Result<LogicTestResultDto, String> {
    let state_clone = (*state).clone();
    if !state_clone.try_acquire() {
        return Err("Another operation is already running".into());
    }

    let state_task = state_clone.clone();
    let result = tokio::task::spawn_blocking(move || {
        let mut handle = state_task.take_handle()?;
        let device = state_task.get_device()?;

        let result = (|| {
            set_icsp_from_mode(&mut handle, &icspMode, &device);

            // Apply VCC override for logic ICs before begin_transaction.
            // Logic ICs only support VCC (from the 4-entry logic table).
            let device = if let Some(ref v) = vcc {
                if !v.is_empty() {
                    let model = handle.info.model;
                    let mut dev = (*device).clone();
                    let options = OperationOptions {
                        skip_erase: false,
                        skip_verify: false,
                        skip_blank: false,
                        check_device_id: false,
                        vpp: None,
                        vcc: Some(v.clone()),
                        vdd: None,
                        icsp_mode: icspMode.clone(),
                        page: "code".to_string(),
                        format: "auto".to_string(),
                        size_mismatch: "error".to_string(),
                        unprotect_before: false,
                        protect_after_op: false,
                        pin_check: false,
                    };
                    apply_voltage_overrides(&mut dev, &options, Some(model))
                        .map_err(|e| e.to_string())?;
                    std::sync::Arc::new(dev)
                } else {
                    device
                }
            } else {
                device
            };

            handle.begin_transaction(device).map_err(|e| e.to_string())?;
            let test_result = logic_ic_test(&mut handle).map_err(|e| e.to_string())?;
            Ok::<LogicTestResultDto, String>(LogicTestResultDto {
                pinCount: test_result.pin_count,
                vectorCount: test_result.vector_count,
                vectors: test_result.vectors,
                step1: test_result.step1,
                step2: test_result.step2,
                errors: test_result.errors,
                pass: test_result.pass,
            })
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

// ── Logic IC identify (auto-find) ───────────────────────────────────────────

#[derive(Serialize)]
pub struct LogicIdentifyResultDto {
    pub name: String,
    pub manufacturer: String,
    pub pass: bool,
    pub errors: u32,
}

/// Test an unknown logic IC against all database entries with a matching pin
/// count.  Returns a sorted list of results (passing entries first).
/// `vcc` is an optional VCC override (e.g. "3.3") for logic ICs.
#[tauri::command]
pub async fn do_logic_identify(
    pinCount: u8,
    vcc: Option<String>,
    state: State<'_, Arc<AppState>>,
    window: Window,
) -> Result<Vec<LogicIdentifyResultDto>, String> {
    let state_clone = (*state).clone();
    if !state_clone.try_acquire() {
        return Err("Another operation is already running".into());
    }

    let state_task = state_clone.clone();
    let window_clone = window.clone();
    let result = tokio::task::spawn_blocking(move || {
        let mut handle = state_task.take_handle()?;
        let db = get_db_paths(&state_task)?;
        let model = handle.info.model;

        let result = (|| {
            let mut progress_cb = |done: usize, total: usize| {
                let _ = window_clone.emit(
                    "progress",
                    ProgressPayload {
                        done,
                        total,
                        operation: "identify".to_string(),
                    },
                );
            };

            let entries = logic_auto_find(
                &mut handle,
                &db,
                pinCount,
                vcc.as_deref(),
                model,
                Some(&mut progress_cb),
            )
            .map_err(|e| e.to_string())?;

            Ok::<Vec<LogicIdentifyResultDto>, String>(
                entries
                    .into_iter()
                    .map(|e| LogicIdentifyResultDto {
                        name: e.name,
                        manufacturer: e.manufacturer,
                        pass: e.pass,
                        errors: e.errors,
                    })
                    .collect(),
            )
        })();

        let _ = state_task.store_handle(handle);
        if let Err(ref e) = result {
            handle_usb_error(&state_task, e);
        }
        result
    })
    .await;

    state_clone.release();

    match result {
        Ok(Ok(dtos)) => Ok(dtos),
        Ok(Err(e)) => Err(e),
        Err(e) => Err(format!("Task panicked: {}", e)),
    }
}

// ── SPI flash autodetect ───────────────────────────────────────────────────

#[derive(Serialize)]
pub struct SpiAutodetectMatchDto {
    pub name: String,
    pub manufacturer: String,
}

#[derive(Serialize)]
pub struct SpiAutodetectResultDto {
    pub jedec_id: u32,
    pub matches: Vec<SpiAutodetectMatchDto>,
}

/// Auto-detect an SPI flash chip by reading its JEDEC ID and searching the database.
///
/// `idType` selects the package: 0 = 8-pin, 1 = 16-pin.
/// Does NOT require a device to be selected — autodetect is a standalone firmware
/// command (0x37) that needs no transaction context.
///
/// On TL866II+/T48 in ZIF mode, a pin contact check is run automatically before
/// autodetect. If bad pins are found, autodetect is aborted with a clear
/// diagnostic message — matching upstream minipro's `-z` + `-a` behavior.
#[tauri::command]
pub async fn do_spi_autodetect(idType: u8, window: Window, state: State<'_, Arc<AppState>>) -> Result<SpiAutodetectResultDto, String> {
    let state_clone = (*state).clone();
    if !state_clone.try_acquire() {
        return Err("Another operation is already running".into());
    }

    let state_task = state_clone.clone();
    let window_clone = window.clone();
    let result = tokio::task::spawn_blocking(move || {
        let mut handle = state_task.take_handle()?;

        let result = (|| {
            let db = get_db_paths(&state_task)?;

            // Automatic pin contact check before autodetect on supported models.
            // Constructs a temporary device with pin_map based on package type,
            // matching upstream minipro's auto_detect function (main.c line 482-503).
            if matches!(
                handle.info.model,
                ProgrammerModel::Tl866iiPlus | ProgrammerModel::T48
            ) && handle.icsp == 0
            {
                let pin_count = if idType == 0 { 8 } else { 16 };
                let temp_device = Arc::new(Device {
                    pin_map: if idType == 0 { 0x01 } else { 0x03 },
                    package_details: PackageDetails {
                        pin_count,
                        ..Default::default()
                    },
                    ..Default::default()
                });
                handle.device = Some(temp_device);
                let pin_result = pin_contact_check(&mut handle, &db.infoic);
                handle.device = None;
                match pin_result {
                    Ok(r) if r.bad_pins.is_empty() => {
                        emit_log(&window_clone, "info", "Pin contact check passed");
                    }
                    Ok(r) => {
                        let count = r.bad_pins.len();
                        let pin_list = r.bad_pins.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(", ");
                        let msg = format!("Pin contact check failed: bad contact on pin(s) {}. Autodetect aborted.", pin_list);
                        emit_log(&window_clone, "warn", &msg);
                        emit_pin_test_result(&window_clone, &PinTestResultDto {
                            supported: true,
                            pass: false,
                            bad_pins: r.bad_pins,
                            message: format!("Bad contact on {} pin(s)", count),
                        });
                        return Err(msg);
                    }
                    Err(e) => {
                        let msg = format!("Pin contact check error: {}. Autodetect aborted.", e);
                        emit_log(&window_clone, "warn", &msg);
                        return Err(msg);
                    }
                }
            }

            let autodetect = spi_autodetect_and_lookup(&mut handle, &db, idType)
                .map_err(|e| e.to_string())?;
            let matches = autodetect
                .matches
                .into_iter()
                .map(|item| SpiAutodetectMatchDto {
                    name: item.name,
                    manufacturer: item.manufacturer,
                })
                .collect();
            Ok::<SpiAutodetectResultDto, String>(SpiAutodetectResultDto {
                jedec_id: autodetect.jedec_id,
                matches,
            })
        })();

        // Always return the handle, even on error
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

/// Check whether the chip database files can be located.
#[tauri::command]
pub async fn check_database(state: State<'_, Arc<AppState>>) -> Result<bool, String> {
    match get_db_paths(&state) {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),
    }
}

/// DTO for database directory status returned to the GUI.
#[derive(Serialize)]
pub struct DbDirStatusDto {
    /// The saved custom directory path, or null if using default search.
    pub customDir: Option<String>,
    /// True if the custom directory is actively in use.
    /// False if a custom dir was saved but is invalid (fell back to default).
    pub active: bool,
}

/// Return the current database directory status for the Settings panel.
#[tauri::command]
pub async fn get_db_status(state: State<'_, Arc<AppState>>) -> Result<DbDirStatusDto, String> {
    // The custom dir is not stored in AppState — the GUI reads it from
    // the settings store. We only return the invalid flag here so the
    // GUI can show a warning if the saved dir fell back to default.
    let invalid = state.db_dir_invalid.load(std::sync::atomic::Ordering::SeqCst);
    Ok(DbDirStatusDto {
        customDir: None, // GUI fills this from its own settings store
        active: !invalid,
    })
}

/// Set or clear a custom database directory.
///
/// When `dir` is `Some(path)`, both `infoic.xml` and `logicic.xml` must
/// exist in that directory. `algorithm.xml` is picked up if present but
/// not required. When `dir` is `None`, reverts to the standard search
/// path. Reloads the device list and clears the selected device.
#[tauri::command]
pub async fn set_custom_db_dir(
    dir: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let paths = match dir.as_deref() {
        Some(d) => {
            let dir_path = std::path::Path::new(d);
            let infoic = dir_path.join("infoic.xml");
            let logicic = dir_path.join("logicic.xml");
            if !infoic.exists() {
                state
                    .db_dir_invalid
                    .store(true, std::sync::atomic::Ordering::SeqCst);
                return Err(format!(
                    "infoic.xml not found in '{}'",
                    dir_path.display()
                ));
            }
            if !logicic.exists() {
                state
                    .db_dir_invalid
                    .store(true, std::sync::atomic::Ordering::SeqCst);
                return Err(format!(
                    "logicic.xml not found in '{}'",
                    dir_path.display()
                ));
            }
            // algorithm.xml is optional — pass it as an override only if present
            let algorithms = dir_path.join("algorithm.xml");
            let algo_override = if algorithms.exists() { Some(algorithms.as_path()) } else { None };
            DatabasePaths::resolve(Some(&infoic), Some(&logicic), algo_override)
                .map_err(|e| format!("Failed to resolve database: {}", e))?
        }
        None => DatabasePaths::resolve(None, None, None)
            .map_err(|e| format!("Failed to resolve database: {}", e))?,
    };

    // Update the cached paths
    {
        let mut guard = state.db_paths.lock().map_err(|e| e.to_string())?;
        *guard = Some(DatabasePaths {
            infoic: paths.infoic.clone(),
            logicic: paths.logicic.clone(),
            algorithms: paths.algorithms.clone(),
        });
    }

    // Clear the invalid flag — the new directory is valid (or we reset to default)
    state
        .db_dir_invalid
        .store(false, std::sync::atomic::Ordering::SeqCst);

    // Reload device names from the new database
    state.load_device_names().map_err(|e| e.to_string())?;

    // Clear the selected device (may not exist in the new database)
    state.set_device(None).map_err(|e| e.to_string())?;

    Ok(())
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
pub async fn read_fuses(icspMode: String, pinCheck: bool, window: Window, state: State<'_, Arc<AppState>>) -> Result<ConfigDataDto, String> {
    let state_clone = (*state).clone();
    if !state_clone.try_acquire() {
        return Err("Another operation is already running".into());
    }

    let state_task = state_clone.clone();
    let window_clone = window.clone();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::task::spawn_blocking(move || {
            let mut handle = state_task.take_handle()?;

            let device = state_task.get_device()?;
            let result = (|| {
                set_icsp_from_mode(&mut handle, &icspMode, &device);
                handle.begin_transaction(device).map_err(|e| e.to_string())?;

                // Pin contact check (pre-operation gate)
                let db_paths = get_db_paths(&state_task)?;
                run_pin_check_if_enabled(
                    &mut handle,
                    pinCheck,
                    &icspMode,
                    &window_clone,
                    &db_paths.infoic,
                )?;

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
pub async fn write_fuses(cfgFuses: Vec<FuseValueDto>, lockBits: Vec<FuseValueDto>, icspMode: String, pinCheck: bool, window: Window, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let state_clone = (*state).clone();
    if !state_clone.try_acquire() {
        return Err("Another operation is already running".into());
    }

    let state_task = state_clone.clone();
    let window_clone = window.clone();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::task::spawn_blocking(move || {
            let mut handle = state_task.take_handle()?;

            let device = state_task.get_device()?;
            let result = (|| {
                set_icsp_from_mode(&mut handle, &icspMode, &device);
                handle.begin_transaction(device.clone()).map_err(|e| e.to_string())?;

                // Pin contact check (pre-operation gate)
                let db_paths = get_db_paths(&state_task)?;
                run_pin_check_if_enabled(
                    &mut handle,
                    pinCheck,
                    &icspMode,
                    &window_clone,
                    &db_paths.infoic,
                )?;

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
                set_icsp_from_mode(&mut handle, &icspMode, &device);
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

                // Determine whether lock bits indicate external read/write
                // protection.  AVR lock bits have a specific layout where only
                // the LB bits (1:0) control external access; BLB0 (3:2) and
                // BLB1 (5:4) only affect internal SPM/LPM instructions and do
                // NOT prevent external ISP read/write.  Arduino bootloaders
                // routinely set BLB1 to mode 3 (protect bootloader section),
                // which is not external protection.
                //
                // The database stores all lock bits in a single "lock" field
                // with mask 0x3F — it doesn't break out LB separately.  So we
                // detect AVR by checking the config name (e.g., "avr_11") and
                // hardcode the LB mask as 0x03.
                let dev = handle.device().map_err(|e| e.to_string())?;
                let is_avr = if let Some(minipro_core::device::ChipConfig::Mcu(ref cfg)) = dev.config {
                    cfg.name.starts_with("avr_")
                } else {
                    false
                };

                let is_protected = if is_avr {
                    // AVR: only LB bits (1:0) control external read/write.
                    // 11 = no protection, 10 = further programming disabled,
                    // 00 = programming and verification disabled.
                    (lock_byte & 0x03) != 0x03
                } else {
                    // Non-AVR: conservative default — any non-erased lock byte
                    // may indicate protection.
                    lock_byte != 0xff
                };

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

/// DTO for pin-contact test results returned to the frontend.
#[derive(Debug, Serialize)]
pub struct PinTestResultDto {
    /// Whether the programmer model supports pin testing.
    pub supported: bool,
    /// Whether all contacted pins passed (true if `bad_pins` is empty).
    pub pass: bool,
    /// Device pin numbers (1-based) that failed contact.
    pub bad_pins: Vec<u16>,
    /// Human-readable status message.
    pub message: String,
}

/// Run a ZIF socket pin-contact test.
///
/// Requires a selected device with pin-map data in the database.
/// Only supported on TL866II+, T48, and T76 (models with bit-banging
/// hardware). Returns structured bad-pin data for diagram highlighting.
#[tauri::command]
pub async fn do_pin_test(
    icspMode: String,
    state: State<'_, Arc<AppState>>,
) -> Result<PinTestResultDto, String> {
    let state_clone = (*state).clone();
    if !state_clone.try_acquire() {
        return Err("Another operation is already running".into());
    }

    let state_task = state_clone.clone();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::task::spawn_blocking(move || -> Result<PinTestResultDto, String> {
            let mut handle = state_task.take_handle()?;

            // Check programmer model support
            if !matches!(
                handle.info.model,
                ProgrammerModel::Tl866iiPlus | ProgrammerModel::T48
            ) {
                let _ = state_task.store_handle(handle);
                return Ok(PinTestResultDto {
                    supported: false,
                    pass: false,
                    bad_pins: vec![],
                    message: "Pin test not supported on this programmer model".into(),
                });
            }

            // Pin test only works in ZIF mode
            if icspMode != "zif" {
                let _ = state_task.store_handle(handle);
                return Ok(PinTestResultDto {
                    supported: false,
                    pass: false,
                    bad_pins: vec![],
                    message: "Pin test is only available in ZIF mode".into(),
                });
            }

            let device = state_task.get_device()?;
            set_icsp_from_mode(&mut handle, &icspMode, &device);

            let db_paths = get_db_paths(&state_task)?;
            let infoic_path = db_paths.infoic.clone();

            handle.begin_transaction(device).map_err(|e| e.to_string())?;
            let test_result = pin_contact_check(&mut handle, &infoic_path);
            let _ = handle.end_transaction();

            let result = test_result.map_err(|e| e.to_string())?;
            let _ = state_task.store_handle(handle);

            let pass = result.bad_pins.is_empty();
            let count = result.bad_pins.len();
            Ok(PinTestResultDto {
                supported: true,
                pass,
                bad_pins: result.bad_pins,
                message: if pass {
                    "All pins OK".into()
                } else {
                    format!("Bad contact on {} pin(s)", count)
                },
            })
        }),
    )
    .await;

    state_clone.release();

    match result {
        Ok(Ok(Ok(dto))) => Ok(dto),
        Ok(Ok(Err(e))) => {
            handle_usb_error(&state_clone, &e);
            Err(e)
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

/// Look up fuse bit definitions for a device's config name and chip name.
///
/// Returns `None` (serialized as JSON `null`) when no bit-level definitions
/// are available — the frontend falls back to hex-only input in that case.
#[tauri::command]
pub fn get_fuse_bit_defs(configName: String, chipName: String) -> Option<&'static crate::fuse_defs::FuseConfigDef> {
    crate::fuse_defs::lookup(&configName, &chipName)
}

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

fn device_to_dto(dev: &Device, model: Option<ProgrammerModel>) -> DeviceInfoDto {
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

    // Detect AVR-family devices by config name (e.g., "avr_11", "avr_6").
    // AVR convention: bit=0 means programmed (active).  PIC and others: bit=1.
    // This is more reliable than name pattern matching — the database itself
    // declares the convention via the config name.
    let invert_fuse_bits = if let Some(minipro_core::device::ChipConfig::Mcu(ref cfg)) = dev.config {
        cfg.name.starts_with("avr_")
    } else {
        false
    };

    let config_name = if let Some(minipro_core::device::ChipConfig::Mcu(ref cfg)) = dev.config {
        Some(cfg.name.clone())
    } else {
        None
    };

    DeviceInfoDto {
        name: dev.name.clone(),
        manufacturer: dev.manufacturer.clone(),
        chip_type: chip_type_str,
        pin_count: dev.package_details.pin_count,
        package_type: package_type_name(&dev.package_details),
        voltages: VoltagesDto::from_voltages(&dev.voltages, model, dev.chip_type, dev.flags.custom_protocol),
        code_memory_size: dev.code_memory_size,
        data_memory_size: dev.data_memory_size,
        can_erase: dev.flags.can_erase,
        has_chip_id: dev.flags.has_chip_id,
        config,
        invert_fuse_bits,
        config_name,
        pin_map: dev.pin_map,
        off_protect_before: dev.flags.off_protect_before,
        protect_after: dev.flags.protect_after,
    }
}

fn package_type_name(pkg: &PackageDetails) -> String {
    if pkg.plcc {
        format!("PLCC{}", pkg.pin_count)
    } else {
        format!("DIP{}", pkg.pin_count)
    }
}
