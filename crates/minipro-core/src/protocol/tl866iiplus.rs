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

use crate::{
    device::Device,
    error::{MiniproError, Result},
    usb::UsbDevice,
};
use super::{DataSet, JedecSet, OvcStatus, Protocol};

// Command bytes
const CMD_GET_INFO:        u8 = 0x00;
const CMD_BEGIN_TRANS:     u8 = 0x03;
const CMD_END_TRANS:       u8 = 0x04;
const CMD_READ_CHIP_ID:    u8 = 0x05;
const CMD_READ_USER:       u8 = 0x06;
const CMD_WRITE_USER:      u8 = 0x07;
const CMD_READ_FUSES:      u8 = 0x08;
const CMD_WRITE_FUSES:     u8 = 0x09;
const CMD_WRITE_CODE:      u8 = 0x0C;
const CMD_READ_CODE:       u8 = 0x0D;
const CMD_ERASE:           u8 = 0x0E;
const CMD_READ_DATA:       u8 = 0x10;
const CMD_WRITE_DATA:      u8 = 0x11;
const CMD_WRITE_LOCK:      u8 = 0x14;
const CMD_READ_LOCK:       u8 = 0x15;
const CMD_READ_CALIB:      u8 = 0x16;
const CMD_PROTECT_OFF:     u8 = 0x18;
const CMD_PROTECT_ON:      u8 = 0x19;
const CMD_UNLOCK_TSOP48:   u8 = 0x38;
const CMD_REQUEST_STATUS:  u8 = 0x39;
const CMD_HARDWARE_CHECK:  u8 = 0x3C;

// Memory page types
const MP_CODE:  u8 = 0x00;
const MP_DATA:  u8 = 0x01;
const MP_USER:  u8 = 0x02;

// Minimum firmware version expected
pub const MIN_FIRMWARE: u32 = 0x255; // 4.2.85

pub struct Tl866iiPlusProtocol;

impl Tl866iiPlusProtocol {
    pub fn new() -> Self { Self }
}

impl Default for Tl866iiPlusProtocol {
    fn default() -> Self { Self::new() }
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
        return Err(MiniproError::ResponseTooShort { expected: 41, actual: resp.len() });
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
    pub model:            crate::device::ProgrammerModel,
    pub status:           crate::device::ProgrammerStatus,
    pub firmware:         u32,
    pub firmware_str:     String,
    pub device_code:      String,
    pub serial_number:    String,
    pub hardware_version: u8,
}

// ── Protocol implementation ───────────────────────────────────────────────────

impl Protocol for Tl866iiPlusProtocol {
    fn begin_transaction(&self, usb: &UsbDevice, device: &Device) -> Result<()> {
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
        pkt[3] = 0; // icsp flag set by caller if needed
        // opts1: vpp | vcc encoded
        let opts1: u16 = ((device.voltages.vcc as u16) << 4) | device.voltages.vpp as u16;
        le16(&mut pkt[5..7], opts1);
        le16(&mut pkt[8..10],  (device.data_memory_size & 0xffff) as u16);
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
        // 8-byte read command: [cmd, protocol, length_lo, length_hi, addr x4]
        let cmd = read_cmd(ds);
        usb.msg_send(&cmd)?;
        ds.data = usb.read_payload(ds.data.len())?;
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
            return Err(MiniproError::Protocol(
                format!("write_block status error: {:#04x}", resp[1])
            ));
        }
        Ok(())
    }

    fn get_chip_id(&self, usb: &UsbDevice) -> Result<(u8, u32)> {
        let mut pkt = [0u8; 8];
        pkt[0] = CMD_READ_CHIP_ID;
        usb.msg_send(&pkt)?;
        let resp = usb.msg_recv(64)?;
        if resp.len() < 6 {
            return Err(MiniproError::ResponseTooShort { expected: 6, actual: resp.len() });
        }
        let id_type = resp[1];
        let chip_id = u32::from_le_bytes([resp[2], resp[3], resp[4], resp[5]]);
        Ok((id_type, chip_id))
    }

    fn read_fuses(
        &self, usb: &UsbDevice, _device: &Device, fuse_type: u8, length: usize,
        items_count: u8,
    ) -> Result<Vec<u8>> {
        let cmd_byte = match fuse_type {
            0x00 => CMD_READ_USER,  // MP_FUSE_USER
            0x01 => CMD_READ_FUSES, // MP_FUSE_CFG
            0x02 => CMD_READ_LOCK,  // MP_FUSE_LOCK
            _    => CMD_READ_FUSES,
        };
        let mut pkt = [0u8; 8];
        pkt[0] = cmd_byte;
        pkt[1] = length as u8;
        pkt[2] = items_count;
        usb.msg_send(&pkt)?;
        usb.msg_recv(64)
    }

    fn write_fuses(
        &self, usb: &UsbDevice, _device: &Device, fuse_type: u8, length: usize,
        items_count: u8, data: &[u8],
    ) -> Result<()> {
        let cmd_byte = match fuse_type {
            0x00 => CMD_WRITE_USER,
            0x01 => CMD_WRITE_FUSES,
            0x02 => CMD_WRITE_LOCK,
            _    => CMD_WRITE_FUSES,
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
        js.data = usb.read_payload(js.data.len())?;
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
            return Err(MiniproError::ResponseTooShort { expected: 8, actual: resp.len() });
        }
        let ovc_flag = resp[1];
        let status = OvcStatus {
            error:   resp[0],
            address: u32::from_le_bytes([resp[2], resp[3], resp[4], resp[5]]),
            c1:      resp[6] as u32,
            c2:      resp[7] as u32,
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

    fn firmware_update(&self, usb: &UsbDevice, firmware: &[u8]) -> Result<()> {
        // Firmware update is a multi-step process; leave as a stub for now.
        let _ = (usb, firmware);
        Err(MiniproError::UnsupportedOperation)
    }

    fn reset_state(&self, usb: &UsbDevice) -> Result<()> {
        let mut pkt = [0u8; 8];
        pkt[0] = 0x2D; // reset pin drivers
        usb.msg_send(&pkt)?;
        Ok(())
    }
}

// ── Packet builders ───────────────────────────────────────────────────────────

fn read_cmd(ds: &DataSet) -> [u8; 8] {
    // [cmd, protocol, len_lo, len_hi, addr0, addr1, addr2, addr3]
    let cmd = match ds.page_type {
        MP_CODE => CMD_READ_CODE,
        MP_DATA => CMD_READ_DATA,
        MP_USER => CMD_READ_USER,
        _       => CMD_READ_CODE,
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
        _       => CMD_WRITE_CODE,
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
