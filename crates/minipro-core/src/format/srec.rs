//! Motorola SREC (.srec / .mot) parser and writer.
//!
//! Supports S1 (16-bit address), S2 (24-bit), and S3 (32-bit) data records.

use std::{
    io::{BufRead, Write},
    path::Path,
};

use crate::error::{MiniproError, Result};

/// Read a Motorola SREC file and return a flat buffer of `target_size` bytes
/// padded with `blank_value`.
pub fn read(path: &Path, target_size: usize, blank_value: u8) -> Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);
    let mut buf = vec![blank_value; target_size];

    for line in reader.lines() {
        let line = line?;
        let line = line.trim();
        if line.len() < 4 {
            continue;
        }
        if !line.starts_with('S') {
            continue;
        }
        let rec_type = &line[1..2];
        let bytes = decode_hex_bytes(&line[2..])?;
        if bytes.is_empty() {
            continue;
        }

        let (addr_len, data_start) = match rec_type {
            "1" => (2usize, 3usize),
            "2" => (3, 4),
            "3" => (4, 5),
            _ => continue,
        };

        if bytes.len() < addr_len + 1 {
            continue;
        }

        let mut addr: u32 = 0;
        for i in 0..addr_len {
            addr = (addr << 8) | bytes[i + 1] as u32;
        }

        let data = &bytes[data_start..bytes.len() - 1]; // strip checksum
        for (i, &b) in data.iter().enumerate() {
            let pos = addr as usize + i;
            if pos < target_size {
                buf[pos] = b;
            }
        }
    }
    Ok(buf)
}

/// Write a flat buffer as a Motorola SREC file using S3/S7 records.
pub fn write(path: &Path, data: &[u8]) -> Result<()> {
    let mut f = std::fs::File::create(path)?;
    writeln!(f, "S0030000FC")?;

    let mut addr: u32 = 0;
    while addr < data.len() as u32 {
        let chunk_size = ((data.len() as u32 - addr) as usize).min(16);
        let chunk = &data[addr as usize..addr as usize + chunk_size];
        write_s3(&mut f, addr, chunk)?;
        addr += chunk_size as u32;
    }
    // S7 — end-of-file with entry address 0
    writeln!(f, "S70500000000FA")?;
    Ok(())
}

fn write_s3(f: &mut std::fs::File, addr: u32, data: &[u8]) -> Result<()> {
    // byte_count = 4 (addr) + data.len() + 1 (checksum)
    let byte_count = 5 + data.len();
    let mut sum: u8 = byte_count as u8;
    sum = sum
        .wrapping_add((addr >> 24) as u8)
        .wrapping_add(((addr >> 16) & 0xff) as u8)
        .wrapping_add(((addr >> 8) & 0xff) as u8)
        .wrapping_add((addr & 0xff) as u8);
    for &b in data {
        sum = sum.wrapping_add(b);
    }
    let checksum = !sum;

    write!(f, "S3{:02X}{:08X}", byte_count, addr)?;
    for &b in data {
        write!(f, "{:02X}", b)?;
    }
    writeln!(f, "{:02X}", checksum)?;
    Ok(())
}

fn decode_hex_bytes(hex: &str) -> Result<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return Err(MiniproError::FileFormat("odd-length SREC record".into()));
    }
    let bytes: std::result::Result<Vec<u8>, _> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
        .collect();
    bytes.map_err(|_| MiniproError::FileFormat("invalid hex in SREC file".into()))
}
