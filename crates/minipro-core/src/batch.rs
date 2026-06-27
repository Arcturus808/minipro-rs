//! Batch programming — program multiple identical chips with the same firmware.
//!
//! The batch loop handles: erase → write → verify for each chip, with callbacks
//! for progress reporting, "ready for next chip" prompting, and buffer patching
//! (for serial number injection).

use std::path::Path;

use crate::{
    error::{MiniproError, Result},
    handle::MiniproHandle,
    operations::{erase_chip, verify_chip_bytes, SizeMismatch},
};

// ── Serial number injection ─────────────────────────────────────────────────

/// Serial number byte format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SerialFormat {
    /// Raw binary in selected endianness.
    Bin,
    /// Zero-padded ASCII decimal string (e.g., "00001\0").
    Ascii,
    /// Binary-coded decimal (each digit as a 4-bit nibble).
    Bcd,
}

impl SerialFormat {
    /// Parse from a string.
    pub fn parse(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "bin" => Ok(Self::Bin),
            "ascii" => Ok(Self::Ascii),
            "bcd" => Ok(Self::Bcd),
            _ => Err(MiniproError::FileFormat(format!(
                "unknown serial format '{s}'; expected bin, ascii, or bcd"
            ))),
        }
    }
}

/// Byte order for binary serial format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SerialEndian {
    Little,
    Big,
}

impl SerialEndian {
    /// Parse from a string.
    pub fn parse(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "little" | "le" => Ok(Self::Little),
            "big" | "be" => Ok(Self::Big),
            _ => Err(MiniproError::FileFormat(format!(
                "unknown endian '{s}'; expected little or big"
            ))),
        }
    }
}

/// Optional checksum appended after the serial bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SerialChecksum {
    /// No checksum.
    None,
    /// XOR of all serial bytes.
    Xor,
    /// CRC-8 (polynomial 0x07, init 0x00).
    Crc8,
}

impl SerialChecksum {
    /// Parse from a string.
    pub fn parse(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "none" => Ok(Self::None),
            "xor" => Ok(Self::Xor),
            "crc8" => Ok(Self::Crc8),
            _ => Err(MiniproError::FileFormat(format!(
                "unknown checksum '{s}'; expected none, xor, or crc8"
            ))),
        }
    }
}

/// Configuration for serial number injection during batch programming.
#[derive(Debug, Clone)]
pub struct SerialConfig {
    /// Starting serial value.
    pub start: u64,
    /// Target address in the chip's memory (byte offset).
    pub address: usize,
    /// Byte width: 1, 2, 4, or 8.
    pub width: usize,
    /// Format: binary, ASCII, or BCD.
    pub format: SerialFormat,
    /// Byte order (binary format only).
    pub endian: SerialEndian,
    /// Increment per chip.
    pub step: u64,
    /// Optional checksum appended after the serial bytes.
    pub checksum: SerialChecksum,
}

impl SerialConfig {
    /// Returns the serial value for the given 1-based chip number.
    /// Chip 1 gets `start`, chip 2 gets `start + step`, etc.
    pub fn value_for_chip(&self, chip_number: usize) -> u64 {
        self.start + (chip_number as u64 - 1) * self.step
    }

    /// Returns the number of bytes the serial occupies (including checksum).
    pub fn total_len(&self) -> usize {
        let serial_len = match self.format {
            SerialFormat::Bin => self.width,
            SerialFormat::Ascii => self.width + 1, // +1 for null terminator
            SerialFormat::Bcd => self.width.div_ceil(2), // 2 digits per byte
        };
        let checksum_len = match self.checksum {
            SerialChecksum::None => 0,
            _ => 1,
        };
        serial_len + checksum_len
    }

    /// Validate that the serial fits within the buffer at the configured address.
    pub fn validate(&self, buf_len: usize) -> Result<()> {
        if self.width == 0 || self.width > 8 {
            return Err(MiniproError::FileFormat(format!(
                "serial width must be 1-8, got {}",
                self.width
            )));
        }
        let end = self.address + self.total_len();
        if end > buf_len {
            return Err(MiniproError::FileFormat(format!(
                "serial at address 0x{:X} needs {} bytes, but buffer is only {} bytes (needs {} more)",
                self.address,
                self.total_len(),
                buf_len,
                end - buf_len
            )));
        }
        Ok(())
    }
}

/// Patch a buffer with the serial number for the given chip.
///
/// Writes the serial value at `config.address` in the format specified.
/// If a checksum is configured, it is appended immediately after the serial bytes.
pub fn patch_serial(buf: &mut [u8], config: &SerialConfig, chip_number: usize) -> Result<()> {
    config.validate(buf.len())?;
    let value = config.value_for_chip(chip_number);
    let addr = config.address;

    // ── Write serial bytes ──────────────────────────────────────────────────
    let serial_len = match config.format {
        SerialFormat::Bin => {
            let bytes = match config.width {
                1 => vec![value as u8],
                2 => match config.endian {
                    SerialEndian::Little => (value as u16).to_le_bytes().to_vec(),
                    SerialEndian::Big => (value as u16).to_be_bytes().to_vec(),
                },
                4 => match config.endian {
                    SerialEndian::Little => (value as u32).to_le_bytes().to_vec(),
                    SerialEndian::Big => (value as u32).to_be_bytes().to_vec(),
                },
                8 => match config.endian {
                    SerialEndian::Little => value.to_le_bytes().to_vec(),
                    SerialEndian::Big => value.to_be_bytes().to_vec(),
                },
                _ => unreachable!("validated above"),
            };
            buf[addr..addr + bytes.len()].copy_from_slice(&bytes);
            bytes.len()
        }
        SerialFormat::Ascii => {
            // Format as zero-padded decimal string with `width` digits + null terminator
            let s = format!("{:0>width$}", value, width = config.width);
            let ascii_bytes = s.as_bytes();
            buf[addr..addr + ascii_bytes.len()].copy_from_slice(ascii_bytes);
            buf[addr + ascii_bytes.len()] = 0; // null terminator
            ascii_bytes.len() + 1
        }
        SerialFormat::Bcd => {
            // Each decimal digit becomes a 4-bit nibble, packed 2 per byte.
            let s = format!("{:0>width$}", value, width = config.width);
            let nibbles: Vec<u8> = s.bytes().map(|b| b - b'0').collect();
            let byte_len = nibbles.len().div_ceil(2);
            for i in 0..byte_len {
                let hi = nibbles[i * 2];
                let lo = if i * 2 + 1 < nibbles.len() {
                    nibbles[i * 2 + 1]
                } else {
                    0xF // pad last nibble with 0xF if odd number of digits
                };
                buf[addr + i] = (hi << 4) | lo;
            }
            byte_len
        }
    };

    // ── Write checksum (if enabled) ─────────────────────────────────────────
    match config.checksum {
        SerialChecksum::None => {}
        SerialChecksum::Xor => {
            let cksum = buf[addr..addr + serial_len]
                .iter()
                .fold(0u8, |acc, &b| acc ^ b);
            buf[addr + serial_len] = cksum;
        }
        SerialChecksum::Crc8 => {
            let cksum = crc8(&buf[addr..addr + serial_len]);
            buf[addr + serial_len] = cksum;
        }
    }

    Ok(())
}

/// Compute CRC-8 with polynomial 0x07, init 0x00 (standard CRC-8).
fn crc8(data: &[u8]) -> u8 {
    let mut crc: u8 = 0;
    for &byte in data {
        crc ^= byte;
        for _ in 0..8 {
            if crc & 0x80 != 0 {
                crc = (crc << 1) ^ 0x07;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

// ── Batch types ─────────────────────────────────────────────────────────────

/// Result of a single chip within a batch run.
#[derive(Debug, Clone)]
pub struct BatchChipResult {
    /// 1-based chip index within the batch.
    pub chip_number: usize,
    /// Whether the write + verify succeeded.
    pub success: bool,
    /// Error message if the chip failed.
    pub error: Option<String>,
}

/// Summary of a completed batch run.
#[derive(Debug, Clone)]
pub struct BatchSummary {
    /// Total number of chips attempted.
    pub total: usize,
    /// Number of successful write + verify cycles.
    pub passed: usize,
    /// Number of failures.
    pub failed: usize,
    /// Per-chip results.
    pub results: Vec<BatchChipResult>,
    /// True if the user aborted the batch before completing.
    pub aborted: bool,
}

impl BatchSummary {
    /// Returns true if all chips passed (no failures, not aborted).
    pub fn all_passed(&self) -> bool {
        self.failed == 0 && !self.aborted
    }
}

/// Configuration for a batch write run.
#[derive(Debug, Clone)]
pub struct BatchConfig {
    /// Path to the firmware file to write.
    pub path: std::path::PathBuf,
    /// Memory page (0 = code, 1 = data).
    pub page: u8,
    /// File format: "auto", "bin", "ihex", "srec", "jedec".
    pub format: String,
    /// How to handle file/device size mismatches.
    pub size_mismatch: SizeMismatch,
    /// Skip writing blank pages (all erase-value).
    pub skip_blank: bool,
    /// Whether to verify chip ID before operations.
    pub check_device_id: bool,
    /// Whether to erase before writing.
    pub erase: bool,
    /// Whether to verify after writing.
    pub verify: bool,
    /// Maximum number of chips to program (None = unlimited).
    pub count: Option<usize>,
}

/// Progress callback type: `(chip_number, total_or_none, phase)`.
pub type BatchProgressFn<'a> = Option<&'a mut dyn FnMut(usize, Option<usize>, &str)>;
/// Chip completion callback type.
pub type BatchChipCompleteFn<'a> = Option<&'a mut dyn FnMut(&BatchChipResult)>;
/// Ready-for-next-chip callback type. Returns false to abort.
pub type BatchReadyFn<'a> = Option<&'a mut dyn FnMut(usize) -> bool>;
/// Buffer patch callback type (for serial injection).
pub type BatchPatchFn<'a> = Option<&'a mut dyn FnMut(usize, &mut Vec<u8>)>;

/// Callbacks invoked during the batch loop.
pub struct BatchCallbacks<'a> {
    /// Called to report per-chip progress: `(chip_number, total_or_none, phase)`.
    /// Phase is "erase", "write", or "verify".
    pub on_progress: BatchProgressFn<'a>,
    /// Called after each chip completes (pass or fail).
    pub on_chip_complete: BatchChipCompleteFn<'a>,
    /// Called before each chip to prompt the user: "Insert chip N and press Enter".
    /// Returns `false` to abort the batch.
    pub on_ready: BatchReadyFn<'a>,
    /// Called before each write to patch the buffer (for serial injection).
    /// Receives the chip number and a mutable reference to the file buffer.
    /// The buffer is re-read from the file before each chip, so patches are
    /// applied to a fresh copy each time.
    pub on_patch_buffer: BatchPatchFn<'a>,
}

/// Run a batch of write operations on multiple identical chips.
///
/// The loop:
/// 1. Prompt user to insert chip N (via `on_ready` callback)
/// 2. Read the firmware file into a buffer
/// 3. Patch the buffer if `on_patch_buffer` is set (for serial injection)
/// 4. Erase (if configured)
/// 5. Write the buffer to the chip
/// 6. Verify (if configured)
/// 7. Record result and advance to next chip
///
/// The loop continues until `count` chips are done, or the user aborts via
/// `on_ready` returning `false`.
pub fn batch_write(
    handle: &mut MiniproHandle,
    config: &BatchConfig,
    callbacks: &mut BatchCallbacks,
) -> Result<BatchSummary> {
    let mut results = Vec::new();
    let mut aborted = false;
    let max = config.count.unwrap_or(usize::MAX);

    for chip_num in 1..=max {
        // ── Prompt user to insert chip ──────────────────────────────────────
        if let Some(ref mut on_ready) = callbacks.on_ready {
            if !on_ready(chip_num) {
                aborted = true;
                break;
            }
        }

        // ── Read the firmware file fresh for each chip ──────────────────────
        // (This allows buffer patching without mutating the original file.)
        let mut buf = read_file_for_batch(&config.path, &config.format, handle)?;

        // ── Patch buffer (e.g., serial number injection) ────────────────────
        if let Some(ref mut on_patch) = callbacks.on_patch_buffer {
            on_patch(chip_num, &mut buf);
        }

        // ── Erase ───────────────────────────────────────────────────────────
        let chip_result = batch_write_single(handle, config, &mut buf, chip_num, callbacks);

        let result = match chip_result {
            Ok(()) => BatchChipResult {
                chip_number: chip_num,
                success: true,
                error: None,
            },
            Err(ref e) => BatchChipResult {
                chip_number: chip_num,
                success: false,
                error: Some(format!("{e:#}")),
            },
        };

        if let Some(ref mut on_complete) = callbacks.on_chip_complete {
            on_complete(&result);
        }

        let was_success = result.success;
        results.push(result);

        if !was_success {
            // Stop on first failure — user should inspect the chip
            break;
        }
    }

    let passed = results.iter().filter(|r| r.success).count();
    let failed = results.iter().filter(|r| !r.success).count();

    Ok(BatchSummary {
        total: results.len(),
        passed,
        failed,
        results,
        aborted,
    })
}

/// Read a file for batch processing, padded to the device's memory size.
fn read_file_for_batch(path: &Path, format: &str, handle: &MiniproHandle) -> Result<Vec<u8>> {
    let device = handle.device()?;
    let size = match 0u8 {
        0x00 => device.code_memory_size as usize,
        0x01 => device.data_memory_size as usize,
        _ => device.code_memory_size as usize,
    };
    let buf = crate::operations::read_file(path, format, size, device.blank_value as u8)?;
    Ok(buf)
}

/// Write + verify a single chip within the batch loop.
fn batch_write_single(
    handle: &mut MiniproHandle,
    config: &BatchConfig,
    buf: &mut Vec<u8>,
    chip_num: usize,
    callbacks: &mut BatchCallbacks,
) -> Result<()> {
    let _ = chip_num;

    // ── Erase ───────────────────────────────────────────────────────────────
    if config.erase {
        if let Some(ref mut on_progress) = callbacks.on_progress {
            on_progress(chip_num, config.count, "erase");
        }
        erase_chip(handle, config.check_device_id)?;

        // Transaction reset after erase (same as CLI write flow)
        let device_arc = handle
            .device
            .clone()
            .ok_or_else(|| MiniproError::DeviceNotFound("no device selected".into()))?;
        handle.end_transaction()?;
        handle.begin_transaction(device_arc)?;
    }

    // ── Write ───────────────────────────────────────────────────────────────
    if let Some(ref mut on_progress) = callbacks.on_progress {
        on_progress(chip_num, config.count, "write");
    }

    // Use write_chip_bytes since we already have the buffer in memory.
    // Clone first so we can verify against the same patched buffer afterwards.
    let verify_buf = if config.verify {
        Some(buf.clone())
    } else {
        None
    };
    crate::operations::write_chip_bytes(
        handle,
        std::mem::take(buf),
        config.page,
        config.size_mismatch,
        config.skip_blank,
        config.check_device_id,
        None,
    )?;

    // ── Verify ──────────────────────────────────────────────────────────────
    if config.verify {
        if let Some(ref mut on_progress) = callbacks.on_progress {
            on_progress(chip_num, config.count, "verify");
        }

        // Transaction reset before verify (same as CLI auto-verify flow)
        let device_arc = handle
            .device
            .clone()
            .ok_or_else(|| MiniproError::DeviceNotFound("no device selected".into()))?;
        handle.end_transaction()?;
        handle.begin_transaction(device_arc)?;

        // Use verify_chip_bytes to verify against the patched buffer,
        // not the original file (which lacks the serial number).
        verify_chip_bytes(
            handle,
            verify_buf.unwrap(),
            config.page,
            config.check_device_id,
            None,
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config(start: u64, addr: usize, width: usize) -> SerialConfig {
        SerialConfig {
            start,
            address: addr,
            width,
            format: SerialFormat::Bin,
            endian: SerialEndian::Little,
            step: 1,
            checksum: SerialChecksum::None,
        }
    }

    #[test]
    fn test_bin_little_endian_4bytes() {
        let mut buf = vec![0u8; 16];
        let cfg = make_config(0x0001, 0, 4);
        patch_serial(&mut buf, &cfg, 1).unwrap();
        assert_eq!(&buf[0..4], &[0x01, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_bin_big_endian_4bytes() {
        let mut buf = vec![0u8; 16];
        let mut cfg = make_config(0x0001, 0, 4);
        cfg.endian = SerialEndian::Big;
        patch_serial(&mut buf, &cfg, 1).unwrap();
        assert_eq!(&buf[0..4], &[0x00, 0x00, 0x00, 0x01]);
    }

    #[test]
    fn test_bin_2bytes() {
        let mut buf = vec![0u8; 16];
        let cfg = make_config(0xABCD, 0, 2);
        patch_serial(&mut buf, &cfg, 1).unwrap();
        assert_eq!(&buf[0..2], &[0xCD, 0xAB]);
    }

    #[test]
    fn test_bin_1byte() {
        let mut buf = vec![0u8; 16];
        let cfg = make_config(0x42, 0, 1);
        patch_serial(&mut buf, &cfg, 1).unwrap();
        assert_eq!(buf[0], 0x42);
    }

    #[test]
    fn test_bin_8bytes() {
        let mut buf = vec![0u8; 16];
        let cfg = make_config(0x0102030405060708, 0, 8);
        patch_serial(&mut buf, &cfg, 1).unwrap();
        assert_eq!(
            &buf[0..8],
            &[0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01]
        );
    }

    #[test]
    fn test_ascii_format() {
        let mut buf = vec![0u8; 16];
        let mut cfg = make_config(1, 0, 5);
        cfg.format = SerialFormat::Ascii;
        patch_serial(&mut buf, &cfg, 1).unwrap();
        assert_eq!(&buf[0..6], b"00001\0");
    }

    #[test]
    fn test_ascii_format_large_number() {
        let mut buf = vec![0u8; 16];
        let mut cfg = make_config(12345, 0, 5);
        cfg.format = SerialFormat::Ascii;
        patch_serial(&mut buf, &cfg, 1).unwrap();
        assert_eq!(&buf[0..6], b"12345\0");
    }

    #[test]
    fn test_bcd_format() {
        let mut buf = vec![0u8; 16];
        let mut cfg = make_config(12345, 0, 5);
        cfg.format = SerialFormat::Bcd;
        patch_serial(&mut buf, &cfg, 1).unwrap();
        // 5 digits → 3 bytes: 0x12, 0x34, 0x5F
        assert_eq!(&buf[0..3], &[0x12, 0x34, 0x5F]);
    }

    #[test]
    fn test_bcd_format_even_digits() {
        let mut buf = vec![0u8; 16];
        let mut cfg = make_config(42, 0, 4);
        cfg.format = SerialFormat::Bcd;
        patch_serial(&mut buf, &cfg, 1).unwrap();
        // 4 digits → 2 bytes: 0x00, 0x42
        assert_eq!(&buf[0..2], &[0x00, 0x42]);
    }

    #[test]
    fn test_increment_step() {
        let mut buf = vec![0u8; 16];
        let cfg = make_config(0x0100, 0, 2);

        patch_serial(&mut buf, &cfg, 1).unwrap();
        assert_eq!(&buf[0..2], &[0x00, 0x01]);

        patch_serial(&mut buf, &cfg, 2).unwrap();
        assert_eq!(&buf[0..2], &[0x01, 0x01]);

        patch_serial(&mut buf, &cfg, 3).unwrap();
        assert_eq!(&buf[0..2], &[0x02, 0x01]);
    }

    #[test]
    fn test_custom_step() {
        let mut buf = vec![0u8; 16];
        let mut cfg = make_config(0x0001, 0, 4);
        cfg.step = 10;

        patch_serial(&mut buf, &cfg, 1).unwrap();
        assert_eq!(&buf[0..4], &[0x01, 0x00, 0x00, 0x00]);

        patch_serial(&mut buf, &cfg, 2).unwrap();
        assert_eq!(&buf[0..4], &[0x0B, 0x00, 0x00, 0x00]);

        patch_serial(&mut buf, &cfg, 3).unwrap();
        assert_eq!(&buf[0..4], &[0x15, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_xor_checksum() {
        let mut buf = vec![0u8; 16];
        let mut cfg = make_config(0x0102, 0, 2);
        cfg.checksum = SerialChecksum::Xor;
        patch_serial(&mut buf, &cfg, 1).unwrap();
        // serial bytes: 0x02, 0x01 → XOR = 0x03
        assert_eq!(&buf[0..3], &[0x02, 0x01, 0x03]);
    }

    #[test]
    fn test_crc8_checksum() {
        let mut buf = vec![0u8; 16];
        let mut cfg = make_config(0x0001, 0, 2);
        cfg.checksum = SerialChecksum::Crc8;
        patch_serial(&mut buf, &cfg, 1).unwrap();
        let expected = crc8(&[0x01, 0x00]);
        assert_eq!(&buf[0..3], &[0x01, 0x00, expected]);
    }

    #[test]
    fn test_address_offset() {
        let mut buf = vec![0u8; 32];
        let cfg = make_config(0xAB, 0x10, 1);
        patch_serial(&mut buf, &cfg, 1).unwrap();
        assert_eq!(buf[0x10], 0xAB);
        assert_eq!(buf[0], 0x00);
    }

    #[test]
    fn test_bounds_check_overflow() {
        let mut buf = vec![0u8; 8];
        let cfg = make_config(0x0001, 6, 4);
        let result = patch_serial(&mut buf, &cfg, 1);
        assert!(result.is_err());
    }

    #[test]
    fn test_width_validation() {
        let mut buf = vec![0u8; 16];
        let cfg = make_config(1, 0, 16);
        let result = patch_serial(&mut buf, &cfg, 1);
        assert!(result.is_err());
    }

    #[test]
    fn test_value_for_chip() {
        let cfg = make_config(100, 0, 4);
        assert_eq!(cfg.value_for_chip(1), 100);
        assert_eq!(cfg.value_for_chip(2), 101);
        assert_eq!(cfg.value_for_chip(10), 109);
    }

    #[test]
    fn test_value_for_chip_with_step() {
        let mut cfg = make_config(0, 0, 4);
        cfg.step = 5;
        assert_eq!(cfg.value_for_chip(1), 0);
        assert_eq!(cfg.value_for_chip(2), 5);
        assert_eq!(cfg.value_for_chip(3), 10);
        assert_eq!(cfg.value_for_chip(11), 50);
    }
}
