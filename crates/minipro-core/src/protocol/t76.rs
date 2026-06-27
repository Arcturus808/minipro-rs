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
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::{
    device::Device,
    error::{MiniproError, Result},
    usb::UsbDevice,
};

/// Minimum firmware version for the T76.
pub const MIN_FIRMWARE_T76: u32 = 0x111; // 0.1.17

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

// NAND / eMMC / parallel-NOR command bytes (T76-specific)
const CMD_BEGIN_TRANS_LOGIC: u8 = 0x02; // 64-byte NAND/logic FPGA-setup prelude
const CMD_NAND_PROGRAM: u8 = 0x1F; // per-block NAND/eMMC program (init + stream)
const CMD_NAND_BAD_BLOCK_CHECK: u8 = 0x3A;
const CMD_PIN_DETECTION: u8 = 0x3E;

// eMMC (protocol_id 0x31) command bytes
const _CMD_EMMC_IO_REG: u8 = 0x01; // FPGA eMMC register I/O
const CMD_EMMC_SEND_CMD: u8 = 0x27; // eMMC CMD tunnel
const EMMC_OP_SWITCH: u8 = 0x46; // CMD6 SWITCH (partition select)
const EMMC_OP_PROGRAM_SETUP: u8 = 0x50; // program setup
const _EMMC_OP_STATUS_POLL: u8 = 0x4D; // erase-status poll
const EMMC_PART_USER: u32 = 0x02B3_0700;
const _EMMC_PART_BOOT1: u32 = 0x01B3_0100;
const _EMMC_PART_BOOT2: u32 = 0x01B3_0200;
const _EMMC_PART_RPMB: u32 = 0x01B3_0300;

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

pub struct T76Protocol {
    bitstream_uploaded: AtomicBool,
}

impl T76Protocol {
    pub fn new() -> Self {
        Self {
            bitstream_uploaded: AtomicBool::new(false),
        }
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
fn upload_bitstream_t76(usb: &UsbDevice, bitstream: &[u8], is_nand: bool) -> Result<()> {
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
    // For NAND, the vendor puts the size of the final (partial) block in
    // msg[2..3] so the FPGA finalizes the last config word. Without it the
    // NAND FPGA is mis-finalized (READID/read return 0xFF).
    let mut msg = [0u8; 8];
    msg[0] = CMD_WRITE_BITSTREAM;
    msg[1] = T76_END_BS;
    if is_nand {
        let mut last_block = bitstream.len() % payload_size;
        if last_block == 0 {
            last_block = payload_size;
        }
        put_le16(&mut msg[2..4], last_block as u32);
    }
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

// ── T76 adapter / pin-detection helpers ───────────────────────────────────────

/// Issue one 0x24 FPGA-register-I/O command.
///
/// The command word carries the response length in msg[2..3]; the device
/// returns that many bytes on EP81 which MUST be read back, otherwise the
/// next transfer desyncs (and a 0xf0 power-down left undrained wedges the
/// device until a USB replug).
fn t76_cmd_24(usb: &UsbDevice, cmd: &[u8; 8]) -> Result<()> {
    let recv_len = u16::from_le_bytes([cmd[2], cmd[3]]) as usize;
    usb.msg_send(cmd)?;
    if recv_len > 0 {
        let _ = usb.msg_recv(recv_len)?;
    }
    Ok(())
}

/// One-time socket-adapter power/init at session start.
///
/// XGPro detects and energizes the adapter via this 0x24 sequence before any
/// chip op; the BGA NAND adapter needs it (without it the NAND is never
/// selected -> READID reads 0xFF and the data read gets no EP82 data).
fn t76_adapter_init(usb: &UsbDevice) -> Result<()> {
    const PWR_DOWN: [u8; 8] = [0x24, 0xf0, 0x08, 0x00, 0x01, 0x00, 0x00, 0x00];
    const READ_ID: [u8; 8] = [0x24, 0xe4, 0x30, 0x00, 0x11, 0x01, 0x08, 0x00];
    const PWR_UP: [u8; 8] = [0x24, 0xf1, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    t76_cmd_24(usb, &PWR_DOWN)?;
    t76_cmd_24(usb, &READ_ID)?;
    t76_cmd_24(usb, &PWR_UP)?;

    // Pin detection (0x3e, 16-byte T76 form), run twice like XGPro right
    // after adapter power-up and before the bitstream. On the T76 this
    // configures the socket pin drivers for the selected package.
    for _ in 0..2 {
        let mut pd = [0u8; 16];
        pd[0] = CMD_PIN_DETECTION;
        usb.msg_send(&pd)?;
        let _ = usb.msg_recv(32)?;
    }
    Ok(())
}

/// eMMC socket-adapter power/init at session start.
///
/// Byte-exact from XGPro's eMMC READ capture: 0x24 f0 power-down (recv 8),
/// 0x24 e0 init (12 bytes, recv 0x28), 0x24 f1 power-up, then ONE 0x3e
/// pin-detect (recv 0x20). This enables the EXT_CSD (0x08) read to return data.
fn t76_emmc_adapter_init(usb: &UsbDevice) -> Result<()> {
    const PWR_DOWN: [u8; 8] = [0x24, 0xf0, 0x08, 0x00, 0x01, 0x00, 0x00, 0x00];
    const E0_INIT: [u8; 12] = [
        0x24, 0xe0, 0x28, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    const PWR_UP: [u8; 8] = [0x24, 0xf1, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];

    t76_cmd_24(usb, &PWR_DOWN)?;
    usb.msg_send(&E0_INIT)?;
    usb.msg_recv(0x28)?;
    t76_cmd_24(usb, &PWR_UP)?;

    let mut pd = [0u8; 16];
    pd[0] = CMD_PIN_DETECTION;
    usb.msg_send(&pd)?;
    let _ = usb.msg_recv(32)?;
    Ok(())
}

/// Send one 0x27 eMMC command tunnel packet.
///
/// Form A: simple command, no payload. send 8 / recv 8.
///   msg[0]=0x27, msg[1]=op, msg[2..3]=0, msg[4..7]=ARG.
///   resp[1]=error (0=OK), resp[4..7]=R1/R1b.
fn t76_emmc_cmd27(usb: &UsbDevice, op: u8, arg: u32) -> Result<()> {
    let mut msg = [0u8; 8];
    msg[0] = CMD_EMMC_SEND_CMD;
    msg[1] = op;
    put_le32(&mut msg[4..8], arg);
    usb.msg_send(&msg)?;
    let resp = usb.msg_recv(8)?;
    if resp.get(1).copied().unwrap_or(1) != 0 {
        return Err(MiniproError::Protocol(format!(
            "eMMC cmd27 op 0x{:02x} failed (status 0x{:02x})",
            op,
            resp.get(1).copied().unwrap_or(0xff)
        )));
    }
    Ok(())
}

/// eMMC per-region timing command (0x27 op 0x00).
///
/// Two fixed variants from the capture: PRE form sent before the read/program
/// init, POST form sent after the data. Byte [9] is the eMMC bus-width code
/// (0=1-bit, 1=4-bit, 2=8-bit), derived from variant>>8.
fn t76_emmc_timing(usb: &UsbDevice, device: &Device, post: bool) -> Result<()> {
    let mut timing = if post {
        [
            0x27, 0x00, 0xff, 0x00, 0x3b, 0x2c, 0x10, 0x0b, 0x00, 0x02, 0xb7, 0x03, 0x00, 0x01,
            0xb9, 0x03,
        ]
    } else {
        [
            0x27, 0x00, 0xff, 0x00, 0x3b, 0x0e, 0x05, 0x02, 0x00, 0x02, 0xb7, 0x03, 0x00, 0x12,
            0xb9, 0x03,
        ]
    };
    let width = match (device.variant >> 8) as u8 {
        0x51 => 0, // 1-bit
        0x54 => 1, // 4-bit
        _ => 2,    // 8-bit (0x53 or default)
    };
    timing[9] = width;
    usb.msg_send(&timing)
}

/// Build the 40-byte 0x0D (read) / 0x1F (program) region init.
///
/// The firmware then streams (read) / accepts (program) `blocks` × 64 KiB
/// on EP82 / EP05.
fn t76_emmc_io_init(init: &mut [u8; 40], opcode: u8, lba: u32, blocks: u32) {
    init.fill(0);
    init[0] = opcode;
    init[1] = 0x01;
    put_le32(&mut init[4..8], lba);
    put_le32(&mut init[8..12], 0x200);
    put_le32(&mut init[12..16], 0x20);
    put_le32(&mut init[16..20], blocks);
    put_le32(&mut init[20..24], 0x80);
    put_le32(&mut init[24..28], 0x20);
    put_le32(&mut init[28..32], 0x04);
    put_le32(&mut init[32..36], 0x01);
}

/// eMMC session bring-up: drain ID queries, then switch to USER partition.
///
/// Reconstructed from XGPro's eMMC READ capture (pcaps/xgpro-3.pcapng):
/// after BEGIN_TRANS the vendor sends three short ID queries whose
/// responses must be drained from EP81, then a CMD6 SWITCH to the
/// target partition.  Skipping the queries desyncs the USB stream.
fn t76_emmc_bring_up(usb: &UsbDevice) -> Result<()> {
    // Drain ID query responses: device-ID/CID (0x21, 32 bytes),
    // READID (0x05, 32 bytes), user-id/status (0x06, 24 bytes).
    const QUERIES: [(u8, usize); 3] = [(0x21, 32), (0x05, 32), (0x06, 24)];
    for &(opcode, resp_len) in &QUERIES {
        let mut cmd = [0u8; 8];
        cmd[0] = opcode;
        usb.msg_send(&cmd)?;
        let _ = usb.msg_recv(resp_len)?;
    }

    // Switch to USER partition (CMD6 SWITCH, default).
    t76_emmc_cmd27(usb, EMMC_OP_SWITCH, EMMC_PART_USER)?;

    Ok(())
}

// ── Protocol implementation ───────────────────────────────────────────────────

impl Protocol for T76Protocol {
    fn begin_transaction(&self, usb: &UsbDevice, device: &Device, icsp: bool) -> Result<()> {
        let is_nand = device.protocol_id == 0x2d;
        let is_emmc = device.protocol_id == 0x31;

        // 1. NAND / eMMC: energize/init the socket adapter before the first
        //    bitstream.  Skipped on subsequent calls — the adapter stays
        //    powered and the FPGA retains its bitstream across end/begin
        //    cycles (matching the C minipro `bitstream_uploaded` flag).
        if !self.bitstream_uploaded.load(Ordering::Relaxed) {
            if is_nand {
                t76_adapter_init(usb)?;
            }
            if is_emmc {
                t76_emmc_adapter_init(usb)?;
            }

            // 2. Upload FPGA algorithm bitstream.
            if let Some(ref algo) = device.algorithm {
                if !algo.bitstream.is_empty() {
                    eprintln!("Using T76 {} algorithm..", algo.name);
                    upload_bitstream_t76(usb, &algo.bitstream, is_nand)?;
                }
            }
            self.bitstream_uploaded.store(true, Ordering::Relaxed);
        }

        // 3. Send begin_transaction (custom bit-bang deferred to Phase 4).
        if !device.flags.custom_protocol {
            // T76 uses a 128-byte BEGIN_TRANS (vs 64-byte for T56).
            // Bytes 0x00..0x3f are the standard chip parameters.
            // Bytes 0x40..0x7f are a chip-class-specific FPGA-setup extension.
            let mut msg = [0u8; 128];
            msg[..64].copy_from_slice(&build_begin_msg(device, icsp));

            // T76 extras: I2C address and algorithm number
            if device.flags.can_adjust_address {
                msg[24] = device.i2c_address;
            }
            // msg[63] = high byte of variant = algorithm number
            msg[63] = (device.variant >> 8) as u8;

            let mut msglen = 64;

            // SPI 25-series NOR needs a geometry block in the extension area.
            // The 8-pin / 16-pin split is keyed off the algorithm/package nibble
            // in `variant >> 8` (0x11 = 8-pin, 0x21 = 16-pin).
            let is_spi_nor = device.protocol_id == 0x03 || device.protocol_id == 0x0f;
            if is_spi_nor {
                let algo = (device.variant >> 8) as u8;
                if algo == 0x21 {
                    // 16-pin (e.g. MX25L12845E)
                    put_le32(&mut msg[0x40..0x44], 0x0002_0000); // read-setup word 1
                    put_le32(&mut msg[0x50..0x54], 0x0200_0000); // read-setup word 2
                } else {
                    // 8-pin (default, e.g. ZB25VQ64A)
                    put_le32(&mut msg[0x40..0x44], 0x0800_0000); // read-setup word 1
                    put_le32(&mut msg[0x50..0x54], 0x0080_0000); // read-setup word 2
                }
                put_le32(&mut msg[0x60..0x64], 0x0f05_172f); // SPI clock config
                msg[0x65] = 0x03; // SPI clock sub-config
                msglen = 128;
            }

            // Parallel NOR (protocol_id 0x12/0x14): vendor packer sub_4b5a70
            // builds the 0x40..0x7f BEGIN extension. Only the x16 family
            // (package_details low byte 0x0b) with >=8 geometry is verified.
            // Verified READ + ERASE on S29GL512N; PROGRAM still non-functional.
            let is_parallel_nor = device.protocol_id == 0x12 || device.protocol_id == 0x14;
            if is_parallel_nor {
                let family = (device.package_details.raw & 0xff) as u8;
                let adapter = (device.variant & 0xf0) as u8;
                let geom = (device.variant & 0x0f) as u8;
                if family == 0x0b && geom >= 8 {
                    put_le32(&mut msg[0x40..0x44], 0x0100_0000);
                    put_le32(&mut msg[0x44..0x48], 0x0000_0040);
                    put_le32(&mut msg[0x50..0x54], 0x1000_0000);
                    put_le32(&mut msg[0x54..0x58], 0x0000_8000);
                    let b48 = match adapter {
                        0x10 => 0x0200,
                        0x20 => 0x1200,
                        0x30 => 0x0a00,
                        0x40 => 0x1000,
                        0x50 => 0x0800,
                        0x60 => 0x1800,
                        0x70 => 0x0400,
                        _ => 0x0800,
                    };
                    put_le32(&mut msg[0x48..0x4c], b48);
                    put_le32(&mut msg[0x60..0x64], 0x0f05_172f);
                    msg[0x65] = 0x03;
                    msglen = 128;
                }
            }

            // NAND (protocol_id 0x2d): the FPGA algorithm bitstream drives the
            // NAND command/address bus, so there is NO 0x40..0x5f geometry
            // block. The only required extension bytes are the clock/timing
            // dword and its cfg byte, plus the 128-byte length.
            if is_nand {
                // Per-block transfer size (data + spare) at msg[0x10].
                if device.pages_per_block > 0 {
                    put_le32(
                        &mut msg[16..20],
                        (device.write_buffer_size as u32) * device.pages_per_block,
                    );
                }
                // NAND flag bit 0x800 in raw_flags.
                put_le32(&mut msg[56..60], device.flags.raw | 0x800);
                put_le32(&mut msg[0x60..0x64], 0x0b09_272f); // NAND clock config
                msg[0x65] = 0x03;
                msg[0x0e] = 0x20;
                msg[0x14] = 0x00;
                msg[0x18] = 0x03;
                msg[0x1c] = 0x03;
                // Pin/family byte for parallel NAND.
                if (device.variant & 0x70) == 0 {
                    put_le32(&mut msg[0x28..0x2c], 0xe200_0000);
                }
                msg[0x30] = 0x40;
                msglen = 128;
            }

            usb.msg_send(&msg[..msglen])?;
        }

        // 4. NAND: send the 64-byte opcode-0x02 "logic begin" prelude immediately
        //    BEFORE the 0x03 BEGIN_TRANS. This programs the FPGA's NAND page/block
        //    geometry and bus clock; without it the FPGA never clocks the NAND.
        if is_nand {
            let mut pre = [0u8; 64];
            pre[0] = CMD_BEGIN_TRANS_LOGIC;

            let page_or_blocks = device.page_size;
            let ppb = device.pages_per_block;
            let wbuf = device.write_buffer_size;
            let mut real_page: u16 = 1;
            while (real_page as u32) * 2 <= (wbuf as u32) {
                real_page <<= 1;
            }
            let ps_code = if real_page < 0x800 {
                4
            } else if real_page == 0x800 {
                8
            } else if real_page == 0x1000 {
                4
            } else if real_page == 0x4000 {
                1
            } else {
                2
            };

            let big = (ppb * page_or_blocks) > 0x10000;
            let busw = if real_page >= 0x800 {
                if big {
                    3
                } else {
                    1
                }
            } else if big {
                2
            } else {
                0
            };
            let serial = (device.variant & 0x70) != 0;

            // Conservative low-speed clock entries from the vendor capture.
            let clock = if serial { 0x0f09_272f } else { 0x0b09_272f };

            put_le16(&mut pre[8..10], ppb);
            put_le16(&mut pre[10..12], page_or_blocks);
            put_le16(&mut pre[12..14], page_or_blocks);
            put_le16(&mut pre[14..16], ppb);
            put_le16(&mut pre[16..18], 1); // plane count
            put_le16(&mut pre[18..20], 1); // LUN count
            put_le32(&mut pre[20..24], busw);
            put_le32(&mut pre[24..28], ps_code);
            put_le32(&mut pre[28..32], 0);
            put_le32(&mut pre[32..36], 1); // adapter-mode byte at +2 = 1
            put_le32(&mut pre[36..40], clock);

            usb.msg_send(&pre)?;
        }

        // 5. eMMC: drain ID queries, then switch to the active partition
        //    (default USER).  The firmware sends response data on EP81 that
        //    the host must consume; skipping the queries desyncs the USB
        //    stream.  The partition must be selected before any
        //    read/write/erase.
        if is_emmc {
            t76_emmc_bring_up(usb)?;
        }

        Ok(())
    }

    fn end_transaction(&self, usb: &UsbDevice) -> Result<()> {
        let mut msg = [0u8; 8];
        msg[0] = CMD_END_TRANS;
        usb.msg_send(&msg)
    }

    fn read_block(&self, usb: &UsbDevice, device: &Device, ds: &mut DataSet) -> Result<()> {
        let size = ds.data.len();

        if ds.page_type == MP_CODE {
            // NAND read: one erase-block (data + spare) per command.
            if device.protocol_id == 0x2d {
                let mut msg = [0u8; 16];
                const NAND_READ_HDR: [u8; 12] = [
                    0x10, 0x00, 0x04, 0x00, // msg[4..7]
                    0x08, 0x00, 0x08, 0x00, // msg[8..b]
                    0x69, 0x01, 0x00, 0x00, // msg[c..f]
                ];
                let block_index = if size > 0 {
                    ds.address / (size as u32)
                } else {
                    0
                };
                msg[0] = CMD_READ_CODE;
                put_le16(&mut msg[2..4], block_index);
                msg[4..16].copy_from_slice(&NAND_READ_HDR);
                usb.msg_send(&msg)?;
                ds.data = usb.read_payload(size)?;
                return Ok(());
            }

            // eMMC read (protocol_id 0x31).
            if device.protocol_id == 0x31 {
                if ds.init {
                    t76_emmc_timing(usb, device, false)?; // PRE-read
                    let mut init = [0u8; 40];
                    // Address is in sectors (LBA); block_count is total 64 KiB blocks.
                    let lba = ds.address / 512;
                    let blocks = ds.total_blocks;
                    t76_emmc_io_init(&mut init, CMD_READ_CODE, lba, blocks);
                    usb.msg_send(&init)?;
                }
                ds.data = usb.read_payload(size)?;
                return Ok(());
            }

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

    fn write_block(&self, usb: &UsbDevice, device: &Device, ds: &DataSet) -> Result<()> {
        let size = ds.data.len();
        let mut msg = [0u8; 64];
        put_le16(&mut msg[2..4], size as u32);
        put_le32(&mut msg[4..8], ds.address);
        put_le32(&mut msg[12..16], size as u32);

        if ds.page_type == MP_CODE {
            // NAND program (protocol_id 0x2d).
            if device.protocol_id == 0x2d {
                let page_full = device.write_buffer_size;
                let ppb = device.pages_per_block;
                let block_index = if size > 0 {
                    ds.address / (size as u32)
                } else {
                    0
                };
                let mut imsg = [0u8; 16];
                imsg[0] = CMD_NAND_PROGRAM;
                put_le16(&mut imsg[2..4], page_full as u32);
                put_le32(&mut imsg[4..8], block_index);
                put_le32(&mut imsg[8..12], ppb);
                put_le32(&mut imsg[12..16], page_full as u32);
                usb.msg_send(&imsg)?;

                let mut pkt = vec![0u8; page_full as usize + 16];
                let page_count = (size as u32).div_ceil(page_full as u32).min(ppb);
                for p in 0..page_count {
                    pkt[..16].fill(0);
                    pkt[0] = CMD_NAND_PROGRAM;
                    let offset = (p as usize) * (page_full as usize);
                    let end = (offset + page_full as usize).min(size);
                    let n = end - offset;
                    pkt[16..16 + n].copy_from_slice(&ds.data[offset..end]);
                    if n < page_full as usize {
                        pkt[16 + n..16 + page_full as usize].fill(0xFF);
                    }
                    usb.write_payload(&pkt)?;
                }

                // Commit the block: a plain 0x39 REQUEST_STATUS waits for the
                // last page's program to finish and returns block status.
                let mut st = [0u8; 32];
                st[0] = CMD_REQUEST_STATUS;
                usb.msg_send(&st[..8])?;
                usb.msg_recv(32)?;
                return Ok(());
            }

            // eMMC program (protocol_id 0x31).
            if device.protocol_id == 0x31 {
                thread_local! {
                    static EMMC_BLK_IDX: Cell<u32> = const { Cell::new(0) };
                    static EMMC_BLK_TOTAL: Cell<u32> = const { Cell::new(0) };
                }
                if ds.init {
                    // Program setup: 0x27 op 0x50 (ARG 0x20000), once.
                    let mut op50 = [0u8; 8];
                    op50[0] = CMD_EMMC_SEND_CMD;
                    op50[1] = EMMC_OP_PROGRAM_SETUP;
                    put_le32(&mut op50[4..8], 0x0002_0000);
                    usb.msg_send(&op50)?;
                    let _ = usb.msg_recv(8)?;

                    t76_emmc_timing(usb, device, false)?; // PRE
                    let mut init = [0u8; 40];
                    // Address is in sectors (LBA); block_count is total 64 KiB blocks.
                    let lba = ds.address / 512;
                    let blocks = ds.total_blocks;
                    t76_emmc_io_init(&mut init, CMD_NAND_PROGRAM, lba, blocks);
                    usb.msg_send(&init)?;
                    EMMC_BLK_TOTAL.set(blocks);
                    EMMC_BLK_IDX.set(0);
                }

                usb.write_payload(&ds.data)?;

                let idx = EMMC_BLK_IDX.get() + 1;
                EMMC_BLK_IDX.set(idx);
                if idx >= EMMC_BLK_TOTAL.get() {
                    // Commit: 0x39 -> POST-timing -> 0x39.
                    let mut st = [0u8; 32];
                    st[0] = CMD_REQUEST_STATUS;
                    usb.msg_send(&st[..8])?;
                    usb.msg_recv(32)?;
                    t76_emmc_timing(usb, device, true)?; // POST
                    st.fill(0);
                    st[0] = CMD_REQUEST_STATUS;
                    usb.msg_send(&st[..8])?;
                    usb.msg_recv(32)?;
                }
                return Ok(());
            }

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

    fn erase(&self, usb: &UsbDevice, device: &Device, num_fuses: u8, is_pld: bool) -> Result<()> {
        // NAND erase: per-block with bad-block skip.
        if device.protocol_id == 0x2d {
            let block_size = (device.write_buffer_size as u32) * device.pages_per_block;
            if block_size == 0 {
                return Err(MiniproError::Protocol(
                    "NAND geometry missing (block size 0).".into(),
                ));
            }
            let block_count = device.code_memory_size / block_size;
            let mut bad = 0u32;

            for blk in 0..block_count {
                // 0x3A bad-block check: skip factory-marked bad blocks.
                let mut msg = [0u8; 64];
                msg[0] = CMD_NAND_BAD_BLOCK_CHECK;
                put_le16(&mut msg[2..4], blk);
                usb.msg_send(&msg[..8])?;
                let resp = usb.msg_recv(8)?;
                if resp.get(1).copied().unwrap_or(0) != 0 {
                    bad += 1;
                    continue;
                }

                // 0x0E erase this block.
                msg.fill(0);
                msg[0] = CMD_ERASE;
                put_le16(&mut msg[2..4], blk);
                usb.msg_send(&msg[..16])?;
                let resp = usb.msg_recv(8)?;
                if resp.get(1).copied().unwrap_or(0) != 0 {
                    bad += 1;
                }
            }

            if bad > 0 {
                eprintln!(
                    "({} bad block{} skipped)",
                    bad,
                    if bad == 1 { "" } else { "s" }
                );
            }
            return Ok(());
        }

        // eMMC erase (protocol_id 0x31).
        if device.protocol_id == 0x31 {
            const POLL: [u8; 8] = [0x27, 0x4d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00];
            let total = (device.code_memory_size as u64) / 512;
            if total == 0 {
                return Ok(());
            }
            for start in (0..total).step_by(0x20000) {
                let end = (start + 0x1ffff).min(total - 1);
                let mut cmd = [0u8; 16];
                cmd[0] = CMD_ERASE;
                put_le32(&mut cmd[4..8], start as u32);
                put_le32(&mut cmd[8..12], end as u32);
                usb.msg_send(&cmd)?;
                let _ = usb.msg_recv(8)?;

                // Poll until the erase completes (resp[5] back to 0x09).
                let mut done = false;
                for _ in 0..2_000_000 {
                    let p = POLL;
                    usb.msg_send(&p)?;
                    let rsp = usb.msg_recv(8)?;
                    if rsp.get(5).copied().unwrap_or(0x0e) != 0x0e {
                        done = true;
                        break;
                    }
                }
                if !done {
                    return Err(MiniproError::Protocol("eMMC erase timed out.".into()));
                }
            }
            return Ok(());
        }

        // Standard T76 erase.
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
        // TODO: For NAND and eMMC the vendor repacks msg[1..7] with the
        // chip-parameter header; a zeroed msg deselects the NAND. The
        // operations layer currently skips this call for NAND; when we add
        // Device to this trait method we should mirror the vendor behaviour.
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

    fn pin_test(
        &self,
        usb: &UsbDevice,
        device: &Device,
        pin_map: &crate::database::PinMap,
    ) -> Result<()> {
        // T76 uses the same ZIF-socket pin-test hardware as the TL866II+.
        super::tl866iiplus::pin_test_tl866(usb, device, pin_map)
    }

    fn firmware_update(&self, usb: &UsbDevice, firmware: &[u8]) -> Result<()> {
        firmware_update_t76(usb, firmware)
    }

    fn logic_ic_test(
        &self,
        usb: &UsbDevice,
        device: &Device,
        out: &mut dyn std::io::Write,
    ) -> Result<()> {
        // T76 uses the same test-vector command (0x28) as the TL866II+.
        // A full implementation would reload the FPGA bitstream between the
        // pull-up and pull-down passes; here we reuse the TL866II+ two-pass
        // logic without FPGA switching (known limitation for T76).
        logic_ic_test_tl866(usb, device, out)
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
