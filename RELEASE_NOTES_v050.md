## [0.5.0] - 2026-06-27

### Added

- **Algorithm XML parser for T56/T76** — parses `algorithm.xml` to load FPGA bitstream algorithms required by T56/T76 programmers. Computes algorithm names from `protocol_id` + `variant` (with special cases for ATmega ICSP, AT89C ICSP, eMMC voltage, reversed packages, and logic chips), base64-decodes and gunzips bitstreams, verifies CRC32, and performs T76 level-2 zero-run decompression. Integrated into `MiniproHandle::begin_transaction` — automatically looks up the algorithm when a T56/T76 device needs one
- **T56 firmware update** — ported from C `minipro` `t56_firmware_update()`. Handles file version/CRC validation, bootloader magic switch, erase, block-by-block reflash (0x814-byte blocks), and reset back to normal mode. Routed through `operations::firmware_update()`
- **eMMC partition selection for T76** — `T76_EMMC_PARTITION` env var selects eMMC hardware partitions: `user` (default), `boot1`, `boot2`, `rpmb`. Uses CMD6 SWITCH to set EXT_CSD[179] PARTITION_CONFIG. Capacity detection uses the correct EXT_CSD field per partition: SEC_COUNT[212] for USER, BOOT_SIZE_MULT[226] for BOOT, RPMB_SIZE_MULT[168] for RPMB
- **T76 OVC status for NAND/eMMC** — `Protocol::get_ovc_status` now takes `&Device` parameter. T76 implementation repacks chip-parameter header (protocol_id, variant, voltages, chip_info, pin_map) into the 0x39 status request for NAND/eMMC, mirroring vendor behavior. A zeroed 0x39 deselects the NAND; the repacked header keeps it selected. OVC checks enabled in `begin_transaction` and `check_ovc`

---

## ⚠️ Community Hardware Testing Needed

These features are **implemented but not yet validated on real hardware**. If you have a T56 or T76 programmer, please help test:

### Testing Priorities

1. **T56/T76 FPGA algorithm loading** — place an `algorithm.xml` file (extracted from XGPro using `dump-alg-minipro.bash`) next to the `minipro` executable or use `--algorithms PATH`. Try programming an SPI NOR or other FPGA-based chip that previously failed with a protocol error.

2. **T56 firmware update** — use `minipro -F updateT56.dat` with a T56 programmer. The bootloader is preserved, so recovery is usually possible by retrying if something goes wrong. **Do not disconnect during the update.**

3. **T76 eMMC partition selection** — set `T76_EMMC_PARTITION=boot1` (or `boot2`, `rpmb`) before running minipro to program an eMMC boot partition. Default is `user`. Verify that the detected capacity matches the partition size (boot partitions are typically 128KB × BOOT_SIZE_MULT).

4. **T76 NAND/eMMC overcurrent protection** — the OVC status check now works for NAND and eMMC (previously skipped). If you have a short circuit or wrong adapter, the programmer should now report overcurrent instead of silently failing.

### How to Report Results

Open an issue on [GitLab](https://gitlab.com/arcturus8081/minipro-rs/-/issues) or [GitHub](https://github.com/Arcturus808/minipro-rs/issues) with:
- Programmer model (T56 or T76)
- Chip name and package
- What operation you tried (read/write/erase/firmware update)
- What happened (success, error message, unexpected behavior)
- Whether the same operation works with XGPro

---

## Installers

- **Linux**: `.deb` (Debian/Ubuntu)
- **Windows users**: Download `.msi` / `.exe` from GitHub Releases — native MSVC builds scan clean. The GitLab Windows binaries are cross-compiled and may trigger AV heuristics.

## CLI Binaries
Standalone `minipro` CLI binaries for Linux and Windows are attached below.

See [CHANGELOG.md](https://gitlab.com/arcturus8081/minipro-rs/-/blob/main/CHANGELOG.md) for full history.
