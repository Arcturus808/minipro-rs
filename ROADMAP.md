# Roadmap

This is a living list of features and improvements planned for minipro-rs.

## Completed

- [x] GUI front-end (Tauri + Svelte)
- [x] Dynamic window sizing based on monitor
- [x] Persistent window size and panel widths
- [x] Percentage-based draggable panel splitters
- [x] Force reconnect for stale USB handles
- [x] Improved programmer error diagnostics
- [x] Colorblind-friendly programmer status badge
- [x] Logic test for logic ICs
- [x] ICSP mode toggle (ZIF / ICSP / ICSP no VCC)
- [x] Operation duration in terminal output
- [x] CRC-32 display in hex viewer
- [x] Expanded device info panel (package, memory, flags)
- [x] Fuse/lock-bit editor for MCUs (read + write)
- [x] Dangerous fuse warnings (RSTDISBL, SPIEN, etc.)
- [x] Hex input fields for direct fuse/lock byte editing
- [x] Chip ID comparison with expected value from database
- [x] Package variant detection and chip ID mismatch handling
- [x] Warning when package variant selected for flash operations
- [x] Blank check result messages (blank vs not-blank with address)
- [x] Lock-bit protection safeguards (pre-read / pre-write warnings)
- [x] Auto-verify after write (with file-size padding support)
- [x] "Size diff" option for handling file/device size mismatches (Error / Warn / Ignore)
- [x] **Write chip directly from hex viewer buffer** — when hex viewer has data, Write operation shows "Write from Hex Buffer" + "Write from File" buttons
- [x] Hex viewer: keyboard arrow key navigation (Left/Right/Up/Down)
- [x] Hex viewer: auto-scroll to keep selected byte visible during navigation
- [x] Hex viewer: go-to-offset navigation (Ctrl+G)
- [x] File format support: Intel HEX, SREC, JEDEC — backend parsers, CLI, and GUI all supported. Hex viewer Save dialog with auto-detection from extension.
- [x] **Skip blank pages** — CLI `--skip-blank` / `-B` flag and GUI checkbox. Skips writing pages that are all blank (0xFF), reducing flash wear and write time.
- [x] **GUI voltage overrides** — collapsible Advanced section in Write panel with VPP, VCC, VDD dropdowns. Shows chip defaults from infoic.xml. Includes "Reset voltages" button.
- [x] **Version badge in app header** — shows `v{x.y.z}` next to MINIPRO-RS title, reading from package.json at build time.
- [x] **Chip ID verification before operations** — automatic chip ID read and comparison before read/write/erase/verify. Fails with clear mismatch message. `-x` / `--skip-id` CLI flag to skip in read mode (rejected for write/erase, matching upstream); `-y` / `--continue-id` to warn but continue on mismatch; GUI "Chip ID check" checkbox to bypass.
- [x] **No-chip-ID warning** — yellow banner in Read/Write/Verify panels when selected device lacks chip ID support, reminding user to verify correct chip insertion.
- [x] **OSCCAL calibration preservation** — for PIC microcontrollers with `osccal_save=1` (e.g., PIC12F509, PIC12F683), the factory RC oscillator calibration word is automatically saved before erase and restored afterward.
- [x] **Calibration page read** — CLI `-c calibration` now reads the chip's calibration bytes instead of erroring.
- [x] **Persistent Config panel** — auto-populates fuse/lock fields from database defaults when a device is selected. Fields are editable immediately without requiring a chip read first. "Read Config from Chip" merges actual chip values into the panel.
- [x] **Side-by-side fuse/lock layout** — Fuses and Lock Bits cards displayed horizontally next to each other in the Config panel.
- [x] **Manufacturer column in search results** — each device search result shows the manufacturer name parsed from `infoic.xml`, making it easy to distinguish similar part numbers from different vendors.
- [x] **Chip ID byte-order normalization** — fixes false mismatch errors on devices (e.g., SPI flash like PM25LV010) where different programmer protocols pack JEDEC ID bytes at different positions in the response word.
- [x] **Smart firmware diff** — byte-aligned comparison with three-way tail classification (padding vs anomalous). CLI `--diff fileA fileB`, GUI "Compare" button with four-state cell highlighting, next/prev navigation (F3), and anomalous-tail warning banner. Configurable erase value. See detailed spec below in Backlog.
- [x] **Batch / queue operations** — CLI `--batch [N]` and GUI "Batch Mode" toggle for programming multiple identical chips. Same device, same file, repeated writes with verify. Architecture includes buffer patching hook for serial number injection. See detailed spec below in Near-term.
- [x] **Auto-incrementing serial number injection** — CLI `--serial-*` flags and GUI "Serial Number" section for patching unique serials during batch programming. Supports bin/ascii/bcd formats, little/big endian, optional XOR/CRC-8 checksum, configurable step. Verify checks against patched buffer. See detailed spec below in Near-term.
- [x] **Serial overflow detection** — `patch_serial()` checks if the value exceeds the width's max and returns an error instead of silently truncating. CLI checks before batch start. GUI shows live warning and blocks start.
- [x] **Manual trim/pad to size** — "Trim/Pad" button in hex viewer toolbar. Trim removes trailing fill bytes; Pad extends to a target size. Fill byte dropdown supports 0xFF (NOR flash) and 0x00 (EEPROM/NAND).
- [x] **USB transfer timeout** — 5-second timeout on USB transfers in `usb.rs` prevents indefinite hangs when the programmer is unresponsive. Applies to both CLI and GUI.
- [x] **Hex editor standard hotkeys** — Ctrl+S (save), Ctrl+C/V (copy/paste bytes), Ctrl+A (select all), Ctrl+Z/Shift+Z/Y (undo/redo), Ctrl+Home/End (jump to start/end), Tab (switch hex/ASCII panes). Copy uses Tauri clipboard plugin to avoid WebView2 permission prompts.
- [x] **Hex editor find/search** — Ctrl+F opens find dialog with hex and ASCII search modes. Match highlighting (light blue for all matches, dark blue for current). F3/Shift+F3 navigates matches. Context-sensitive F3: navigates whichever mode (find or diff) was most recently activated.
- [x] **Pending edits overwrite protection** — Read and Open operations prompt before replacing the hex buffer if pending edits or unsaved applied changes exist. Svelte-based confirm modal (not native dialog) to avoid WebView2 JS event loop freeze. `bufferDirty` flag tracks applied-but-unsaved changes (set by Apply/Trim/Pad, cleared by Save/load).
- [x] **Entropy indicator in hex viewer** — per-row Shannon entropy bar in the gutter between offset and hex columns. Green=uniform, red=high entropy. Toggle in Settings (off by default).
- [x] **Hex viewer help overlay** — ?/F1 key or "i" toolbar icon opens a modal with keyboard shortcuts and feature descriptions. Same pattern reusable for other panels.
- [x] **Config/fuses help overlay** — "i" icon in config panel opens a modal explaining fuse basics, dangerous fuses (RSTDISBL, SPIEN, JTAGEN, DWEN), and lock bits. Global Escape listener closes the modal.
- [x] **Config panel stale fuses fix** — switching devices no longer shows the previous device's fuse names. Changed `$effect` to `$effect.pre` so configData refreshes before DOM re-render.
- [x] **Favorites show manufacturer** — device favorites in the search panel now display the manufacturer alongside the device name. Old favorites auto-migrate to the new format.
- [x] **Hex edit blur fix** — clicking outside the hex viewer (e.g., the device search field) now properly commits the active edit and releases keyboard focus.

## Near-term

- [x] **Batch / queue operations** — program multiple identical chips with the same firmware image
  - **Scope (initial):** same device, same file, repeated writes with verify. Covers 90%+ of batch use cases (classroom sets, bootloader burning, small production runs).
  - **Architecture designed for serial injection:** the batch loop includes a "patch buffer before write" hook where auto-incrementing serial numbers will plug in later, without restructuring the core logic.
  - **Implementation plan:**
    1. **CLI batch mode** — `minipro -p DEVICE -w file.bin --batch [--count N]`
       - Writes firmware, verifies, prints "Chip 1/N: PASS", waits for keypress (Enter to continue, Ctrl+C to abort)
       - If `--count` omitted, runs indefinitely until user aborts
       - Prints summary at end: total programmed, passes, failures
       - Core logic in `minipro-core::operations::batch_write` — reusable by GUI
       - `batch_write` takes a callback for: progress reporting, "ready for next chip" prompt, and buffer patching hook (for serial injection)
    2. **GUI batch mode** — "Batch Mode" toggle in operations panel
       - When enabled, Start button becomes "Start Batch"
       - After each successful write+verify, shows "Next Chip" button and progress counter ("3/50 completed")
       - Batch summary panel: pass/fail count, elapsed time, export log option
       - Reuses `batch_write` from `minipro-core` via Tauri command
    3. **Serial number injection (implemented):**
       - `--serial-start 1 --serial-addr 0x1FF0 --serial-width 4 [--serial-format bin|ascii|bcd]`
       - Patches buffer at target address before each write, increments after each successful write
       - GUI: collapsible "Serial Number" section in batch options
       - Device-specific: user specifies address manually (different chips store serials in different locations)
       - May include checksum byte option
       - Implemented as the "patch buffer before write" hook in `batch_write`
  - **Design decisions:**
    - Batch without serial numbers first: useful on its own, simpler to validate
    - Serial injection as optional layer: adds device-specific complexity (address, format, endianness, checksums) — better as a separate iteration
    - CLI first, then GUI: CLI is a linear loop with no UI paradigm change; GUI needs batch state management and "Next Chip" flow
    - Same device + same file only (initial): different devices/files is a production-line scenario, rare for hobbyist users
  - Status: CLI and GUI batch mode implemented. Serial number injection implemented (see below).

- [x] **Auto-incrementing serial number injection** — patch a unique serial into each chip during batch programming
  - **Problem:** Embedded products need unique serial numbers stored at a known address in flash/EEPROM. Without automation, the user must manually edit the firmware file between each chip — tedious and error-prone.
  - **Use case:** Manufacturer programming 1000 identical boards. Each chip gets the same firmware but a different serial number at a fixed address (e.g., `0x1FF0`).
  - **Architecture:** Plugs into the existing `on_patch_buffer` hook in `batch_write`. The buffer is re-read from the file before each chip, so the patch is always applied to a fresh copy — no need to undo the previous serial.
  - **Configuration:**
    - `--serial-start <VALUE>` — starting serial number (hex or decimal, e.g., `0x0001` or `1`)
    - `--serial-addr <OFFSET>` — target address in the chip's memory (hex, e.g., `0x1FF0`)
    - `--serial-width <N>` — byte width: 1, 2, 4, or 8 (default: 4)
    - `--serial-format <FORMAT>` — `bin` (raw binary), `ascii` (zero-padded decimal string), `bcd` (binary-coded decimal). Default: `bin`
    - `--serial-endian <ENDIAN>` — `little` or `big` (default: `little`). Only applies to `bin` format.
    - `--serial-step <N>` — increment per chip (default: 1). Allows skipping numbers (e.g., step=10 for batch-labeled units).
    - `--serial-checksum <TYPE>` — optional: `crc8`, `xor`, or `none` (default: `none`). Appends a checksum byte after the serial.
  - **CLI usage:**
    ```
    minipro -p AT28C256 -w firmware.bin --batch 50 \
      --serial-start 0x0001 --serial-addr 0x1FF0 --serial-width 4 \
      --serial-format bin --serial-endian little
    ```
    Output per chip: `Chip 1/50: PASS (serial 0x0001)`
  - **GUI usage:**
    - Collapsible "Serial Number" section in the batch options panel (only visible when Batch Mode is on)
    - Fields: Start, Address, Width (dropdown), Format (dropdown), Endian (dropdown), Step
    - Preview: shows "Chip 1: 0x0001 → bytes 01 00 00 00 at 0x1FF0" so user can verify before starting
    - Per-chip log shows the serial that was written
  - **Implementation plan:**
    1. **Core: `SerialConfig` struct and `patch_serial()` function in `minipro-core::batch`**
       - `SerialConfig` holds start, addr, width, format, endian, step, checksum type
       - `patch_serial(buf: &mut [u8], config: &SerialConfig, chip_number: usize)` writes the serial bytes at the configured address
       - Serial value for chip N = `start + (N-1) * step`
       - Format conversions:
         - `bin`: write value as N bytes in selected endianness
         - `ascii`: format as zero-padded decimal string (width = number of digits, not bytes), null-terminated
         - `bcd`: pack each decimal digit as 4-bit nibble
       - Checksum (if enabled): compute over serial bytes, append at `addr + width`
       - Bounds check: error if `addr + width + checksum_len > buf.len()`
    2. **CLI: add `--serial-*` flags, wire into `on_patch_buffer` callback**
       - Validate serial config before starting batch
       - Print serial value in per-chip output
    3. **GUI: add `serialConfig` parameter to `do_batch_write_chip` Tauri command**
       - Takes optional `SerialConfigDto` as additional parameter
       - When present, backend reads file into buffer, patches with `patch_serial()`, then uses `write_chip_bytes` + `verify_chip_bytes` (not file-based versions)
       - Keeps serial logic in Rust, testable, consistent with CLI
    4. **GUI: Serial Number section in batch panel**
       - Collapsible section, only visible when Batch Mode is on
       - 3-column field layout: Address | Start | Step / Format | Width | Endian / Checksum
       - Live preview shows serial range: "Chip 1 of 10: serial 1 → 10, 4 bytes at 0x1FF0" (bounded) or "Chip 1 (unlimited): serial 1, 2, 3, ..." (unlimited)
       - Validation: rejects empty address or invalid start value before starting batch
    5. **Tests: unit tests for `patch_serial()`**
       - Binary little-endian, binary big-endian, ASCII, BCD
       - Checksum types (crc8, xor)
       - Bounds checking (address out of range)
       - Multi-chip sequence (verify increment + step)
  - **Design decisions:**
    - Serial config is optional — batch works without it (already implemented)
    - Address is user-specified, not database-driven: different products store serials at different locations, even on the same chip type. No reliable way to auto-detect.
    - ASCII format uses decimal, not hex: matches typical product labeling (SN00001, not SN0x0001)
    - Checksum is optional and simple: CRC8 or XOR covers most use cases without over-engineering
    - Serial increments on chip number, not on success: if a chip fails and is retried, it gets the same serial (not the next one). This prevents serial gaps from failed chips.
    - GUI defaults: decimal start (1), empty address (required field), to match typical user expectations
  - **Edge cases handled:**
    - Address + width beyond buffer: `patch_serial()` validates and errors before writing
    - ASCII format with width > buffer space at address: caught by bounds validation
    - Verify after write: uses `verify_chip_bytes` against the patched buffer, not the original file
    - Serial overflow: if `start + (N-1) * step` exceeds the width's max value (e.g., 0xFFFF for 2-byte), `patch_serial()` returns an error. CLI checks via `check_overflow()` before starting the batch. GUI shows a red warning in the serial panel and blocks batch start.
  - **Edge cases not yet handled:**
    - None known.
  - Status: Implemented. Core `patch_serial()` with 18 unit tests, CLI `--serial-*` flags, GUI Serial Number section with live preview and validation.

## Backlog

- [ ] **Protocol parity with original minipro + Matt Brown's t76 branch**

  The README states "Full feature parity with the C minipro 0.7.x" as a goal.
  This section tracks every known gap against that goal. Gaps are organized
  by priority — critical (blocks core functionality) first.

  ### Execution Plan

  **Branch strategy:** Work on a `protocol-parity` feature branch. GitLab CI
  only runs on `main`, tags, and manual web triggers — feature branch pushes
  are free. GitHub only triggers on tags. Push freely to the feature branch
  with zero compute cost on either side.

  **Workflow:**
  1. Create `protocol-parity` from `main`
  2. Work through gaps in priority order, one commit per gap
  3. Push to the feature branch freely (no CI cost)
  4. Run `cargo fmt`, `cargo clippy`, `cargo test` locally before merging
  5. Merge to `main` with `[skip ci]` to avoid a CI run, OR let CI run once
     if validation is needed
  6. Tag a release only when everything is stable

  **Order of attack:**

  | # | Item | Complexity | Status |
  |---|------|------------|--------|
  | 1 | Algorithm XML parser | High | Pending |
  | 2 | T56 firmware update (port from C) | Low | Pending |
  | 3 | T56/T76 ZIF pin control + voltage | Medium | Pending |
  | 4 | eMMC partition selection | Medium | Pending |
  | 5 | T76 adapter ID validation | Medium | Pending |
  | 6 | T76 OVC for NAND/eMMC | Low-Medium | Pending |
  | 7 | Database refresh (V12.91 → V13.19) | Low | Pending |
  | 8 | Parallel NOR programming | Unknown | Pending |
  | 9 | VGA/HDMI investigation | Low | Pending |

  **Rationale:** Algorithm XML parser first (unblocks all T56/T76 FPGA ops).
  T56 firmware update second (quick port, confidence builder). ZIF/voltage
  third (other features depend on it). eMMC partitions fourth (well-specified
  gap). Remaining items follow in decreasing impact.

  **Deferred (require hardware):**
  - Hardware validation of all T56/T76 chip classes
  - eMMC io_init hardcoded constants (need hardware to test fixes)
  - eMMC bring-up query response lengths (need hardware to validate)

  **Compute cost conservation:**
  - Feature branch pushes: free on both GitLab and GitHub
  - Local builds/tests: free
  - Merge to main with `[skip ci]`: free
  - Only cost: final release tag (one pipeline + one Actions run)

  ### Critical — blocks T56/T76 operations

  - [x] **Algorithm XML parser** — DONE (protocol-parity branch). The
    `algorithm.rs` module parses `algorithm.xml`, computes algorithm names
    from `protocol_id` + `variant` (with special cases for ATmega ICSP,
    AT89C ICSP, eMMC voltage, reversed packages, and logic chips),
    base64-decodes and gunzips the bitstream, verifies CRC32, and performs
    T76 level-2 zero-run decompression. Integrated into
    `MiniproHandle::begin_transaction` — automatically looks up the
    algorithm when a T56/T76 device needs one.
    **Impact:** T56/T76 FPGA-based chip operations now work when
    `algorithm.xml` is present.

  - [x] **T56/T76 ZIF pin control and voltage control** — NOT APPLICABLE.
    Investigation confirmed the C minipro itself does NOT implement
    `set_zif_direction`, `set_zif_state`, `get_zif_state`, `set_pin_drivers`,
    or `set_voltages` for T56/T76. These function pointers are NULL in the
    C handle setup. The T56/T76 use FPGA bitstream algorithms that handle
    pin control and voltage internally through the FPGA, not through direct
    ZIF pin manipulation commands. This is an architectural difference, not
    a gap.

  ### High — known gaps vs Matt Brown's t76 branch

  - [x] **eMMC partition selection** — DONE (protocol-parity branch).
    Implemented via `T76_EMMC_PARTITION` env var (user|boot1|boot2|rpmb).
    Uses CMD6 SWITCH to set EXT_CSD[179] PARTITION_CONFIG. Capacity
    detection now uses the correct EXT_CSD field per partition:
    USER: SEC_COUNT[212], BOOT: BOOT_SIZE_MULT[226], RPMB:
    RPMB_SIZE_MULT[168].

  - [ ] **T76 adapter ID validation** — DEFERRED. The mainline C minipro
    does NOT implement adapter ID validation for T76. The `t76_adapter_init`
    sends a READ_ID command (0x24, 0xe4) but discards the response. Matt
    Brown's merged MR #292 (https://gitlab.com/DavidGriffith/minipro/-/merge_requests/292)
    also does not implement adapter ID validation. Implementing this would
    require reverse-engineering the adapter ID response format from XGPro
    captures with different adapters.
    **Impact:** User can select a chip that requires an adapter they haven't
    connected, leading to confusing protocol errors instead of a clear
    "wrong adapter" message.

  - [x] **T76 OVC status for NAND/eMMC** — DONE (protocol-parity branch).
    The `get_ovc_status` trait method now takes `&Device`. For NAND/eMMC
    (protocol_id 0x2d/0x31), the T76 implementation repacks the chip-
    parameter header (protocol_id, variant, voltages, chip_info, pin_map)
    into `msg[1..7]` of the 0x39 status request, mirroring the vendor
    behavior. A zeroed 0x39 deselects the NAND; the repacked header keeps
    it selected. OVC checks are now enabled for NAND/eMMC in
    `begin_transaction`, per-block write, and `check_ovc`.

  ### Medium — missing features from original minipro

  - [ ] **SPI flash autodetect (`-a` / `--auto_detect`)** — PARTIAL.
    Upstream's `-a` reads a JEDEC ID from SPI flash (25xx devices) via
    firmware command 0x37, then searches `infoic.xml` for devices with
    matching `chip_id` and pin count, printing all matching device names.

    **What we have:**
    - CLI flag `-a` / `--spi-autodetect` (with `auto_detect` alias)
    - Protocol implementations for TL866A/CS and TL866II+ (firmware
      command 0x37, 3-byte big-endian JEDEC ID parse)
    - Operations layer `spi_autodetect()` and
      `spi_autodetect_and_lookup()` in `operations.rs`
    - Database lookup by JEDEC ID: `find_devices_by_chip_id()` in
      `database.rs` — DONE. Searches the model-appropriate `infoic.xml`
      section, matches on `chip_id` equality and optional pin-count
      filter, splits comma-separated names, skips degenerate chip_id=0
      entries. CLI output now matches upstream format:
      `Autodetecting device (ID:0xXXXX)` + device names + count.

    **What we're missing (2 remaining gaps):**

    1. **T56 autodetect** — T56 has `CMD_AUTODETECT = 0x37` defined but
       no `spi_autodetect` trait method override (falls through to
       `UnsupportedOperation`). Upstream's `t56_spi_autodetect()` must
       first upload an FPGA bitstream (SPI25F11 for 8-pin or SPI25F21
       for 16-pin) via `t56_send_bitstream()`, then send the 0x37
       command.
       **Difficulty: Medium.** The bitstream upload infrastructure
       already exists (`upload_bitstream()` in `t56.rs`, algorithm
       lookup via `get_algorithm()` in `algorithm.rs`). The
       `compute_algorithm_name()` function already maps
       `protocol_id=0x03, variant=0x1100` → `"SPI25F11"` and
       `variant=0x2100` → `"SPI25F21"` (verified by existing tests in
       `algorithm.rs`). The implementation needs to: construct a
       temporary `Device` with `protocol_id=0x03` and the appropriate
       variant, call `get_algorithm()`, upload the bitstream, then send
       the 0x37 command and parse the 3-byte response. ~50 lines of
       protocol code + trait wiring. The tricky part is managing the
       `bitstream_uploaded` flag — autodetect uses a different
       bitstream than the subsequently-selected device, so the flag
       must be reset after autodetect.

    2. **T76 autodetect** — same pattern as T56. Upstream's
       `t76_spi_autodetect()` uploads SPI25F11/SPI25F21 bitstream via
       `t76_send_bitstream()`, then sends 0x37. No trait override in
       our `t76.rs`.
       **Difficulty: Medium.** Same infrastructure as T56
       (`upload_bitstream_t76()`, `get_algorithm()`). Same
       `bitstream_uploaded` flag management concern. ~50 lines.

    **Also missing (minor):**
    - Pin-contact pre-check before autodetect (upstream optionally runs
      `minipro_pin_test` on TL866II+ before autodetect). We have pin
      test support; wiring it in is trivial but low-value.

    **Overall difficulty: Medium.** The database lookup is now done.
    T56/T76 protocol implementations are mechanical ports from C (~50
    lines each) with existing bitstream infrastructure. The main
    subtlety is bitstream-flag management across the autodetect →
    device-selection transition. T56/T76 protocol implementations
    ideally need hardware testing but the code path is identical to
    upstream C.

  - [x] **T56 firmware update** — DONE (protocol-parity branch). Ported
    from C `t56_firmware_update()`. Implemented as `firmware_update_t56()`
    standalone function (needs `&mut MiniproHandle` for USB reconnect).
    Handles: file version/CRC validation, bootloader magic switch (0x3D),
    erase (0x3C), block-by-block reflash (0x814-byte blocks via 0x3B),
    and reset back to normal mode. Routed through `operations::firmware_update()`.

  - [x] **Database refresh** — DONE. Updated `infoic.xml` from Griffith's upstream
    (XGPro V13.19). Device count went from 28,772 to 30,808 (+2,036 new T76 chips).
    Same GPL v3 license, no IP issues. `logicic.xml` was unchanged.

  - [ ] **Parallel NOR programming (T76)** — READ and ERASE work, PROGRAM is
    non-functional. The vendor uses a per-command descriptor that hasn't been
    reverse-engineered. Confirmed as a shared limitation: Matt Brown's merged
    MR #292 implements parallel NOR program (protocol 0x12/0x14) in the C
    minipro, but our Rust port has not yet incorporated this. Requires porting
    Matt Brown's parallel NOR write implementation from the upstream C code.
    **Impact:** Parallel NOR flash chips can be read and erased but not
    written. Niche use case (parallel NOR is uncommon).

  ### Low — niche or deferred

  - [x] **VGA/HDMI chip support** — NOT APPLICABLE. Investigated: the
    database (both ours and Matt Brown's) contains zero type="8" (VGA/HDMI)
    entries. The type is defined in the XML comment but unused. `ChipType::Vga`
    exists as an enum variant for completeness but no device can ever be
    selected with this type. No filtering or protocol implementation needed.

  - [ ] **T76 eMMC io_init hardcoded constants** — the 40-byte region init
    uses hardcoded geometry constants from a KLM8G1GEAC capture. These may
    not generalize to other eMMC chips. Needs parameterization from device
    database or EXT_CSD fields.
    **Impact:** eMMC operations may fail on non-KLM8G1GEAC chips.

  - [ ] **T76 eMMC bring-up query response lengths** — response lengths
    (32B, 32B, 24B) are from a single chip capture and may not generalize.
    **Impact:** eMMC init may desync on other chips.

  ### Gaps vs Matt Brown's merged MR #292

  Matt Brown's MR #292 (https://gitlab.com/DavidGriffith/minipro/-/merge_requests/292)
  was merged to upstream C minipro master on 2026-06-01. A detailed comparison
  identified 3 items we are missing. These may be partially addressed by
  Agnius's pending MR (GitLab work item #3).

  - [ ] **SPI-NAND geometry fix-up (database layer)** — ~1780 SPI-NAND chips
    in the database have `code_memory_size == 0` because XGecu packs geometry
    into other fields: `page_size` holds block count, `pages_per_block` has
    vendor flags in the top byte, `write_buffer_size` is page + spare. Matt
    Brown added fix-up logic in `database.c` that computes the real
    `code_memory_size = block_count * pages_per_block * (page + spare)`. Our
    `database.rs` reads these fields directly with no unpacking.
    **Impact:** SPI-NAND chips with packed geometry read as zero-size.
    **Location:** `crates/minipro-core/src/database.rs:1037`

  - [ ] **pages_per_block masking (0xFFFF)** — the upper bits of
    `pages_per_block` contain database flags, not geometry. Matt Brown's
    database fix-up masks these. We use the raw value in at least 3 places
    in `t76.rs` (lines 648, 830, 1045). Agnius independently found this same
    bug during hardware testing.
    **Impact:** Incorrect block calculations for NAND chips with non-zero
    flag bits in pages_per_block.
    **Location:** `crates/minipro-core/src/database.rs:1037`,
    `crates/minipro-core/src/protocol/t76.rs:648, 830, 1045`

  - [ ] **status_recv on EP83** — Matt Brown added a `status_recv()` function
    that reads per-block NAND program status from EP83 (endpoint 0x83 IN).
    Our `usb.rs` uses EP83 for payload reads but has no dedicated status
    polling function. Our NAND program at `t76.rs:837-865` may not properly
    check per-block status responses.
    **Impact:** NAND program may not detect per-block write failures.
    **Location:** `crates/minipro-core/src/usb.rs` (missing function),
    `crates/minipro-core/src/protocol/t76.rs:837-865`

  ### Hardware validation (separate from code parity)

  These items are not code gaps — the code is written to match the C source
  but has never been tested on real hardware. Validation requires physical
  T56/T76 devices and chips.

  - [ ] T76 SPI NOR (8-pin and 16-pin) — read/erase/program
  - [x] T76 SPI-NAND — read/erase/program — VALIDATED by Agnius (GitLab work
    item #3) on GD5F1GQ5UExxG(x4)@WSON8, T76 firmware 00.1.18, XGPro v13.21.
    Full read (142MB, SHA-256 verified across two reads), erase, write, and
    verify all working with local patches. MR pending.
  - [ ] T76 parallel NAND — read/erase/program
  - [ ] T76 eMMC USER partition — read/erase/program
  - [ ] T76 parallel NOR — read/erase (program known broken)
  - [ ] T56 all chip classes — read/erase/program
  - [ ] T76 firmware update
  - [ ] T76 logic IC test (two-pass with bitstream reload)

  Matt Brown's MR #292 was validated on: ZB25VQ64A, MX25L12845E (SPI NOR),
  S29GL512N (parallel NOR), W29N02GZ and GD5F1GM7UEYIG (NAND), KLM8G1GEAC-B001
  (eMMC).

- [x] **Smart firmware diff** — compare firmware files or chip dumps with intelligent trailing-padding handling
  - **Problem:** Minipro read-back is always full chip size (e.g., 8192 bytes), but source files are often smaller (e.g., 1936 bytes). Simple byte-wise comparison fails even when executable code is identical. Naive "strip trailing 0xFF and compare" is insufficient because it silently ignores cases where the reference has real data beyond the dump length (truncated read, wrong chip selected) or where the dump has non-erased data beyond the reference (leftover from previous programming — forensically interesting).
  - **Algorithm: byte-aligned, three-way tail classification (not LCS)**
    - Firmware images are fixed-size, offset-stable binaries — insertions/deletions are rare. Byte-by-byte comparison at matching offsets is the correct default. LCS/Myers diff adds complexity for a scenario that doesn't occur in chip programming.
    - Erase value is configurable per device (from `blank_value` in the database), not hardcoded to `0xFF`. NOR flash erases to `0xFF`, some EEPROM/NAND erase to `0x00`.
    - **Three-way tail classification** instead of simple trim-and-compare:
      - **Compared region** (offsets where both files have data): normal byte diff applies
      - **Padding-tail region** (offsets beyond shorter file where longer file is all erase-value): benign, shown in neutral/gray — not a real diff
      - **Anomalous-tail region** (offsets beyond shorter file where longer file has non-erase-value data): real problem — likely truncated read, wrong chip selected, or leftover data from previous programming. Flag prominently in red/amber.
    - Do NOT silently truncate or pad the shorter file before diffing — this loses the ability to distinguish "padding" from "actual diff." Keep classification metadata separate from the raw comparison.
  - **Implementation plan:**
    1. **Core algorithm in `minipro-core` Rust** — `smart_diff(a, b, erase_value)` returns:
       - `Vec<DiffEntry>` — list of differing bytes with offset, expected, actual
       - `Vec<TailRegion>` — classified tail regions (padding vs anomalous) with offset range and classification
       - `is_equal: bool` — true if no real differences (ignoring benign padding)
       - `summary: DiffSummary` — counts: N bytes differ across M contiguous regions, tail classification breakdown
    2. **CLI:** `minipro diff file1 file2 [-f format] [--erase-value 0xFF]` — outputs human-readable table with diff entries, tail classification, and summary. Exit code 0 on match, 1 on mismatch.
    3. **GUI: Hex viewer "Compare" button** — uses loaded buffer as one side, pick reference file, backend runs `smart_diff` and returns structured result. Hex viewer switches to diff mode:
       - **Single-pane view** (not side-by-side) — shows the chip buffer with color highlighting. Lets user edit mismatches in-place without switching panes. No architectural change to add a second hex viewer pane.
       - **Four-state cell highlighting:**
         - Unchanged bytes: no highlight
         - Differing bytes: red background (`#fee2e2`), red text (`#991b1b`), bold
         - Beyond-reference bytes (in dump, not in file): amber background (`#fef3c7`) — "present in chip but not in file"
         - Beyond-dump bytes (in file, not in dump): blue background (`#dbeafe`) — "present in file but not in chip"
       - **Toolbar summary:** "3 differences across 2 regions" or "Files match (ignoring trailing padding)" + "[Clear Compare]"
       - **Next/Prev diff navigation** — buttons + keyboard shortcuts (F3 / Shift+F3) to jump between differing bytes. Diff counter: "Diff 2 of 3"
       - **Tail warning banner** if anomalous-tail detected: "Reference has 47 bytes of non-padding data beyond dump length — possible truncated read or wrong chip selected"
    4. **Single source of truth** — algorithm in Rust (`minipro-core`), GUI sends both buffers via base64 for comparison. CLI calls the same function directly.
    5. **CRC-32 shortcut** — if CRC-32 of both files match (already shown in hex viewer toolbar), skip the full diff and report "Files are identical." Fast path for the common case.
  - **Possible extensions (not in initial implementation):**
    - "File vs Chip" comparison without requiring an intermediate save — read chip to buffer, then compare against file
    - Minimap / diff density strip for large files (1MB+ NOR flash) — defer until we regularly deal with 16MB+ NAND dumps
    - Structure-aware overlays (template/struct definitions for known memory layouts) — separate project, too ambitious for initial implementation
  - **Design decisions and rationale:**
    - Byte-aligned over LCS: firmware is offset-stable, LCS adds complexity for a non-issue
    - Single-pane over side-by-side: enables in-place editing, no architectural change, familiar to hexdump+diff users
    - Eager diff computation over lazy: even 1MB takes <10ms in Rust; virtual scrolling already handles rendering
    - Three-way tail classification over simple trim: catches real errors (truncated reads) instead of silently ignoring them; surfaces forensically interesting leftover data
    - Configurable erase value over hardcoded 0xFF: NOR erases to 0xFF, some EEPROM/NAND erase to 0x00; device database already has `blank_value`
  - Status: Ready to implement when prioritized.

- [x] Auto SN_NUM — production programming with auto-incrementing serial numbers
  - Implemented as `--serial-*` CLI flags and GUI "Serial Number" section in batch mode
  - Supports: start value, step, target address, width (1-8 bytes), format (bin/ascii/bcd), endian (little/big), optional checksum (XOR/CRC-8)
  - See completed entry above and detailed spec in Near-term section

- [x] **Manual trim/pad to size** — let advanced users resize firmware files before saving
  - Trim trailing fill bytes to reduce a read-back (8192 bytes) to actual code size (1936 bytes)
  - Pad with fill byte to a specific size (e.g., exact device memory size) for tools that require full-size files
  - Useful when exporting to other tools, version control, or creating "canonical" firmware files
  - Implemented as a "Trim/Pad" button in the hex viewer toolbar with inline panel:
    - **Trim trailing** — removes trailing bytes equal to the fill byte (0xFF or 0x00)
    - **Pad to: [size]** — enter target size, pads with fill byte to that size
    - Fill byte dropdown: 0xFF (NOR flash) / 0x00 (EEPROM/NAND)
  - Core functions in `hex.ts`: `trimTrailing()` and `padToSize()`

- [~] **ASCII insert mode in hex editor** — **Won't fix.** Overtype is the correct model for chip memory.
  - Chip memory is address-indexed. Inserting bytes in the middle shifts all subsequent data to wrong offsets — when written to a chip, the data lands at wrong addresses. That's not editing, that's corruption.
  - Overtype mode (current behavior) respects the address-space model: byte at offset 0x1FF0 stays at 0x1FF0. This is what embedded developers actually need.
  - Users who need to insert bytes in a firmware file should use a dedicated binary editor (HxD, hexcurse) before loading the file into the programmer.
  - A PR for this would need to address the fundamental address-shift problem before being considered.

- [ ] **Logic Test GUI panel** — replace raw text output with a visual grid for testing logic ICs
  - Current state: backend returns ANSI-colored text table (vectors × pins). The GUI just dumps this to the terminal.
  - Design challenges:
    - Backend outputs unstructured text with ANSI codes; needs structured DTO (JSON with per-cell pass/fail/expected/actual)
    - Grid scales with pin count and vector count (e.g., 74HC00 = 14 pins × 8 vectors = 112 cells; larger ICs = more)
    - Need visual encoding for 8+ state types: L=Low, H=High, Z=Hi-Z, G=GND, V=VCC, C=Clock, X=Don't care, 0/1=Logic levels
    - Two-pass test data (pull-up vs pull-down) — show both or just conclusion?
    - Error highlighting must be prominent (red cells, summary banner)
  - Requires: new backend DTO, dedicated `LogicTestPanel.svelte` component, device support check (must be from `logicic.xml` with `vector_count > 0`)
  - Priority: medium — useful for debugging logic ICs, but most users program MCUs and memory chips

- [ ] **Logic IC auto-find ("Auto Find")** — automatically identify an unknown logic IC by iterating test vectors
  - **Problem:** When a user has an unmarked or unknown logic IC, they must manually guess the part number and select it before running a logic test. XGPro's "Auto Find" feature iterates through all logic ICs in the database, runs each one's test vectors, and reports which ones pass — no manual selection needed.
  - **Upstream parity note:** The C minipro does NOT implement this. Upstream's `-a` / `--auto_detect` is SPI flash only (JEDEC ID read via firmware command 0x37). The `compare_device()` function in `database.c` explicitly skips logic ICs (`if (sm->db_version == LOGIC_DATABASE) return EXIT_SUCCESS;`). Logic ICs have no chip ID — they only have test vectors. XGPro's Auto Find is a software-level loop, not a firmware command.
  - **Implementation approach:**
    1. **CLI:** `minipro --logic-autofind [--pin-count N]` — iterates all `logicic.xml` entries (optionally filtered by pin count), runs `logic_ic_test` for each, prints passing candidates
    2. **Core:** new `logic_ic_autofind()` function in `operations.rs` — takes a callback for progress reporting and candidate reporting. Reuses existing `logic_ic_test` per candidate. Must handle `begin_transaction` / `end_transaction` per device.
    3. **GUI:** "Auto Find" button in the Logic Test options panel (visible only when no device is selected, or when a logic IC device type is selected). Shows progress ("Testing 74HC00... pass / Testing 74HC02... fail"). Results list with clickable entries that select the device.
  - **Design considerations:**
    - Pin count filter is important: testing a 14-pin IC against 16-pin vectors wastes time and can report false passes
    - Some logic ICs share the same pin count and similar pinouts — multiple candidates may pass. The results list should show all passing candidates, not just the first.
    - Each candidate requires a fresh `begin_transaction` with that device's parameters (voltages, pin map, etc.)
    - Progress reporting is important — the database has thousands of logic IC entries, and each test takes ~1-2 seconds
    - Should be cancelable (user may want to stop after finding a match)
  - **Scope:**
    - Phase 1: CLI only (`--logic-autofind`), no pin-count filter (test all), prints passing candidates
    - Phase 2: GUI "Auto Find" button with progress and clickable results
    - Phase 3: Pin-count auto-detection (ask user to enter pin count, or detect from chip insertion)
  - **Priority: low-medium** — useful feature not available in upstream minipro, but niche (most users program MCUs/memory, not logic ICs). XGPro has this, so it's a parity gap vs XGPro but not vs upstream minipro.

- [x] **GUI SPI flash autodetect button** — "Auto Detect" button in the device selector area that reads the JEDEC ID from an inserted SPI flash and shows matching devices from the database
  - **Core logic already implemented:** `spi_autodetect_and_lookup()` in `operations.rs` combines the firmware JEDEC ID read with `find_devices_by_chip_id()` database lookup. Works for TL866A/CS and TL866II+ today; T56/T76 pending protocol implementation (gaps 2/3 in the parity section above).
  - **GUI workflow:** user inserts unknown SPI flash → clicks "Auto Detect" → backend reads JEDEC ID and searches `infoic.xml` → frontend shows list of matching device names (with manufacturer) → user clicks one to select it in the DeviceSelector
  - **Implementation plan:**
    1. **Backend:** new `do_spi_autodetect` Tauri command — takes `id_type` (0 = 8-pin, 1 = 16-pin), calls `spi_autodetect_and_lookup()`, returns `SpiAutodetectResultDto { jedec_id: u32, matches: Vec<DeviceListItemDto> }`
    2. **Frontend:** "Auto Detect" button in DeviceSelector (or a small toolbar above the search box). On click, calls the command, shows results in a dropdown or inline list. Each result is clickable and sets `$selectedDevice`. If no matches, shows "No device found (JEDEC ID: 0xXXXX)" with a hint to try the other pin-count option.
    3. **Pin-count selector:** a small toggle (8-pin / 16-pin) next to the button, defaulting to 8-pin (most common). Or two buttons: "Auto Detect 8-pin" / "Auto Detect 16-pin".
  - **Design considerations:**
    - Button should be disabled when no programmer is connected
    - Button should be disabled for T56/T76 until protocol support is implemented (gaps 2/3) — or show a "not yet supported for this programmer" tooltip
    - Results list should show manufacturer alongside name (same as search results)
    - If multiple matches, show all — user picks the correct one (e.g., W25Q128 vs W25Q128JV may have the same JEDEC ID)
    - If JEDEC ID is 0x000000, show "No SPI chip detected" (chip not inserted or not SPI flash)
  - **Priority: medium** — XGPro has this feature, core logic is done, GUI work is small (~1 Tauri command + ~1 button + results list). Most useful for users with unmarked SPI flash or salvaged chips.

- [x] **Contextual help overlay for batch/serial panel** — "i" icon next to the Serial Number Injection label opens a modal explaining serial injection, all fields (address, start, step, format, width, endian, checksum), and validation (live preview, overflow detection, blocking errors). Escape listener shared with config help modal.

- [~] **Fuse bit decoder for config panel** — decode raw fuse bytes into individual named bits with checkboxes
  - **AVR phase: DONE.** All 18 AVR config variants (`avr_1` through `avr_18`) have bit-level definitions sourced from avr-libc device headers and Microchip datasheets.
  - **PIC phase: DONE.** All database-referenced PIC config variants now have bit-level definitions:
    - PIC10F/PIC12F5 baseline 12-bit: `pic_1`–`pic_8`
    - PIC12F/PIC16F mid-range 14-bit: `pic_9`–`pic_13`, `pic_21`, `pic_23`–`pic_25`
    - PIC16F baseline 12-bit: `pic_15`–`pic_18`, `pic_27`
    - PIC18F 16-bit: `pic_28`–`pic_33`, `pic_34`–`pic_43`, `pic_49` (including aliases `pic_38`→`pic_34`, `pic_39`→`pic_35`, `pic_40`→`pic_36`, `pic_41`→`pic_37`)
    - 5 configs remain skipped due to database/datasheet mask discrepancies: `pic_14`, `pic_19`, `pic_20`, `pic_22`, `pic_26` (see `docs/XGPRO-DATABASE-DISCREPANCIES.md`)
    - Bit definitions verified against gputils configuration documentation and Microchip datasheets
    - Fixed pre-existing CPB/CPD and WRTC/WRTD bit position swap in shared PIC18F protection word definitions
  - **Implementation:**
    - Backend: `gui/src-tauri/src/fuse_defs.rs` — static bit definitions keyed by infoic.xml `<config name="...">` attribute, with chip-prefix overrides for configs that span multiple architectures.
    - Frontend: `FuseBitDecoder.svelte` component renders an 8-bit grid (MSB→LSB) with clickable bit cells, field names, descriptions, and dangerous-bit warnings. Raw hex input remains visible and stays in sync (editing hex updates bits, clicking bits updates hex).
    - Fallback: when no bit definitions exist for a config name (e.g., the 5 skipped PIC configs, or unknown), the config panel falls back to hex-only input.
  - **Config-name keying analysis (verified):**
    - 13 of 18 AVR configs map to a single chip family with consistent fuse bit meanings — one definition per config name.
    - 5 AVR configs span multiple architectures and require chip-prefix overrides:
      - `avr_4`: ATmega48 vs ATtiny24/44 (different hfuse bit assignments)
      - `avr_6`: ATtiny25/45/85 vs ATtiny2313/4313 (completely different hfuse bit order)
      - `avr_13`: ATmega128A (legacy BODEN lfuse) vs ATmega164/324/644/1284 family (modern CKDIV8 lfuse)
      - `avr_15`: ATmega8 (RSTDISBL in hfuse bit 7) vs ATmega8535 (S8535C in hfuse bit 7)
      - `avr_17`: U2 chips (DWEN/RSTDISBL in hfuse) vs U4 chips (OCDEN/JTAGEN in hfuse) vs ATmega328PB (BODLEVEL in hfuse, BOOTRST/BOOTSZ in efuse)
    - Several PIC configs share layouts via aliases: `pic_16`/`pic_17`, `pic_30`/`pic_31` (separate defs), `pic_37`/`pic_41`
    - Prefix match order matters: longer prefixes (e.g., `ATMEGA1284`) must be checked before shorter ones (e.g., `ATMEGA128`) to avoid false matches.
  - **Dangerous bit highlighting:** RSTDISBL, DWEN, SPIEN, OCDEN, JTAGEN are flagged with red styling and a ⚠ indicator in the field description list.
  - **AVR fuse convention:** bit = 0 means programmed (active). The decoder shows "Programmed"/"Unprogrammed" labels for AVR devices and raw "1"/"0" for non-AVR.
  - Priority: medium — hex input works but is error-prone for users unfamiliar with bit manipulation

- [ ] **ZIF socket placement diagram** — visual panel showing the selected device correctly oriented and positioned in the programmer's ZIF socket
  - **Goal:** prevent the most common user error — inserting a chip in the wrong position or wrong orientation in the ZIF socket
  - **Programmer model differences (VERIFIED):**
    - All models use the same chip insertion convention: **chip pin 1 → ZIF pin 1, at the top of the socket**
    - The ZIF socket is physically **upside down** on T48/T56/T76 compared to TL866A/CS/II+ — the lever moved from the top (pin 1 end) to the bottom (opposite end)
    - Pin numbering is the same standard U-shaped arrangement on all models
    - The `pin_map` mask data works uniformly — ZIF pin 1 is always ZIF pin 1 regardless of model

    | Model | ZIF Socket | Pin 1 Position | Lever Position |
    |-------|-----------|----------------|----------------|
    | TL866A/CS | 40-pin | Top | Top (same end as pin 1) |
    | TL866II+ | 40-pin | Top | Top (same end as pin 1) |
    | T48 | 48-pin | Top | Bottom (opposite end from pin 1) |
    | T56 | 48-pin | Top | Bottom (opposite end from pin 1) |
    | T76 | 48-pin | Top | Bottom (opposite end from pin 1) |

  - **Data available from database:**
    - `pin_count` — number of pins on the device
    - `package_type` — DIP{N} or PLCC{N} (derived from `package_details`)
    - `adapter` — adapter type index (TSOP48, SOP44, etc.) — for adapter-based devices
    - `pin_map` — index into infoic.xml `<maps>` section; the `mask` array tells which ZIF pins are occupied, implicitly encoding placement position
    - `icsp` — ICSP mode flags from `package_details`
  - **Data NOT available (must be derived or static):**
    - No explicit "insert at position X" field — derive from `pin_map` mask data, or fallback to pin_count-based placement (pin 1 at ZIF pin 1, chip at top of socket)
    - ICSP wiring diagrams cannot be derived from the database — need static SVG images per programmer model showing VCC/GND/SCK/MISO/MOSI/RESET pin assignments
  - **Design decisions (RESOLVED):**
    - **Chip placement:** identical for all models — pin 1 at top (ZIF pin 1). Use `pin_map` mask when available (pin_map != 0), fallback to pin_count-based placement otherwise
    - **Diagram rendering:** always render pin 1 at top. Draw lever at top (TL866A/CS/II+) or bottom (T48/T56/T76) based on `programmer.model`. Two SVG templates: 40-pin and 48-pin
    - **UI placement:** ZIF diagram immediately below DeviceSelector (semantic continuity: "which chip" → "how to place it"). DiagnosticsPanel drops to bottom with collapsible buttons. Right sidebar stays focused on log
    - **Socket size:** 40-pin (TL866A/CS/II+) or 48-pin (T48/T56/T76), determined by `programmer.model`
  - **Implementation plan:**
    - **Layout (left sidebar):**
      ```
      DeviceSelector (flex-1)
        ├── search box
        ├── device list
        └── selected device info
      ZifSocketDiagram (fixed height, ~200px)
        ├── ZIF socket SVG (40 or 48 pin, based on programmer model)
        ├── Chip overlay positioned on occupied pins
        ├── Lever indicator (top or bottom, based on model)
        └── Chip name + pin count label
      DiagnosticsPanel (shrink-0, natural height)
        ├── Programmer info (Model, FW, SN) — always visible
        └── Collapsible buttons (collapsed by default)
      ```
    - **Component: `ZifSocketDiagram.svelte`**
      - Reads `$selectedDevice` and `$programmer` stores directly (no props)
      - `$derived`: socketSize (40/48), leverAtTop (bool), occupiedPins (from pin_map mask or pin_count fallback), chipName
      - SVG: socket body (rounded rect, theme-aware fill), pin slots (currentColor + opacity), pin number labels (U-shape), chip overlay (semi-transparent primary accent), pin 1 notch/dot, lever icon
      - Uses `currentColor` and Skeleton theme classes for automatic dark mode support
    - **Fallback placement (pin_map == 0):** use `pin_count` — left side pins 1 to N/2, right side pins N/2+1 to N, all at top of socket
    - **Backend:** add `get_pin_map` Tauri command wrapping existing `database::get_pin_map()`, returns mask array for selected device
    - **DiagnosticsPanel:** wrap 4 diagnostic buttons in `<details>` collapsed by default, keep programmer info always visible
    - **Files to create/modify:**
      | File | Action |
      |------|--------|
      | `gui/src/lib/components/ZifSocketDiagram.svelte` | Create — SVG diagram component |
      | `gui/src/lib/components/DiagnosticsPanel.svelte` | Modify — collapsible buttons |
      | `gui/src/App.svelte` | Modify — import, reorder left sidebar |
      | `gui/src-tauri/src/commands.rs` | Modify — add `get_pin_map` command |
      | `gui/src-tauri/src/lib.rs` | Modify — register `get_pin_map` command |
    - **Verification:** `cargo tauri build`, test DIP8/14/28/40 placement, verify lever position per model, verify dark mode, verify pin_map==0 fallback
  - **Remaining design questions:**
    - How to handle non-DIP packages (PLCC, TSOP, SOP) — these use adapters; should the diagram show the adapter + chip, or just indicate "requires adapter X"?
    - Should the diagram update in real-time when the user toggles ICSP mode, or only when a device is selected?
  - **Scope:**
    - Phase 1: DIP packages only (most common), derive placement from pin_map mask (fallback to pin_count), render SVG ZIF socket with chip overlay and lever indicator — **DONE**
    - Phase 2: ICSP connector pin-numbering diagram (see details below)
    - Phase 3: Adapter-based packages (TSOP, SOP, PLCC) — more complex, lower priority
  - **ICSP wiring diagrams with signal labels — dropped:**
    ICSP pin assignment is not in `infoic.xml` — the `pin_map` field is for ZIF socket pin-contact testing, not ICSP header pinout. The ICSP signal routing is handled entirely in the programmer's firmware: `begin_transaction` sends an ICSP bitmask in byte 3 (0x80 = enable, 0x01 = VCC), and the firmware internally multiplexes the ICSP header pins based on `protocol_id` and chip family. The same physical ICSP pin carries different signals (VPP, VCC, GND, MISO, MOSI, SCK, SDA, CLK) depending on which chip is selected.
    A static per-model diagram with signal labels would be actively dangerous — showing "pin 1 = VCC" would be wrong for some chip families and could cause users to apply VPP to the wrong line, damaging the target chip or the programmer. Only Xgpro's "[View ICSP Connection]" feature generates chip-specific wiring diagrams, and that logic is embedded in Xgpro's binary, not derivable from the XML database.
    Reverse-engineering Xgpro to extract the pinout logic was considered and rejected due to legal risk (EULA prohibitions on RE), safety risk (RE errors could lead to hardware damage), and technical uncertainty (the pinout may be algorithmically generated, not a simple lookup table).
  - **Phase 2: ICSP connector pin-numbering diagram (replaces signal-label diagrams):**
    When ICSP mode is selected, show a physical diagram of the ICSP connector with pin numbers only (no signal labels). This helps users identify pin 1 (for ribbon cable red-stripe alignment) and cross-reference pin numbers with Xgpro's chip-specific "[View ICSP Connection]" diagram.
    A note will direct users to Xgpro for chip-specific signal assignment: "ICSP mode active. Pin numbering shown for reference. For chip-specific signal assignment (VCC, GND, MISO, MOSI, SCK, RST), use Xgpro's [View ICSP Connection] button."
    **ICSP header physical layouts:**
    | Model | Header | Pins | Numbering |
    |-------|--------|------|-----------|
    | TL866A | 1×6 | 6 | Linear, pin 1 = leftmost |
    | TL866CS | None | — | No ICSP support (show "not supported") |
    | TL866II+ | 1×6 | 6 | Linear, pin 1 = leftmost |
    | T48 | 2×8 | 16 | Zigzag: pin 1 = bottom-left, pin 2 = above pin 1, pin 3 = right of pin 1, etc. |
    | T56 | 1×8 | 8 | Linear, pin 1 = leftmost |
    | T76 | 2×14 | 28 | Zigzag: pin 1 = bottom-left, pin 2 = above pin 1, pin 3 = right of pin 1, etc. |
    **Implementation:**
    - New component `IcspConnectorDiagram.svelte` (or extend `ZifSocketDiagram.svelte` with a mode toggle)
    - Renders when `icspMode` store is not `"zif"` (i.e. `"icsp"` or `"icsp_no_vcc"`)
    - Layout selected by `$programmer.model`
    - SVG: connector body, pin slots, pin number labels, pin-1 indicator (notch/dot)
    - No signal labels — just pin numbers
    - Note text below diagram directing to Xgpro for signal assignment
    - No backend changes needed (layouts are hardcoded constants)
    - TL866CS: show "ICSP not supported on this model" instead of a diagram
  - **Priority: medium-high** — prevents the most common user error; the original XGECU software has this feature and users rely on it
  - **Status:** Phase 1 complete, Phase 2 complete (pin-numbering only, no signal labels), Phase 3 not started

- [x] **GUI voltage override dropdowns** — replace hardcoded voltage option lists with model-specific dropdowns
  - **Problem:** The GUI Advanced section used hardcoded VPP/VCC option lists that only matched the XG (T48/T56) tables. TL866A and TL866II+ users saw invalid options, logic ICs showed VPP/VDD dropdowns that shouldn't exist, and T56/T76 custom-protocol devices showed options when overrides aren't supported.
  - **Solution:** Added `get_voltage_options` Tauri command that returns valid VCC/VPP values from the per-model voltage tables (`vcc_voltage_table()`, `vpp_voltage_table()` in `device.rs`). The frontend dropdowns are now populated from the backend, with inapplicable dropdowns hidden.
  - **Backend changes:**
    - `get_voltage_options` command — returns `VoltageOptionsDto { vcc, vpp, is_logic }` based on connected programmer model and selected device's chip_type/custom_protocol. Falls back to TL866II+ tables when no programmer is connected. Returns all `None` when no device is selected.
    - ~~Update `VoltagesDto` to use the correct per-model table for display strings~~ — DONE (earlier release)
    - ~~Fix `apply_voltage_overrides` to use per-model tables~~ — DONE (earlier release)
  - **Frontend changes:**
    - `voltageOptions` store and `loadVoltageOptions()` in `device.ts`
    - `$effect` in `App.svelte` reloads voltage options when `$programmer` or `$selectedDevice` changes; resets override values to empty
    - Dropdowns populated from `$voltageOptions.vcc` / `$voltageOptions.vpp`
    - VPP hidden when `vpp` is null (logic ICs, custom protocol)
    - VDD hidden for logic ICs; uses VCC table (matching backend behavior)
    - "Voltage overrides not supported for this device" shown when both vcc and vpp are null
  - **Edge cases handled:**
    - No device selected: all options null, "not supported" message shown
    - No programmer connected: falls back to TL866II+ tables
    - Custom protocol on T56/T76: both vcc and vpp null, "not supported" message
    - Logic IC: VCC shows 4-entry logic table (1.8, 2.5, 3.3, 5V), VPP/VDD hidden
    - T76 PLD: VPP uses the PLD table (capped at 18V)
    - Device switch: override values reset, options reload
  - **Priority: medium** — prevents invalid voltage selections that would fail at the backend
  - **Status:** implemented

- [ ] **Remove `check_device_id` parameter from core API** — the per-operation `check_device_id: bool` parameter in `read_chip`, `write_chip`, `verify_chip`, `erase_chip`, `write_chip_bytes`, `verify_chip_bytes`, and `BatchConfig` is now dead weight from the CLI's perspective (the CLI does a single top-level `check_chip_id` call and passes `false` to all per-operation checks). The GUI still uses the parameter for its own pre-operation checks, but also has the same redundancy (calls `check_chip_id` separately AND passes `check_device_id: true` to the operation). Removing the parameter would eliminate the redundancy and simplify the API, but requires updating all GUI command calls in `commands.rs`.
  - **Files to modify:** `crates/minipro-core/src/operations.rs` (remove parameter from all functions), `crates/minipro-cli/src/main.rs` (remove `false` arguments), `gui/src-tauri/src/commands.rs` (remove `check_device_id` field from `OperationOptions` and the per-operation arguments; the GUI's separate `check_chip_id` calls already handle it)
  - **Priority: low** — code cleanup, no user-facing impact
  - **Status:** not started

- [x] **GUI custom database directory** — allow users to select a custom directory containing `infoic.xml` / `logicic.xml` from the GUI settings
  - **Problem:** The CLI supports custom database files via `--infoic` / `--logicic` flags and the `MINIPRO_HOME` env var, but the GUI had no in-app way to specify custom database files. Users who want custom device definitions (especially custom logic IC test vectors) had to set `MINIPRO_HOME` before launching the GUI or place files in the current directory — neither was discoverable.
  - **Context:** Custom logic IC test vectors are an established workflow in the XGecu ecosystem. Xgpro supports importing `.lgc` files (since v10.70), and community tools like [xgpro-logic](https://github.com/evolutional/xgpro-logic) convert `.lgc` to minipro's `logicic.xml` format. Parastream distributes expanded logic IC vector packs. The C minipro CLI supports `--infoic` / `--logicic` overrides. Our CLI has parity (including `--algorithms` for T56/T76). The GUI was the gap — no GUI in the ecosystem offered in-app custom database selection.
  - **Solution:** Added a directory picker to SettingsPanel. The selected directory overrides the standard search path for `infoic.xml` and `logicic.xml`. Persists across restarts via the settings store.
  - **Backend changes:**
    - `set_custom_db_dir(dir: Option<String>)` Tauri command — validates that both `infoic.xml` and `logicic.xml` exist in the directory, updates `AppState.db_paths` cache, reloads device names, clears selected device
    - `get_db_status` Tauri command — returns whether the saved custom directory is actively in use or fell back to default (for the Settings warning)
    - `db_dir_invalid` AtomicBool on AppState — set during startup if the saved directory is missing, so the GUI can show a warning
    - On startup, reads saved custom dir from settings and passes to `DatabasePaths::resolve()` as file-level overrides; falls back to standard search if invalid
  - **Frontend changes:**
    - `customDbDir` field in settings store (persisted)
    - SettingsPanel Database section: current path display, "Browse..." button (directory picker via Tauri dialog), "Reset to default" button, inline warning when saved dir is invalid
    - `reloadDatabase()` in device store — clears selected device and device list after a directory change
    - `pickDirectory()` helper in file-dialog.ts
  - **Startup fallback behavior:** If the saved custom directory is missing or lacks `infoic.xml`/`logicic.xml`, the app silently falls back to standard search paths. A warning is shown in Settings → Database prompting the user to browse for a new directory or reset to default.
  - **Limitation:** If a programmer is connected when the database directory is changed, the connected handle's algorithm lookup path is not updated until reconnection. Chip definitions and logic IC vectors take effect immediately.
  - **Out of scope:**
    - Individual file overrides (directory only — covers `infoic.xml`, `logicic.xml`, and `algorithm.xml` if present)
    - `.lgc` file import (use xgpro-logic to convert to `logicic.xml` first)
  - **Priority: medium-high** — established ecosystem workflow, real user demand, CLI already has parity
  - **Status:** implemented

- [x] **GUI pin-contact test with ZIF diagram highlighting** — run the pin-contact test from the GUI and visually indicate bad pins on the ZIF socket diagram, matching XGPro's behavior
  - **Problem:** The core pin-contact test exists (`pin_contact_check()` in `operations.rs`, `pin_test_tl866()` in `tl866iiplus.rs`) and the CLI exposes it via `-z` / `--pin_check`. But the GUI's DiagnosticsPanel has a disabled "Pin Test (unsupported)" button, and even if enabled, the test results go to `eprintln!` (stderr) — the GUI can't capture them. XGPro shows specific bad pins (e.g., "Bad Pin: ZIF1 - PIN#1") and the user can see exactly which socket positions have poor contact.
  - **Current state:**
    - `pin_test_tl866()` prints "Bad contact on pin: {d_pin}" to stderr and returns `Err(PinContactFailed)` — no structured result
    - `Protocol::pin_test()` trait method returns `Result<()>` — no bad-pin list
    - `pin_contact_check()` in `operations.rs` is a thin wrapper, also returns `Result<()>`
    - DiagnosticsPanel.svelte line 95: disabled button with title "Not yet implemented"
    - ZifSocketDiagram.svelte: shows occupied/unoccupied pins but has no concept of pass/fail state
  - **Implementation plan:**
    1. **Core: change `pin_test` to return structured results**
       - Change `Protocol::pin_test()` trait signature from `Result<()>` to `Result<PinTestResult>` where `PinTestResult { bad_pins: Vec<u16> }` (empty = all good)
       - Update `pin_test_tl866()` to collect bad pins into a `Vec` instead of `eprintln!`-ing them
       - Update `pin_contact_check()` in `operations.rs` to return `Result<PinTestResult>` instead of `Result<()>`
       - Update CLI `-z` handler to print bad pins from the returned struct (preserve existing output format)
       - T48 and T76 inherit/delegate to `pin_test_tl866` — no change needed beyond the shared function's return type
       - TL866A/CS, T56: `pin_test` returns `UnsupportedOperation` (no change — hardware doesn't support direct ZIF pin testing)
    2. **Backend: new `do_pin_test` Tauri command**
       - In `commands.rs`: `do_pin_test(icspMode: String, state: State<'_, Arc<AppState>>)` → returns `PinTestResultDto { bad_pins: Vec<u16> }`
       - Follows existing `try_acquire` / `spawn_blocking` / `take_handle` pattern
       - Calls `begin_transaction` → `pin_contact_check` → returns bad pins
       - No transaction needed beyond begin/end (pin test is read-only)
       - Register in `lib.rs`
    3. **Frontend: enable Pin Test button in DiagnosticsPanel**
       - Replace disabled button with active button
       - Disable when: no programmer connected, no device selected, device has `pin_map == 0` (no contact-test data), or an operation is running
       - On click: call `do_pin_test`, store result in a new `pinTestResult` store (transient, not persisted)
       - Show toast/log with summary: "Pin test passed" or "Pin test failed: N bad pin(s)"
    4. **Frontend: highlight bad pins on ZifSocketDiagram**
       - Add optional `badPins` prop to `ZifSocketDiagram.svelte` (defaults to empty array)
       - When `badPins` is non-empty, render those pin slots in red (or amber) with a warning indicator
       - Good pins in the occupied set render in green (or keep default color)
       - Show pin number labels on bad pins so user knows which physical pin to check
       - Clear highlighting when a new test is run or when the device/programmer changes
       - Wire `pinTestResult` store to the diagram via App.svelte
    5. **Frontend: pin test result panel**
       - Below the ZIF diagram (or in DiagnosticsPanel), show a compact result list:
         - "✓ All pins OK" (green) when `bad_pins` is empty
         - "✗ Bad contact on pins: 1, 10, 11" (red) when bad pins exist
       - "Clear" button to dismiss results and reset diagram to normal
  - **Design considerations:**
    - Pin test requires a selected device (needs `pin_map` from database) — button disabled when no device selected
    - Pin test is only meaningful in ZIF mode (not ICSP) — button disabled when `icspMode != "zif"`
    - **Model support:**
      - TL866II+: supported (`tl866iiplus_pin_test`)
      - T48: supported — our Rust code aliases `T48Protocol` to `Tl866iiPlusProtocol`, so T48 inherits `pin_test` automatically. The T48 hardware has the same bit-banging commands (0x2D-0x36) as the TL866II+. XGPro supports pin detect on T48. Note: upstream C minipro does NOT set `minipro_pin_test` for T48 (it's a gap — upstream warns "T48 support is not yet complete"), but this is a software gap, not a hardware limitation. Our code is more correct than upstream here.
      - T76: **not supported** — the T76 is FPGA-based and lacks the direct ZIF pin bit-banging hardware (commands 0x2D-0x36) that the TL866II+/T48 use. Matt Brown's t76-improvements branch discovered that the T76's `0x3E` command (T76_PIN_DETECTION) is an adapter-init pin-driver configuration step, not a standalone contact test — his `t76_adapter_init()` uses it to configure socket pin drivers before bitstream upload. Running it standalone returns meaningless data and can corrupt subsequent reads by disrupting FPGA state. The upstream C minipro's `t76_pin_test` receives the response but never parses it (`value` is initialized to 0 and never updated from the response buffer, so every pin reports as bad). The [xgecu-pro](https://github.com/jfabienke/xgecu-pro) project confirmed on real hardware that it "measured nothing and corrupted every read." A true T76 contact test would require a dedicated FPGA bitstream that XGecu has never written.
      - TL866A/CS: **not supported** — upstream does not implement pin test for TL866A/CS. Our Rust code returns `UnsupportedOperation` (default trait impl). The C `minipro` source does not define the bit-banging commands (0x2D-0x36) for TL866A/CS.
      - T56: **not supported** — upstream does not implement pin test for T56. Our Rust code returns `UnsupportedOperation` (default trait impl). The C `minipro` source does not define the bit-banging commands (0x2D-0x36) for T56, and the T56 defines the same `0x3E` command as the T76 but has no `t56_pin_test` function. It may share the T76's situation where `0x3E` is an adapter-init step rather than a standalone contact test — this has not been verified on T56 hardware. Anyone with a T56 could investigate.
    - Button disabled for TL866A/CS and T56 with tooltip "Pin test not supported on this programmer model"
    - The test briefly drives ZIF pins — should we warn the user before running? XGPro runs it automatically before operations when "Pin Detect" is checked. We could add an optional "auto pin check before operations" setting later.
    - Bad pins are reported as device pin numbers (1-based), not ZIF socket positions — the diagram maps device pins to ZIF positions using the same logic as `occupiedPins`
  - **Future extension (not in this task):**
    - ~~Optional auto pin-check before read/write/erase operations (XGPro's "Pin Detect" checkbox)~~ — **implemented** in 0.7.1 (unreleased). CLI `-z` now gates subsequent operations. GUI has "Pin Contact Check" checkbox (default on).
    - ~~Pin-contact pre-check before SPI autodetect (upstream optionally runs `minipro_pin_test` on TL866II+ before autodetect — noted in the SPI autodetect roadmap item above)~~ — **implemented** in 0.7.1 (unreleased). Automatic pin check runs on TL866II+/T48 in ZIF mode before autodetect.
  - **Priority: medium** — core logic exists, XGPro has this feature, prevents the most common cause of failed programming (poor contact / misaligned chip). The structured-result refactor is the main effort; the GUI work is straightforward once the backend returns a bad-pin list.
  - **Status:** implemented


