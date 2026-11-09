// Tauri v2 auto-converts top-level invoke keys to camelCase before matching
// to Rust parameter names, so all #[tauri::command] params must use
// camelCase (e.g. icspMode, not icsp_mode). This triggers Rust's
// non_snake_case lint, which we suppress file-wide rather than per-function.
#![allow(non_snake_case)]

use std::path::Path;
use std::sync::Arc;

use minipro_core::{
    batch::{patch_serial, SerialChecksum, SerialConfig, SerialEndian, SerialFormat},
    database::{find_device, find_device_any, get_pin_map, list_devices_by_model, DatabasePaths},
    device::{ChipType, Device, PackageDetails, ProgrammerModel, Voltages},
    operations::{
        blank_check, check_chip_id, chip_id_info, erase_chip, firmware_update, hardware_check,
        health_check, logic_auto_find, logic_ic_test, pin_check_gate_applies, pin_contact_check,
        read_chip, read_chip_stable, read_file, spi_autodetect_and_lookup, verify_chip,
        verify_chip_bytes, write_chip, write_chip_bytes, write_file, OpStats, SizeMismatch,
    },
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
        "unknown error",  // nusb generic error when device is gone
        "timed out",      // our USB transfer timeout
        "cannot open it", // open_programmer error when device can't be opened
        "cannot claim",   // interface claim failure
    ];
    if usb_errors.iter().any(|&keyword| err.contains(keyword)) {
        state.clear_programmer();
        log::warn!(
            "USB error detected, clearing cached programmer state: {}",
            err
        );
    }
}

/// Emit a log message to the frontend terminal.
fn emit_log(window: &Window, level: &str, message: &str) {
    let _ = window.emit(
        "app-log",
        serde_json::json!({
            "level": level,
            "message": message,
        }),
    );
}

/// Emit pin-test results to the frontend for ZIF diagram highlighting.
fn emit_pin_test_result(window: &Window, dto: &PinTestResultDto) {
    let _ = window.emit(
        "pin-test-result",
        serde_json::json!({
            "supported": dto.supported,
            "pass": dto.pass,
            "bad_pins": dto.bad_pins,
            "expected_pins": dto.expected_pins,
            "untestable_pins": dto.untestable_pins,
            "message": dto.message,
        }),
    );
}

/// Run a pin contact check before an operation if enabled.
///
/// Silently returns `Ok(())` when:
/// - `pin_check` is false (user unchecked the box)
/// - Programmer model is not TL866II+ — the only model with an upstream
///   pin-test implementation. The T48's own 0x3E pin-detect command is a
///   documented but never-implemented protocol.
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
    let device = handle.device().map_err(|e| e.to_string())?.clone();
    if !pin_check_gate_applies(handle.info.model, icsp_mode != "zif", &device) {
        return Ok(());
    }

    emit_log(window, "info", "Running pin contact check...");
    let result = pin_contact_check(handle, infoic_path).map_err(|e| e.to_string())?;
    let pass = result.bad_pins.is_empty();
    let count = result.bad_pins.len();
    let all_open = result.all_expected_open();
    let dto = PinTestResultDto {
        supported: true,
        pass,
        bad_pins: result.bad_pins.clone(),
        expected_pins: result.expected_pins.clone(),
        untestable_pins: result.untestable_pins.clone(),
        message: if pass {
            "All pins OK".into()
        } else {
            format!("Bad contact on {} pin(s)", count)
        },
    };

    // Emit the result on pass AND fail — the diagram colors confirm the
    // check ran and which pins were measured (TUI + standalone button
    // already behave this way).
    emit_pin_test_result(window, &dto);
    if pass {
        emit_log(window, "info", "Pin contact check passed");
    } else {
        let pin_list = result
            .bad_pins
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        // Every expected pin open almost always means no chip is seated.
        let detail = if all_open {
            format!("all {count} expected pins open — is a chip seated in the socket?")
        } else {
            format!("bad contact on pin(s) {pin_list}")
        };
        emit_log(
            window,
            "warn",
            &format!("Pin contact check failed: {detail}. Operation aborted."),
        );
        // Detail was already logged above; return the abort marker so
        // runOp reports "X aborted (pin contact check)" instead of
        // re-logging the full reason as an op failure.
        return Err("aborted (pin contact check)".to_string());
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
    simulated: bool,
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
    Mcu {
        fuses: Vec<FuseFieldDto>,
        locks: Vec<FuseFieldDto>,
    },
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
    /// ICSP wiring-class index from `package_details` bits 8–15.
    /// 0 = no ICSP. Nonzero selects a wiring diagram (see `icsp::icsp_wiring`).
    icsp: u8,
    /// `flags.prog_support == MP_ICSP_ONLY` — the part can only be
    /// programmed on the ICSP header, never in the ZIF socket.
    icsp_only: bool,
}

#[derive(Serialize)]
pub struct IcspWireDto {
    header_pin: u8,
    signal: &'static str,
    chip_pin: u8,
}

#[derive(Serialize)]
pub struct IcspWiringDto {
    title: &'static str,
    /// False = generic MCU target; labels are signal names, not pin numbers.
    numbered: bool,
    chip_labels: Vec<&'static str>,
    wires: Vec<IcspWireDto>,
    notes: Vec<&'static str>,
}

impl From<&'static minipro_core::icsp::IcspWiring> for IcspWiringDto {
    fn from(w: &'static minipro_core::icsp::IcspWiring) -> Self {
        Self {
            title: w.title,
            numbered: w.numbered,
            chip_labels: w.chip_labels.to_vec(),
            wires: w
                .wires
                .iter()
                .map(|x| IcspWireDto {
                    header_pin: x.header_pin,
                    signal: x.signal,
                    chip_pin: x.chip_pin,
                })
                .collect(),
            notes: w.notes.to_vec(),
        }
    }
}

/// Look up the verified ICSP wiring for a (programmer model, class) pair.
/// Returns null when no verified table exists.
#[tauri::command]
pub async fn get_icsp_wiring(
    model: String,
    icspClass: u8,
) -> Result<Option<IcspWiringDto>, String> {
    let model: ProgrammerModel = model.parse().map_err(|e: String| e)?;
    Ok(minipro_core::icsp::icsp_wiring(model, icspClass).map(IcspWiringDto::from))
}

/// ZIF socket layout for the connected programmer, resolved for a DIP
/// chip of `pinCount` pins.
///
/// `zifPins[i]` is the ZIF socket pin occupied by device pin `i+1`
/// (0 = unmapped).  All device↔ZIF mapping rules live in
/// `minipro_core::zif`; the frontend consumes this verbatim.
#[derive(Serialize)]
pub struct ZifLayoutDto {
    pins: usize,
    lever_top: bool,
    insertion: &'static str,
    zif_pins: Vec<usize>,
}

#[tauri::command]
pub async fn get_zif_layout(
    pinCount: usize,
    state: State<'_, Arc<AppState>>,
) -> Result<ZifLayoutDto, String> {
    let model = {
        let guard = state.programmer_info.lock().map_err(|e| e.to_string())?;
        guard.as_ref().map(|info| info.model)
    };
    // No programmer connected: widest sensible default, same as the
    // component's pre-core fallback (48-pin, top lever, top insertion).
    let spec = match model {
        Some(m) => minipro_core::zif::zif_spec(m),
        None => minipro_core::zif::ZifSpec {
            pins: 48,
            lever_top: true,
            insertion: minipro_core::zif::ZifInsertion::Top,
        },
    };
    let zif_pins = (1..=pinCount)
        .map(|d| minipro_core::zif::device_to_zif(&spec, d, pinCount).unwrap_or(0))
        .collect();
    Ok(ZifLayoutDto {
        pins: spec.pins,
        lever_top: spec.lever_top,
        insertion: match spec.insertion {
            minipro_core::zif::ZifInsertion::Top => "top",
            minipro_core::zif::ZifInsertion::Bottom => "bottom",
        },
        zif_pins,
    })
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
    fn from_voltages(
        v: &Voltages,
        model: Option<ProgrammerModel>,
        chip_type: u32,
        custom_protocol: bool,
    ) -> Self {
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
            vpp: if is_logic {
                "—".to_string()
            } else {
                lookup(v.vpp, vpp_table)
            },
            vdd: if is_logic {
                "—".to_string()
            } else {
                lookup(v.vdd, vcc_table)
            },
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
    /// Read the chip several times and compare passes (read path only).
    #[serde(default)]
    pub stability_check: bool,
}

fn default_icsp_mode() -> String {
    "zif".into()
}

/// Set ICSP mode from the GUI's mode string — thin wrapper over
/// `MiniproHandle::set_icsp_mode` (which owns the MP_ICSP_ONLY /
/// MP_ZIF_ONLY auto-activation rules).
fn set_icsp_from_mode(
    handle: &mut minipro_core::handle::MiniproHandle,
    mode: &str,
    device: &minipro_core::device::Device,
) {
    handle.set_icsp_mode(mode, device);
}
fn default_page() -> String {
    "code".into()
}
fn default_format() -> String {
    "auto".into()
}
fn default_size_mismatch() -> String {
    "error".into()
}
fn default_true() -> bool {
    true
}

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
    let vcc_table = minipro_core::device::vcc_voltage_table(
        model,
        device.chip_type,
        device.flags.custom_protocol,
    );
    let vpp_table = minipro_core::device::vpp_voltage_table(
        model,
        device.chip_type,
        device.flags.custom_protocol,
    );

    let valid_list = |table: Option<&[(&str, u8)]>| -> String {
        match table {
            Some(t) => t.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", "),
            None => "(no voltage overrides supported for this device)".to_string(),
        }
    };

    if let Some(ref v) = options.vpp {
        let table = vpp_table.ok_or_else(|| {
            format!(
                "VPP override not supported for this device; valid values: {}",
                valid_list(vpp_table)
            )
        })?;
        let code = minipro_core::device::lookup_voltage(table, v).ok_or_else(|| {
            format!(
                "invalid vpp voltage '{v}'; valid values: {}",
                valid_list(Some(table))
            )
        })?;
        device.voltages.vpp = code;
    }
    if let Some(ref v) = options.vdd {
        let table = vcc_table.ok_or_else(|| {
            format!(
                "VDD override not supported for this device; valid values: {}",
                valid_list(vcc_table)
            )
        })?;
        let code = minipro_core::device::lookup_voltage(table, v).ok_or_else(|| {
            format!(
                "invalid vdd voltage '{v}'; valid values: {}",
                valid_list(Some(table))
            )
        })?;
        device.voltages.vdd = code;
    }
    if let Some(ref v) = options.vcc {
        let table = vcc_table.ok_or_else(|| {
            format!(
                "VCC override not supported for this device; valid values: {}",
                valid_list(vcc_table)
            )
        })?;
        let code = minipro_core::device::lookup_voltage(table, v).ok_or_else(|| {
            format!(
                "invalid vcc voltage '{v}'; valid values: {}",
                valid_list(Some(table))
            )
        })?;
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
pub async fn get_voltage_options(
    state: State<'_, Arc<AppState>>,
) -> Result<VoltageOptionsDto, String> {
    let device = state.get_device();
    let model = {
        let guard = state.programmer_info.lock().map_err(|e| e.to_string())?;
        guard.as_ref().map(|info| info.model)
    };

    let device = match device {
        Ok(dev) => dev,
        Err(_) => {
            return Ok(VoltageOptionsDto {
                vcc: None,
                vpp: None,
                is_logic: false,
            })
        }
    };

    let model = model.unwrap_or(ProgrammerModel::Tl866iiPlus);
    let chip_type = device.chip_type;
    let custom_protocol = device.flags.custom_protocol;
    let is_logic = chip_type == ChipType::Logic as u32;

    let names = |table: Option<&'static [(&'static str, u8)]>| -> Option<Vec<String>> {
        table.map(|t| t.iter().map(|(n, _)| n.to_string()).collect())
    };

    let vcc = names(minipro_core::device::vcc_voltage_table(
        model,
        chip_type,
        custom_protocol,
    ));
    let vpp = names(minipro_core::device::vpp_voltage_table(
        model,
        chip_type,
        custom_protocol,
    ));

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

/// If MINIPRO_SIMULATE_PROGRAMMER is set (e.g. "T48"), install a synthetic
/// programmer of that model so the GUI can be exercised without hardware.
/// No USB handle is opened — operations that need the device will fail.
fn simulated_programmer_from_env(state: &AppState) -> Result<Option<ProgrammerInfoDto>, String> {
    let Some(info) =
        minipro_core::device::ProgrammerInfo::simulated_from_env().map_err(|e| e.to_string())?
    else {
        return Ok(None);
    };
    let model = info.model;
    {
        let mut guard = state.programmer_info.lock().map_err(|e| e.to_string())?;
        *guard = Some(info);
    }
    let _ = state.reload_device_names_for_model(model);
    log::warn!("MINIPRO_SIMULATE_PROGRAMMER={model}: no hardware connected, model simulated");
    Ok(Some(ProgrammerInfoDto {
        model: model.to_string(),
        firmware: "sim".to_string(),
        serial_number: "SIM".to_string(),
        hardware_version: "00".to_string(),
        simulated: true,
    }))
}

/// Turn simulated-programmer mode on/off at runtime (Settings → Simulation).
/// Sets or clears MINIPRO_SIMULATE_PROGRAMMER for this process, drops any
/// cached info/open handle, and re-resolves — simulation wins over real
/// hardware while enabled. `model: null` turns simulation off.
#[tauri::command]
pub async fn set_simulated_programmer(
    state: State<'_, Arc<AppState>>,
    model: Option<String>,
) -> Result<ProgrammerInfoDto, String> {
    match model.as_deref().map(str::trim).filter(|m| !m.is_empty()) {
        Some(m) => {
            m.parse::<ProgrammerModel>()
                .map_err(|_| format!("unknown programmer model '{m}'"))?;
            std::env::set_var("MINIPRO_SIMULATE_PROGRAMMER", m);
        }
        None => {
            std::env::remove_var("MINIPRO_SIMULATE_PROGRAMMER");
            std::env::remove_var("MINIPRO_FAKE_PROGRAMMER");
        }
    }
    {
        let mut guard = state.handle.lock().map_err(|e| e.to_string())?;
        *guard = None;
    }
    {
        let mut guard = state.programmer_info.lock().map_err(|e| e.to_string())?;
        *guard = None;
    }
    get_programmer_info(state).await
}

/// Open the programmer and return its info.
///
/// Retries a few times at startup because Windows USB enumeration can lag
/// behind the physical plug event by several seconds.
#[tauri::command]
pub async fn get_programmer_info(
    state: State<'_, Arc<AppState>>,
) -> Result<ProgrammerInfoDto, String> {
    if let Some(dto) = simulated_programmer_from_env(&state)? {
        return Ok(dto);
    }
    {
        let guard = state.programmer_info.lock().map_err(|e| e.to_string())?;
        if let Some(ref info) = *guard {
            return Ok(ProgrammerInfoDto {
                model: info.model.to_string(),
                firmware: info.firmware_str.clone(),
                serial_number: info.serial_number.clone(),
                hardware_version: format!("{:02x}", info.hardware_version),
                simulated: false,
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
        )
        .await;

        let (info, handle) = match result {
            Ok(Ok(Ok(v))) => v,
            Ok(Ok(Err(e))) => {
                last_err = e;
                eprintln!(
                    "get_programmer_info attempt {} failed: {}",
                    attempt + 1,
                    last_err
                );
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

        // Reload device list filtered by this programmer's model so that
        // only chips supported by the connected hardware are shown.
        let _ = state.reload_device_names_for_model(info.model);

        return Ok(ProgrammerInfoDto {
            model: info.model.to_string(),
            firmware: info.firmware_str,
            serial_number: info.serial_number,
            hardware_version: format!("{:02x}", info.hardware_version),
            simulated: false,
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

    if let Some(dto) = simulated_programmer_from_env(&state)? {
        return Ok(dto);
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
        )
        .await;

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
                // Reload device list filtered by this programmer's model.
                let _ = state.reload_device_names_for_model(info.model);
                return Ok(ProgrammerInfoDto {
                    model: info.model.to_string(),
                    firmware: info.firmware_str,
                    serial_number: info.serial_number,
                    hardware_version: format!("{:02x}", info.hardware_version),
                    simulated: false,
                });
            }
            Ok(Ok(Err(e))) => {
                last_err = e;
                eprintln!(
                    "force_reconnect attempt {} failed: {}",
                    attempt + 1,
                    last_err
                );
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
pub async fn search_devices(
    query: String,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<DeviceSearchResultDto>, String> {
    let filter = query.trim().to_ascii_lowercase();
    if filter.is_empty() {
        return Ok(vec![]);
    }
    // Use pre-loaded device names (loaded once at startup) for instant search
    let items = state.search_device_names(&filter)?;
    Ok(items
        .into_iter()
        .map(|item| DeviceSearchResultDto {
            name: item.name,
            manufacturer: item.manufacturer,
        })
        .collect())
}

/// Availability of a favorited device name for the connected programmer.
#[derive(Serialize)]
pub struct FavoriteStatusDto {
    /// The stored favorite name.
    name: String,
    /// True when the name resolves for the connected programmer (or for any
    /// programmer when none is connected).
    available: bool,
}

/// Check favorite names against every programmer model's device list.
///
/// Device names are not portable across database sections (e.g. the TL866A
/// section has bare "PIC16F628A" while T76 only has "PIC16F628A@DIP18"), so
/// a favorite saved under one model may not resolve under another.  The GUI
/// hides such favorites while that model is connected.
#[tauri::command]
pub async fn check_favorite_devices(
    names: Vec<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<FavoriteStatusDto>, String> {
    if names.is_empty() {
        return Ok(vec![]);
    }
    let db = get_db_paths(&state)?;
    let model = {
        let guard = state.programmer_info.lock().map_err(|e| e.to_string())?;
        guard.as_ref().map(|info| info.model)
    };

    tokio::task::spawn_blocking(move || {
        const ALL_MODELS: [ProgrammerModel; 6] = [
            ProgrammerModel::Tl866a,
            ProgrammerModel::Tl866cs,
            ProgrammerModel::Tl866iiPlus,
            ProgrammerModel::T48,
            ProgrammerModel::T56,
            ProgrammerModel::T76,
        ];
        let by_model = list_devices_by_model(&db).map_err(|e| e.to_string())?;
        let sets: std::collections::HashMap<ProgrammerModel, std::collections::HashSet<String>> =
            by_model
                .iter()
                .map(|(m, items)| (*m, items.iter().map(|i| i.name.to_lowercase()).collect()))
                .collect();

        let out = names
            .iter()
            .map(|name| {
                let lc = name.to_lowercase();
                let available = match model {
                    Some(m) => sets[&m].contains(&lc),
                    // With no programmer connected, selection resolves via
                    // find_device_any — the name is usable if it exists in
                    // any model's section.
                    None => ALL_MODELS.iter().any(|m| sets[m].contains(&lc)),
                };
                FavoriteStatusDto {
                    name: name.clone(),
                    available,
                }
            })
            .collect();
        Ok::<Vec<FavoriteStatusDto>, String>(out)
    })
    .await
    .map_err(|e| format!("Task panicked: {}", e))?
}

/// Get detailed info for a single device (no programmer required).
#[tauri::command]
pub async fn get_device_info(
    name: String,
    state: State<'_, Arc<AppState>>,
) -> Result<DeviceInfoDto, String> {
    let db = get_db_paths(&state)?;
    let name_clone = name.clone();
    let model = {
        let guard = state.programmer_info.lock().map_err(|e| e.to_string())?;
        guard.as_ref().map(|info| info.model)
    };

    tokio::task::spawn_blocking(move || {
        let dev = if let Some(m) = model {
            find_device(&db, &name_clone, m).map_err(|e| e.to_string())?
        } else {
            find_device_any(&db, &name_clone).map_err(|e| e.to_string())?
        };
        Ok::<DeviceInfoDto, String>(device_to_dto(&dev, model))
    })
    .await
    .map_err(|e| format!("Task panicked: {}", e))?
}

/// Get the pin-contact map for a device (ZIF pin numbers that must make contact).
/// Returns None when pin_map index is 0 (no contact-test data).
#[tauri::command]
pub async fn get_device_pin_map(
    pinMap: u32,
    state: State<'_, Arc<AppState>>,
) -> Result<Option<PinMapDto>, String> {
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
pub async fn select_device(
    name: String,
    state: State<'_, Arc<AppState>>,
) -> Result<DeviceInfoDto, String> {
    let db = get_db_paths(&state)?;

    let model = {
        let guard = state.programmer_info.lock().map_err(|e| e.to_string())?;
        guard.as_ref().map(|info| info.model)
    };

    let name_clone = name.clone();
    let (dto, device) = tokio::task::spawn_blocking(move || {
        let dev = if let Some(m) = model {
            find_device(&db, &name_clone, m).map_err(|e| e.to_string())?
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

/// Read passes for the stability-check read option.
const READ_STABILITY_PASSES: usize = 5;

/// Shared stability-checked read for `do_read` / `read_chip_to_bytes`:
/// runs `read_chip_stable` with progress events and logs the outcome.
/// Returns the effective image — the consistent buffer when all passes
/// agree, or the majority-vote consensus when they disagree.
fn read_chip_stable_logged(
    handle: &mut MiniproHandle,
    page: u8,
    window: &Window,
    op_name: &str,
) -> Result<(Vec<u8>, OpStats), String> {
    emit_log(
        window,
        "info",
        &format!("Read stability check: {} passes", READ_STABILITY_PASSES),
    );
    let win = window.clone();
    let op = op_name.to_string();
    let (data, report) = read_chip_stable(
        handle,
        page,
        READ_STABILITY_PASSES,
        Some(&mut |done, total| {
            let _ = win.emit(
                "progress",
                ProgressPayload {
                    done,
                    total,
                    operation: op.clone(),
                },
            );
        }),
    )
    .map_err(|e| e.to_string())?;
    for note in &report.notes {
        emit_log(window, "info", note);
    }
    for err in &report.pass_errors {
        emit_log(window, "warn", err);
    }
    if report.unstable_bytes > 0 {
        emit_log(
            window,
            "warn",
            &format!(
                "Read instability: {} byte position(s) disagreed across {}/{} passes — using majority-vote consensus",
                report.unstable_bytes, report.read_passes_completed, report.read_passes_requested
            ),
        );
    } else {
        emit_log(
            window,
            "info",
            &format!(
                "Read stable: {}/{} passes byte-identical",
                report.read_passes_completed, report.read_passes_requested
            ),
        );
    }
    let stats = OpStats::from_buffer(&data);
    Ok((data, stats))
}

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
                        return Err("aborted (chip ID check)".to_string());
                    }
                }
            }

            let stats = if options_clone.stability_check {
                let (data, stats) =
                    read_chip_stable_logged(&mut handle, page, &window_clone, &op_name)?;
                write_file(
                    Path::new(&path_clone),
                    &options_clone.format,
                    &data,
                    Some(&device.name),
                )
                .map_err(|e| e.to_string())?;
                stats
            } else {
                read_chip(
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
                .map_err(|e| e.to_string())?
            };

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

            if options_clone.check_device_id {
                match check_chip_id(&mut handle) {
                    Ok(()) => {
                        emit_log(&window_clone, "info", "Chip ID check passed");
                    }
                    Err(e) => {
                        emit_log(&window_clone, "error", &format!("Chip ID check failed: {}", e));
                        return Err("aborted (chip ID check)".to_string());
                    }
                }
            }

            let (base64, stats) = if options_clone.stability_check {
                let (data, stats) =
                    read_chip_stable_logged(&mut handle, page, &window_clone, &op_name)?;
                (
                    base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &data),
                    stats,
                )
            } else {
                let stats = read_chip(
                    &mut handle,
                    &temp_path,
                    page,
                    "bin", // always read raw binary for the hex viewer
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

                (
                    base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes),
                    stats,
                )
            };
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
        let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &base64Data)
            .map_err(|e| format!("Failed to decode base64: {}", e))?;

        std::fs::write(&path, &bytes).map_err(|e| format!("Failed to write file: {}", e))
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
        let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &base64Data)
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

fn open_dir_in_manager(dir: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(dir)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(dir)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(dir)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }
    Ok(())
}

/// Open the folder containing the given file path in the system file
/// manager. Directory paths are opened directly.
#[tauri::command]
pub fn open_folder(path: String) -> Result<(), String> {
    let path = std::path::Path::new(&path);
    let dir = if path.is_dir() {
        path
    } else {
        path.parent().ok_or("Path has no parent directory")?
    };
    open_dir_in_manager(dir)
}

/// Open the directory `download_algorithms` writes to: `dir` when given
/// (the configured custom database directory), otherwise the per-user
/// data directory. Creates it when missing so the button works before
/// any download has run.
#[tauri::command]
pub fn open_download_dir(dir: Option<String>) -> Result<(), String> {
    let dir = match dir {
        Some(d) => std::path::PathBuf::from(d),
        None => minipro_core::user_data_dir()
            .ok_or("Could not determine the per-user data directory")?,
    };
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create {}: {}", dir.display(), e))?;
    open_dir_in_manager(&dir)
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
        let guard = state_clone
            .programmer_info
            .lock()
            .map_err(|e| e.to_string())?;
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
            handle
                .begin_transaction(device.clone())
                .map_err(|e| e.to_string())?;

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
                        emit_log(
                            &window_clone,
                            "error",
                            &format!("Chip ID check failed: {}", e),
                        );
                        return Err("aborted (chip ID check)".to_string());
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
                handle
                    .protocol
                    .protect_off(&handle.usb)
                    .map_err(|e| e.to_string())?;
                emit_log(&window_clone, "info", "Protect off...OK");
                if is_t76 {
                    handle.end_transaction().map_err(|e| e.to_string())?;
                    handle
                        .begin_transaction(device.clone())
                        .map_err(|e| e.to_string())?;
                }
            }

            if !options_clone.skip_erase {
                erase_chip(&mut handle).map_err(|e| e.to_string())?;
                handle.end_transaction().map_err(|e| e.to_string())?;
                handle
                    .begin_transaction(device.clone())
                    .map_err(|e| e.to_string())?;
            }

            let stats = write_chip(
                &mut handle,
                Path::new(&path_clone),
                page,
                &options_clone.format,
                size_mismatch,
                options_clone.skip_blank,
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
                handle
                    .protocol
                    .protect_on(&handle.usb)
                    .map_err(|e| e.to_string())?;
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
        let guard = state_clone
            .programmer_info
            .lock()
            .map_err(|e| e.to_string())?;
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
            handle
                .begin_transaction(device.clone())
                .map_err(|e| e.to_string())?;

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
                        emit_log(
                            &window_clone,
                            "error",
                            &format!("Chip ID check failed: {}", e),
                        );
                        return Err("aborted (chip ID check)".to_string());
                    }
                }
            }

            // ── Protect off (before erase/write) ──────────────────────────────
            let off_protect = device.flags.off_protect_before;
            let is_t76 = handle.info.model == minipro_core::device::ProgrammerModel::T76;
            if off_protect && (is_t76 || options_clone.unprotect_before) {
                emit_log(
                    &window_clone,
                    "info",
                    &format!("Chip {}: protect off...", chipNumber),
                );
                handle
                    .protocol
                    .protect_off(&handle.usb)
                    .map_err(|e| e.to_string())?;
                emit_log(
                    &window_clone,
                    "info",
                    &format!("Chip {}: protect off...OK", chipNumber),
                );
                if is_t76 {
                    handle.end_transaction().map_err(|e| e.to_string())?;
                    handle
                        .begin_transaction(device.clone())
                        .map_err(|e| e.to_string())?;
                }
            }

            if !options_clone.skip_erase {
                emit_log(
                    &window_clone,
                    "info",
                    &format!("Chip {}: erasing...", chipNumber),
                );
                erase_chip(&mut handle).map_err(|e| e.to_string())?;
                handle.end_transaction().map_err(|e| e.to_string())?;
                handle
                    .begin_transaction(device.clone())
                    .map_err(|e| e.to_string())?;
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
                    &format!(
                        "Chip {}: serial = 0x{:0>width$X}",
                        chipNumber,
                        serial_value,
                        width = sc.width * 2
                    ),
                );

                let write_window = window_clone.clone();
                let stats = write_chip_bytes(
                    &mut handle,
                    buf.clone(),
                    page,
                    size_mismatch,
                    options_clone.skip_blank,
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
                    emit_log(
                        &window_clone,
                        "info",
                        &format!("Chip {}: protect on...", chipNumber),
                    );
                    handle
                        .protocol
                        .protect_on(&handle.usb)
                        .map_err(|e| e.to_string())?;
                    emit_log(
                        &window_clone,
                        "info",
                        &format!("Chip {}: protect on...OK", chipNumber),
                    );
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
                emit_log(
                    &window_clone,
                    "info",
                    &format!("Chip {}: protect on...", chipNumber),
                );
                handle
                    .protocol
                    .protect_on(&handle.usb)
                    .map_err(|e| e.to_string())?;
                emit_log(
                    &window_clone,
                    "info",
                    &format!("Chip {}: protect on...OK", chipNumber),
                );
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
        let guard = state_clone
            .programmer_info
            .lock()
            .map_err(|e| e.to_string())?;
        guard.as_ref().map(|info| info.model)
    };

    let result = tokio::task::spawn_blocking(move || {
        let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &base64Data)
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
            handle
                .begin_transaction(device.clone())
                .map_err(|e| e.to_string())?;

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
                        emit_log(
                            &window_clone,
                            "error",
                            &format!("Chip ID check failed: {}", e),
                        );
                        return Err("aborted (chip ID check)".to_string());
                    }
                }
            }

            // ── Protect off (before erase/write) ──────────────────────────────
            let off_protect = device.flags.off_protect_before;
            let is_t76 = handle.info.model == minipro_core::device::ProgrammerModel::T76;
            if off_protect && (is_t76 || options_clone.unprotect_before) {
                emit_log(&window_clone, "info", "Protect off...");
                handle
                    .protocol
                    .protect_off(&handle.usb)
                    .map_err(|e| e.to_string())?;
                emit_log(&window_clone, "info", "Protect off...OK");
                if is_t76 {
                    handle.end_transaction().map_err(|e| e.to_string())?;
                    handle
                        .begin_transaction(device.clone())
                        .map_err(|e| e.to_string())?;
                }
            }

            if !options_clone.skip_erase {
                erase_chip(&mut handle).map_err(|e| e.to_string())?;
                handle.end_transaction().map_err(|e| e.to_string())?;
                handle
                    .begin_transaction(device.clone())
                    .map_err(|e| e.to_string())?;
            }

            let verify_bytes = bytes.clone();
            let stats = write_chip_bytes(
                &mut handle,
                bytes,
                page,
                size_mismatch,
                options_clone.skip_blank,
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
                handle
                    .protocol
                    .protect_on(&handle.usb)
                    .map_err(|e| e.to_string())?;
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
            handle
                .begin_transaction(device)
                .map_err(|e| e.to_string())?;

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
                        emit_log(
                            &window_clone,
                            "error",
                            &format!("Chip ID check failed: {}", e),
                        );
                        return Err("aborted (chip ID check)".to_string());
                    }
                }
            }

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
pub async fn do_erase(
    icspMode: String,
    checkDeviceId: bool,
    pinCheck: bool,
    window: Window,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
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
            handle
                .begin_transaction(device)
                .map_err(|e| e.to_string())?;

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
                        emit_log(
                            &window_clone,
                            "error",
                            &format!("Chip ID check failed: {}", e),
                        );
                        return Err("aborted (chip ID check)".to_string());
                    }
                }
            }

            erase_chip(&mut handle).map_err(|e| e.to_string())?;
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
pub async fn do_blank_check(
    icspMode: String,
    pinCheck: bool,
    window: Window,
    state: State<'_, Arc<AppState>>,
) -> Result<BlankCheckResultDto, String> {
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
            handle
                .begin_transaction(device)
                .map_err(|e| e.to_string())?;

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
            Ok::<BlankCheckResultDto, String>(BlankCheckResultDto {
                is_blank: true,
                address: 0,
            })
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
                    return Ok(BlankCheckResultDto {
                        is_blank: false,
                        address: addr,
                    });
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
}

/// Read the chip ID.
#[tauri::command]
pub async fn do_chip_id(
    icspMode: String,
    pinCheck: bool,
    window: Window,
    state: State<'_, Arc<AppState>>,
) -> Result<ChipIdResultDto, String> {
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
            handle
                .begin_transaction(device.clone())
                .map_err(|e| e.to_string())?;

            // Pin contact check (pre-operation gate)
            let db_paths = get_db_paths(&state_task)?;
            run_pin_check_if_enabled(
                &mut handle,
                pinCheck,
                &icspMode,
                &window_clone,
                &db_paths.infoic,
            )?;

            // chip_id_info owns the masking width, normalized comparison,
            // and package-variant rules (the frontend decides how to
            // present a variant mismatch — hard warning vs. info note).
            let info = chip_id_info(&mut handle).map_err(|e| e.to_string())?;
            let mask = info.mask();
            let width = (info.bytes * 2) as usize;
            Ok::<ChipIdResultDto, String>(ChipIdResultDto {
                id: format!("0x{:0width$x}", info.id & mask, width = width),
                expected: format!("0x{:0width$x}", info.expected & mask, width = width),
                is_match: info.is_match,
                is_variant: info.is_variant,
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

/// A single instability region (4 KiB bucket) from the health check.
#[derive(Serialize)]
pub struct HealthRegionDto {
    pub start: usize,
    pub len: usize,
    pub unstable: usize,
}

/// Structured health-check result for the GUI.
#[derive(Serialize)]
pub struct HealthCheckResultDto {
    pub verdict: String,
    pub deviceName: String,
    pub size: usize,
    pub passesRequested: usize,
    pub passesCompleted: usize,
    pub idRunsCompleted: usize,
    pub uniqueChipIds: Vec<String>,
    pub unstableBytes: usize,
    pub flipsUp: u64,
    pub flipsDown: u64,
    pub periodicity: Option<u32>,
    pub regions: Vec<HealthRegionDto>,
    pub passErrors: Vec<String>,
    pub chipIdErrors: Vec<String>,
    pub notes: Vec<String>,
    /// Mismatched byte positions vs the reference image (None when no
    /// reference file was supplied).
    pub referenceMismatches: Option<usize>,
    /// Majority-vote reconstruction (present only when instability was found).
    pub consensusB64: Option<String>,
}

/// Read-only chip health check: repeat chip-ID and full-memory reads, then
/// report how the results disagree. Intended to flag marginal/degrading
/// chips (drifting IDs, bit flips) without touching flash contents.
#[tauri::command]
pub async fn run_health_check(
    icspMode: String,
    pinCheck: bool,
    passes: u32,
    referencePath: Option<String>,
    window: Window,
    state: State<'_, Arc<AppState>>,
) -> Result<HealthCheckResultDto, String> {
    let state_clone = (*state).clone();
    if !state_clone.try_acquire() {
        return Err("Another operation is already running".into());
    }

    let state_task = state_clone.clone();
    let window_clone = window.clone();
    let result = tokio::task::spawn_blocking(move || {
        let mut handle = state_task.take_handle()?;
        let device = state_task.get_device()?;
        let window_inner = window_clone.clone();

        let result = (|| {
            set_icsp_from_mode(&mut handle, &icspMode, &device);
            handle
                .begin_transaction(device.clone())
                .map_err(|e| e.to_string())?;

            // Pin contact check (pre-operation gate)
            let db_paths = get_db_paths(&state_task)?;
            run_pin_check_if_enabled(
                &mut handle,
                pinCheck,
                &icspMode,
                &window_clone,
                &db_paths.infoic,
            )?;

            let passes = (passes as usize).clamp(2, 64);
            let mut cb = |done: usize, total: usize| {
                let _ = window_inner.emit(
                    "progress",
                    ProgressPayload {
                        done,
                        total,
                        operation: "Health check".to_string(),
                    },
                );
            };
            let reference = match &referencePath {
                Some(p) => {
                    let size = handle.protocol.effective_code_size(&device) as usize;
                    Some(
                        read_file(Path::new(p), "auto", size, device.blank_value as u8)
                            .map_err(|e| format!("reference file: {}", e))?,
                    )
                }
                None => None,
            };
            const ID_RUNS: usize = 20;
            let report = health_check(
                &mut handle,
                passes,
                ID_RUNS,
                reference.as_deref(),
                Some(&mut cb),
            )
            .map_err(|e| e.to_string())?;

            use minipro_core::HealthVerdict as HV;
            let mut unique: Vec<u32> = report.chip_ids.clone();
            unique.sort_unstable();
            unique.dedup();
            let mut regions = report.regions;
            regions.sort_by_key(|r| std::cmp::Reverse(r.unstable));
            regions.truncate(20);
            let dto = HealthCheckResultDto {
                verdict: report.verdict.label().to_string(),
                deviceName: report.device_name,
                size: report.size,
                passesRequested: report.read_passes_requested,
                passesCompleted: report.read_passes_completed,
                idRunsCompleted: report.chip_ids.len(),
                uniqueChipIds: unique.iter().map(|id| format!("0x{:06x}", id)).collect(),
                unstableBytes: report.unstable_bytes,
                flipsUp: report.flips_up,
                flipsDown: report.flips_down,
                periodicity: report.periodicity,
                regions: regions
                    .into_iter()
                    .map(|r| HealthRegionDto {
                        start: r.start,
                        len: r.len,
                        unstable: r.unstable,
                    })
                    .collect(),
                passErrors: report.pass_errors.clone(),
                chipIdErrors: report.chip_id_errors.clone(),
                notes: report.notes.clone(),
                referenceMismatches: report.reference_mismatches,
                consensusB64: report
                    .consensus
                    .map(|c| base64::Engine::encode(&base64::engine::general_purpose::STANDARD, c)),
            };
            let level = match report.verdict {
                HV::Stable => "info",
                HV::TransportError => "error",
                _ => "warn",
            };
            emit_log(
                &window_inner,
                level,
                &format!(
                    "Health check — {}: {} pass(es) completed, {} unstable byte(s)",
                    report.verdict.label(),
                    report.read_passes_completed,
                    report.unstable_bytes
                ),
            );
            for n in &report.notes {
                emit_log(&window_inner, "info", &format!("  {}", n));
            }
            Ok::<HealthCheckResultDto, String>(dto)
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
pub async fn do_logic_test(
    icspMode: String,
    vcc: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<LogicTestResultDto, String> {
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
                        stability_check: false,
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

            handle
                .begin_transaction(device)
                .map_err(|e| e.to_string())?;
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
pub async fn do_spi_autodetect(
    idType: u8,
    window: Window,
    state: State<'_, Arc<AppState>>,
) -> Result<SpiAutodetectResultDto, String> {
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
            // Upstream runs this check on the TL866II+ only. The T48's own
            // 0x3E pin-detect is implemented, but the synthetic pin_map
            // indices below (0x01/0x03) are II+ conventions — leave the
            // autodetect pre-check II+-only.
            if matches!(handle.info.model, ProgrammerModel::Tl866iiPlus) && handle.icsp == 0 {
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
                        let pin_list = r
                            .bad_pins
                            .iter()
                            .map(|p| p.to_string())
                            .collect::<Vec<_>>()
                            .join(", ");
                        let detail = if r.all_expected_open() {
                            format!(
                                "all {count} expected pins open — is a chip seated in the socket?"
                            )
                        } else {
                            format!("bad contact on pin(s) {pin_list}")
                        };
                        let msg =
                            format!("Pin contact check failed: {detail}. Autodetect aborted.");
                        emit_log(&window_clone, "warn", &msg);
                        emit_pin_test_result(
                            &window_clone,
                            &PinTestResultDto {
                                supported: true,
                                pass: false,
                                bad_pins: r.bad_pins,
                                expected_pins: r.expected_pins,
                                untestable_pins: r.untestable_pins,
                                message: format!("Bad contact on {} pin(s)", count),
                            },
                        );
                        return Err(msg);
                    }
                    Err(e) => {
                        let msg = format!("Pin contact check error: {}. Autodetect aborted.", e);
                        emit_log(&window_clone, "warn", &msg);
                        return Err(msg);
                    }
                }
            }

            let autodetect =
                spi_autodetect_and_lookup(&mut handle, &db, idType).map_err(|e| e.to_string())?;
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
    let invalid = state
        .db_dir_invalid
        .load(std::sync::atomic::Ordering::SeqCst);
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
                return Err(format!("infoic.xml not found in '{}'", dir_path.display()));
            }
            if !logicic.exists() {
                state
                    .db_dir_invalid
                    .store(true, std::sync::atomic::Ordering::SeqCst);
                return Err(format!("logicic.xml not found in '{}'", dir_path.display()));
            }
            // algorithm.xml is optional — pass it as an override only if present
            let algorithms = dir_path.join("algorithm.xml");
            let algo_override = if algorithms.exists() {
                Some(algorithms.as_path())
            } else {
                None
            };
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
pub async fn get_programmer_details(
    state: State<'_, Arc<AppState>>,
) -> Result<ProgrammerDetailsDto, String> {
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
            let result = handle
                .protocol
                .get_ovc_status(&handle.usb, &device)
                .map_err(|e| e.to_string());
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
            let result = handle
                .protocol
                .read_calibration(&handle.usb, 4)
                .map_err(|e| e.to_string());
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
    value: u16,
}

#[derive(Serialize)]
pub struct ConfigDataDto {
    cfg_fuses: Vec<FuseValueDto>,
    lock_bits: Vec<FuseValueDto>,
    user_fuses: Vec<u8>,
    calibration: Vec<u8>,
    element_size: usize,
}

/// Read all fuse / lock / user / calibration data from the chip.
#[tauri::command]
pub async fn read_fuses(
    icspMode: String,
    pinCheck: bool,
    window: Window,
    state: State<'_, Arc<AppState>>,
) -> Result<ConfigDataDto, String> {
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
                handle
                    .begin_transaction(device)
                    .map_err(|e| e.to_string())?;

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
                let named =
                    minipro_core::operations::read_fuses(&mut handle).map_err(|e| e.to_string())?;

                let dev = handle.device().map_err(|e| e.to_string())?;
                let (fuse_len, element_size) =
                    if let Some(minipro_core::device::ChipConfig::Mcu(ref cfg)) = dev.config {
                        (
                            cfg.fuses.len(),
                            minipro_core::operations::fuse_element_size(&cfg.name),
                        )
                    } else {
                        (0, 1)
                    };

                // Read chip calibration bytes (OSCCAL word for PIC devices)
                let calibration = minipro_core::operations::read_chip_calibration(&mut handle)
                    .map_err(|e| e.to_string())?;

                Ok::<ConfigDataDto, String>(ConfigDataDto {
                    cfg_fuses: named
                        .iter()
                        .take(fuse_len)
                        .map(|v| FuseValueDto {
                            name: v.name.clone(),
                            value: v.value,
                        })
                        .collect(),
                    lock_bits: named
                        .iter()
                        .skip(fuse_len)
                        .map(|v| FuseValueDto {
                            name: v.name.clone(),
                            value: v.value,
                        })
                        .collect(),
                    user_fuses: vec![], // TODO: TL866A user fuse read hangs firmware
                    calibration,
                    element_size,
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
pub async fn write_fuses(
    cfgFuses: Vec<FuseValueDto>,
    lockBits: Vec<FuseValueDto>,
    icspMode: String,
    pinCheck: bool,
    window: Window,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
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
                handle
                    .begin_transaction(device.clone())
                    .map_err(|e| e.to_string())?;

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
                let mut all: Vec<minipro_core::operations::FuseValue> = cfgFuses
                    .iter()
                    .map(|d| minipro_core::operations::FuseValue {
                        name: d.name.clone(),
                        value: d.value,
                    })
                    .collect();
                all.extend(
                    lockBits
                        .iter()
                        .map(|d| minipro_core::operations::FuseValue {
                            name: d.name.clone(),
                            value: d.value,
                        }),
                );

                minipro_core::operations::write_fuses(&mut handle, &all)
                    .map_err(|e| e.to_string())?;

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
    lock_byte: u16,
    /// Raw READ_LOCK (0x15) reply bytes for chips with no declared lock
    /// element — e.g. SPI NOR status registers. Surfaced for diagnostics.
    raw_lock: Vec<u8>,
    /// Per-fuse-type probe results for chips with no declared lock element.
    probe_notes: Vec<String>,
}

/// Quick check whether the chip's lock bits indicate read/write protection.
#[tauri::command]
pub async fn check_lock_protection(
    icspMode: String,
    state: State<'_, Arc<AppState>>,
) -> Result<LockStatusDto, String> {
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
                handle
                    .begin_transaction(device)
                    .map_err(|e| e.to_string())?;

                let (lock_count, element_size) =
                    if let Some(minipro_core::device::ChipConfig::Mcu(ref cfg)) =
                        handle.device().map_err(|e| e.to_string())?.config
                    {
                        (
                            cfg.locks.len() as u8,
                            minipro_core::operations::fuse_element_size(&cfg.name),
                        )
                    } else {
                        (0, 1)
                    };

                let mut raw_lock: Vec<u8> = Vec::new();
                let mut probe_notes: Vec<String> = Vec::new();
                let lock_byte = if lock_count > 0 {
                    handle
                        .protocol
                        .read_fuses(
                            &handle.usb,
                            handle.device().map_err(|e| e.to_string())?,
                            minipro_core::operations::MP_FUSE_LOCK,
                            lock_count as usize * element_size,
                            element_size as u8,
                        )
                        .map(|b| minipro_core::operations::parse_fuse_element(&b, 0, element_size))
                        .unwrap_or(0xffff)
                } else {
                    // No declared lock element (SPI NOR etc.): probe every fuse
                    // type — the firmware may expose a status register under
                    // any of them (lock 0x02, config 0x01, user 0x00).
                    for ft in [0x02u8, 0x01, 0x00] {
                        match handle.protocol.read_fuses(
                            &handle.usb,
                            handle.device().map_err(|e| e.to_string())?,
                            ft,
                            8,
                            1,
                        ) {
                            Ok(b) if !b.is_empty() => {
                                if ft == minipro_core::operations::MP_FUSE_LOCK {
                                    raw_lock = b.clone();
                                }
                                probe_notes.push(format!(
                                    "fuse {ft:#04x}: {}",
                                    b.iter()
                                        .map(|x| format!("{x:02x}"))
                                        .collect::<Vec<_>>()
                                        .join(" ")
                                ));
                            }
                            Ok(_) => probe_notes.push(format!("fuse {ft:#04x}: empty")),
                            Err(e) => probe_notes.push(format!("fuse {ft:#04x}: {e}")),
                        }
                    }
                    0xffff
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
                let is_avr =
                    if let Some(minipro_core::device::ChipConfig::Mcu(ref cfg)) = dev.config {
                        cfg.name.starts_with("avr_")
                    } else {
                        false
                    };

                let is_protected = if lock_count == 0 {
                    // No declared lock element — the 0xffff sentinel is not protection.
                    false
                } else if is_avr {
                    // AVR: only LB bits (1:0) control external read/write.
                    // 11 = no protection, 10 = further programming disabled,
                    // 00 = programming and verification disabled.
                    (lock_byte & 0x03) != 0x03
                } else if element_size == 2 {
                    // PIC: 2-byte lock word — any non-erased value may indicate protection.
                    lock_byte != 0xffff
                } else {
                    // Other 1-byte: conservative default — any non-erased lock byte
                    // may indicate protection.
                    lock_byte != 0xff
                };

                Ok::<LockStatusDto, String>(LockStatusDto {
                    is_protected,
                    lock_byte,
                    raw_lock,
                    probe_notes,
                })
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
pub async fn run_hardware_check(
    state: State<'_, Arc<AppState>>,
) -> Result<HardwareCheckResultDto, String> {
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
    /// Device pin numbers (1-based) the mask actually tested — lets the
    /// diagram distinguish tested-good from untested (e.g. GND) pins.
    pub expected_pins: Vec<u16>,
    /// Device pins excluded by design (GND — held low by the die, so
    /// contact can't be distinguished electrically).
    pub untestable_pins: Vec<u16>,
    /// Human-readable status message.
    pub message: String,
}

/// Run a ZIF socket pin-contact test.
///
/// Requires a selected device with pin-map data in the database.
/// Supported on TL866II+ (upstream bit-bang) and T48 (`0x3E` contact
/// bitmap). Returns structured bad-pin data for diagram highlighting.
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

            // Check programmer model support (T76 included — its package
            // restrictions are enforced inside pin_test_t76)
            if !matches!(
                handle.info.model,
                ProgrammerModel::Tl866iiPlus | ProgrammerModel::T48 | ProgrammerModel::T76
            ) {
                let _ = state_task.store_handle(handle);
                return Ok(PinTestResultDto {
                    supported: false,
                    pass: false,
                    bad_pins: vec![],
                    expected_pins: vec![],
                    untestable_pins: vec![],
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
                    expected_pins: vec![],
                    untestable_pins: vec![],
                    message: "Pin test is only available in ZIF mode".into(),
                });
            }

            let device = state_task.get_device()?;
            set_icsp_from_mode(&mut handle, &icspMode, &device);

            let db_paths = get_db_paths(&state_task)?;
            let infoic_path = db_paths.infoic.clone();

            handle
                .begin_transaction(device)
                .map_err(|e| e.to_string())?;
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
                expected_pins: result.expected_pins,
                untestable_pins: result.untestable_pins,
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
        let handle = guard
            .as_mut()
            .ok_or_else(crate::state::no_programmer_error)?;
        let mut output = Vec::new();
        let result = firmware_update(
            handle,
            &fw_data,
            &mut output,
            Some(&mut |done, total| {
                let _ = window_clone.emit(
                    "progress",
                    ProgressPayload {
                        done,
                        total,
                        operation: "firmware_update".to_string(),
                    },
                );
            }),
        );
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
pub async fn read_file_bytes(
    path: String,
    target_size: Option<u32>,
    blank_value: Option<u8>,
) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let p = Path::new(&path);
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
        let is_text_format = matches!(ext.to_lowercase().as_str(), "hex" | "srec" | "mot" | "jed");

        let bytes = if is_text_format {
            let size = target_size.unwrap_or(65536) as usize;
            let blank = blank_value.unwrap_or(0xFF);
            let buf = read_file(p, "auto", size, blank)
                .map_err(|e| format!("Cannot parse file: {}", e))?;
            trim_trailing_blanks(buf, blank)
        } else {
            std::fs::read(p).map_err(|e| format!("Cannot read file: {}", e))?
        };

        Ok(base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            &bytes,
        ))
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
        let buf_a = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &base64Data)
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
    let monitor = app
        .primary_monitor()
        .map_err(|e| e.to_string())?
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
pub fn get_fuse_bit_defs(
    configName: String,
    chipName: String,
) -> Option<&'static crate::fuse_defs::FuseConfigDef> {
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
            fuses: fuse_cfg
                .fuses
                .iter()
                .map(|f| FuseFieldDto {
                    name: f.name.clone(),
                    display_name: fuse_display_name(&f.name),
                    mask: f.mask,
                    default_value: f.default,
                })
                .collect(),
            locks: fuse_cfg
                .locks
                .iter()
                .map(|f| FuseFieldDto {
                    name: f.name.clone(),
                    display_name: fuse_display_name(&f.name),
                    mask: f.mask,
                    default_value: f.default,
                })
                .collect(),
        },
        minipro_core::device::ChipConfig::Pld(_) => ChipConfigDto::Pld {},
    });

    // Detect AVR-family devices by config name (e.g., "avr_11", "avr_6").
    // AVR convention: bit=0 means programmed (active).  PIC and others: bit=1.
    // This is more reliable than name pattern matching — the database itself
    // declares the convention via the config name.
    let invert_fuse_bits = if let Some(minipro_core::device::ChipConfig::Mcu(ref cfg)) = dev.config
    {
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
        voltages: VoltagesDto::from_voltages(
            &dev.voltages,
            model,
            dev.chip_type,
            dev.flags.custom_protocol,
        ),
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
        icsp: dev.package_details.icsp,
        icsp_only: dev.flags.prog_support == minipro_core::device::MP_ICSP_ONLY,
    }
}

fn package_type_name(pkg: &PackageDetails) -> String {
    if pkg.plcc {
        format!("PLCC{}", pkg.pin_count)
    } else {
        format!("DIP{}", pkg.pin_count)
    }
}

// ── XGPro algorithm/firmware downloader ─────────────────────────────────────

/// One downloaded firmware image, for display in the Settings result.
#[derive(Serialize)]
pub struct DownloadedFirmwareDto {
    pub model: String,
    pub version: String,
    pub path: String,
}

/// Result of `download_algorithms`.
#[derive(Serialize)]
pub struct AlgoDownloadOutcomeDto {
    pub output_dir: String,
    pub xml_path: String,
    pub t56_algorithms: usize,
    pub t76_algorithms: usize,
    pub firmware: Vec<DownloadedFirmwareDto>,
    pub warnings: Vec<String>,
}

/// Download the XGPro setup archives, extract the T56/T76 FPGA algorithms
/// and programmer firmware images, and generate `algorithm.xml`.
///
/// `outputDir` — `None` writes to the per-user data directory, which the
/// database resolver already searches; pass the custom db dir when the user
/// keeps their database files elsewhere.
///
/// Runs on a blocking thread; progress rides the standard `progress`
/// channel (`{done, total, operation}`) plus `app-log` stage lines.
/// `cancel_algo_download` flips a flag the core progress callback observes.
#[tauri::command]
pub async fn download_algorithms(
    window: Window,
    state: State<'_, Arc<AppState>>,
    outputDir: Option<String>,
) -> Result<AlgoDownloadOutcomeDto, String> {
    if !state.try_acquire() {
        return Err("Another operation is already running".into());
    }
    state
        .download_cancel
        .store(false, std::sync::atomic::Ordering::SeqCst);

    let win = window.clone();
    let cancel = (*state).clone();
    let output_dir = outputDir.map(std::path::PathBuf::from);

    let result = tokio::task::spawn_blocking(move || {
        let mut progress = |ev: minipro_core::xgpro_fetch::Progress| -> bool {
            if cancel
                .download_cancel
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                return false;
            }
            use minipro_core::xgpro_fetch::Progress as P;
            match ev {
                P::DownloadStart { archive, total } => {
                    emit_log(&win, "info", &format!("Downloading {archive}..."));
                    let _ = win.emit(
                        "progress",
                        serde_json::json!({
                            "done": 0,
                            "total": total.unwrap_or(0),
                            "operation": "download",
                        }),
                    );
                }
                P::DownloadBytes { received, .. } => {
                    let _ = win.emit(
                        "progress",
                        serde_json::json!({
                            "done": received,
                            "total": 0,
                            "operation": "download",
                        }),
                    );
                }
                P::DownloadFinished { archive } => {
                    emit_log(&win, "info", &format!("{archive}: download complete"));
                }
                P::Stage { archive, stage } => {
                    emit_log(&win, "info", &format!("{archive}: {stage}"));
                }
                P::Processed { done, total, .. } => {
                    let _ = win.emit(
                        "progress",
                        serde_json::json!({
                            "done": done,
                            "total": total,
                            "operation": "extract",
                        }),
                    );
                }
                P::Written { path } => {
                    emit_log(&win, "info", &format!("wrote {}", path.display()));
                }
            }
            true
        };
        let mut opts = minipro_core::xgpro_fetch::FetchOptions {
            output_dir,
            progress: Some(&mut progress),
            ..Default::default()
        };
        minipro_core::xgpro_fetch::fetch(&mut opts).map_err(|e| e.to_string())
    })
    .await;

    state.release();

    let out = match result {
        Ok(Ok(o)) => o,
        Ok(Err(e)) if e == "Operation cancelled" => {
            emit_log(&window, "warn", "Download cancelled");
            return Err("cancelled".into());
        }
        Ok(Err(e)) => {
            emit_log(&window, "error", &format!("Download failed: {e}"));
            return Err(e);
        }
        Err(e) => {
            emit_log(&window, "error", &format!("Download task failed: {e}"));
            return Err(format!("Task panicked: {e}"));
        }
    };

    // Refresh the algorithms slot of the resolved db paths so the new file
    // is picked up. An existing still-present file wins (explicit overrides
    // are never clobbered by the download).
    {
        let mut guard = state.db_paths.lock().map_err(|e| e.to_string())?;
        if let Some(ref mut paths) = *guard {
            let stale = match &paths.algorithms {
                Some(p) => !p.exists(),
                None => true,
            };
            if stale {
                if let Ok(fresh) = DatabasePaths::resolve(None, None, None) {
                    paths.algorithms = fresh.algorithms;
                }
            }
        }
    }

    for w in &out.warnings {
        emit_log(&window, "warn", w);
    }
    emit_log(
        &window,
        "info",
        &format!(
            "algorithm.xml: {} T56 + {} T76 algorithms → {}",
            out.t56_algorithms,
            out.t76_algorithms,
            out.xml_path.display()
        ),
    );
    emit_log(
        &window,
        "info",
        "If a programmer is connected, reconnect it to use the new algorithm.xml",
    );

    Ok(AlgoDownloadOutcomeDto {
        output_dir: out.output_dir.display().to_string(),
        xml_path: out.xml_path.display().to_string(),
        t56_algorithms: out.t56_algorithms,
        t76_algorithms: out.t76_algorithms,
        firmware: out
            .firmware
            .iter()
            .map(|f| DownloadedFirmwareDto {
                model: f.model.to_string(),
                version: f.version.to_string(),
                path: f.path.display().to_string(),
            })
            .collect(),
        warnings: out.warnings,
    })
}

/// Cancel a running `download_algorithms` call. Fire-and-forget: the
/// download thread notices at the next progress checkpoint.
#[tauri::command]
pub async fn cancel_algo_download(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state
        .download_cancel
        .store(true, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

/// A downloaded firmware image matching the connected programmer model.
#[derive(Serialize)]
pub struct FirmwareFileDto {
    pub path: String,
    pub model: String,
    pub version: String,
}

fn parse_fw_version(v: &str) -> Option<[u32; 3]> {
    let mut it = v.split('.');
    let parsed = [
        it.next()?.parse().ok()?,
        it.next()?.parse().ok()?,
        it.next()?.parse().ok()?,
    ];
    if it.next().is_some() {
        return None;
    }
    Some(parsed)
}

/// Find firmware images previously written by `download_algorithms` that
/// match `model` (e.g. "T76" → `firmware-T76-*.dat`). Scans `customDir`
/// first (the configured custom database dir, which the GUI downloader
/// writes to), then the per-user data dir (CLI default). Returns matches
/// newest-version-first, deduplicated by version — empty vec when none
/// exist (e.g. TL866A/CS, whose firmware isn't in the pinned archives).
#[tauri::command]
pub fn find_downloaded_firmware(
    model: String,
    customDir: Option<String>,
) -> Result<Vec<FirmwareFileDto>, String> {
    let mut dirs = Vec::new();
    if let Some(d) = customDir {
        dirs.push(std::path::PathBuf::from(d));
    }
    if let Some(d) = minipro_core::user_data_dir() {
        dirs.push(d);
    }

    let prefix = format!("firmware-{model}-");
    let mut found = Vec::new();
    for dir in dirs {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(version) = name
                .strip_prefix(&prefix)
                .and_then(|s| s.strip_suffix(".dat"))
                .filter(|v| parse_fw_version(v).is_some())
            {
                found.push(FirmwareFileDto {
                    path: entry.path().display().to_string(),
                    model: model.clone(),
                    version: version.to_string(),
                });
            }
        }
    }
    // Stable sort keeps the custom-dir entry ahead of the per-user one
    // when both carry the same version.
    found.sort_by_key(|f| std::cmp::Reverse(parse_fw_version(&f.version)));
    found.dedup_by(|a, b| a.version == b.version);
    Ok(found)
}
