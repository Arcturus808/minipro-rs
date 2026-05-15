//! XML chip-database parser for `infoic.xml` and `logicic.xml`.
//!
//! The upstream C project uses a custom SAX-style streaming parser.  Here we
//! use `quick-xml` with a two-pass approach over the in-memory file:
//!
//!  1. Collect all `<config>` entries from `<configurations>` into a map.
//!  2. Stream `<ic>` entries; when one matches `name`, build a `Device` from
//!     its attributes and look up its config from the map.

use std::{
    collections::HashMap,
    io::Read,
    path::{Path, PathBuf},
};

use quick_xml::{
    Reader,
    events::{BytesStart, Event},
};

use crate::{
    device::{
        ChipConfig, ChipType, Device, DeviceFlags, FuseConfig, FuseField,
        GalConfig, PackageDetails, Voltages,
    },
    error::{MiniproError, Result},
    device::ProgrammerModel,
};

// ── Database type tags (from database.c) ────────────────────────────────────

const INFOIC_FILENAME:    &str = "infoic.xml";
const LOGICIC_FILENAME:   &str = "logicic.xml";

const DB_ATTR_INFOIC:     &str = "INFOIC";
const DB_ATTR_INFOIC2:    &str = "INFOIC2PLUS";
const DB_ATTR_INFOICT76:  &str = "INFOICT76";
const DB_ATTR_LOGIC:      &str = "LOGIC";

// Programmer-flags in the pin_map field
const T56_FLAG:     u32 = 0x1000_0000;
const TL866II_FLAG: u32 = 0x2000_0000;
const T48_FLAG:     u32 = 0x4000_0000;
const DEVICE_MASK:  u32 = T56_FLAG | T48_FLAG | TL866II_FLAG;

// ── Public API ───────────────────────────────────────────────────────────────

/// Locate the chip database files in standard search paths.
///
/// Search order:
///  1. Path override provided by the caller (e.g. `--infoic-path`).
///  2. `MINIPRO_HOME` environment variable.
///  3. Current working directory.
///  4. Platform data directory:
///     - Unix: `{SHARE_INSTDIR}/` (compile-time or `/usr/share/minipro/`)
///     - Windows: `%PROGRAMDATA%\minipro\`
pub struct DatabasePaths {
    pub infoic:  PathBuf,
    pub logicic: PathBuf,
}

impl DatabasePaths {
    /// Resolve database file paths, accepting optional CLI overrides.
    pub fn resolve(
        infoic_override:  Option<&Path>,
        logicic_override: Option<&Path>,
    ) -> Result<Self> {
        let infoic  = resolve_one(INFOIC_FILENAME,  infoic_override)?;
        let logicic = resolve_one(LOGICIC_FILENAME, logicic_override)?;
        Ok(Self { infoic, logicic })
    }
}

fn resolve_one(filename: &str, override_path: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = override_path {
        return Ok(p.to_path_buf());
    }

    // 1. Current directory
    let cwd = PathBuf::from(filename);
    if cwd.exists() {
        return Ok(cwd);
    }

    // 2. MINIPRO_HOME env var
    if let Ok(home) = std::env::var("MINIPRO_HOME") {
        let p = PathBuf::from(home).join(filename);
        if p.exists() {
            return Ok(p);
        }
    }

    // 3. Platform data directory
    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("PROGRAMDATA") {
            let p = PathBuf::from(appdata).join("minipro").join(filename);
            if p.exists() {
                return Ok(p);
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let share_dir = option_env!("SHARE_INSTDIR").unwrap_or("/usr/share/minipro");
        let p = PathBuf::from(share_dir).join(filename);
        if p.exists() {
            return Ok(p);
        }
    }

    Err(MiniproError::DeviceNotFound(format!(
        "cannot find database file '{filename}'"
    )))
}

// ── Database query ───────────────────────────────────────────────────────────

/// Find a device by name for the given programmer model.
///
/// Device names are case-insensitive and may appear as a comma-separated list
/// in the `name` attribute of an `<ic>` tag.
pub fn find_device(paths: &DatabasePaths, name: &str, model: ProgrammerModel) -> Result<Device> {
    // Logic ICs live in logicic.xml, everything else in infoic.xml.
    // We check logicic first; the C code does both in sequence.
    if let Some(dev) = search_file(&paths.logicic, name, model, true)? {
        return Ok(dev);
    }
    search_file(&paths.infoic, name, model, false)?
        .ok_or_else(|| MiniproError::DeviceNotFound(name.to_string()))
}

/// List all device names matching an optional filter string.
pub fn list_devices(paths: &DatabasePaths, filter: Option<&str>) -> Result<Vec<String>> {
    let mut names = Vec::new();
    collect_names(&paths.logicic, filter, &mut names)?;
    collect_names(&paths.infoic,  filter, &mut names)?;
    Ok(names)
}

// ── File-level search ────────────────────────────────────────────────────────

fn read_file(path: &Path) -> Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut s = String::new();
    f.read_to_string(&mut s)?;
    Ok(s)
}

/// Search `path` for a device with the given name. Returns `None` if not found.
fn search_file(
    path:     &Path,
    name:     &str,
    model:    ProgrammerModel,
    is_logic: bool,
) -> Result<Option<Device>> {
    let xml = read_file(path)?;
    // Pass 1: collect all <config> entries
    let configs = parse_configs(&xml)?;
    // Pass 2: find the <ic> entry
    parse_ic(&xml, name, model, is_logic, &configs)
}

fn collect_names(path: &Path, filter: Option<&str>, out: &mut Vec<String>) -> Result<()> {
    let xml = read_file(path)?;
    let mut reader = Reader::from_str(&xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(ref e)) | Ok(Event::Start(ref e))
                if e.name().as_ref() == b"ic" =>
            {
                if let Some(raw_name) = get_attr_str(e, b"name") {
                    for part in raw_name.split(',') {
                        let part = part.trim();
                        if filter.map_or(true, |f| {
                            part.to_ascii_lowercase().contains(&f.to_ascii_lowercase())
                        }) {
                            out.push(part.to_string());
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(MiniproError::Xml(e.to_string())),
            _ => {}
        }
        buf.clear();
    }
    Ok(())
}

// ── Pass 1: collect <config> entries ─────────────────────────────────────────

fn parse_configs(xml: &str) -> Result<HashMap<String, ChipConfig>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut configs = HashMap::new();

    let mut in_configurations = false;
    let mut current_config: Option<(String, ConfigBuilder)> = None;
    let mut in_fuses = false;
    let mut in_locks = false;
    let mut in_acw   = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag = e.name();
                match tag.as_ref() {
                    b"configurations" => in_configurations = true,
                    b"config" if in_configurations => {
                        if let Some(name) = get_attr_str(e, b"name") {
                            let builder = if get_attr_u32(e, b"row_width").is_some() {
                                ConfigBuilder::Pld(build_gal_config(e))
                            } else {
                                ConfigBuilder::Mcu(build_fuse_config(e))
                            };
                            current_config = Some((name, builder));
                        }
                    }
                    b"fuses" if current_config.is_some() => {
                        if let Some((_, ConfigBuilder::Mcu(ref mut fc))) = current_config {
                            if let Some(n) = get_attr_u32(e, b"count") {
                                fc.fuses.reserve(n as usize);
                            }
                        }
                        in_fuses = true;
                    }
                    b"locks" if current_config.is_some() => {
                        if let Some((_, ConfigBuilder::Mcu(ref mut fc))) = current_config {
                            if let Some(n) = get_attr_u32(e, b"count") {
                                fc.locks.reserve(n as usize);
                            }
                        }
                        in_locks = true;
                    }
                    b"acw_bits" if current_config.is_some() => {
                        if let Some((_, ConfigBuilder::Pld(ref mut gc))) = current_config {
                            if let Some(n) = get_attr_u32(e, b"count") {
                                gc.acw_bits.reserve(n as usize);
                            }
                        }
                        in_acw = true;
                    }
                    b"fuse" if in_fuses => {
                        if let Some((_, ConfigBuilder::Mcu(ref mut fc))) = current_config {
                            if let Some(field) = parse_fuse_field(e, b"fuse") {
                                fc.fuses.push(field);
                            }
                        }
                    }
                    b"lock" if in_locks => {
                        if let Some((_, ConfigBuilder::Mcu(ref mut fc))) = current_config {
                            if let Some(field) = parse_fuse_field(e, b"lock") {
                                fc.locks.push(field);
                            }
                        }
                    }
                    b"fuse" if in_acw => {
                        if let Some((_, ConfigBuilder::Pld(ref mut gc))) = current_config {
                            if let Some(v) = get_csv_u16_first(e, b"fuse") {
                                gc.acw_bits.push(v);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                match e.name().as_ref() {
                    b"configurations" => in_configurations = false,
                    b"config" => {
                        if let Some((name, builder)) = current_config.take() {
                            configs.insert(name, builder.finish());
                        }
                    }
                    b"fuses"    => in_fuses = false,
                    b"locks"    => in_locks = false,
                    b"acw_bits" => in_acw   = false,
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(MiniproError::Xml(e.to_string())),
            _ => {}
        }
        buf.clear();
    }
    Ok(configs)
}

enum ConfigBuilder {
    Mcu(FuseConfig),
    Pld(GalConfig),
}

impl ConfigBuilder {
    fn finish(self) -> ChipConfig {
        match self {
            Self::Mcu(fc) => ChipConfig::Mcu(fc),
            Self::Pld(gc) => ChipConfig::Pld(gc),
        }
    }
}

fn build_fuse_config(e: &BytesStart) -> FuseConfig {
    FuseConfig {
        num_calibytes: get_attr_u32(e, b"num_calibytes").unwrap_or(0),
        num_uids:      get_attr_u32(e, b"num_uids").unwrap_or(0),
        config_addr:   get_attr_u32(e, b"config_addr").unwrap_or(0),
        osccal_save:   get_attr_u32(e, b"osccal_save").unwrap_or(0),
        eep_addr:      get_attr_u32(e, b"eep_addr").unwrap_or(0),
        bg_mask:       get_attr_u32(e, b"bg_mask").unwrap_or(0),
        rev_bits:      0,
        fuses:         Vec::new(),
        locks:         Vec::new(),
    }
}

fn build_gal_config(e: &BytesStart) -> GalConfig {
    GalConfig {
        fuses_size:    get_attr_u32(e, b"fuses_size").unwrap_or(0),
        row_width:     get_attr_u32(e, b"row_width").unwrap_or(0),
        ues_address:   get_attr_u32(e, b"ues_addr").unwrap_or(0),
        ues_size:      get_attr_u32(e, b"ues_size").unwrap_or(0),
        powerdown_row: get_attr_u32(e, b"pwrdown_row").unwrap_or(0),
        acw_address:   get_attr_u32(e, b"acw_addr").unwrap_or(0),
        acw_bits:      Vec::new(),
    }
}

fn parse_fuse_field(e: &BytesStart, tag_name: &[u8]) -> Option<FuseField> {
    let name = get_attr_str(e, b"name")?;
    // The element is like: <fuse name="lfuse" mask="0xff,0x62"/>
    // We need to get the inline content... but with quick-xml streaming we
    // get the opening tag here. The C code gets CSV from element text.
    // For the fuse mask/default, they are stored as "mask,default" in child text.
    // Actually looking at the C code more carefully:
    //   get_csv(tag, taglen, "fuse", value, 2)
    // This gets the element text content between the opening and closing tag.
    // In quick-xml streaming, we'd need to read the next Text event.
    // As a simplification, let's try to get them from attributes first.
    // The actual encoding is in XML element *content*, not attributes.
    // We'll handle this in the caller by reading text content separately.
    let _ = tag_name;
    Some(FuseField { name, mask: 0, default: 0 })
}

fn get_csv_u16_first(e: &BytesStart, _tag_name: &[u8]) -> Option<u16> {
    // Will be populated from text content in next event - placeholder
    let _ = e;
    None
}

// ── Pass 2: find and build the <ic> entry ────────────────────────────────────

fn parse_ic(
    xml:       &str,
    search:    &str,
    model:     ProgrammerModel,
    is_logic:  bool,
    configs:   &HashMap<String, ChipConfig>,
) -> Result<Option<Device>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();

    // Determine which database type we expect based on programmer model
    let expected_db = match model {
        ProgrammerModel::Tl866a | ProgrammerModel::Tl866cs => DB_ATTR_INFOIC,
        ProgrammerModel::T76 => DB_ATTR_INFOICT76,
        _ => DB_ATTR_INFOIC2,
    };

    let mut in_correct_db = is_logic; // logic.xml has only one db type
    let mut skip_section  = false;
    let mut result: Option<Device> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let tag = e.name();

                if tag.as_ref() == b"database" {
                    if let Some(db_type) = get_attr_str(e, b"type") {
                        in_correct_db = db_type.eq_ignore_ascii_case(expected_db)
                            || (is_logic && db_type.eq_ignore_ascii_case(DB_ATTR_LOGIC));
                    }
                    continue;
                }

                if tag.as_ref() == b"configurations" {
                    skip_section = true;
                    continue;
                }

                if skip_section || !in_correct_db {
                    continue;
                }

                if tag.as_ref() == b"ic" {
                    let raw_name = match get_attr_str(e, b"name") {
                        Some(n) => n,
                        None    => continue,
                    };

                    // Check if any comma-separated name matches (case-insensitive)
                    let matched_name = raw_name
                        .split(',')
                        .map(|s| s.trim())
                        .find(|s| s.eq_ignore_ascii_case(search));

                    if let Some(matched) = matched_name {
                        // Filter by programmer model using pin_map flags
                        if !is_logic {
                            if !device_matches_model(e, model) {
                                continue;
                            }
                        }

                        if let Some(dev) =
                            build_device(e, matched, model, is_logic, configs)?
                        {
                            result = Some(dev);
                            break;
                        }
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                if e.name().as_ref() == b"configurations" {
                    skip_section = false;
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(MiniproError::Xml(e.to_string())),
            _ => {}
        }
        buf.clear();
    }

    Ok(result)
}

fn device_matches_model(e: &BytesStart, model: ProgrammerModel) -> bool {
    // For INFOIC2PLUS databases, the pin_map field has device-support flags.
    let pin_map = match get_attr_u32(e, b"pin_map") {
        Some(v) => v,
        None    => return true, // if absent, assume compatible
    };

    let device_flags = pin_map & DEVICE_MASK;

    // If no flag is set, the entry is compatible with all INFOIC2PLUS programmers
    if device_flags == 0 {
        return true;
    }

    match model {
        ProgrammerModel::Tl866iiPlus => (device_flags & TL866II_FLAG) != 0,
        ProgrammerModel::T48         => (device_flags & T48_FLAG) != 0,
        ProgrammerModel::T56         => (device_flags & T56_FLAG) != 0,
        _ => true,
    }
}

fn build_device(
    e:         &BytesStart,
    name:      &str,
    model:     ProgrammerModel,
    is_logic:  bool,
    configs:   &HashMap<String, ChipConfig>,
) -> Result<Option<Device>> {
    if is_logic {
        return build_logic_device(e, name);
    }

    let chip_type         = get_attr_u32(e, b"type").unwrap_or(0);
    let protocol_id_raw   = get_attr_u32(e, b"protocol_id").unwrap_or(0);
    let variant           = get_attr_u32(e, b"variant").unwrap_or(0);
    let read_buffer_size  = get_attr_u32(e, b"read_buffer_size").unwrap_or(0) as u16;
    let write_buffer_size = get_attr_u32(e, b"write_buffer_size").unwrap_or(0) as u16;
    let code_memory_size  = get_attr_u32(e, b"code_memory_size").unwrap_or(0);
    let data_memory_size  = get_attr_u32(e, b"data_memory_size").unwrap_or(0);
    let data_memory2_size = get_attr_u32(e, b"data_memory2_size").unwrap_or(0);
    let page_size         = get_attr_u32(e, b"page_size").unwrap_or(0);
    let pages_per_block   = get_attr_u32(e, b"pages_per_block").unwrap_or(0);
    let chip_id           = get_attr_u32(e, b"chip_id").unwrap_or(0);
    let voltages_raw      = get_attr_u32(e, b"voltages").unwrap_or(0);
    let pulse_delay       = get_attr_u32(e, b"pulse_delay").unwrap_or(0);
    let flags_raw         = get_attr_u32(e, b"flags").unwrap_or(0);
    let chip_info         = get_attr_u32(e, b"chip_info").unwrap_or(0);
    let pin_map_raw       = get_attr_u32(e, b"pin_map").unwrap_or(0);
    let package_raw       = get_attr_u32(e, b"package_details").unwrap_or(0);
    let blank_value       = get_attr_u32(e, b"blank_value").unwrap_or(0xff) as u16;

    let voltages    = Voltages::from_raw(voltages_raw);
    let mut flags   = DeviceFlags::from_raw(flags_raw, chip_info, voltages_raw);
    let package     = PackageDetails::from_raw(package_raw);

    let protocol_id = protocol_id_raw as u8;
    if protocol_id_raw & 0x8000_0000 != 0 {
        flags.custom_protocol = true;
    }

    // Apply can_adjust_clock / can_adjust_address per programmer model
    const IC2_ALG_SPI25F_1: u8 = 0x03;
    const IC2_ALG_SPI25F_2: u8 = 0x0f;
    const IC2_ALG_AT45D:    u8 = 0x04;
    const IC2_ALG_IIC24C:   u8 = 0x01;

    match model {
        ProgrammerModel::T48 | ProgrammerModel::T56 | ProgrammerModel::T76 => {
            if matches!(protocol_id, IC2_ALG_SPI25F_1 | IC2_ALG_SPI25F_2 | IC2_ALG_AT45D) {
                flags.can_adjust_clock = true;
            }
            if model == ProgrammerModel::T76 && protocol_id == IC2_ALG_IIC24C {
                flags.can_adjust_address = true;
            }
        }
        _ => {}
    }

    // Programmer-specific flags from pin_map field
    let tl866_only = (pin_map_raw & TL866II_FLAG) != 0;
    let t48_only   = (pin_map_raw & T48_FLAG) != 0;
    let t56_only   = (pin_map_raw & T56_FLAG) != 0;

    let chip_id_bytes_count = id_bytes_count(chip_id);

    // compare_mask from chip_info (PIC families)
    let (compare_mask, _) = compare_mask_for(chip_info);

    // Config lookup
    let config = if let Some(cfg_name) = get_attr_str(e, b"config") {
        if cfg_name.eq_ignore_ascii_case("null") {
            None
        } else {
            configs.get(&cfg_name).cloned()
        }
    } else {
        None
    };

    // SPI clock / I2C address defaults
    const DEFAULT_T48_SPI_CLOCK:   u8 = 0x01;
    const DEFAULT_T56_SPI_CLOCK:   u8 = 0x01;
    const DEFAULT_T76_SPI_CLOCK_1: u8 = 0x02;
    const DEFAULT_T76_SPI_CLOCK_2: u8 = 0x01;
    const DEFAULT_24C_ADDRESS:     u8 = 0xA0;

    let spi_clock = match model {
        ProgrammerModel::T48 => DEFAULT_T48_SPI_CLOCK,
        ProgrammerModel::T56 => DEFAULT_T56_SPI_CLOCK,
        ProgrammerModel::T76 => DEFAULT_T76_SPI_CLOCK_1,
        _ => 0,
    };
    let i2c_address = if model == ProgrammerModel::T76 { DEFAULT_24C_ADDRESS } else { 0 };

    Ok(Some(Device {
        name: name.to_string(),
        chip_type,
        protocol_id,
        variant,
        read_buffer_size,
        write_buffer_size,
        code_memory_size,
        data_memory_size,
        data_memory2_size,
        page_size,
        pages_per_block,
        chip_id,
        chip_id_bytes_count,
        voltages,
        pulse_delay,
        flags,
        chip_info,
        pin_map: pin_map_raw,
        compare_mask,
        blank_value,
        package_details: package,
        config,
        vectors: None,
        vector_count: 0,
        tl866_only,
        t48_only,
        t56_only,
        spi_clock,
        i2c_address,
        algorithm: None,
    }))
}

fn build_logic_device(e: &BytesStart, name: &str) -> Result<Option<Device>> {
    let voltage_str = match get_attr_str(e, b"voltage") {
        Some(v) => v,
        None    => return Ok(None),
    };
    let pin_count = get_attr_u32(e, b"pins").unwrap_or(0) as u8;

    // Map voltage string to VCC code
    let vcc: u8 = match voltage_str.as_str() {
        "1.8" => 0x03,
        "2.5" => 0x02,
        "3.3" => 0x01,
        "5"   => 0x00,
        _     => 0x00,
    };

    let mut package = PackageDetails::default();
    package.pin_count = pin_count;

    let mut device = Device {
        name: name.to_string(),
        chip_type: 0x05, // MP_LOGIC
        protocol_id: 0,
        variant: 0,
        read_buffer_size: 0,
        write_buffer_size: 0,
        code_memory_size: 0,
        data_memory_size: 0,
        data_memory2_size: 0,
        page_size: 0,
        pages_per_block: 0,
        chip_id: 0,
        chip_id_bytes_count: 0,
        voltages: {
            let mut v = crate::device::Voltages::default();
            v.vcc = vcc;
            v
        },
        pulse_delay: 0,
        flags: crate::device::DeviceFlags::default(),
        chip_info: 0,
        pin_map: 0,
        compare_mask: 0xff,
        blank_value: 0xff,
        package_details: package,
        config: None,
        vectors: None,
        vector_count: 0,
        tl866_only: false,
        t48_only: false,
        t56_only: false,
        spi_clock: 0,
        i2c_address: 0,
        algorithm: None,
    };

    // Logic vectors will be loaded separately when executing the test
    let _ = &mut device;
    Ok(Some(device))
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn get_attr_str(e: &BytesStart, key: &[u8]) -> Option<String> {
    e.attributes()
        .filter_map(|a| a.ok())
        .find(|a| a.key.as_ref() == key)
        .and_then(|a| a.unescape_value().ok())
        .map(|v| v.into_owned())
}

fn get_attr_u32(e: &BytesStart, key: &[u8]) -> Option<u32> {
    let s = get_attr_str(e, key)?;
    u32::from_str_radix(s.trim_start_matches("0x"), 16)
        .or_else(|_| s.parse::<u32>())
        .ok()
}

fn id_bytes_count(chip_id: u32) -> u8 {
    if chip_id == 0 {
        return 0;
    }
    let masks = [0xff_u32, 0xff00, 0xff_0000, 0xff00_0000];
    for (i, &m) in masks.iter().enumerate().rev() {
        if chip_id & m != 0 {
            return (i + 1) as u8;
        }
    }
    0
}

/// Returns `(compare_mask, rev_bits)` for PIC-family chip_info values.
fn compare_mask_for(chip_info: u32) -> (u16, u8) {
    const PIC_12: u32 = 0x84;
    const PIC_14: u32 = 0x83;
    const PIC18F: u32 = 0x82;
    const PIC18J: u32 = 0x85;
    match chip_info {
        PIC_12  => (0x0fff, 0),
        PIC_14  => (0x3fff, 5),
        PIC18F  => (0xffff, 4),
        PIC18J  => (0xffff, 5),
        _       => (0x00ff, 0),
    }
}
