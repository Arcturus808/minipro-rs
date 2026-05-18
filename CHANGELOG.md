# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

---

## [0.1.5] - 2026-05-18

### Added

- **`-o KEY=VALUE` voltage/timing override** — passes raw device-parameter overrides to the
  programmer (e.g. `-o vpp=6.0 -o vcc=3.3`).  Multiple `-o` flags may be combined.
- **Individual long-form override options** — convenience aliases for the most common `-o` keys:
  - `--vpp V` — programming voltage
  - `--vcc V` — logic supply voltage
  - `--vdd V` — additional supply voltage
  - `--pulse US` — programming pulse width (microseconds)
  - `--spi_clock N` (also `--spi-clock`) — SPI clock divisor
  - `--address HEX` — starting address (decimal or `0x`-prefixed hex)
- **`--algorithms PATH`** — override the path to `algorithms.xml` (T56/T76 FPGA bitstream
  descriptions).  Searched via the same four-location fallback chain as `infoic.xml`.
- **`--logicic-out FILE`** — redirect logic IC test result table to a file instead of stdout.
- **`--fuses` / `--uid` / `--lock`** — page-selector shortcuts equivalent to
  `--page config` / `--page user` / `--page config` respectively; mirrors the upstream
  `minipro` 0.7.x interface.
- **Short flag aliases** — added the following short options present in upstream but previously
  missing: `-T` (logic-test), `-x` (no-size-error), `-y` (no-size-warning), `-P` (no-pin-check),
  `-u` (unsafe), `-f` (fill), `-F` (format), `-a` (start-address).
- **Progress callbacks for read/write/verify** — `read_chip`, `write_chip`, and `verify_chip` now
  accept an `Option<&mut dyn FnMut(usize, usize)>` progress callback invoked after each block
  with `(bytes_done, total_bytes)`.  Pass `None` for the original behaviour.  The CLI wires this
  to `indicatif` progress bars; a Tauri front-end can wire it to `window.emit("progress", …)`
  without any changes to `minipro-core`.
- **`OpStats` return value** — `read_chip` and `write_chip` now return `Result<OpStats>` instead
  of `Result<()>`.  `OpStats` carries `bytes: usize` and `crc32: u32` (CRC-32/ISO-HDLC of the
  data buffer).  The CLI prints these after each operation, e.g.
  `Saved "dump.bin"  (262144 bytes, CRC-32: 0xc56d40d3)`.

---

## [0.1.4] - 2026-05-17

### Fixed

- **TL866A write truncated to 64 bytes** — `write_block` was calling `msg_send` (which caps the
  transfer at 64 bytes) instead of `msg_send_large`.  For a 256-byte page this left 199 bytes of
  data unsent, causing the firmware to stall waiting for the remainder and hanging every subsequent
  operation.  Fix: route the full payload (7-byte header + page data) through `msg_send_large`.

---

## [0.1.3] - 2026-05-17

### Added

- **`--icsp` flag** — wired through the `Protocol` trait (`begin_transaction` now takes
  `icsp: bool`); TL866A sets byte 11, TL866II+/T48 sets byte 3 of the begin-transaction packet.
- **`--verbose` / `-v` flag** — controls the default `env_logger` level (`info` with the flag,
  `warn` without).  `MINIPRO_LOG` / `RUST_LOG` override as usual.
- **`info!` logging** — programmer model + firmware version, device name, chip-ID result,
  and byte counts for read/write/verify are logged at `INFO` level.

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
[0.1.1]: https://gitlab.com/arcturus8081/minipro-rs/-/releases/v0.1.1
[0.1.2]: https://gitlab.com/arcturus8081/minipro-rs/-/releases/v0.1.2
[0.1.3]: https://gitlab.com/arcturus8081/minipro-rs/-/releases/v0.1.3
[0.1.4]: https://gitlab.com/arcturus8081/minipro-rs/-/releases/v0.1.4
[0.1.5]: https://gitlab.com/arcturus8081/minipro-rs/-/releases/v0.1.5
