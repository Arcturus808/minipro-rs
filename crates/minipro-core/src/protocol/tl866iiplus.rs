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
//! | 0x3C | Hardware check               |

use super::{DataSet, JedecSet, OvcStatus, Protocol};
use crate::{
    device::Device,
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
const CMD_SET_VCC_VOLTAGE: u8 = 0x1B;
const CMD_SET_VPP_VOLTAGE: u8 = 0x1C;
const CMD_LOGIC_IC_TEST: u8 = 0x28;
const CMD_RESET_PIN_DRV: u8 = 0x2D;
#[allow(dead_code)]
const CMD_SET_VCC_PIN: u8 = 0x2E;
#[allow(dead_code)]
const CMD_SET_VPP_PIN: u8 = 0x2F;
#[allow(dead_code)]
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
const CMD_HARDWARE_CHECK: u8 = 0x3C; // same byte as BTLDR_ERASE, context-dependent
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

/// Printable character for each logic state (matches C `pst[]`).
const PSTATE: &[u8] = b"01LHCZXGV";

// Memory page types
const MP_CODE: u8 = 0x00;
const MP_DATA: u8 = 0x01;
const MP_USER: u8 = 0x02;

// Minimum firmware version expected
pub const MIN_FIRMWARE: u32 = 0x255; // 4.2.85

pub struct Tl866iiPlusProtocol;

impl Tl866iiPlusProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Tl866iiPlusProtocol {
    fn default() -> Self {
        Self::new()
    }
}

// ── System info ──────────────────────────────────────────────────────────────

/// Parse the 64-byte system-info response.
///
/// Layout (from tl866iiplus.md):
/// ```text
/// u8   echo (0x00)
/// u8   device_status
/// u16  report_size
/// u8   firmware_minor
/// u8   firmware_major
/// u16  device_version   (5 = TL866II+, 7 = T48, 6 = T56, 8 = T76)
/// u8[8]  device_code
/// u8[24] serial_number
/// u8   hardware_version
/// ```
pub fn get_system_info(usb: &UsbDevice) -> Result<SystemInfo> {
    let mut cmd = [0u8; 64];
    cmd[0] = CMD_GET_INFO;
    usb.msg_send(&cmd)?;
    let resp = usb.msg_recv(64)?;

    if resp.len() < 41 {
        return Err(MiniproError::ResponseTooShort {
            expected: 41,
            actual: resp.len(),
        });
    }

    let status = match resp[1] {
        1 => crate::device::ProgrammerStatus::Normal,
        2 => crate::device::ProgrammerStatus::Bootloader,
        _ => crate::device::ProgrammerStatus::Normal,
    };

    let fw_minor = resp[4];
    let fw_major = resp[5];
    let firmware = ((fw_major as u32) << 8) | fw_minor as u32;

    let device_version = u16::from_le_bytes([resp[6], resp[7]]);
    let model = match device_version {
        5 => crate::device::ProgrammerModel::Tl866iiPlus,
        6 => crate::device::ProgrammerModel::T56,
        7 => crate::device::ProgrammerModel::T48,
        8 => crate::device::ProgrammerModel::T76,
        _ => crate::device::ProgrammerModel::Tl866iiPlus,
    };

    let device_code = String::from_utf8_lossy(&resp[8..16])
        .trim_end_matches('\0')
        .to_string();
    let serial_number = String::from_utf8_lossy(&resp[16..40])
        .trim_end_matches('\0')
        .to_string();
    let hardware_version = resp[40];

    let firmware_str = format!("{:02}.{}.{}", fw_major / 10, fw_major % 10, fw_minor);

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
    fn begin_transaction(&self, usb: &UsbDevice, device: &Device, icsp: bool) -> Result<()> {
        // 64-byte begin_transaction packet (from tl866iiplus.md):
        // [0]  cmd = 0x03
        // [1]  protocol
        // [2]  variant
        // [3]  icsp
        // [4]  unknown
        // [5-6] opts1 (voltages / config flags)
        // [7]  unknown
        // [8-9]  data_memory_size  (16-bit LE)
        // [10-11] opts2
        // [12-13] opts3
        // [14-15] data_memory2_size
        // [16-19] code_memory_size (32-bit LE)
        // [20-39] zeroes
        // [40-43] package_details
        // [44-63] zeroes
        let mut pkt = [0u8; 64];
        pkt[0] = CMD_BEGIN_TRANS;
        pkt[1] = device.protocol_id;
        pkt[2] = (device.variant & 0xff) as u8;
        pkt[3] = icsp as u8;
        // opts1: vpp | vcc encoded
        let opts1: u16 = ((device.voltages.vcc as u16) << 4) | device.voltages.vpp as u16;
        le16(&mut pkt[5..7], opts1);
        le16(&mut pkt[8..10], (device.data_memory_size & 0xffff) as u16);
        le16(&mut pkt[14..16], (device.data_memory2_size & 0xffff) as u16);
        le32(&mut pkt[16..20], device.code_memory_size);
        le32(&mut pkt[40..44], device.package_details.raw);
        usb.msg_send(&pkt)
    }

    fn end_transaction(&self, usb: &UsbDevice) -> Result<()> {
        let mut pkt = [0u8; 8];
        pkt[0] = CMD_END_TRANS;
        usb.msg_send(&pkt)
    }

    fn read_block(&self, usb: &UsbDevice, ds: &mut DataSet) -> Result<()> {
        // 8-byte read command: [cmd, block_count, length_lo, length_hi, addr x4]
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
        trace!(
            "read_block: cmd sent, awaiting {} bytes on EP 0x82",
            ds.data.len()
        );
        // Use single-EP2 read: pass length as both `length` and `limit` so
        // read_payload_limit takes the `length <= limit` branch (EP2 only).
        // The dual-EP interleaved path is for MCU flash writes, not SPI reads.
        ds.data = usb.read_payload_limit(ds.data.len(), ds.data.len())?;
        trace!("read_block: got {} bytes", ds.data.len());
        Ok(())
    }

    fn write_block(&self, usb: &UsbDevice, ds: &DataSet) -> Result<()> {
        let cmd = write_cmd(ds);
        usb.msg_send(&cmd)?;
        usb.write_payload(&ds.data)?;
        // Read back 64-byte status response
        let resp = usb.msg_recv(64)?;
        // Status byte 1 should be 0 on success
        if resp.len() > 1 && resp[1] != 0 {
            return Err(MiniproError::Protocol(format!(
                "write_block status error: {:#04x}",
                resp[1]
            )));
        }
        Ok(())
    }

    fn get_chip_id(&self, usb: &UsbDevice) -> Result<(u8, u32)> {
        let mut pkt = [0u8; 8];
        pkt[0] = CMD_READ_CHIP_ID;
        usb.msg_send(&pkt)?;
        let resp = usb.msg_recv(64)?;
        if resp.len() < 6 {
            return Err(MiniproError::ResponseTooShort {
                expected: 6,
                actual: resp.len(),
            });
        }
        let id_type = resp[1];
        let chip_id = u32::from_le_bytes([resp[2], resp[3], resp[4], resp[5]]);
        Ok((id_type, chip_id))
    }

    fn read_fuses(
        &self,
        usb: &UsbDevice,
        _device: &Device,
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
        let mut pkt = [0u8; 8];
        pkt[0] = cmd_byte;
        pkt[1] = length as u8;
        pkt[2] = items_count;
        usb.msg_send(&pkt)?;
        usb.msg_recv(64)
    }

    fn write_fuses(
        &self,
        usb: &UsbDevice,
        _device: &Device,
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
        let mut pkt = vec![0u8; 64];
        pkt[0] = cmd_byte;
        pkt[1] = length as u8;
        pkt[2] = items_count;
        let copy_len = data.len().min(61);
        pkt[3..3 + copy_len].copy_from_slice(&data[..copy_len]);
        usb.msg_send(&pkt)?;
        usb.msg_recv(64)?;
        Ok(())
    }

    fn read_calibration(&self, usb: &UsbDevice, size: usize) -> Result<Vec<u8>> {
        let mut pkt = [0u8; 8];
        pkt[0] = CMD_READ_CALIB;
        pkt[1] = size as u8;
        usb.msg_send(&pkt)?;
        usb.msg_recv(64)
    }

    fn erase(&self, usb: &UsbDevice, num_fuses: u8, is_pld: bool) -> Result<()> {
        let mut pkt = [0u8; 64];
        pkt[0] = CMD_ERASE;
        pkt[1] = num_fuses;
        pkt[2] = is_pld as u8;
        usb.msg_send(&pkt)?;
        usb.msg_recv(64)?;
        Ok(())
    }

    fn read_jedec_row(&self, usb: &UsbDevice, js: &mut JedecSet) -> Result<()> {
        let mut pkt = [0u8; 8];
        pkt[0] = 0x0A; // read data2 payload
        pkt[1] = js.row;
        pkt[2] = js.flags;
        pkt[3] = js.set_type;
        usb.msg_send(&pkt)?;
        js.data = usb.read_payload_limit(js.data.len(), js.data.len())?;
        Ok(())
    }

    fn write_jedec_row(&self, usb: &UsbDevice, js: &JedecSet) -> Result<()> {
        let mut pkt = [0u8; 8];
        pkt[0] = 0x0B; // write data2 payload
        pkt[1] = js.row;
        pkt[2] = js.flags;
        pkt[3] = js.set_type;
        usb.msg_send(&pkt)?;
        usb.write_payload(&js.data)?;
        usb.msg_recv(64)?;
        Ok(())
    }

    fn protect_off(&self, usb: &UsbDevice) -> Result<()> {
        let mut pkt = [0u8; 8];
        pkt[0] = CMD_PROTECT_OFF;
        usb.msg_send(&pkt)?;
        usb.msg_recv(64)?;
        Ok(())
    }

    fn protect_on(&self, usb: &UsbDevice) -> Result<()> {
        let mut pkt = [0u8; 8];
        pkt[0] = CMD_PROTECT_ON;
        usb.msg_send(&pkt)?;
        usb.msg_recv(64)?;
        Ok(())
    }

    fn get_ovc_status(&self, usb: &UsbDevice) -> Result<(OvcStatus, u8)> {
        let mut pkt = [0u8; 8];
        pkt[0] = CMD_REQUEST_STATUS;
        usb.msg_send(&pkt)?;
        let resp = usb.msg_recv(64)?;
        if resp.len() < 8 {
            return Err(MiniproError::ResponseTooShort {
                expected: 8,
                actual: resp.len(),
            });
        }
        let ovc_flag = resp[1];
        let status = OvcStatus {
            error: resp[0],
            address: u32::from_le_bytes([resp[2], resp[3], resp[4], resp[5]]),
            c1: resp[6] as u32,
            c2: resp[7] as u32,
        };
        Ok((status, ovc_flag))
    }

    fn unlock_tsop48(&self, usb: &UsbDevice) -> Result<u8> {
        let mut pkt = [0u8; 8];
        pkt[0] = CMD_UNLOCK_TSOP48;
        usb.msg_send(&pkt)?;
        let resp = usb.msg_recv(64)?;
        Ok(if resp.len() > 1 { resp[1] } else { 0 })
    }

    fn hardware_check(&self, usb: &UsbDevice) -> Result<()> {
        let mut pkt = [0u8; 8];
        pkt[0] = CMD_HARDWARE_CHECK;
        usb.msg_send(&pkt)?;
        usb.msg_recv(64)?;
        Ok(())
    }

    fn pin_test(
        &self,
        usb: &UsbDevice,
        device: &Device,
        pin_map: &crate::database::PinMap,
    ) -> Result<()> {
        pin_test_tl866(usb, device, pin_map)
    }

    fn firmware_update(&self, usb: &UsbDevice, firmware: &[u8]) -> Result<()> {
        firmware_update_tl866(usb, firmware)
    }

    fn logic_ic_test(
        &self,
        usb: &UsbDevice,
        device: &Device,
        out: &mut dyn std::io::Write,
    ) -> Result<()> {
        logic_ic_test_tl866(usb, device, out)
    }

    fn spi_autodetect(&self, usb: &UsbDevice, id_type: u8) -> Result<u32> {
        let mut msg = [0u8; 64];
        msg[0] = CMD_AUTODETECT;
        msg[8] = id_type;
        usb.msg_send(&msg[..10])?;
        let resp = usb.msg_recv(16)?;
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
    // [cmd, protocol, len_lo, len_hi, addr0, addr1, addr2, addr3]
    let cmd = match ds.page_type {
        MP_CODE => CMD_READ_CODE,
        MP_DATA => CMD_READ_DATA,
        MP_USER => CMD_READ_USER,
        _ => CMD_READ_CODE,
    };
    let len = ds.data.len() as u16;
    let mut pkt = [0u8; 8];
    pkt[0] = cmd;
    pkt[1] = ds.block_count as u8;
    le16(&mut pkt[2..4], len);
    le32(&mut pkt[4..8], ds.address);
    pkt
}

fn write_cmd(ds: &DataSet) -> [u8; 8] {
    let cmd = match ds.page_type {
        MP_CODE => CMD_WRITE_CODE,
        MP_DATA => CMD_WRITE_DATA,
        MP_USER => CMD_WRITE_USER,
        _ => CMD_WRITE_CODE,
    };
    let len = ds.data.len() as u16;
    let mut pkt = [0u8; 8];
    pkt[0] = cmd;
    pkt[1] = ds.block_count as u8;
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
pub(super) fn do_ic_test_pass(usb: &UsbDevice, device: &Device, pull: bool) -> Result<Vec<u8>> {
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
pub(super) fn logic_ic_test_tl866(
    usb: &UsbDevice,
    device: &Device,
    out: &mut dyn std::io::Write,
) -> Result<()> {
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

    // Step 1: pull-ups active (pull = 0)
    let step1 = do_ic_test_pass(usb, device, false)?;
    // Step 2: pull-downs active (pull = 1)
    let step2 = do_ic_test_pass(usb, device, true)?;

    // Print header
    write!(out, "      ").ok();
    for pin in 1..=pin_count {
        write!(out, "{:<3}", pin).ok();
    }
    writeln!(out).ok();

    let mut errors = 0usize;

    for v in 0..vec_count {
        write!(out, "{:04}: ", v).ok();
        for p in 0..pin_count {
            let idx = v * pin_count + p;
            let state = vectors[idx];
            let s1 = step1[idx];
            let s2 = step2[idx];

            let err = match state {
                LOGIC_L => s1 != 0 || s2 != 0, // must be LOW in both passes
                LOGIC_H => s1 == 0 || s2 == 0, // must be HIGH in both passes
                LOGIC_Z => s1 == 0 || s2 != 0, // HIGH when pulled up, LOW when pulled down
                _ => false,                    // G, V, C, X, 0, 1 — no comparison
            };
            let ch = PSTATE.get(state as usize).copied().unwrap_or(b'?') as char;
            if err {
                write!(out, "\x1b[0;91m{}-\x1b[0m ", ch).ok();
                errors += 1;
            } else {
                write!(out, "{}  ", ch).ok();
            }
        }
        writeln!(out).ok();
    }

    if errors > 0 {
        eprintln!("Logic test FAILED: {} error(s).", errors);
        Err(MiniproError::Protocol(format!(
            "logic test failed: {} error(s)",
            errors
        )))
    } else {
        eprintln!("Logic test OK.");
        Ok(())
    }
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
/// On success prints "Pin test passed." and returns `Ok(())`.
/// Bad-contact pins are printed as they are found; the function returns
/// `Err(MiniproError::PinContactFailed)` if any pin fails.
pub(super) fn pin_test_tl866(
    usb: &UsbDevice,
    device: &Device,
    pin_map: &crate::database::PinMap,
) -> Result<()> {
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
    let mut ok = true;
    for &p_pin in &pin_map.mask {
        let p = p_pin as usize;
        let d_pin = if p > x_pin { p - pno } else { p };
        if (1..=40).contains(&p) && pins[p - 1] == 0 {
            eprintln!("Bad contact on pin: {}", d_pin);
            ok = false;
        }
    }

    if ok {
        eprintln!("Pin test passed.");
        Ok(())
    } else {
        Err(MiniproError::PinContactFailed)
    }
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

    eprintln!("Firmware image OK ({} blocks + last block).", n_blocks);

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

    eprintln!("Firmware written successfully. Please reconnect the programmer.");
    Ok(())
}
