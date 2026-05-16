//! Device and chip descriptor types.
//!
//! This module contains all the strongly-typed structs and enums that describe a
//! programmable chip and the programmer hardware.  They correspond to the C
//! `device_t`, `package_t` and related structs in the upstream minipro source.

/// Programmer model identifiers (matches the C MP_* defines).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ProgrammerModel {
    Tl866a      = 1,
    Tl866cs     = 2,
    Tl866iiPlus = 5,
    T56         = 6,
    T48         = 7,
    T76         = 8,
}

impl std::fmt::Display for ProgrammerModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProgrammerModel::Tl866a      => write!(f, "TL866A"),
            ProgrammerModel::Tl866cs     => write!(f, "TL866CS"),
            ProgrammerModel::Tl866iiPlus => write!(f, "TL866II+"),
            ProgrammerModel::T56         => write!(f, "T56"),
            ProgrammerModel::T48         => write!(f, "T48"),
            ProgrammerModel::T76         => write!(f, "T76"),
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
    Mcu    = 0x02,
    Pld    = 0x03,
    Sram   = 0x04,
    Logic  = 0x05,
    Nand   = 0x06,
    Emmc   = 0x07,
    Vga    = 0x08,
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
            x    => Err(x),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum DataOrg {
    #[default]
    Bytes = 0x00,
    Words = 0x01,
    Bits  = 0x02,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FuseType {
    User   = 0x00,
    Config = 0x01,
    Lock   = 0x02,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum Endianness {
    #[default]
    Little = 0,
    Big    = 1,
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
            vcc: ((raw >> 8)  & 0x0f) as u8,
            vpp: (raw & 0xff) as u8,
            raw,
        }
    }
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

const PIN_COUNT_MASK:  u32 = 0x3f00_0000;
const ICSP_MASK:       u32 = 0x0000_ff00;
const ADAPTER_MASK:    u32 = 0x0000_00ff;
const PLCC20_ADAPTER:  u32 = 0x38;
const PLCC28_ADAPTER:  u32 = 0x3e;
const PLCC32_ADAPTER:  u32 = 0x3f;
const PLCC44_ADAPTER:  u32 = 0x3d;

impl PackageDetails {
    pub fn from_raw(raw: u32) -> Self {
        let adapter = (raw & ADAPTER_MASK) as u8;
        let icsp    = ((raw & ICSP_MASK) >> 8) as u8;
        let pin_cnt = ((raw & PIN_COUNT_MASK) >> 24) as u32;

        // Some PLCC adapters encode the pin count differently.
        let pin_count = match pin_cnt {
            p if p == PLCC20_ADAPTER => 20,
            p if p == PLCC28_ADAPTER => 28,
            p if p == PLCC32_ADAPTER => 32,
            p if p == PLCC44_ADAPTER => 44,
            p => p,
        } as u8;

        let plcc = pin_count > 0x30;

        Self { pin_count, adapter, plcc, icsp, raw }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DeviceFlags {
    pub can_erase:            bool,
    pub has_chip_id:          bool,
    pub has_data_offset:      bool,
    pub off_protect_before:   bool,
    pub protect_after:        bool,
    pub lock_bit_write_only:  bool,
    pub has_calibration:      bool,
    /// Supported programming modes bitmask.
    pub prog_support:         u8,
    /// Word size in bytes (1 or 2).
    pub word_size:            u8,
    pub data_org:             DataOrg,
    pub can_adjust_vpp:       bool,
    pub can_adjust_vcc:       bool,
    pub can_adjust_clock:     bool,
    pub can_adjust_address:   bool,
    pub custom_protocol:      bool,
    pub has_power_down:       bool,
    pub is_powerdown_disabled:bool,
    pub reversed_package:     bool,
    /// Raw packed flags value as stored in the XML.
    pub raw: u32,
}

// Flag bit masks (from database.c)
const MP_REVERSED_PACKAGE:     u32 = 0x0000_0002;
const MP_ERASE_MASK:            u32 = 0x0000_0010;
const MP_ID_MASK:               u32 = 0x0000_0020;
const MP_DATA_MEMORY_ADDRESS:   u32 = 0x0000_1000;
const MP_DATA_BUS_WIDTH:        u32 = 0x0000_2000; // == MP_DATA_ORG
const MP_OFF_PROTECT_BEFORE:    u32 = 0x0000_4000;
const MP_PROTECT_AFTER:         u32 = 0x0000_8000;
const MP_LOCK_BIT_WRITE_ONLY:   u32 = 0x0004_0000;
const MP_CALIBRATION:           u32 = 0x0008_0000;
const MP_SUPPORTED_PROGRAMMING: u32 = 0x0030_0000;
const MP_DATA_ORG:              u32 = MP_DATA_BUS_WIDTH;

// Voltage chip_info values
const MP_VOLTAGES1: u32 = 0x0006;
const MP_VOLTAGES2: u32 = 0x0007;

// Last-JEDEC-bit / powerdown flags live in the voltages field
const LAST_JEDEC_BIT_IS_POWERDOWN_ENABLE: u32 = 0x1000;
const POWERDOWN_MODE_DISABLE:             u32 = 0x2000;

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
            can_erase:             (flags & MP_ERASE_MASK) != 0,
            has_chip_id:           (flags & MP_ID_MASK) != 0,
            has_data_offset:       (flags & MP_DATA_MEMORY_ADDRESS) != 0,
            off_protect_before:    (flags & MP_OFF_PROTECT_BEFORE) != 0,
            protect_after:         (flags & MP_PROTECT_AFTER) != 0,
            lock_bit_write_only:   (flags & MP_LOCK_BIT_WRITE_ONLY) != 0,
            has_calibration:       (flags & MP_CALIBRATION) != 0,
            prog_support,
            word_size,
            data_org,
            can_adjust_vcc:        chip_info == MP_VOLTAGES1,
            can_adjust_vpp:        chip_info == MP_VOLTAGES2,
            custom_protocol:       (flags & CUSTOM_PROTOCOL_MASK) != 0,
            has_power_down:        (voltages_raw & LAST_JEDEC_BIT_IS_POWERDOWN_ENABLE) != 0,
            is_powerdown_disabled: (voltages_raw & POWERDOWN_MODE_DISABLE) != 0,
            reversed_package:      (flags & MP_REVERSED_PACKAGE) != 0,
            // can_adjust_clock / can_adjust_address set later by database layer
            can_adjust_clock:   false,
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
    pub num_calibytes: u32,
    pub num_uids:      u32,
    pub config_addr:   u32,
    pub osccal_save:   u32,
    pub eep_addr:      u32,
    pub bg_mask:       u32,
    pub rev_bits:      u8,
    pub fuses:         Vec<FuseField>,
    pub locks:         Vec<FuseField>,
}

/// PLD (GAL) configuration block.
#[derive(Debug, Clone, Default)]
pub struct GalConfig {
    pub fuses_size:     u32,
    pub row_width:      u32,
    pub ues_address:    u32,
    pub ues_size:       u32,
    pub powerdown_row:  u32,
    pub acw_address:    u32,
    pub acw_bits:       Vec<u16>,
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
    pub name:      String,
    pub bitstream: Vec<u8>,
}

// ── Main device descriptor ───────────────────────────────────────────────────

/// Full description of a programmable device, derived from infoic.xml or
/// logicic.xml.  Mirrors `device_t` in the C implementation.
#[derive(Debug, Clone)]
pub struct Device {
    pub name:               String,
    pub chip_type:          u32,
    pub protocol_id:        u8,
    pub variant:            u32,
    pub read_buffer_size:   u16,
    pub write_buffer_size:  u16,
    pub code_memory_size:   u32,
    pub data_memory_size:   u32,
    pub data_memory2_size:  u32,
    pub page_size:          u32,
    /// NAND flash: pages per erase block.
    pub pages_per_block:    u32,
    pub chip_id:            u32,
    pub chip_id_bytes_count: u8,
    pub voltages:           Voltages,
    pub pulse_delay:        u32,
    pub flags:              DeviceFlags,
    /// Chip-info word (encodes PIC word width, Atmel arch, voltage caps…).
    pub chip_info:          u32,
    pub pin_map:            u32,
    pub compare_mask:       u16,
    pub blank_value:        u16,
    pub package_details:    PackageDetails,
    pub config:             Option<ChipConfig>,
    /// Logic-IC test vectors (one row per test step, pin_count bytes wide).
    pub vectors:            Option<Vec<u8>>,
    pub vector_count:       usize,
    pub tl866_only:         bool,
    pub t48_only:           bool,
    pub t56_only:           bool,
    pub spi_clock:          u8,
    pub i2c_address:        u8,
    pub algorithm:          Option<Algorithm>,
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
    pub model:           ProgrammerModel,
    pub status:          ProgrammerStatus,
    pub firmware:        u32,
    pub firmware_str:    String,
    pub device_code:     String,
    pub serial_number:   String,
    pub hardware_version: u8,
}
