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
- [x] **Chip ID verification before operations** — automatic chip ID read and comparison before read/write/erase/verify. Fails with clear mismatch message. `--skip-device-id` / `-S` CLI flag and GUI "Chip ID check" checkbox to bypass.
- [x] **No-chip-ID warning** — yellow banner in Read/Write/Verify panels when selected device lacks chip ID support, reminding user to verify correct chip insertion.
- [x] **OSCCAL calibration preservation** — for PIC microcontrollers with `osccal_save=1` (e.g., PIC12F509, PIC12F683), the factory RC oscillator calibration word is automatically saved before erase and restored afterward.
- [x] **Calibration page read** — CLI `-c calibration` now reads the chip's calibration bytes instead of erroring.
- [x] **Persistent Config panel** — auto-populates fuse/lock fields from database defaults when a device is selected. Fields are editable immediately without requiring a chip read first. "Read Config from Chip" merges actual chip values into the panel.
- [x] **Side-by-side fuse/lock layout** — Fuses and Lock Bits cards displayed horizontally next to each other in the Config panel.
- [x] **Manufacturer column in search results** — each device search result shows the manufacturer name parsed from `infoic.xml`, making it easy to distinguish similar part numbers from different vendors.
- [x] **Chip ID byte-order normalization** — fixes false mismatch errors on devices (e.g., SPI flash like PM25LV010) where different programmer protocols pack JEDEC ID bytes at different positions in the response word.

## Near-term

- [ ] Batch / queue operations (write + verify)

## Backlog

- [ ] **Smart firmware diff** — compare firmware files or chip dumps with intelligent trailing-padding handling
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

- [ ] Auto SN_NUM — production programming with auto-incrementing serial numbers
  - Requires: production-mode UI (start value, step, target address), buffer injection, auto-increment on successful write
  - Priority: low — factory/production feature, most hobbyist users don't need it

- [ ] **Manual trim/pad to size** — let advanced users resize firmware files before saving
  - Trim trailing `0xFF` bytes to reduce a read-back (8192 bytes) to actual code size (1936 bytes)
  - Pad with `0xFF` to a specific size (e.g., exact device memory size) for tools that require full-size files
  - Useful when exporting to other tools, version control, or creating "canonical" firmware files
  - Could be a right-click menu in the hex viewer or a Save dialog option

- [ ] **ASCII insert mode in hex editor** — type characters to insert new bytes (shift existing data right)
  - Current behavior: overtype mode (typing replaces existing bytes, file size stays fixed)
  - Insert mode: each typed character grows the buffer by 1 byte and shifts subsequent bytes right
  - Challenges: `Uint8Array` is fixed-size (requires reallocation), virtual scrolling sync on size change, mixed insert/edit operations need ordered operation log instead of sparse map
  - Toggle UI: Insert key, toolbar "OVR/INS" button, or `Ctrl+Shift+I` shortcut
  - Priority: medium — useful for text editing within binary files, but overtype handles most embedded use cases

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
