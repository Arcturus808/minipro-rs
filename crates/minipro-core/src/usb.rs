//! USB transport layer.
//!
//! Abstracts over `nusb` bulk transfers so the rest of the crate never
//! touches raw USB primitives.  All transfers are made synchronous via
//! `pollster::block_on`.
//!
//! Endpoint map (from usb_nix.c):
//!
//! | Endpoint | Direction | Purpose                         |
//! |----------|-----------|---------------------------------|
//! | 0x01     | OUT       | Send command packet (64 B)      |
//! | 0x81     | IN        | Receive response packet (64 B)  |
//! | 0x02     | OUT       | Write payload, even 64-B blocks |
//! | 0x03     | OUT       | Write payload, odd 64-B blocks  |
//! | 0x82     | IN        | Read payload, even 64-B blocks  |
//! | 0x83     | IN        | Read payload, odd 64-B blocks   |
//! | 0x05     | OUT       | T76 write payload (single EP)   |
//! | 0x82     | IN        | T76 read payload                |

use log::trace;
use nusb::transfer::RequestBuffer;
use nusb::{DeviceInfo, Interface};

use crate::{
    device::ProgrammerModel,
    error::{MiniproError, Result},
};

// ── USB VID/PID constants ────────────────────────────────────────────────────

const TL866_VID: u16 = 0x04d8;
const TL866_PID: u16 = 0xe11c;
const TL866II_VID: u16 = 0xa466;
const TL866II_PID: u16 = 0x0a53;
const T76_VID: u16 = 0xa466;
const T76_PID: u16 = 0x1a86;

#[allow(dead_code)]
const USB_TIMEOUT_MS: u32 = 5_000;
#[allow(dead_code)]
const USB_READ_TIMEOUT_MS: u32 = 360_000;

const CMD_EP_OUT: u8 = 0x01;
const CMD_EP_IN: u8 = 0x81;
const DATA_EP2_OUT: u8 = 0x02;
const DATA_EP3_OUT: u8 = 0x03;
const DATA_EP2_IN: u8 = 0x82;
const DATA_EP3_IN: u8 = 0x83;
const T76_DATA_EP_OUT: u8 = 0x05;

// ── Device open ──────────────────────────────────────────────────────────────

/// Identifies which USB VID/PID a `DeviceInfo` matches.
fn classify(info: &DeviceInfo) -> Option<ProgrammerModel> {
    let (vid, pid) = (info.vendor_id(), info.product_id());
    match (vid, pid) {
        (T76_VID, T76_PID) => Some(ProgrammerModel::T76),
        (TL866II_VID, TL866II_PID) => {
            // T56 and T48 share the same VID/PID as TL866II+; the model is
            // determined after opening by reading the system-info response.
            Some(ProgrammerModel::Tl866iiPlus)
        }
        (TL866_VID, TL866_PID) => Some(ProgrammerModel::Tl866a),
        _ => None,
    }
}

/// A claimed USB interface ready for transfers.
#[derive(Default)]
pub struct UsbDevice {
    interface: Option<Interface>,
    /// The USB PID, used to select T76-specific payload endpoints.
    pid: u16,
}

impl UsbDevice {
    /// Return the USB product ID of this device.
    pub fn pid(&self) -> u16 {
        self.pid
    }

    fn iface(&self) -> Result<&Interface> {
        self.interface
            .as_ref()
            .ok_or_else(|| MiniproError::Protocol("USB device not open".into()))
    }
}

/// Poll USB until the programmer with the given PID disappears.
/// Returns `Ok(())` when disconnected or `Err` on timeout.
pub fn wait_for_disconnect(pid: u16, timeout_ms: u64) -> Result<()> {
    let start = std::time::Instant::now();
    while start.elapsed().as_millis() < timeout_ms as u128 {
        let devices = nusb::list_devices().map_err(MiniproError::Usb)?;
        let found = devices
            .filter(|d| classify(d).is_some() && d.product_id() == pid)
            .count();
        if found == 0 {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Err(MiniproError::Protocol(
        "Timed out waiting for programmer to disconnect".into(),
    ))
}

/// Poll USB until the programmer with the given PID reappears.
/// Returns `Ok(())` when reconnected or `Err` on timeout.
pub fn wait_for_reconnect(pid: u16, timeout_ms: u64) -> Result<()> {
    let start = std::time::Instant::now();
    while start.elapsed().as_millis() < timeout_ms as u128 {
        let devices = nusb::list_devices().map_err(MiniproError::Usb)?;
        let found = devices
            .filter(|d| classify(d).is_some() && d.product_id() == pid)
            .count();
        if found > 0 {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Err(MiniproError::Protocol(
        "Timed out waiting for programmer to reconnect".into(),
    ))
}

/// Open the first connected programmer.
///
/// Selects among TL866 (VID 0x04D8), TL866II+/T48/T56 (VID 0xA466 PID 0x0A53),
/// and T76 (VID 0xA466 PID 0x1A86).
pub fn open_programmer() -> Result<(UsbDevice, ProgrammerModel)> {
    let devices = nusb::list_devices().map_err(MiniproError::Usb)?;

    let mut found: Vec<(DeviceInfo, ProgrammerModel)> = devices
        .filter_map(|info| classify(&info).map(|model| (info, model)))
        .collect();

    match found.len() {
        0 => Err(MiniproError::NoProgrammerFound),
        1 => {
            let (info, model) = found.remove(0);
            let pid = info.product_id();
            let device = info.open().map_err(|e| {
                MiniproError::Protocol(format!(
                    "USB device found but cannot open it. \
                     Try unplugging the programmer, wait a few seconds, and plug it back in. \
                     If the problem persists, the WinUSB driver may need reinstallation \
                     with Zadig (https://zadig.akeo.ie/). (nusb error: {})",
                    e
                ))
            })?;
            let interface = device.claim_interface(0).map_err(|e| {
                MiniproError::Protocol(format!(
                    "USB device opened but cannot claim interface 0. \
                     Try unplugging the programmer, wait a few seconds, and plug it back in. \
                     If the problem persists, use Zadig to reinstall the WinUSB driver. (nusb error: {})",
                    e
                ))
            })?;
            Ok((
                UsbDevice {
                    interface: Some(interface),
                    pid,
                },
                model,
            ))
        }
        _ => Err(MiniproError::MultipleProgrammersFound),
    }
}

// ── Transfer primitives ──────────────────────────────────────────────────────

impl UsbDevice {
    /// Send a command packet.  `buf` must be exactly 64 bytes; it is
    /// zero-padded to that length if shorter.
    pub fn msg_send(&self, buf: &[u8]) -> Result<()> {
        let mut data = vec![0u8; 64];
        let len = buf.len().min(64);
        data[..len].copy_from_slice(&buf[..len]);
        trace!("msg_send: cmd=0x{:02x} buf={:02x?}", data[0], &data[..len]);
        let completion = pollster::block_on(self.iface()?.bulk_out(CMD_EP_OUT, data));
        completion
            .status
            .map_err(|e| MiniproError::Protocol(e.to_string()))?;
        Ok(())
    }

    /// Send an arbitrary-length packet to the command endpoint (EP 0x01).
    ///
    /// Unlike [`msg_send`], this does **not** pad or truncate the buffer.
    /// Used for T56/T76 FPGA bitstream upload where packets are larger than 64 B.
    pub fn msg_send_large(&self, buf: &[u8]) -> Result<()> {
        self.bulk_out_raw(CMD_EP_OUT, buf.to_vec())
    }

    /// Receive a response packet (64 bytes by default; pass a larger size for
    /// commands that return more).
    pub fn msg_recv(&self, size: usize) -> Result<Vec<u8>> {
        trace!("msg_recv: waiting for {size} bytes on EP 0x81");
        let completion =
            pollster::block_on(self.iface()?.bulk_in(CMD_EP_IN, RequestBuffer::new(size)));
        completion
            .status
            .map_err(|e| MiniproError::Protocol(e.to_string()))?;
        trace!(
            "msg_recv: got {} bytes: {:02x?}",
            completion.data.len(),
            &completion.data[..completion.data.len().min(16)]
        );
        Ok(completion.data)
    }

    /// Write a payload to the chip.
    ///
    /// For payloads > 64 bytes the data is split across EP2/EP3 in the
    /// interleaved pattern expected by the TL866II+ firmware.
    pub fn write_payload(&self, data: &[u8]) -> Result<()> {
        self.write_payload_limit(data, 64)
    }

    pub fn write_payload_limit(&self, data: &[u8], limit: usize) -> Result<()> {
        // T76 uses a single endpoint for writes
        if self.pid == T76_PID {
            return self.bulk_out_raw(T76_DATA_EP_OUT, data.to_vec());
        }

        if data.len() <= limit {
            return self.bulk_out_raw(DATA_EP2_OUT, data.to_vec());
        }

        // Split into two halves for EP2 / EP3 (see usb_nix.c write_payload2)
        let (ep2_len, ep3_len) = split_lengths(data.len());
        let ep2_data = data[..ep2_len].to_vec();
        let ep3_data = data[ep2_len..ep2_len + ep3_len].to_vec();

        // Submit both transfers and wait
        self.bulk_out_raw(DATA_EP2_OUT, ep2_data)?;
        self.bulk_out_raw(DATA_EP3_OUT, ep3_data)?;
        Ok(())
    }

    /// Read a payload from the chip.
    pub fn read_payload(&self, length: usize) -> Result<Vec<u8>> {
        self.read_payload_limit(length, 64)
    }

    pub fn read_payload_limit(&self, length: usize, limit: usize) -> Result<Vec<u8>> {
        trace!("read_payload_limit: length={length} limit={limit}");
        // T76 uses EP 0x82 only for reads
        if self.pid == T76_PID {
            trace!("  -> T76 path: bulk_in(EP 0x82, {length})");
            let c = pollster::block_on(
                self.iface()?
                    .bulk_in(DATA_EP2_IN, RequestBuffer::new(length)),
            );
            trace!(
                "  <- EP 0x82 complete: {} bytes, status={:?}",
                c.data.len(),
                c.status
            );
            c.status
                .map_err(|e| MiniproError::Protocol(e.to_string()))?;
            return Ok(c.data);
        }

        // Small reads: single EP2 transfer
        if length < 64 {
            trace!("  -> small path: bulk_in(EP 0x82, 64)");
            let c = pollster::block_on(self.iface()?.bulk_in(DATA_EP2_IN, RequestBuffer::new(64)));
            trace!(
                "  <- EP 0x82 complete: {} bytes, status={:?}",
                c.data.len(),
                c.status
            );
            c.status
                .map_err(|e| MiniproError::Protocol(e.to_string()))?;
            return Ok(c.data[..length].to_vec());
        }

        if length == 64 || length <= limit || limit == 0 {
            trace!("  -> single-EP2 path: bulk_in(EP 0x82, {length})");
            let c = pollster::block_on(
                self.iface()?
                    .bulk_in(DATA_EP2_IN, RequestBuffer::new(length)),
            );
            trace!(
                "  <- EP 0x82 complete: {} bytes, status={:?}",
                c.data.len(),
                c.status
            );
            c.status
                .map_err(|e| MiniproError::Protocol(e.to_string()))?;
            return Ok(c.data);
        }

        // Large reads: interleaved EP2 + EP3, then de-interleave
        let half = length / 2;
        trace!("  -> dual-EP path: bulk_in(EP 0x82, {half})");
        let c2 = pollster::block_on(self.iface()?.bulk_in(DATA_EP2_IN, RequestBuffer::new(half)));
        trace!(
            "  <- EP 0x82 complete: {} bytes, status={:?}",
            c2.data.len(),
            c2.status
        );
        c2.status
            .map_err(|e| MiniproError::Protocol(e.to_string()))?;
        trace!("  -> dual-EP path: bulk_in(EP 0x83, {half})");
        let c3 = pollster::block_on(self.iface()?.bulk_in(DATA_EP3_IN, RequestBuffer::new(half)));
        trace!(
            "  <- EP 0x83 complete: {} bytes, status={:?}",
            c3.data.len(),
            c3.status
        );
        c3.status
            .map_err(|e| MiniproError::Protocol(e.to_string()))?;

        // De-interleave 64-byte blocks: even blocks from EP2, odd from EP3
        Ok(deinterleave(&c2.data, &c3.data, length))
    }

    fn bulk_out_raw(&self, ep: u8, data: Vec<u8>) -> Result<()> {
        trace!(
            "bulk_out_raw: ep=0x{:02x} len={} data={:02x?}",
            ep,
            data.len(),
            &data[..data.len().min(16)]
        );
        let c = pollster::block_on(self.iface()?.bulk_out(ep, data));
        c.status
            .map_err(|e| MiniproError::Protocol(e.to_string()))?;
        Ok(())
    }
}

// ── Payload split/deinterleave helpers ───────────────────────────────────────

/// Compute (ep2_len, ep3_len) for a split payload write (mirrors usb_nix.c).
fn split_lengths(total: usize) -> (usize, usize) {
    let j = total % 128;
    if j != 0 {
        let k = (total - j) / 2;
        if j > 64 {
            (k + 64, j + k - 64)
        } else {
            (k, j + k)
        }
    } else {
        let half = total / 2;
        (half, half)
    }
}

/// Re-interleave data from EP2 and EP3 into contiguous buffer.
/// Even-numbered 64-byte blocks come from `ep2`, odd from `ep3`.
fn deinterleave(ep2: &[u8], ep3: &[u8], total: usize) -> Vec<u8> {
    let blocks = total / 64;
    let mut out = vec![0u8; total];
    let mut ep2_off = 0usize;
    let mut ep3_off = 0usize;

    for i in 0..blocks {
        let dst = i * 64;
        if i % 2 == 0 {
            out[dst..dst + 64].copy_from_slice(&ep2[ep2_off..ep2_off + 64]);
            ep2_off += 64;
        } else {
            out[dst..dst + 64].copy_from_slice(&ep3[ep3_off..ep3_off + 64]);
            ep3_off += 64;
        }
    }
    out
}
