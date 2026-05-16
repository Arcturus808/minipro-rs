//! Intel HEX (.hex) parser and writer.
//!
//! Supports I8HEX (20-bit, common for MCUs) and I32HEX (32-bit, common for
//! flash memory). Produces a flat byte buffer padded to `size` with the
//! chip's blank value.

use std::{
    io::{BufRead, Write},
    path::Path,
};

use crate::error::{MiniproError, Result};

const RECORD_DATA: u8 = 0x00;
const RECORD_EOF: u8 = 0x01;
const RECORD_EXTSEG: u8 = 0x02;
const RECORD_EXTALIN: u8 = 0x04;

/// Read an Intel HEX file and return a flat byte buffer of `target_size` bytes,
/// padded with `blank_value`.
pub fn read(path: &Path, target_size: usize, blank_value: u8) -> Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);
    let mut buf = vec![blank_value; target_size];
    let mut base_addr: u32 = 0;

    for line in reader.lines() {
        let line = line?;
        let line = line.trim();
        if !line.starts_with(':') {
            continue;
        }
        let bytes = decode_hex_line(line)?;
        if bytes.len() < 5 {
            return Err(MiniproError::FileFormat("HEX line too short".into()));
        }
        let byte_count = bytes[0] as usize;
        let addr = u16::from_be_bytes([bytes[1], bytes[2]]) as u32;
        let record_type = bytes[3];
        let data = &bytes[4..4 + byte_count];

        match record_type {
            RECORD_DATA => {
                let abs_addr = base_addr + addr;
                for (i, &b) in data.iter().enumerate() {
                    let pos = abs_addr as usize + i;
                    if pos < target_size {
                        buf[pos] = b;
                    }
                }
            }
            RECORD_EOF => break,
            RECORD_EXTSEG => {
                if data.len() >= 2 {
                    base_addr = (u16::from_be_bytes([data[0], data[1]]) as u32) << 4;
                }
            }
            RECORD_EXTALIN => {
                if data.len() >= 2 {
                    base_addr = (u16::from_be_bytes([data[0], data[1]]) as u32) << 16;
                }
            }
            _ => {}
        }
    }
    Ok(buf)
}

/// Write a flat byte buffer as an Intel HEX file (I32HEX format).
pub fn write(path: &Path, data: &[u8]) -> Result<()> {
    let mut f = std::fs::File::create(path)?;
    let mut addr: u32 = 0;

    while addr < data.len() as u32 {
        // Emit extended linear address record every 64 KiB
        if addr.is_multiple_of(0x10000) {
            let upper = (addr >> 16) as u16;
            write_record(&mut f, RECORD_EXTALIN, 0, &upper.to_be_bytes())?;
        }
        let chunk_size = ((data.len() as u32 - addr) as usize).min(16);
        let chunk = &data[addr as usize..addr as usize + chunk_size];
        write_record(&mut f, RECORD_DATA, addr as u16, chunk)?;
        addr += chunk_size as u32;
    }
    write_record(&mut f, RECORD_EOF, 0, &[])?;
    Ok(())
}

fn write_record(f: &mut std::fs::File, rtype: u8, addr: u16, data: &[u8]) -> Result<()> {
    let mut sum: u8 = 0;
    let len = data.len() as u8;
    let ah = (addr >> 8) as u8;
    let al = (addr & 0xff) as u8;

    sum = sum
        .wrapping_add(len)
        .wrapping_add(ah)
        .wrapping_add(al)
        .wrapping_add(rtype);
    for &b in data {
        sum = sum.wrapping_add(b);
    }
    let checksum = (!sum).wrapping_add(1);

    write!(f, ":{:02X}{:04X}{:02X}", len, addr, rtype)?;
    for &b in data {
        write!(f, "{:02X}", b)?;
    }
    writeln!(f, "{:02X}", checksum)?;
    Ok(())
}

fn decode_hex_line(line: &str) -> Result<Vec<u8>> {
    let hex = &line[1..]; // skip ':'
    if !hex.len().is_multiple_of(2) {
        return Err(MiniproError::FileFormat("odd-length HEX record".into()));
    }
    let bytes: std::result::Result<Vec<u8>, _> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
        .collect();
    bytes.map_err(|_| MiniproError::FileFormat("invalid hex digit in HEX file".into()))
}
