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

## Near-term

- [ ] Hex viewer: go-to-offset navigation
- [ ] File format support: Intel HEX, SREC, JEDEC
- [ ] Batch / queue operations (write + verify)
- [ ] **Smart firmware diff** — compare two firmware files ignoring trailing `0xFF` padding
  - Problem: minipro read-back is always full chip size (e.g., 8192 bytes), but source files are often smaller (e.g., 1936 bytes). Simple byte-wise comparison fails even when executable code is identical.
  - Solution: Strip trailing blank bytes (`0xFF`) from both files, then compare remaining content. Report "identical" or first difference with offset/expected/actual.
  - Could extend to "File vs Chip" comparison without requiring an intermediate save.

## Backlog

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
