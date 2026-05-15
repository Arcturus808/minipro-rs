//! High-level chip operations.
//!
//! Implements read, write, verify, erase, blank-check and chip-id operations
//! using `MiniproHandle` and the `Protocol` trait.

use std::{
    path::Path,
    sync::Arc,
};

use crate::{
    database::{DatabasePaths, find_device},
    device::{Device, ProgrammerModel},
    error::{MiniproError, Result},
    format::{ihex, jedec, srec},
    handle::MiniproHandle,
    protocol::DataSet,
};

/// Auto-detect the file format from its extension.
fn detect_format(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("hex")          => "ihex",
        Some("srec") |
        Some("mot")          => "srec",
        Some("jed")          => "jedec",
        Some("bin") | _      => "bin",
    }
}

/// Read a buffer from a file; format is inferred from the extension.
pub fn read_file(path: &Path, size: usize, blank_value: u8) -> Result<Vec<u8>> {
    match detect_format(path) {
        "ihex"  => ihex::read(path, size, blank_value),
        "srec"  => srec::read(path, size, blank_value),
        "jedec" => {
            let bits = jedec::read(path, size)?;
            Ok(bits)
        }
        _ => Ok(std::fs::read(path)?),
    }
}

/// Write a buffer to a file; format is inferred from the extension.
pub fn write_file(path: &Path, data: &[u8], device_name: Option<&str>) -> Result<()> {
    match detect_format(path) {
        "ihex"  => ihex::write(path, data),
        "srec"  => srec::write(path, data),
        "jedec" => jedec::write(path, data, device_name),
        _       => Ok(std::fs::write(path, data)?),
    }
}

/// Read chip memory and save to `path`.
pub fn read_chip(
    handle:  &mut MiniproHandle,
    path:    &Path,
    page:    u8,
) -> Result<()> {
    let device = handle.device()?.clone();
    let size = match page {
        0x00 => device.code_memory_size as usize,
        0x01 => device.data_memory_size as usize,
        _    => device.code_memory_size as usize,
    };

    let read_size = if device.read_buffer_size > 0 {
        device.read_buffer_size as usize
    } else {
        size
    };

    let mut buf = vec![device.blank_value as u8; size];
    let mut offset = 0usize;

    while offset < size {
        let block = (read_size).min(size - offset);
        let mut ds = DataSet {
            data:        vec![0u8; block],
            address:     offset as u32,
            block_count: (block / 64) as u32,
            page_type:   page,
        };
        handle.protocol.read_block(&handle.usb, &mut ds)?;
        buf[offset..offset + block].copy_from_slice(&ds.data);
        offset += block;
    }

    write_file(path, &buf, Some(&device.name))
}

/// Write `path` to chip memory.
pub fn write_chip(
    handle:  &mut MiniproHandle,
    path:    &Path,
    page:    u8,
) -> Result<()> {
    let device = handle.device()?.clone();
    let size = match page {
        0x00 => device.code_memory_size as usize,
        0x01 => device.data_memory_size as usize,
        _    => device.code_memory_size as usize,
    };

    let buf = read_file(path, size, device.blank_value as u8)?;

    let write_size = if device.write_buffer_size > 0 {
        device.write_buffer_size as usize
    } else {
        size
    };

    let mut offset = 0usize;
    while offset < size {
        let block = write_size.min(size - offset);
        let ds = DataSet {
            data:        buf[offset..offset + block].to_vec(),
            address:     offset as u32,
            block_count: (block / 64) as u32,
            page_type:   page,
        };
        handle.protocol.write_block(&handle.usb, &ds)?;
        offset += block;
    }
    Ok(())
}

/// Verify chip memory against `path`.
pub fn verify_chip(
    handle:  &mut MiniproHandle,
    path:    &Path,
    page:    u8,
) -> Result<()> {
    let device = handle.device()?.clone();
    let size = match page {
        0x00 => device.code_memory_size as usize,
        0x01 => device.data_memory_size as usize,
        _    => device.code_memory_size as usize,
    };
    let expected = read_file(path, size, device.blank_value as u8)?;

    let read_size = if device.read_buffer_size > 0 {
        device.read_buffer_size as usize
    } else {
        size
    };

    let mut offset = 0usize;
    while offset < size {
        let block = read_size.min(size - offset);
        let mut ds = DataSet {
            data:        vec![0u8; block],
            address:     offset as u32,
            block_count: (block / 64) as u32,
            page_type:   page,
        };
        handle.protocol.read_block(&handle.usb, &mut ds)?;
        for (i, (&got, &want)) in ds.data.iter().zip(expected[offset..].iter()).enumerate() {
            if got != want {
                return Err(MiniproError::VerifyFailed {
                    address:  (offset + i) as u32,
                    expected: want,
                    actual:   got,
                });
            }
        }
        offset += block;
    }
    Ok(())
}

/// Blank-check the chip.
pub fn blank_check(handle: &mut MiniproHandle) -> Result<()> {
    let device = handle.device()?.clone();
    let size = device.code_memory_size as usize;
    let blank = device.blank_value as u8;

    let read_size = if device.read_buffer_size > 0 {
        device.read_buffer_size as usize
    } else {
        size
    };

    let mut offset = 0usize;
    while offset < size {
        let block = read_size.min(size - offset);
        let mut ds = DataSet {
            data:        vec![0u8; block],
            address:     offset as u32,
            block_count: (block / 64) as u32,
            page_type:   0x00,
        };
        handle.protocol.read_block(&handle.usb, &mut ds)?;
        for (i, &b) in ds.data.iter().enumerate() {
            if b != blank {
                return Err(MiniproError::NotBlank { address: (offset + i) as u32 });
            }
        }
        offset += block;
    }
    Ok(())
}

/// Erase the chip.
pub fn erase_chip(handle: &mut MiniproHandle) -> Result<()> {
    let device = handle.device()?.clone();
    let num_fuses = device
        .config
        .as_ref()
        .map(|c| match c {
            crate::device::ChipConfig::Mcu(fc) => fc.fuses.len() as u8,
            crate::device::ChipConfig::Pld(_)  => 0,
        })
        .unwrap_or(0);
    handle.protocol.erase(&handle.usb, num_fuses, device.chip_type == 0x03)
}

/// Read chip ID and compare against expected value.
pub fn check_chip_id(handle: &mut MiniproHandle) -> Result<()> {
    let device = handle.device()?.clone();
    if device.chip_id == 0 || !device.flags.has_chip_id {
        return Ok(());
    }
    let (_id_type, actual) = handle.protocol.get_chip_id(&handle.usb)?;
    if actual != device.chip_id {
        return Err(MiniproError::ChipIdMismatch {
            expected: device.chip_id,
            actual,
        });
    }
    Ok(())
}

/// Check over-current and return `true` if an OVC event occurred.
pub fn check_ovc(handle: &mut MiniproHandle) -> Result<bool> {
    let (status, flag) = handle.protocol.get_ovc_status(&handle.usb)?;
    if flag != 0 || status.error != 0 {
        return Err(MiniproError::Overcurrent { address: status.address });
    }
    Ok(false)
}

// ── Fuse sub-type constants ──────────────────────────────────────────────────
/// MCU user-data fuses (TL866A cmd 0x10/0x11).
pub const MP_FUSE_USER: u8 = 0x00;
/// MCU configuration fuses (e.g. AVR lfuse/hfuse/efuse, PIC config words).
pub const MP_FUSE_CFG:  u8 = 0x01;
/// MCU lock bits.
pub const MP_FUSE_LOCK: u8 = 0x02;

/// Named fuse value.
#[derive(Debug, Clone)]
pub struct FuseValue {
    pub name:  String,
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

    // Read CFG fuses
    let cfg_bytes = handle.protocol.read_fuses(
        &handle.usb, MP_FUSE_CFG, fuse_count as usize, fuse_count,
    ).unwrap_or_default();

    // Read LOCK bits (optional — not all devices have them)
    let lock_bytes = if lock_count > 0 {
        handle.protocol.read_fuses(
            &handle.usb, MP_FUSE_LOCK, lock_count as usize, lock_count,
        ).unwrap_or_default()
    } else {
        vec![]
    };

    let mut result = Vec::with_capacity(config.fuses.len() + config.locks.len());

    for (i, field) in config.fuses.iter().enumerate() {
        result.push(FuseValue {
            name:  field.name.clone(),
            value: cfg_bytes.get(i).copied().unwrap_or(0xff),
        });
    }
    for (i, field) in config.locks.iter().enumerate() {
        result.push(FuseValue {
            name:  field.name.clone(),
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

    let cfg_data: Vec<u8> = fuses.iter()
        .take(fuse_count)
        .map(|f| f.value)
        .collect();

    let lock_data: Vec<u8> = fuses.iter()
        .skip(fuse_count)
        .take(lock_count)
        .map(|f| f.value)
        .collect();

    if !cfg_data.is_empty() {
        handle.protocol.write_fuses(
            &handle.usb,
            MP_FUSE_CFG,
            fuse_count,
            fuse_count as u8,
            &cfg_data,
        )?;
    }
    if !lock_data.is_empty() {
        handle.protocol.write_fuses(
            &handle.usb,
            MP_FUSE_LOCK,
            lock_count,
            lock_count as u8,
            &lock_data,
        )?;
    }
    Ok(())
}
