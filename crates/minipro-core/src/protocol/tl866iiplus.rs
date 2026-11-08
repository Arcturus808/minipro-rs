//! TL866II+ / T48 / T56 protocol implementation.
//!
//! Command byte reference (from tl866iiplus.md and tl866iiplus.h):
//!
//! | Cmd  | Description                  |
//! |------|------------------------------|
//! | 0x00 | Get system info              |
//! | 0x03 | Begin transaction            |
//! | 0x04 | End transaction              |
//! | 0x05 | Read chip ID                 |
//! | 0x06 | Read USER memory             |
//! | 0x07 | Write USER memory            |
//! | 0x08 | Read fuses                   |
//! | 0x09 | Write fuses                  |
//! | 0x0C | Write code memory payload    |
//! | 0x0D | Read code memory payload     |
//! | 0x0E | Erase chip                   |
//! | 0x10 | Read data memory payload     |
//! | 0x11 | Write data memory payload    |
//! | 0x14 | Write LOCK bits              |
//! | 0x15 | Read LOCK bits               |
//! | 0x16 | Read RC calibration          |
//! | 0x18 | Protect off                  |
//! | 0x19 | Protect on                   |
//! | 0x38 | Unlock TSOP48                |
//! | 0x39 | Request status / OVC check   |
//! | 0x3C | Bootloader erase             |

use super::{DataSet, Device, JedecSet, LogicTestResult, OvcStatus, Protocol};
use crate::{
    error::{MiniproError, Result},
    usb::UsbDevice,
};
use log::trace;

// Command bytes
const CMD_GET_INFO: u8 = 0x00;
const CMD_BEGIN_TRANS: u8 = 0x03;
const CMD_END_TRANS: u8 = 0x04;
const CMD_READ_CHIP_ID: u8 = 0x05;
const CMD_READ_USER: u8 = 0x06;
const CMD_WRITE_USER: u8 = 0x07;
const CMD_READ_FUSES: u8 = 0x08;
const CMD_WRITE_FUSES: u8 = 0x09;
const CMD_WRITE_USER_DATA: u8 = 0x0A;
const CMD_READ_USER_DATA: u8 = 0x0B;
const CMD_WRITE_CODE: u8 = 0x0C;
const CMD_READ_CODE: u8 = 0x0D;
const CMD_ERASE: u8 = 0x0E;
const CMD_READ_DATA: u8 = 0x10;
const CMD_WRITE_DATA: u8 = 0x11;
const CMD_WRITE_LOCK: u8 = 0x14;
const CMD_READ_LOCK: u8 = 0x15;
const CMD_READ_CALIB: u8 = 0x16;
const CMD_PROTECT_OFF: u8 = 0x18;
const CMD_PROTECT_ON: u8 = 0x19;
const CMD_READ_JEDEC: u8 = 0x1D;
const CMD_WRITE_JEDEC: u8 = 0x1E;
const CMD_SET_VCC_VOLTAGE: u8 = 0x1B;
const CMD_SET_VPP_VOLTAGE: u8 = 0x1C;
const CMD_LOGIC_IC_TEST: u8 = 0x28;
const CMD_RESET_PIN_DRV: u8 = 0x2D;
const CMD_SET_VCC_PIN: u8 = 0x2E;
const CMD_SET_VPP_PIN: u8 = 0x2F;
const CMD_SET_GND_PIN: u8 = 0x30;
const CMD_SET_PULLDOWNS: u8 = 0x31;
const CMD_SET_PULLUPS: u8 = 0x32;
const CMD_SET_DIR: u8 = 0x34;
const CMD_READ_PINS: u8 = 0x35;
const CMD_SET_OUT: u8 = 0x36;
const CMD_AUTODETECT: u8 = 0x37;
const CMD_UNLOCK_TSOP48: u8 = 0x38;
const CMD_REQUEST_STATUS: u8 = 0x39;
const CMD_BTLDR_WRITE: u8 = 0x3B;
const CMD_BTLDR_ERASE: u8 = 0x3C;
const CMD_SWITCH: u8 = 0x3D;

// ZIF bus width (max 40 pins)
const ZIF_PINS: usize = 40;

// Firmware update constants
const BTLDR_MAGIC: u32 = 0xA578_B986;
const UPDATE_FILE_VERS_MASK: u32 = 0xffff_0000;
const UPDATE_FILE_VERSION: u32 = 0xf8cc_0000;

// Logic pin state constants (match C `pst[] = "01LHCZXGV"` indices)
const LOGIC_L: u8 = 2; // expected output Low
const LOGIC_H: u8 = 3; // expected output High
const LOGIC_Z: u8 = 5; // expected High-Z (pull-up=H, pull-down=L)

// Memory page types
const MP_CODE: u8 = 0x00;
const MP_DATA: u8 = 0x01;
const MP_USER: u8 = 0x02;

// Minimum firmware version expected
pub const MIN_FIRMWARE: u32 = 0x255; // 4.2.85

// ── Hardware-check pin tables ────────────────────────────────────────────────

/// A ZIF pin descriptor: physical pin number, byte offset within the 48-byte
/// message, and bitmask within that byte.
#[derive(Copy, Clone)]
struct ZifPin {
    pin: u8,
    byte: usize,
    mask: u8,
}

// 21 VPP pin drivers
const VPP_PINS: &[ZifPin] = &[
    ZifPin {
        pin: 1,
        byte: 10,
        mask: 0x01,
    },
    ZifPin {
        pin: 2,
        byte: 11,
        mask: 0x01,
    },
    ZifPin {
        pin: 3,
        byte: 12,
        mask: 0x01,
    },
    ZifPin {
        pin: 4,
        byte: 13,
        mask: 0x01,
    },
    ZifPin {
        pin: 5,
        byte: 14,
        mask: 0x01,
    },
    ZifPin {
        pin: 6,
        byte: 8,
        mask: 0x01,
    },
    ZifPin {
        pin: 7,
        byte: 8,
        mask: 0x02,
    },
    ZifPin {
        pin: 8,
        byte: 8,
        mask: 0x04,
    },
    ZifPin {
        pin: 9,
        byte: 8,
        mask: 0x08,
    },
    ZifPin {
        pin: 10,
        byte: 8,
        mask: 0x10,
    },
    ZifPin {
        pin: 30,
        byte: 8,
        mask: 0x20,
    },
    ZifPin {
        pin: 31,
        byte: 8,
        mask: 0x40,
    },
    ZifPin {
        pin: 32,
        byte: 8,
        mask: 0x80,
    },
    ZifPin {
        pin: 33,
        byte: 9,
        mask: 0x01,
    },
    ZifPin {
        pin: 34,
        byte: 9,
        mask: 0x02,
    },
    ZifPin {
        pin: 35,
        byte: 9,
        mask: 0x04,
    },
    ZifPin {
        pin: 36,
        byte: 9,
        mask: 0x08,
    },
    ZifPin {
        pin: 37,
        byte: 9,
        mask: 0x10,
    },
    ZifPin {
        pin: 38,
        byte: 9,
        mask: 0x20,
    },
    ZifPin {
        pin: 39,
        byte: 9,
        mask: 0x40,
    },
    ZifPin {
        pin: 40,
        byte: 9,
        mask: 0x80,
    },
];

// 32 VCC pin drivers
const VCC_PINS: &[ZifPin] = &[
    ZifPin {
        pin: 1,
        byte: 8,
        mask: 0x01,
    },
    ZifPin {
        pin: 2,
        byte: 8,
        mask: 0x02,
    },
    ZifPin {
        pin: 3,
        byte: 8,
        mask: 0x04,
    },
    ZifPin {
        pin: 4,
        byte: 8,
        mask: 0x08,
    },
    ZifPin {
        pin: 5,
        byte: 8,
        mask: 0x10,
    },
    ZifPin {
        pin: 6,
        byte: 8,
        mask: 0x20,
    },
    ZifPin {
        pin: 7,
        byte: 8,
        mask: 0x40,
    },
    ZifPin {
        pin: 8,
        byte: 8,
        mask: 0x80,
    },
    ZifPin {
        pin: 9,
        byte: 9,
        mask: 0x01,
    },
    ZifPin {
        pin: 10,
        byte: 9,
        mask: 0x02,
    },
    ZifPin {
        pin: 11,
        byte: 9,
        mask: 0x04,
    },
    ZifPin {
        pin: 12,
        byte: 9,
        mask: 0x08,
    },
    ZifPin {
        pin: 13,
        byte: 9,
        mask: 0x10,
    },
    ZifPin {
        pin: 14,
        byte: 9,
        mask: 0x20,
    },
    ZifPin {
        pin: 15,
        byte: 9,
        mask: 0x40,
    },
    ZifPin {
        pin: 16,
        byte: 9,
        mask: 0x80,
    },
    ZifPin {
        pin: 25,
        byte: 10,
        mask: 0x01,
    },
    ZifPin {
        pin: 26,
        byte: 10,
        mask: 0x02,
    },
    ZifPin {
        pin: 27,
        byte: 10,
        mask: 0x04,
    },
    ZifPin {
        pin: 28,
        byte: 10,
        mask: 0x08,
    },
    ZifPin {
        pin: 29,
        byte: 10,
        mask: 0x10,
    },
    ZifPin {
        pin: 30,
        byte: 10,
        mask: 0x20,
    },
    ZifPin {
        pin: 31,
        byte: 10,
        mask: 0x40,
    },
    ZifPin {
        pin: 32,
        byte: 10,
        mask: 0x80,
    },
    ZifPin {
        pin: 33,
        byte: 11,
        mask: 0x01,
    },
    ZifPin {
        pin: 34,
        byte: 11,
        mask: 0x02,
    },
    ZifPin {
        pin: 35,
        byte: 11,
        mask: 0x04,
    },
    ZifPin {
        pin: 36,
        byte: 11,
        mask: 0x08,
    },
    ZifPin {
        pin: 37,
        byte: 11,
        mask: 0x10,
    },
    ZifPin {
        pin: 38,
        byte: 11,
        mask: 0x20,
    },
    ZifPin {
        pin: 39,
        byte: 11,
        mask: 0x40,
    },
    ZifPin {
        pin: 40,
        byte: 11,
        mask: 0x80,
    },
];

// 34 GND pin drivers
const GND_PINS: &[ZifPin] = &[
    ZifPin {
        pin: 1,
        byte: 8,
        mask: 0x01,
    },
    ZifPin {
        pin: 2,
        byte: 8,
        mask: 0x02,
    },
    ZifPin {
        pin: 3,
        byte: 8,
        mask: 0x04,
    },
    ZifPin {
        pin: 4,
        byte: 8,
        mask: 0x08,
    },
    ZifPin {
        pin: 5,
        byte: 8,
        mask: 0x10,
    },
    ZifPin {
        pin: 6,
        byte: 8,
        mask: 0x20,
    },
    ZifPin {
        pin: 7,
        byte: 8,
        mask: 0x40,
    },
    ZifPin {
        pin: 8,
        byte: 8,
        mask: 0x80,
    },
    ZifPin {
        pin: 9,
        byte: 9,
        mask: 0x01,
    },
    ZifPin {
        pin: 10,
        byte: 9,
        mask: 0x02,
    },
    ZifPin {
        pin: 11,
        byte: 9,
        mask: 0x04,
    },
    ZifPin {
        pin: 12,
        byte: 9,
        mask: 0x08,
    },
    ZifPin {
        pin: 13,
        byte: 9,
        mask: 0x10,
    },
    ZifPin {
        pin: 14,
        byte: 9,
        mask: 0x20,
    },
    ZifPin {
        pin: 15,
        byte: 9,
        mask: 0x40,
    },
    ZifPin {
        pin: 16,
        byte: 9,
        mask: 0x80,
    },
    ZifPin {
        pin: 20,
        byte: 12,
        mask: 0x01,
    },
    ZifPin {
        pin: 21,
        byte: 13,
        mask: 0x01,
    },
    ZifPin {
        pin: 25,
        byte: 10,
        mask: 0x01,
    },
    ZifPin {
        pin: 26,
        byte: 10,
        mask: 0x02,
    },
    ZifPin {
        pin: 27,
        byte: 10,
        mask: 0x04,
    },
    ZifPin {
        pin: 28,
        byte: 10,
        mask: 0x08,
    },
    ZifPin {
        pin: 29,
        byte: 10,
        mask: 0x10,
    },
    ZifPin {
        pin: 30,
        byte: 10,
        mask: 0x20,
    },
    ZifPin {
        pin: 31,
        byte: 10,
        mask: 0x40,
    },
    ZifPin {
        pin: 32,
        byte: 10,
        mask: 0x80,
    },
    ZifPin {
        pin: 33,
        byte: 11,
        mask: 0x01,
    },
    ZifPin {
        pin: 34,
        byte: 11,
        mask: 0x02,
    },
    ZifPin {
        pin: 35,
        byte: 11,
        mask: 0x04,
    },
    ZifPin {
        pin: 36,
        byte: 11,
        mask: 0x08,
    },
    ZifPin {
        pin: 37,
        byte: 11,
        mask: 0x10,
    },
    ZifPin {
        pin: 38,
        byte: 11,
        mask: 0x20,
    },
    ZifPin {
        pin: 39,
        byte: 11,
        mask: 0x40,
    },
    ZifPin {
        pin: 40,
        byte: 11,
        mask: 0x80,
    },
];

// Indices into the pin tables for the overcurrent-protection tests
const VPP1: usize = 0; // VPP pin 1
const VCC1: usize = 0; // VCC pin 1
const GND1: usize = 0; // GND pin 1

/// Reset pin drivers, set all ZIF pins to input, and configure pull-up
/// resistors (`pullup` = 0 enables, 1 disables). Matches the C `init_zif`.
fn init_zif(usb: &UsbDevice, pullup: u8) -> Result<()> {
    // Reset pin drivers state (8-byte message)
    let mut msg = [0u8; 48];
    msg[0] = CMD_RESET_PIN_DRV;
    usb.msg_send(&msg[..8])?;

    // Set all ZIF pins to input (bytes 8..48 = 0x01)
    msg[8..48].fill(0x01);
    msg[0] = CMD_SET_DIR;
    usb.msg_send(&msg)?;

    // Set pull-up resistors (0 = enable, 1 = disable)
    msg[8..48].fill(pullup);
    msg[0] = CMD_SET_PULLUPS;
    usb.msg_send(&msg)?;
    Ok(())
}

pub struct Tl866iiPlusProtocol {
    /// True when this instance drives a T48 (re-exported as `T48Protocol`).
    ///
    /// The T48 shares the TL866II+ command set for chip operations but has
    /// its own pin-driver domain — 56 drivers, bit-packed pin-state reads,
    /// and different pin tables (upstream `t48.c`).  Operations that
    /// diverge branch on this flag; currently only `hardware_check`.
    is_t48: bool,
}

impl Tl866iiPlusProtocol {
    pub fn new() -> Self {
        Self { is_t48: false }
    }

    /// Constructor for the T48 (`protocol/t48.rs` re-exports this type as
    /// `T48Protocol`; `handle.rs` constructs it via this method).
    pub fn new_t48() -> Self {
        Self { is_t48: true }
    }
}

impl Default for Tl866iiPlusProtocol {
    fn default() -> Self {
        Self::new()
    }
}

// ── System info ──────────────────────────────────────────────────────────────

/// Parse the system-info response (up to 80 bytes).
///
/// Upstream `minipro_get_system_info` sends a 5-byte command and receives
/// into an 80-byte buffer.  Field layout depends on the model reported in
/// byte 6 (from minipro.c):
///
/// ```text
/// TL866II+:  device_code @ 8 (8 B),  serial @ 16 (20 B), hw_version @ 40
/// T48/T56/T76: mfg_date @ 8 (16 B), device_code @ 24 (8 B),
///              serial @ 32 (24 B), voltage @ 56 (4 B), usb_speed @ 60,
///              hw_version reported as 0
/// ```
pub fn get_system_info(usb: &UsbDevice) -> Result<SystemInfo> {
    let mut cmd = [0u8; 5];
    cmd[0] = CMD_GET_INFO;
    usb.msg_send(&cmd)?;
    let resp = usb.msg_recv(80)?;

    if resp.len() < 41 {
        return Err(MiniproError::ResponseTooShort {
            expected: 41,
            actual: resp.len(),
        });
    }

    let status = if resp[4] == 0 {
        crate::device::ProgrammerStatus::Bootloader
    } else {
        crate::device::ProgrammerStatus::Normal
    };

    let fw_minor = resp[4];
    let fw_major = resp[5];
    let firmware = ((fw_major as u32) << 8) | fw_minor as u32;

    let device_type = resp[6];
    let model = match device_type {
        5 => crate::device::ProgrammerModel::Tl866iiPlus,
        6 => crate::device::ProgrammerModel::T56,
        7 => crate::device::ProgrammerModel::T48,
        8 => crate::device::ProgrammerModel::T76,
        _ => crate::device::ProgrammerModel::Tl866iiPlus,
    };

    // T48/T56/T76 shift the identification fields by 16 bytes (the first
    // 16 bytes after the header carry the manufacture date instead).
    let (dev_off, serial_off, serial_len, hw_off) = match model {
        crate::device::ProgrammerModel::Tl866iiPlus => (8, 16, 20, Some(40)),
        _ => (24, 32, 24, None),
    };
    let need = serial_off + serial_len;
    if resp.len() < need {
        return Err(MiniproError::ResponseTooShort {
            expected: need,
            actual: resp.len(),
        });
    }

    let device_code = String::from_utf8_lossy(&resp[dev_off..dev_off + 8])
        .trim_end_matches(|c: char| c.is_control())
        .to_string();
    let serial_number = String::from_utf8_lossy(&resp[serial_off..serial_off + serial_len])
        .trim_end_matches(|c: char| c.is_control())
        .to_string();
    let hardware_version = hw_off.map(|o| resp[o]).unwrap_or(0);

    let firmware_str = format!("{:02}.{}.{:02}", hardware_version, fw_major, fw_minor);

    Ok(SystemInfo {
        model,
        status,
        firmware,
        firmware_str,
        device_code,
        serial_number,
        hardware_version,
    })
}

pub struct SystemInfo {
    pub model: crate::device::ProgrammerModel,
    pub status: crate::device::ProgrammerStatus,
    pub firmware: u32,
    pub firmware_str: String,
    pub device_code: String,
    pub serial_number: String,
    pub hardware_version: u8,
}

// ── Protocol implementation ───────────────────────────────────────────────────

impl Protocol for Tl866iiPlusProtocol {
    fn begin_transaction(&self, usb: &UsbDevice, device: &Device, icsp: u8) -> Result<()> {
        // 64-byte begin_transaction packet (matches upstream C tl866iiplus.c):
        // [0]     cmd = 0x03
        // [1]     protocol_id
        // [2]     variant (low byte)
        // [3]     icsp
        // [4..5]  raw_voltages low 16 bits (LE): vpp | (vcc << 8) | (vdd << 12)
        // [6]     chip_info (low byte)
        // [7]     pin_map (low byte)
        // [8..9]  data_memory_size (16-bit LE)
        // [10..11] page_size (16-bit LE)
        // [12..13] pulse_delay (16-bit LE)
        // [14..15] data_memory2_size (16-bit LE)
        // [16..19] code_memory_size (32-bit LE)
        // [20]    (raw_voltages >> 16) & 0xff
        // [21..22] voltage config flags
        // [40..43] package_details (32-bit LE)
        // [44..45] read_buffer_size (16-bit LE)
        // [56..59] raw_flags (32-bit LE)
        let mut pkt = [0u8; 64];
        pkt[0] = CMD_BEGIN_TRANS;
        pkt[1] = device.protocol_id;
        pkt[2] = (device.variant & 0xff) as u8;
        pkt[3] = icsp;
        le16(&mut pkt[4..6], (device.voltages.raw & 0xffff) as u16);
        pkt[6] = (device.chip_info & 0xff) as u8;
        pkt[7] = (device.pin_map & 0xff) as u8;
        le16(&mut pkt[8..10], (device.data_memory_size & 0xffff) as u16);
        le16(&mut pkt[10..12], (device.page_size & 0xffff) as u16);
        le16(&mut pkt[12..14], (device.pulse_delay & 0xffff) as u16);
        le16(&mut pkt[14..16], (device.data_memory2_size & 0xffff) as u16);
        le32(&mut pkt[16..20], device.code_memory_size);
        pkt[20] = ((device.voltages.raw >> 16) & 0xff) as u8;
        // Voltage config flags from raw_voltages
        if (device.voltages.raw & 0xf0) == 0xf0 {
            pkt[22] = (device.voltages.raw & 0xff) as u8;
        } else {
            pkt[21] = (device.voltages.raw & 0x0f) as u8;
            pkt[22] = (device.voltages.raw & 0xf0) as u8;
        }
        if device.voltages.raw & 0x8000_0000 != 0 {
            pkt[22] = ((device.voltages.raw >> 16) & 0x0f) as u8;
        }
        // T48 only (upstream t48_begin_transaction): msg[24]=1 enables the
        // SPI-clock override at msg[28].  Without it --speed is silently
        // ignored (e.g. in-circuit SPI flashes that need a slower clock).
        if self.is_t48 && device.flags.can_adjust_clock {
            pkt[24] = 1;
            pkt[28] = device.spi_clock;
        }
        le32(&mut pkt[40..44], device.package_details.raw);
        le16(&mut pkt[44..46], device.read_buffer_size);
        le32(&mut pkt[56..60], device.flags.raw);
        usb.msg_send(&pkt)
    }

    fn end_transaction(&self, usb: &UsbDevice) -> Result<()> {
        let mut pkt = [0u8; 8];
        pkt[0] = CMD_END_TRANS;
        usb.msg_send(&pkt)
    }

    fn read_block(&self, usb: &UsbDevice, _device: &Device, ds: &mut DataSet) -> Result<()> {
        // 8-byte read command: [cmd, 0, length_lo, length_hi, addr x4]
        // msg[1] is left 0 (C code zero-initialises msg[64] and never sets [1]).
        let cmd = read_cmd(ds);
        trace!(
            "read_block: page={} addr={:#x} len={} block_count={} cmd={:02x?}",
            ds.page_type,
            ds.address,
            ds.data.len(),
            ds.block_count,
            &cmd
        );
        usb.msg_send(&cmd)?;
        if self.is_t48 {
            // T48 (upstream t48_read_block): read_payload2(..., limit 0) —
            // every page type, MP_USER included, is read over EP2-IN only.
            // The T48 firmware never services EP3-IN during reads, so the
            // dual-endpoint path stalls/cancels for blocks > 64 bytes.
            ds.data = usb.read_payload_limit(ds.data.len(), 0)?;
            return Ok(());
        }
        // MP_USER (data_memory2) is always read over the command endpoint (EP1).
        if ds.page_type == MP_USER {
            ds.data = usb.msg_recv(ds.data.len())?;
            return Ok(());
        }
        // MP_CODE / MP_DATA: use read_payload (limit=64) so that reads > 64
        // bytes take the dual-EP interleaved path (EP2 + EP3), matching the
        // C `read_payload(handle, data, size)` call in tl866iiplus_read_block.
        // Passing limit=length would force single-EP2 and hang on large reads.
        ds.data = usb.read_payload(ds.data.len())?;
        trace!("read_block: got {} bytes", ds.data.len());
        Ok(())
    }

    fn write_block(&self, usb: &UsbDevice, device: &Device, ds: &DataSet) -> Result<()> {
        let cmd = write_cmd(ds);
        if self.is_t48 {
            // T48 (upstream t48_write_block): always an 8-byte header on EP1,
            // then the payload on EP2-OUT only — even for small writes (no
            // combined header+payload message like the TL866II+ path).
            // Upstream transfers device->write_buffer_size bytes regardless
            // of ds->size; pad a short final block so the transfer length
            // matches what the firmware expects.
            usb.msg_send(&cmd)?;
            let wbs = device.write_buffer_size as usize;
            if wbs > ds.data.len() {
                let mut payload = ds.data.to_vec();
                payload.resize(wbs, device.blank_value as u8);
                return usb.write_payload_limit(&payload, 0);
            }
            return usb.write_payload_limit(&ds.data, 0);
        }
        // Small writes (< 57 bytes payload): combine header + payload in a
        // single EP1 message (8 + payload <= 64).  Matches C tl866iiplus_write_block.
        if ds.data.len() < 57 {
            let mut msg = [0u8; 64];
            msg[..8].copy_from_slice(&cmd);
            msg[8..8 + ds.data.len()].copy_from_slice(&ds.data);
            usb.msg_send(&msg[..8 + ds.data.len()])?;
        } else {
            // Large writes: send 8-byte header on EP1, then payload on EP2/EP3.
            // The C write_payload wrapper always passes limit=64 to
            // write_payload2, and the TL866II+ firmware expects payloads > 64
            // bytes to be split across EP2/EP3.  Passing write_buffer_size
            // (e.g. 128 for 27512) as the limit caused all 128 bytes to go to
            // EP2 only, leaving EP3 waiting — the firmware hung and the USB
            // transfer timed out.  This affected all DIP28 EPROMs
            // (write_buffer_size=0x80) but not DIP24 (write_buffer_size=0x20,
            // which uses the small < 57 byte single-EP1 path).
            usb.msg_send(&cmd)?;
            // Upstream always transfers write_buffer_size payload bytes —
            // on a short final block it reads that many from ds.data (into
            // padded file data).  Pad a short block the same way so the
            // EP2/EP3 transfer length matches what the firmware expects.
            let wbs = device.write_buffer_size as usize;
            if wbs > ds.data.len() {
                let mut payload = ds.data.to_vec();
                payload.resize(wbs, device.blank_value as u8);
                usb.write_payload(&payload)?;
            } else {
                usb.write_payload(&ds.data)?;
            }
        }
        // C tl866iiplus_write_block does not read a status response here.
        Ok(())
    }

    fn get_chip_id(&self, usb: &UsbDevice, device: &Device) -> Result<(u8, u32)> {
        let mut pkt = [0u8; 8];
        pkt[0] = CMD_READ_CHIP_ID;
        usb.msg_send(&pkt)?;
        // Upstream receives 6 bytes on TL866II+, 32 on T48.
        let resp = usb.msg_recv(if self.is_t48 { 32 } else { 6 })?;
        // ID type is in resp[0], ID bytes start at resp[2].
        // Length is chip_id_bytes_count (1-4), endianness depends on type.
        let id_length = device.chip_id_bytes_count.min(4) as usize;
        let min_len = 2 + id_length;
        if resp.len() < min_len {
            return Err(MiniproError::ResponseTooShort {
                expected: min_len,
                actual: resp.len(),
            });
        }
        let id_type = resp[0];
        let chip_id = if id_length == 0 {
            0
        } else if id_type == 3 || id_type == 4 {
            // Little-endian
            let mut bytes = [0u8; 4];
            bytes[..id_length].copy_from_slice(&resp[2..2 + id_length]);
            u32::from_le_bytes(bytes)
        } else {
            // Big-endian
            let mut bytes = [0u8; 4];
            bytes[..id_length].copy_from_slice(&resp[2..2 + id_length]);
            u32::from_be_bytes(bytes) >> (8 * (4 - id_length))
        };
        Ok((id_type, chip_id))
    }

    fn read_fuses(
        &self,
        usb: &UsbDevice,
        device: &Device,
        fuse_type: u8,
        length: usize,
        items_count: u8,
    ) -> Result<Vec<u8>> {
        let cmd_byte = match fuse_type {
            0x00 => CMD_READ_USER,  // MP_FUSE_USER
            0x01 => CMD_READ_FUSES, // MP_FUSE_CFG
            0x02 => CMD_READ_LOCK,  // MP_FUSE_LOCK
            _ => CMD_READ_FUSES,
        };
        // Upstream tl866iiplus_read_fuses / t48_read_fuses (identical):
        // msg[1]=protocol_id, msg[2]=items_count, msg[4..8]=code_memory_size.
        let mut pkt = [0u8; 8];
        pkt[0] = cmd_byte;
        pkt[1] = device.protocol_id;
        pkt[2] = items_count;
        le32(&mut pkt[4..8], device.code_memory_size);
        usb.msg_send(&pkt)?;
        let resp = usb.msg_recv(64)?;
        // Fuse data starts at resp[8]; the response has an 8-byte header.
        let start = 8.min(resp.len());
        Ok(resp[start..].iter().copied().take(length).collect())
    }

    fn write_fuses(
        &self,
        usb: &UsbDevice,
        device: &Device,
        fuse_type: u8,
        length: usize,
        items_count: u8,
        data: &[u8],
    ) -> Result<()> {
        let cmd_byte = match fuse_type {
            0x00 => CMD_WRITE_USER,
            0x01 => CMD_WRITE_FUSES,
            0x02 => CMD_WRITE_LOCK,
            _ => CMD_WRITE_FUSES,
        };
        // Upstream tl866iiplus_write_fuses / t48_write_fuses (identical):
        // msg[1]=protocol_id, msg[2]=items_count,
        // msg[4..8]=code_memory_size - 0x38 (upstream comment: "firmware bug?"),
        // fuse data at msg[8], full 64-byte packet, no response read.
        let mut pkt = [0u8; 64];
        pkt[0] = cmd_byte;
        pkt[1] = device.protocol_id;
        pkt[2] = items_count;
        le32(&mut pkt[4..8], device.code_memory_size.wrapping_sub(0x38));
        let n = length.min(data.len()).min(56);
        pkt[8..8 + n].copy_from_slice(&data[..n]);
        usb.msg_send(&pkt)
    }

    fn read_calibration(&self, usb: &UsbDevice, size: usize) -> Result<Vec<u8>> {
        // Upstream maps no read_calibration handler for the T48.
        if self.is_t48 {
            return Err(MiniproError::UnsupportedOperation);
        }
        // Upstream tl866iiplus_read_calibration: length is a LE16 at
        // msg[2..4], a full 64-byte command is sent, and exactly `size`
        // bytes are received.
        let mut pkt = [0u8; 64];
        pkt[0] = CMD_READ_CALIB;
        le16(&mut pkt[2..4], size as u16);
        usb.msg_send(&pkt)?;
        usb.msg_recv(size)
    }

    fn erase(&self, usb: &UsbDevice, _device: &Device, num_fuses: u8, is_pld: bool) -> Result<()> {
        // Upstream tl866iiplus_erase / t48_erase (identical): num_fuses at
        // msg[2], pld flag at msg[4], 15-byte command packet.
        let mut pkt = [0u8; 15];
        pkt[0] = CMD_ERASE;
        pkt[2] = num_fuses;
        pkt[4] = is_pld as u8;
        usb.msg_send(&pkt)?;
        usb.msg_recv(64)?;
        Ok(())
    }

    fn read_jedec_row(&self, usb: &UsbDevice, device: &Device, js: &mut JedecSet) -> Result<()> {
        // Upstream tl866iiplus_read_jedec_row / t48_read_jedec_row
        // (identical): msg[1]=protocol_id, msg[2]=row size in bits,
        // msg[4]=row, msg[5]=flags; the response carries the packed row
        // data starting at msg[0].
        let mut pkt = [0u8; 8];
        pkt[0] = CMD_READ_JEDEC;
        pkt[1] = device.protocol_id;
        pkt[2] = (js.data.len() * 8).min(255) as u8;
        pkt[4] = js.row;
        pkt[5] = js.flags;
        usb.msg_send(&pkt)?;
        let resp = usb.msg_recv(32)?;
        let byte_len = js.data.len();
        js.data = resp[..byte_len.min(resp.len())].to_vec();
        Ok(())
    }

    fn write_jedec_row(&self, usb: &UsbDevice, device: &Device, js: &JedecSet) -> Result<()> {
        // Upstream tl866iiplus_write_jedec_row / t48_write_jedec_row
        // (identical): same header as read, row data at msg[8], 64-byte
        // packet, no response read.
        let mut pkt = [0u8; 64];
        pkt[0] = CMD_WRITE_JEDEC;
        pkt[1] = device.protocol_id;
        pkt[2] = (js.data.len() * 8).min(255) as u8;
        pkt[4] = js.row;
        pkt[5] = js.flags;
        let n = js.data.len().min(56);
        pkt[8..8 + n].copy_from_slice(&js.data[..n]);
        usb.msg_send(&pkt)
    }

    fn protect_off(&self, usb: &UsbDevice) -> Result<()> {
        // Upstream sends the command only — no response is read.
        let mut pkt = [0u8; 8];
        pkt[0] = CMD_PROTECT_OFF;
        usb.msg_send(&pkt)
    }

    fn protect_on(&self, usb: &UsbDevice) -> Result<()> {
        let mut pkt = [0u8; 8];
        pkt[0] = CMD_PROTECT_ON;
        usb.msg_send(&pkt)
    }

    fn get_ovc_status(&self, usb: &UsbDevice, _device: &Device) -> Result<(OvcStatus, u8)> {
        // Upstream tl866iiplus_get_ovc_status / t48_get_ovc_status
        // (identical): 32-byte response; error=msg[0], c1=LE16@2,
        // c2=LE16@4, address=LE32@8, ovc flag=msg[12].
        let mut pkt = [0u8; 8];
        pkt[0] = CMD_REQUEST_STATUS;
        usb.msg_send(&pkt)?;
        let resp = usb.msg_recv(32)?;
        // Upstream parses a zeroed 32-byte buffer — bytes beyond a short
        // response read as 0 rather than raising an error.
        let byte = |i: usize| resp.get(i).copied().unwrap_or(0);
        let le16_at = |i: usize| u32::from(u16::from_le_bytes([byte(i), byte(i + 1)]));
        let le32_at =
            |i: usize| u32::from_le_bytes([byte(i), byte(i + 1), byte(i + 2), byte(i + 3)]);
        let status = OvcStatus {
            error: byte(0),
            c1: le16_at(2),
            c2: le16_at(4),
            address: le32_at(8),
        };
        Ok((status, byte(12)))
    }

    fn unlock_tsop48(&self, usb: &UsbDevice) -> Result<u8> {
        // Upstream maps no unlock_tsop48 handler for the T48.
        if self.is_t48 {
            return Err(MiniproError::UnsupportedOperation);
        }
        // Upstream tl866iiplus_unlock_tsop48 sends a 48-byte packet:
        // 8 pseudo-random bytes at [8..16], a CRC16 of those bytes packed
        // into msg[10]/msg[12], and the original random bytes at [10]/[12]
        // echoed to msg[16]/msg[17] so the firmware can verify the CRC.
        let mut pkt = [0u8; 48];
        pkt[0] = CMD_UNLOCK_TSOP48;
        let mut seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() ^ (d.as_secs() as u32))
            .unwrap_or(0x9e37_79b9);
        let mut crc: u16 = 0;
        for b in pkt.iter_mut().take(16).skip(8) {
            // xorshift32 — same role as upstream's rand() fill.
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            *b = seed as u8;
            crc = crc.rotate_left(8);
            crc ^= u16::from(*b);
            crc ^= (crc & 0xFF) >> 4;
            crc ^= crc << 12;
            crc ^= (crc & 0xFF) << 5;
        }
        pkt[16] = pkt[10];
        pkt[17] = pkt[12];
        pkt[10] = crc as u8;
        pkt[12] = (crc >> 8) as u8;
        usb.msg_send(&pkt)?;
        let resp = usb.msg_recv(8)?;
        Ok(if resp.len() > 1 { resp[1] } else { 0 })
    }

    fn hardware_check(&self, usb: &UsbDevice) -> Result<()> {
        // The T48 has its own pin-driver layout and self-test protocol
        // (upstream t48_hardware_check) — a different implementation.
        if self.is_t48 {
            return super::t48::hardware_check(self, usb);
        }
        let mut msg = [0u8; 48];
        let mut errors: u32 = 0;

        // Testing 21 VPP pin drivers (no pull-up resistors)
        init_zif(usb, 1)?;

        for p in VPP_PINS {
            msg[8..48].fill(0);
            msg[0] = CMD_SET_VPP_PIN;
            msg[p.byte] = p.mask;
            usb.msg_send(&msg)?;

            std::thread::sleep(std::time::Duration::from_micros(5000));
            msg[0] = CMD_READ_PINS;
            usb.msg_send(&msg[..8])?;
            let read_buffer = usb.msg_recv(48)?;
            if read_buffer[1] != 0 {
                msg[0] = CMD_RESET_PIN_DRV;
                usb.msg_send(&msg[..8])?;
                crate::outln!(
                    "Overcurrent protection detected while testing VPP pin driver {}!\u{07}",
                    p.pin
                );
                return Err(MiniproError::Protocol(format!(
                    "VPP overcurrent on pin {}",
                    p.pin
                )));
            }
            if read_buffer[7 + p.pin as usize] == 0 {
                errors += 1;
            }
            crate::outln!(
                "VPP driver pin {} is {}",
                p.pin,
                if read_buffer[7 + p.pin as usize] != 0 {
                    "OK"
                } else {
                    "Bad"
                }
            );
        }
        crate::outln!();

        // Testing 32 VCC pin drivers (no pull-up resistors)
        init_zif(usb, 1)?;

        for p in VCC_PINS {
            msg[8..48].fill(0);
            msg[0] = CMD_SET_VCC_PIN;
            msg[p.byte] = p.mask;
            usb.msg_send(&msg)?;

            std::thread::sleep(std::time::Duration::from_micros(5000));
            msg[0] = CMD_READ_PINS;
            usb.msg_send(&msg[..8])?;
            let read_buffer = usb.msg_recv(48)?;
            if read_buffer[1] != 0 {
                msg[0] = CMD_RESET_PIN_DRV;
                usb.msg_send(&msg[..8])?;
                crate::outln!(
                    "Overcurrent protection detected while testing VCC pin driver {}!\u{07}",
                    p.pin
                );
                return Err(MiniproError::Protocol(format!(
                    "VCC overcurrent on pin {}",
                    p.pin
                )));
            }
            if read_buffer[7 + p.pin as usize] == 0 {
                errors += 1;
            }
            crate::outln!(
                "VCC driver pin {} is {}",
                p.pin,
                if read_buffer[7 + p.pin as usize] != 0 {
                    "OK"
                } else {
                    "Bad"
                }
            );
        }
        crate::outln!();

        // Testing 34 GND pin drivers (active pull-up resistors)
        init_zif(usb, 0)?;

        for p in GND_PINS {
            msg[8..48].fill(0);
            msg[0] = CMD_SET_GND_PIN;
            msg[p.byte] = p.mask;
            usb.msg_send(&msg)?;

            std::thread::sleep(std::time::Duration::from_micros(5000));
            msg[0] = CMD_READ_PINS;
            usb.msg_send(&msg[..8])?;
            let read_buffer = usb.msg_recv(48)?;
            if read_buffer[1] != 0 {
                msg[0] = CMD_RESET_PIN_DRV;
                usb.msg_send(&msg[..8])?;
                crate::outln!(
                    "Overcurrent protection detected while testing GND pin driver {}!\u{07}",
                    p.pin
                );
                return Err(MiniproError::Protocol(format!(
                    "GND overcurrent on pin {}",
                    p.pin
                )));
            }
            if read_buffer[7 + p.pin as usize] != 0 {
                errors += 1;
            }
            crate::outln!(
                "GND driver pin {} is {}",
                p.pin,
                if read_buffer[7 + p.pin as usize] != 0 {
                    "Bad"
                } else {
                    "OK"
                }
            );
        }
        crate::outln!();
        crate::outln!();

        // Testing VPP overcurrent protection
        init_zif(usb, 1)?;

        // Set VPP on pin 1
        msg[8..48].fill(0);
        msg[0] = CMD_SET_VPP_PIN;
        msg[VPP_PINS[VPP1].byte] = VPP_PINS[VPP1].mask;
        usb.msg_send(&msg)?;
        // Set GND also on pin 1
        msg[8..48].fill(0);
        msg[0] = CMD_SET_GND_PIN;
        msg[GND_PINS[GND1].byte] = GND_PINS[GND1].mask;
        usb.msg_send(&msg)?;
        // Reset pins
        msg[8..48].fill(0);
        msg[0] = CMD_SET_GND_PIN;
        usb.msg_send(&msg)?;

        msg[0] = CMD_READ_PINS;
        usb.msg_send(&msg[..8])?;
        let read_buffer = usb.msg_recv(48)?;
        if read_buffer[1] != 0 {
            crate::outln!("VPP overcurrent protection is OK.");
        } else {
            crate::outln!("VPP overcurrent protection failed!\u{07}");
            errors += 1;
        }

        // Testing VCC overcurrent protection
        init_zif(usb, 1)?;

        // Set VCC voltage to 4.5V
        msg[8..48].fill(0);
        msg[0] = CMD_SET_VCC_VOLTAGE;
        msg[8] = 0x01;
        usb.msg_send(&msg)?;
        // Set VCC on pin 1
        msg[8..48].fill(0);
        msg[0] = CMD_SET_VCC_PIN;
        msg[VCC_PINS[VCC1].byte] = VCC_PINS[VCC1].mask;
        usb.msg_send(&msg)?;
        // Set GND also on pin 1
        msg[8..48].fill(0);
        msg[0] = CMD_SET_GND_PIN;
        msg[GND_PINS[GND1].byte] = GND_PINS[GND1].mask;
        usb.msg_send(&msg)?;
        // Reset pins
        msg[8..48].fill(0);
        msg[0] = CMD_SET_GND_PIN;
        usb.msg_send(&msg)?;

        msg[0] = CMD_READ_PINS;
        usb.msg_send(&msg[..8])?;
        let read_buffer = usb.msg_recv(48)?;
        if read_buffer[1] != 0 {
            crate::outln!("VCC overcurrent protection is OK.");
        } else {
            crate::outln!("VCC overcurrent protection failed!\u{07}");
            errors += 1;
        }

        if errors != 0 {
            crate::outln!("\nHardware test completed with {} error(s).\u{07}", errors);
        } else {
            crate::outln!("\nHardware test completed successfully!");
        }

        // Reset pin drivers
        msg[0] = CMD_RESET_PIN_DRV;
        usb.msg_send(&msg[..8])?;
        Ok(())
    }

    fn pin_test(
        &self,
        usb: &UsbDevice,
        device: &Device,
        pin_map: &crate::database::PinMap,
    ) -> Result<crate::protocol::PinTestResult> {
        // Upstream maps no pin_test handler for the T48.
        if self.is_t48 {
            return pin_test_t48(self, usb, device, pin_map);
        }
        pin_test_tl866(usb, device, pin_map)
    }

    fn firmware_update(&self, usb: &UsbDevice, firmware: &[u8]) -> Result<()> {
        if self.is_t48 {
            // T48 firmware update requires USB reconnect (switch to
            // bootloader and back), so it's handled in operations.rs via
            // t48::firmware_update_t48 which takes &mut MiniproHandle.
            return Err(MiniproError::UnsupportedOperation);
        }
        firmware_update_tl866(usb, firmware)
    }

    fn logic_ic_test(&self, usb: &UsbDevice, device: &Device) -> Result<LogicTestResult> {
        logic_ic_test_tl866(usb, device)
    }

    fn spi_autodetect(&self, usb: &UsbDevice, id_type: u8) -> Result<u32> {
        let mut msg = [0u8; 64];
        msg[0] = CMD_AUTODETECT;
        msg[8] = id_type;
        usb.msg_send(&msg[..10])?;
        // Upstream receives 16 bytes on TL866II+, 32 on T48.
        let resp = usb.msg_recv(if self.is_t48 { 32 } else { 16 })?;
        if resp.len() < 5 {
            return Err(MiniproError::ResponseTooShort {
                expected: 5,
                actual: resp.len(),
            });
        }
        // Device ID is 3 bytes big-endian at resp[2..5]
        let id = ((resp[2] as u32) << 16) | ((resp[3] as u32) << 8) | resp[4] as u32;
        Ok(id)
    }

    fn reset_state(&self, usb: &UsbDevice) -> Result<()> {
        let mut pkt = [0u8; 8];
        pkt[0] = CMD_RESET_PIN_DRV;
        usb.msg_send(&pkt)?;
        Ok(())
    }

    fn set_zif_direction(&self, usb: &UsbDevice, zif: &[u8]) -> Result<()> {
        set_zif_direction_tl866(usb, zif)
    }

    fn set_zif_state(&self, usb: &UsbDevice, zif: &[u8]) -> Result<()> {
        let mut msg = [0u8; 48];
        msg[0] = CMD_SET_OUT;
        let n = zif.len().min(ZIF_PINS);
        msg[8..8 + n].copy_from_slice(&zif[..n]);
        usb.msg_send(&msg)
    }

    fn get_zif_state(&self, usb: &UsbDevice) -> Result<Vec<u8>> {
        let mut pkt = [0u8; 8];
        pkt[0] = CMD_READ_PINS;
        usb.msg_send(&pkt)?;
        let resp = usb.msg_recv(48)?;
        if resp.len() < 48 {
            return Err(MiniproError::ResponseTooShort {
                expected: 48,
                actual: resp.len(),
            });
        }
        Ok(resp[8..48].to_vec())
    }

    fn set_voltages(&self, usb: &UsbDevice, vcc: u8, vpp: u8) -> Result<()> {
        set_voltages_tl866(usb, vcc, vpp)
    }
}

// ── Packet builders ───────────────────────────────────────────────────────────

fn read_cmd(ds: &DataSet) -> [u8; 8] {
    // [cmd, 0, len_lo, len_hi, addr0, addr1, addr2, addr3]
    // msg[1] is left 0 — C code zero-initialises msg[64] and never sets [1].
    let cmd = match ds.page_type {
        MP_CODE => CMD_READ_CODE,
        MP_DATA => CMD_READ_DATA,
        MP_USER => CMD_READ_USER_DATA,
        _ => CMD_READ_CODE,
    };
    let len = ds.data.len() as u16;
    let mut pkt = [0u8; 8];
    pkt[0] = cmd;
    le16(&mut pkt[2..4], len);
    le32(&mut pkt[4..8], ds.address);
    pkt
}

fn write_cmd(ds: &DataSet) -> [u8; 8] {
    // [cmd, 0, len_lo, len_hi, addr0, addr1, addr2, addr3]
    // msg[1] is left 0 — C code zero-initialises msg[64] and never sets [1].
    let cmd = match ds.page_type {
        MP_CODE => CMD_WRITE_CODE,
        MP_DATA => CMD_WRITE_DATA,
        MP_USER => CMD_WRITE_USER_DATA,
        _ => CMD_WRITE_CODE,
    };
    let len = ds.data.len() as u16;
    let mut pkt = [0u8; 8];
    pkt[0] = cmd;
    le16(&mut pkt[2..4], len);
    le32(&mut pkt[4..8], ds.address);
    pkt
}

// ── Little-endian helpers ─────────────────────────────────────────────────────

fn le16(buf: &mut [u8], val: u16) {
    buf[0] = (val & 0xff) as u8;
    buf[1] = (val >> 8) as u8;
}

fn le32(buf: &mut [u8], val: u32) {
    buf[0] = (val & 0xff) as u8;
    buf[1] = ((val >> 8) & 0xff) as u8;
    buf[2] = ((val >> 16) & 0xff) as u8;
    buf[3] = (val >> 24) as u8;
}

// ── ZIF pin control ───────────────────────────────────────────────────────────

/// Set ZIF pin directions (and pull-up resistors).
///
/// Each byte in `zif[0..40]`:
///  - bits [6:0] → direction (0 = output, 1 = input; passed to SET_DIR)
///  - bit 7      → pull-up enable (0 = pull-up, 1 = no pull-up; inverted for SET_PULLUPS)
fn set_zif_direction_tl866(usb: &UsbDevice, zif: &[u8]) -> Result<()> {
    let n = zif.len().min(ZIF_PINS);

    // SET_DIR packet
    let mut msg = [0u8; 48];
    msg[0] = CMD_SET_DIR;
    for i in 0..n {
        msg[8 + i] = zif[i] & 0x7f;
    }
    usb.msg_send(&msg)?;

    // SET_PULLUPS packet (bit7 of zif: 0 → pull-up active → send 0x00, 1 → no pull-up → send 0x01)
    let mut msg2 = [0u8; 48];
    msg2[0] = CMD_SET_PULLUPS;
    for i in 0..n {
        msg2[8 + i] = if zif[i] & 0x80 != 0 { 0x00 } else { 0x01 };
    }
    usb.msg_send(&msg2)?;
    Ok(())
}

/// Encode VCC / VPP indices through the hardware lookup tables and send to
/// the programmer.
///
/// Lookup tables from tl866iiplus.c (`vcc_t[]` and `vpp_t[]`):
fn set_voltages_tl866(usb: &UsbDevice, vcc: u8, vpp: u8) -> Result<()> {
    const VCC_T: [u8; 16] = [0, 1, 2, 4, 3, 5, 6, 8, 7, 9, 10, 12, 11, 13, 14, 15];
    const VPP_T: [u8; 16] = [0, 8, 1, 9, 2, 10, 3, 4, 11, 12, 5, 13, 6, 14, 7, 15];

    let vi = (vcc as usize).min(15);
    let pi = (vpp as usize).min(15);
    let vcc_enc = VCC_T[vi];
    let vpp_enc = VPP_T[pi];

    // SET_VCC_VOLTAGE: bits [0], [1], [2] and a high bit packed into msg[8..12]
    let mut msg = [0u8; 48];
    msg[0] = CMD_SET_VCC_VOLTAGE;
    msg[8] = vcc_enc & 0x01;
    msg[9] = (vcc_enc >> 1) & 0x01;
    msg[10] = (vcc_enc >> 2) & 0x01;
    msg[11] = (vcc_enc << 4) & 0x80;
    usb.msg_send(&msg)?;

    // SET_VPP_VOLTAGE
    let mut msg2 = [0u8; 48];
    msg2[0] = CMD_SET_VPP_VOLTAGE;
    msg2[8] = vpp_enc & 0x01;
    msg2[9] = (vpp_enc >> 1) & 0x01;
    msg2[10] = (vpp_enc >> 2) & 0x01;
    msg2[11] = (vpp_enc << 4) & 0x80;
    usb.msg_send(&msg2)?;
    Ok(())
}

// ── Logic IC test ─────────────────────────────────────────────────────────────

/// Run a single logic-IC test pass (pull = 0 → pull-ups active, 1 → pull-downs active).
///
/// Returns a flat buffer of `vector_count * pin_count` nibble values (one per pin per
/// vector), each holding the firmware-reported logic level (0 = low, non-zero = high).
/// Validate that the device has test vectors; returns
/// `(vectors, pin_count, vec_count)`.
pub(crate) fn logic_test_params(device: &Device) -> Result<(&[u8], usize, usize)> {
    let vectors = match device.vectors.as_deref() {
        Some(v) if !v.is_empty() => v,
        _ => {
            return Err(MiniproError::Protocol(
                "no test vectors for this device".into(),
            ))
        }
    };
    let pin_count = device.package_details.pin_count as usize;
    let vec_count = device.vector_count;
    if vec_count == 0 || pin_count == 0 {
        return Err(MiniproError::Protocol(
            "no test vectors for this device".into(),
        ));
    }
    Ok((vectors, pin_count, vec_count))
}

/// Count mismatches across the two passes and build the result struct.
/// `logic_test_params` must have been called first (validates vectors exist).
pub(crate) fn build_logic_test_result(
    device: &Device,
    step1: Vec<u8>,
    step2: Vec<u8>,
) -> LogicTestResult {
    let vectors = device.vectors.as_deref().unwrap_or(&[]);
    let pin_count = device.package_details.pin_count as usize;
    let vec_count = device.vector_count;

    let mut errors = 0u32;
    for v in 0..vec_count {
        for p in 0..pin_count {
            let idx = v * pin_count + p;
            let state = vectors[idx];
            let s1 = step1[idx];
            let s2 = step2[idx];
            let err = match state {
                LOGIC_L => s1 != 0 || s2 != 0,
                LOGIC_H => s1 == 0 || s2 == 0,
                LOGIC_Z => s1 == 0 || s2 != 0,
                _ => false,
            };
            if err {
                errors += 1;
            }
        }
    }

    let pass = errors == 0;
    LogicTestResult {
        pin_count: pin_count as u16,
        vector_count: vec_count as u16,
        vectors: vectors.to_vec(),
        step1,
        step2,
        errors,
        pass,
    }
}

pub(crate) fn do_ic_test_pass(usb: &UsbDevice, device: &Device, pull: bool) -> Result<Vec<u8>> {
    let vectors = device.vectors.as_deref().unwrap_or(&[]);
    let pin_count = device.package_details.pin_count as usize;
    let vec_count = device.vector_count;

    let mut result = vec![0u8; vec_count * pin_count];

    for n in 0..vec_count {
        // Initialise message to 0xff (important: unpopulated nibbles stay 0xf)
        let mut msg = [0xffu8; 32];
        msg[0] = CMD_LOGIC_IC_TEST;
        msg[1] = device.voltages.vcc | ((pull as u8) << 7);
        msg[2] = pin_count as u8;
        msg[3] = (pin_count >> 8) as u8;
        msg[4] = (n & 0xff) as u8;
        msg[5] = ((n >> 8) & 0xff) as u8;
        msg[6] = ((n >> 16) & 0xff) as u8;
        msg[7] = ((n >> 24) & 0xff) as u8;

        // Pack vector: 2 pin states per byte (low nibble = even pin, high nibble = odd pin)
        for i in 0..pin_count {
            let v = vectors[n * pin_count + i];
            if i & 1 != 0 {
                msg[8 + i / 2] |= v << 4;
            } else {
                msg[8 + i / 2] = v;
            }
        }

        usb.msg_send(&msg)?;
        let resp = usb.msg_recv(32)?;
        if resp.len() < 9 {
            return Err(MiniproError::ResponseTooShort {
                expected: 9,
                actual: resp.len(),
            });
        }

        // Overcurrent: firmware sets msg[1] != 0 on OVC
        if resp[1] != 0 {
            return Err(MiniproError::Overcurrent { address: 0 });
        }

        // Unpack result nibbles
        for i in 0..pin_count {
            result[n * pin_count + i] = (resp[8 + i / 2] >> (4 * (i & 1))) & 0xf;
        }
    }
    Ok(result)
}

/// Run the full two-pass logic-IC test and print a pass/fail table.
pub(super) fn logic_ic_test_tl866(usb: &UsbDevice, device: &Device) -> Result<LogicTestResult> {
    logic_test_params(device)?;
    // Step 1: pull-ups active (pull = 0)
    let step1 = do_ic_test_pass(usb, device, false)?;
    // Step 2: pull-downs active (pull = 1)
    let step2 = do_ic_test_pass(usb, device, true)?;
    Ok(build_logic_test_result(device, step1, step2))
}

// ── Firmware update (TL866II+ / T48) ─────────────────────────────────────────

/// Parse and flash an UpdateII.dat firmware image.
///
/// File layout (from tl866iiplus.c):
/// ```text
/// [0..4]      version  (LE32)   must have (version & 0xffff0000) == 0xf8cc0000
/// [4..8]      file CRC (LE32)   must equal ~crc32(covered regions)
/// [8..1032]   XOR table (1024 bytes)
/// Pin-contact test shared by TL866II+ and T76.
///
/// Mirrors `tl866iiplus_pin_test` from the upstream C source.
/// Returns `Ok(PinTestResult)` with a list of bad device pin numbers
/// (empty if all pins contacted successfully).
pub(super) fn pin_test_tl866(
    usb: &UsbDevice,
    device: &Device,
    pin_map: &crate::database::PinMap,
) -> Result<crate::protocol::PinTestResult> {
    let p_pins: usize = ZIF_PINS; // 40 ZIF positions
    let d_pins = device.package_details.pin_count as usize;
    let x_pin = d_pins / 2; // split: left / right halves
    let pno = p_pins - d_pins; // pin offset for right-half mapping

    let mut msg = [0u8; 48];
    let mut pins = [0u8; 40];

    // ── Step 1: SET_DIR – all INPUT, GND pins OUTPUT ──────────────────────
    msg[0] = CMD_SET_DIR;
    msg[8..48].fill(0x01);
    for &gnd_pin in &pin_map.gnd_table {
        let idx = gnd_pin as usize + 7;
        if idx < 48 {
            msg[idx] = 0x00;
        }
    }
    usb.msg_send(&msg)?;

    // ── Step 2: SET_OUT – all outputs HIGH ────────────────────────────────
    msg[0] = CMD_SET_OUT;
    msg[8..48].fill(0x01);
    usb.msg_send(&msg)?;

    // ── Step 3: PULLUPS – right side active (0x00), left inactive (0x01) ─
    msg[0] = CMD_SET_PULLUPS;
    // msg[8..28] already 0x01 (left = inactive)
    msg[28..48].fill(0x00); // right = active
    usb.msg_send(&msg)?;

    // ── Step 4: PULLDOWNS – left side active (0x00), right inactive ───────
    msg[0] = CMD_SET_PULLDOWNS;
    msg[8..28].fill(0x00); // left = active
    msg[28..48].fill(0x01); // right = inactive
    usb.msg_send(&msg)?;

    // ── Step 5: READ_PINS – left half (ZIF pins 1-20) ────────────────────
    msg[0] = CMD_READ_PINS;
    usb.msg_send(&msg[..8])?;
    let resp = usb.msg_recv(48)?;
    if resp.len() >= 28 {
        pins[..20].copy_from_slice(&resp[8..28]);
    }

    // ── Step 6: PULLUPS – left side active, right inactive ────────────────
    msg[0] = CMD_SET_PULLUPS;
    msg[8..28].fill(0x00); // left = active
    msg[28..48].fill(0x01); // right = inactive
    usb.msg_send(&msg)?;

    // ── Step 7: PULLDOWNS – right side active, left inactive ──────────────
    msg[0] = CMD_SET_PULLDOWNS;
    msg[8..28].fill(0x01); // left = inactive
    msg[28..48].fill(0x00); // right = active
    usb.msg_send(&msg)?;

    // ── Step 8: READ_PINS – right half (ZIF pins 21-40) ──────────────────
    msg[0] = CMD_READ_PINS;
    usb.msg_send(&msg[..8])?;
    let resp = usb.msg_recv(48)?;
    if resp.len() >= 48 {
        pins[20..40].copy_from_slice(&resp[28..48]);
    }

    // ── Steps 9-12: Reset ZIF state ───────────────────────────────────────
    msg[0] = CMD_SET_OUT;
    msg[8..48].fill(0x00);
    usb.msg_send(&msg)?;

    msg[0] = CMD_SET_DIR;
    msg[8..48].fill(0x01);
    usb.msg_send(&msg)?;

    msg[0] = CMD_SET_PULLUPS;
    msg[8..48].fill(0x01);
    usb.msg_send(&msg)?;

    msg[0] = CMD_SET_PULLDOWNS;
    msg[8..48].fill(0x00);
    usb.msg_send(&msg)?;

    // ── Step 13: End ZIF transaction ──────────────────────────────────────
    let mut end = [0u8; 8];
    end[0] = CMD_END_TRANS;
    usb.msg_send(&end)?;

    // ── Step 14: Check contact for each masked pin ────────────────────────
    let mut bad_pins = Vec::new();
    let mut expected_pins = Vec::new();
    for &p_pin in &pin_map.mask {
        let p = p_pin as usize;
        if !(1..=40).contains(&p) {
            continue;
        }
        let d_pin = if p > x_pin { p - pno } else { p };
        expected_pins.push(d_pin as u16);
        if pins[p - 1] == 0 {
            bad_pins.push(d_pin as u16);
        }
    }

    Ok(crate::protocol::PinTestResult {
        bad_pins,
        expected_pins,
        untestable_pins: gnd_device_pins(&pin_map.gnd_table, x_pin, pno),
    })
}

/// Translate ZIF-pin `<gnd>` entries to device-pin numbers with the same
/// mapping the mask loop uses — pins > `x_pin` sit `pno` lower on the
/// right side of the socket.
pub(crate) fn gnd_device_pins(gnd_table: &[u16], x_pin: usize, pno: usize) -> Vec<u16> {
    gnd_table
        .iter()
        .filter(|&&p| (1..=40).contains(&(p as usize)))
        .map(|&p| {
            let p = p as usize;
            (if p > x_pin { p - pno } else { p }) as u16
        })
        .collect()
}

/// T48 pin-contact check (`0x3E`) — a single command returning a per-pin
/// contact bitmap; no bit-banging like the TL866II+ path (upstream never
/// implemented it; protocol per proghq USB captures).
///
/// ```text
/// send 3E 00 <chip_id_lo> <chip_id_hi> 00 00 00 00
/// recv 3E 00 xx xx xx xx xx xx b0 b1 b2 b3 b4 b5 b6 00
/// ```
///
/// `b0..b4` is the ZIF1–ZIF40 contact bitmap (LSB-first, bit `p-1` of
/// `resp[8 + (p-1)/8]`); `b5..b6` is the ISP1–ISP16 bitmap (unused — this
/// surface is ZIF-mode only). A set bit means contact; every
/// `pin_map.mask` pin whose bit is clear is reported bad. The request's
/// `chip_id` field is ignored by the firmware (the response carries
/// constant fields, not an echo) — `variant` is sent per the captures.
///
/// The bitmap is only valid with the pin drivers idle: inside a
/// transaction, configured/driven pins read as open (hardware-verified —
/// a fully seated DIP-8 reads `0F 00 00 00 F0` idle but `01 00 00 00 C0`
/// in-transaction). XGPro runs pin detect before the transaction begins;
/// callers run inside one, so end it, measure, then re-begin to leave the
/// caller's state untouched.
fn pin_test_t48(
    proto: &Tl866iiPlusProtocol,
    usb: &UsbDevice,
    device: &Device,
    pin_map: &crate::database::PinMap,
) -> Result<crate::protocol::PinTestResult> {
    const T48_PIN_DETECT: u8 = 0x3E;

    proto.end_transaction(usb)?;

    let mut msg = [0u8; 8];
    msg[0] = T48_PIN_DETECT;
    msg[2..4].copy_from_slice(&(device.variant as u16).to_le_bytes());
    usb.msg_send(&msg)?;

    let resp = usb.msg_recv(16)?;
    proto.begin_transaction(usb, device, 0)?;

    if resp.first().copied() != Some(T48_PIN_DETECT) || resp.len() < 15 {
        return Err(MiniproError::Protocol("T48 pin detect failed".into()));
    }

    let d_pins = device.package_details.pin_count as usize;
    let x_pin = d_pins / 2;
    let pno = ZIF_PINS - d_pins;

    let mut bad_pins = Vec::new();
    let mut expected_pins = Vec::new();
    for &p_pin in &pin_map.mask {
        let p = p_pin as usize;
        if !(1..=40).contains(&p) {
            continue;
        }
        let d_pin = if p > x_pin { p - pno } else { p };
        expected_pins.push(d_pin as u16);
        if resp[8 + (p - 1) / 8] & (1 << ((p - 1) & 7)) == 0 {
            bad_pins.push(d_pin as u16);
        }
    }

    Ok(crate::protocol::PinTestResult {
        bad_pins,
        expected_pins,
        untestable_pins: gnd_device_pins(&pin_map.gnd_table, x_pin, pno),
    })
}

pub(super) fn firmware_update_tl866(usb: &UsbDevice, dat: &[u8]) -> Result<()> {
    use crc::{Crc, CRC_32_ISO_HDLC};
    const CRC32: Crc<u32> = Crc::<u32>::new(&CRC_32_ISO_HDLC);

    // ── Parse header ─────────────────────────────────────────────────────────
    if dat.len() < 1036 {
        return Err(MiniproError::FileFormat("UpdateII.dat too short".into()));
    }
    let version = u32::from_le_bytes([dat[0], dat[1], dat[2], dat[3]]);
    let file_crc = u32::from_le_bytes([dat[4], dat[5], dat[6], dat[7]]);

    if version & UPDATE_FILE_VERS_MASK != UPDATE_FILE_VERSION {
        return Err(MiniproError::FileFormat(format!(
            "Unsupported firmware version {:#010x}",
            version
        )));
    }

    let n_blocks = u32::from_le_bytes([dat[1032], dat[1033], dat[1034], dat[1035]]) as usize;
    let expected_size = n_blocks * 272 + 3100;
    if dat.len() != expected_size {
        return Err(MiniproError::FileFormat(format!(
            "UpdateII.dat wrong size: got {}, expected {}",
            dat.len(),
            expected_size
        )));
    }

    // XOR table lives at bytes 8..1032
    let xortable = &dat[8..1032];

    // Offsets of each section
    let blocks_start = 1036usize;
    let last_blk_off = blocks_start + n_blocks * 272;

    // ── CRC check ────────────────────────────────────────────────────────────
    // CRC is computed over: regular blocks, last block, then (xortable + blocks_count field).
    // Result inverted (~crc) must equal file_crc.
    let mut digest = CRC32.digest();
    digest.update(&dat[blocks_start..blocks_start + n_blocks * 272]);
    digest.update(&dat[last_blk_off..last_blk_off + 2064]);
    digest.update(&dat[8..1036]); // xortable (1024 B) + blocks_count (4 B)
    let computed = digest.finalize();
    if computed != file_crc {
        return Err(MiniproError::AlgorithmCrc);
    }

    crate::outln!("Firmware image OK ({} blocks + last block).", n_blocks);

    // ── Switch to bootloader ─────────────────────────────────────────────────
    {
        let mut msg = [0u8; 8];
        msg[0] = CMD_SWITCH;
        le32(&mut msg[4..8], BTLDR_MAGIC);
        let _ = usb.msg_send(&msg); // best-effort; device reboots
    }
    // The device re-enumerates; we can't reopen here (no handle to the UsbDevice
    // internals), so we rely on the existing open connection (some firmware versions
    // keep the EP alive during the brief bootloader switch).

    // ── Erase ────────────────────────────────────────────────────────────────
    {
        let mut msg = [0u8; 8];
        msg[0] = CMD_BTLDR_ERASE;
        usb.msg_send(&msg)?;
        let resp = usb.msg_recv(8)?;
        if resp.first().copied() != Some(CMD_BTLDR_ERASE) {
            return Err(MiniproError::Protocol("bootloader erase rejected".into()));
        }
    }

    // ── Flash regular blocks ──────────────────────────────────────────────────
    for b in 0..n_blocks {
        let blk_off = blocks_start + b * 272;
        let blk = &dat[blk_off..blk_off + 272];

        // Block layout: [0..4] block_crc, [4..8] xorptr, [8..12] dest_addr,
        //               [12..16] internal_ptr, [16..272] data (256 bytes)
        let xorptr = u32::from_le_bytes([blk[4], blk[5], blk[6], blk[7]]) as usize;
        let addr = u32::from_le_bytes([blk[8], blk[9], blk[10], blk[11]]);

        // Deobfuscate: XOR each of the 264 bytes at blk[8..272] with the XOR table.
        // The table is 1024 bytes, indexed as (xorptr-1 + i) & 0x3FF (1-indexed).
        let mut data = [0u8; 256];
        for i in 0..256 {
            let tbl_idx = (xorptr.wrapping_add(i).wrapping_sub(1)) & 0x3FF;
            data[i] = blk[16 + i] ^ xortable[tbl_idx];
        }

        // Send write command
        let mut hdr = [0u8; 8];
        hdr[0] = CMD_BTLDR_WRITE;
        hdr[1] = ((xorptr >> 1) & 0x7F) as u8; // xor_ptr_idx & 0x7F
        hdr[2] = 0;
        hdr[3] = 1; // block type 1 = regular
        le32(&mut hdr[4..8], addr);
        usb.msg_send(&hdr)?;
        usb.write_payload(&data)?;

        // Check status
        let mut sreq = [0u8; 8];
        sreq[0] = CMD_REQUEST_STATUS;
        usb.msg_send(&sreq)?;
        let sr = usb.msg_recv(32)?;
        if sr.get(1).copied().unwrap_or(1) != 0 {
            return Err(MiniproError::Protocol(format!(
                "block {} flash status error: {:#04x}",
                b,
                sr.get(1).copied().unwrap_or(0xff)
            )));
        }
    }

    // ── Flash last block ──────────────────────────────────────────────────────
    {
        let lb = &dat[last_blk_off..last_blk_off + 2064];
        let xorptr = u32::from_le_bytes([lb[4], lb[5], lb[6], lb[7]]) as usize;
        let addr = u32::from_le_bytes([lb[8], lb[9], lb[10], lb[11]]);

        // Deobfuscate last block: 514 iterations × 4 XOR ops on 2056 bytes (lb[8..2064])
        let mut data = [0u8; 2048];
        for i in 0..2048 {
            let tbl_idx = (xorptr.wrapping_add(i).wrapping_sub(1)) & 0x3FF;
            data[i] = lb[16 + i] ^ xortable[tbl_idx];
        }

        let mut hdr = [0u8; 8];
        hdr[0] = CMD_BTLDR_WRITE;
        hdr[1] = (((xorptr >> 1) & 0x7F) | 0x80) as u8; // xor_ptr_idx | 0x80 (last-block flag)
        hdr[2] = 0;
        hdr[3] = 8; // block type 8 = last block
        le32(&mut hdr[4..8], addr);
        usb.msg_send(&hdr)?;
        usb.write_payload(&data)?;

        let mut sreq = [0u8; 8];
        sreq[0] = CMD_REQUEST_STATUS;
        usb.msg_send(&sreq)?;
        let sr = usb.msg_recv(32)?;
        if sr.get(1).copied().unwrap_or(1) != 0 {
            return Err(MiniproError::Protocol(format!(
                "last block flash status error: {:#04x}",
                sr.get(1).copied().unwrap_or(0xff)
            )));
        }
    }

    crate::outln!("Firmware written successfully. Please reconnect the programmer.");
    Ok(())
}
