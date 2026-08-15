# XGPro Database Discrepancies and Known Bugs

This document records discrepancies between the XGPro device database
(`data/infoic.xml`) and Microchip datasheets, along with known bugs reported on
the XGecu user forums.  It explains why certain PIC configuration definitions
were skipped during the `minipro-rs` PIC fuse decoder implementation and
documents the verification methodology used.

## Verification methodology

Every PIC configuration definition in `gui/src-tauri/src/fuse_defs.rs` was
cross-referenced against Microchip datasheets and programming specifications
before being committed.  The process:

1. Extract the config name (`pic_NN`) and fuse mask/defaults from
   `data/infoic.xml`.
2. Identify the chip families using that config via grep.
3. Research the configuration word bit layout in Microchip datasheets.
4. Compare the datasheet bit layout against the database mask.
5. If the mask matches the datasheet, implement the definition.
6. If the mask does **not** match, skip the config and record the discrepancy
   here.

Configs were only implemented when the database mask was consistent with the
datasheet bit definitions.  This avoids propagating XGPro errors into
`minipro-rs`.

## Discrepancies: database masks vs datasheets

### pic_14 — PIC16F87/PIC16F870/PIC16F88

The database assigns `pic_14` to PIC16F87, PIC16F870, PIC16F88, and PIC16LF88.
However, PIC16F87/PIC16F870 and PIC16F88 have **different** configuration word
layouts:

- PIC16F87/870: single 14-bit config word, mask `0x20cf`
- PIC16F88: single 14-bit config word, mask `0x2fcf` (includes CCPMX, WRT bits)

A single `pic_14` config cannot correctly describe both layouts.  PIC16F88 was
implemented separately as `pic_24` (mask `0x2fcf`) and PIC16F88A as `pic_25`
(mask `0x3bff`).  The `pic_14` config is skipped pending per-chip prefix
override research.

### pic_19 — PIC16F84A

The database mask `0x005f` does not match the PIC16F84A datasheet.  The
PIC16F84A has a 14-bit config word with bits 13-4 as CP (code protection),
bit 3 as PWRTE, bit 2 as WDTE, and bits 1-0 as FOSC.  The correct mask should
be `0x3fff` (all 14 bits defined) or at minimum `0x000f` (the lower 4
configurable bits).  The mask `0x005f` (bits 0-3, 4, 6) does not correspond to
any known PIC16F84A bit layout.

**Status:** Skipped.  Needs verification against the actual XGPro programming
behavior to determine whether the mask is a typo or reflects a non-standard
programming convention.

### pic_20 — PIC16F887 (two-word config)

The database defines `pic_20` with two config words: word1 mask `0x3fff`,
word2 mask `0x0700`.  The PIC16F887 datasheet (DS41291) confirms two config
words at addresses 0x2007 and 0x2008, but word2's mask `0x0700` only covers
bit 8 (BOR4V).  Research indicates this may be correct but the full CONFIG2
layout includes additional reserved bits.  Not enough confidence to implement
without verifying the actual programming behavior.

**Status:** Skipped pending further research.

### pic_22 — PIC16F84/PIC16LF84

Similar to `pic_19`.  The database defines `pic_22` with two words (word1 mask
`0x39ff`, word2 mask `0x0043`).  The PIC16F84 uses a single 14-bit config word
at 0x2007 with the same layout as PIC16F84A.  The two-word structure and mask
`0x0043` for word2 do not match the datasheet.

**Status:** Skipped.  The PIC16F84/PIC16F84A share the same config layout per
Microchip migration document DS30072B, but the database treats them
differently.

### pic_26 — PIC16F630 (mask mismatch)

The database mask `0x20cf` for `pic_26` does **not** match the PIC16F630
datasheet.  According to Microchip DS41191D (PIC12F629/675/PIC16F630/676
Memory Programming Specification), PIC16F630 has a config mask of `0x01ff`
(9 bits: CPD, CP, BODEN, MCLRE, PWRTE, WDTE, FOSC2:0).

The mask `0x20cf` actually corresponds to **PIC16F716**, a different chip with
bits CP (13), BORV (7), BOREN (6), PWRTE (3), WDTE (2), FOSC1:0 (1:0).

**Status:** Skipped.  This appears to be a database error where `pic_26` was
assigned the wrong mask.  The actual PIC16F630 config would need mask `0x01ff`.

## Skipped PIC18F configs

### pic_28-31 — Classic PIC18F242/252/258/248

**Status:** Implemented.  These older PIC18F chips use a 3-bit FOSC selection
(FOSC2:0) rather than the 4-bit FOSC3:0 used by newer PIC18F devices.  They
also lack XINST and have different CONFIG3H/CONFIG4L layouts.  Verified against
gputils configuration documentation and Microchip DS39564 (PIC18FXX2),
DS41159 (PIC18FXX8).

- `pic_28` (PIC18F242): 3-bit FOSC, CCP2MX, 2 protection blocks.
- `pic_29` (PIC18F252): 3-bit FOSC, CCP2MX, 4 protection blocks.
- `pic_30` (PIC18F258): 3-bit FOSC, no CCP2MX, 4 protection blocks.
- `pic_31` (PIC18F248): 3-bit FOSC, no CCP2MX, 2 protection blocks.

### pic_32 — PIC18F1220/1320

**Status:** Implemented.  18-pin device with FSCM (not FCMEN), 4-bit FOSC,
MCLRE-only CONFIG3H, and 2 protection blocks.  Verified against gputils
PIC18F1220 configuration page and Microchip DS39636.

### pic_33 — PIC18F2450

**Status:** Implemented.  USB device with PLLDIV/CPUDIV/USBDIV in CONFIG1L,
VREGEN in CONFIG2L, BBSIZ at bit 3, and no CPD/WRTD/EBTRB.  Verified against
gputils PIC18F2450 configuration page and Microchip DS39632.

### pic_37 — PIC18F2480

**Status:** Implemented.  CAN-enabled variant with PBADEN, LPT1OSC, MCLRE in
CONFIG3H (no CCP2MX), BBSIZ at bit 4, XINST, and 2 protection blocks.  Verified
against gputils PIC18F2480 configuration page.

### pic_41 — PIC18F2580

**Status:** Implemented.  Shares the same layout as `pic_37` (PIC18F2480).

### pic_42/43 — PIC18F2515/2525

**Status:** Implemented.  Larger flash devices with CCP2MX, PBADEN, LPT1OSC,
MCLRE in CONFIG3H, XINST, and 3 protection blocks.

- `pic_42` (PIC18F2515): 3 blocks without CPD/WRTD.
- `pic_43` (PIC18F2525): 3 blocks with CPD/WRTD.

Verified against gputils PIC18F2515/2525 configuration pages.

### pic_49 — PIC18F2221/2321

**Status:** Implemented.  Nanowatt-technology device with CCP2MX, PBADEN,
LPT1OSC, MCLRE in CONFIG3H, 2-bit BBSIZ (BBSIZ1:BBSIZ0), XINST, and 2
protection blocks.  Verified against gputils PIC18F2221 configuration page.

### pic_44-48 — Not referenced

Configs `pic_44` through `pic_48` are defined in the database but **no chips
reference them** in `data/infoic.xml`.  These may be placeholder entries or
reserved for future chips.  No implementation is needed.

### pic_50-88 — Not referenced (PIC24/dsPIC configs)

Configs `pic_50` through `pic_88` are defined in the database but **no chips
reference them** in `data/infoic.xml`.  These appear to be reserved for
PIC24/dsPIC families, but the current database does not include any chips that
use them.

Research confirms that PIC24F, dsPIC30F, and dsPIC33F have **fundamentally
different** configuration word layouts from each other and from PIC18F:

- **PIC24F**: 4 flash config words (CW1-CW4) at end of program memory
- **dsPIC30F**: 7 config registers (FBS, FGS, FOSCSEL, FOSC, FWDT, FPOR, FICD)
  at F80000-F8000E, with 2-bit FNOSC
- **dsPIC33F**: 8 config registers (adds FSS), with 3-bit FNOSC and additional
  features like PLLKEN, WINDIS

If PIC24/dsPIC chips are added to the database in the future, these configs
would need family-specific definitions rather than a unified layout.

## PIC18F protection bit position fix

During implementation of the additional PIC18F configs, a pre-existing bit
position bug was identified and corrected in the shared PIC18F protection word
definitions:

- **CONFIG5H (word5):** `CPD` is at bit 7 (packed bit 15) and `CPB` is at
  bit 6 (packed bit 14).  The original implementation had these swapped.
- **CONFIG6H (word6):** `WRTD` is at bit 7 (packed bit 15), `WRTB` is at
  bit 6 (packed bit 14), and `WRTC` is at bit 5 (packed bit 13).  The original
  implementation had `WRTC` and `WRTD` swapped.

This affected all PIC18F configs using the shared `PIC18F_WORD5_2BLK`,
`PIC18F_WORD5_4BLK`, `PIC18F_WORD6_2BLK`, and `PIC18F_WORD6_4BLK` field sets
(`pic_34` through `pic_40`).  The fix was verified against gputils
configuration pages for PIC18F252, PIC18F2480, PIC18F1220, PIC18F2450,
PIC18F2515, PIC18F2525, and PIC18F2221, all of which consistently place CPD
above CPB and WRTD above WRTB above WRTC.

## Known XGPro bugs from user forums

The following bugs were reported on the XGecu Programmer Forums
(http://forums.xgecu.com) and EEVblog.  These confirm a pattern of scattered,
chip-specific issues in XGPro that motivate the `minipro-rs` reimplementation.

### PIC12F629/675 erase bug (tid=1021)

**Reported:** Admin confirmed "There is a BUG with erasure when CP/CPD
programmed, which will be corrected in the next version."

Users reported that the programmer zeros out config words and cannot erase
chips with CP/CPD fuse bits set.  The admin's initial response incorrectly
stated that 0x3FF is the OSCCAL word; the user corrected this (OSCCAL is at
0x90h, 0x3FF contains a RETLW instruction).

### PIC12F629 broken on T48 (tid=1274)

**Reported:** "PIC12F629 is not working with T48, but was working with
TL866II."

Multiple users reported the T48 writes all zeros instead of the correct config
when programming PIC12F629.  One user stated they would have to buy a Microchip
PicKit instead.  The issue persisted across multiple XGPro versions.

### PIC12F509 missing last byte (tid=1147)

**Reported:** T48 with v12.57 stops addressing at 0x3FE, missing the OSCCAL
calibration byte at 0x3FF.

This turned out to be intentional (the last byte is factory-calibrated and
should not be overwritten), but the UI did not communicate this to the user,
causing confusion.

### EEPROM write bug on ATtiny25/24 (EEVblog)

**Reported:** XGPro cannot write EEPROM-only when the EESAVE fuse is asserted.

AVRDUDE handles this correctly.  XGPro's response was essentially "not a
problem, no need to fix."  The issue stems from XGPro not properly handling
the AVR EEPROM auto-erase cycle.

### ATmega328P EEPROM write error on T48 (tid=1085)

**Reported:** EESAVE bit handling broken on T48 specifically; works on
TL866II+.

The T48 fails to write EEPROM when EESAVE is programmed (0).  The workaround is
to first program the high fuse with EESAVE=1 (default), write EEPROM, then
re-program with EESAVE=0.

### SPI Clock Frequency UI bug (tid=20)

**Reported:** "For a long time I have been observing the same bug in different
versions of the program, which manifests itself if the program window is
expanded to full screen. The bug is that from the part of the window called
'IC Config Information' the ability to configure the 'SPI Clock Frequency'
disappears."

This bug recurred across multiple XGPro versions.  One user noted: "the
software for modern programmers has become noticeably lower quality than it was
in the days of the MiniPro TL866CS/A programmers."

### Chips in support list but not in software (tid=1240)

**Reported:** Multiple chips (HY27UU08AG5A, TC58NVG5D2FTA00, TH58NVG5S0ETA20)
appear in the support list but do not appear in the software's chip selector.

### PIC16F18877 not supported (tid=1060)

**Reported:** Users requested PIC16F18877 support; the chip does not appear in
XGPro despite being a common modern PIC.

## Implications for minipro-rs

These findings validate the `minipro-rs` approach of:

1. **Verifying every config against datasheets** rather than trusting the
   XGPro database masks blindly.
2. **Skipping configs with discrepancies** rather than guessing the correct
   layout.
3. **Building a correct, open-source alternative** that fixes these
   chip-specific bugs.
4. **Documenting discrepancies** so users understand why certain configs are
   not yet supported and can contribute verified definitions.

The XGPro database appears to have been compiled with a mix of datasheet
research and empirical testing, leading to inconsistencies between the
recorded masks and the actual chip behavior.  The forum reports confirm that
these inconsistencies translate into real user-facing bugs that persist across
versions.

## Forum bug applicability to minipro-rs

An investigation was conducted to determine whether the XGPro forum bugs
listed above also affect minipro-rs.  The findings are recorded here so future
work has a starting point.

### Likely shared (code-level gap)

**EEPROM write with EESAVE asserted (#4 ATtiny25/24, #5 ATmega328P on T48):**
`write_chip` in `crates/minipro-core/src/operations.rs` writes to the EEPROM
page (0x01) without reading or checking the EESAVE fuse.  The EESAVE bit is
defined in `fuse_defs.rs` for GUI display only — no write-path logic consults
it.  Whether this produces the same failure as XGPro depends on programmer
firmware behavior, which cannot be confirmed without hardware testing.
**Action needed:** Investigate whether EEPROM erase/write cycles should be
gated on EESAVE state, and whether the firmware handles this automatically.

### Partially mitigated

**PIC12F629/675 erase with CP/CPD set (#1):** `erase_chip` in
`operations.rs` includes OSCCAL preservation (read before erase, restore
after), but does not check or handle CP/CPD fuse bits before erasing.
Whether the programmer firmware handles CP/CPD correctly during erase is
unknown without hardware testing.

### Likely not affected

**PIC12F629 broken on T48 (#2):** T48 and T56 share the same protocol code
path in `crates/minipro-core/src/protocol/t56.rs`, which sends the actual
fuse data directly (`msg[8..8+n].copy_from_slice(&data[..n])`).  There is no
T48-specific code path that would zero out config data.  This was likely an
XGPro-specific firmware quirk.

**SPI Clock Frequency UI bug (#6):** minipro-rs does not have an SPI clock
frequency setting in its GUI.  Not applicable.

### Database limitations (not code bugs)

**PIC12F509 missing last byte (#3):** The database sets
`code_memory_size="0x7fe"` for PIC12F509, so minipro-rs inherits the same
addressing limit (stops at 0x3FE).  OSCCAL preservation code exists in
`operations.rs` but depends on `osccal_save` configuration attributes that
are not present in the current `infoic.xml`.

**Chips in support list but not in software (#7):** HY27UU08AG5A exists in
`infoic.xml` and would appear in minipro-rs.  TC58NVG5D2FTA00 and
TH58NVG5S0ETA20 do not exist in the database at all.  The database parser
(`crates/minipro-core/src/database.rs`) does not filter or drop valid chip
entries.

**PIC16F18877 not supported (#8):** PIC16F18877 is not in `infoic.xml`.
Supporting it would require adding a database entry, not a code change.

## References

- XGecu Programmer Forums: http://forums.xgecu.com
- EEVblog microcontroller forum: https://www.eevblog.com/forum/microcontrollers/
- Microchip datasheets (referenced individually in `fuse_defs.rs` comments)
- XGPro database: `data/infoic.xml` in this repository
