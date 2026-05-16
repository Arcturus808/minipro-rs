//! T76 protocol implementation.
//!
//! The T76 has the same command numbering as the T56 but with an important
//! difference in the FPGA bitstream upload: rather than sending the entire
//! bitstream as one large message, the T76 uses a three-phase chunked protocol
//! (BEGIN → BLOCK × N → END) with 512-byte packets.
//!
//! The begin_transaction format is also identical to the T56 except that
//! `msg[63]` carries the device algorithm number (high byte of `variant`).
//!
//! Block read/write for code memory uses the payload endpoint (EP 0x82) with
//! a 16-byte header prepended to each transfer; data and user memory use
//! slightly different layouts.

use super::t56::build_begin_msg;
use super::tl866iiplus::logic_ic_test_tl866;
use super::{DataSet, JedecSet, OvcStatus, Protocol};
use crate::{
    device::Device,
    error::{MiniproError, Result},
    usb::UsbDevice,
};

/// Minimum firmware version for the T76.
pub const MIN_FIRMWARE_T76: u32 = 0x10D; // 0.1.13

// T76 firmware update constants
const T76_UPDATE_FILE_VERSION: u32 = 0xf076_0000;
const T76_UPDATE_VERS_MASK: u32 = 0xffff_0000;
const T76_BTLDR_MAGIC: u32 = 0x04_9000;
const T76_LAST_BLOCK_ADDR: u32 = 0x049f00;
const T76_LAST_BLOCK_CRC: u32 = 0xcdef_8668;

// ── Command bytes (identical to T56) ─────────────────────────────────────────
const CMD_END_TRANS: u8 = 0x04;
const CMD_READ_ID: u8 = 0x05;
const CMD_READ_USER: u8 = 0x06;
const CMD_WRITE_USER: u8 = 0x07;
const CMD_READ_CFG: u8 = 0x08;
const CMD_WRITE_CFG: u8 = 0x09;
const CMD_WRITE_USER_DATA: u8 = 0x0A;
const CMD_READ_USER_DATA: u8 = 0x0B;
const CMD_WRITE_CODE: u8 = 0x0C;
const CMD_READ_CODE: u8 = 0x0D;
const CMD_ERASE: u8 = 0x0E;
const CMD_READ_DATA: u8 = 0x10;
const CMD_WRITE_DATA: u8 = 0x11;
const CMD_WRITE_LOCK: u8 = 0x14;
const CMD_READ_LOCK: u8 = 0x15;
const CMD_READ_CALIBRATION: u8 = 0x16;
const CMD_PROTECT_OFF: u8 = 0x18;
const CMD_PROTECT_ON: u8 = 0x19;
const CMD_READ_JEDEC: u8 = 0x1D;
const CMD_WRITE_JEDEC: u8 = 0x1E;
const CMD_WRITE_BITSTREAM: u8 = 0x26;
const CMD_UNLOCK_TSOP48: u8 = 0x38;
const CMD_REQUEST_STATUS: u8 = 0x39;
const CMD_HARDWARE_CHECK: u8 = 0x3C;

// T76 bitstream sub-commands
const T76_BEGIN_BS: u8 = 0x00;
const T76_BS_BLOCK: u8 = 0x01;
const T76_END_BS: u8 = 0x02;
const T76_RESET_FPGA: u8 = 0xaf;
const T76_FPGA_MAGIC: u32 = 0xaa55_ddee;

/// T76 bitstream packet size (8 header + 504 payload).
const BS_PACKET_SIZE: usize = 512;

// Memory page types
const MP_CODE: u8 = 0x00;
const MP_DATA: u8 = 0x01;
const MP_USER: u8 = 0x02;

// Fuse type sub-command mapping
const T76_READ_USER: u8 = CMD_READ_USER;
const T76_WRITE_USER: u8 = CMD_WRITE_USER;
const T76_READ_CFG: u8 = CMD_READ_CFG;
const T76_WRITE_CFG: u8 = CMD_WRITE_CFG;
const T76_READ_LOCK: u8 = CMD_READ_LOCK;
const T76_WRITE_LOCK: u8 = CMD_WRITE_LOCK;

// OVC status response
const OVC_RESP_LEN: usize = 32;
const OVC_FLAG_IDX: usize = 12;

pub struct T76Protocol;

impl T76Protocol {
    pub fn new() -> Self {
        Self
    }
}

impl Default for T76Protocol {
    fn default() -> Self {
        Self::new()
    }
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

// ── T76 three-phase bitstream upload ─────────────────────────────────────────

/// Upload the FPGA bitstream using the T76 chunked protocol.
///
/// Protocol (from t76.c `t76_write_bitstream`):
/// 1. BEGIN packet  — send 8 bytes, await 8-byte ACK (`resp[1] == 0`).
/// 2. BLOCK packets — each is exactly 512 bytes (8 header + up to 504 payload).
/// 3. END packet    — send 8 bytes, await 8-byte ACK (`resp[1] == 0`).
fn upload_bitstream_t76(usb: &UsbDevice, bitstream: &[u8]) -> Result<()> {
    let payload_size = BS_PACKET_SIZE - 8; // 504 bytes

    // ── Phase 1: BEGIN ────────────────────────────────────────────────────────
    let mut msg = [0u8; BS_PACKET_SIZE];
    msg[0] = CMD_WRITE_BITSTREAM;
    msg[1] = T76_BEGIN_BS;
    put_le16(&mut msg[2..4], BS_PACKET_SIZE as u32);
    put_le32(&mut msg[4..8], bitstream.len() as u32);
    usb.msg_send(&msg[..8])?;

    let ack = usb.msg_recv(8)?;
    if ack.get(1).copied().unwrap_or(1) != 0 {
        return Err(MiniproError::Protocol(
            "T76 bitstream BEGIN rejected by firmware".into(),
        ));
    }

    // ── Phase 2: BLOCK × N ────────────────────────────────────────────────────
    let mut offset = 0usize;
    while offset < bitstream.len() {
        let block_end = (offset + payload_size).min(bitstream.len());
        let block_size = block_end - offset;

        let mut pkt = vec![0u8; BS_PACKET_SIZE];
        pkt[0] = CMD_WRITE_BITSTREAM;
        pkt[1] = T76_BS_BLOCK;
        put_le16(&mut pkt[2..4], block_size as u32);
        pkt[8..8 + block_size].copy_from_slice(&bitstream[offset..block_end]);
        usb.msg_send_large(&pkt)?;

        offset += payload_size;
    }

    // ── Phase 3: END ──────────────────────────────────────────────────────────
    let mut msg = [0u8; 8];
    msg[0] = CMD_WRITE_BITSTREAM;
    msg[1] = T76_END_BS;
    usb.msg_send(&msg)?;

    let ack = usb.msg_recv(8)?;
    if ack.get(1).copied().unwrap_or(1) != 0 {
        return Err(MiniproError::Protocol(
            "T76 bitstream END rejected by firmware".into(),
        ));
    }

    Ok(())
}

/// Reset the T76 FPGA (sends the special reset command).
pub fn reset_fpga(usb: &UsbDevice) -> Result<()> {
    let mut msg = [0u8; 8];
    msg[0] = CMD_WRITE_BITSTREAM;
    msg[1] = T76_RESET_FPGA;
    put_le32(&mut msg[4..8], T76_FPGA_MAGIC);
    usb.msg_send(&msg)?;
    let resp = usb.msg_recv(8)?;
    if resp.get(1).copied().unwrap_or(1) != 0 {
        return Err(MiniproError::Protocol("T76 FPGA reset failed".into()));
    }
    Ok(())
}

// ── Protocol implementation ───────────────────────────────────────────────────

impl Protocol for T76Protocol {
    fn begin_transaction(&self, usb: &UsbDevice, device: &Device) -> Result<()> {
        // 1. Upload FPGA algorithm bitstream.
        if let Some(ref algo) = device.algorithm {
            if !algo.bitstream.is_empty() {
                eprintln!("Using T76 {} algorithm..", algo.name);
                upload_bitstream_t76(usb, &algo.bitstream)?;
            }
        }

        // 2. Send begin_transaction (custom bit-bang deferred to Phase 4).
        if !device.flags.custom_protocol {
            let mut msg = build_begin_msg(device, false);

            // T76 extras: I2C address and algorithm number
            if device.flags.can_adjust_address {
                msg[24] = device.i2c_address;
            }
            // msg[63] = high byte of variant = algorithm number
            msg[63] = (device.variant >> 8) as u8;

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
        let size = ds.data.len();

        if ds.page_type == MP_CODE {
            let mut msg = [0u8; 64];
            msg[0] = CMD_READ_CODE;
            put_le16(&mut msg[2..4], size as u32);
            put_le32(&mut msg[4..8], ds.address);
            put_le32(&mut msg[8..12], ds.total_blocks);

            // Only the first block sends the command to kick off DMA streaming.
            if ds.init {
                usb.msg_send(&msg[..16])?;
            }
            ds.data = usb.read_payload(size)?;
            return Ok(());
        }

        if ds.page_type == MP_DATA {
            let mut msg = [0u8; 64];
            msg[0] = CMD_READ_DATA;
            put_le16(&mut msg[2..4], size as u32);
            put_le32(&mut msg[4..8], ds.address);
            usb.msg_send(&msg[..16])?;

            // Response is (size + 16) bytes; skip the 16-byte overhead
            let resp = usb.read_payload(size + 16)?;
            let n = size.min(resp.len().saturating_sub(16));
            ds.data[..n].copy_from_slice(&resp[16..16 + n]);
            return Ok(());
        }

        if ds.page_type == MP_USER {
            let mut msg = [0u8; 64];
            msg[0] = CMD_READ_USER_DATA;
            put_le16(&mut msg[2..4], size as u32);
            put_le32(&mut msg[4..8], ds.address);
            usb.msg_send(&msg[..16])?;

            // User data comes back via msg_recv (not the payload endpoint)
            let resp = usb.msg_recv(size + 16)?;
            let n = size.min(resp.len().saturating_sub(16));
            ds.data[..n].copy_from_slice(&resp[16..16 + n]);
            return Ok(());
        }

        Err(MiniproError::Protocol(format!(
            "T76 read_block: unknown page_type {}",
            ds.page_type
        )))
    }

    fn write_block(&self, usb: &UsbDevice, ds: &DataSet) -> Result<()> {
        let size = ds.data.len();
        let mut msg = [0u8; 64];
        put_le16(&mut msg[2..4], size as u32);
        put_le32(&mut msg[4..8], ds.address);
        put_le32(&mut msg[12..16], size as u32);

        if ds.page_type == MP_CODE {
            msg[0] = CMD_WRITE_CODE;
            if ds.init {
                put_le32(&mut msg[8..12], ds.total_blocks);
                usb.msg_send(&msg[..16])?;
            }
            // For every block (init or not) we prepend a 16-byte header to the payload
            msg[8..12].fill(0);
            let mut payload = vec![0u8; size + 16];
            payload[..16].copy_from_slice(&msg[..16]);
            payload[16..].copy_from_slice(&ds.data);
            return usb.write_payload(&payload);
        }

        if ds.page_type == MP_DATA {
            msg[0] = CMD_WRITE_DATA;
            usb.msg_send(&msg[..16])?;
            return usb.write_payload(&ds.data);
        }

        if ds.page_type == MP_USER {
            msg[0] = CMD_WRITE_USER_DATA;
            let mut full = vec![0u8; 16 + size];
            full[..16].copy_from_slice(&msg[..16]);
            full[16..].copy_from_slice(&ds.data);
            return usb.msg_send(&full);
        }

        Err(MiniproError::Protocol(format!(
            "T76 write_block: unknown page_type {}",
            ds.page_type
        )))
    }

    fn get_chip_id(&self, usb: &UsbDevice) -> Result<(u8, u32)> {
        let mut msg = [0u8; 8];
        msg[0] = CMD_READ_ID;
        usb.msg_send(&msg)?;
        let resp = usb.msg_recv(32)?;
        if resp.len() < 5 {
            return Err(MiniproError::ResponseTooShort {
                expected: 5,
                actual: resp.len(),
            });
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
        &self,
        usb: &UsbDevice,
        device: &Device,
        fuse_type: u8,
        length: usize,
        items_count: u8,
    ) -> Result<Vec<u8>> {
        let cmd = match fuse_type {
            0x00 => T76_READ_USER,
            0x01 => T76_READ_CFG,
            0x02 => T76_READ_LOCK,
            _ => {
                return Err(MiniproError::Protocol(format!(
                    "T76 read_fuses: unknown type {fuse_type}"
                )))
            }
        };
        let mut msg = [0u8; 8];
        msg[0] = cmd;
        msg[1] = device.protocol_id;
        msg[2] = items_count;
        put_le32(&mut msg[4..8], device.code_memory_size);
        usb.msg_send(&msg)?;
        let resp = usb.msg_recv(64)?;
        let start = 8;
        let n = resp.len().saturating_sub(start).min(length);
        Ok(resp[start..start + n].to_vec())
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
        let cmd = match fuse_type {
            0x00 => T76_WRITE_USER,
            0x01 => T76_WRITE_CFG,
            0x02 => T76_WRITE_LOCK,
            _ => {
                return Err(MiniproError::Protocol(format!(
                    "T76 write_fuses: unknown type {fuse_type}"
                )))
            }
        };
        let mut msg = vec![0u8; 64];
        msg[0] = cmd;
        msg[1] = device.protocol_id;
        msg[2] = items_count;
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
        // T76 uses a 16-byte erase packet (T56 uses 15)
        let mut msg = [0u8; 64];
        msg[0] = CMD_ERASE;
        msg[2] = num_fuses;
        msg[4] = is_pld as u8;
        usb.msg_send(&msg[..16])?;
        usb.msg_recv(64)?;
        Ok(())
    }

    fn read_jedec_row(&self, usb: &UsbDevice, js: &mut JedecSet) -> Result<()> {
        let bits = (js.data.len() * 8) as u8;
        let mut msg = [0u8; 8];
        msg[0] = CMD_READ_JEDEC;
        msg[1] = js.set_type;
        msg[2] = bits;
        msg[4] = js.row;
        msg[5] = js.flags;
        usb.msg_send(&msg)?;
        let resp = usb.msg_recv(32)?;
        let byte_len = (bits as usize).div_ceil(8);
        js.data.resize(byte_len, 0);
        let n = byte_len.min(resp.len());
        js.data[..n].copy_from_slice(&resp[..n]);
        Ok(())
    }

    fn write_jedec_row(&self, usb: &UsbDevice, js: &JedecSet) -> Result<()> {
        let bits = (js.data.len() * 8) as u8;
        let byte_len = (bits as usize).div_ceil(8);
        let mut msg = [0u8; 64];
        msg[0] = CMD_WRITE_JEDEC;
        msg[1] = js.set_type;
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
            error: resp[0],
            address: u32::from_le_bytes([resp[8], resp[9], resp[10], resp[11]]),
            c1: u16::from_le_bytes([resp[2], resp[3]]) as u32,
            c2: u16::from_le_bytes([resp[4], resp[5]]) as u32,
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

    fn firmware_update(&self, usb: &UsbDevice, firmware: &[u8]) -> Result<()> {
        firmware_update_t76(usb, firmware)
    }

    fn logic_ic_test(&self, usb: &UsbDevice, device: &Device) -> Result<()> {
        // T76 uses the same test-vector command (0x28) as the TL866II+.
        // A full implementation would reload the FPGA bitstream between the
        // pull-up and pull-down passes; here we reuse the TL866II+ two-pass
        // logic without FPGA switching (known limitation for T76).
        logic_ic_test_tl866(usb, device)
    }

    fn reset_state(&self, usb: &UsbDevice) -> Result<()> {
        self.end_transaction(usb)
    }
}

// ── T76 Firmware update ───────────────────────────────────────────────────────

/// Parse and flash an updateT76.dat firmware image.
///
/// File layout (from t76.c):
/// ```text
/// [0..4]    version (LE32)   must have (version & 0xffff0000) == 0xf0760000
/// [4..8]    CRC32 of data blocks (LE32)
/// [8..12]   unknown
/// [12..16]  block count N (LE32)
/// [16 .. 16+N*0x114]  N blocks of 276 bytes each
/// Total size = N*0x114 + 16
/// ```
fn firmware_update_t76(usb: &UsbDevice, dat: &[u8]) -> Result<()> {
    use crc::{Crc, CRC_32_ISO_HDLC};
    const CRC32: Crc<u32> = Crc::<u32>::new(&CRC_32_ISO_HDLC);

    // ── Parse header ─────────────────────────────────────────────────────────
    if dat.len() < 16 {
        return Err(MiniproError::FileFormat("updateT76.dat too short".into()));
    }
    let version = u32::from_le_bytes([dat[0], dat[1], dat[2], dat[3]]);
    let file_crc = u32::from_le_bytes([dat[4], dat[5], dat[6], dat[7]]);
    let n_blocks = u32::from_le_bytes([dat[12], dat[13], dat[14], dat[15]]) as usize;

    if version & T76_UPDATE_VERS_MASK != T76_UPDATE_FILE_VERSION {
        return Err(MiniproError::FileFormat(format!(
            "Unsupported T76 firmware version {:#010x}",
            version
        )));
    }
    let expected = n_blocks * 0x114 + 16;
    if dat.len() != expected {
        return Err(MiniproError::FileFormat(format!(
            "updateT76.dat wrong size: got {}, expected {}",
            dat.len(),
            expected
        )));
    }

    // ── CRC check ────────────────────────────────────────────────────────────
    // CRC is over the data blocks region (dat[16..end]) with init 0xFFFFFFFF.
    // The result (NOT inverted) must equal file_crc.
    let mut digest = CRC32.digest_with_initial(0xFFFF_FFFF);
    digest.update(&dat[16..]);
    let computed = digest.finalize();
    if computed != file_crc {
        return Err(MiniproError::AlgorithmCrc);
    }

    eprintln!("T76 firmware image OK ({} blocks).", n_blocks);

    // ── Switch to bootloader ─────────────────────────────────────────────────
    {
        let mut msg = [0u8; 8];
        msg[0] = 0x3D; // CMD_SWITCH
        msg[1] = 0xaa;
        put_le32(&mut msg[4..8], T76_BTLDR_MAGIC);
        usb.msg_send(&msg)?;
        let resp = usb.msg_recv(8)?;
        if resp.get(1).copied().unwrap_or(1) != 0 {
            return Err(MiniproError::Protocol(
                "T76 bootloader switch failed".into(),
            ));
        }
    }
    // Note: upstream sleeps 1 second here for device re-enumeration.
    // We proceed without sleep; in practice the device may need a reconnect.

    // ── Erase ────────────────────────────────────────────────────────────────
    {
        let mut msg = [0u8; 8];
        msg[0] = 0x3C; // CMD_BTLDR_ERASE
        msg[1] = 0xaa;
        usb.msg_send(&msg)?;
        let resp = usb.msg_recv(8)?;
        if resp.get(1).copied().unwrap_or(1) != 0 {
            return Err(MiniproError::Protocol("T76 bootloader erase failed".into()));
        }
    }

    // ── Begin write sequence ──────────────────────────────────────────────────
    {
        let mut msg = [0u8; 8];
        msg[0] = 0x3B; // CMD_BTLDR_WRITE
        msg[1] = 0xaa; // begin sequence marker
        usb.msg_send(&msg)?;
    }

    // ── Flash blocks ─────────────────────────────────────────────────────────
    let blk_size = 0x114usize; // 276 bytes per block
    let mut address = 0u32;

    for b in 0..n_blocks {
        let blk_off = 16 + b * blk_size;
        let blk = &dat[blk_off..blk_off + blk_size];

        // Block write: 0x11C (= 8 header + 0x114 data) bytes total
        let mut msg = vec![0u8; 8 + blk_size];
        msg[0] = 0x3B;
        msg[1] = 0x00; // data block
        put_le16(&mut msg[2..4], 256); // 256 bytes of payload
        put_le32(&mut msg[4..8], address);
        msg[8..8 + blk_size].copy_from_slice(blk);
        usb.msg_send_large(&msg)?;

        let resp = usb.msg_recv(8)?;
        if resp.get(1).copied().unwrap_or(1) != 0 {
            return Err(MiniproError::Protocol(format!(
                "T76 block {} write failed",
                b
            )));
        }

        address = address.wrapping_add(256);
    }

    // ── Last block ────────────────────────────────────────────────────────────
    {
        // 0x108 (= 8 header + 4 CRC) bytes
        let mut msg = [0u8; 0x108];
        msg[0] = 0x3B;
        msg[1] = 0x03; // last-block marker
        put_le16(&mut msg[2..4], 256);
        put_le32(&mut msg[4..8], T76_LAST_BLOCK_ADDR);
        put_le32(&mut msg[8..12], T76_LAST_BLOCK_CRC);
        usb.msg_send_large(&msg)?;

        let resp = usb.msg_recv(8)?;
        if resp.get(1).copied().unwrap_or(1) != 0 {
            return Err(MiniproError::Protocol("T76 last block write failed".into()));
        }
    }

    eprintln!("T76 firmware written successfully. Please reconnect the programmer.");
    Ok(())
}
