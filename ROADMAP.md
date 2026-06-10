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

## Near-term

- [ ] Hex viewer: go-to-offset navigation
- [ ] File format support: Intel HEX, SREC, JEDEC
- [ ] Batch / queue operations (write + verify)

## Backlog

- [ ] Auto SN_NUM — production programming with auto-incrementing serial numbers
  - Requires: production-mode UI (start value, step, target address), buffer injection, auto-increment on successful write
  - Priority: low — factory/production feature, most hobbyist users don't need it
