# Firmware Update File Formats

Per-model `.dat` layouts, version semantics, and bootloader wire
protocols. Reference for anyone touching `operations::firmware_update`,
the per-model `firmware_update_*` ports, or the GUI's firmware-image
resolution. Sources: upstream `t48.c` / `t56.c` / `t76.c` /
`tl866iiplus.c` / `tl866a.c`, mirrored in
`crates/minipro-core/src/protocol/`.

## Version semantics — read before comparing versions

The LE32 at file offset 0 is `FAMILY << 16 | major << 8 | minor`. The
comparable firmware version is the **low u16** (`version & 0xffff` →
`major.minor`), matching what the device reports as `firmware` (u16).

The three-part display string is `{hw}.{major}.{minor:02}` — but for
T48/T56/T76 upstream **hardcodes `hw = 0`** (only TL866A/CS/II+ read a
real hardware byte, sysinfo `msg[40]`). Consequences:

- `00.1.03` on a T48 means firmware **1.03**; the `00` is a constant.
- The `firmware-<model>-<ver>.dat` filenames carry XGecu's product-line
  label (`01.1.35` for T48, `00.1.17` for T76, `04.2.132` for II+), not
  a hardware revision. For T48 the file prefix is *always* `01` while
  the device *always* reports `00` — a first-component mismatch on these
  models means nothing.
- Compare firmware by the low u16 only; gate hardware-revision warnings
  on models that report a real hw byte (TL866A/II+).

## File layouts

| Field | TL866II+ `UpdateII.dat` | T48 `updateT48.dat` | T56 `updateT56.dat` | T76 `updateT76.dat` |
|---|---|---|---|---|
| version @0 (LE32) | `0xf8cc____` | `0xf048____` | `0x5600____` | `0xf076____` |
| family check | `& 0xffff0000` | same | same | same |
| CRC32 @4 | `~crc_32` over blocks+last+xortable+count | `crc_32(dat[16..], 0xFFFFFFFF)` | same | same |
| block count | @1032 | @12 | @12 | @12 |
| block size | 272 B (16 B hdr + 256 B data) + 2064 B last | `0x114` (276 B) | `0x814` (2068 B) | `0x114` (276 B) |
| total size | `n*272 + 3100` | `n*0x114 + 16` | `n*0x814 + 16` | `n*0x114 + 16` |
| extra | 1024 B XOR table @8..1032 | — | — | — |

TL866A/CS `update.dat` is an entirely different encrypted format
(`tl866a.c`) — not covered by this table. Its final firmware line is
3.2.82 (clone-safe) / 3.2.86 (genuine-only); see the clone warning in
`GUI-INTERNALS.md` and the version table in `ROADMAP.md`.

## Bootloader wire protocols

Common opcodes: `0x3D` switch-to-bootloader, `0x3C` erase, `0x3B`
write, `0x3F` reset (device re-enumerates). Magics are little-endian.

**T48** — upstream `t48.c::t48_firmware_update`, ported as
`t48::firmware_update_t48` (16-byte messages unless noted):

- switch (only if `status == Normal`): `[0]=0x3D`, magic
  `0xCDEF89AB45670123` (8 B) @8 → recv 32, require `msg[0]==0` →
  `0x3F` reset → `reconnect(true)` → require `Bootloader`
- erase: `[0]=0x3C`, magic@8 → recv 32, require `msg[1]==0`
- begin write: `[0]=0x3B`, `[1]=1`, magic@8 (no recv)
- per block: `[0]=0x3B`, `[1]=0`, `[2..4]=0x0114` len, payload@8,
  send `0x11c` → recv 32, require `msg[1]==0`
- trailer: `[0]=0x3B`, `[1]=3`, `[3]=1`, `[5]=0xFF`, `[6]=3`, `[7]=8`,
  `0xCDEF8668` LE @260, send 264 B → recv 32, require `msg[1]==0`
- end write: `[0]=0x3B`, `[1]=2`, magic@8 (no recv) → `0x3F` reset →
  `reconnect(false)` → require `Normal`

**T56** — `t56::firmware_update_t56` (takes `&mut MiniproHandle`, uses
`handle.reconnect(bool)`): magic `0xA578B986` u32 @4; erase/blocks check
`msg[1]==0`; blocks are uniform `0x3B`/`0`/len `0x0814`@2, `0x81c`-byte
messages — no separate last-block/trailer messages.

**T76** — `t76::firmware_update_t76` (best-effort, **no** reconnect —
marked unvalidated): `0x3D`+`0xaa` marker, blocks carry a flash address
@4..8; see `t76.rs` for the full sequence.

**TL866II+** — `firmware_update_tl866` (best-effort, no reconnect):
8-byte headers + separate 256 B payload writes (`write_payload`),
XOR-deobfuscated blocks, `0x39` status polls; see `tl866iiplus.rs`.

## Failure semantics

Every format checks the family mask **before any USB write** — a
wrong-model or corrupt image fails safe. If a flash dies mid-way the
device stays in bootloader mode; `handle.reconnect(true)` accepts
bootloader-mode devices, so re-running the update recovers (matching
upstream, which also accepts a device already in bootloader state by
skipping the switch step).
