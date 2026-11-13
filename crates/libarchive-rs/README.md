# libarchive-rs

A pure-Rust crate that decodes RAR4 archives and extracts contained files.

The primary use case is pulling resources (firmware, etc.) out of installer
executables that embed RAR4 data — i.e. self-extracting archives — so the
archive signature is located by scanning, not assumed to sit at offset 0.

## Provenance and licensing

This crate is a clean-room Rust derivation of libarchive's RAR4 reader
(`libarchive/archive_read_support_format_rar.c`, BSD-2-Clause) together with
its bundled PPMd variant-H decoder (`libarchive/archive_ppmd7.c`, public
domain by Igor Pavlov, based on work by Dmitry Shkarin). No code from the
`unrar` sources was consulted or ported — the unrar license carries a use
restriction that is GPL-incompatible.

The crate is licensed under `GPL-3.0-or-later`. See `LICENSE`.

Copyright (C) 2026 libarchive-rs contributors. Portions derived from
libarchive remain under their original BSD-2-Clause and public-domain
terms — see the provenance note above.

## Features

- Scan arbitrary input for the RAR4 signature (SFX/embedded archives)
- Parse mark, main, file, and end-of-archive headers with CRC validation
- Extract entries compressed with methods 0x30–0x35 (store through "best")
- LZSS + Huffman decoding, RAR VM filters (delta, x86 E8/E9, RGB, audio),
  and PPMd variant-H decompression
- Multi-volume (split file data) continuation

## Example

```rust
use libarchive_rs::Archive;

let data = std::fs::read("setup.exe")?;
let mut archive = Archive::new(&data[..]);
while let Some(entry) = archive.next_entry()? {
    if !(entry.is_directory || entry.is_symlink) {
        let mut out = Vec::new();
        archive.read_data(&mut out)?;
        // out now holds the decompressed file; CRC verified
        assert_eq!(out.len() as u64, entry.size);
    }
}
# Ok::<(), libarchive_rs::Error>(())
```

## Status

Early development. Only RAR4 is read — RAR5+ uses a different signature
and yields `Error::SignatureNotFound`. Solid archives and encrypted
entries are reported as `Error::Unsupported`, matching libarchive's
reader.

## Deriving your own crate

The module map in `INTERNALS.md` and the behavioral quirks in
`docs/KNOWN-BUGS.md` are the starting points; each `src/` file is
doc-commented on its own.

Licensing: this crate is `GPL-3.0-or-later`, so anything derived from it
must carry a GPL-3.0-compatible license. If you need a permissive license
instead, port directly from the original sources — libarchive's RAR
reader is BSD-2-Clause and its bundled PPMd decoder is public domain —
this crate's structure maps closely onto them.
