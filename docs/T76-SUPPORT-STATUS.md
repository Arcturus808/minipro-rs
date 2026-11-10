# T76 Programmer Support — Status & Findings

This document captures the current state of T76 support in minipro-rs,
what works, what doesn't, and known issues. It is intended as a
reference for future debugging and development.

## Overview

The T76 (XGPro T76) is the newest XGeku programmer, supporting SPI NOR,
SPI-NAND, parallel NAND, eMMC, and parallel NOR. The protocol was
reverse-engineered from XGPro_T76.exe V13.19 and ported from Matt
Brown's `t76-improvements` branch of C minipro.

**Hardware validation (2026-11):** SPI NOR chip ID, read, erase, program
and verify tested on a real T76 (firmware 00.1.18). W25X20CL passed the
full pipeline — ID `0x00ef3012`, byte-identical read vs TL866A, and a
256 KB write+verify with zero diffs. PM25LV010 ID/read also worked
(`0x00009d7c`) — though that specific chip later showed marginal/degrading
output-driver behavior on *both* programmers (non-deterministic reads;
a silicon issue, not a code issue). LE25FS406M@SOP8 (SOP8→DIP adapter)
reads deterministically and programs 2047/2048 pages; its last page
erases but refuses to program — a chip-specific quirk (works fine on
W25X20CL), not a write-path bug. SPI autodetect (`-a` / GUI "8-pin")
validated on W25X20CL (`0xef3012`, 21 matches) and LE25FS406M@SOP8
(`0x621613`, 5 matches) — both via SOP8→DIP adapters;
the 16-pin path (SPI25F21) is implemented but unvalidated pending a
16-pin SPI chip. All other chip classes remain
unvalidated — the code is correct as far as we can tell from reading the
C source, but hardware testing is required to confirm.

## Chip Class Status

| Chip Class | Protocol ID | Read | Erase | Program | Validated? |
|------------|-------------|------|-------|---------|------------|
| SPI NOR (8-pin) | 0x03 | ✅ | ✅ | ✅ | **Yes — ID, read, write+verify (fw 00.1.18)** |
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
(same chip, same algorithm). Exceptions that *do* reset the flag:
SPI autodetect (it uploads SPI25F11/SPI25F21 outside
`begin_transaction`) and the pin-contact test below — clearing the
flag forces the next selected device's own algorithm to upload.

### Pin contact test (`0x3E`)

`0x3E` is dual-purpose: it reconfigures the socket pin drivers *and*
returns a 48-pin ZIF contact bitmap. Both uses are real — the
adapter-init sequences (NAND ×2, eMMC ×1) run it for configuration,
and its 32-byte reply carries the contact map at bytes 16–21 (LE,
bit *n* = ZIF pin *n*+1). Reply format and the DIP→ZIF mapping
(device pin *d* of an *N*-pin DIP = ZIF pin `d + 24 − N/2`,
bottom-justified) come from upstream MR !304's XGPro captures;
eligibility is plain DIP only (`@DIPn`/`@PDIPn`, *n* even ≤ 40,
`prog_support != MP_ICSP_ONLY`), enforced by
`t76_pin_test_supported`.

**Critical**: the detect leaves the FPGA in pin-sense mode and only a
bitstream re-upload restores chip driving — `END_TRANS` and a plain
`BEGIN_TRANS` don't. Without the forced re-upload (`pin_test_t76`
clears `bitstream_uploaded` before re-beginning, mirroring XGPro's
detect-before-bitstream order) the programmer stays corrupted:
verified on hardware — chip ID read `0x0000fcff` instead of
`0x001e950f`, persisted across ops with the check off, and a second
detect returned all-bad. xgecu-pro's "0x3E corrupts reads, not a pin
test" finding was this mode change misread as garbage. Upstream's
merged `t76_pin_test` never re-uploads the bitstream either, so it
likely wedges the T76 for the next command.

**Validated (fw 00.1.18):** DIP28 pass (ATMEGA328P@DIP28, all
expected pins incl. NC-pin exclusion), DIP8 fault detection
(ATTINY85V@DIP8 with pins 2 + 8 lifted → reported exactly 2, 8).
DIP16/20/24/40 and reversed insertion untested.

### OVC safety check
`MiniproHandle::begin_transaction()` polls `get_ovc_status()` after
FPGA initialization. NAND and eMMC skip this (zeroed 0x39 deselects
the chip). Applies to all programmer models.

### SPI status register (fuse type `0x01`)

For 25-series SPI NOR (`protocol_id 0x03`) the T76 surfaces the chip's
real status register through the fuse commands:

- `READ_CFG` (`0x08`, fuse type `0x01`) → RDSR, returns the SR byte
- `WRITE_CFG` (`0x09`, fuse type `0x01`) → WRSR, arbitrary byte writes
  persist across reads
- `READ_USER` (`0x06`, fuse type `0x00`) returns the same byte — an alias
- `READ_LOCK` (`0x15`, fuse type `0x02`) returns nothing for SPI parts

(On MCU devices fuse type `0x00` instead serves the `user_idN` rows —
`num_uids` devices only; TL866A/CS firmware hangs on it and is gated off
— see `docs/KNOWN-BUGS.md`.)
- `PROTECT_OFF` (`0x18`) clears the SR's block-protect bits

Verified on W25X20CL (fw 00.1.18): writing `0x04`/`0x1C` to fuse `0x01`
read back exactly, and a subsequent `protect_off` reset the register to
`0x00` — proving the register is the on-chip SR, not a firmware shadow.
Exposed as `--spi-status` / `--spi-status-write <BYTE>` in the CLI
(`spi_status_read`/`spi_status_write` in `operations.rs`). The BP/SRP
bits are non-volatile on most parts; `minipro -u` (protect off) clears
BP bits if a write leaves the chip protected — verified on all three
models below, each via its own unprotect opcode.

**Model verification (W25X20CL):**

| Model | Read | Write persists | Data side effects |
|---|---|---|---|
| T76 (fw 00.1.18) | yes | yes | none — `protect_off` (`0x18`) clears the same SR |
| TL866A (fw 03.2.72) | yes | yes | none — incl. cold-boot-first-command; `protect_off` (`0x44`) clears the SR |
| T48 (fw 00.1.35) | yes | yes | none — `protect_off` (`0x18`) clears the SR |
| T56 | same command family — untested | | |
| TL866II+ | same impl as T48 — untested | | |

TL866A detail: `CMD_WRITE_CFG` (`0x13`, fuse type `0x01`) writes the same
physical SR — `0x04` set on the TL866A persisted across processes and was
cleared by a T76 `protect_off`. T48 rides the TL866II+ impl
(`CMD_READ_FUSES`/`CMD_WRITE_FUSES`, fuse type `0x01`).

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

### NAND program commit
Per-block program (`0x1F` init + per-page EP05 packets, 16-byte header
each) is followed by a plain `0x39` REQUEST_STATUS (8-byte send, 32-byte
recv). The firmware cache-programs: page N commits when page N+1's data
arrives, so the last page is left pending until the `0x39` poll — without
it the final page of each block reads back erased (upstream-observed).
The vendor also posts EP83 status reads, but they complete with 0 bytes
(overlapped/aborted telemetry) — upstream's `status_recv()` for EP83
exists but has no call sites; we do not read EP83 here either.

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
| `T76_OVC_LEVEL` | OVC trip level 1–9 fallback when `-o ovc_level` is unset | `5` (~220 mA) |

## Known Issues

1. **Limited hardware validation** — SPI NOR (8-pin) chip ID + read
   and the pin contact test (DIP28 pass / DIP8 lifted-pin) verified on
   real hardware; all other chip classes untested
2. **`algorithm.xml` required, not bundled** — the T56/T76 FPGA ships
   blank, so every chip needs an uploaded bitstream. Without
   `algorithm.xml`, `begin_transaction` now fails hard (matching
   upstream); generate it from the XGPro installer with upstream
   `dump-alg-minipro.bash`
3. **Database is outdated** — V12.91 vs V13.19 (2,028 missing chips)
4. **`eprintln!` logging** — protocol layer doesn't integrate with GUI
   log system (cross-cutting issue, not T76-specific)
5. **`thread_local` eMMC block tracking** — fragile in theory, works in
   practice. Counter resets on `ds.init=true`
6. **Parallel NOR program** — non-functional (upstream limitation)
7. **eMMC partition selection** — only USER partition accessible;
   BOOT1/BOOT2/RPMB need a `--partition` CLI flag
8. ~~**Logic IC test** — T76 needs bitstream reload between
   pull-up/pull-down passes~~ FIXED: `operations::logic_ic_test` now
   uploads `TestLgcPull`/`TestLgcDown` per pass and invalidates the
   bitstream cache afterwards, matching upstream. Verified on a 7408.

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
