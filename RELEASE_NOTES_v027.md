## What's New in v0.2.7

### Chip ID Verification
Automatic chip ID read and comparison before every read, write, erase, and verify operation. If the chip in the socket doesn't match the selected device, the operation fails with a clear mismatch message instead of silently writing to the wrong chip. Bypass with `--skip-device-id` / `-S` CLI flag or the "Chip ID check" checkbox in the GUI.

### OSCCAL Calibration Preservation
For PIC microcontrollers with `osccal_save=1` (e.g., PIC12F509, PIC12F683), the factory RC oscillator calibration word is automatically read before erase and restored afterward. This prevents the common mistake of losing clock accuracy after programming.

### Config Panel Improvements
- **Persistent defaults** — fuse and lock bit fields auto-populate from database defaults when a device is selected, so you can edit them immediately without reading the chip first
- **"Read Config from Chip"** merges actual chip values into the existing panel state instead of replacing everything
- **Side-by-side layout** — Fuses and Lock Bits cards are now displayed horizontally for better screen usage

### Manufacturer Column
Device search results now show the manufacturer name (parsed from `infoic.xml`) alongside each part number, making it much easier to distinguish chips like `AT25F512B` (Atmel) vs `MX25L512` (Macronix).

### Bug Fixes
- **Chip ID byte-order normalization** — fixes false mismatch errors on SPI flash chips like PM25LV010 where different programmer protocols pack JEDEC ID bytes at different positions in the response word
