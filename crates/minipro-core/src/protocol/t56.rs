//! T56 protocol implementation.
//!
//! The T56 programmer uses a Xilinx FPGA that must be programmed with a
//! device-specific algorithm bitstream before each programming session.
//! The bitstream is uploaded over the command endpoint (EP 0x01) before
//! the normal begin-transaction command is sent.
//!
//! Command bytes are the same numbering as TL866II+ (0x03–0x1E) with the
//! addition of T56-specific commands 0x26 (write bitstream), 0x2A (write
//! bitstream 2, logic chips), 0x39 (request OVC status), etc.

use crate::{
    device::Device,
    error::{MiniproError, Result},
    usb::UsbDevice,
};
use super::{DataSet, JedecSet, OvcStatus, Protocol};

/// Minimum firmware version for the T56.
pub const MIN_FIRMWARE_T56: u32 = 0x149; // 1.73

// ── Command bytes ─────────────────────────────────────────────────────────────
const CMD_BEGIN_TRANS:      u8 = 0x03;
const CMD_END_TRANS:        u8 = 0x04;
const CMD_READ_ID:          u8 = 0x05;
const CMD_READ_USER:        u8 = 0x06;
const CMD_WRITE_USER:       u8 = 0x07;
const CMD_READ_CFG:         u8 = 0x08;
const CMD_WRITE_CFG:        u8 = 0x09;
const CMD_WRITE_USER_DATA:  u8 = 0x0A;
const CMD_READ_USER_DATA:   u8 = 0x0B;
const CMD_WRITE_CODE:       u8 = 0x0C;
const CMD_READ_CODE:        u8 = 0x0D;
const CMD_ERASE:            u8 = 0x0E;
const CMD_READ_DATA:        u8 = 0x10;
const CMD_WRITE_DATA:       u8 = 0x11;
const CMD_WRITE_LOCK:       u8 = 0x14;
const CMD_READ_LOCK:        u8 = 0x15;
const CMD_READ_CALIBRATION: u8 = 0x16;
const CMD_PROTECT_OFF:      u8 = 0x18;
const CMD_PROTECT_ON:       u8 = 0x19;
const CMD_READ_JEDEC:       u8 = 0x1D;
const CMD_WRITE_JEDEC:      u8 = 0x1E;
const CMD_WRITE_BITSTREAM:  u8 = 0x26;
#[allow(dead_code)]
const CMD_WRITE_BITSTREAM2: u8 = 0x2A; // logic-chip round 2 (Phase 4)
#[allow(dead_code)]
const CMD_AUTODETECT:       u8 = 0x37;
const CMD_UNLOCK_TSOP48:    u8 = 0x38;
const CMD_REQUEST_STATUS:   u8 = 0x39;
const CMD_HARDWARE_CHECK:   u8 = 0x3C;

// Memory page types (must match operations.rs constants)
const MP_CODE: u8 = 0x00;
const MP_DATA: u8 = 0x01;
const MP_USER: u8 = 0x02;

// Fuse type sub-command mapping
const T56_READ_USER:  u8 = CMD_READ_USER;
const T56_WRITE_USER: u8 = CMD_WRITE_USER;
const T56_READ_CFG:   u8 = CMD_READ_CFG;
const T56_WRITE_CFG:  u8 = CMD_WRITE_CFG;
const T56_READ_LOCK:  u8 = CMD_READ_LOCK;
const T56_WRITE_LOCK: u8 = CMD_WRITE_LOCK;

// OVC status response layout
const OVC_RESP_LEN: usize = 32;
const OVC_FLAG_IDX: usize = 12;

pub struct T56Protocol;

impl T56Protocol {
    pub fn new() -> Self { Self }
}

impl Default for T56Protocol {
    fn default() -> Self { Self::new() }
}

// ── Byte-packing helpers ──────────────────────────────────────────────────────

#[inline]
fn put_le16(dst: &mut [u8], v: u32) {
    dst[0] = (v & 0xff) as u8;
    dst[1] = ((v >> 8) & 0xff) as u8;
}

#[inline]
fn put_le32(dst: &mut [u8], v: u32) {
    dst[0] = (v & 0xff) as u8;
    dst[1] = ((v >> 8) & 0xff) as u8;
    dst[2] = ((v >> 16) & 0xff) as u8;
    dst[3] = ((v >> 24) & 0xff) as u8;
}

// ── Bitstream upload ──────────────────────────────────────────────────────────

/// Upload the FPGA algorithm bitstream for a normal (non-logic) device.
///
/// Protocol (from t56.c `t56_write_bitstream`):
/// 1. Send an 8-byte header: `[0x26, 0, 0, 0, len_LE32]`
/// 2. Send the full bitstream as a large packet to the command endpoint.
fn upload_bitstream(usb: &UsbDevice, bitstream: &[u8]) -> Result<()> {
    let mut hdr = [0u8; 8];
    hdr[0] = CMD_WRITE_BITSTREAM;
    put_le32(&mut hdr[4..8], bitstream.len() as u32);
    usb.msg_send(&hdr)?;
    usb.msg_send_large(bitstream)
}

// ── Shared begin-transaction message encoder ──────────────────────────────────

/// Build the 64-byte begin_transaction message shared by T56 and T76.
///
/// Encoding taken directly from t56.c / t76.c.
/// The caller may write additional bytes (e.g. T76's msg[63] algorithm number)
/// before sending.
pub(super) fn build_begin_msg(device: &Device, icsp: bool) -> [u8; 64] {
    let mut msg = [0u8; 64];
    let v = device.voltages.raw;

    msg[0] = CMD_BEGIN_TRANS;
    msg[1] = device.protocol_id;
    msg[2] = device.variant as u8;
    msg[3] = icsp as u8;

    put_le16(&mut msg[4..6],   v & 0xffff);
    msg[6]  = device.chip_info as u8;
    msg[7]  = device.pin_map as u8;

    put_le16(&mut msg[8..10],  device.data_memory_size);
    put_le16(&mut msg[10..12], device.page_size);
    put_le16(&mut msg[12..14], device.pulse_delay);
    put_le16(&mut msg[14..16], device.data_memory2_size);
    put_le32(&mut msg[16..20], device.code_memory_size);

    msg[20] = (v >> 16) as u8;

    if (v & 0xf0) == 0xf0 {
        msg[22] = v as u8;
    } else {
        msg[21] = (v & 0x0f) as u8;
        msg[22] = (v & 0xf0) as u8;
    }
    if v & 0x8000_0000 != 0 {
        msg[22] = ((v >> 16) & 0x0f) as u8;
    }

    if device.flags.can_adjust_clock {
        msg[28] = device.spi_clock;
    }

    put_le32(&mut msg[40..44], device.package_details.raw);
    put_le16(&mut msg[44..46], device.read_buffer_size as u32);
    put_le32(&mut msg[56..60], device.flags.raw);

    msg
}

// ── Protocol implementation ───────────────────────────────────────────────────

impl Protocol for T56Protocol {
    fn begin_transaction(&self, usb: &UsbDevice, device: &Device) -> Result<()> {
        // 1. Upload FPGA algorithm bitstream if the device provides one.
        if let Some(ref algo) = device.algorithm {
            if !algo.bitstream.is_empty() {
                eprintln!("Using T56 {} algorithm..", algo.name);
                upload_bitstream(usb, &algo.bitstream)?;
            }
        }

        // 2. Send the begin_transaction command (unless the device uses a
        //    custom bit-bang protocol, which is deferred to Phase 4).
        if !device.flags.custom_protocol {
            let msg = build_begin_msg(device, false);
            usb.msg_send(&msg)?;
        }

        Ok(())
    }

    fn end_transaction(&self, usb: &UsbDevice) -> Result<()> {
        let mut msg = [0u8; 8];
        msg[0] = CMD_END_TRANS;
        usb.msg_send(&msg)
    }

    fn read_block(&self, usb: &UsbDevice, ds: &mut DataSet) -> Result<()> {
        let cmd = match ds.page_type {
            MP_CODE => CMD_READ_CODE,
            MP_DATA => CMD_READ_DATA,
            MP_USER => CMD_READ_USER_DATA,
            _       => return Err(MiniproError::Protocol(
                format!("T56 read_block: unknown page_type {}", ds.page_type)
            )),
        };

        let mut msg = [0u8; 8];
        msg[0] = cmd;
        put_le16(&mut msg[2..4], ds.data.len() as u32);
        put_le32(&mut msg[4..8], ds.address);
        usb.msg_send(&msg)?;

        // The T56 firmware returns (size + 16) bytes due to a firmware quirk;
        // we read the extra bytes and discard them.
        let wanted = ds.data.len();
        let mut resp = usb.read_payload(wanted + 16)?;
        resp.truncate(wanted);
        ds.data = resp;
        Ok(())
    }

    fn write_block(&self, usb: &UsbDevice, ds: &DataSet) -> Result<()> {
        let cmd = match ds.page_type {
            MP_CODE => CMD_WRITE_CODE,
            MP_DATA => CMD_WRITE_DATA,
            MP_USER => CMD_WRITE_USER_DATA,
            _       => return Err(MiniproError::Protocol(
                format!("T56 write_block: unknown page_type {}", ds.page_type)
            )),
        };

        let mut msg = [0u8; 8];
        msg[0] = cmd;
        put_le16(&mut msg[2..4], ds.data.len() as u32);
        put_le32(&mut msg[4..8], ds.address);
        usb.msg_send(&msg)?;
        usb.write_payload(&ds.data)
    }

    fn get_chip_id(&self, usb: &UsbDevice) -> Result<(u8, u32)> {
        let mut msg = [0u8; 8];
        msg[0] = CMD_READ_ID;
        usb.msg_send(&msg)?;
        let resp = usb.msg_recv(32)?;
        if resp.len() < 5 {
            return Err(MiniproError::ResponseTooShort { expected: 5, actual: resp.len() });
        }
        let id_type = resp[0];
        let chip_id = if id_type == 3 || id_type == 4 {
            u32::from_le_bytes([resp[2], resp[3], resp[4], 0])
        } else {
            ((resp[2] as u32) << 16) | ((resp[3] as u32) << 8) | resp[4] as u32
        };
        Ok((id_type, chip_id))
    }

    fn read_fuses(
        &self, usb: &UsbDevice, device: &Device, fuse_type: u8, length: usize,
        items_count: u8,
    ) -> Result<Vec<u8>> {
        let cmd = match fuse_type {
            0x00 => T56_READ_USER,
            0x01 => T56_READ_CFG,
            0x02 => T56_READ_LOCK,
            _    => return Err(MiniproError::Protocol(
                format!("T56 read_fuses: unknown type {fuse_type}")
            )),
        };
        let mut msg = [0u8; 8];
        msg[0] = cmd;
        msg[1] = device.protocol_id;
        msg[2] = items_count;
        put_le32(&mut msg[4..8], device.code_memory_size);
        usb.msg_send(&msg)?;
        let resp = usb.msg_recv(64)?;
        // T56 response: data starts at offset 8
        let start = 8;
        let n = resp.len().saturating_sub(start).min(length);
        Ok(resp[start..start + n].to_vec())
    }

    fn write_fuses(
        &self, usb: &UsbDevice, device: &Device, fuse_type: u8, length: usize,
        items_count: u8, data: &[u8],
    ) -> Result<()> {
        let cmd = match fuse_type {
            0x00 => T56_WRITE_USER,
            0x01 => T56_WRITE_CFG,
            0x02 => T56_WRITE_LOCK,
            _    => return Err(MiniproError::Protocol(
                format!("T56 write_fuses: unknown type {fuse_type}")
            )),
        };
        let mut msg = vec![0u8; 64];
        msg[0] = cmd;
        msg[1] = device.protocol_id;
        msg[2] = items_count;
        // T56 firmware quirk: subtract 0x38 from code_memory_size
        let sz = device.code_memory_size.saturating_sub(0x38);
        put_le32(&mut msg[4..8], sz);
        let n = data.len().min(length).min(56);
        msg[8..8 + n].copy_from_slice(&data[..n]);
        usb.msg_send(&msg)
    }

    fn read_calibration(&self, usb: &UsbDevice, size: usize) -> Result<Vec<u8>> {
        let mut msg = [0u8; 64];
        msg[0] = CMD_READ_CALIBRATION;
        put_le16(&mut msg[2..4], size as u32);
        usb.msg_send(&msg)?;
        usb.msg_recv(size)
    }

    fn erase(&self, usb: &UsbDevice, num_fuses: u8, is_pld: bool) -> Result<()> {
        // T56 uses a 15-byte erase packet (T76 uses 16)
        let mut msg = [0u8; 15];
        msg[0] = CMD_ERASE;
        msg[2] = num_fuses;
        msg[4] = is_pld as u8;
        usb.msg_send(&msg)?;
        usb.msg_recv(64)?;
        Ok(())
    }

    fn read_jedec_row(&self, usb: &UsbDevice, js: &mut JedecSet) -> Result<()> {
        let bits = (js.data.len() * 8) as u8;
        let mut msg = [0u8; 8];
        msg[0] = CMD_READ_JEDEC;
        msg[2] = bits;
        msg[4] = js.row;
        msg[5] = js.flags;
        usb.msg_send(&msg)?;
        let resp = usb.msg_recv(32)?;
        let byte_len = ((bits as usize) + 7) / 8;
        js.data.resize(byte_len, 0);
        let n = byte_len.min(resp.len());
        js.data[..n].copy_from_slice(&resp[..n]);
        Ok(())
    }

    fn write_jedec_row(&self, usb: &UsbDevice, js: &JedecSet) -> Result<()> {
        let bits = (js.data.len() * 8) as u8;
        let byte_len = ((bits as usize) + 7) / 8;
        let mut msg = [0u8; 64];
        msg[0] = CMD_WRITE_JEDEC;
        msg[2] = bits;
        msg[4] = js.row;
        msg[5] = js.flags;
        let n = byte_len.min(js.data.len()).min(56);
        msg[8..8 + n].copy_from_slice(&js.data[..n]);
        usb.msg_send(&msg)
    }

    fn protect_off(&self, usb: &UsbDevice) -> Result<()> {
        let mut msg = [0u8; 8];
        msg[0] = CMD_PROTECT_OFF;
        usb.msg_send(&msg)
    }

    fn protect_on(&self, usb: &UsbDevice) -> Result<()> {
        let mut msg = [0u8; 8];
        msg[0] = CMD_PROTECT_ON;
        usb.msg_send(&msg)
    }

    fn get_ovc_status(&self, usb: &UsbDevice) -> Result<(OvcStatus, u8)> {
        let mut msg = [0u8; 8];
        msg[0] = CMD_REQUEST_STATUS;
        usb.msg_send(&msg)?;
        let resp = usb.msg_recv(OVC_RESP_LEN)?;
        if resp.len() < OVC_RESP_LEN {
            return Err(MiniproError::ResponseTooShort {
                expected: OVC_RESP_LEN,
                actual: resp.len(),
            });
        }
        let status = OvcStatus {
            error:   resp[0],
            address: u32::from_le_bytes([resp[8], resp[9], resp[10], resp[11]]),
            c1:      u16::from_le_bytes([resp[2], resp[3]]) as u32,
            c2:      u16::from_le_bytes([resp[4], resp[5]]) as u32,
        };
        let ovc = resp[OVC_FLAG_IDX];
        Ok((status, ovc))
    }

    fn unlock_tsop48(&self, usb: &UsbDevice) -> Result<u8> {
        let mut msg = [0u8; 8];
        msg[0] = CMD_UNLOCK_TSOP48;
        usb.msg_send(&msg)?;
        let resp = usb.msg_recv(8)?;
        Ok(resp.get(1).copied().unwrap_or(0))
    }

    fn hardware_check(&self, usb: &UsbDevice) -> Result<()> {
        let mut msg = [0u8; 8];
        msg[0] = CMD_HARDWARE_CHECK;
        usb.msg_send(&msg)?;
        usb.msg_recv(64)?;
        Ok(())
    }

    fn firmware_update(&self, _usb: &UsbDevice, _firmware: &[u8]) -> Result<()> {
        Err(MiniproError::UnsupportedOperation)
    }

    fn reset_state(&self, usb: &UsbDevice) -> Result<()> {
        self.end_transaction(usb)
    }
}

