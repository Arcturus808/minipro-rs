# Developer Guide — libarchive-rs (vendored)

Rules for working inside `crates/libarchive-rs`. The crate is vendored
from the standalone `libarchive-rs` project; decoder internals and bug
history live in `INTERNALS.md` and `docs/KNOWN-BUGS.md`, and the
`xgpro_fetch` integration contract is documented in
`docs/LIBARCHIVE-INTERNALS.md` at the workspace root.

## Critical rules — read first

- The host repo's rules apply (root `AGENTS.md`): branch prefixes,
  `[skip ci]` on every commit, no AI attribution, doc/changelog sync.
- Code changed? Verify locally before committing:
  `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test -p libarchive-rs`, `cargo deny check` (workspace `deny.toml`).
- MSRV is **1.85** (workspace `rust-version`) even though the crate uses
  edition 2024 — edition 2024 stabilized in 1.85, so this is valid; do
  not add language features newer than the 1.85 toolchain supports.

## Project

`libarchive-rs` is a pure-Rust, zero-dependency crate that decodes RAR4
archives and extracts contained files. It must remain strictly derived
from libarchive's cleanroom RAR4 implementation (BSD-2-Clause), which
permits forward licensing under GPL-3.0 — see Licensing below.

Primary consumer: `minipro-core`'s `xgpro_fetch`, which pulls
`update.dat` (TL866A/CS firmware) out of the legacy MiniPro installers —
self-extracting archives whose RAR4 signature sits inside an SFX stub
and must be located by scanning, not assumed at offset 0.

Specimen for development: `MiniPro_setup671.exe` (gitignored — never
commit it or anything extracted from it).

## Licensing

- Crate license: `GPL-3.0-or-later` (see `LICENSE`). All dependencies
  must be GPL-3.0-compatible — enforced by the workspace `cargo-deny`
  config. Keep the crate zero-dependency.
- Do NOT port code from the unrar sources: its license carries a use
  restriction that is GPL-incompatible. Acceptable references are
  libarchive's BSD-2-Clause RAR reader and unarr (LGPLv3).

## Feature internals

Format notes, decoder design, and bug history live in `INTERNALS.md` and
`docs/` (start with `docs/KNOWN-BUGS.md`).
