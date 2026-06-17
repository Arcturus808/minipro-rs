//! High-level chip operations.
//!
//! Implements read, write, verify, erase, blank-check and chip-id operations
//! using `MiniproHandle` and the `Protocol` trait.

use std::{
    io::{BufWriter, Read, Write},
    path::Path,
};

use log::info;

use crate::{
    device::{ChipType, DataOrg, Device},
    error::{MiniproError, Result},
    format::{ihex, jedec, srec},
    handle::MiniproHandle,
    protocol::DataSet,
};

/// Compute the effective read/write block size for a device.
///
/// For NAND chips this is the erase-block size (`write_buffer_size` *
/// `pages_per_block`).  For eMMC it is the fixed 64 KiB firmware block.
/// For all other chip types it falls back to the database `buffer_size` field.
fn effective_block_size(device: &Device, buffer_size: u16) -> usize {
    if device.chip_type == ChipType::Nand as u32 && device.pages_per_block > 0 {
        (buffer_size as usize) * (device.pages_per_block as usize)
    } else if device.chip_type == ChipType::Emmc as u32 {
        0x10000
    } else {
        buffer_size as usize
    }
}

/// Controls how a file-size mismatch between the input file and the device
/// memory is handled in [`write_chip`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeMismatch {
    /// Return an error (default).
    Error,
    /// Print a warning to stderr but continue, padding or truncating as needed.
    Warn,
    /// Silently pad or truncate without any message.
    Ignore,
}

/// Statistics returned by [`read_chip`] and [`write_chip`].
#[derive(Debug, Clone, Copy)]
pub struct OpStats {
    /// Number of bytes transferred.
    pub bytes: usize,
    /// CRC-32 (ISO-HDLC / PKZIP) of the data buffer.
    pub crc32: u32,
}

/// Resolve the effective file format: use `fmt` unless it is `"auto"`, in
/// which case infer from the file extension.
fn effective_format<'a>(fmt: &'a str, path: &Path) -> &'a str {
    if fmt != "auto" {
        return fmt;
    }
    match path.extension().and_then(|e| e.to_str()) {
        Some("hex") => "ihex",
        Some("srec") | Some("mot") => "srec",
        Some("jed") => "jedec",
        _ => "bin",
    }
}

/// Read a buffer from a file, or from stdin when `path` is `"-"`.
///
/// `format` is one of `"auto"` (detect from extension), `"bin"`, `"ihex"`,
/// `"srec"`, or `"jedec"`.  When reading from stdin and format is `"auto"`,
/// binary is assumed; pass an explicit format to decode text formats.
pub fn read_file(path: &Path, format: &str, size: usize, blank_value: u8) -> Result<Vec<u8>> {
    if path == Path::new("-") {
        let stdin = std::io::stdin();
        let mut reader = std::io::BufReader::new(stdin.lock());
        return match effective_format(format, Path::new("stdin")) {
            "ihex" => ihex::read_from(&mut reader, size, blank_value),
            "srec" => srec::read_from(&mut reader, size, blank_value),
            "jedec" => jedec::read_from(&mut reader, size),
            _ => {
                let mut buf = Vec::new();
                reader.read_to_end(&mut buf)?;
                Ok(buf)
            }
        };
    }
    match effective_format(format, path) {
        "ihex" => ihex::read(path, size, blank_value),
        "srec" => srec::read(path, size, blank_value),
        "jedec" => jedec::read(path, size),
        _ => Ok(std::fs::read(path)?),
    }
}

/// Write a buffer to a file, or to stdout when `path` is `"-"`.
///
/// `format` is one of `"auto"` (detect from extension), `"bin"`, `"ihex"`,
/// `"srec"`, or `"jedec"`.  When writing to stdout and format is `"auto"`,
/// binary is assumed; pass an explicit format to encode text formats.
pub fn write_file(path: &Path, format: &str, data: &[u8], device_name: Option<&str>) -> Result<()> {
    if path == Path::new("-") {
        let stdout = std::io::stdout();
        let mut writer = BufWriter::new(stdout.lock());
        return match effective_format(format, Path::new("stdout")) {
            "ihex" => ihex::write_to(&mut writer, data),
            "srec" => srec::write_to(&mut writer, data),
            "jedec" => jedec::write_to(&mut writer, data, device_name),
            _ => {
                writer.write_all(data)?;
                Ok(())
            }
        };
    }
    match effective_format(format, path) {
        "ihex" => ihex::write(path, data),
        "srec" => srec::write(path, data),
        "jedec" => jedec::write(path, data, device_name),
        _ => Ok(std::fs::write(path, data)?),
    }
}

/// Read chip memory and save to `path`.
///
/// `format` controls the output file format: `"auto"` (default, detect from
/// extension), `"bin"`, `"ihex"`, `"srec"`, or `"jedec"`.
///
/// `progress` is an optional callback invoked after each block with
/// `(bytes_done, total_bytes)`. Pass `None` to disable.
pub fn read_chip(
    handle: &mut MiniproHandle,
    path: &Path,
    page: u8,
    format: &str,
    check_device_id: bool,
    mut progress: Option<&mut dyn FnMut(usize, usize)>,
) -> Result<OpStats> {
    if check_device_id {
        check_chip_id(handle)?;
    }
    let device = handle.device()?.clone();
    let size = match page {
        0x00 => device.code_memory_size as usize,
        0x01 => device.data_memory_size as usize,
        _ => device.code_memory_size as usize,
    };

    let read_size = if device.read_buffer_size > 0 {
        effective_block_size(&device, device.read_buffer_size)
    } else {
        size
    };

    info!("Reading {} bytes...", size);
    let mut buf = vec![device.blank_value as u8; size];
    let total_blocks = if read_size > 0 {
        size.div_ceil(read_size)
    } else {
        1
    } as u32;
    let mut offset = 0usize;

    // Convert byte offset to word address when device uses 16-bit word
    // organisation for code memory (matches C read_page_ram address shift).
    let use_word_addr = device.flags.data_org == DataOrg::Words && page == 0x00;
    while offset < size {
        let block = (read_size).min(size - offset);
        let address = if use_word_addr {
            (offset as u32) >> 1
        } else {
            offset as u32
        };
        let mut ds = DataSet {
            data: vec![0u8; block],
            address,
            block_count: (block / 64) as u32,
            page_type: page,
            init: offset == 0,
            total_blocks,
        };
        handle.protocol.read_block(&handle.usb, &device, &mut ds)?;
        buf[offset..offset + block].copy_from_slice(&ds.data);
        offset += block;
        if let Some(ref mut f) = progress {
            f(offset, size);
        }
    }

    write_file(path, format, &buf, Some(&device.name))?;
    let crc32 = crc::Crc::<u32>::new(&crc::CRC_32_ISO_HDLC).checksum(&buf);
    Ok(OpStats { bytes: size, crc32 })
}

/// Write `path` to chip memory.
///
/// `format` controls how the input file is parsed: `"auto"` (default, detect
/// from extension), `"bin"`, `"ihex"`, `"srec"`, or `"jedec"`.
///
/// `progress` is an optional callback invoked after each block with
/// `(bytes_done, total_bytes)`. Pass `None` to disable.
#[allow(clippy::too_many_arguments)]
pub fn write_chip(
    handle: &mut MiniproHandle,
    path: &Path,
    page: u8,
    format: &str,
    size_mismatch: SizeMismatch,
    skip_blank: bool,
    check_device_id: bool,
    mut progress: Option<&mut dyn FnMut(usize, usize)>,
) -> Result<OpStats> {
    if check_device_id {
        check_chip_id(handle)?;
    }
    let device = handle.device()?.clone();
    let size = match page {
        0x00 => device.code_memory_size as usize,
        0x01 => device.data_memory_size as usize,
        _ => device.code_memory_size as usize,
    };

    let mut buf = read_file(path, format, size, device.blank_value as u8)?;

    // Size mismatch check (most relevant for raw binary files).
    if buf.len() != size {
        match size_mismatch {
            SizeMismatch::Error => {
                return Err(MiniproError::FileFormat(format!(
                    "file size {} does not match device size {}. \
Set Size Diff to 'Warn' or 'Ignore' to proceed.",
                    buf.len(),
                    size
                )));
            }
            SizeMismatch::Warn => eprintln!(
                "Warning: file size {} does not match device size {}; padding/truncating",
                buf.len(),
                size
            ),
            SizeMismatch::Ignore => {}
        }
        // Pad with blank_value or truncate to fit the device.
        buf.resize(size, device.blank_value as u8);
    }

    info!("Writing {} bytes...", size);

    let write_size = if device.write_buffer_size > 0 {
        effective_block_size(&device, device.write_buffer_size)
    } else {
        size
    };

    let total_blocks = if write_size > 0 {
        size.div_ceil(write_size)
    } else {
        1
    } as u32;
    let blank = device.blank_value as u8;
    // Convert byte offset to word address when device uses 16-bit word
    // organisation for code memory (matches C write_page_ram address shift).
    let use_word_addr = device.flags.data_org == DataOrg::Words && page == 0x00;
    let mut offset = 0usize;
    let mut has_written = false;
    let mut skipped = 0usize;
    while offset < size {
        let block = write_size.min(size - offset);
        let address = if use_word_addr {
            (offset as u32) >> 1
        } else {
            offset as u32
        };
        let page_data = &buf[offset..offset + block];
        let is_blank = page_data.iter().all(|&b| b == blank);
        if skip_blank && is_blank {
            skipped += block;
        } else {
            let ds = DataSet {
                data: page_data.to_vec(),
                address,
                block_count: (block / 64) as u32,
                page_type: page,
                init: !has_written,
                total_blocks,
            };
            handle.protocol.write_block(&handle.usb, &device, &ds)?;
            // The TL866A firmware writes the EEPROM asynchronously and uses the
            // GET_STATUS (0xFE) poll to wait for each write cycle to complete.
            // NAND and eMMC handle their own per-block status internally (0x39
            // commit in write_block); a zeroed 0x39 deselects them.
            if device.chip_type != ChipType::Nand as u32
                && device.chip_type != ChipType::Emmc as u32
            {
                let (wstatus, ovc) = handle.protocol.get_ovc_status(&handle.usb)?;
                if ovc != 0 {
                    return Err(MiniproError::Overcurrent {
                        address: wstatus.address,
                    });
                }
                if wstatus.error != 0 {
                    return Err(MiniproError::VerifyFailed {
                        address: wstatus.address,
                        expected: wstatus.c2 as u8,
                        actual: wstatus.c1 as u8,
                    });
                }
            }
            has_written = true;
        }
        offset += block;
        if let Some(ref mut f) = progress {
            f(offset, size);
        }
    }
    if skipped > 0 {
        info!("Skipped {} blank bytes", skipped);
    }
    let crc32 = crc::Crc::<u32>::new(&crc::CRC_32_ISO_HDLC).checksum(&buf);
    Ok(OpStats { bytes: size, crc32 })
}

/// Write `buf` (raw bytes) to chip memory.
///
/// Same as [`write_chip`] but takes an in-memory buffer instead of a file path.
/// The buffer is padded with the device's blank value or truncated to match the
/// selected memory page size.
#[allow(clippy::too_many_arguments)]
pub fn write_chip_bytes(
    handle: &mut MiniproHandle,
    mut buf: Vec<u8>,
    page: u8,
    size_mismatch: SizeMismatch,
    skip_blank: bool,
    check_device_id: bool,
    mut progress: Option<&mut dyn FnMut(usize, usize)>,
) -> Result<OpStats> {
    if check_device_id {
        check_chip_id(handle)?;
    }
    let device = handle.device()?.clone();
    let size = match page {
        0x00 => device.code_memory_size as usize,
        0x01 => device.data_memory_size as usize,
        _ => device.code_memory_size as usize,
    };

    // Size mismatch check.
    if buf.len() != size {
        match size_mismatch {
            SizeMismatch::Error => {
                return Err(MiniproError::FileFormat(format!(
                    "buffer size {} does not match device size {}. \
Set Size Diff to 'Warn' or 'Ignore' to proceed.",
                    buf.len(),
                    size
                )));
            }
            SizeMismatch::Warn => eprintln!(
                "Warning: buffer size {} does not match device size {}; padding/truncating",
                buf.len(),
                size
            ),
            SizeMismatch::Ignore => {}
        }
        buf.resize(size, device.blank_value as u8);
    }

    info!("Writing {} bytes...", size);

    let write_size = if device.write_buffer_size > 0 {
        effective_block_size(&device, device.write_buffer_size)
    } else {
        size
    };

    let total_blocks = if write_size > 0 {
        size.div_ceil(write_size)
    } else {
        1
    } as u32;
    let blank = device.blank_value as u8;
    let use_word_addr = device.flags.data_org == DataOrg::Words && page == 0x00;
    let mut offset = 0usize;
    let mut has_written = false;
    let mut skipped = 0usize;
    while offset < size {
        let block = write_size.min(size - offset);
        let address = if use_word_addr {
            (offset as u32) >> 1
        } else {
            offset as u32
        };
        let page_data = &buf[offset..offset + block];
        let is_blank = page_data.iter().all(|&b| b == blank);
        if skip_blank && is_blank {
            skipped += block;
        } else {
            let ds = DataSet {
                data: page_data.to_vec(),
                address,
                block_count: (block / 64) as u32,
                page_type: page,
                init: !has_written,
                total_blocks,
            };
            handle.protocol.write_block(&handle.usb, &device, &ds)?;
            // NAND and eMMC handle their own per-block status internally.
            if device.chip_type != ChipType::Nand as u32
                && device.chip_type != ChipType::Emmc as u32
            {
                let (wstatus, ovc) = handle.protocol.get_ovc_status(&handle.usb)?;
                if ovc != 0 {
                    return Err(MiniproError::Overcurrent {
                        address: wstatus.address,
                    });
                }
                if wstatus.error != 0 {
                    return Err(MiniproError::VerifyFailed {
                        address: wstatus.address,
                        expected: wstatus.c2 as u8,
                        actual: wstatus.c1 as u8,
                    });
                }
            }
            has_written = true;
        }
        offset += block;
        if let Some(ref mut f) = progress {
            f(offset, size);
        }
    }
    if skipped > 0 {
        info!("Skipped {} blank bytes", skipped);
    }
    let crc32 = crc::Crc::<u32>::new(&crc::CRC_32_ISO_HDLC).checksum(&buf);
    Ok(OpStats { bytes: size, crc32 })
}

/// Verify chip memory against `path`.
///
/// `format` controls how the reference file is parsed: `"auto"` (default),
/// `"bin"`, `"ihex"`, `"srec"`, or `"jedec"`.
///
/// `progress` is an optional callback invoked after each block with
/// `(bytes_done, total_bytes)`. Pass `None` to disable.
pub fn verify_chip(
    handle: &mut MiniproHandle,
    path: &Path,
    page: u8,
    format: &str,
    check_device_id: bool,
    mut progress: Option<&mut dyn FnMut(usize, usize)>,
) -> Result<()> {
    if check_device_id {
        check_chip_id(handle)?;
    }
    let device = handle.device()?.clone();
    let size = match page {
        0x00 => device.code_memory_size as usize,
        0x01 => device.data_memory_size as usize,
        _ => device.code_memory_size as usize,
    };
    let mut expected = read_file(path, format, size, device.blank_value as u8)?;
    // Pad or truncate the reference file to match the device size.
    expected.resize(size, device.blank_value as u8);
    info!("Verifying {} bytes...", size);

    let read_size = if device.read_buffer_size > 0 {
        effective_block_size(&device, device.read_buffer_size)
    } else {
        size
    };

    let total_blocks = if read_size > 0 {
        size.div_ceil(read_size)
    } else {
        1
    } as u32;
    // Convert byte offset to word address for word-organised code memory,
    // mirroring the same shift applied in read_chip and write_chip.
    let use_word_addr = device.flags.data_org == DataOrg::Words && page == 0x00;
    let mut offset = 0usize;
    while offset < size {
        let block = read_size.min(size - offset);
        let address = if use_word_addr {
            (offset as u32) >> 1
        } else {
            offset as u32
        };
        let mut ds = DataSet {
            data: vec![0u8; block],
            address,
            block_count: (block / 64) as u32,
            page_type: page,
            init: offset == 0,
            total_blocks,
        };
        handle.protocol.read_block(&handle.usb, &device, &mut ds)?;
        for (i, (&got, &want)) in ds.data.iter().zip(expected[offset..].iter()).enumerate() {
            if got != want {
                return Err(MiniproError::VerifyFailed {
                    address: (offset + i) as u32,
                    expected: want,
                    actual: got,
                });
            }
        }
        offset += block;
        if let Some(ref mut f) = progress {
            f(offset, size);
        }
    }
    Ok(())
}

/// Verify chip memory against `expected` (raw bytes).
///
/// Same as [`verify_chip`] but takes an in-memory buffer instead of a file path.
pub fn verify_chip_bytes(
    handle: &mut MiniproHandle,
    mut expected: Vec<u8>,
    page: u8,
    check_device_id: bool,
    mut progress: Option<&mut dyn FnMut(usize, usize)>,
) -> Result<()> {
    if check_device_id {
        check_chip_id(handle)?;
    }
    let device = handle.device()?.clone();
    let size = match page {
        0x00 => device.code_memory_size as usize,
        0x01 => device.data_memory_size as usize,
        _ => device.code_memory_size as usize,
    };
    expected.resize(size, device.blank_value as u8);
    info!("Verifying {} bytes...", size);

    let read_size = if device.read_buffer_size > 0 {
        effective_block_size(&device, device.read_buffer_size)
    } else {
        size
    };

    let total_blocks = if read_size > 0 {
        size.div_ceil(read_size)
    } else {
        1
    } as u32;
    let use_word_addr = device.flags.data_org == DataOrg::Words && page == 0x00;
    let mut offset = 0usize;
    while offset < size {
        let block = read_size.min(size - offset);
        let address = if use_word_addr {
            (offset as u32) >> 1
        } else {
            offset as u32
        };
        let mut ds = DataSet {
            data: vec![0u8; block],
            address,
            block_count: (block / 64) as u32,
            page_type: page,
            init: offset == 0,
            total_blocks,
        };
        handle.protocol.read_block(&handle.usb, &device, &mut ds)?;
        for (i, (&got, &want)) in ds.data.iter().zip(expected[offset..].iter()).enumerate() {
            if got != want {
                return Err(MiniproError::VerifyFailed {
                    address: (offset + i) as u32,
                    expected: want,
                    actual: got,
                });
            }
        }
        offset += block;
        if let Some(ref mut f) = progress {
            f(offset, size);
        }
    }
    Ok(())
}

/// Blank-check the chip.
pub fn blank_check(handle: &mut MiniproHandle) -> Result<()> {
    let device = handle.device()?.clone();
    let size = device.code_memory_size as usize;
    let blank = device.blank_value as u8;

    let read_size = if device.read_buffer_size > 0 {
        effective_block_size(&device, device.read_buffer_size)
    } else {
        size
    };

    info!(
        "blank_check: device={} size={} blank=0x{:02x} read_buffer_size={} read_size={}",
        device.name, size, blank, device.read_buffer_size, read_size
    );

    let total_blocks = if read_size > 0 {
        size.div_ceil(read_size)
    } else {
        1
    } as u32;
    let mut offset = 0usize;
    while offset < size {
        let block = read_size.min(size - offset);
        info!(
            "blank_check: reading block offset={} block={} total_blocks={}",
            offset, block, total_blocks
        );
        let mut ds = DataSet {
            data: vec![0u8; block],
            address: offset as u32,
            block_count: (block / 64) as u32,
            page_type: 0x00,
            init: offset == 0,
            total_blocks,
        };
        handle.protocol.read_block(&handle.usb, &device, &mut ds)?;
        for (i, &b) in ds.data.iter().enumerate() {
            if b != blank {
                return Err(MiniproError::NotBlank {
                    address: (offset + i) as u32,
                });
            }
        }
        offset += block;
    }
    Ok(())
}

/// Read the OSCCAL calibration word from the last location of code memory.
///
/// For PIC microcontrollers with `osccal_save == 1`, the factory RC oscillator
/// calibration value is stored in the last word of program memory.
fn read_osccal(handle: &mut MiniproHandle) -> Result<Vec<u8>> {
    use crate::device::ChipConfig;

    let device = handle.device()?.clone();
    let config = match device.config.as_ref() {
        Some(ChipConfig::Mcu(fc)) => fc,
        _ => return Ok(vec![]),
    };
    if config.osccal_save == 0 {
        return Ok(vec![]);
    }

    let word_size = device.word_size();
    let address = device.code_memory_size / word_size as u32;
    let mut ds = DataSet {
        data: vec![0u8; word_size],
        address,
        block_count: (word_size / 64) as u32,
        page_type: 0x00,
        init: true,
        total_blocks: 1,
    };
    handle.protocol.read_block(&handle.usb, &device, &mut ds)?;
    Ok(ds.data)
}

/// Write the OSCCAL calibration word to the last location of code memory.
fn write_osccal(handle: &mut MiniproHandle, data: &[u8]) -> Result<()> {
    use crate::device::ChipConfig;

    let device = handle.device()?.clone();
    let config = match device.config.as_ref() {
        Some(ChipConfig::Mcu(fc)) => fc,
        _ => return Ok(()),
    };
    if config.osccal_save == 0 {
        return Ok(());
    }

    let word_size = device.word_size();
    if data.len() != word_size {
        return Err(MiniproError::Protocol(format!(
            "OSCCAL word size mismatch: expected {} bytes, got {}",
            word_size,
            data.len()
        )));
    }

    let address = device.code_memory_size / word_size as u32;
    let ds = DataSet {
        data: data.to_vec(),
        address,
        block_count: (word_size / 64) as u32,
        page_type: 0x00,
        init: true,
        total_blocks: 1,
    };
    handle.protocol.write_block(&handle.usb, &device, &ds)?;
    Ok(())
}

/// Erase the chip.
pub fn erase_chip(handle: &mut MiniproHandle, check_device_id: bool) -> Result<()> {
    if check_device_id {
        check_chip_id(handle)?;
    }
    let device = handle.device()?.clone();

    // Save OSCCAL calibration word before erase
    let osccal = read_osccal(handle)?;

    let num_fuses = device
        .config
        .as_ref()
        .map(|c| match c {
            crate::device::ChipConfig::Mcu(fc) => fc.fuses.len() as u8,
            crate::device::ChipConfig::Pld(_) => 0,
        })
        .unwrap_or(0);
    handle
        .protocol
        .erase(&handle.usb, &device, num_fuses, device.chip_type == 0x03)?;

    // Restore OSCCAL after erase
    if !osccal.is_empty() {
        write_osccal(handle, &osccal)?;
    }

    Ok(())
}

/// Left-align a chip ID so the most-significant non-zero byte sits at bit 31.
/// This allows comparison across programmer models that pack bytes differently.
pub fn normalize_chip_id(id: u32) -> u32 {
    if id == 0 {
        return 0;
    }
    let masks = [0xff_u32, 0xff00, 0xff_0000, 0xff00_0000];
    for (i, &m) in masks.iter().enumerate().rev() {
        if id & m != 0 {
            let shift = (3 - i) * 8;
            return id << shift;
        }
    }
    id << 24
}

/// Read chip ID and compare against expected value.
///
/// Always attempts the ID read when the device claims ID support (`has_chip_id`).
/// If the database has an expected value, compares against it. If the database has
/// no expected value (`chip_id == 0`) but a non-zero/blank ID is read, the chip
/// is present but of an unknown/wrong type — still report mismatch.
pub fn check_chip_id(handle: &mut MiniproHandle) -> Result<()> {
    let device = handle.device()?.clone();
    if !device.flags.has_chip_id {
        return Ok(());
    }
    let (_id_type, actual) = handle.protocol.get_chip_id(&handle.usb)?;
    let expected_norm = normalize_chip_id(device.chip_id);
    let actual_norm = normalize_chip_id(actual);
    if device.chip_id == 0 {
        // No expected ID in database, but the chip supports ID read.
        // A non-zero/blank response means a chip is present; we can't verify
        // it's the right one, so warn and let the caller decide.
        if actual_norm != 0 {
            return Err(MiniproError::ChipIdMismatch {
                expected: 0,
                actual,
            });
        }
        info!(
            "Chip ID read returned {:#010x}; database has no expected value for this device",
            actual
        );
    } else if actual_norm != expected_norm {
        return Err(MiniproError::ChipIdMismatch {
            expected: device.chip_id,
            actual,
        });
    } else {
        info!("Chip ID OK: {:#010x}", actual);
    }
    Ok(())
}

/// Check over-current and return `true` if an OVC event occurred.
pub fn check_ovc(handle: &mut MiniproHandle) -> Result<bool> {
    // NAND and eMMC: get_ovc_status with a zeroed header deselects the chip.
    // Skip the poll; per-block status is handled internally by write_block.
    if let Some(ref device) = handle.device {
        if device.chip_type == ChipType::Nand as u32 || device.chip_type == ChipType::Emmc as u32 {
            return Ok(false);
        }
    }
    let (status, flag) = handle.protocol.get_ovc_status(&handle.usb)?;
    if flag != 0 || status.error != 0 {
        return Err(MiniproError::Overcurrent {
            address: status.address,
        });
    }
    Ok(false)
}

// ── Fuse sub-type constants ──────────────────────────────────────────────────
/// MCU user-data fuses (TL866A cmd 0x10/0x11).
pub const MP_FUSE_USER: u8 = 0x00;
/// MCU configuration fuses (e.g. AVR lfuse/hfuse/efuse, PIC config words).
pub const MP_FUSE_CFG: u8 = 0x01;
/// MCU lock bits.
pub const MP_FUSE_LOCK: u8 = 0x02;

/// Named fuse value.
#[derive(Debug, Clone)]
pub struct FuseValue {
    pub name: String,
    pub value: u8,
}

/// Read all fuse bytes from the chip and map them to named fields.
///
/// Returns a `Vec<FuseValue>` with one entry per fuse field defined in the
/// device's `ChipConfig`.  Fields that don't have a corresponding read result
/// byte default to `0xff`.
pub fn read_fuses(handle: &mut MiniproHandle) -> Result<Vec<FuseValue>> {
    use crate::device::ChipConfig;

    let device = handle.device()?.clone();
    let config = match device.config.as_ref() {
        Some(ChipConfig::Mcu(fc)) => fc.clone(),
        _ => return Err(MiniproError::UnsupportedOperation),
    };

    let fuse_count = config.fuses.len() as u8;
    let lock_count = config.locks.len() as u8;

    let device_ref = &device;
    // Read CFG fuses
    let cfg_bytes = handle
        .protocol
        .read_fuses(
            &handle.usb,
            device_ref,
            MP_FUSE_CFG,
            fuse_count as usize,
            fuse_count,
        )
        .unwrap_or_default();

    // Read LOCK bits (optional — not all devices have them)
    let lock_bytes = if lock_count > 0 {
        handle
            .protocol
            .read_fuses(
                &handle.usb,
                device_ref,
                MP_FUSE_LOCK,
                lock_count as usize,
                lock_count,
            )
            .unwrap_or_default()
    } else {
        vec![]
    };

    let mut result = Vec::with_capacity(config.fuses.len() + config.locks.len());

    for (i, field) in config.fuses.iter().enumerate() {
        result.push(FuseValue {
            name: field.name.clone(),
            value: cfg_bytes.get(i).copied().unwrap_or(0xff),
        });
    }
    for (i, field) in config.locks.iter().enumerate() {
        result.push(FuseValue {
            name: field.name.clone(),
            value: lock_bytes.get(i).copied().unwrap_or(0xff),
        });
    }
    Ok(result)
}

/// Write fuse values to the chip.
///
/// `fuses` should contain values in the same order as returned by `read_fuses`.
/// Values are split back into CFG fuses and LOCK bits based on the device config.
pub fn write_fuses(handle: &mut MiniproHandle, fuses: &[FuseValue]) -> Result<()> {
    use crate::device::ChipConfig;

    let device = handle.device()?.clone();
    let config = match device.config.as_ref() {
        Some(ChipConfig::Mcu(fc)) => fc.clone(),
        _ => return Err(MiniproError::UnsupportedOperation),
    };

    let fuse_count = config.fuses.len();
    let lock_count = config.locks.len();

    let cfg_data: Vec<u8> = fuses.iter().take(fuse_count).map(|f| f.value).collect();

    let lock_data: Vec<u8> = fuses
        .iter()
        .skip(fuse_count)
        .take(lock_count)
        .map(|f| f.value)
        .collect();

    let device_ref = &device;

    if !cfg_data.is_empty() {
        handle.protocol.write_fuses(
            &handle.usb,
            device_ref,
            MP_FUSE_CFG,
            fuse_count,
            fuse_count as u8,
            &cfg_data,
        )?;
    }
    if !lock_data.is_empty() {
        handle.protocol.write_fuses(
            &handle.usb,
            device_ref,
            MP_FUSE_LOCK,
            lock_count,
            lock_count as u8,
            &lock_data,
        )?;
    }
    Ok(())
}

/// Read the chip's calibration bytes (OSCCAL word for PIC devices).
///
/// For devices with `osccal_save == 1`, returns the calibration word bytes
/// from the last location of code memory. For other devices, returns an empty
/// vector.
pub fn read_chip_calibration(handle: &mut MiniproHandle) -> Result<Vec<u8>> {
    read_osccal(handle)
}

// ── Phase 4 operations ───────────────────────────────────────────────────────

/// Test a logic IC against its built-in test vectors.
///
/// Run the programmer's built-in hardware self-test.
///
/// No chip needs to be inserted or a device selected.  Returns an error on
/// test failure or if the programmer does not support the test (TL866A/CS).
pub fn hardware_check(handle: &mut MiniproHandle) -> Result<()> {
    handle.protocol.hardware_check(&handle.usb)
}

/// Perform a pin-contact test on the device currently loaded in the ZIF socket.
///
/// `infoic_path` must point to `infoic.xml` so that the programmer-independent
/// pin-map table (`<maps>`) can be located.  If the device has `pin_map == 0`
/// (no contact-test data in the database) this returns `Ok(())` immediately.
pub fn pin_contact_check(handle: &mut MiniproHandle, infoic_path: &std::path::Path) -> Result<()> {
    let device = handle.device()?.clone();
    let index = device.pin_map & 0xFF;
    let pin_map = match crate::database::get_pin_map(infoic_path, index)? {
        Some(pm) => pm,
        None => {
            eprintln!("Pin contact check not available for this device.");
            return Ok(());
        }
    };
    handle.protocol.pin_test(&handle.usb, &device, &pin_map)
}

/// The device must have been opened with `begin_transaction` against a logic IC
/// entry from `logicic.xml`.  Returns an error if the IC fails any vector.
pub fn logic_ic_test(handle: &mut MiniproHandle, out: &mut dyn std::io::Write) -> Result<()> {
    let device = handle.device()?.clone();
    handle.protocol.logic_ic_test(&handle.usb, &device, out)
}

/// Flash new firmware from a binary image file.
///
/// Supported formats:
///  - TL866II+/T48: `UpdateII.dat`
///  - T76: `updateT76.dat`
pub fn firmware_update(handle: &mut MiniproHandle, firmware_data: &[u8]) -> Result<()> {
    handle.protocol.firmware_update(&handle.usb, firmware_data)
}

/// Auto-detect an SPI flash chip by reading its JEDEC ID.
///
/// `id_type` selects the package: 0 = 8-pin SOP/DIP, 1 = 16-pin.
/// Returns the 3-byte JEDEC manufacturer+device ID packed into a u32.
pub fn spi_autodetect(handle: &mut MiniproHandle, id_type: u8) -> Result<u32> {
    handle.protocol.spi_autodetect(&handle.usb, id_type)
}
