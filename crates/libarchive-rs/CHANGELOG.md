# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Initial crate scaffold.
- RAR4 reader ported from libarchive's BSD-2-Clause implementation:
  - `Archive::new(data)` — opens an in-memory buffer; scans for the RAR
    signature so self-extracting EXEs work directly.
  - `Archive::next_entry()` — yields `Entry` metadata (name, size, packed
    size, CRC-32, mode, timestamps, symlink target) until `Ok(None)`.
  - `Archive::read_data(w)` / `read_all()` — decompresses stored (`0x30`)
    and normal-compression (`0x31`–`0x35`) entries: LZSS + canonical Huffman,
    PPMd variant-H, and the delta / x86 E8/E9 / RGB / audio VM filters.
    Verifies the entry CRC at end of stream for compressed entries.
  - `Archive::skip_data()` — skips the current entry's packed data,
    following multi-volume continuation headers.
  - Multi-volume split entries: a split file's continuation parts are
    surfaced as repeated entries for the same name, matching libarchive.
- `Entry`/`Timestamp`/`Error` public types.
- `examples/rarex.rs` — lists an archive's entries and optionally extracts
  them (`cargo run --example rarex -- <archive> [outdir]`).
- Copyright notice (`libarchive-rs contributors`) in README and
  `Cargo.toml` `authors`, so downstream derivatives have a notice to
  preserve under GPL §4.

### Changed

- Clarified the public API contract: `next_entry` requires the previous
  entry's data be drained or skipped first, and `read_data` CRC-verifies
  compressed entries only (stored entries pass through unverified).
  README now documents the RAR4-only scope and guidance for downstream
  derivation.
