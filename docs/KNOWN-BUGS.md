# Known Bugs & Fixes — History

Historical bug fixes that encode non-obvious protocol and platform
requirements. Extracted from AGENTS.md — check here before touching related
code; several fixes document upstream C minipro behavior that must be
preserved.

## XGecu CRC-32 is not zlib CRC-32 — no final complement (fixed)

Upstream `crc_32()` (in `minipro.c`) is a reflected CRC-32 with a
caller-supplied initial value and **no final XOR-out** — it returns the raw
accumulator, whereas the standard `CRC_32_ISO_HDLC` profile complements the
result with `0xFFFFFFFF`. The port "corrected" it to `CRC_32_ISO_HDLC` in
three places, which made every embedded CRC check fail:

- `algorithm.rs::decode_bitstream` — every `algorithm.xml` bitstream was
  rejected with `AlgorithmCrc`, so T56/T76 chip operations silently ran
  with an unconfigured FPGA and returned zeros.
- `t76.rs::firmware_update_t76` / `t56.rs::firmware_update` — firmware
  images always failed the CRC gate (the comments even said "NOT
  inverted", but `finalize()` on the ISO_HDLC profile applies the
  complement anyway).

Call sites where upstream explicitly complements (`~crc_32(...)`, e.g.
`tl866a` firmware check, `tl866iiplus` file CRC) were already correct
under `CRC_32_ISO_HDLC`.

**Fix:** `util::crc_32(data, init)` ports the upstream function verbatim
(no xor-out); the three call sites use it. Do not replace it with
`CRC_32_ISO_HDLC` — the checks will always mismatch.

## T76 level-2 decompression must tolerate short output (fixed)

The T76 `.alg` payload decompresses zero-run-encoded u16 words into a
buffer whose declared size (`ALGO_SIZE_OFFSET`) can be **odd** — e.g.
`SPI25F11` declares 701,145 bytes but the stream emits 701,144. Upstream C
`calloc`s the declared size, writes words without any output-bound check,
and lets trailing slack stay zero (it even overflows the odd byte by one
word). The port required `out_pos == algo_size` *and* stopped at
`out_pos + 2 <= algo_size`, so it both dropped the final word and then
rejected the stream.

**Fix:** `level2_decompress` rounds the buffer up to even, decodes the
whole stream, then truncates to `algo_size`. Unwritten tail bytes stay
zero exactly like the upstream `calloc` slack. Do not re-add an
exact-length assertion — real `.alg` files legitimately end a few bytes
short.

## Custom database directory startup fallback

If the user sets a custom database directory in Settings and later moves or
deletes that directory, the app cannot find `infoic.xml` / `logicic.xml` at
the saved path on next launch. The startup code in `lib.rs` checks whether
both files exist in the saved `customDbDir`; if not, it sets
`db_dir_invalid` on `AppState`, logs a warning to stderr, and falls back to
the standard search paths (CWD, exe dir, `MINIPRO_HOME`, platform data
dirs, Tauri resources). The GUI reads `get_db_status` when opening Settings
and shows an amber warning if `active` is false, prompting the user to
browse for a new directory or reset to default. No popup or modal is shown —
the warning is inline in the Settings panel only.

## `selectedDevice` store held string instead of object

`DeviceSelector.svelte` was doing `selectedDevice.set(name)` (a string), but
the store is typed as `DeviceInfo | null`. Fixed by storing the full
`DeviceInfo` object: `selectedDevice.set(selectedInfo)`.

## DeviceSelector search results collapsed to just the selection (fixed)

An `$effect` in `DeviceSelector.svelte` watches `$selectedDevice` and sets
`searchQuery = dev.name` so devices selected *externally* (e.g. from
IdentifyResults) appear highlighted in the list. But the effect also fired
for selections made *inside* the component: `onSelect` →
`selectedDevice.set()` → effect overwrote `searchQuery` with the full
device name → the debounced re-search returned only that device,
collapsing the results list.

**Fix:** `onSelect` sets `lastSyncedDevice = name` before calling
`selectedDevice.set()`, marking the selection as already synced so the
effect skips the `searchQuery` rewrite. External selections still sync.
Preserve the `lastSyncedDevice` guard — removing or reordering it
reintroduces the bug.

## T48 rejected at connect — firmware minimum used TL866II+ scale (fixed)

`MiniproHandle::open` applied the TL866II+ minimum firmware (`0x255`,
version 4.2.85) to the T48, but the T48 runs its own firmware line —
latest `0x127` (01.1.39) — so every real T48 was refused with
`FirmwareTooOld`. The original check shared one minimum across
TL866II+/T48/T56/T76; T56 and T76 got their own constants later but T48
was left grouped with TL866II+.

**Fix:** `MIN_FIRMWARE_T48` in `protocol/t48.rs` is `0` (no hard floor),
matching upstream C minipro, which compares firmware against a per-model
*expected* version (`0x127` for T48) and only warns — it never refuses to
connect. Do not reintroduce a nonzero T48 floor without verifying a real
command-set dependency; firmware numbers are not comparable across models.
TL866A/CS likewise has no enforced floor (`MIN_FIRMWARE_A` is unused).

## `minipro -t` panic on T48 — hardware check used TL866II+ pin domain (fixed)

`T48Protocol` was a plain re-export of `Tl866iiPlusProtocol`, so the
hardware self-test ran with TL866II+ assumptions: byte-per-pin state reads
(`read_buffer[7 + pin]` for pins 1..21) and TL866II+ pin tables. The T48
returns ~16 bytes from READ_PINS with pin states **bit-packed** at
`msg[8 + pin/8]`, so every VPP pin reported "Bad" and pin 9 panicked
(`len is 16 but the index is 16`).

**Fix:** `Tl866iiPlusProtocol` now carries an `is_t48` flag
(`T48Protocol::new_t48()`); `hardware_check` branches to a port of
upstream `t48_hardware_check` in `protocol/t48.rs` — T48 pin tables (17
VPP / 34 VCC / 35 GND entries with byte offsets 8–16), `zif_pin_state`
bit-unpacking, `zif_no_io` (unconnected ICSP pins 52/54/56), per-pin
cross-short checks, logic drive tests at both levels, and the OVC tail.
Note the pull-up/pull-down opcodes are **swapped** between models:
T48 `0x31`=pull-ups/`0x32`=pull-downs vs TL866II+ `0x31`=pull-downs/
`0x32`=pull-ups.

**Divergence warning:** upstream still dispatches T48-specific pin-driver
functions that we have not ported (`t48_set_zif_direction/state`,
`t48_get_zif_state`, `t48_set_pin_drivers`, `t48_set_voltages`, the OVC
trip level at `msg[0x34]`). The shared TL866II+ implementation remains in
use for those paths — if ZIF pin state/direction or voltage overrides
misbehave on real T48 hardware, port the corresponding `t48.c` functions
rather than assuming TL866II+ layout. (`t48_begin_transaction` SPI-clock
fields and the read/write payload path have since been ported — see the
next entry.)

## T48 reads/writes > 64 bytes stall — payload must use EP2 only (fixed)

Symptom: `Protocol error: hardware fault or protocol violation` (or
`transfer was cancelled`) on the first "Reading Code" block for any T48
chip whose `read_buffer_size` exceeds 64 bytes (e.g. ATMEGA328P@DIP28 at
`0x100`, BY25Q128@SOIC8 at `0x1000`), while a chip with `read_buffer_size`
`0x40` (ATTINY85) read fine.

**Root cause:** for payloads larger than 64 bytes the TL866II+ firmware
interleaves the transfer across two bulk endpoints (EP2 + EP3), so the
shared code submitted concurrent `bulk_in`/`bulk_out` transfers on both.
The T48 firmware never services the EP3 endpoint during read/write block
operations — upstream `t48_read_block` calls `read_payload2(..., 0)` and
`t48_write_block` calls `write_payload2(..., write_buffer_size, 0)`, where
`limit == 0` means "single endpoint, EP2 only". Pending EP3 transfers on
T48 stall (`Fault`) or sit unsubmitted and get torn down (`Cancelled`).

**Fix:** when `is_t48` is set, `read_block` reads every page type over
EP2-IN only via `read_payload_limit(len, 0)` — including MP_USER, which
the TL866II+ reads over the command endpoint instead — and `write_block`
always sends an 8-byte header on EP1 followed by a `write_buffer_size`
EP2-OUT payload (a short final block is padded; upstream transfers
`write_buffer_size` bytes regardless of `ds->size`). `write_payload_limit`
now treats `limit == 0` as single-EP2-OUT like upstream (`!limit`);
`read_payload_limit` already handled it.

Related fixes in the same area (shared TL866II+/T48 code had drifted
from upstream packet layouts; upstream's `t48.c` and `tl866iiplus.c`
versions of these are byte-identical):

- `read_fuses`/`write_fuses`: `protocol_id` at `msg[1]`, `items_count` at
  `msg[2]`, `code_memory_size` (`- 0x38` on write — upstream notes a
  firmware quirk) at `msg[4..8]`, fuse data at `msg[8]`; the response's
  first 8 bytes are a header, so fuse data is returned starting at
  `resp[8]`. `write_fuses` does not read a response.
- `erase`: `num_fuses` at `msg[2]`, PLD flag at `msg[4]`, 15-byte packet.
- `protect_off`/`protect_on`: command only — the firmware sends no
  response, and reading one could block for the full transfer timeout.
- `get_ovc_status`: `error`=`msg[0]`, `c1`/`c2`=LE16 at `msg[2]`/`msg[4]`,
  `address`=LE32 at `msg[8]`, OVC flag at `msg[12]` (was parsed at wrong
  offsets).
- `read_jedec_row`/`write_jedec_row`: commands are `0x1D`/`0x1E` with
  `protocol_id` at `msg[1]`, row size in bits at `msg[2]`, row at
  `msg[4]`, flags at `msg[5]`; row data at `msg[8]` on write and at
  `resp[0]` on read.
- `read_calibration`: byte count is LE16 at `msg[2..4]` in a 64-byte
  command.
- `unlock_tsop48`: a 48-byte packet — 8 random bytes at `msg[8..16]` with
  a CRC16 packed into `msg[10]`/`msg[12]` and the original bytes echoed to
  `msg[16]`/`msg[17]`; reads an 8-byte status.
- `get_system_info`: the request is a 5-byte command and the response can
  be 80 bytes; T48/T56/T76 use a shifted field layout (`device_code` at
  `resp[24]`, `serial_number` at `resp[32]`, `hardware_version` reported
  as 0) vs TL866II+ (`8`/`16`/`40`).
- `begin_transaction` (T48): `msg[24] = 1` enables the SPI clock override
  at `msg[28]` when `can_adjust_clock` is set — without it `--speed` was
  silently ignored on T48.
- `read_block`/`write_block` command byte for MP_USER is `0x0B`/`0x0A`
  (`READ/WRITE_USER_DATA`), not the fuse `READ/WRITE_USER` (`0x06`/`0x07`).
- `pin_test`, `unlock_tsop48`, and `read_calibration` have no upstream
  T48 handler and now return `UnsupportedOperation` on T48 instead of
  running TL866II+ commands against different hardware — the II+
  pin-test bit-bang commands (0x2D-0x36) target the II+ pin-driver
  domain, which the T48 does not share (the same reason `hardware_check`
  needed its own port). Pin-test surfaces were aligned to TL866II+-only
  accordingly: the `-z` model gate and help text, the GUI Diagnostics
  button, the pre-operation "Pin Contact Check" (default-on — without
  the fix every T48 operation would abort), and the autodetect
  pre-check, which upstream's `auto_detect` also runs on TL866II+ only
  (other models print "not supported" and continue).
  Note this is "unimplemented upstream", not "impossible on T48":
  XGPro's Pin Detect does run on T48 (vendor manual + XGecu forum
  tid=802) via its own single-command protocol — `0x3E` carrying the
  chip ID, returning a per-pin contact bitmap (ZIF1-40 + ISP1-16).
  Documented with USB captures at https://proghq.org/wiki/XGecu_Protocol,
  which is also the best existing reference for the rest of the T48
  pin-control protocol (0x2D-0x36 command maps, 0x29 AUTO_FIND, 0x33
  MEASURE_VOLTAGES, 0x24 adapter DRM). Do NOT conflate T48 `0x3E` with
  T76 `0x3E` — the latter is an unrelated adapter-init command that
  corrupts reads when run standalone (xgecu-pro finding).
- `write_block` (TL866II+ path): upstream always transfers
  `write_buffer_size` payload bytes even when the final block is short
  (it reads ahead into padded file data). We now pad short blocks to
  `write_buffer_size` with `blank_value` before the EP2/EP3 split so the
  transfer length matches what the firmware drains — same fix as the
  T48 path. A `ds->size < write_buffer_size` block ≥ 57 bytes would
  previously have left the firmware waiting for the remainder.
- `read_jedec_row`/`write_jedec_row` put the row size **in bits** at
  `msg[2]`; we currently derive it as `data.len() * 8`, which overstates
  by up to 7 bits when the JEDEC fuse count isn't byte-aligned (upstream
  passes `js->size` verbatim). Harmless today — no caller constructs
  `JedecSet` yet — but if JEDEC fuse-map write support is wired up
  later, thread the exact fuse count through rather than inferring it
  from the packed byte length.

## `do_write` called `erase_chip` before `begin_transaction`

The handle had no active device, so the firmware returned "Protocol error:
no device selected". Fixed by calling `begin_transaction(device)` before
`erase_chip`.

## Global `select-none` prevented text selection

Adding `select-none` to the root app container blocked selection everywhere
including terminal logs. Fixed by only applying it conditionally during
active drag operations.

## `verify_chip` panic when file smaller than device

`verify_chip` read the reference file but did not pad it to device size.
When auto-verify ran after a write with a smaller file,
`expected[offset..]` panicked at offsets beyond the file length. Fixed by
resizing the expected buffer to `size` with blank_value padding, matching
`write_chip` behavior.

## USB sleep/wake Code 10 (Windows)

When a Windows laptop goes to sleep with the programmer connected, the USB
host controller suspends the port. On wake, the WinUSB driver sometimes
fails to re-initialise the device, leaving it in a Code 10 state ("This
device cannot start"). The device shows a yellow triangle in Device Manager
and cannot be opened by the app until physically replugged.

**Root cause:** Windows USB power management (selective suspend). Not a bug
in our code — the device is broken at the OS driver level.

**Workaround for users:**
1. Unplug the USB cable, wait 20-30 seconds, plug it back in
2. Click the reconnect button in the GUI (it retries for ~15 seconds)
3. To prevent recurrence: disable "USB selective suspend" in Windows Power
   Options, or uncheck "Allow the computer to turn off this device to save
   power" for the USB root hub in Device Manager

**App-side mitigation:** `force_reconnect` retries 8 times over ~15 seconds
with increasing delays. The error message instructs the user to unplug,
wait, and replug. The reconnect button tooltip also mentions the 20-30
second wait.

## Voltage display and overrides used wrong lookup tables (fixed)

`VoltagesDto` and `apply_voltage_overrides` in `commands.rs` converted raw
database voltage values using a single hardcoded 16-entry table that only
matched T48/T56 firmware encoding. TL866A and TL866II+ use different
encodings (e.g. TL866A VPP code `0x00` = 12.5V, not 9V), so the GUI showed
wrong voltages for those programmers.

**Fix:** Both now use `minipro_core::device::{vcc_voltage_table,
vpp_voltage_table, voltage_name, lookup_voltage}` which select the correct
table per `ProgrammerModel`. The model is read from
`AppState::programmer_info`; when no programmer is connected, falls back to
TL866II+ tables.

## GUI voltage dropdowns used hardcoded option lists (fixed)

The Advanced voltage override section in `App.svelte` used hardcoded
`VPP_OPTIONS` and `VCC_OPTIONS` constants that only matched the XG
(T48/T56) tables. TL866A and TL866II+ users saw invalid options, logic ICs
showed VPP/VDD dropdowns that can't be used, and T56/T76 custom-protocol
devices showed options when overrides aren't supported.

**Fix:** Added `get_voltage_options` Tauri command in `commands.rs` that
returns `VoltageOptionsDto { vcc, vpp, is_logic }` from the per-model
voltage tables. The frontend `voltageOptions` store in `device.ts` is
loaded via `$effect` in `App.svelte` whenever `$programmer` or
`$selectedDevice` changes. Dropdowns are populated from the backend
response; VPP is hidden when null (logic ICs, custom protocol), VDD is
hidden for logic ICs, and "Voltage overrides not supported for this device"
is shown when both are null. Override values reset to empty on
device/programmer change.

## CLI `--vcc`/`--vdd`/`--vpp` overrides used wrong voltage tables (fixed)

`apply_overrides` in `minipro-cli/src/main.rs` mapped voltage names to
**sequential indices** of a single hardcoded 16-entry table for all
programmer models. On TL866II+ (and TL866A, T48, T56, T76) the firmware
expects model-specific **encoded** values, so overrides sent the wrong
codes — e.g. a `--vcc` sweep on a logic IC produced 5 V on every run
(verified on a scope). Upstream C minipro rejects `--vcc` entirely for
logic ICs (their `vcc_table` is NULL for logic devices).

**Fix:** `minipro-core/src/device.rs` now has the full upstream table set
(`TL866A_*`, `TL866II_*`, `XG_*`, `XG_PLD_VPP`, `T48_BB_*`,
`LOGIC_VCC_VOLTAGES`) plus `vcc_voltage_table()` / `vpp_voltage_table()`
(per-model selection, mirrors upstream `load_device()`) and
`lookup_voltage()` (case-insensitive, tolerates trailing `V` and `.0`).
`apply_overrides` takes the programmer model and validates against these
tables.

**Logic-IC `--vcc` is now supported** (upstream advertises the voltages in
device info but offers no way to select them): valid values are exactly
`1.8`, `2.5`, `3.3`, `5` (encodings `0x03`/`0x02`/`0x01`/`0x00`, sent in
`msg[1]` of the logic-test command). `--vpp`/`--vdd` on logic ICs are
rejected. Caveats: the programmer drives logic inputs at ~3.3 V regardless
of VCC, and its input thresholds don't scale — sub-3.3 V tests are stress
indicators, not conformance tests.

**Related fix:** `build_logic_device` in `database.rs` matched the
logicic.xml `voltage` attribute against `"5"`/`"3.3"` etc., but the XML
stores `"5V"` — every entry fell through to the 5 V default. Now parsed via
`lookup_voltage(LOGIC_VCC_VOLTAGES, …)`; unknown voltages are a hard error.

**Note:** the "Voltage display uses wrong lookup tables" entry above claims
T48/T56 use sequential-index tables; upstream actually assigns the encoded
`xg_*` tables to T48/T56/T76 as well, so that entry's table breakdown should
be revisited when the GUI display bug is fixed.

## CLI warns when VCC override differs from database default (fixed)

When `--vcc` (or `-o vcc=...`) changes VCC away from the database default,
the CLI prints a warning to stderr:
```
WARNING: VCC overridden from 5V to 3.3V; results may be unreliable for this chip.
  The database default is 5V. Reading or blank-checking at a different VCC may produce false results (e.g. all 0xFF).
```
This prevents silent false positives (e.g. blank-checking a 5V EPROM at
3.3V reports "BLANK" because the chip can't power up). The override is
still applied — the warning is informational, not blocking. Logic ICs get
the first line only (no "false results" explanation, since logic tests at
different VCC are intentional stress tests).

## Chip ID read had wrong type byte, endianness, and length (fixed)

`get_chip_id` in all protocol implementations had three bugs compared to
the upstream C minipro:
1. **Wrong type byte**: TL866II+ read `resp[1]` as the ID type; should be
   `resp[0]` (matching upstream `msg[0]`)
2. **Fixed 4-byte ID read**: Always read 4 bytes little-endian. Should read
   `chip_id_bytes_count` bytes (1-4) with endianness based on ID type (LE
   for type 3/4, BE otherwise)
3. **Overly strict minimum length**: Required 6 bytes minimum. Should only
   require `2 + chip_id_bytes_count`

**Impact:** Write operations failed with "Response too short: expected 6
bytes, got 4" on TL866II+ for chips with 2-byte IDs (e.g. 27512@DIP28).
Blank check was unaffected (doesn't read chip ID).

**Fix:** Changed `get_chip_id` trait signature to take `&Device` so
`chip_id_bytes_count` is available. All four protocol implementations
(TL866A, TL866II+, T56, T76) now use the same logic: read `resp[0]` as
type, read `chip_id_bytes_count` bytes from `resp[2..]` with correct
endianness.

## Duplicate `--skip-id` / `--skip-device-id` flags diverged from upstream (fixed)

The CLI had two separate flags for skipping chip ID verification:
- `-x` / `--skip-id` — controlled the top-level `check_chip_id()` call
  before write/read
- `--skip-device-id` — controlled the per-operation `check_device_id`
  parameter passed to `erase_chip`, `write_chip`, `read_chip`,
  `verify_chip`

These looked identical in `--help` but gated different code paths. Passing
`-x` alone skipped the top-level check but per-operation checks still ran.
Passing `--skip-device-id` alone did the opposite. Neither matched
upstream: the C minipro has a single `-x` / `--skip_id` flag that is
**explicitly rejected** in write/erase mode (enforced at `main.c` lines
1062-1067).

**Fix:** Removed `--skip-device-id` entirely. `-x` / `--skip-id` now
controls both code paths (top-level and per-operation). Write and erase
actions with `-x` are rejected with an error message directing the user to
`-y` / `--continue-id` (which reads the ID but warns instead of aborting on
mismatch — matching upstream `--no_id_error`).

## `-y` / `--continue-id` didn't propagate to per-operation checks (fixed)

After consolidating `-x`/`--skip-device-id`, `-y` printed "WARNING: chip ID
mismatch — continuing" at the top-level check, but the per-operation
`check_chip_id` calls inside `write_chip`, `read_chip`, `verify_chip`, and
`erase_chip` still ran and aborted with a hard error. The `continue_id`
flag only gated the top-level check.

**Fix:** Consolidated to a single check point (matching upstream's
architecture). The top-level `check_chip_id` call now covers write, read,
erase, and verify. All per-operation `check_device_id` parameters are
`false` — the top-level check handles `-x` (skip), `-y` (warn + continue),
and the default (error) in one place. This also fixes batch mode:
previously the ID was re-checked for every chip in a batch; now it's
checked once at the start.

**Cleanup (done):** The `check_device_id: bool` parameter has been removed
from the core API (`read_chip`, `read_chip_buffer`, `write_chip`,
`write_chip_bytes`, `verify_chip`, `verify_chip_bytes`, `erase_chip`) and
from `BatchConfig` — callers had all passed `false` since the
consolidation above. The GUI keeps its `check_device_id` *option*, which
gates the explicit pre-operation `check_chip_id` calls (the user's
"skip ID check" escape hatch, analogous to CLI `-x`).

## `erase_chip` didn't check `can_erase` flag (fixed)

`erase_chip` in `operations.rs` unconditionally called
`handle.protocol.erase()` without checking `device.flags.can_erase`.
Upstream minipro checks this flag in `erase_device()` (main.c line 1738)
and silently skips the erase for chips that don't support electrical erase
(e.g. UV EPROMs like the 27512). Our code was sending erase commands to the
programmer for UV EPROMs, which could apply VPP pulses to pins not meant
for electrical erase — undefined behavior and a potential safety issue.

**Fix:** `erase_chip` now checks `device.flags.can_erase` and returns
`Ok(())` early if false. The CLI also checks `can_erase` before showing the
"Erasing..." spinner. For explicit `-E` on a non-erasable chip, the CLI
prints "This chip does not support electrical erase (use UV light for UV
EPROMs)." instead of silently succeeding. For auto-erase before write on a
non-erasable chip, the erase step is silently skipped (matching upstream —
the write proceeds without a pre-erase).

## Favorited device name failed to resolve under a different programmer (fixed)

A favorite saved while a TL866A was connected (e.g. bare `PIC16F628A`)
silently failed `select_device` under a T76 — the T76 database section only
has package-qualified names (`PIC16F628A@DIP18` etc.), so the invoke
rejected and `selectedDevice` stayed null with no visible error. Device
names are not portable across `infoic.xml` sections: the display name is
the record key and naming conventions differ per programmer family.

**Fix:** `check_favorite_devices` (commands.rs) reports per favorite whether
its exact name resolves for the connected model, using a single-pass
`list_devices_by_model()` in `database.rs` (per-section grouping + `pin_map`
model flags). `DeviceSelector` hides favorites that don't resolve; they
stay in `localStorage` and reappear on a compatible programmer. An earlier
approach that auto-selected a same-base-name variant on click was reverted —
substituting `ATMEGA328P` for `ATMEGA328P@DIP28` silently picked a
potentially different package variant and was too surprising.

## TUI editing-state key swallow — search focus dead-ends navigation

Modal/editing blocks in `app.rs::handle_key` return early and swallow
every key without an explicit arm. The search-editing block handled
text-editing keys only, so while `/` was focused, ↑/↓/PgUp/PgDn did
nothing and Enter merely left edit mode — the results list was
reachable only by discovering Esc first. Fix: the search block maps
arrows to `move_cursor` (results navigate while typing, the fzf
convention), Enter exits + activates the highlighted device, and
Tab/BackTab exit the field + cycle panes; Esc remains the pure cancel.
Three distinct exits — commit, move-on, abandon. When adding new
editing/modal states, either route navigation keys deliberately or
document why they're dead — "input focus eats arrows" reads as a hang
to users.

## TUI resize repaint strategy — atomic frames, no throttling

`run()` in `crates/minipro-tui/src/main.rs` repaints on every resize
burst — deliberately **not** throttled or debounced. Three attempts
established that paint timing was never the real problem:

- **Repainting per step "flickered" — but the flicker was write
  tearing, not paint frequency.** `io::Stdout` is a `LineWriter` with
  an ~8KB buffer; a full repaint is tens of KB of escapes, so each draw
  flushed several times mid-frame and the terminal composited a torn
  half-painted screen — perceived as contents shearing vertically. The
  backend now writes through `io::BufWriter::with_capacity(256 * 1024,
  stdout)` — one write per draw, atomic frames. **Do not** revert to
  raw `Stdout` or shrink the buffer below a frame's escape volume.
- **Debouncing (paint only after a quiet gap) is worse**: the OS
  reflows the last frame at the new size, so the screen shows stale
  misaligned content for the whole drag and reads as frozen (tried and
  reverted in e466b6d).
- **Throttling repaint rate causes a subtler artifact**: during rapid
  *shrinking* drags, skipped paints leave a stale too-wide frame on
  screen; Windows Terminal reflow-wraps the overhanging rows (the
  separator/`┤`/keybar reach the last column) and the bottom line
  scrolls off — contents jump up ~1 row until the next paint. Prompt
  repaints keep the stale window smaller than the reflow interval.

Related: both responsive breakpoints (the <80×24 message screen and the
~100-col right-pane collapse) use a dead band in `App::note_size` —
leaving a regime takes a few more cells than entering it. Without
hysteresis, a drag parked on a boundary alternates between structurally
different layouts every repaint, which reads as violent jumping rather
than flicker. The `]` override is cleared only on a real regime
crossing, so it survives a mid-drag wobble across 100 cols.

Two more mid-drag blanking mechanisms are guarded in `run()`:

- **Drain starvation** — an unbounded event drain lets a continuous
  event flood postpone `term.draw` indefinitely; contents "disappear"
  until the stream slows. The drain is capped at ~8 ms so a paint is
  always imminent. Do not remove the deadline.
- **Transient degenerate sizes** — conpty can report a 0-row/col size
  mid-resize; drawing at it resizes buffers and `clear()`s, producing a
  blank frame until the next good size. `run()` checks
  `terminal::size()` itself and skips the draw when either dim is 0.

`MINIPRO_TUI_DEBUG_PAINT` (any value) overlays draws-per-second and the
painted size on the keybar — distinguishes starved repaints (0 d/s
during a drag) from terminal-side compositing artifacts.

## T48 firmware update silently shared the TL866II+ parser — wrong format, always rejected (fixed)

`t48.rs` re-exports `Tl866iiPlusProtocol` as `T48Protocol`, so
`Protocol::firmware_update` on a T48 ran `firmware_update_tl866` — the
`UpdateII.dat` parser, which gates on family `0xf8cc`. Every genuine T48
image (`0xf048` family, 276-byte blocks, no XOR table) failed the version
check with "Unsupported firmware version". Safe-fail — the check precedes
any USB write — but the feature was a stub: upstream has a dedicated
`t48_firmware_update` that was never ported.

**Fix:** `t48::firmware_update_t48(handle, dat, out, progress)` ports
`t48.c::t48_firmware_update` verbatim (16-byte header, `0x114`-byte
blocks, `0xCDEF89AB45670123` bootloader magic @8, `0x3F` reset +
`handle.reconnect(bool)` following the T56 handle-based pattern), and
`operations::firmware_update` dispatches `ProgrammerModel::T48` to it.
The shared trait impl returns `UnsupportedOperation` when `is_t48`,
mirroring T56. Full format + wire spec: `docs/FIRMWARE-FORMATS.md`.

## "Hardware version" is always 0 on T48/T56/T76 — never compare it (fixed)

Upstream `minipro.c` hardcodes `handle->hw = 0` for T48/T56/T76; only
TL866A/CS/II+ read a real hardware byte (sysinfo `msg[40]`). The firmware
display string is `{hw}.{major}.{minor:02}`, so `00.1.03` on a T48 means
firmware **1.03** — the `00` is a constant, not a revision. Downloader
filenames (`firmware-T48-01.1.35.dat`) carry XGecu's product-line label,
so the file's first component is *always* `01` while the device's is
*always* `00`.

The GUI's update-confirm `firmwareVersionNote` treated first-component
mismatch as a "hardware revision differs" warning — it would have fired
on every T48 update, telling users their image was incompatible when
`1.03 → 1.35` is a routine same-line upgrade.

**Fix:** `firmwareVersionNote` compares `{major}.{minor}` (the low u16 of
the file version, which is what the device reports) and only evaluates
the hw-rev warning when the installed side reports a real hw byte
(`installed[0] !== 0` — TL866A/II+).

## T48 `0x3E` pin-detect only works with pin drivers idle (fixed)

The T48 contact bitmap reads **driven-pin state**, not socket contact,
when a transaction is active: a fully seated DIP-8 returns
`0F 00 00 00 F0` (ZIF 1-4 + 37-40, all contacts good) after
`end_transaction`, but `01 00 00 00 C0` inside one — every driven pin
reads open, so good chips report random bad contacts and operations
abort on the pre-op gate. XGPro runs pin detect *before* beginning the
transaction for the same reason.

**Fix:** `pin_test_t48` sends `end_transaction` (0x04), issues `0x3E`,
then re-issues `begin_transaction` so the caller's transaction state is
restored transparently. Do not hoist the `0x3E` send above the end, and
do not run it "inline" to save the re-begin — in-transaction results are
garbage by design. Also confirmed from the same capture: the request's
`chip_id` field is ignored (all values return identical responses); the
bitmap is `resp[8..13]` LSB-first, bit set = contact.

## T76 all-zeros chip ID / reads on one physical chip can be silicon marginality (environmental)

Observed on a pair of ATTINY85V DIP-8 parts: chip A reads
`0x1e930b` and programs fine on T48, T76, and TL866A; chip B reads
`0x0000` chip ID and all-`0x00` data **on the T76 only**, while the
same chip works on T48 and TL866A. Upstream C minipro on the same T76
reproduces the failure identically (`expected 0x1E930B, got 0x0000` —
same `ATTINY_A2` algorithm, FPGA reset OK), so the port is faithful:
the T76's FPGA-driven timing exposes a marginal die that the
bit-banged paths tolerate.

The observed part was genuine (new from an authorized distributor),
so clone/remark is not required for this failure — parametric
outliers ship in genuine lots. Eliminations performed on chip B:
reseat + lead inspection, per-pin ESD-diode DMM test (all pins
healthy ~0.5-0.6 V), `--pulse` stretch to 500 us, and the ICSP
physical path — all failed identically on the T76. Note the
diagnostic flags that *don't* reach this device: `--spi_clock` is
silently ignored (`can_adjust_clock` only arms for SPI-flash
protocols, not AVR `0x1e`), and `--vcc`/`--vdd`/`--vpp` are rejected
(`chip_info=0x81` is not a voltage-adjustable class). `--pulse`
(`msg[12]`) is the only timing knob that flows to the T76 algorithm
for this entry.

Mechanism: same `ATTINY_A2` bitstream, different execution
substrate. The T48/TL866A drive pins from MCU firmware loops with
microseconds of natural slop; the T76 runs the algorithm as an FPGA
bitstream at hardware-clock precision, likely at minimum legal
setup/hold. A die whose HVSP-entry or input-threshold window is
narrower than nominal gets forgiven by the sloppy path and missed by
the precise one — direction is not inherent, a different marginal
unit could fail on T48 and pass on T76.

**Diagnostic rule before suspecting a port bug:** if one unit fails
but a sibling of the same device entry succeeds on the same
programmer and algorithm, and upstream C minipro reproduces the
failure, treat it as chip marginality — reseat, inspect/clean leads,
DMM-compare ESD diode drops, try `--pulse` and the ICSP path, and
swap the chip. A `0x0000` ID with a successful "FPGA Reset OK" means
transport and bitstream are working; the die simply isn't answering
within the programmer's window.

## T76 logic test ran against the wrong FPGA bitstream (fixed)

`T76Protocol::logic_ic_test` reused the TL866 two-pass path verbatim,
which assumes the pull-up/pull-down direction is a runtime flag on one
engine. On the T76 the pull direction is baked into the FPGA
configuration: upstream `t76_logic_ic_test` uploads `TestLgcPull`
before the pull-up pass and `TestLgcDown` before the pull-down pass
(`protocol_id = IC2_ALG_NONE`, util-algorithm variants `0x0300`/`0x0200`
from `algorithm.xml`).

The visible symptom was diagnostic: a good 7408 (verified on T48 and
TL866A) failed every vector expecting `H`. `begin_transaction` had
loaded `Test_100M` (util index 0 — logic devices carry `variant = 0`),
so the 0x28 vector command ran on the SRAM-test engine and outputs
read low. Vectors expecting `L` passed trivially; only the all-high
vector failed — the signature to look for.

The fix orchestrates bitstream upload + vector pass per direction in
`operations::logic_ic_test` (T76 branch), shares `do_ic_test_pass`
with the TL866 path, and calls `protocol.invalidate_bitstream()`
afterwards so a stale `TestLgcDown` isn't mistaken for the next
device's algorithm. T56 needs no per-pass upload — its TTL1/TTL2
bitstream honors the runtime pull flag, so the shared path stays.
