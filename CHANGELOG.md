# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

### Added

- **Algorithm XML parser for T56/T76** — parses `algorithm.xml` to load FPGA bitstream algorithms required by T56/T76 programmers. Computes algorithm names from `protocol_id` + `variant` (with special cases for ATmega ICSP, AT89C ICSP, eMMC voltage, reversed packages, and logic chips), base64-decodes and gunzips bitstreams, verifies CRC32, and performs T76 level-2 zero-run decompression. Integrated into `MiniproHandle::begin_transaction` — automatically looks up the algorithm when a T56/T76 device needs one
- **T56 firmware update** — ported from C `minipro` `t56_firmware_update()`. Handles file version/CRC validation, bootloader magic switch, erase, block-by-block reflash (0x814-byte blocks), and reset back to normal mode. Routed through `operations::firmware_update()`
- **eMMC partition selection for T76** — `T76_EMMC_PARTITION` env var selects eMMC hardware partitions: `user` (default), `boot1`, `boot2`, `rpmb`. Uses CMD6 SWITCH to set EXT_CSD[179] PARTITION_CONFIG. Capacity detection uses the correct EXT_CSD field per partition: SEC_COUNT[212] for USER, BOOT_SIZE_MULT[226] for BOOT, RPMB_SIZE_MULT[168] for RPMB
- **T76 OVC status for NAND/eMMC** — `Protocol::get_ovc_status` now takes `&Device` parameter. T76 implementation repacks chip-parameter header (protocol_id, variant, voltages, chip_info, pin_map) into the 0x39 status request for NAND/eMMC, mirroring vendor behavior. A zeroed 0x39 deselects the NAND; the repacked header keeps it selected. OVC checks enabled in `begin_transaction` and `check_ovc`

## [0.4.1] - 2026-06-27

### Fixed

- **Programmer serial number display** — control characters (e.g., ETX `0x03`) were not stripped from the serial number and device code strings returned by the programmer. Only null bytes were trimmed, but some programmers pad unused bytes with other control characters. Now all ASCII control characters (0x00–0x1F, 0x7F) are stripped. Affects both CLI and GUI display

## [0.4.0] - 2026-06-27

### Added

- **Smart firmware diff** — CLI `minipro --diff fileA fileB [--erase-value 0xFF]` compares two firmware files with byte-aligned diff and three-way tail classification (padding vs anomalous). Exit code 0 on match, 1 on mismatch. GUI hex viewer "Compare" button highlights differing bytes in red, padding in gray, anomalous tail in amber, with next/prev navigation (F3/Shift+F3) and anomalous-tail warning banner
- **Batch / queue operations** — CLI `minipro -p DEVICE -w file.bin --batch [N]` programs multiple identical chips with the same firmware. After each successful write + verify, prompts to insert the next chip. Optional count limits the run. GUI "Batch Mode" toggle in write panel with "Next Chip" / "Retry" / "Stop Batch" buttons and live pass/fail counter. Architecture includes buffer patching hook for future auto-incrementing serial number injection
- **Auto-incrementing serial number injection** — patch a unique serial into each chip during batch programming. CLI: `--serial-start 1 --serial-addr 0x1FF0 --serial-width 4 --serial-format bin --serial-endian little --serial-step 1 --serial-checksum none`. Supports binary (little/big endian), ASCII (zero-padded decimal), and BCD formats. Optional XOR or CRC-8 checksum. GUI: collapsible "Serial Number" section in batch options with live preview showing serial range. Verify checks against the patched buffer, not the original file
- **Serial overflow detection** — `patch_serial()` errors if the value exceeds the width's max (e.g., 0xFFFF for 2-byte) instead of silently truncating. CLI checks before batch start. GUI shows live red warning and blocks start
- **Manual trim/pad to size** — "Trim/Pad" button in hex viewer toolbar. Trim removes trailing fill bytes; Pad extends to a target size. Fill byte dropdown supports 0xFF (NOR flash) and 0x00 (EEPROM/NAND)
- **USB reconnect hints** — connection button tooltip advises replugging on USB errors. Operation and batch error messages detect USB-related failures and append replug advice. Helps with Windows USB Selective Suspend, Linux USB autosuspend, and macOS sleep power management
- **T76 eMMC EXT_CSD capacity auto-detection** — eMMC database entries have `code_memory_size=0x200` (placeholder). Real capacity is now detected at runtime from EXT_CSD (`SEC_COUNT[212] * 512`). `T76_EMMC_SIZE_MB` env var overrides detection (MiB). New `Protocol::effective_code_size()` trait method returns detected capacity for eMMC; operations layer and GUI use it for read/write/verify/erase size calculations
- **Live device search** — device selector now searches as you type with 200ms debounce. Race-condition guard discards stale responses. 2-character minimum query length. Replaces the old ComboSearch dropdown (search history + query favorites) with a cleaner single-input UX
- **Device favorites** — star icon on each search result toggles favorite status (persisted to localStorage). Favorites are pinned in a collapsible section at the top of the results panel, always visible regardless of the current search query. Collapse state is persisted. Clicking the star does not select the device — star and selection are independent actions
- **CI version consistency check** — GitHub Actions release workflow now verifies all four version fields (root Cargo.toml, gui Cargo.toml, tauri.conf.json, package.json) match the git tag before any builds start. Fails fast with clear error if any file is out of sync

### Fixed

- **T76/T56 overcurrent safety check** — `begin_transaction()` now polls `get_ovc_status()` after FPGA initialization and aborts on overcurrent before any chip operation begins. NAND and eMMC skip this (zeroed 0x39 deselects them). Applies to all programmer models
- **T76/T56 bitstream upload caching** — FPGA bitstream (~775KB) is now uploaded only once per session instead of on every `begin_transaction` call. For batch operations programming N chips, this eliminates N-1 redundant uploads (1500+ USB packets each). T76 also skips NAND/eMMC adapter init on subsequent calls
- **T76 eMMC bring-up queries** — added ID query drain (0x21/CID, 0x05/READID, 0x06/user-id) before CMD6 SWITCH partition select. Without these, firmware responses sit in the USB buffer and desync the next transfer. Matches Matt Brown's t76-improvements branch
- **T76 `effective_code_size` truncation** — trait method now returns `u64` instead of `u32`, preventing capacity truncation for eMMC chips larger than 4GiB
- **T76 `self.device` state on OVC failure** — device was set before the overcurrent check, leaving incorrect state on abort. Now set only after the check passes
- **T76 bitstream flag set without upload** — `bitstream_uploaded` flag was set even when the upload was skipped. Now only set after a successful upload
- **Hex viewer header crowding** — header metadata split into two rows to prevent wrapping when the toolbar grows. "Erase" label relabeled to "Fill byte" with descriptive options. Fill byte dropdown moved before the Trim/Pad actions

## [0.3.0] - 2026-06-24

### Added

- **TL866A/CS firmware update** — CLI `minipro -F update.dat` and GUI Diagnostics panel now support flashing stock firmware on TL866A and TL866CS programmers. The `update.dat` file is decrypted (simple XOR cipher), CRC-verified, and flashed via the built-in bootloader. GUI includes confirmation dialog and progress bar
- **TL866A/CS logic IC test** — `-T` / `--logic-test` now works on TL866A and TL866CS using the bit-bang algorithm (commands 0xD0–0xD5) from upstream `minipro`. Validated with 7408 (MM74HC08N) on real hardware
- **GUI Firmware Update button** — new "Firmware Update" button in the Programmer Diagnostics panel, with a file picker for `update.dat` / `UpdateII.dat` / `updateT76.dat`. Includes confirmation dialog warning before flashing. T56 is not supported (firmware update protocol not yet reverse-engineered)
- **GUI firmware update progress** — progress bar shows block count during firmware flashing, with status messages in the terminal log

### Fixed

- **XML attribute parsing** — `get_attr_u32` no longer interprets decimal values as hex. This fixes logic IC `pins="14"` being parsed as 20 (0x14), which caused VCC/GND misconfiguration and logic test failures
- **Logic test header alignment** — pin number column headers now align with data rows. Rust format specifier `{:-3}` was incorrectly used instead of `{:<3}` for left-alignment, causing right-aligned pin numbers and a 2-character offset
- **GUI terminal whitespace preservation** — terminal output now uses `<pre>` with `white-space: pre` and builds content as a single HTML string to prevent Svelte template whitespace from leaking into rendered output
- **GUI terminal ANSI color rendering** — ANSI escape codes (`\x1b[0;91m` for red, `\x1b[0m` for reset) are now converted to inline HTML `<span>` tags so error markers appear in color instead of showing raw escape codes
- **TL866A firmware version display** — diagnostics panel and CLI now show the correct format (e.g., "2.72" instead of "00.2.72")

## [0.2.7] - 2026-06-16

### Added

- **Chip ID verification** — automatic chip ID read and comparison before read/write/erase/verify. Fails with clear mismatch message if inserted chip doesn't match selected device. `--skip-device-id` / `-S` CLI flag and GUI "Chip ID check" checkbox to bypass
- **OSCCAL calibration preservation** — for PIC microcontrollers with `osccal_save=1` (e.g., PIC12F509, PIC12F683), the factory RC oscillator calibration word is automatically read before erase and restored afterward, preventing clock accuracy loss
- **Calibration page read** — CLI `-c calibration` now reads the chip's calibration bytes instead of erroring
- **Persistent Config panel** — auto-populates fuse/lock fields from database defaults when a device is selected. Fields are editable immediately without requiring a chip read first. "Read Config from Chip" merges actual chip values into the existing panel state
- **Side-by-side fuse/lock layout** — Fuses and Lock Bits cards are now displayed horizontally next to each other in the Config panel
- **No-chip-ID warning** — yellow banner in Read/Write/Verify panels when the selected device does not support chip ID verification, reminding the user to verify the correct chip is inserted
- **Manufacturer column** — each device search result now shows the manufacturer name (from `infoic.xml`) on the right side of the list, making it easier to distinguish between devices with similar part numbers from different vendors

### Fixed

- **Chip ID byte-order normalization** — devices with multi-byte JEDEC IDs (e.g., SPI flash chips like PM25LV010) could fail chip ID verification because different programmer protocols pack bytes at different positions. Now the most significant non-zero byte is left-aligned before comparison, matching the database value regardless of protocol-specific packing

## [0.2.6] - 2026-06-15

### Added

- **Skip blank pages** — new `--skip-blank` / `-B` CLI flag and GUI checkbox. During write, pages that are all blank (0xFF) are skipped, reducing write time and flash wear
- **GUI voltage overrides** — collapsible Advanced section in the Write operation panel with dropdowns for VPP, VCC, and VDD. Shows chip defaults from infoic.xml. Includes "Reset voltages" button
- **Version badge** — `v0.2.6` shown in the app header next to the MINIPRO-RS title, reading from package.json at build time

### Fixed

- **Voltage display in GUI** — DeviceSelector footer and Advanced dropdowns now show actual voltage values (e.g., 9.0V, 5.0V) instead of raw index codes (0–15)

### Changed

- **GUI installer filenames** — Tauri productName changed to `MINIPRO-RS-GUI` so installer files (`.msi`, `.exe`) clearly indicate they are GUI builds
- **Write options layout** — reorganized into two rows: Format/Page/Size diff dropdowns on row 1, Skip erase/verify/blank checkboxes + Advanced toggle on row 2

## [0.2.5] - 2026-06-15

### Added

- **T76 NAND support** (protocol 0x2d) — read, erase, and program for parallel NAND and SPI-NAND. Includes adapter power/init sequence, 64-byte FPGA prelude with page/block geometry, per-erase-block read (0x0D), per-page program with 0x39 commit (0x1F), and per-block erase with 0x3A bad-block skip
- **T76 eMMC support** (protocol 0x31) — read, erase, and program via 0x27 command tunnel (timing, partition switch, program setup, status poll). Supports 64 KiB block streaming on EP82/EP05. Defaults to USER partition
- **T76 parallel NOR support** (protocols 0x12/0x14) — x16 family BEGIN extension with vendor packer sub_4b5a70 equivalent. READ and ERASE verified against upstream; PROGRAM deferred

### Fixed

- **T76 SPI NOR 8-pin/16-pin geometry** — the 128-byte BEGIN_TRANS now branches on `variant >> 8` (0x11 = 8-pin, 0x21 = 16-pin) with correct read-setup word pairs for each package type
- **T76 bitstream END for NAND** — last-block size is now sent in the END packet, fixing NAND FPGA finalization (without it READID returned 0xFF)
- **NAND/eMMC OVC skip** — per-block `get_ovc_status` poll is now skipped for NAND and eMMC chips; a zeroed 0x39 deselects these devices
- **eMMC block size and addressing** — `effective_block_size` returns 64 KiB for eMMC; read/write init uses LBA (sectors) and total block count matching firmware expectations

### Changed

- **Protocol trait** — `read_block`, `write_block`, and `erase` now take `&Device` so implementations can branch on `protocol_id` and `chip_type`
- **Erase-block-aware I/O** — `operations.rs` now uses `effective_block_size()` for all chip operations (read, write, verify, blank-check), correctly handling NAND erase-block size and eMMC 64 KiB blocks

## [0.2.4] - 2026-06-13

### Fixed

- **Hex viewer row shift on edit** — selecting a byte for editing no longer causes the row to shift downward. The `<input>` element replacing the `<span>` had a visible border and different default `vertical-align`, causing misalignment. Fixed by removing the border and normalizing both elements to `display: inline-block` + `vertical-align: middle`

## [0.2.3] - 2026-06-12

### Added

- **Write chip directly from hex viewer buffer** — when Write is selected and the hex viewer contains data, the start button splits into "Write from Hex Buffer" (primary) and "Write from File" (secondary). The hex buffer path auto-applies pending edits, respects size-diff settings, and supports skip erase/verify options

### Fixed

- **Hex viewer arrow key navigation** — ArrowLeft/Right/Up/Down now correctly move the selection after `commitEdit()` sets `editingOffset` to `null`. Previously ArrowRight always jumped to offset 1 and ArrowDown to offset 16 due to JavaScript `null + n` coercion
- **Hex viewer auto-scroll on navigation** — arrow keys now scroll the viewport to keep the selected byte visible when navigating outside the current view

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
- **`--algorithms PATH`** — override the path to `algorithm.xml` (T56/T76 FPGA bitstream
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
