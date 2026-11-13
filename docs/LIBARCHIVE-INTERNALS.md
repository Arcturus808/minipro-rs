# libarchive-rs — vendored RAR4 decoder

`crates/libarchive-rs` is a vendored, zero-dependency, pure-Rust decoder
for **RAR4** archives, vendored from the standalone `libarchive-rs`
project. It exists to extract `update.dat` (the TL866A/CS firmware blob)
out of the legacy MiniPro installers — the only pinned payloads that are
not RAR5. Decoder internals live in `crates/libarchive-rs/INTERNALS.md`
and `crates/libarchive-rs/docs/KNOWN-BUGS.md`; this file covers how
`xgpro_fetch` integrates it.

## Provenance & licensing

The code is a cleanroom port of libarchive's
`archive_read_support_format_rar.c` (BSD-2-Clause) and
`archive_ppmd7.c` (public domain), relicensed `GPL-3.0-or-later` — same
as the rest of the workspace, so `cargo deny` stays clean. **Unrar
sources were not consulted** (its license is GPL-incompatible); do not
port from them. RAR5 payloads remain handled by `rar-codec` —
libarchive-rs is RAR4 only, same scope as libarchive's own reader
(solid archives and encrypted entries report `Unsupported`).

## Archive formats in the wild

| Payload | Outer container | Inner container | Decoder |
|---------|----------------|-----------------|---------|
| XGPro (T48/T56/TL866II+) | `.rar` RAR5 | RAR5 inside SFX `.exe` | `rar-codec` |
| XGPro (T76) | `.rar` RAR5 | RAR5 inside SFX `.exe` | `rar-codec` |
| MiniPro 6.82 / 6.85 | `.rar` RAR4 | RAR4 inside SFX `.exe` | `libarchive-rs` |

Format is sniffed from the mark version byte: `Rar!\x1a\x07` + `0x00` =
RAR4, `0x01` = RAR5 (`sniff_format`). `find_signature` additionally
scans a blob byte-precisely for embedded marks (SFX stubs).

## Integration points (`xgpro_fetch.rs`)

- `rar4_archive(data)` — builds `libarchive_rs::Archive` over the input
  **sliced at the mark found by `find_signature`**. This matters:
  `Archive`'s own `skip_sfx` (faithful to upstream) only scans
  **16-byte-aligned offsets**, so a mark at an unaligned offset inside
  an SFX stub would not be found. Pre-locating is a deliberate superset
  of upstream behavior — real RAR SFX stubs are aligned, but
  repacked/hand-built stubs need not be.
- `unwrap_rar4(data)` — descends the nested structure: iterate entries;
  a member whose basename is `update.dat` means this level is already
  the inner archive (hand the whole blob back as `Payload::Rar4`); a
  member ending `.exe` is extracted and recursed into. When it can't
  descend further it returns the blob unclassified —
  `find_rar4_member` does the verdict.
- `find_rar4_member(data, name)` — case-insensitive basename member
  extraction, `Ok(None)` when absent (or no RAR4 mark at all). Used by
  `file_role` to classify MiniPro payloads by `update.dat`'s version
  byte (`0x55` = 03.2.85 = MiniPro 6.82, `0x56` = 03.2.86 = 6.85).
- `process_inner_rar4` — spec-driven member extraction (MiniPro specs
  carry no `.alg` files; TL866A/CS have no FPGA).

## Streaming API contract

`Archive::next_entry()` returns entries one at a time; each entry's
data must be consumed (`read_all`/`read_data`) or skipped (`skip_data`)
before advancing — entries cannot be revisited, and `Archive` borrows
the input. Nested descent therefore extracts the `.exe` member fully,
then constructs a new `Archive` over the extracted bytes.

## Tests

- `xgpro_fetch::tests` builds synthetic RAR4 archives in-memory
  (MARK + MAIN_HEAD + stored FILE_HEADs) covering the two-layer
  outer→SFX→inner shape, version classification, and rejection paths.
- `MINIPRO_RAR=<path> cargo test test_minipro_real_installer` runs the
  full classification+extraction path against a real installer (verified
  on both pinned payloads).
- `crates/libarchive-rs`'s own suite includes a specimen-gated test over
  `MiniPro_setup671.exe` (gitignored — never commit it or anything
  extracted from it).
