# Vintage EPROM / Parallel-Memory Validation Chips

Reference for selecting EPROMs, EEPROMs, and flash parts to validate the
parallel-memory read/write/erase path across all supported programmer
models. Companion to `PIC-VALIDATION-CHIPS.md` (which covers the
config-word/fuse path).

Data source: bundled `data/infoic.xml` (XGPro V12.90 for
INFOIC/INFOIC2PLUS, XGPro_T76 V12.91 for INFOICT76). If the database is
regenerated, re-verify before relying on this document.

---

## Background

`infoic.xml` contains three database sections:

| Section | Programmer models | Filtering |
|---|---|---|
| `INFOIC` | TL866A, TL866CS | none (all entries apply) |
| `INFOIC2PLUS` | TL866II+, T48, T56 | `pin_map` high bits: 0x1=T56, 0x2=II+, 0x4=T48 (no flag = all three) |
| `INFOICT76` | T76 | none |

See `device_matches_model()` in `crates/minipro-core/src/database.rs`.

### Per-model voltage ceilings (`device.rs`)

| Model | Vpp range | Vcc range |
|---|---|---|
| TL866A/CS | 10–21V | 3.3–6.5V |
| TL866II+ | 9–18V | 3.3–6.5V |
| T48 / T56 / T76 | 9–25V | 1.2–6.5V |
| T76 PLD devices only | 9–18V | — |

Note that although T48/T56/T76 hardware reaches 25V, the database itself
never requests >18V for INFOIC2PLUS devices — NMOS EPROM entries are
programmed at 18V there vs 21V on TL866A/CS.

### Reading this document

"✓" below means **the device exists in that model's database section** —
it is a validation *candidate*, not a verified result. Some entries
(e.g. TMS2716, a three-rail part) may have caveats that only hardware
testing will resolve. "–" means absent from the database; such parts are
still useful as negative tests (clean "not supported" error paths).

---

## Vintage UV EPROM coverage

| Part | Capacity | TL866A/CS | TL866II+ | T48 | T56 | T76 | Notes |
|---|---|---|---|---|---|---|---|
| 2708 | 1K×8 | – | – | – | – | – | Not in DB on any model. Negative-test candidate. |
| TMS2716 | 2K×8 | ✓ @21V | ✓ @18V | ✓ | ✓ | ✓ | `TMS2716@DIP24` everywhere. TI three-rail part — verify on hardware. |
| 2716 (M2716, AM2716…) | 2K×8 | ✓ @21V | ✓ @18V | ✓ | ✓ | ✓ | NMOS parts spec'd to 25V are programmed at 18V on newer models — marginal for stubborn silicon. |
| 2532 / TMS2532 | 4K×8 | – | – | – | – | – | Absent (TI non-JEDEC pinout). Negative-test candidate. |
| 2732 / 2732A | 4K×8 | ✓ @12.5–21V | ✓ @12–18V | ✓ | ✓ | ✓ | ~35 vendor entries. |
| 2764 / 2764A | 8K×8 | ✓ | ✓ | ✓ | ✓ | ✓ | ~45 entries; NMOS at 18V (newer) / 21V (A/CS). |
| 27128 / 27128A | 16K×8 | ✓ | ✓ | ✓ | ✓ | ✓ | |
| 27256 / 27C256 | 32K×8 | ✓ | ✓ | ✓ | ✓ | ✓ | Deepest coverage (~200 entries). The default validation part. |
| 27512 / 27C512 | 64K×8 | ✓ | ✓ | ✓ | ✓ | ✓ | |
| 27C010 / 27C020 | 128/256K×8 | ✓ | ✓ | ✓ | ✓ | ✓ | |
| 27C040 / M27C4001 | 512K×8 | ✓ | ✓ | ✓ | ✓ | ✓ | |
| 27C080 / M27C801 | 1M×8 | ✓ | ✓ | ✓ | ✓ | ✓ | |
| 27C800 / M27C160 / M27C322 | 1–2M×16 (DIP42) | – | – | ✓ | ✓ | ✓ | `pin_map` flags T48/T56 only; absent for II+ and A/CS. Exercises the >40-pin path. |
| D2364C (mask ROM) | 8K×8 | ✓ read-only | ✓ | ✓ | ✓ | – | Bit-bang protocol; for dumping C64 KERNAL ROMs. Read-only — no write path. |
| Bipolar PROMs (82S123/126/129/130/131/135/141/147/180/181, 74S570/571) | ≤2K×8 | ✓ bit-bang | ✓ bit-bang | ✓ bit-bang | ✓ bit-bang | – | All use `custom_protocol`. **None exist in INFOICT76.** |

## Modern replacement-part coverage

These are the electrically erasable parts the retro community substitutes
for UV EPROMs; all are ordinary memory devices in the DB.

| Part | Typical use | TL866A/CS | TL866II+ | T48 | T56 | T76 |
|---|---|---|---|---|---|---|
| AT28C16 / 28C17 | 2716-class EEPROM | ✓ | ✓ | ✓ | ✓ | ✓ |
| AT28C64 | 2764-class EEPROM | ✓ | ✓ | ✓ | ✓ | ✓ |
| AT28C256 | 27256-class EEPROM | ✓ | ✓ | ✓ | ✓ | ✓ |
| 28C010 / 28C011 | 27C010-class EEPROM | ✓ | ✓ | ✓ | ✓ | ✓ |
| W27C512 / W27E512 | 27512 drop-in; C64 mask-ROM adapters | ✓ | ✓ | ✓ | ✓ | ✓ |
| W27C257 / W27E257 | 27256 EEPROM-pinout | ✓ | ✓ | ✓ | ✓ | ✓ |
| W27C010/E010, W27C020/E020 | 27C010/020 drop-in | ✓ | ✓ | ✓ | ✓ | ✓ |
| W27C040 / W27E040 | 27C040 drop-in | ✓ | ✓ | ✓ | ✓ | ✓ |
| W27C080 | 27C080 | – | – | – | – | – |
| SST39SF010/020/040 | 5V flash | ✓ | ✓ | ✓ | ✓ | ✓ |
| SST39SF512 | 512K flash | ✓ | ✓ | ✓ | ✓ | ✓ |
| AM29F010 | 1Mbit flash | ✓ | ✓ | ✓ | ✓ | ✓ |
| AM29F020 | 2Mbit flash | – | – | – | – | – |
| AT49F020 | 2Mbit flash (equivalent) | ✓ | ✓ | ✓ | ✓ | ✓ |
| AM29F040 / AT49F040 | 27C040 w/ adapter | ✓ | ✓ | ✓ | ✓ | ✓ |
| AM29F032B | M27C322 replacement (SOP44/TSOP40) | – | – | – | ✓ | ✓ |
| AT27C256R | Still-produced OTP; PROM-eliminator donor | ✓ | ✓ | ✓ | ✓ | ✓ |

---

## Edge cases that make good validation targets

1. **Vpp ceiling asymmetry.** The same NMOS 2716/2732/2764 is programmed
   at 21V on TL866A/CS but 18V on II+/T48/T56/T76. A genuine NMOS part
   exercises both paths and the `--vpp` override validation.

2. **`pin_map` model gating.** DIP42 parts (27C800, M27C160, M27C322)
   are T48/T56-only in INFOIC2PLUS — verify `find_device` rejects them
   cleanly on a connected TL866II+ with the "use a different model"
   error path.

3. **Bit-bang / `custom_protocol` devices.** All 82Sxxx/74Sxxx PROMs and
   the D2364C mask ROM use it. None exist in INFOICT76 — a T76 should
   report them as unsupported. On T56/T76 custom-protocol devices have
   no voltage-override table; verify overrides are rejected.

4. **Absent devices.** 2708, 2532, W27C080, AM29F020 are nowhere in the
   DB — good cases for the `DeviceNotFound` error path (and for
   confirming `find_device_any` exhausts all model sections).

5. **Read-only device.** D2364C has no write algorithm — verify write
   attempts produce a clean error rather than a protocol failure.

6. **Flash command sets.** 29F/39SF/49F parts use JEDEC byte-program +
   sector erase — a different code path from EPROM pulse programming.
   At least one flash part should be in the validation set.

---

## Suggested shopping list

Cheap, common parts that together exercise every branch above:

| Part | Why | ~Cost |
|---|---|---|
| 27C256 or 27256 (any vendor) | Baseline EPROM path; deepest DB coverage | $1–2 |
| AT28C256 | EEPROM path; byte-write, no Vpp needed | $5–15 |
| W27C512 | EPROM-pinout EEPROM; the community's adapter part | $2–3 |
| SST39SF040 or AM29F040 | JEDEC flash command set | $2–4 |
| M2716 or TMS2716 | Vpp ceiling edge case (18V vs 21V) | $3–8 |
| M27C322 or M27C160 (DIP42) | >40-pin handling, T48/T56/T76 only | $5–10 |
| 82S123 or 82S129 | Bit-bang/custom protocol path | $2–5 |

Negative tests (no purchase needed): 2708, TMS2532, W27C080, AM29F020.

---

## Verifying coverage yourself

```bash
# List what the connected programmer's model supports
minipro -l 2716        # all 2716 variants for this model

# Or with an explicit model section search (any connected programmer)
minipro -l 27C800      # empty on TL866II+; populated on T48/T56/T76
```

To re-derive this document after a database update, grep `infoic.xml`
for `name="...PART..."` and record which `<database type="...">` section
each hit falls in (INFOICT76 = lines before the second `<database>`,
INFOIC2PLUS = between 2nd and 3rd, INFOIC = last section), then decode
`pin_map & 0x70000000` for INFOIC2PLUS model flags.
