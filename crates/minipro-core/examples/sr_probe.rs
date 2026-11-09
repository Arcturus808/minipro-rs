//! SPI status-register probe: reads all three fuse sub-types for a chip
//! (READ_LOCK 0x15 / READ_CFG 0x08 / READ_USER 0x06), and optionally does
//! a write -> readback -> restore cycle on fuse type 0x01, which is the
//! real SPI status register on T76 (verified on W25X20CL).
//!
//! Usage: sr_probe [PART] [WRITE_BYTE_HEX]
//!   MINIPRO_HOME=<repo>/data cargo run --release --example sr_probe -- W25X20CL 0x1c

use minipro_core::{find_device, DatabasePaths, MiniproHandle};
use std::sync::Arc;

fn show(handle: &MiniproHandle, dev: &minipro_core::device::Device, ft: u8) {
    match handle.protocol.read_fuses(&handle.usb, dev, ft, 8, 1) {
        Ok(b) => println!("fuse {ft:#04x}: {:02x?}", b),
        Err(e) => println!("fuse {ft:#04x}: {e}"),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let part = std::env::args().nth(1).unwrap_or_else(|| "W25X20CL".into());
    let write_byte = std::env::args().nth(2).map(|s| {
        let hex = s.trim_start_matches("0x").trim_start_matches("0X");
        u8::from_str_radix(hex, 16).expect("WRITE_BYTE_HEX must be hex")
    });

    let mut handle = MiniproHandle::open()?;
    let db = DatabasePaths::resolve(None, None, None)?;
    handle.db_paths = Some(DatabasePaths::resolve(None, None, None)?);
    let device = Arc::new(find_device(&db, &part, handle.info.model)?);
    handle.begin_transaction(device)?;
    let dev = handle.device()?.clone();

    println!("== baseline ==");
    for ft in [0x02u8, 0x01, 0x00] {
        show(&handle, &dev, ft);
    }
    let original = handle
        .protocol
        .read_fuses(&handle.usb, &dev, 0x01, 8, 1)
        .ok()
        .and_then(|b| b.first().copied());

    if let Some(v) = write_byte {
        println!("== write fuse 0x01 = {v:#04x} ==");
        handle
            .protocol
            .write_fuses(&handle.usb, &dev, 0x01, 1, 1, &[v])?;
        show(&handle, &dev, 0x01);

        if let Some(orig) = original {
            println!("== restore fuse 0x01 = {orig:#04x} ==");
            handle
                .protocol
                .write_fuses(&handle.usb, &dev, 0x01, 1, 1, &[orig])?;
            show(&handle, &dev, 0x01);
        }
    }

    let _ = handle.end_transaction();
    Ok(())
}
