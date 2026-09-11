# Known Bugs & Fixes — History

Historical bug fixes that encode non-obvious protocol and platform
requirements. Extracted from AGENTS.md — check here before touching related
code; several fixes document upstream C minipro behavior that must be
preserved.

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

**Remaining cleanup:** The `check_device_id: bool` parameter is still in
the core API signatures (`operations.rs`) and the GUI still passes `true`
to per-operation calls (with its own separate `check_chip_id` calls before
each op). Removing the parameter entirely is tracked in ROADMAP.md.

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
