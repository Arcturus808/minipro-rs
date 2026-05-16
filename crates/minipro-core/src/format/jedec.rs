//! JEDEC fuse-map (.jed) parser and writer.
//!
//! Used for PLD / GAL devices.  Only the essential fields (device name, fuse
//! count, fuse data, and checksum) are processed; non-standard fields are
//! silently ignored.

use std::{
    io::{BufRead, Write},
    path::Path,
};

use crate::error::Result;

/// Parse a JEDEC file and return a flat bit buffer as `Vec<u8>` where each
/// byte is 0 or 1.  Pads to `fuse_count` bits if necessary.
pub fn read(path: &Path, fuse_count: usize) -> Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);
    let content: String = reader
        .lines()
        .collect::<std::result::Result<Vec<_>, _>>()?
        .join("\n");

    // Find the STX field (0x02) and extract the body
    let stx = content.find('\x02').unwrap_or(0);
    let etx = content.find('\x03').unwrap_or(content.len());
    let body = &content[stx..etx];

    let mut fuses = vec![1u8; fuse_count]; // default: erased = 1

    for field in body.split('*') {
        let field = field.trim();
        if field.is_empty() {
            continue;
        }
        let tag = &field[..1];

        match tag {
            // QF — fuse count (informational; we already know it)
            "Q" if field.starts_with("QF") => {}

            // L — fuse data: "L addr bits..."
            "L" => {
                let rest = &field[1..].trim_start();
                let mut parts = rest.splitn(2, char::is_whitespace);
                let addr_str = parts.next().unwrap_or("0");
                let bits_str = parts.next().unwrap_or("").replace(char::is_whitespace, "");
                let start: usize = addr_str.parse().unwrap_or(0);
                for (i, c) in bits_str.chars().enumerate() {
                    let idx = start + i;
                    if idx < fuse_count {
                        fuses[idx] = if c == '1' { 1 } else { 0 };
                    }
                }
            }
            _ => {}
        }
    }

    Ok(fuses)
}

/// Write a bit buffer as a JEDEC file.
/// `device_name` is written to the `N` device name field if provided.
pub fn write(path: &Path, fuses: &[u8], device_name: Option<&str>) -> Result<()> {
    let mut f = std::fs::File::create(path)?;

    // STX
    write!(f, "\x02")?;
    writeln!(f, "N Created by minipro-rs;")?;
    if let Some(name) = device_name {
        writeln!(f, "N Device {name};")?;
    }
    writeln!(f, "QF{}*", fuses.len())?;
    writeln!(f, "QP1*")?;
    writeln!(f, "F0*")?;

    // Fuse data in 32-bit columns, 16 per line
    for (i, chunk) in fuses.chunks(64).enumerate() {
        write!(f, "L{:04} ", i * 64)?;
        for &bit in chunk {
            write!(f, "{}", bit)?;
        }
        writeln!(f, "*")?;
    }

    // Checksum (sum of all fuse bits mod 65536, packed into 4 hex nibbles)
    let checksum: u16 = fuses.iter().map(|&b| b as u16).sum();
    writeln!(f, "C{:04X}*", checksum)?;

    // ETX + Unix checksum placeholder
    writeln!(f, "\x030000")?;
    Ok(())
}
