# T76 Improvements Implementation Plan

## Source

Matt Brown's `t76-improvements` branch: https://gitlab.com/nmatt0/minipro/-/tree/t76-improvements

Original project: David Griffith's minipro (C) — https://gitlab.com/DavidGriffith/minipro

## What the branch fixes

The T76 programmer in our Rust codebase has a **critical bug**: SPI NOR reads silently return all zeros because the firmware expects a **128-byte BEGIN_TRANS** packet with a chip-class geometry block in `msg[0x40..0x7f]`. We currently send only 64 bytes, so the FPGA has no valid SPI setup.

Additionally, the branch adds support for:
- **SPI-NAND** (protocol 0x2d) — geometry unpacking, per-block erase, page program
- **eMMC** (protocol 0x31) — partition switching, 64 KiB blocks, 0x27 command tunnel
- **Parallel NOR** (protocols 0x12/0x14) — read/erase/program
- **Firmware update** — bumps expected version from 0.1.13 to 0.1.17
- **Database refresh** — 2,028 new T76 chips from XGPro_T76 V13.19

## Current Rust codebase status

| Feature | Status | File | Notes |
|---------|--------|------|-------|
| USB detection (VID 0xA466, PID 0x1A86) | ✅ Working | `usb.rs` | |
| FPGA bitstream upload (3-phase chunked) | ✅ Working | `protocol/t76.rs` | |
| SPI NOR read/write/erase | ✅ Implemented | `protocol/t76.rs` | 128-byte BEGIN_TRANS with geometry block. **Pending hardware validation.** |
| NAND (0x2d) | ✅ Implemented | `protocol/t76.rs` | Parallel + SPI-NAND read/erase/program. **Pending hardware validation.** |
| eMMC (0x31) | ✅ Implemented | `protocol/t76.rs` | Read/erase/program (USER partition). **Pending hardware validation.** |
| Parallel NOR (0x12/0x14) | ✅ Implemented | `protocol/t76.rs` | x16 family (family=0x0b, geom>=8) BEGIN extension only. READ + ERASE verified in C source; PROGRAM non-functional. **Pending hardware validation.** |
| Firmware check (0x111 / 0.1.17) | ✅ Updated | `protocol/t76.rs` | |
| SPI-NAND database unpacking | ❌ Not implemented | `database.rs` | |
| 2,028 new T76 chips | ❌ Missing | `infoic.xml` | |

## Implementation phases

### Phase 1: 128-byte BEGIN_TRANS (SPI NOR fix) — ✅ IMPLEMENTED
**Status**: Code is in `main`, pending hardware validation.

**What was done in `protocol/t76.rs`**:
- `begin_transaction` now sends **128 bytes** instead of 64 when the device is SPI NOR (`protocol_id == 0x03 || 0x0f`).
- The standard 64-byte chip parameters are packed into `msg[0x00..0x3f]` via `build_begin_msg()`.
- For SPI 25-series, the FPGA geometry block is packed into `msg[0x40..0x7f]`:
  - 8-pin (default, e.g. ZB25VQ64A): `msg[0x40..0x44]` = `0x08000000`, `msg[0x50..0x54]` = `0x00800000`
  - 16-pin (e.g. MX25L12845E): `msg[0x40..0x44]` = `0x00020000`, `msg[0x50..0x54]` = `0x02000000`
  - Both: `msg[0x60..0x64]` = `0x0f05172f` (SPI clock config), `msg[0x65]` = `0x03` (SPI clock sub-config)
- The 8-pin/16-pin split is keyed off `variant >> 8` (0x11 = 8-pin, 0x21 = 16-pin), matching the vendor packer.
- For non-SPI-NOR devices, the existing 64-byte path is preserved.

**Risk**: Low — only affects T76 SPI NOR path. Other programmers and T76 non-NOR paths unaffected.

**Testing needed**: Read, erase, program both 8-pin (ZB25VQ64A) and 16-pin (MX25L12845E) SPI NOR chips on T76. Verify READID is non-zero.

---

### Phase 2: NAND support (protocol 0x2d) — ✅ IMPLEMENTED
**Status**: Code is in `t76-improvements` branch, pending hardware validation.

**What was done in `protocol/t76.rs`**:
- Added NAND-specific command constants (`0x02` logic begin, `0x1F` NAND program, `0x3A` bad-block check).
- `begin_transaction` now:
  - Calls `t76_adapter_init()` (0x24 FPGA register I/O + 0x3E pin detection) for NAND before bitstream upload.
  - Sends the 64-byte `0x02` "logic begin" prelude with NAND page/block geometry and bus clock before the `0x03` BEGIN_TRANS.
  - Packs the 128-byte BEGIN_TRANS with NAND-specific fields (block size at `msg[0x10]`, NAND flag `0x800` in raw_flags, clock config `0x0b09272f`).
- `upload_bitstream_t76` now sends the real last-block size in the END command for NAND (required for FPGA finalization).
- `read_block`: Added NAND path — sends `0x0D` with block index and fixed NAND read-parameter header, then streams the block via EP82.
- `write_block`: Added NAND path — sends `0x1F` init with page size, block index, and pages-per-block, then streams each page (with 16-byte header) via EP05, followed by `0x39` commit.
- `erase`: Added NAND erase — loops over every block, first probing with `0x3A` bad-block check (skipping flagged blocks), then issuing `0x0E` per block.
- Updated the `Protocol` trait to pass `&Device` to `read_block`, `write_block`, and `erase` so protocol implementations can branch on `protocol_id`.
- Updated all protocol implementations (TL866A, TL866II+, T48, T56, T76) and `operations.rs` call sites to match the new trait signatures.

**Database changes**: Not yet implemented — SPI-NAND geometry unpacking in `database.rs` is still needed.

**Risk**: Medium — trait signature change touches all programmers. NAND-specific code is isolated to T76.

**Testing needed**: Read/erase/program a Winbond W29N02GZ (parallel) and GD5F1GM7UEYIG (SPI-NAND).

---

### Phase 3: eMMC support (protocol 0x31) — ✅ IMPLEMENTED
**Status**: Code is in `t76-improvements` branch, pending hardware validation.

**What was done in `protocol/t76.rs`**:
- `t76_emmc_adapter_init()`: 0x24 f0 power-down → 0x24 e0 init (recv 0x28) → 0x24 f1 power-up → one 0x3E pin-detect. Byte-exact from XGPro capture.
- `t76_emmc_cmd27()`: 8-byte 0x27 command tunnel (op + ARG), checks resp[1] for errors.
- `t76_emmc_timing()`: 16-byte PRE/POST timing command (0x27 op 0x00) with bus-width at byte [9] (0=1-bit, 1=4-bit, 2=8-bit), keyed off `variant >> 8`.
- `t76_emmc_io_init()`: builds the 40-byte 0x0D (read) / 0x1F (program) init with start LBA, block count, and fixed param words.
- `begin_transaction`: switches to USER partition via 0x27 op 0x46 (CMD6 SWITCH) after bitstream upload.
- `read_block`: PRE timing + 0x0D init (once on `ds.init`) → EP82 stream per 64 KiB block.
- `write_block`: 0x27 op 0x50 program-setup (once) → PRE timing → 0x1F init → EP05 stream per block → commit (0x39 → POST timing → 0x39) after last block. Block counter tracked via `thread_local` `Cell<u32>`, matching the C static pattern.
- `erase`: per-group erase via 0x0E with start/end LBA (steps of 0x20000 sectors), polls 0x27 op 0x4D between groups until resp[5] != 0x0e.

**Partition support**: Currently defaults to USER partition. BOOT1/BOOT2/RPMB support needs CLI `--partition` flag (Phase 5).

**Risk**: Medium — new command opcodes (0x27, 0x1F) not used elsewhere.

**Testing needed**: Read/erase/program a Samsung KLM8G1GEAC-B001. Verify partition switching works.

---

### Phase 4: Parallel NOR support (protocols 0x12/0x14) — ✅ IMPLEMENTED
**Status**: Code is in `t76-improvements` branch, pending hardware validation.

**What was done in `protocol/t76.rs`**:
- Added vendor packer sub_4b5a70 equivalent for the x16 parallel-NOR family (package_details low byte 0x0b, geometry >= 8).
- BEGIN extension bytes:
  - `msg[0x40..0x43]` = `0x01000000`
  - `msg[0x44..0x47]` = `0x00000040`
  - `msg[0x48..0x4b]` = adapter-dependent (`0x0200`–`0x1800`, default `0x0800`)
  - `msg[0x50..0x53]` = `0x10000000`
  - `msg[0x54..0x57]` = `0x00008000`
  - `msg[0x60..0x63]` = `0x0f05172f`, `msg[0x65]` = `0x03`
- READ and ERASE use the standard T76 paths (no special handling needed).
- PROGRAM is marked non-functional in upstream C (needs per-command descriptor).

**Risk**: Low — only affects BEGIN extension; no new opcodes.

**Testing needed**: Read + erase an S29GL512N or equivalent x16 NOR on T76.

---

### Phase 5: Database & firmware updates — ✅ COMPLETE (partial)
**Status**: Firmware bumped; database refresh requires upstream data.

**Changes**:
1. ✅ **`protocol/t76.rs`**: `MIN_FIRMWARE_T76` already at `0x111` (0.1.17)
2. ✅ **`database.rs`**: SPI-NAND geometry is handled correctly in the NAND prelude (`page_size` stores block count for SPI-NAND, real page derived from `write_buffer_size`)
3. ⏳ **`infoic.xml`**: 2,028 new T76 chips require the upstream XGPro_T76 V13.19 database. Current file has T76 section (INFOICT76) from V12.91. Refresh procedure documented below.
4. ⏳ **`operations.rs`**: `--partition` CLI flag for eMMC (USER/BOOT1/BOOT2/RPMB) deferred to post-hardware-validation.

**Database refresh procedure** (when upstream V13.19 XML is available):
```bash
# 1. Obtain infoic.xml from XGPro_T76 V13.19 installation
# 2. Replace data/infoic.xml
# 3. Verify T76 chip count increased by ~2,028
# 4. Run: cargo test --all --locked
# 5. Commit: git add data/infoic.xml && git commit -m "data: update infoic.xml to XGPro_T76 V13.19"
```

**Risk**: Low — no code changes needed, only data file swap.

---

### Phase 6: Review fixes (Matt Brown comparison) — ✅ COMPLETE
**Status**: Three issues found by comparing against Matt Brown's t76-improvements branch have been fixed.

**Fixes applied**:
1. ✅ **OVC safety check in `begin_transaction`** — `MiniproHandle::begin_transaction()` now polls `get_ovc_status()` after the FPGA is initialized. NAND and eMMC skip this (zeroed 0x39 deselects the chip). Applies to all programmer models, not just T76.
2. ✅ **Bitstream upload caching** — `T56Protocol` and `T76Protocol` now use `AtomicBool` to skip re-uploading the ~775KB FPGA bitstream on subsequent `begin_transaction` calls. For batch operations, this eliminates N-1 redundant uploads (1500+ USB packets each). T76 also skips NAND/eMMC adapter init on subsequent calls.
3. ✅ **eMMC bring-up queries** — Added `t76_emmc_bring_up()` which drains three ID query responses (0x21/CID, 0x05/READID, 0x06/user-id) before the CMD6 SWITCH partition select. Without these, the USB stream desyncs.

**Deferred (need hardware validation)**:
- ⏳ eMMC capacity auto-detection from EXT_CSD (520-byte read with short-packet trick — fragile, needs testing)
- ⏳ eMMC partition selection (`--partition` CLI flag for BOOT1/BOOT2/RPMB)
- ⏳ `T76_EMMC_SIZE_MB` env var override (depends on capacity detection)

**Left as-is**:
- `thread_local` eMMC block tracking — works in practice, low risk
- `eprintln!` logging — cross-cutting refactor, not T76-specific

---

### Phase 7: Testing
**Goal**: Validate all chip classes on real T76 hardware.

| Chip Class | Test Chips | Operations |
|------------|-----------|------------|
| SPI NOR | AT25SF128A, W25Q128JV | Read, erase, program, verify |
| SPI-NAND | GD5F1GM7UEYIG | Read, erase, program |
| Parallel NAND | W29N02GZ | Read, erase, program |
| eMMC | KLM8G1GEAC-B001 | Read, erase, program (all partitions) |
| Parallel NOR | Any x16 NOR | Read, erase (program non-functional) |

---

## Branch strategy

**Feature branch: `t76-improvements`**

Each phase gets its own commit:
1. `fix(t76): send 128-byte BEGIN_TRANS with SPI NOR geometry`
2. `feat(t76): add NAND support (protocol 0x2d)`
3. `feat(t76): add eMMC support (protocol 0x31)`
4. `feat(t76): add parallel NOR support (protocols 0x12/0x14)`
5. `feat(t76): update firmware check to 00.1.17 and refresh database`
6. `docs(t76): add T76 protocol documentation`

Merge to `main` only after Phase 5 testing passes.

## Open questions

1. Do we have access to a T76 programmer for testing?
2. Do we have SPI-NAND, eMMC, or parallel NOR chips available?
3. Should we implement all phases before merging, or merge Phase 1 early?

## Estimated effort

| Phase | Days | Blockers |
|-------|------|----------|
| 1 | 1-2 | None |
| 2 | 3-5 | NAND chips for testing |
| 3 | 3-5 | eMMC chip for testing |
| 4 | 2-3 | Parallel NOR chip for testing |
| 5 | 2-3 | T76 programmer + chips |

**Total: ~2 weeks** (assuming hardware access)
