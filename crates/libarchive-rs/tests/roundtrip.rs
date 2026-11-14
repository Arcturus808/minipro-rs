//! Roundtrip tests: synthetic stored archives, SFX scanning, and the
//! `MiniPro_setup671.exe` specimen (gated on its presence; it is gitignored).

use libarchive_rs::{Archive, Error};

const RAR_SIG: [u8; 7] = *b"Rar!\x1a\x07\x00";

fn crc32(crc: u32, data: &[u8]) -> u32 {
    let mut c = !crc;
    for &b in data {
        let mut x = (c ^ b as u32) & 0xFF;
        for _ in 0..8 {
            x = if x & 1 != 0 {
                0xEDB8_8320 ^ (x >> 1)
            } else {
                x >> 1
            };
        }
        c = x ^ (c >> 8);
    }
    !c
}

fn main_head() -> Vec<u8> {
    // type 0x73, flags 0, size 13, reserved1 0, reserved2 0
    let mut h = vec![0, 0, 0x73, 0, 0, 13, 0, 0, 0, 0, 0, 0, 0];
    let c = (crc32(0, &h[2..]) & 0xffff) as u16;
    h[0..2].copy_from_slice(&c.to_le_bytes());
    h
}

/// One stored-mode file header + body for `name`/`data`.
fn file_head(name: &[u8], data: &[u8], host_os: u8) -> Vec<u8> {
    let file_crc = crc32(0, data);
    let body_size = 25 + name.len();
    let header_size = 7 + body_size;
    let mut h = Vec::with_capacity(header_size);
    h.extend_from_slice(&[0, 0]); // crc
    h.push(0x74); // FILE_HEAD
    h.extend_from_slice(&0x8000u16.to_le_bytes()); // ADD_SIZE flag set (real archives set it)
    h.extend_from_slice(&(header_size as u16).to_le_bytes());
    h.extend_from_slice(&(data.len() as u32).to_le_bytes()); // pack_size
    h.extend_from_slice(&(data.len() as u32).to_le_bytes()); // unp_size
    h.push(host_os);
    h.extend_from_slice(&file_crc.to_le_bytes());
    h.extend_from_slice(&0u32.to_le_bytes()); // file_time
    h.push(29); // unp_ver
    h.push(0x30); // method: store
    h.extend_from_slice(&(name.len() as u16).to_le_bytes());
    h.extend_from_slice(&0x20u32.to_le_bytes()); // attr (win32) / mode (unix)
    h.extend_from_slice(name);
    let c = (crc32(0, &h[2..]) & 0xffff) as u16;
    h[0..2].copy_from_slice(&c.to_le_bytes());
    h.extend_from_slice(data);
    h
}

fn rar_stored(entries: &[(&[u8], &[u8], u8)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&RAR_SIG);
    out.extend_from_slice(&main_head());
    for &(name, data, host) in entries {
        out.extend_from_slice(&file_head(name, data, host));
    }
    out
}

#[test]
fn stored_single_file() {
    let rar = rar_stored(&[(b"test.txt", b"hello world", 2)]);
    let mut ar = Archive::new(&rar[..]);
    let e = ar.next_entry().unwrap().unwrap();
    assert_eq!(e.name, "test.txt");
    assert_eq!(e.size, 11);
    assert_eq!(e.compression_method, 0x30);
    assert_eq!(e.crc32, crc32(0, b"hello world"));
    let data = ar.read_all().unwrap();
    assert_eq!(data, b"hello world");
    assert!(ar.next_entry().unwrap().is_none());
}

#[test]
fn stored_multiple_files() {
    let rar = rar_stored(&[
        (b"a.txt", b"AAA", 2),
        (b"dir/b.txt", b"BBB", 2),
        (b"c.bin", &[0; 257], 3),
    ]);
    let mut ar = Archive::new(&rar[..]);
    let mut names = Vec::new();
    while let Some(e) = ar.next_entry().unwrap() {
        let data = ar.read_all().unwrap();
        names.push((e.name.clone(), data));
    }
    assert_eq!(names.len(), 3);
    assert_eq!(names[0].0, "a.txt");
    assert_eq!(names[0].1, b"AAA");
    assert_eq!(names[1].0, "dir/b.txt");
    assert_eq!(names[1].1, b"BBB");
    assert_eq!(names[2].0, "c.bin");
    assert_eq!(names[2].1, vec![0u8; 257]);
}

#[test]
fn backslash_names_become_slashes() {
    let rar = rar_stored(&[(b"dir\\sub\\f.txt", b"x", 2)]);
    let mut ar = Archive::new(&rar[..]);
    let e = ar.next_entry().unwrap().unwrap();
    assert_eq!(e.name, "dir/sub/f.txt");
}

#[test]
fn sfx_signature_scan() {
    let mut rar = vec![b'M', b'Z'];
    // 16-byte-aligned signature offset like a real SFX stub.
    rar.resize(0x40, 0xCC);
    rar.extend_from_slice(&rar_stored(&[(b"f.txt", b"data", 2)]));
    let mut ar = Archive::new(&rar[..]);
    let e = ar.next_entry().unwrap().unwrap();
    assert_eq!(e.name, "f.txt");
    assert_eq!(ar.read_all().unwrap(), b"data");
}

#[test]
fn garbage_without_signature_fails() {
    let rar = [0xAA; 128];
    let mut ar = Archive::new(&rar[..]);
    assert!(ar.next_entry().is_err());
}

#[test]
fn bad_header_crc_fails() {
    let mut rar = rar_stored(&[(b"test.txt", b"hello", 2)]);
    // Corrupt a byte inside the file header body.
    rar[7 + 13 + 7 + 5] ^= 0xFF;
    let mut ar = Archive::new(&rar[..]);
    assert!(matches!(ar.next_entry(), Err(Error::InvalidHeader(_))));
}

/// Corrupting packed data inside a compressed entry must surface an error —
/// either a decode failure or the file-CRC check at end of stream.
/// (Stored-mode data is not CRC-checked even by libarchive: the
/// `offset_seek >= unp_size` EOF path fires first. See docs/KNOWN-BUGS.md.)
#[test]
fn corrupted_compressed_data_fails() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/MiniPro_setup671.exe");
    let Ok(mut data) = std::fs::read(path) else {
        eprintln!("specimen not present, skipping");
        return;
    };
    // First file header is at 0x3f8b4 (header_size 40); its packed data
    // begins right after. Flip a byte well inside MiniPro.exe's payload.
    let off = 0x3f8b4 + 40 + 1000;
    data[off] ^= 0x5A;
    let mut ar = Archive::new(&data[..]);
    let e = ar.next_entry().unwrap().unwrap();
    assert_eq!(e.name, "MiniPro.exe");
    assert!(ar.read_all().is_err());
}

#[test]
fn truncated_archive_fails() {
    let rar = rar_stored(&[(b"test.txt", b"hello world", 2)]);
    let mut ar = Archive::new(&rar[..rar.len() - 3]);
    ar.next_entry().unwrap();
    assert!(ar.read_all().is_err());
}

#[test]
fn skip_data_advances() {
    let rar = rar_stored(&[(b"one.txt", b"111", 2), (b"two.txt", b"22222", 2)]);
    let mut ar = Archive::new(&rar[..]);
    let e1 = ar.next_entry().unwrap().unwrap();
    assert_eq!(e1.name, "one.txt");
    ar.skip_data().unwrap();
    let e2 = ar.next_entry().unwrap().unwrap();
    assert_eq!(e2.name, "two.txt");
    assert_eq!(ar.read_all().unwrap(), b"22222");
}

/// The development specimen: an installer EXE embedding a RAR4 archive at
/// offset 0x3f400. Skipped silently when the file isn't present.
#[test]
fn specimen_extract_all() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/MiniPro_setup671.exe");
    let Ok(data) = std::fs::read(path) else {
        eprintln!("specimen not present, skipping");
        return;
    };
    let mut ar = Archive::new(&data[..]);
    let mut count = 0;
    let mut first: Option<(String, Vec<u8>, u64)> = None;
    while let Some(e) = ar.next_entry().unwrap() {
        count += 1;
        let mut buf = Vec::new();
        let n = ar.read_data(&mut buf).unwrap();
        if !e.is_symlink {
            assert_eq!(n, e.size, "size mismatch for {}", e.name);
            assert_eq!(
                crc32(0, &buf),
                e.crc32,
                "crc mismatch for {} (method {:#x})",
                e.name,
                e.compression_method
            );
        }
        if first.is_none() && !e.is_directory {
            first = Some((e.name.clone(), buf, e.size));
        }
    }
    assert!(count >= 5, "expected several entries, got {count}");
    let (name, buf, size) = first.expect("no file entry");
    eprintln!("extracted {name}: {} bytes (unp {size})", buf.len());
    assert!(!buf.is_empty());
}

/// Extract `MiniPro.exe` specifically — method 0x33 (LZSS + Huffman +
/// probably E8/E9 filters), the core decompression path.
#[test]
fn specimen_compressed_exe() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/MiniPro_setup671.exe");
    let Ok(data) = std::fs::read(path) else {
        eprintln!("specimen not present, skipping");
        return;
    };
    let mut ar = Archive::new(&data[..]);
    loop {
        let Some(e) = ar.next_entry().unwrap() else {
            panic!("MiniPro.exe not found in archive");
        };
        if e.name == "MiniPro.exe" {
            assert_eq!(e.compression_method, 0x33);
            assert_eq!(e.size, 1_024_000);
            let buf = ar.read_all().unwrap();
            assert_eq!(buf.len() as u64, e.size);
            assert_eq!(crc32(0, &buf), e.crc32);
            assert_eq!(&buf[..2], b"MZ");
            return;
        }
        ar.skip_data().unwrap();
    }
}

/// `Update.dat` is the programmer's firmware blob — an obfuscated
/// container (no magic bytes), so the archive CRC is the integrity check.
#[test]
fn specimen_firmware() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/MiniPro_setup671.exe");
    let Ok(data) = std::fs::read(path) else {
        eprintln!("specimen not present, skipping");
        return;
    };
    let mut ar = Archive::new(&data[..]);
    loop {
        let Some(e) = ar.next_entry().unwrap() else {
            panic!("Update.dat not found in archive");
        };
        if e.name == "Update.dat" {
            assert_eq!(e.size, 312_348);
            let buf = ar.read_all().unwrap();
            assert_eq!(buf.len() as u64, e.size);
            assert_eq!(crc32(0, &buf), e.crc32);
            // Pinned from a CRC-verified extraction.
            assert_eq!(&buf[..4], &[0x52, 0xe0, 0xcc, 0xaa]);
            return;
        }
        ar.skip_data().unwrap();
    }
}
