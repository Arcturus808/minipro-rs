//! Batch programming — program multiple identical chips with the same firmware.
//!
//! The batch loop handles: erase → write → verify for each chip, with callbacks
//! for progress reporting, "ready for next chip" prompting, and buffer patching
//! (for future serial number injection).

use std::path::Path;

use crate::{
    error::{MiniproError, Result},
    handle::MiniproHandle,
    operations::{erase_chip, verify_chip, SizeMismatch},
};

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

    // Use write_chip_bytes since we already have the buffer in memory
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

        verify_chip(
            handle,
            &config.path,
            config.page,
            &config.format,
            config.check_device_id,
            None,
        )?;
    }

    Ok(())
}
