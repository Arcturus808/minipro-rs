# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

---

## [0.1.2] - 2026-05-21

### Fixed

- **`-r` hangs indefinitely on Windows** — `read_payload_limit()` incorrectly routed any read
  larger than 64 bytes through the dual-EP2+EP3 interleaved path.  For SPI flash chips the
  programmer sends all data on EP2 only, so the code blocked forever waiting for EP3 data that
  never arrived.  Fix: change the single-EP condition from `length < limit` to `length <= limit`,
  and have `read_block()` / `read_jedec_row()` pass the actual read length as both arguments —
  matching the behaviour of the C reference implementation.

---

## [0.1.1] - 2026-05-17

### Fixed

- **TL866A/CS "Response too short" error** — `MiniproHandle::open()` always used the TL866II+
  system-info parser, which expects a 41-byte response. The TL866A/CS returns 40 bytes with a
  different layout (`hardware_version` at byte 6, `device_type` at byte 7). A model-specific
  parser is now selected based on the USB VID/PID detected at open time.

---

## [0.1.0] - 2026-05-16

### Added

- **Core library (`minipro-core`)** — USB device access via [`nusb`](https://crates.io/crates/nusb) (pure Rust, no `libusb` dependency, no C FFI)
- **Protocol support** for XGecu TL866A/CS, TL866II+, T48, T56 (with FPGA bitstream upload), and T76
- **Chip database** — XML-driven device definitions parsed from vendored `infoic.xml` and `logicic.xml`
- **File format support** — Intel HEX, Motorola S-Record, and JEDEC fuse-map read/write with auto-detection
- **Operations** — read, write, erase, verify, blank-check, fuse read/write, logic-IC test, and firmware update
- **CLI binary (`minipro-cli`)** — `clap`-based interface targeting feature parity with upstream C `minipro` 0.7.x
- **Shell completions** for Bash, Zsh, Fish, and PowerShell via `clap_complete`
- **Integration test framework** with `MockUsb` fixture replay (no physical hardware required)
- **CI/CD pipeline** (GitLab CI) with the following stages:
  - `check` — `cargo clippy` and `cargo fmt --check`
  - `test` — `cargo test` on Linux
  - `build` — release binaries for Linux x86_64 and Windows x86_64 (cross-compiled via `x86_64-pc-windows-gnu`)
  - `package` — `.deb`, `.rpm`, `.msi` (Windows installer via `wixl`), and shell completion archive
  - `release` — tag-triggered GitLab release with links to all build artifacts

### Notes

- Windows binaries are fully self-contained — no Cygwin, MSYS2, WSL, Visual C++ Redistributable, or `libusb.dll` required. A one-time [Zadig](https://zadig.akeo.ie/) WinUSB driver association is needed per machine.
- macOS binaries are not produced in CI (no macOS runner available on the free tier). Mac users should build from source with `cargo build --release`. Community contributions for macOS testing are welcome.
- This is an initial release. Not all of the 13,000+ devices in the chip database have been validated against physical hardware.

[0.1.0]: https://gitlab.com/arcturus8081/minipro-rs/-/releases/v0.1.0
