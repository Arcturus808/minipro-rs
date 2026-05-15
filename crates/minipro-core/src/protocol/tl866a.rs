//! TL866A/CS protocol implementation.
//!
//! Command reference (from upstream tl866a.c/h):
//!
//! | Cmd  | Description            |
//! |------|------------------------|
//! | 0x03 | Start transaction      |
//! | 0x04 | End transaction        |
//! | 0x05 | Get chip ID            |
//! | 0x10 | Read USER fuses        |
//! | 0x11 | Write USER fuses       |
//! | 0x12 | Read CFG fuses         |
//! | 0x13 | Write CFG fuses        |
//! | 0x14 | Write USER data        |
//! | 0x15 | Read USER data         |
//! | 0x20 | Write code             |
//! | 0x21 | Read code              |
//! | 0x22 | Erase                  |
//! | 0x30 | Read data              |
//! | 0x31 | Write data             |
//! | 0x40 | Write lock bits        |
//! | 0x41 | Read lock bits         |
//! | 0x42 | Read calibration       |
//! | 0x44 | Protect off            |
//! | 0x45 | Protect on             |
//! | 0xD0 | Reset pin drivers      |
//! | 0xFC | SPI autodetect         |
//! | 0xFD | Unlock TSOP48          |
//! | 0xFE | Get OVC status         |

use crate::{
    device::Device,
    error::{MiniproError, Result},
    usb::UsbDevice,
};
use super::{DataSet, JedecSet, OvcStatus, Protocol};

/// Minimum firmware version for TL866A/CS.
pub const MIN_FIRMWARE_A: u32 = 0x0256; // 3.2.86

// Command bytes
const CMD_START_TRANSACTION: u8 = 0x03;
const CMD_END_TRANSACTION:   u8 = 0x04;
const CMD_GET_CHIP_ID:       u8 = 0x05;
const CMD_READ_USER:         u8 = 0x10;
const CMD_WRITE_USER:        u8 = 0x11;
const CMD_READ_CFG:          u8 = 0x12;
const CMD_WRITE_CFG:         u8 = 0x13;
const CMD_WRITE_USER_DATA:   u8 = 0x14;
const CMD_READ_USER_DATA:    u8 = 0x15;
const CMD_WRITE_CODE:        u8 = 0x20;
const CMD_READ_CODE:         u8 = 0x21;
const CMD_ERASE:             u8 = 0x22;
const CMD_READ_DATA:         u8 = 0x30;
const CMD_WRITE_DATA:        u8 = 0x31;
const CMD_WRITE_LOCK:        u8 = 0x40;
const CMD_READ_LOCK:         u8 = 0x41;
const CMD_READ_CALIBRATION:  u8 = 0x42;
const CMD_PROTECT_OFF:       u8 = 0x44;
const CMD_PROTECT_ON:        u8 = 0x45;
const CMD_AUTODETECT:        u8 = 0xFC;
const CMD_UNLOCK_TSOP48:     u8 = 0xFD;
const CMD_GET_STATUS:        u8 = 0xFE;
const CMD_RESET_PIN_DRIVERS: u8 = 0xD0;

// Memory page types
const MP_DATA: u8 = 0x01;
const MP_USER: u8 = 0x02;

// Fuse sub-types
const MP_FUSE_USER: u8 = 0x00;
const MP_FUSE_CFG:  u8 = 0x01;
const MP_FUSE_LOCK: u8 = 0x02;

pub struct Tl866aProtocol;

impl Tl866aProtocol {
    pub fn new() -> Self { Self }
}

impl Default for Tl866aProtocol {
    fn default() -> Self { Self::new() }
}

/// Store a little-endian integer of `len` bytes starting at `buf`.
fn put_le(buf: &mut [u8], val: u32, len: usize) {
    for (i, b) in buf[..len].iter_mut().enumerate() {
        *b = (val >> (i * 8)) as u8;
    }
}

impl Protocol for Tl866aProtocol {
    fn begin_transaction(&self, usb: &UsbDevice, device: &Device) -> Result<()> {
        let mut msg = [0u8; 64];
        msg[0] = CMD_START_TRANSACTION;
        msg[1] = device.protocol_id;
        msg[2] = device.variant as u8;
        // [3..4]  data_memory_size  (16-bit LE)
        put_le(&mut msg[3..], device.data_memory_size, 2);
        // [5]     vpp << 4
        msg[5] = device.voltages.vpp << 4;
        // [6..7]  page_size         (16-bit LE)
        put_le(&mut msg[6..], device.page_size, 2);
        // [8]     (vdd << 4) | vcc
        msg[8] = (device.voltages.vdd << 4) | device.voltages.vcc;
        // [9..10] pulse_delay       (16-bit LE)
        put_le(&mut msg[9..], device.pulse_delay, 2);
        // [11]    icsp (set when entering ICSP mode — handled by caller)
        // [12..14] code_memory_size (24-bit LE)
        put_le(&mut msg[12..], device.code_memory_size, 3);
        usb.msg_send(&msg[..48])?;
        Ok(())
    }

    fn end_transaction(&self, usb: &UsbDevice) -> Result<()> {
        usb.msg_send(&[CMD_END_TRANSACTION, 0, 0, 0])?;
        Ok(())
    }

    fn read_block(&self, usb: &UsbDevice, ds: &mut DataSet) -> Result<()> {
        let cmd = match ds.page_type {
            MP_DATA => CMD_READ_DATA,
            MP_USER => CMD_READ_USER_DATA,
            _       => CMD_READ_CODE,
        };
        let mut msg = [0u8; 18];
        msg[0] = cmd;
        // [2..3] size (16-bit LE)  — overwrites variant byte intentionally
        put_le(&mut msg[2..], ds.data.len() as u32, 2);
        // [4..6] address (24-bit LE)
        put_le(&mut msg[4..], ds.address, 3);
        usb.msg_send(&msg)?;
        let resp = usb.read_payload(ds.data.len())?;
        let n = resp.len().min(ds.data.len());
        ds.data[..n].copy_from_slice(&resp[..n]);
        Ok(())
    }

    fn write_block(&self, usb: &UsbDevice, ds: &DataSet) -> Result<()> {
        let cmd = match ds.page_type {
            MP_DATA => CMD_WRITE_DATA,
            MP_USER => CMD_WRITE_USER_DATA,
            _       => CMD_WRITE_CODE,
        };
        // Payload: [cmd, 0, size(2), addr(3), data...]
        let mut payload = vec![0u8; ds.data.len() + 7];
        payload[0] = cmd;
        put_le(&mut payload[2..], ds.data.len() as u32, 2);
        put_le(&mut payload[4..], ds.address, 3);
        payload[7..].copy_from_slice(&ds.data);
        usb.msg_send(&payload)?;
        Ok(())
    }

    fn get_chip_id(&self, usb: &UsbDevice) -> Result<(u8, u32)> {
        let mut msg = [0u8; 8];
        msg[0] = CMD_GET_CHIP_ID;
        usb.msg_send(&msg)?;
        let resp = usb.msg_recv(32)?;
        if resp.len() < 5 {
            return Err(MiniproError::ResponseTooShort { expected: 5, actual: resp.len() });
        }
        let id_type = resp[0];
        let id = u32::from_be_bytes([0, resp[2], resp[3], resp[4]]);
        Ok((id_type, id))
    }

    fn spi_autodetect(&self, usb: &UsbDevice, id_type: u8) -> Result<u32> {
        let mut msg = [0u8; 10];
        msg[0] = CMD_AUTODETECT;
        msg[7] = id_type;
        usb.msg_send(&msg)?;
        let resp = usb.msg_recv(16)?;
        if resp.len() < 5 {
            return Err(MiniproError::ResponseTooShort { expected: 5, actual: resp.len() });
        }
        Ok(u32::from_be_bytes([0, resp[2], resp[3], resp[4]]))
    }

    fn read_fuses(
        &self, usb: &UsbDevice, _device: &Device, fuse_type: u8, _length: usize,
        items_count: u8,
    ) -> Result<Vec<u8>> {
        let cmd = match fuse_type {
            MP_FUSE_USER => CMD_READ_USER,
            MP_FUSE_CFG  => CMD_READ_CFG,
            MP_FUSE_LOCK => CMD_READ_LOCK,
            other        => return Err(MiniproError::Protocol(format!("unknown fuse type {other}"))),
        };
        let mut msg = [0u8; 18];
        msg[0] = cmd;
        msg[2] = items_count;
        usb.msg_send(&msg)?;
        let resp = usb.msg_recv(64)?;
        Ok(resp.get(7..).unwrap_or_default().to_vec())
    }

    fn write_fuses(
        &self, usb: &UsbDevice, _device: &Device, fuse_type: u8, _length: usize,
        items_count: u8, data: &[u8],
    ) -> Result<()> {
        let cmd = match fuse_type {
            MP_FUSE_USER => CMD_WRITE_USER,
            MP_FUSE_CFG  => CMD_WRITE_CFG,
            MP_FUSE_LOCK => CMD_WRITE_LOCK,
            other        => return Err(MiniproError::Protocol(format!("unknown fuse type {other}"))),
        };
        let mut msg = [0u8; 64];
        msg[0] = cmd;
        msg[2] = items_count;
        let n = data.len().min(57);
        msg[7..7 + n].copy_from_slice(&data[..n]);
        usb.msg_send(&msg)?;
        Ok(())
    }

    fn read_calibration(&self, usb: &UsbDevice, size: usize) -> Result<Vec<u8>> {
        let mut msg = [0u8; 64];
        msg[0] = CMD_READ_CALIBRATION;
        put_le(&mut msg[2..], size as u32, 2);
        usb.msg_send(&msg)?;
        usb.msg_recv(size)
    }

    fn erase(&self, usb: &UsbDevice, num_fuses: u8, _is_pld: bool) -> Result<()> {
        let mut msg = [0u8; 15];
        msg[0] = CMD_ERASE;
        msg[2] = num_fuses;
        usb.msg_send(&msg)?;
        let _ = usb.msg_recv(64);
        Ok(())
    }

    fn read_jedec_row(&self, usb: &UsbDevice, js: &mut JedecSet) -> Result<()> {
        let mut msg = [0u8; 18];
        msg[0] = CMD_READ_CODE;
        msg[4] = js.row;
        msg[5] = js.flags;
        usb.msg_send(&msg)?;
        let resp = usb.msg_recv(64)?;
        let n = js.data.len().min(resp.len());
        js.data[..n].copy_from_slice(&resp[..n]);
        Ok(())
    }

    fn write_jedec_row(&self, usb: &UsbDevice, js: &JedecSet) -> Result<()> {
        let mut msg = [0u8; 64];
        msg[0] = CMD_WRITE_CODE;
        msg[4] = js.row;
        msg[5] = js.flags;
        let n = js.data.len().min(57);
        msg[7..7 + n].copy_from_slice(&js.data[..n]);
        usb.msg_send(&msg)?;
        Ok(())
    }

    fn protect_off(&self, usb: &UsbDevice) -> Result<()> {
        let mut msg = [0u8; 10];
        msg[0] = CMD_PROTECT_OFF;
        usb.msg_send(&msg)?;
        Ok(())
    }

    fn protect_on(&self, usb: &UsbDevice) -> Result<()> {
        let mut msg = [0u8; 10];
        msg[0] = CMD_PROTECT_ON;
        usb.msg_send(&msg)?;
        Ok(())
    }

    fn get_ovc_status(&self, usb: &UsbDevice) -> Result<(OvcStatus, u8)> {
        usb.msg_send(&[CMD_GET_STATUS, 0, 0, 0, 0])?;
        let resp = usb.msg_recv(64)?;
        if resp.len() < 10 {
            return Err(MiniproError::ResponseTooShort { expected: 10, actual: resp.len() });
        }
        let status = OvcStatus {
            error:   resp[0],
            address: u32::from_le_bytes([resp[6], resp[7], resp[8], 0]),
            c1:      u32::from_le_bytes([resp[2], resp[3], 0, 0]),
            c2:      u32::from_le_bytes([resp[4], resp[5], 0, 0]),
        };
        Ok((status, resp[9]))
    }

    fn unlock_tsop48(&self, usb: &UsbDevice) -> Result<u8> {
        // Pseudo-random payload + simple CRC16 for TSOP48 unlock authentication
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos();
        let mut msg = [0u8; 17];
        msg[0] = CMD_UNLOCK_TSOP48;
        let mut crc: u16 = 0;
        for i in 0usize..8 {
            let byte = (seed.wrapping_shr((i * 4) as u32)) as u8;
            msg[7 + i] = byte;
            // CRC16-CCITT update
            crc = (crc >> 8) | (crc << 8);
            crc ^= byte as u16;
            crc ^= (crc & 0xff) >> 4;
            crc ^= crc << 12;
            crc ^= (crc & 0xff) << 5;
        }
        // Swap two bytes then embed CRC
        msg[15] = msg[9];
        msg[16] = msg[11];
        msg[9]  = crc as u8;
        msg[11] = (crc >> 8) as u8;
        usb.msg_send(&msg)?;
        let resp = usb.msg_recv(64)?;
        Ok(resp.get(1).copied().unwrap_or(0))
    }

    fn hardware_check(&self, _usb: &UsbDevice) -> Result<()> {
        // Full ZIF pin matrix self-test — deferred to Phase 4
        Err(MiniproError::UnsupportedOperation)
    }

    fn firmware_update(&self, _usb: &UsbDevice, _firmware: &[u8]) -> Result<()> {
        // TL866A firmware update requires decrypting update.dat — deferred to Phase 4
        Err(MiniproError::UnsupportedOperation)
    }

    fn reset_state(&self, usb: &UsbDevice) -> Result<()> {
        usb.msg_send(&[CMD_RESET_PIN_DRIVERS, 0, 0, 0, 0, 0, 0, 0])?;
        Ok(())
    }
}
