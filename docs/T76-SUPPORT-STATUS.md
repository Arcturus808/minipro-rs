# T76 Programmer Support — Status & Findings

This document captures the current state of T76 support in minipro-rs,
what works, what doesn't, and known issues. It is intended as a
reference for future debugging and development.

## Overview

The T76 (XGPro T76) is the newest XGeku programmer, supporting SPI NOR,
SPI-NAND, parallel NAND, eMMC, and parallel NOR. The protocol was
reverse-engineered from XGPro_T76.exe V13.19 and ported from Matt
Brown's `t76-improvements` branch of C minipro.

**Nothing has been validated on real T76 hardware.** All code is
correct as far as we can tell from reading the C source, but hardware
testing is required to confirm.

## Chip Class Status

| Chip Class | Protocol ID | Read | Erase | Program | Validated? |
|------------|-------------|------|-------|---------|------------|
| SPI NOR (8-pin) | 0x03 | ✅ | ✅ | ✅ | No |
| SPI NOR (16-pin) | 0x0f | ✅ | ✅ | ✅ | No |
| SPI-NAND | 0x2d | ✅ | ✅ | ✅ | No |
| Parallel NAND | 0x2d | ✅ | ✅ | ✅ | No |
| eMMC (USER) | 0x31 | ✅* | ✅* | ✅* | No |
| Parallel NOR (x16) | 0x12/0x14 | ✅ | ✅ | ❌ | No |

*eMMC: capacity auto-detection from EXT_CSD is implemented but
untested. If EXT_CSD read fails, set `T76_EMMC_SIZE_MB=<size in MiB>`
to bypass it.

## Key Implementation Details

### Bitstream upload caching
The FPGA bitstream (~775KB) is uploaded only once per session
(`AtomicBool bitstream_uploaded`). Subsequent `begin_transaction`
calls skip the upload. For batch operations, this eliminates N-1
redundant uploads.

**Known limitation**: If a different chip requiring a different
algorithm is selected in the same session, the flag is not reset.
This matches C minipro behavior. Not an issue for batch operations
(same chip, same algorithm).

### OVC safety check
`MiniproHandle::begin_transaction()` polls `get_ovc_status()` after
FPGA initialization. NAND and eMMC skip this (zeroed 0x39 deselects
the chip). Applies to all programmer models.

### eMMC capacity detection
eMMC database entries have `code_memory_size="0x200"` (512 bytes), a
placeholder. Real capacity is detected at runtime:
1. If `T76_EMMC_SIZE_MB` env var is set: use it (MiB), skip EXT_CSD read
2. Otherwise: read EXT_CSD via opcode 0x08 (520-byte response), parse
   `SEC_COUNT[212] * 512` for USER capacity

The `Protocol::effective_code_size()` trait method returns the detected
capacity for eMMC and `device.code_memory_size` for everything else.

### eMMC adapter init
`t76_emmc_adapter_init()` (0x24 f0/e0/f1 + 0x3E pin detect) is only
called when `T76_EMMC_SIZE_MB` is NOT set. The full adapter init is
needed for the 0x08 EXT_CSD read to return data. With the env var
override, a lighter path is used.

### eMMC bring-up queries
`t76_emmc_bring_up()` drains three ID query responses (0x21/CID 32B,
0x05/READID 32B, 0x06/user-id 24B) before CMD6 SWITCH partition select.
Skipping these desyncs the USB stream. Response lengths are from one
capture (KLM8G1GEAC) and may not generalize.

### eMMC io_init constants
The 40-byte region init has hardcoded geometry constants (0x200, 0x20,
0x80, 0x20, 0x04, 0x01) from a KLM8G1GEAC capture. These may not
generalize to other eMMC chips.

### eMMC bus width
Timing PRE/POST commands set byte [9] based on `variant >> 8`:
- 0x51 = 1-bit
- 0x54 = 4-bit
- 0x53 = 8-bit

Database entries carry these variants, so bus width selection should
work correctly.

### NAND bad-block check
Per-block erase uses 0x3A bad-block check to skip factory-marked bad
blocks. Bad block count is printed to stderr.

### Parallel NOR program
PROGRAM is non-functional (upstream C has the same limitation). READ
and ERASE may work. The vendor uses a per-command descriptor that
hasn't been reverse-engineered.

## Database

The `infoic.xml` is from XGPro V12.91. The T76 section (INFOICT76)
has ~95 eMMC entries and various SPI/NAND/NOR entries. XGPro V13.19
adds 2,028 new T76 chips. A database refresh requires the upstream
XML data.

eMMC entries all have `code_memory_size="0x200"` — this is a
placeholder. Real capacity is detected at runtime (see above).

## Environment Variables

| Variable | Purpose | Example |
|----------|---------|---------|
| `T76_EMMC_SIZE_MB` | Override eMMC capacity (MiB), skip EXT_CSD read | `4096` for 4 GiB |

## Known Issues

1. **No hardware validation** — all chip classes are untested
2. **`algorithm.xml` not bundled** — users must provide it; without it,
   T56/T76 can't upload bitstreams and most operations fail
3. **Database is outdated** — V12.91 vs V13.19 (2,028 missing chips)
4. **`eprintln!` logging** — protocol layer doesn't integrate with GUI
   log system (cross-cutting issue, not T76-specific)
5. **`thread_local` eMMC block tracking** — fragile in theory, works in
   practice. Counter resets on `ds.init=true`
6. **Parallel NOR program** — non-functional (upstream limitation)
7. **eMMC partition selection** — only USER partition accessible;
   BOOT1/BOOT2/RPMB need a `--partition` CLI flag
8. **Logic IC test** — T76 needs bitstream reload between pull-up/pull-
   down passes (known limitation, reuses TL866II+ two-pass logic)

## Comparison with Matt Brown's t76-improvements branch

### Parity achieved
- 128-byte BEGIN_TRANS with SPI NOR geometry (8P/16P split)
- Three-phase chunked bitstream upload (BEGIN/BLOCK/END, 512-byte packets)
- NAND adapter init (0x24 power/read-id/power-up + 0x3E pin detect x2)
- NAND logic-begin prelude (0x02 with page/block geometry)
- NAND bad-block check (0x3A) and per-block erase (0x0E)
- eMMC adapter init (0x24 f0/e0/f1 + 0x3E)
- eMMC cmd27 tunnel, timing (PRE/POST), io_init (40-byte region init)
- eMMC bring-up queries (0x21, 0x05, 0x06)
- eMMC EXT_CSD capacity auto-detection (SEC_COUNT[212])
- T76_EMMC_SIZE_MB env var override
- Parallel NOR BEGIN extension (x16 family, adapter nibble mapping)
- Firmware update (CRC32, bootloader magic, block flashing)
- OVC safety check in begin_transaction
- Bitstream upload caching (bitstream_uploaded flag)

### Still missing vs C branch
- eMMC partition selection (BOOT1/BOOT2/RPMB via --partition flag)
- eMMC BOOT_SIZE_MULT / RPMB_SIZE_MULT parsing (for non-USER partitions)
- Adapter ID validation (t76_adapter_detect / t76_adapter_compat_check)
- Parallel NOR program (per-command descriptor)

### Intentionally different
- `thread_local` instead of static variable for eMMC block tracking
- `AtomicBool`/`AtomicU64` instead of handle struct fields for
  bitstream_uploaded and emmc_capacity (Rust trait method takes `&self`)
- `Protocol::effective_code_size()` trait method instead of
  `handle->emmc_capacity` field (Rust doesn't allow mutating `&self` in
  trait methods without interior mutability)
