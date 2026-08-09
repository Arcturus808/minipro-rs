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
    sends a READ_ID command (0x24, 0xe4) but discards the response. The
    referenced "Matt Brown branch" cannot be found publicly. Implementing
    this would require reverse-engineering the adapter ID response format
    from XGPro captures with different adapters.
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
    reverse-engineered. Confirmed as a shared limitation: Matt Brown's
    t76-improvements branch also states "parallel-NOR *program* (0x11) is
    still non-functional (needs its own per-command descriptor); read and
    erase work." Requires a vendor write capture to reverse engineer.
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

  ### Hardware validation (separate from code parity)

  These items are not code gaps — the code is written to match the C source
  but has never been tested on real hardware. Validation requires physical
  T56/T76 devices and chips.

  - [ ] T76 SPI NOR (8-pin and 16-pin) — read/erase/program
  - [ ] T76 SPI-NAND — read/erase/program
  - [ ] T76 parallel NAND — read/erase/program
  - [ ] T76 eMMC USER partition — read/erase/program
  - [ ] T76 parallel NOR — read/erase (program known broken)
  - [ ] T56 all chip classes — read/erase/program
  - [ ] T76 firmware update
  - [ ] T76 logic IC test (two-pass with bitstream reload)

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

- [x] **Contextual help overlay for batch/serial panel** — "i" icon next to the Serial Number Injection label opens a modal explaining serial injection, all fields (address, start, step, format, width, endian, checksum), and validation (live preview, overflow detection, blocking errors). Escape listener shared with config help modal.

- [ ] **Fuse bit decoder for config panel** — decode raw fuse bytes into individual named bits with dropdowns/checkboxes
  - **Current state:** config panel shows hex input fields for each fuse/lock byte (e.g., lfuse, hfuse, efuse). User must manually compute bit values from the datasheet.
  - **Goal:** like AVR Studio / Atmel Studio, break out each fuse byte into individual named bits (CKSEL3, CKSEL2, SUT1, SUT0, BODLEVEL, etc.) with human-readable descriptions and dropdowns for multi-valued fields.
  - **Challenge:** the XGPro database (`infoic.xml`) stores fuses as monolithic bytes with a mask and default value — it does NOT break out individual bit fields or provide human-readable names for each bit. A fuse bit database would need to be built or sourced.
  - **Possible approaches:**
    1. Build a static fuse bit database for common AVR/PIC parts (ATmega328, ATtiny85, etc.) — maintain manually
    2. Parse Atmel ATDF (.atdf) or Microchip .pic files for bit-level fuse definitions
    3. Use a community fuse database (e.g., avrdude's fuse definitions)
  - **Scope:** start with AVR (most common use case), expand to PIC later
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
    ICSP pin assignment is not in `infoic.xml` — the `pin_map` field is for ZIF socket pin-contact testing, not ICSP header pinout. The ICSP signal routing is handled entirely in the programmer's firmware: `begin_transaction` sends a single boolean (`icsp: true/false`) in byte 3, and the firmware internally multiplexes the ICSP header pins based on `protocol_id` and chip family. The same physical ICSP pin carries different signals (VPP, VCC, GND, MISO, MOSI, SCK, SDA, CLK) depending on which chip is selected.
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
  - **Status:** Phase 1 complete, Phase 2 scoped (pin-numbering only, no signal labels), Phase 3 not started

- [ ] **GUI voltage override dropdowns** — replace free-text voltage inputs with model-specific dropdowns
  - **Problem:** The GUI Advanced section currently uses free-text VPP/VCC/VDD inputs. While the backend now uses the correct per-model voltage tables (display and override encoding are fixed), free-text inputs still allow users to enter voltages that aren't supported by the connected programmer.
  - **Solution:** Use the per-model voltage tables already implemented in `minipro-core/src/device.rs` (`vcc_voltage_table()`, `vpp_voltage_table()`) to populate dropdowns with only valid values for the connected programmer. For logic ICs, show only the 4-entry logic VCC table (1.8, 2.5, 3.3, 5V) and hide VPP/VDD.
  - **Backend changes:**
    - Expose voltage tables to the frontend via a new Tauri command (e.g. `get_voltage_options`) that returns valid VCC/VPP/VDD values for the connected programmer model and selected device type
    - ~~Update `VoltagesDto` to use the correct per-model table for display strings~~ — DONE
    - ~~Fix `apply_voltage_overrides` to use per-model tables~~ — DONE
  - **Frontend changes:**
    - Replace free-text inputs with `<select>` dropdowns populated from the backend
    - Show the database default as the selected option
    - Disable/hide VPP and VDD for logic ICs
    - Show a warning badge when the user selects a VCC different from the database default (matching the CLI warning)
  - **Files affected:**
    | File | Action |
    |------|--------|
    | `gui/src-tauri/src/commands.rs` | Add `get_voltage_options` command (display + override encoding already fixed) |
    | `gui/src/lib/stores/device.ts` | Add voltage options store |
    | `gui/src/App.svelte` | Replace free-text voltage inputs with dropdowns |
  - **Priority: medium** — the wrong-voltage risk from free-text entry remains; the display bug is fixed
  - **Status:** backend display + override encoding fixed; dropdown UI not started

- [ ] **Remove `check_device_id` parameter from core API** — the per-operation `check_device_id: bool` parameter in `read_chip`, `write_chip`, `verify_chip`, `erase_chip`, `write_chip_bytes`, `verify_chip_bytes`, and `BatchConfig` is now dead weight from the CLI's perspective (the CLI does a single top-level `check_chip_id` call and passes `false` to all per-operation checks). The GUI still uses the parameter for its own pre-operation checks, but also has the same redundancy (calls `check_chip_id` separately AND passes `check_device_id: true` to the operation). Removing the parameter would eliminate the redundancy and simplify the API, but requires updating all GUI command calls in `commands.rs`.
  - **Files to modify:** `crates/minipro-core/src/operations.rs` (remove parameter from all functions), `crates/minipro-cli/src/main.rs` (remove `false` arguments), `gui/src-tauri/src/commands.rs` (remove `check_device_id` field from `OperationOptions` and the per-operation arguments; the GUI's separate `check_chip_id` calls already handle it)
  - **Priority: low** — code cleanup, no user-facing impact
  - **Status:** not started

