# PIC Validation Chip Shopping List

Reference for purchasing PIC microcontrollers to validate the config
word read/write path across all supported programmer models and PIC
config variants.

---

## Background

Each PIC config family varies in:
- **Word width** — 12, 14, or 16 bits
- **Word count** — 1, 2, 4, or 7 config words
- **Protocol ID** — 0x17, 0x18, 0x19, 0x1a, 0x63, 0x64, 0x65
- **Programmer model** — each programmer (TL866A, TL866II+, T48, T56,
  T76) has a separate `write_fuses` implementation, so validation on one
  does NOT cover the others

The goal is to test every combination of word width, word count, and
protocol implementation across all programmer models.

---

## Already Validated

| Programmer | Chip | Config | Width | Words | Proto | Status |
|---|---|---|---|---|---|---|
| TL866A | PIC16F628A | pic_21 | 14-bit | 1 | 0x18 | ✓ |
| TL866A | PIC12F508 | pic_6 | 12-bit | 1 | 0x65 | ✓ |

---

## PIC Config Variant Matrix

| Config family | Width | Words | chip_info | Proto (TL866A) | Proto (II+/T48/T56/T76) | ISP only? |
|---|---|---|---|---|---|---|
| pic_1-8 | 12-bit | 1 | 0x84 | 0x65 | 0x1a | No |
| pic_9-12, 19, 21, 23-26 | 14-bit | 1 | 0x83 | 0x18 | 0x18 | No |
| pic_13 | 14-bit | 2 | 0x83 | N/A | 0x18 | No |
| pic_14, 20, 22 | 14-bit | 2 | 0x83 | 0x63 | 0x18 | No |
| pic_15-18, 27 | 12-bit | 1 | 0x84 | 0x65 | 0x1a | No |
| pic_28-49 | 16-bit | 7 | 0x82 | 0x64 | 0x19 | No |
| pic_50-88 | 16-bit | 4 | 0x85 | 0x17 | 0x17 | Yes (ISP) |

**Note:** pic_13 (PIC12F1822) is NOT in the TL866A/CS database — it only
appears in INFOIC2PLUS and INFOICT76. The TL866A firmware silently
accepts the write packet but does not program config for variant 0xa233.

---

## Remaining Gaps by Programmer

### TL866A / TL866CS

| Priority | Chip | Config | Width | Words | Proto | Package | Why |
|---|---|---|---|---|---|---|---|
| 1 | **PIC16F87** | pic_14 | 14-bit | 2 | 0x63 | DIP18 | Multi-word stride, simplest/cheapest |
| 2 | **PIC18F2420** | pic_35 | 16-bit | 7 | 0x64 | DIP28 | 16-bit width + 7-word stride (most demanding) |
| 3 | PIC18F24J11 | pic_83 | 16-bit | 4 | 0x17 | ISP | ISP-only J-type (needs ICSP adapter) |

### TL866II+ / T48

All PIC config paths need validation — nothing tested yet on this
protocol implementation (`tl866iiplus.rs`).

| Priority | Chip | Config | Width | Words | Proto | Package | Why |
|---|---|---|---|---|---|---|---|
| 1 | **PIC12F508** | pic_6 | 12-bit | 1 | 0x1a | DIP8 | Already have — validates 12-bit on II+ protocol |
| 2 | **PIC16F628A** | pic_21 | 14-bit | 1 | 0x18 | DIP18 | Already have — validates 14-bit on II+ protocol |
| 3 | **PIC12F1822** | pic_13 | 14-bit | 2 | 0x18 | DIP8 | Multi-word on II+ (not on TL866A) — small/cheap |
| 4 | **PIC18F2420** | pic_35 | 16-bit | 7 | 0x19 | DIP28 | 16-bit + 7-word stride |
| 5 | PIC18F24J11 | pic_83 | 16-bit | 4 | 0x17 | ISP | ISP-only J-type (needs ICSP adapter) |

### T56

Same gaps as TL866II+ — separate protocol implementation (`t56.rs`),
same chips reused.

### T76

Same gaps as TL866II+ — separate protocol implementation (`t76.rs`),
same chips reused.

---

## Minimum Chip Purchase

To cover all gaps across ALL programmer models with the fewest chips:

| # | Chip | Config | Width | Words | Est. Cost | Covers |
|---|---|---|---|---|---|---|
| 1 | **PIC16F87** (DIP18) | pic_14 | 14-bit | 2 | ~$2-3 | TL866A multi-word |
| 2 | **PIC12F1822** (DIP8) | pic_13 | 14-bit | 2 | ~$2-3 | II+/T48/T56/T76 multi-word (not on TL866A) |
| 3 | **PIC18F2420** (DIP28) | pic_35 | 16-bit | 7 | ~$5-8 | 16-bit + 7-word on ALL programmers |
| 4 | **PIC18F24J11** (ISP) | pic_83 | 16-bit | 4 | ~$5-8 | ISP-only J-type on ALL programmers |

**Chips already owned (reuse on other programmers):**
- PIC12F508 — 12-bit validation on TL866II+/T48/T56/T76
- PIC16F628A — 14-bit single-word validation on TL866II+/T48/T56/T76

**Total new chips: 4** (about $15-25 total)

The PIC18F24J11 is optional if you don't want to deal with ICSP mode —
it's the only ISP-only chip. Without it, you'd still cover all ZIF-socket
PIC config variants.

---

## Validation Coverage Matrix

After purchasing the 4 chips above, this matrix shows what gets
validated. Each cell represents a (width, words) combination tested on
that programmer's `write_fuses` implementation.

| Programmer | 12-bit/1 | 14-bit/1 | 14-bit/2 | 16-bit/7 | 16-bit/4 (ISP) |
|---|---|---|---|---|---|
| TL866A | ✓ PIC12F508 | ✓ PIC16F628A | ✓ PIC16F87 | ✓ PIC18F2420 | ✓ PIC18F24J11 |
| TL866II+ | ✓ PIC12F508 | ✓ PIC16F628A | ✓ PIC12F1822 | ✓ PIC18F2420 | ✓ PIC18F24J11 |
| T48 | ✓ PIC12F508 | ✓ PIC16F628A | ✓ PIC12F1822 | ✓ PIC18F2420 | ✓ PIC18F24J11 |
| T56 | ✓ PIC12F508 | ✓ PIC16F628A | ✓ PIC12F1822 | ✓ PIC18F2420 | ✓ PIC18F24J11 |
| T76 | ✓ PIC12F508 | ✓ PIC16F628A | ✓ PIC12F1822 | ✓ PIC18F2420 | ✓ PIC18F24J11 |

---

## Test Procedure

For each chip/programmer combination:

1. **Erase** the chip first (some PICs require a bulk erase before
   config writes will commit)
2. **Read config** — should show `0xFFFF` (erased state)
3. **Write** a test value with a safe bit toggled:
   - Avoid code protection bits (CP, CPD)
   - Avoid write-protection bits (WRT)
   - Prefer oscillator (FOSC) or BOR-related bits
   - For multi-word chips, change a different bit in each word to
     confirm they don't alias or overwrite each other
4. **Read config** back — confirm the value persisted
5. **Repeat unchanged writeback** — write the same value again and read
   back to confirm stability
6. For multi-word chips, verify neither word overwrites or aliases the
   other

### Safe test values by config family

| Config | Word | Mask | Safe test value | Bit toggled |
|---|---|---|---|---|
| pic_6 (PIC12F508) | word1 | 0x001F | 0xFFFE | FOSC0 (bit 0) |
| pic_13 (PIC12F1822) | word1 | 0x3FFF | 0xFFFE | FOSC0 (bit 0) |
| pic_13 (PIC12F1822) | word2 | 0x3713 | 0xFEFF | BORV (bit 8) |
| pic_14 (PIC16F87) | word1 | 0x3FFF | 0xFFFE | FOSC0 (bit 0) |
| pic_14 (PIC16F87) | word2 | 0x0003 | 0xFFFE | bit 0 |
| pic_21 (PIC16F628A) | word1 | 0x21FF | 0xFFFE | FOSC0 (bit 0) |
| pic_35 (PIC18F2420) | word1 | 0xFFFF | 0xFFFE | bit 0 |
| pic_35 (PIC18F2420) | word7 | 0x4003 | 0xFFFE | bit 0 |
| pic_83 (PIC18F24J11) | word1 | 0xF4E1 | 0xF4E0 | bit 0 |

---

## Chip Details

### PIC16F87 (DIP18)

- Config: pic_14, 2 config words (14-bit)
- Protocol: 0x63 on TL866A, 0x18 on TL866II+/T48/T56/T76
- Variant: 0x54 (TL866A), 0xb554 (II+/T76)
- Code memory: 0x2000 (8KB)
- chip_id: 0x00000720
- Common, cheap (~$2-3)
- Datasheet: Microchip DS30487C

### PIC12F1822 (DIP8)

- Config: pic_13, 2 config words (14-bit)
- Protocol: 0x18 (TL866II+/T48/T56/T76 only — NOT TL866A)
- Variant: 0xa233
- Code memory: 0x1000 (4KB)
- chip_id: 0x00002709
- Common, cheap (~$2-3)
- Datasheet: Microchip DS41413C

### PIC18F2420 (DIP28)

- Config: pic_35, 7 config words (16-bit)
- Protocol: 0x64 on TL866A, 0x19 on TL866II+/T48/T56/T76
- Variant: 0xa101
- Code memory: 0x4000 (16KB)
- chip_id: 0x00000420
- Common (~$5-8)
- Datasheet: Microchip DS39631C

### PIC18F24J11 (ISP only)

- Config: pic_83, 4 config words (16-bit)
- Protocol: 0x17 (all programmers)
- Variant: 0xa100
- Code memory: 0x3ff8 (16KB)
- chip_id: 0x00001d00
- ISP only — requires ICSP adapter cable
- Common (~$5-8)
- Datasheet: Microchip DS39933C

---

## References

- Database: `data/infoic.xml` — `<configurations>` section defines
  config word counts and masks
- Fuse bit definitions: `gui/src-tauri/src/fuse_defs.rs`
- Protocol implementations:
  - `crates/minipro-core/src/protocol/tl866a.rs`
  - `crates/minipro-core/src/protocol/tl866iiplus.rs` (also T48)
  - `crates/minipro-core/src/protocol/t56.rs`
  - `crates/minipro-core/src/protocol/t76.rs`
- Upstream reference: `tl866a_write_fuses()` in upstream C `minipro`
