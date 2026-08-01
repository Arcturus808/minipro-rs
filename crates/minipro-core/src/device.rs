//! Device and chip descriptor types.
//!
//! This module contains all the strongly-typed structs and enums that describe a
//! programmable chip and the programmer hardware.  They correspond to the C
//! `device_t`, `package_t` and related structs in the upstream minipro source.

/// Programmer model identifiers (matches the C MP_* defines).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ProgrammerModel {
    Tl866a = 1,
    Tl866cs = 2,
    Tl866iiPlus = 5,
    T56 = 6,
    T48 = 7,
    T76 = 8,
}

impl std::fmt::Display for ProgrammerModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProgrammerModel::Tl866a => write!(f, "TL866A"),
            ProgrammerModel::Tl866cs => write!(f, "TL866CS"),
            ProgrammerModel::Tl866iiPlus => write!(f, "TL866II+"),
            ProgrammerModel::T56 => write!(f, "T56"),
            ProgrammerModel::T48 => write!(f, "T48"),
            ProgrammerModel::T76 => write!(f, "T76"),
        }
    }
}

impl TryFrom<u8> for ProgrammerModel {
    type Error = u8;
    fn try_from(v: u8) -> std::result::Result<Self, u8> {
        match v {
            1 => Ok(Self::Tl866a),
            2 => Ok(Self::Tl866cs),
            5 => Ok(Self::Tl866iiPlus),
            6 => Ok(Self::T56),
            7 => Ok(Self::T48),
            8 => Ok(Self::T76),
            x => Err(x),
        }
    }
}

impl std::str::FromStr for ProgrammerModel {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, String> {
        match s.to_ascii_uppercase().as_str() {
            "TL866A" => Ok(Self::Tl866a),
            "TL866CS" => Ok(Self::Tl866cs),
            "TL866II" | "TL866II+" | "TL866IIPLUS" => Ok(Self::Tl866iiPlus),
            "T56" => Ok(Self::T56),
            "T48" => Ok(Self::T48),
            "T76" => Ok(Self::T76),
            other => Err(format!(
                "unknown programmer model '{other}'; expected one of: TL866A, TL866CS, TL866II, T48, T56, T76"
            )),
        }
    }
}

/// Programmer status returned in system-info response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgrammerStatus {
    Normal,
    Bootloader,
}

// ── Chip-level enumerations ──────────────────────────────────────────────────

/// Chip type / family classification (matches the C MP_* type defines).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ChipType {
    Memory = 0x01,
    Mcu = 0x02,
    Pld = 0x03,
    Sram = 0x04,
    Logic = 0x05,
    Nand = 0x06,
    Emmc = 0x07,
    Vga = 0x08,
}

impl TryFrom<u32> for ChipType {
    type Error = u32;
    fn try_from(v: u32) -> std::result::Result<Self, u32> {
        match v {
            0x01 => Ok(Self::Memory),
            0x02 => Ok(Self::Mcu),
            0x03 => Ok(Self::Pld),
            0x04 => Ok(Self::Sram),
            0x05 => Ok(Self::Logic),
            0x06 => Ok(Self::Nand),
            0x07 => Ok(Self::Emmc),
            0x08 => Ok(Self::Vga),
            x => Err(x),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum DataOrg {
    #[default]
    Bytes = 0x00,
    Words = 0x01,
    Bits = 0x02,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FuseType {
    User = 0x00,
    Config = 0x01,
    Lock = 0x02,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum Endianness {
    #[default]
    Little = 0,
    Big = 1,
}

// ── Sub-structures ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct Voltages {
    pub vcc: u8,
    pub vdd: u8,
    pub vpp: u8,
    /// Raw packed value as stored in the XML `voltages` attribute.
    pub raw: u32,
}

impl Voltages {
    pub fn from_raw(raw: u32) -> Self {
        Self {
            vdd: ((raw >> 12) & 0x0f) as u8,
            vcc: ((raw >> 8) & 0x0f) as u8,
            vpp: (raw & 0xff) as u8,
            raw,
        }
    }

    /// Repack the low 16 bits of `raw` from the current `vdd`/`vcc`/`vpp`
    /// field values, preserving the high 16 bits.
    ///
    /// Mirrors `pack_voltages()` in the upstream C minipro (`database.c`).
    /// Called before `begin_transaction` so that `--vcc`/`--vdd`/`--vpp`
    /// overrides are reflected in the raw value sent to the firmware.
    pub fn pack(&mut self) {
        self.raw = (self.raw & 0xffff_0000)
            | ((self.vdd as u32) << 12)
            | ((self.vcc as u32) << 8)
            | self.vpp as u32;
    }
}

// ── Voltage parameter tables ─────────────────────────────────────────────────
//
// Maps human-readable voltage names to the firmware encoding that goes into
// `voltages.vcc` / `voltages.vpp` / `voltages.vdd`.  These are not raw DAC
// values but indices into lookup tables inside the programmer firmware.
// Tables mirror the `parameters_t` arrays in the upstream C `database.c`.

/// TL866A/CS VPP table (`tl866a_vpp_voltages`).
static TL866A_VPP_VOLTAGES: &[(&str, u8)] = &[
    ("10", 0x40),
    ("12.5", 0x00),
    ("13.5", 0x30),
    ("14", 0x50),
    ("16", 0x10),
    ("17", 0x70),
    ("18", 0x60),
    ("21", 0x20),
];

/// TL866A/CS VCC table (`tl866a_vcc_voltages`).
static TL866A_VCC_VOLTAGES: &[(&str, u8)] = &[
    ("3.3", 0x02),
    ("4", 0x01),
    ("4.5", 0x05),
    ("5", 0x00),
    ("5.5", 0x04),
    ("6.5", 0x03),
];

/// TL866II+ VPP table (`tl866ii_vpp_voltages`).
static TL866II_VPP_VOLTAGES: &[(&str, u8)] = &[
    ("9", 0x10),
    ("9.5", 0x20),
    ("10", 0x30),
    ("11", 0x40),
    ("11.5", 0x50),
    ("12", 0x00),
    ("12.5", 0x60),
    ("13", 0x70),
    ("13.5", 0x80),
    ("14", 0x90),
    ("14.5", 0xa0),
    ("15.5", 0xb0),
    ("16", 0xc0),
    ("16.5", 0xd0),
    ("17", 0xe0),
    ("18", 0xf0),
];

/// TL866II+ VCC table (`tl866ii_vcc_voltages`).
static TL866II_VCC_VOLTAGES: &[(&str, u8)] = &[
    ("3.3", 0x01),
    ("4", 0x02),
    ("4.5", 0x03),
    ("5", 0x00),
    ("5.5", 0x04),
    ("6.5", 0x05),
];

/// XGecu T48/T56/T76 VPP table (`xg_vpp_voltages`).
static XG_VPP_VOLTAGES: &[(&str, u8)] = &[
    ("9", 0x10),
    ("9.5", 0x20),
    ("10", 0x30),
    ("11", 0x40),
    ("11.5", 0x50),
    ("12", 0x00),
    ("12.5", 0x60),
    ("13", 0x70),
    ("13.5", 0x80),
    ("14", 0x90),
    ("14.5", 0xa0),
    ("15.5", 0xb0),
    ("16", 0xc0),
    ("16.5", 0xd0),
    ("17", 0xe0),
    ("18", 0xf0),
    ("21", 0xf2),
    ("25", 0xf1),
];

/// XGecu T76 PLD VPP table (`xg_pld_vpp_voltages`) — PLD VPP capped at 18 V.
static XG_PLD_VPP_VOLTAGES: &[(&str, u8)] = &[
    ("9", 0x10),
    ("9.5", 0x20),
    ("10", 0x30),
    ("11", 0x40),
    ("11.5", 0x50),
    ("12", 0x00),
    ("12.5", 0x60),
    ("13", 0x70),
    ("13.5", 0x80),
    ("14", 0x90),
    ("14.5", 0xa0),
    ("15.5", 0xb0),
    ("16", 0xc0),
    ("16.5", 0xd0),
    ("17", 0xe0),
    ("18", 0xf0),
];

/// XGecu T48/T56/T76 VCC table (`xg_vcc_voltages`).
static XG_VCC_VOLTAGES: &[(&str, u8)] = &[
    ("1.2", 0x09),
    ("1.8", 0x06),
    ("2.5", 0x07),
    ("3", 0x08),
    ("3.3", 0x01),
    ("4", 0x02),
    ("4.5", 0x03),
    ("4.75", 0x0a),
    ("5", 0x00),
    ("5.25", 0x0b),
    ("5.5", 0x04),
    ("5.75", 0x0c),
    ("6", 0x0d),
    ("6.25", 0x0e),
    ("6.5", 0x05),
];

/// T48 bit-bang (custom protocol) VCC table (`t48_bb_vcc_voltages`).
static T48_BB_VCC_VOLTAGES: &[(&str, u8)] = &[
    ("1.75", 0x01),
    ("1.8", 0x02),
    ("1.9", 0x03),
    ("2", 0x04),
    ("2.1", 0x05),
    ("2.2", 0x06),
    ("2.3", 0x08),
    ("2.4", 0x09),
    ("2.5", 0x0a),
    ("2.6", 0x0b),
    ("2.7", 0x0d),
    ("2.8", 0x0e),
    ("2.9", 0x0f),
    ("3", 0x10),
    ("3.3", 0x14),
    ("3.5", 0x16),
    ("3.7", 0x18),
    ("3.8", 0x1a),
    ("4", 0x1c),
    ("4.2", 0x1e),
    ("4.3", 0x20),
    ("4.4", 0x21),
    ("4.5", 0x22),
    ("4.7", 0x25),
    ("4.8", 0x26),
    ("4.9", 0x27),
    ("5", 0x28),
    ("5.2", 0x2b),
    ("5.3", 0x2c),
    ("5.4", 0x2d),
    ("5.5", 0x2f),
    ("5.6", 0x30),
    ("5.7", 0x31),
    ("5.8", 0x32),
    ("5.9", 0x33),
    ("6", 0x34),
    ("6.1", 0x35),
    ("6.2", 0x36),
    ("6.3", 0x38),
    ("6.5", 0x3b),
    ("6.6", 0x3c),
    ("6.7", 0x3d),
    ("6.8", 0x3e),
    ("6.9", 0x3f),
];

/// T48 bit-bang (custom protocol) VPP table (`t48_bb_vpp_voltages`).
static T48_BB_VPP_VOLTAGES: &[(&str, u8)] = &[
    ("9", 0x00),
    ("9.5", 0x01),
    ("10", 0x03),
    ("11", 0x07),
    ("11.5", 0x09),
    ("12", 0x0b),
    ("12.5", 0x0d),
    ("13", 0x0e),
    ("13.5", 0x11),
    ("14", 0x13),
    ("14.5", 0x15),
    ("15.5", 0x18),
    ("16", 0x1a),
    ("16.5", 0x1c),
    ("17", 0x1e),
    ("18", 0x23),
    ("21", 0x2f),
    ("25", 0x3e),
];

/// Logic-IC test VCC table (`vcc_logic_voltages`), shared by all programmer
/// models.  Used both to parse the `voltage` attribute of logicic.xml entries
/// and to validate `--vcc` overrides for logic devices.
pub static LOGIC_VCC_VOLTAGES: &[(&str, u8)] =
    &[("1.8", 0x03), ("2.5", 0x02), ("3.3", 0x01), ("5", 0x00)];

/// VCC/VDD voltage table for a given programmer model and device.
///
/// Mirrors the table assignment in the upstream C `load_device()`:
/// logic ICs always use [`LOGIC_VCC_VOLTAGES`]; T56/T76 bit-bang (custom
/// protocol) devices have no table in upstream, so `None` is returned and the
/// caller should reject the override.
pub fn vcc_voltage_table(
    model: ProgrammerModel,
    chip_type: u32,
    custom_protocol: bool,
) -> Option<&'static [(&'static str, u8)]> {
    if chip_type == ChipType::Logic as u32 {
        return Some(LOGIC_VCC_VOLTAGES);
    }
    match model {
        ProgrammerModel::Tl866a | ProgrammerModel::Tl866cs => Some(TL866A_VCC_VOLTAGES),
        ProgrammerModel::Tl866iiPlus => Some(TL866II_VCC_VOLTAGES),
        ProgrammerModel::T48 => Some(if custom_protocol {
            T48_BB_VCC_VOLTAGES
        } else {
            XG_VCC_VOLTAGES
        }),
        ProgrammerModel::T56 | ProgrammerModel::T76 => {
            if custom_protocol {
                None
            } else {
                Some(XG_VCC_VOLTAGES)
            }
        }
    }
}

/// VPP voltage table for a given programmer model and device.
///
/// Logic ICs have no VPP (returns `None`).  On the T76, PLD devices are
/// limited to the 18 V table (`xg_pld_vpp_voltages` in upstream).
pub fn vpp_voltage_table(
    model: ProgrammerModel,
    chip_type: u32,
    custom_protocol: bool,
) -> Option<&'static [(&'static str, u8)]> {
    if chip_type == ChipType::Logic as u32 {
        return None;
    }
    match model {
        ProgrammerModel::Tl866a | ProgrammerModel::Tl866cs => Some(TL866A_VPP_VOLTAGES),
        ProgrammerModel::Tl866iiPlus => Some(TL866II_VPP_VOLTAGES),
        ProgrammerModel::T48 => Some(if custom_protocol {
            T48_BB_VPP_VOLTAGES
        } else {
            XG_VPP_VOLTAGES
        }),
        ProgrammerModel::T56 => {
            if custom_protocol {
                None
            } else {
                Some(XG_VPP_VOLTAGES)
            }
        }
        ProgrammerModel::T76 => {
            if custom_protocol {
                None
            } else if chip_type == ChipType::Pld as u32 {
                Some(XG_PLD_VPP_VOLTAGES)
            } else {
                Some(XG_VPP_VOLTAGES)
            }
        }
    }
}

/// Look up a voltage name in a parameter table, returning the firmware code.
///
/// The match is case-insensitive and tolerates a trailing `V` (as in the
/// logicic.xml `voltage="5V"` attribute) and a trailing `.0` (so `--vcc 5.0`
/// works as well as `--vcc 5`).
pub fn lookup_voltage(table: &[(&str, u8)], value: &str) -> Option<u8> {
    let v = value.trim();
    let v = v.strip_suffix(['V', 'v']).unwrap_or(v);
    let v = v.strip_suffix(".0").unwrap_or(v);
    table
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(v))
        .map(|(_, code)| *code)
}

/// List the valid voltage names of a table for error messages.
pub fn voltage_table_names(table: &[(&str, u8)]) -> String {
    table
        .iter()
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .join(", ")
}

#[derive(Debug, Clone, Default)]
pub struct PackageDetails {
    pub pin_count: u8,
    /// Adapter type index (TSOP48, SOP44, …).
    pub adapter: u8,
    /// True when a PLCC adapter is in use.
    pub plcc: bool,
    /// ICSP mode flags.
    pub icsp: u8,
    /// Raw packed value as stored in the XML `package_details` attribute.
    pub raw: u32,
}

const PIN_COUNT_MASK: u32 = 0x3f00_0000;
const ICSP_MASK: u32 = 0x0000_ff00;
const ADAPTER_MASK: u32 = 0x0000_00ff;
const PLCC20_ADAPTER: u32 = 0x38;
const PLCC28_ADAPTER: u32 = 0x3e;
const PLCC32_ADAPTER: u32 = 0x3f;
const PLCC44_ADAPTER: u32 = 0x3d;

impl PackageDetails {
    pub fn from_raw(raw: u32) -> Self {
        let adapter = (raw & ADAPTER_MASK) as u8;
        let icsp = ((raw & ICSP_MASK) >> 8) as u8;
        let pin_cnt = (raw & PIN_COUNT_MASK) >> 24;

        // Some PLCC adapters encode the pin count differently.
        let pin_count = match pin_cnt {
            p if p == PLCC20_ADAPTER => 20,
            p if p == PLCC28_ADAPTER => 28,
            p if p == PLCC32_ADAPTER => 32,
            p if p == PLCC44_ADAPTER => 44,
            p => p,
        } as u8;

        let plcc = pin_count > 0x30;

        Self {
            pin_count,
            adapter,
            plcc,
            icsp,
            raw,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DeviceFlags {
    pub can_erase: bool,
    pub has_chip_id: bool,
    pub has_data_offset: bool,
    pub off_protect_before: bool,
    pub protect_after: bool,
    pub lock_bit_write_only: bool,
    pub has_calibration: bool,
    /// Supported programming modes bitmask.
    pub prog_support: u8,
    /// Word size in bytes (1 or 2).
    pub word_size: u8,
    pub data_org: DataOrg,
    pub can_adjust_vpp: bool,
    pub can_adjust_vcc: bool,
    pub can_adjust_clock: bool,
    pub can_adjust_address: bool,
    pub custom_protocol: bool,
    pub has_power_down: bool,
    pub is_powerdown_disabled: bool,
    pub reversed_package: bool,
    /// Raw packed flags value as stored in the XML.
    pub raw: u32,
}

// Flag bit masks (from database.c)
const MP_REVERSED_PACKAGE: u32 = 0x0000_0002;
const MP_ERASE_MASK: u32 = 0x0000_0010;
const MP_ID_MASK: u32 = 0x0000_0020;
const MP_DATA_MEMORY_ADDRESS: u32 = 0x0000_1000;
const MP_DATA_BUS_WIDTH: u32 = 0x0000_2000; // == MP_DATA_ORG
const MP_OFF_PROTECT_BEFORE: u32 = 0x0000_4000;
const MP_PROTECT_AFTER: u32 = 0x0000_8000;
const MP_LOCK_BIT_WRITE_ONLY: u32 = 0x0004_0000;
const MP_CALIBRATION: u32 = 0x0008_0000;
const MP_SUPPORTED_PROGRAMMING: u32 = 0x0030_0000;
const MP_DATA_ORG: u32 = MP_DATA_BUS_WIDTH;

// Voltage chip_info values
const MP_VOLTAGES1: u32 = 0x0006;
const MP_VOLTAGES2: u32 = 0x0007;

// Last-JEDEC-bit / powerdown flags live in the voltages field
const LAST_JEDEC_BIT_IS_POWERDOWN_ENABLE: u32 = 0x1000;
const POWERDOWN_MODE_DISABLE: u32 = 0x2000;

const CUSTOM_PROTOCOL_MASK: u32 = 0x8000_0000;

impl DeviceFlags {
    pub fn from_raw(flags: u32, chip_info: u32, voltages_raw: u32) -> Self {
        let prog_support = ((flags & MP_SUPPORTED_PROGRAMMING) >> 20) as u8;
        let data_org = if flags & MP_DATA_ORG != 0 {
            DataOrg::Words
        } else {
            DataOrg::Bytes
        };
        let word_size = if flags & MP_DATA_ORG != 0 { 2 } else { 1 };

        Self {
            can_erase: (flags & MP_ERASE_MASK) != 0,
            has_chip_id: (flags & MP_ID_MASK) != 0,
            has_data_offset: (flags & MP_DATA_MEMORY_ADDRESS) != 0,
            off_protect_before: (flags & MP_OFF_PROTECT_BEFORE) != 0,
            protect_after: (flags & MP_PROTECT_AFTER) != 0,
            lock_bit_write_only: (flags & MP_LOCK_BIT_WRITE_ONLY) != 0,
            has_calibration: (flags & MP_CALIBRATION) != 0,
            prog_support,
            word_size,
            data_org,
            can_adjust_vcc: chip_info == MP_VOLTAGES1,
            can_adjust_vpp: chip_info == MP_VOLTAGES2,
            custom_protocol: (flags & CUSTOM_PROTOCOL_MASK) != 0,
            has_power_down: (voltages_raw & LAST_JEDEC_BIT_IS_POWERDOWN_ENABLE) != 0,
            is_powerdown_disabled: (voltages_raw & POWERDOWN_MODE_DISABLE) != 0,
            reversed_package: (flags & MP_REVERSED_PACKAGE) != 0,
            // can_adjust_clock / can_adjust_address set later by database layer
            can_adjust_clock: false,
            can_adjust_address: false,
            raw: flags,
        }
    }
}

// ── Fuse / configuration data ────────────────────────────────────────────────

/// A single named fuse/lock field with mask and default value.
#[derive(Debug, Clone)]
pub struct FuseField {
    pub name: String,
    pub mask: u16,
    pub default: u16,
}

/// MCU fuse/lock/calibration configuration block.
#[derive(Debug, Clone, Default)]
pub struct FuseConfig {
    /// Config name from the XML `<config name="...">` attribute (e.g., "avr_11", "pic_11").
    /// Used to determine fuse bit convention: AVR configs use bit=0 (programmed),
    /// PIC and other configs use bit=1 (programmed).
    pub name: String,
    pub num_calibytes: u32,
    pub num_uids: u32,
    pub config_addr: u32,
    pub osccal_save: u32,
    pub eep_addr: u32,
    pub bg_mask: u32,
    pub rev_bits: u8,
    pub fuses: Vec<FuseField>,
    pub locks: Vec<FuseField>,
}

/// PLD (GAL) configuration block.
#[derive(Debug, Clone, Default)]
pub struct GalConfig {
    pub fuses_size: u32,
    pub row_width: u32,
    pub ues_address: u32,
    pub ues_size: u32,
    pub powerdown_row: u32,
    pub acw_address: u32,
    pub acw_bits: Vec<u16>,
}

/// Chip-specific configuration data, one variant per chip family.
#[derive(Debug, Clone)]
pub enum ChipConfig {
    Mcu(FuseConfig),
    Pld(GalConfig),
}

// ── Algorithm (T56 / T76 FPGA bitstream) ────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct Algorithm {
    pub name: String,
    pub bitstream: Vec<u8>,
}

// ── Main device descriptor ───────────────────────────────────────────────────

/// Full description of a programmable device, derived from infoic.xml or
/// logicic.xml.  Mirrors `device_t` in the C implementation.
#[derive(Debug, Clone, Default)]
pub struct Device {
    pub name: String,
    pub manufacturer: String,
    pub chip_type: u32,
    pub protocol_id: u8,
    pub variant: u32,
    pub read_buffer_size: u16,
    pub write_buffer_size: u16,
    pub code_memory_size: u32,
    pub data_memory_size: u32,
    pub data_memory2_size: u32,
    pub page_size: u32,
    /// NAND flash: pages per erase block.
    pub pages_per_block: u32,
    pub chip_id: u32,
    pub chip_id_bytes_count: u8,
    pub voltages: Voltages,
    pub pulse_delay: u32,
    pub flags: DeviceFlags,
    /// Chip-info word (encodes PIC word width, Atmel arch, voltage caps…).
    pub chip_info: u32,
    pub pin_map: u32,
    pub compare_mask: u16,
    pub blank_value: u16,
    pub package_details: PackageDetails,
    pub config: Option<ChipConfig>,
    /// Logic-IC test vectors (one row per test step, pin_count bytes wide).
    pub vectors: Option<Vec<u8>>,
    pub vector_count: usize,
    pub tl866_only: bool,
    pub t48_only: bool,
    pub t56_only: bool,
    pub spi_clock: u8,
    pub i2c_address: u8,
    pub algorithm: Option<Algorithm>,
}

impl Device {
    /// Helper: bytes per addressable word (1 or 2).
    pub fn word_size(&self) -> usize {
        self.flags.word_size as usize
    }

    /// Total code memory in bytes.
    pub fn code_memory_bytes(&self) -> usize {
        self.code_memory_size as usize * self.word_size()
    }
}

// ── System info returned from programmer ─────────────────────────────────────

/// Information returned by the "get system info" command.
#[derive(Debug, Clone)]
pub struct ProgrammerInfo {
    pub model: ProgrammerModel,
    pub status: ProgrammerStatus,
    pub firmware: u32,
    pub firmware_str: String,
    pub device_code: String,
    pub serial_number: String,
    pub hardware_version: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    const MEM: u32 = ChipType::Memory as u32;
    const LOGIC: u32 = ChipType::Logic as u32;
    const PLD: u32 = ChipType::Pld as u32;

    #[test]
    fn test_lookup_voltage_normalization() {
        // Plain names, logicic.xml style 'V' suffix, and '.0' suffix all work.
        assert_eq!(lookup_voltage(LOGIC_VCC_VOLTAGES, "5"), Some(0x00));
        assert_eq!(lookup_voltage(LOGIC_VCC_VOLTAGES, "5V"), Some(0x00));
        assert_eq!(lookup_voltage(LOGIC_VCC_VOLTAGES, "5.0"), Some(0x00));
        assert_eq!(lookup_voltage(LOGIC_VCC_VOLTAGES, "3.3v"), Some(0x01));
        assert_eq!(lookup_voltage(LOGIC_VCC_VOLTAGES, "2.5"), Some(0x02));
        assert_eq!(lookup_voltage(LOGIC_VCC_VOLTAGES, "1.8"), Some(0x03));
        // Not in the logic table.
        assert_eq!(lookup_voltage(LOGIC_VCC_VOLTAGES, "7.0"), None);
        assert_eq!(lookup_voltage(LOGIC_VCC_VOLTAGES, "1.9"), None);
        assert_eq!(lookup_voltage(LOGIC_VCC_VOLTAGES, "abc"), None);
    }

    #[test]
    fn test_logic_devices_always_use_logic_table() {
        for model in [
            ProgrammerModel::Tl866a,
            ProgrammerModel::Tl866cs,
            ProgrammerModel::Tl866iiPlus,
            ProgrammerModel::T48,
            ProgrammerModel::T56,
            ProgrammerModel::T76,
        ] {
            let t = vcc_voltage_table(model, LOGIC, false).unwrap();
            assert!(std::ptr::eq(t, LOGIC_VCC_VOLTAGES));
            // Logic ICs have no VPP/VDD.
            assert!(vpp_voltage_table(model, LOGIC, false).is_none());
        }
    }

    #[test]
    fn test_tl866iiplus_memory_tables() {
        let vcc = vcc_voltage_table(ProgrammerModel::Tl866iiPlus, MEM, false).unwrap();
        assert_eq!(lookup_voltage(vcc, "5"), Some(0x00));
        assert_eq!(lookup_voltage(vcc, "6.5"), Some(0x05));
        assert_eq!(lookup_voltage(vcc, "1.9"), None); // not a TL866II+ voltage
        let vpp = vpp_voltage_table(ProgrammerModel::Tl866iiPlus, MEM, false).unwrap();
        assert_eq!(lookup_voltage(vpp, "12"), Some(0x00));
        assert_eq!(lookup_voltage(vpp, "12.5"), Some(0x60));
        assert_eq!(lookup_voltage(vpp, "21"), None); // TL866II+ VPP caps at 18 V
    }

    #[test]
    fn test_tl866a_memory_tables() {
        let vcc = vcc_voltage_table(ProgrammerModel::Tl866a, MEM, false).unwrap();
        assert_eq!(lookup_voltage(vcc, "4"), Some(0x01));
        assert_eq!(lookup_voltage(vcc, "6.5"), Some(0x03));
        let vpp = vpp_voltage_table(ProgrammerModel::Tl866a, MEM, false).unwrap();
        assert_eq!(lookup_voltage(vpp, "21"), Some(0x20));
        assert_eq!(lookup_voltage(vpp, "12.5"), Some(0x00));
        assert_eq!(lookup_voltage(vpp, "9"), None); // TL866A VPP starts at 10 V
    }

    #[test]
    fn test_xg_tables() {
        for model in [
            ProgrammerModel::T48,
            ProgrammerModel::T56,
            ProgrammerModel::T76,
        ] {
            let vcc = vcc_voltage_table(model, MEM, false).unwrap();
            assert_eq!(lookup_voltage(vcc, "1.2"), Some(0x09));
            assert_eq!(lookup_voltage(vcc, "5"), Some(0x00));
            assert_eq!(lookup_voltage(vcc, "6.5"), Some(0x05));
            assert_eq!(lookup_voltage(vcc, "7.0"), None);
            let vpp = vpp_voltage_table(model, MEM, false).unwrap();
            assert_eq!(lookup_voltage(vpp, "25"), Some(0xf1));
            assert_eq!(lookup_voltage(vpp, "12"), Some(0x00));
        }
        // T76 PLD VPP is capped at 18 V.
        let pld_vpp = vpp_voltage_table(ProgrammerModel::T76, PLD, false).unwrap();
        assert_eq!(lookup_voltage(pld_vpp, "18"), Some(0xf0));
        assert_eq!(lookup_voltage(pld_vpp, "21"), None);
        assert_eq!(lookup_voltage(pld_vpp, "25"), None);
    }

    #[test]
    fn test_custom_protocol_tables() {
        // T48 bit-bang devices use the dedicated bb tables.
        let vcc = vcc_voltage_table(ProgrammerModel::T48, MEM, true).unwrap();
        assert_eq!(lookup_voltage(vcc, "3.3"), Some(0x14));
        // T56/T76 bit-bang devices have no voltage table in upstream.
        assert!(vcc_voltage_table(ProgrammerModel::T56, MEM, true).is_none());
        assert!(vcc_voltage_table(ProgrammerModel::T76, MEM, true).is_none());
        assert!(vpp_voltage_table(ProgrammerModel::T56, MEM, true).is_none());
        // TL866A/II+ bit-bang tables are the same as the normal ones.
        let vcc = vcc_voltage_table(ProgrammerModel::Tl866iiPlus, MEM, true).unwrap();
        assert_eq!(lookup_voltage(vcc, "5"), Some(0x00));
    }

    #[test]
    fn test_voltage_table_names() {
        assert_eq!(voltage_table_names(LOGIC_VCC_VOLTAGES), "1.8, 2.5, 3.3, 5");
    }
}
