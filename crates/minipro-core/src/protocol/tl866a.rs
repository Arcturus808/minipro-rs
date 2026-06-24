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

use super::{DataSet, Device, JedecSet, OvcStatus, Protocol};
use crate::{
    error::{MiniproError, Result},
    usb::UsbDevice,
};
use log::trace;

/// Minimum firmware version for TL866A/CS.
pub const MIN_FIRMWARE_A: u32 = 0x0256; // 3.2.86

// Command bytes
const CMD_GET_INFO: u8 = 0x00;
const CMD_START_TRANSACTION: u8 = 0x03;
const CMD_END_TRANSACTION: u8 = 0x04;
const CMD_GET_CHIP_ID: u8 = 0x05;
const CMD_READ_USER: u8 = 0x10;
const CMD_WRITE_USER: u8 = 0x11;
const CMD_READ_CFG: u8 = 0x12;
const CMD_WRITE_CFG: u8 = 0x13;
const CMD_WRITE_USER_DATA: u8 = 0x14;
const CMD_READ_USER_DATA: u8 = 0x15;
const CMD_WRITE_CODE: u8 = 0x20;
const CMD_READ_CODE: u8 = 0x21;
const CMD_ERASE: u8 = 0x22;
const CMD_READ_DATA: u8 = 0x30;
const CMD_WRITE_DATA: u8 = 0x31;
const CMD_WRITE_LOCK: u8 = 0x40;
const CMD_READ_LOCK: u8 = 0x41;
const CMD_READ_CALIBRATION: u8 = 0x42;
const CMD_PROTECT_OFF: u8 = 0x44;
const CMD_PROTECT_ON: u8 = 0x45;
const CMD_AUTODETECT: u8 = 0xFC;
const CMD_UNLOCK_TSOP48: u8 = 0xFD;
const CMD_GET_STATUS: u8 = 0xFE;
const CMD_RESET_PIN_DRIVERS: u8 = 0xD0;

// Memory page types
const MP_DATA: u8 = 0x01;
const MP_USER: u8 = 0x02;

// Fuse sub-types
const MP_FUSE_USER: u8 = 0x00;
const MP_FUSE_CFG: u8 = 0x01;
const MP_FUSE_LOCK: u8 = 0x02;

pub struct Tl866aProtocol {
    /// Cached protocol_id from the last begin_transaction, needed by
    /// read_block and write_block (byte [1] of every command packet).
    protocol_id: std::sync::atomic::AtomicU8,
    /// Low byte of the device variant, cached from begin_transaction.
    /// Used in end_transaction and get_ovc_status to match the C msg_init.
    variant_lo: std::sync::atomic::AtomicU8,
}

impl Tl866aProtocol {
    pub fn new() -> Self {
        Self {
            protocol_id: std::sync::atomic::AtomicU8::new(0),
            variant_lo: std::sync::atomic::AtomicU8::new(0),
        }
    }
}

impl Default for Tl866aProtocol {
    fn default() -> Self {
        Self::new()
    }
}

/// Query firmware version and device info from a TL866A/CS.
///
/// Response layout (40 bytes, from tl866a.h):
/// ```text
/// u8      echo (0x00)
/// u8      device_status      (1 = normal, 2 = bootloader)
/// u16     report_size        (LE)
/// u8      firmware_minor
/// u8      firmware_major
/// u8      hardware_version
/// u8      device_type        (1 = TL866A, 2 = TL866CS)
/// u8[8]   device_code
/// u8[24]  serial_number
/// ```
pub fn get_system_info(usb: &UsbDevice) -> Result<crate::protocol::tl866iiplus::SystemInfo> {
    use crate::device::{ProgrammerModel, ProgrammerStatus};

    let mut cmd = [0u8; 64];
    cmd[0] = CMD_GET_INFO;
    usb.msg_send(&cmd)?;
    let resp = usb.msg_recv(64)?;

    if resp.len() < 40 {
        return Err(MiniproError::ResponseTooShort {
            expected: 40,
            actual: resp.len(),
        });
    }

    let status = match resp[1] {
        2 => ProgrammerStatus::Bootloader,
        _ => ProgrammerStatus::Normal,
    };

    let fw_minor = resp[4];
    let fw_major = resp[5];
    let firmware = ((fw_major as u32) << 8) | fw_minor as u32;
    let hardware_version = resp[6];
    let device_type = resp[7];

    let model = match device_type {
        2 => ProgrammerModel::Tl866cs,
        _ => ProgrammerModel::Tl866a,
    };

    let device_code = String::from_utf8_lossy(&resp[8..16])
        .trim_end_matches('\0')
        .to_string();
    let serial_number = String::from_utf8_lossy(&resp[16..40])
        .trim_end_matches('\0')
        .to_string();
    // TL866A/CS firmware: raw major.minor (upstream C minipro formats as "major.minor:02")
    let firmware_str = format!("{}.{:02}", fw_major, fw_minor);

    Ok(crate::protocol::tl866iiplus::SystemInfo {
        model,
        status,
        firmware,
        firmware_str,
        device_code,
        serial_number,
        hardware_version,
    })
}

/// Store a little-endian integer of `len` bytes starting at `buf`.
fn put_le(buf: &mut [u8], val: u32, len: usize) {
    for (i, b) in buf[..len].iter_mut().enumerate() {
        *b = (val >> (i * 8)) as u8;
    }
}

impl Protocol for Tl866aProtocol {
    fn begin_transaction(&self, usb: &UsbDevice, device: &Device, icsp: bool) -> Result<()> {
        self.protocol_id
            .store(device.protocol_id, std::sync::atomic::Ordering::Relaxed);
        self.variant_lo
            .store(device.variant as u8, std::sync::atomic::Ordering::Relaxed);
        let mut msg = [0u8; 64];
        // Matches C tl866a_begin_transaction packet layout.
        // msg_init() sets [0]=cmd, [1]=protocol_id, [2]=variant_lo.
        // The subsequent field writes start at [3] (NOT [2]), so variant_lo
        // at [2] is preserved — it is NOT overwritten by data_memory_size.
        msg[0] = CMD_START_TRANSACTION;
        msg[1] = device.protocol_id;
        // [2]     variant (low byte) — kept per C msg_init
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
        // [10]    icsp
        msg[11] = icsp as u8;
        // [12..14] code_memory_size (24-bit LE)
        put_le(&mut msg[12..], device.code_memory_size, 3);
        usb.msg_send(&msg[..48])?;
        // C tl866a_begin_transaction calls get_ovc_status immediately after
        // sending the begin packet.  This clears any stale OVC latch from the
        // previous operation (e.g. power-on inrush after a transaction reset).
        // We call it here too, but don't fail on OVC since some adapter/chip
        // combinations produce a transient flag at power-up that self-clears.
        match self.get_ovc_status(usb) {
            Ok((_, ovc)) => {
                if ovc != 0 {
                    trace!(
                        "begin_transaction: OVC flag set ({:#04x}); ignoring transient",
                        ovc
                    );
                }
            }
            Err(e) => {
                trace!(
                    "begin_transaction: get_ovc_status failed: {}; continuing",
                    e
                );
            }
        }
        Ok(())
    }

    fn end_transaction(&self, usb: &UsbDevice) -> Result<()> {
        let pid = self.protocol_id.load(std::sync::atomic::Ordering::Relaxed);
        let var = self.variant_lo.load(std::sync::atomic::Ordering::Relaxed);
        usb.msg_send(&[CMD_END_TRANSACTION, pid, var, 0])?;
        Ok(())
    }

    fn read_block(&self, usb: &UsbDevice, _device: &Device, ds: &mut DataSet) -> Result<()> {
        let cmd = match ds.page_type {
            MP_DATA => CMD_READ_DATA,
            MP_USER => CMD_READ_USER_DATA,
            _ => CMD_READ_CODE,
        };
        let mut msg = [0u8; 18];
        msg[0] = cmd;
        msg[1] = self.protocol_id.load(std::sync::atomic::Ordering::Relaxed);
        // [2..3] size (16-bit LE)  — overwrites variant byte intentionally
        put_le(&mut msg[2..], ds.data.len() as u32, 2);
        // [4..6] address (24-bit LE)
        put_le(&mut msg[4..], ds.address, 3);
        trace!(
            "TL866A read_block: cmd=0x{:02x} protocol_id={} addr={:#x} len={} block_count={}",
            cmd,
            msg[1],
            ds.address,
            ds.data.len(),
            ds.block_count
        );
        usb.msg_send(&msg)?;
        // TL866A sends all payload data on EP 0x81 (command IN endpoint) in
        // 64-byte USB packets.  We must NOT use read_payload_limit here —
        // that reads from EP 0x82 which does not carry data on TL866A and
        // would hang indefinitely.
        let mut buf = Vec::with_capacity(ds.data.len());
        while buf.len() < ds.data.len() {
            let chunk = usb.msg_recv(64)?;
            let take = chunk.len().min(ds.data.len() - buf.len());
            buf.extend_from_slice(&chunk[..take]);
        }
        trace!("TL866A read_block: received {} bytes", buf.len());
        let len = ds.data.len();
        ds.data.copy_from_slice(&buf[..len]);
        Ok(())
    }

    fn write_block(&self, usb: &UsbDevice, _device: &Device, ds: &DataSet) -> Result<()> {
        let cmd = match ds.page_type {
            MP_DATA => CMD_WRITE_DATA,
            MP_USER => CMD_WRITE_USER_DATA,
            _ => CMD_WRITE_CODE,
        };
        // Packet layout: [cmd, protocol_id, size(2), addr(3), data...]
        // C reference sends exactly ds->size + 7 bytes (no 64-byte padding).
        let mut payload = vec![0u8; ds.data.len() + 7];
        payload[0] = cmd;
        payload[1] = self.protocol_id.load(std::sync::atomic::Ordering::Relaxed);
        put_le(&mut payload[2..], ds.data.len() as u32, 2);
        put_le(&mut payload[4..], ds.address, 3);
        payload[7..].copy_from_slice(&ds.data);
        usb.msg_send_large(&payload)?;
        Ok(())
    }

    fn get_chip_id(&self, usb: &UsbDevice) -> Result<(u8, u32)> {
        let mut msg = [0u8; 8];
        msg[0] = CMD_GET_CHIP_ID;
        usb.msg_send(&msg)?;
        let resp = usb.msg_recv(32)?;
        if resp.len() < 5 {
            return Err(MiniproError::ResponseTooShort {
                expected: 5,
                actual: resp.len(),
            });
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
            return Err(MiniproError::ResponseTooShort {
                expected: 5,
                actual: resp.len(),
            });
        }
        Ok(u32::from_be_bytes([0, resp[2], resp[3], resp[4]]))
    }

    fn read_fuses(
        &self,
        usb: &UsbDevice,
        _device: &Device,
        fuse_type: u8,
        _length: usize,
        items_count: u8,
    ) -> Result<Vec<u8>> {
        let cmd = match fuse_type {
            MP_FUSE_USER => CMD_READ_USER,
            MP_FUSE_CFG => CMD_READ_CFG,
            MP_FUSE_LOCK => CMD_READ_LOCK,
            other => return Err(MiniproError::Protocol(format!("unknown fuse type {other}"))),
        };
        let mut msg = [0u8; 18];
        msg[0] = cmd;
        msg[1] = self.protocol_id.load(std::sync::atomic::Ordering::Relaxed);
        msg[2] = items_count;
        usb.msg_send(&msg)?;
        let resp = usb.msg_recv(64)?;
        Ok(resp.get(7..).unwrap_or_default().to_vec())
    }

    fn write_fuses(
        &self,
        usb: &UsbDevice,
        _device: &Device,
        fuse_type: u8,
        _length: usize,
        items_count: u8,
        data: &[u8],
    ) -> Result<()> {
        let cmd = match fuse_type {
            MP_FUSE_USER => CMD_WRITE_USER,
            MP_FUSE_CFG => CMD_WRITE_CFG,
            MP_FUSE_LOCK => CMD_WRITE_LOCK,
            other => return Err(MiniproError::Protocol(format!("unknown fuse type {other}"))),
        };
        let mut msg = [0u8; 64];
        msg[0] = cmd;
        msg[1] = self.protocol_id.load(std::sync::atomic::Ordering::Relaxed);
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

    fn erase(&self, usb: &UsbDevice, _device: &Device, num_fuses: u8, _is_pld: bool) -> Result<()> {
        let mut msg = [0u8; 15];
        msg[0] = CMD_ERASE;
        msg[1] = self.protocol_id.load(std::sync::atomic::Ordering::Relaxed);
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
        let pid = self.protocol_id.load(std::sync::atomic::Ordering::Relaxed);
        let var = self.variant_lo.load(std::sync::atomic::Ordering::Relaxed);
        usb.msg_send(&[CMD_GET_STATUS, pid, var, 0, 0])?;
        let resp = usb.msg_recv(64)?;
        if resp.len() < 10 {
            return Err(MiniproError::ResponseTooShort {
                expected: 10,
                actual: resp.len(),
            });
        }
        let status = OvcStatus {
            error: resp[0],
            address: u32::from_le_bytes([resp[6], resp[7], resp[8], 0]),
            c1: u32::from_le_bytes([resp[2], resp[3], 0, 0]),
            c2: u32::from_le_bytes([resp[4], resp[5], 0, 0]),
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
            crc = crc.rotate_left(8);
            crc ^= byte as u16;
            crc ^= (crc & 0xff) >> 4;
            crc ^= crc << 12;
            crc ^= (crc & 0xff) << 5;
        }
        // Swap two bytes then embed CRC
        msg[15] = msg[9];
        msg[16] = msg[11];
        msg[9] = crc as u8;
        msg[11] = (crc >> 8) as u8;
        usb.msg_send(&msg)?;
        let resp = usb.msg_recv(64)?;
        Ok(resp.get(1).copied().unwrap_or(0))
    }

    fn hardware_check(&self, _usb: &UsbDevice) -> Result<()> {
        // Full ZIF pin matrix self-test — deferred to Phase 4
        Err(MiniproError::UnsupportedOperation)
    }

    fn logic_ic_test(
        &self,
        usb: &UsbDevice,
        device: &Device,
        out: &mut dyn std::io::Write,
    ) -> Result<()> {
        // End any active transaction before bit-banging (matches upstream C)
        self.end_transaction(usb)?;
        tl866a_logic_ic_test(usb, device, out)
    }

    fn firmware_update(&self, _usb: &UsbDevice, _firmware: &[u8]) -> Result<()> {
        // TL866A firmware update is handled in operations.rs via firmware_update_tl866a
        Err(MiniproError::UnsupportedOperation)
    }

    fn reset_state(&self, usb: &UsbDevice) -> Result<()> {
        usb.msg_send(&[CMD_RESET_PIN_DRIVERS, 0, 0, 0, 0, 0, 0, 0])?;
        Ok(())
    }
}

// ── Firmware update (TL866A / TL866CS) ───────────────────────────────────────

// Bootloader commands
const CMD_BTLDR_WRITE: u8 = 0xAA;
const CMD_BTLDR_ERASE: u8 = 0xCC;
const CMD_RESET: u8 = 0xFF;

/// Size of an update.dat file for TL866A/CS.
const UPDATE_DAT_SIZE: usize = 312_348;

/// Encrypted firmware size in the update.dat file (0x25C00).
const ENC_FIRMWARE_SIZE: usize = 0x25C00; // 154_880

/// Bootloader offset (where firmware starts in the PIC flash).
const BOOTLOADER_SIZE: u32 = 0x1800; // 6_144

/// Flash block size for bootloader writes.
const BLOCK_SIZE: usize = 0x50; // 80

/// Parse, decrypt, verify and flash a TL866A/CS `update.dat` firmware image.
///
/// The file is exactly 312_348 bytes and contains both A and CS firmware
/// images (encrypted with a simple XOR cipher).
///
/// Supported on stock firmware up to 03.2.82; newer firmware versions
/// (03.2.84+) may block the software reset-to-bootloader mechanism.
pub fn firmware_update_tl866a(
    handle: &mut crate::handle::MiniproHandle,
    dat: &[u8],
    out: &mut dyn std::io::Write,
    mut progress: Option<&mut dyn FnMut(usize, usize)>,
) -> Result<()> {
    use crate::device::{ProgrammerModel, ProgrammerStatus};
    use crc::{Crc, CRC_32_ISO_HDLC};
    const CRC32: Crc<u32> = Crc::<u32>::new(&CRC_32_ISO_HDLC);

    // ── Validate file size ───────────────────────────────────────────────────
    if dat.len() != UPDATE_DAT_SIZE {
        return Err(MiniproError::FileFormat(format!(
            "update.dat size mismatch: got {}, expected {}",
            dat.len(),
            UPDATE_DAT_SIZE
        )));
    }

    // ── Parse header offsets (packed layout) ─────────────────────────────────
    // 0..4      header[4]
    // 4..8      a_crc32
    // 8..12     pad1 + a_erase + pad2 + pad3
    // 12..16    cs_crc32
    // 16..20    pad4 + cs_erase + pad5 + pad6
    // 20..24    a_index
    // 24..280   a_xortable1[256]
    // 280..1304 a_xortable2[1024]
    // 1304..1308 cs_index
    // 1308..1564 cs_xortable1[256]
    // 1564..2588 cs_xortable2[1024]
    // 2588..157980 a_firmware[154880]
    // 157980..312348 cs_firmware[154880]

    let a_crc32 = u32::from_le_bytes([dat[4], dat[5], dat[6], dat[7]]);
    let a_erase = dat[9];
    let cs_crc32 = u32::from_le_bytes([dat[12], dat[13], dat[14], dat[15]]);
    let cs_erase = dat[17];
    let a_index = u32::from_le_bytes([dat[20], dat[21], dat[22], dat[23]]);
    let a_xortable1 = &dat[24..280];
    let a_xortable2 = &dat[280..1304];
    let cs_index = u32::from_le_bytes([dat[1304], dat[1305], dat[1306], dat[1307]]);
    let cs_xortable1 = &dat[1308..1564];
    let cs_xortable2 = &dat[1564..2588];
    let a_firmware_enc = &dat[2588..2588 + ENC_FIRMWARE_SIZE];
    let cs_firmware_enc = &dat[2588 + ENC_FIRMWARE_SIZE..];

    // ── Decrypt ──────────────────────────────────────────────────────────────
    let mut a_firmware = vec![0u8; ENC_FIRMWARE_SIZE];
    let mut cs_firmware = vec![0u8; ENC_FIRMWARE_SIZE];

    for i in 0..ENC_FIRMWARE_SIZE {
        a_firmware[i] = a_firmware_enc[i]
            ^ a_xortable2[((i as u32 + a_index) & 0x3FF) as usize]
            ^ a_xortable1[(i / 80) & 0xFF];
        cs_firmware[i] = cs_firmware_enc[i]
            ^ cs_xortable2[((i as u32 + cs_index) & 0x3FF) as usize]
            ^ cs_xortable1[(i / 80) & 0xFF];
    }

    // ── CRC check ────────────────────────────────────────────────────────────
    let a_computed = CRC32.checksum(&a_firmware);
    let cs_computed = CRC32.checksum(&cs_firmware);
    if a_computed != a_crc32 || cs_computed != cs_crc32 {
        return Err(MiniproError::AlgorithmCrc);
    }

    writeln!(out, "Firmware image OK (decrypted A + CS firmware).").ok();

    // ── Select firmware matching current device ──────────────────────────────
    let (firmware, erase_byte) = match handle.info.model {
        ProgrammerModel::Tl866cs => (&cs_firmware[..], cs_erase),
        _ => (&a_firmware[..], a_erase), // TL866A or unknown -> use A firmware
    };

    let file_version_minor = dat[0];
    writeln!(
        out,
        "update.dat contains firmware version 03.2.{:02}",
        file_version_minor
    )
    .ok();
    if (handle.info.firmware & 0xFF) > file_version_minor as u32 {
        writeln!(out, "  (older than current firmware)").ok();
    } else if (handle.info.firmware & 0xFF) < file_version_minor as u32 {
        writeln!(out, "  (newer than current firmware)").ok();
    }

    // ── Switch to bootloader ─────────────────────────────────────────────────
    if handle.info.status != ProgrammerStatus::Bootloader {
        write!(out, "Switching to bootloader... ").ok();
        let mut msg = [0u8; 4];
        msg[0] = CMD_RESET;
        handle.usb.msg_send(&msg)?;
        handle.reconnect(true)?;
        if handle.info.status != ProgrammerStatus::Bootloader {
            return Err(MiniproError::Protocol("Bootloader switch failed".into()));
        }
        writeln!(out, "OK").ok();
    }

    // ── Erase ──────────────────────────────────────────────────────────────────
    write!(out, "Erasing... ").ok();
    {
        let mut msg = [0u8; 20];
        msg[0] = CMD_BTLDR_ERASE;
        msg[7] = erase_byte;
        handle.usb.msg_send(&msg)?;
        let resp = handle.usb.msg_recv(32)?;
        if resp.first().copied() != Some(CMD_BTLDR_ERASE) {
            return Err(MiniproError::Protocol("Bootloader erase rejected".into()));
        }
    }
    writeln!(out, "OK").ok();

    // ── Flash ─────────────────────────────────────────────────────────────────
    write!(out, "Reflashing {} firmware... ", handle.info.model).ok();
    let n_blocks = ENC_FIRMWARE_SIZE / BLOCK_SIZE; // 154880 / 80 = 1936
    let mut address = BOOTLOADER_SIZE;
    for b in 0..n_blocks {
        let blk = &firmware[b * BLOCK_SIZE..(b + 1) * BLOCK_SIZE];
        let mut msg = [0u8; 87]; // 7 byte header + 80 bytes data
        msg[0] = CMD_BTLDR_WRITE;
        msg[1] = 0;
        msg[2] = BLOCK_SIZE as u8;
        msg[3] = 0;
        msg[4] = (address & 0xFF) as u8;
        msg[5] = ((address >> 8) & 0xFF) as u8;
        msg[6] = ((address >> 16) & 0xFF) as u8;
        msg[7..7 + BLOCK_SIZE].copy_from_slice(blk);
        handle.usb.msg_send(&msg)?;
        address += 64; // Next block address (64-byte alignment)
        if let Some(ref mut cb) = progress {
            cb(b + 1, n_blocks);
        }
    }
    writeln!(out, "100%").ok();

    // ── Reset back to normal mode ──────────────────────────────────────────────
    write!(out, "Resetting device... ").ok();
    {
        let mut msg = [0u8; 4];
        msg[0] = CMD_RESET;
        handle.usb.msg_send(&msg)?;
    }
    handle.reconnect(false)?;
    if handle.info.status != ProgrammerStatus::Normal {
        return Err(MiniproError::Protocol(
            "Device did not return to normal mode after firmware update".into(),
        ));
    }
    writeln!(out, "OK").ok();

    writeln!(out, "Firmware update completed successfully.").ok();
    Ok(())
}

// ── Logic IC test (TL866A / TL866CS) ───────────────────────────────────────────

// Bit-banging commands (not used by normal read/write)
const CMD_SET_LATCH: u8 = 0xD1;
const CMD_READ_ZIF_PINS: u8 = 0xD2;
const CMD_SET_DIR: u8 = 0xD4;
const CMD_SET_OUT: u8 = 0xD5;
#[allow(dead_code)]
const CMD_POWER_ON: u8 = 0x51;

// OE (output-enable) constants for latch setup
#[allow(dead_code)]
const OE_NONE: u8 = 0x00;
#[allow(dead_code)]
const OE_VPP: u8 = 0x01;
const OE_VCC_GND: u8 = 0x02;
#[allow(dead_code)]
const OE_ALL: u8 = 0x03;

/// One ZIF pin driver entry (maps logical pin → latch register + mask).
#[derive(Clone, Copy)]
#[allow(dead_code)]
struct ZifPin {
    pin: u8,
    latch: u8,
    oe: u8,
    mask: u8,
}

/// 16 VPP pins (NPN transistor, mask bits).
#[allow(dead_code)]
const VPP_PINS: &[ZifPin] = &[
    ZifPin {
        pin: 1,
        latch: 1,
        oe: 1,
        mask: 0x04,
    },
    ZifPin {
        pin: 2,
        latch: 1,
        oe: 1,
        mask: 0x08,
    },
    ZifPin {
        pin: 3,
        latch: 0,
        oe: 1,
        mask: 0x04,
    },
    ZifPin {
        pin: 4,
        latch: 0,
        oe: 1,
        mask: 0x08,
    },
    ZifPin {
        pin: 9,
        latch: 0,
        oe: 1,
        mask: 0x20,
    },
    ZifPin {
        pin: 10,
        latch: 0,
        oe: 1,
        mask: 0x10,
    },
    ZifPin {
        pin: 30,
        latch: 1,
        oe: 1,
        mask: 0x01,
    },
    ZifPin {
        pin: 31,
        latch: 0,
        oe: 1,
        mask: 0x01,
    },
    ZifPin {
        pin: 32,
        latch: 1,
        oe: 1,
        mask: 0x80,
    },
    ZifPin {
        pin: 33,
        latch: 0,
        oe: 1,
        mask: 0x40,
    },
    ZifPin {
        pin: 34,
        latch: 0,
        oe: 1,
        mask: 0x02,
    },
    ZifPin {
        pin: 36,
        latch: 1,
        oe: 1,
        mask: 0x02,
    },
    ZifPin {
        pin: 37,
        latch: 0,
        oe: 1,
        mask: 0x80,
    },
    ZifPin {
        pin: 38,
        latch: 1,
        oe: 1,
        mask: 0x40,
    },
    ZifPin {
        pin: 39,
        latch: 1,
        oe: 1,
        mask: 0x20,
    },
    ZifPin {
        pin: 40,
        latch: 1,
        oe: 1,
        mask: 0x10,
    },
];

/// 24 VCC pins (PNP transistor, mask bits).
const VCC_PINS: &[ZifPin] = &[
    ZifPin {
        pin: 1,
        latch: 2,
        oe: 2,
        mask: 0x7f,
    },
    ZifPin {
        pin: 2,
        latch: 2,
        oe: 2,
        mask: 0xef,
    },
    ZifPin {
        pin: 3,
        latch: 2,
        oe: 2,
        mask: 0xdf,
    },
    ZifPin {
        pin: 4,
        latch: 3,
        oe: 2,
        mask: 0xfe,
    },
    ZifPin {
        pin: 5,
        latch: 2,
        oe: 2,
        mask: 0xfb,
    },
    ZifPin {
        pin: 6,
        latch: 3,
        oe: 2,
        mask: 0xfb,
    },
    ZifPin {
        pin: 7,
        latch: 4,
        oe: 2,
        mask: 0xbf,
    },
    ZifPin {
        pin: 8,
        latch: 4,
        oe: 2,
        mask: 0xfd,
    },
    ZifPin {
        pin: 9,
        latch: 4,
        oe: 2,
        mask: 0xfb,
    },
    ZifPin {
        pin: 10,
        latch: 4,
        oe: 2,
        mask: 0xf7,
    },
    ZifPin {
        pin: 11,
        latch: 4,
        oe: 2,
        mask: 0xfe,
    },
    ZifPin {
        pin: 12,
        latch: 4,
        oe: 2,
        mask: 0x7f,
    },
    ZifPin {
        pin: 13,
        latch: 4,
        oe: 2,
        mask: 0xef,
    },
    ZifPin {
        pin: 21,
        latch: 4,
        oe: 2,
        mask: 0xdf,
    },
    ZifPin {
        pin: 30,
        latch: 3,
        oe: 2,
        mask: 0xbf,
    },
    ZifPin {
        pin: 32,
        latch: 3,
        oe: 2,
        mask: 0xfd,
    },
    ZifPin {
        pin: 33,
        latch: 3,
        oe: 2,
        mask: 0xdf,
    },
    ZifPin {
        pin: 34,
        latch: 3,
        oe: 2,
        mask: 0xf7,
    },
    ZifPin {
        pin: 35,
        latch: 3,
        oe: 2,
        mask: 0xef,
    },
    ZifPin {
        pin: 36,
        latch: 3,
        oe: 2,
        mask: 0x7f,
    },
    ZifPin {
        pin: 37,
        latch: 2,
        oe: 2,
        mask: 0xf7,
    },
    ZifPin {
        pin: 38,
        latch: 2,
        oe: 2,
        mask: 0xbf,
    },
    ZifPin {
        pin: 39,
        latch: 2,
        oe: 2,
        mask: 0xfe,
    },
    ZifPin {
        pin: 40,
        latch: 2,
        oe: 2,
        mask: 0xfd,
    },
];

/// 25 GND pins (NPN transistor, mask bits).
const GND_PINS: &[ZifPin] = &[
    ZifPin {
        pin: 1,
        latch: 6,
        oe: 2,
        mask: 0x04,
    },
    ZifPin {
        pin: 2,
        latch: 6,
        oe: 2,
        mask: 0x08,
    },
    ZifPin {
        pin: 3,
        latch: 6,
        oe: 2,
        mask: 0x40,
    },
    ZifPin {
        pin: 4,
        latch: 6,
        oe: 2,
        mask: 0x02,
    },
    ZifPin {
        pin: 5,
        latch: 5,
        oe: 2,
        mask: 0x04,
    },
    ZifPin {
        pin: 6,
        latch: 5,
        oe: 2,
        mask: 0x08,
    },
    ZifPin {
        pin: 7,
        latch: 5,
        oe: 2,
        mask: 0x40,
    },
    ZifPin {
        pin: 8,
        latch: 5,
        oe: 2,
        mask: 0x02,
    },
    ZifPin {
        pin: 9,
        latch: 5,
        oe: 2,
        mask: 0x01,
    },
    ZifPin {
        pin: 10,
        latch: 5,
        oe: 2,
        mask: 0x80,
    },
    ZifPin {
        pin: 11,
        latch: 5,
        oe: 2,
        mask: 0x10,
    },
    ZifPin {
        pin: 12,
        latch: 5,
        oe: 2,
        mask: 0x20,
    },
    ZifPin {
        pin: 14,
        latch: 7,
        oe: 2,
        mask: 0x08,
    },
    ZifPin {
        pin: 16,
        latch: 7,
        oe: 2,
        mask: 0x40,
    },
    ZifPin {
        pin: 20,
        latch: 9,
        oe: 2,
        mask: 0x01,
    },
    ZifPin {
        pin: 30,
        latch: 7,
        oe: 2,
        mask: 0x04,
    },
    ZifPin {
        pin: 31,
        latch: 6,
        oe: 2,
        mask: 0x01,
    },
    ZifPin {
        pin: 32,
        latch: 6,
        oe: 2,
        mask: 0x80,
    },
    ZifPin {
        pin: 34,
        latch: 6,
        oe: 2,
        mask: 0x10,
    },
    ZifPin {
        pin: 35,
        latch: 6,
        oe: 2,
        mask: 0x20,
    },
    ZifPin {
        pin: 36,
        latch: 7,
        oe: 2,
        mask: 0x20,
    },
    ZifPin {
        pin: 37,
        latch: 7,
        oe: 2,
        mask: 0x10,
    },
    ZifPin {
        pin: 38,
        latch: 7,
        oe: 2,
        mask: 0x02,
    },
    ZifPin {
        pin: 39,
        latch: 7,
        oe: 2,
        mask: 0x80,
    },
    ZifPin {
        pin: 40,
        latch: 7,
        oe: 2,
        mask: 0x01,
    },
];

/// ZIF socket physical-to-logical pin mapping (40 pins).
const ZIF_TABLE: &[u8] = &[
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37,
    38, 39, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
];

/// Logic vector constants (match C `enum logic` indices and `pst[] = "01LHCZXGV"`).
const LOGIC_0: u8 = 0;
const LOGIC_1: u8 = 1;
const LOGIC_L: u8 = 2;
const LOGIC_H: u8 = 3;
const LOGIC_C: u8 = 4;
const LOGIC_Z: u8 = 5;
#[allow(dead_code)]
const LOGIC_X: u8 = 6;
const LOGIC_G: u8 = 7;
const LOGIC_V: u8 = 8;

/// Convert logical pin index to physical ZIF pin number.
fn pin_map(pin: usize, count: usize) -> usize {
    if pin < count / 2 {
        pin
    } else {
        40 - count + pin
    }
}

/// Find a power-pin entry for the given physical pin number.
fn get_pwr_pin(pin: u8, table: &[ZifPin]) -> Option<&ZifPin> {
    // Pins 17-24 have no VCC/GND driver on TL866A/CS
    if pin > 16 && pin < 25 {
        return None;
    }
    table.iter().find(|p| p.pin == pin)
}

/// Initialise power pins (VCC / GND) and enable pull-up resistors.
fn pwr_init(usb: &UsbDevice, vector: &[u8], pin_count: usize) -> Result<()> {
    // Step 1: reset pin drivers and enable all pull-ups
    let mut pwr = [0u8; 21];
    pwr[0] = CMD_RESET_PIN_DRIVERS;
    pwr[1] = 0x01;
    pwr[7] = 0x06;
    pwr[8] = OE_VCC_GND;
    pwr[9] = 0x02;
    pwr[10] = 0xff;
    pwr[11] = 0x03;
    pwr[12] = 0xff;
    pwr[13] = 0x04;
    pwr[14] = 0xff;
    pwr[15] = 0x05;
    pwr[16] = 0x00;
    pwr[17] = 0x06;
    pwr[18] = 0x00;
    pwr[19] = 0x07;
    pwr[20] = 0x00;
    usb.msg_send(&pwr[..10])?;

    // Step 2: configure VCC / GND latches according to vector
    pwr[0] = CMD_SET_LATCH;
    for i in 0..pin_count {
        let pin = pin_map(i, pin_count) as u8 + 1; // 1-based physical pin
        let idx = i; // vector index
        match vector[idx] {
            LOGIC_G => {
                let p = get_pwr_pin(pin, GND_PINS).ok_or_else(|| {
                    MiniproError::Protocol(format!(
                        "Pin {} not supported for GND on TL866A/CS",
                        pin
                    ))
                })?;
                let off = (p.latch - 2) as usize * 2 + 10;
                pwr[off] |= p.mask;
            }
            LOGIC_V => {
                let p = get_pwr_pin(pin, VCC_PINS).ok_or_else(|| {
                    MiniproError::Protocol(format!(
                        "Pin {} not supported for VCC on TL866A/CS",
                        pin
                    ))
                })?;
                let off = (p.latch - 2) as usize * 2 + 10;
                pwr[off] &= p.mask;
            }
            _ => {}
        }
    }
    usb.msg_send(&pwr)?;
    Ok(())
}

/// Run the logic test and return a flat buffer of raw pin readings.
///
/// Result layout: `vector_count × pin_count` bytes, each 0 = low, non-zero = high.
fn do_ic_test(usb: &UsbDevice, device: &Device) -> Result<Vec<u8>> {
    let pin_count = device.package_details.pin_count as usize;
    let vector_count = device.vector_count;
    let vectors = device.vectors.as_deref().unwrap_or(&[]);

    if vector_count == 0 {
        return Err(MiniproError::Protocol(
            "Test vectors are not defined for this device type".into(),
        ));
    }

    let mut result = vec![0u8; pin_count * vector_count];

    pwr_init(usb, vectors, pin_count)?;

    let mut dir = [0u8; 47];
    let mut out = [0u8; 47];
    let mut msg = [0u8; 47];

    // All pins start as inputs (dir = 1) with output value 0
    dir[0] = CMD_SET_DIR;
    out[0] = CMD_SET_OUT;
    dir[7..].fill(0x01);
    out[7..].fill(0x00);

    let mut vout = result.as_mut_slice();

    for v in 0..vector_count {
        // Three state-machine steps per vector
        for sm in 0..3 {
            for pin in 0..pin_count {
                let zif_idx = ZIF_TABLE[pin_map(pin, pin_count)] as usize + 7;
                let value = vectors[v * pin_count + pin];

                if sm == 0 {
                    // State 1: set all pins according to test vector
                    match value {
                        LOGIC_0 => {
                            dir[zif_idx] = 0x00;
                            out[zif_idx] = 0x00;
                        }
                        LOGIC_1 => {
                            dir[zif_idx] = 0x00;
                            out[zif_idx] = 0x01;
                        }
                        LOGIC_C => {
                            dir[zif_idx] = 0x00;
                            // Clock pin retains previous value in first state
                        }
                        _ => {
                            dir[zif_idx] = 0x01; // input
                            out[zif_idx] = 0x00;
                        }
                    }
                    // Set pin state then direction (matches upstream C)
                    usb.msg_send(&out)?;
                    usb.msg_send(&dir)?;
                } else if value == LOGIC_C {
                    // States 2 & 3: clock lines toggle 1 then 0
                    out[zif_idx] = if sm == 1 { 0x01 } else { 0x00 };
                    usb.msg_send(&out)?;
                }
            }
        }

        // Read back all pin states
        msg[0] = CMD_READ_ZIF_PINS;
        usb.msg_send(&msg)?;
        let resp = usb.msg_recv(64)?;

        for (pin, slot) in vout.iter_mut().enumerate().take(pin_count) {
            let phys = pin_map(pin, pin_count);
            *slot = resp.get(phys + 7).copied().unwrap_or(0);
        }
        vout = &mut vout[pin_count..];
    }

    Ok(result)
}

/// Run the full logic-IC test, print a pass/fail table, and report errors.
pub fn tl866a_logic_ic_test(
    usb: &UsbDevice,
    device: &Device,
    out: &mut dyn std::io::Write,
) -> Result<()> {
    let pin_count = device.package_details.pin_count as usize;
    if !pin_count.is_multiple_of(2) {
        return Err(MiniproError::Protocol("Invalid pin count!".into()));
    }

    let vectors = device.vectors.as_deref().unwrap_or(&[]);
    let vector_count = device.vector_count;

    let result = do_ic_test(usb, device)?;

    // Printable character for each logic state
    const PST: &[u8] = b"01LHCZXGV";
    let mut n = 0usize;

    writeln!(
        out,
        "      {}",
        (1..=pin_count)
            .map(|p| format!("{:<3}", p))
            .collect::<Vec<_>>()
            .join("")
    )
    .map_err(|e| MiniproError::Protocol(format!("write error: {}", e)))?;

    let mut errors = 0usize;
    for i in 0..vector_count {
        write!(out, "{:04}: ", i)
            .map_err(|e| MiniproError::Protocol(format!("write error: {}", e)))?;
        for _pin in 0..pin_count {
            let expected = vectors[n];
            let actual = result[n];
            let err = match expected {
                LOGIC_L => actual != 0,
                LOGIC_H | LOGIC_Z => actual == 0,
                _ => false,
            };
            if err {
                errors += 1;
            }
            write!(
                out,
                "{}{}{} ",
                if err { "\x1b[0;91m" } else { "\x1b[0m" },
                PST[expected as usize] as char,
                if err { "-" } else { " " }
            )
            .map_err(|e| MiniproError::Protocol(format!("write error: {}", e)))?;
            n += 1;
        }
        writeln!(out, "\x1b[0m")
            .map_err(|e| MiniproError::Protocol(format!("write error: {}", e)))?;
    }

    if errors > 0 {
        Err(MiniproError::Protocol(format!(
            "Logic test failed: {} errors encountered",
            errors
        )))
    } else {
        Ok(())
    }
}
