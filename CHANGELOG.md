# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

## [0.2.2] - 2026-06-12

### Fixed

- **Hex viewer header in dark mode** — header row (Offset, 00-0F, ASCII) now uses theme-aware colors (`bg-surface-100-900`, `border-surface-300-700`) and proper opacity, making it readable in both light and dark themes
- **T76 SPI NOR silent failure** — T76 firmware requires a 128-byte `BEGIN_TRANS` with chip-class geometry in `msg[0x40..0x7f]`. Previously sending only 64 bytes caused SPI-NOR reads to clock out all zeros, READID to return `0x0000`, and erase to be a no-op. Now packs verified read-setup constants for SPI 25-series (protocol_id `0x03` / `0x0f`). Also bumps expected T76 firmware from `0.1.13` to `0.1.17`

## [0.2.1] - 2026-06-11

### Fixed

- **Hex editor column consistency** — when editing in ASCII mode, the hex column now correctly shows the hex value span (highlighted in amber) instead of displaying the ASCII character in the input field. The input field only appears in the column that was clicked (hex or ASCII), making the editing mode visually clear.

---

## [0.2.0] - 2026-06-11

### Added

- **Fuse/lock-bit editor** — interactive Config panel for reading and writing MCU fuses and lock bits with checkbox UI, hex value display, and direct hex input fields
- **Chip ID comparison** — reads chip ID and compares against expected value from database, with clear match/mismatch/no-expected-value messages
- **Package variant warning** — warns user when a package variant (e.g., `@DIP8`) is selected for flash operations, as these often have incorrect protocol configurations
- **Package variant chip ID handling** — skips misleading mismatch warnings for variants and suggests selecting the base device name instead
- **Lock-bit protection safeguards** — warns before read/write when lock bits are active, and detects read-protected chips after read operations
- **"Size diff" option** — `Error` / `Warn` / `Ignore` modes for handling file size mismatches during write operations
- **Auto-verify after write** — automatically verifies chip contents after write unless `Skip verify` is checked
- **Blank check status messages** — explicitly reports whether chip is blank or not blank with address
- **Erase success message** — explicitly confirms chip was erased successfully
- **Hex value display for fuses** — each fuse/lock byte shows its raw hex value next to its name
- **AVR fuse bit inversion** — checkbox state correctly reflects AVR convention where bit=0 means programmed
- **Progress callbacks** — `read_chip_to_bytes` returns `{ base64, stats }` with CRC-32 for hex viewer display
- **In-place hex editor** — click any hex byte to edit in overwrite mode; type hex characters to replace nibbles with automatic overflow to the next byte
- **ASCII editing mode** — click any ASCII character to edit in overwrite mode; type printable characters to set byte values directly with automatic overflow
- **Keyboard navigation** — arrow keys move between bytes, Enter commits, Escape cancels, Backspace resets
- **Edit highlighting** — modified cells shown in yellow in both hex and ASCII columns
- **Column header** — sticky header row showing byte offsets `00`–`0F` for easy column identification
- **Apply/Reset/Save** — toolbar buttons to commit edits to memory, discard pending edits, or write modified buffer to disk

### Fixed

- **Tauri v2 command parameter naming** — fixed `write_fuses` and `save_bytes_to_file` commands where Tauri's auto-camelCase key mapping caused "missing required key" errors
- **`verify_chip` panic** — fixed panic when verifying a file smaller than the device memory (now pads with blank_value, matching `write_chip` behavior)
- **`doReadToBuffer` TypeError** — fixed `TypeError: Cannot read properties of undefined` caused by mismatched return shape from `runOp`
- **Blank check as error** — changed "not blank" result from `[ERROR]` to `[INFO]` since a non-blank chip is a valid state
- **TL866A fuse read/write** — fixed missing `protocol_id` in fuse command messages and corrected `items_count` for multi-byte fuse operations

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

- Logic IC detection (`search_logic_ic`) no longer incorrectly falls back to analogue ICs when no
  exact match is found.  Searching for `74HC00` now correctly returns the `74HC00` NAND gate
  instead of a `TC74HC00AP` optocoupler.
