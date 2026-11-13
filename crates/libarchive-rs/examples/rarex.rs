//! List a RAR4 archive's entries, optionally extracting them.
//!
//! Usage: `cargo run --example rarex -- <archive-or-sfx.exe> [outdir]`
//!
//! With no `outdir`, prints an entry table. With `outdir`, additionally
//! writes each file's decompressed contents beneath it (directories are
//! created; symlinks are reported but not written).

use libarchive_rs::{Archive, Entry};
use std::fs;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("usage: rarex <archive> [outdir]")?;
    let outdir = args.next().map(std::path::PathBuf::from);

    let data = fs::read(&path)?;
    let mut archive = Archive::new(&data[..]);

    println!(
        "{:<4} {:>10} {:>10} {:>4} {:>8}  name",
        "#", "size", "packed", "meth", "crc32"
    );
    let mut i = 0u32;
    while let Some(entry) = archive.next_entry()? {
        println!(
            "{:<4} {:>10} {:>10} {:>4} {:08x}  {}{}",
            i,
            entry.size,
            entry.packed_size,
            format!("{:#04x}", entry.compression_method),
            entry.crc32,
            entry.name,
            describe(&entry),
        );
        if let Some(dir) = &outdir {
            extract(&mut archive, &entry, dir)?;
        } else {
            archive.skip_data()?;
        }
        i += 1;
    }
    println!("{i} entries");
    Ok(())
}

fn describe(e: &Entry) -> String {
    if e.is_directory {
        "  [dir]".into()
    } else if e.is_symlink {
        format!("  -> {}", e.link_target_str().unwrap_or_default())
    } else {
        String::new()
    }
}

fn extract(
    archive: &mut Archive<&[u8]>,
    entry: &Entry,
    outdir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let dest = outdir.join(&entry.name);
    if entry.is_directory {
        fs::create_dir_all(&dest)?;
    } else if entry.is_symlink {
        archive.skip_data()?;
    } else {
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        archive.read_data(&mut fs::File::create(&dest)?)?;
    }
    Ok(())
}
