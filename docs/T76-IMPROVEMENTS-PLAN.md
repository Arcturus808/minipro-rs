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
| NAND (0x2d) | ❌ Not implemented | — | |
| eMMC (0x31) | ❌ Not implemented | — | |
| Parallel NOR (0x12/0x14) | ❌ Not implemented | — | |
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
  - `msg[0x40..0x44]` = `0x08000000` (read-setup word)
  - `msg[0x50..0x54]` = `0x00800000` (read-setup word 2)
  - `msg[0x60..0x64]` = `0x0f05172f` (SPI clock config)
  - `msg[0x65]` = `0x03` (SPI clock sub-config)
- These values are verified by USB capture of XGPro V13.19 (fw 00.1.17) reading a ZB25VQ64A.
- For non-SPI-NOR devices, the existing 64-byte path is preserved.

**Risk**: Low — only affects T76 SPI NOR path. Other programmers and T76 non-NOR paths unaffected.

**Testing needed**: Read, erase, program a SPI NOR chip on T76. Verify READID is non-zero.

---

### Phase 2: NAND support (protocol 0x2d)
**Goal**: Add parallel and SPI-NAND read/erase/program.

**Changes**:
1. **`protocol/t76.rs`**:
   - Add 0x02 "logic begin" prelude (64 bytes sent before BEGIN_TRANS for NAND)
   - Implement per-block erase with 0x3A bad-block check
   - Implement 0x1F per-page program with 0x39 commit
   - Add `read_block`/`write_block` paths for NAND page size

2. **`database.rs`**:
   - Add SPI-NAND geometry unpacking: when `chip_type == Nand` and `code_memory_size == 0`, compute real size from packed fields
   - `page_size` field holds BLOCK COUNT
   - `pages_per_block` carries vendor flags in top byte
   - `write_buffer_size` = real page + spare

**Risk**: Medium — touches database parser which affects all programmers.

**Testing**: Read/erase/program a Winbond W29N02GZ (parallel) and GD5F1GM7UEYIG (SPI-NAND).

---

### Phase 3: eMMC support (protocol 0x31)
**Goal**: Add eMMC read/erase/program with partition switching.

**Changes in `protocol/t76.rs`**:
- Implement 0x27 command tunnel (partition switch, status, EXT_CSD)
- Add `--partition` flag mapping (user/boot1/boot2/rpmb)
- Implement 64 KiB block read via 0x0D + EP82 stream
- Implement 64 KiB block program via 0x1F + EP05 stream
- Implement per-group erase via 0x0E (CMD35/36/38)
- Auto-detect capacity from EXT_CSD (SEC_COUNT, BOOT_SIZE_MULT, RPMB_SIZE_MULT)

**Risk**: Medium — new command opcodes (0x27, 0x1F) not used elsewhere.

**Testing**: Read/erase/program a Samsung KLM8G1GEAC-B001 in all partitions.

---

### Phase 4: Database & firmware updates
**Goal**: Update database parser, firmware version, and chip list.

**Changes**:
1. **`protocol/t76.rs`**: Bump `MIN_FIRMWARE_T76` from `0x10D` to `0x111`
2. **`database.rs`**: Add SPI-NAND geometry fix-up (see Phase 2)
3. **`infoic.xml`**: Add 2,028 new T76 chips (or document how to refresh)
4. **`operations.rs`**: Add `--partition` CLI argument for eMMC

**Risk**: Low — version bump is trivial. Database refresh needs validation.

---

### Phase 5: Testing
**Goal**: Validate all chip classes on real T76 hardware.

| Chip Class | Test Chips | Operations |
|------------|-----------|------------|
| SPI NOR | AT25SF128A, W25Q128JV | Read, erase, program, verify |
| SPI-NAND | GD5F1GM7UEYIG | Read, erase, program |
| Parallel NAND | W29N02GZ | Read, erase, program |
| eMMC | KLM8G1GEAC-B001 | Read, erase, program (all partitions) |
| Parallel NOR | Any x16 NOR | Read, erase, program |

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
