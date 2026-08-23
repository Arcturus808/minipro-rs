# Minipro GUI — Developer Guide

## Tech Stack

| Layer | Version | Constraint |
|-------|---------|------------|
| Svelte | 5.x | **Use Svelte 5 runes exclusively** (`$state`, `$derived`, `$effect`). Do NOT mix with legacy `$:` syntax. |
| Tauri | 2.x | WebView2 on Windows. Native dialogs freeze the JS event loop. |
| Vite | 6.x | Frontend bundler. `npm run build` produces `dist/`. |
| Rust | 1.88+ (GUI) / 1.85+ (CLI) | Backend commands in `src-tauri/src/commands.rs`. |

## Build Commands

```bash
# Fast dev build (reuses cached Rust artifacts)
# ⚠️ Only use when ONLY Rust code changed.
cd gui && npm run build && cargo build --release

# Full production build (embeds fresh frontend into binary)
# Use this when ANY frontend code (Svelte, CSS, JS, HTML) changed.
# Run from the repo root:
cd gui && cargo tauri build

# The `.exe` is at:
# gui/src-tauri/target/release/minipro-gui.exe
```

**Critical rule:** If you change any `.svelte`, `.ts`, `.css`, or `.html` file, you **must** run `cargo tauri build`. `cargo build --release` will keep stale embedded frontend assets from the previous full build.

### GUI development workflows

**Fast iteration (during active UI development):**
```bash
cd gui && npm run dev
```
Starts the Vite dev server with HMR (hot module replacement). Svelte/CSS/TS changes appear instantly in the browser preview without rebuilding the Rust binary. Use this while iterating on layout, styles, or component logic. The Tauri commands won't work (no Rust backend), but visual and state changes are immediate. Each cycle is seconds, not minutes.

**Full verification (after changes are done):**
```powershell
# The running .exe locks the output binary on Windows — kill it first.
Get-Process minipro-gui -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 1
# Run from the repo root:
cd gui && cargo tauri build
Start-Process "gui\src-tauri\target\release\minipro-gui.exe"
```
Embeds the fresh frontend into the Rust binary and launches it for final testing with the real backend. Use this once after the iterative phase is complete, or when you need to test Tauri command integration.

**Rust-only changes (backend, no frontend):**
```bash
cd gui && npm run build && cargo build --release
```
Reuses cached frontend assets. Faster than `cargo tauri build` because it skips the bundler. Only safe when no `.svelte`/`.ts`/`.css`/`.html` files changed.

## Store Patterns (CRITICAL)

### Rule 1: All state lives in writable stores
NEVER use module-level variables for state that components read.

```ts
// BAD — _hexData is invisible to Svelte reactivity
let _hexData: Uint8Array | null = null;
export const hexMeta = writable({ size: 0, path: null });

// GOOD — everything the component needs is in the store
interface HexState {
  data: Uint8Array | null;
  path: string | null;
  size: number;
}
export const hexState = writable<HexState>({ data: null, path: null, size: 0 });
```

### Rule 2: Read store data directly in templates
Do NOT extract store values into local variables. Read `$store.field` directly in `{#each}` and `{#if}` blocks.

```svelte
<!-- GOOD -->
{#each $hexState.data.slice(0, 16) as b}
  <span>{b.toString(16).padStart(2, '0')}</span>
{/each}

<!-- BAD — data may not update reactively -->
<script>
  let bytes = $derived($hexState.data);  // avoid this pattern
</script>
{#each bytes as b}...{/each}
```

### Rule 3: Loading state is part of the store
Keep `loading` in the same store as the data, or in a dedicated companion store. Never infer loading from "data is null."

```ts
export const hexLoading = writable(false);
```

## Tauri / WebView2 Gotchas

### Dialogs freeze the JS event loop
When a Tauri native file dialog is open, the WebView2 thread is paused. **Any reactive update that triggers DOM work during or immediately after the dialog can deadlock.**

**Rules:**
1. Do NOT call `tick()` after a dialog closes.
2. Do NOT update stores that trigger heavy DOM updates (e.g., 16,000-row `{#each}`) inside the same microtask as the dialog close.
3. If you must update stores after a dialog, wrap in `requestAnimationFrame(() => { ... })` or `setTimeout(..., 0)`.
4. Prefer moving `logs.info()` calls **outside** `loadFile()` and into the caller, after the dialog scope has exited.

### DevTools
DevTools must be enabled in **both** places:

```toml
# gui/src-tauri/Cargo.toml
tauri = { version = "2", features = ["devtools"] }
```

```json
// gui/src-tauri/tauri.conf.json
"app": {
  "windows": [ ... ],
  "security": { "csp": null },
  "devtools": true
}
```

Right-click → Inspect and F12 will not work otherwise.

### Tauri window permissions (Tauri 2.x)
Adding `window:default` to capabilities does **not** work. Use specific granular permissions:

```json
"permissions": [
  "core:window:allow-set-size",
  "core:window:allow-center"
]
```

### Tauri v2 command parameter naming
Tauri v2 automatically camelCases top-level invoke keys before matching them to Rust function parameter names. The Rust parameter names must use camelCase to match.

**Rule:** When JS sends `{ snake_case: value }`, Tauri converts it to `camelCase`. Rust params must match that camelCase.

```ts
// JS invoke — Tauri auto-converts keys to camelCase
await invoke("write_fuses", { cfg_fuses: cfg, lock_bits: lock, icsp_mode });
// Tauri converts: cfg_fuses -> cfgFuses, lock_bits -> lockBits, icsp_mode -> icspMode
```

```rust
// Rust handler — parameter names must match camelCase keys
#[tauri::command]
pub async fn write_fuses(cfgFuses: Vec<FuseValueDto>, lockBits: Vec<FuseValueDto>, icspMode: String) { ... }
```

**Note:** This only applies to top-level invoke keys. Nested objects (like `options`) are serialized directly by serde and are not affected by Tauri's key mapping.

**Commands using this convention:**
| Command | JS sends | Rust expects |
|---------|----------|--------------|
| `write_fuses` | `cfg_fuses`, `lock_bits`, `icsp_mode` | `cfgFuses`, `lockBits`, `icspMode` |
| `save_bytes_to_file` | `base64Data` | `base64Data` |
| `do_erase`, `do_blank_check`, `do_chip_id`, `do_logic_test`, `read_fuses`, `check_lock_protection` | `icspMode` | `icspMode` (already camelCase in JS) |

## Data Handling

### Large binary files
- The Rust backend returns file bytes as **base64** strings via `invoke("read_file_bytes")`.
- Convert to `Uint8Array` with `atob()` in the frontend.
- **Do NOT fear Svelte reactivity with 256KB Uint8Arrays.** The browser handles 16,384 `<div>` rows natively. Only optimize with virtual scrolling if profiling shows a real problem.
- When rendering hex rows, use direct array indexing (`data[offset + j]`) instead of `.slice()` inside reactive blocks.

### Base64 encoding large arrays
`String.fromCharCode(...data)` crashes with "Maximum call stack size exceeded" for arrays >65K elements. Chunk the conversion:

```ts
const CHUNK = 0x8000; // 32KB
let result = "";
for (let i = 0; i < data.length; i += CHUNK) {
  result += String.fromCharCode(...data.subarray(i, i + CHUNK));
}
return btoa(result);
```

## Component Conventions

1. **Use Svelte 5 runes exclusively.** Prefer `$state` for local variables, `$derived` for computed values, and `$effect` for side effects. Do NOT use legacy `$:` syntax.
2. **No virtual scrolling until needed.** Start with native `overflow: auto` and `{#each}`. Browser scrolling is highly optimized.
3. **Loading indicators** should be conditional on a dedicated loading store, not inferred from data absence:
   ```svelte
   {#if $hexLoading}
     <Spinner />
   {:else if $hexState.data}
     <HexRows data={$hexState.data} />
   {:else}
     <EmptyState />
   {/if}
   ```

## Hex Viewer Layout

Use `ch` (character-width) units for columns so spacing scales with font size:

```svelte
<!-- Offset column: 8 hex chars + 1ch padding -->
<span style="width: 9ch;">{formatOffset(offset)}</span>

<!-- Hex bytes: 32 chars + 15 spaces = 47ch, rounded up -->
<span style="width: 48ch;">{bytes.map(b => formatHex(b)).join(' ')}</span>

<!-- ASCII: natural width -->
<span>{bytes.map(b => toAscii(b)).join('')}</span>
```

## Hex Viewer Hotkeys

| Hotkey | Action |
|--------|--------|
| Ctrl+S | Save buffer to file (commits pending edits first) |
| Ctrl+C | Copy selected bytes as hex string (uses Tauri clipboard plugin) |
| Ctrl+V | Paste hex bytes from clipboard at cursor (parses hex, C-style, or continuous) |
| Ctrl+A | Select all bytes |
| Ctrl+Z | Undo last edit |
| Ctrl+Shift+Z / Ctrl+Y | Redo last undone edit |
| Ctrl+Home | Jump to first byte |
| Ctrl+End | Jump to last byte |
| Tab | Switch between hex and ASCII panes on the same byte |
| Ctrl+F | Open find dialog (hex or ASCII search) |
| Ctrl+Scroll | Increase/decrease font size (10-16px, 1px per notch) |
| F3 / Shift+F3 | Navigate find matches or diff results (whichever was last activated) |

### Selection model

- **Click** a byte — selects it (amber) and opens the edit input (bright amber)
- **Drag** — selects a range of bytes
- **Arrow keys** — move edit cursor, clear selection
- Selection persists for copy/paste even while editing

### Find vs Diff F3 navigation

F3 navigates whichever mode was most recently activated (`lastNavMode` state).
Running a Find sets `lastNavMode = "find"`. Running a Compare sets
`lastNavMode = "diff"`. Clearing one mode falls back to the other if it has
results. Both sets of highlights can coexist visually (blue for find, red for
diffs), but F3 only moves one cursor at a time.

### Pending edits and dirty flag

- **`hexEdits`** — sparse map of pending byte edits (not yet applied to buffer)
- **`bufferDirty`** — true when the buffer has been modified by Apply, Trim, or
  Pad but not yet saved to disk
- Read and Open operations call `confirmOverwriteEdits()` before replacing the
  buffer. If pending edits or an unsaved buffer exist, a Svelte-based confirm
  modal appears (not a native dialog — avoids WebView2 JS event loop freeze).
- `setHexData()` clears `bufferDirty`. `applyHexEdits()`, `trimTrailing()`, and
  `padToSize()` set it. `saveBufferToFile()` clears it.
- Undo/redo history is cleared by `clearHexEdits()` (called by Apply, Reset, and
  `loadFile`).

### Clipboard

Uses `tauri-plugin-clipboard-manager` (not `navigator.clipboard`) to avoid the
WebView2 clipboard permission prompt. The plugin is registered in `lib.rs` and
the permissions are in `capabilities/default.json`.

### Entropy indicator

Per-row Shannon entropy bar in the gutter between offset and hex columns.
Computed in TypeScript on rendered byte values (works in diff mode too).
Normalized to 0.0–1.0, mapped to four color tiers (green/yellow-green/amber/red).
Toggle via `showEntropyBar` setting in Settings panel (off by default).
Gutter column is always rendered (1ch wide) to avoid layout shift when toggled.

### Help overlays

The hex viewer, config/fuses panel, batch serial injection, and logic test
panel all have help overlays triggered by an "i" icon and/or keyboard
shortcut (?/F1 for hex viewer). Modal with grouped content, dismissed by
Escape, backdrop click, or ✕ button. Global keydown listener handles Escape
(modal doesn't receive focus on open).

The logic test help modal explains the single-character vector symbols
(0, 1, L, H, C, Z, X, G, V) used in the test result table, matching the
XGPro definitions.

### Pin-contact test (GUI)

The Diagnostics panel has a "Pin Test" button that runs the ZIF socket
contact test and highlights bad pins on the ZIF socket diagram. This
matches XGPro's "Pin Detect" feature.

**Model support:** TL866II+ and T48 only. T48 inherits pin test
from TL866II+ via protocol alias (`T48Protocol = Tl866iiPlusProtocol`).
TL866A/CS, T56, and T76 are not supported — button is disabled with a
tooltip. The T76 is FPGA-based and lacks the direct ZIF pin bit-banging
hardware (commands 0x2D-0x36). Its `0x3E` command is an adapter-init
pin-driver configuration step, not a standalone contact test — running
it standalone returns meaningless data and can corrupt subsequent reads.
XGPro itself removed pin detect from the T76 UI. The upstream C minipro's
`t76_pin_test` is also broken (never reads the response, reports every
pin as bad). The xgecu-pro project confirmed on real hardware that it
"measured nothing and corrupted every read."

**Button disabled when:** no programmer connected, no device selected,
device has `pin_map == 0` (no contact-test data in database), ICSP mode
active, unsupported programmer model, or a test is already running.

**Backend:** `do_pin_test` Tauri command in `commands.rs` follows the
existing `try_acquire` / `spawn_blocking` / `take_handle` pattern with
a 10s timeout. Returns `PinTestResultDto { supported, pass, bad_pins,
message }`. Core `Protocol::pin_test()` returns `Result<PinTestResult>`
with `bad_pins: Vec<u16>` (device pin numbers, 1-based, empty = all
good). The CLI `-z` handler prints from the returned struct, preserving
the existing "Bad contact on pin: N" output format.

**Frontend stores:** `pinTestResult` and `pinTestRunning` in
`operations.ts`. `doPinTest()` invokes the Tauri command and logs
results. `clearPinTestResult()` is called in an `$effect` in App.svelte
when `$programmer` or `$selectedDevice` changes, preventing stale
bad-pin highlights from a previous device.

**ZIF diagram highlighting:** `ZifSocketDiagram.svelte` accepts a
`badPins` prop (device pin numbers). Device pins are mapped to ZIF
socket positions using the same logic as `occupiedPins`. Bad ZIF slots
render red with "PIN N" labels. Good occupied slots render green when
the test passes. The "ZIF PIN 1" label is hidden when pin 1 is bad to
avoid overlap with the red "PIN 1" label. Chip pin stubs always use the
chip color (never change for bad/good — only the socket slots change).

**Result panel:** Below the ZIF diagram, a compact panel shows
"✓ All pins OK" (green) or "✗ Bad contact on N pin(s)" (red) with the
pin list and a "Clear" button to dismiss results.

### Config panel state (`$effect.pre`)

`configData` in `App.svelte` is initialized via `$effect.pre` (not `$effect`)
so it refreshes before DOM re-render when `$selectedDevice` changes. Using
`$effect` caused stale fuse names from the previous device to flash briefly
before the effect ran. The effect also has an `else` branch to reset
`configData` to `null` when the device has no MCU config.

### Fuse bit decoder

The config panel shows a bit-level fuse decoder (`FuseBitDecoder.svelte`) when
the `fuseBitDefs` store is non-null. The store is loaded via
`loadFuseBitDefs()` in the same `$effect` that loads voltage options, and
cleared on device deselect. The backend `get_fuse_bit_defs` command looks up
static bit definitions in `fuse_defs.rs` by config name + chip name prefix.
When no definitions exist (e.g., PIC devices, unknown configs), the config
panel falls back to hex-only input. The `DeviceInfoDto` includes a
`config_name` field (the XML `<config name="...">` attribute) that the
frontend uses for the lookup. Bit definitions are sourced from avr-libc
device headers and Microchip datasheets — see `fuse_defs.rs` for the
config-name keying analysis and chip-prefix override logic.

## Project Structure

```
gui/
  src/
    App.svelte                 — main layout, operations panel, draggable splitters
    lib/
      stores/
        hex.ts                 — file data, loading state
        operations.ts          — chip read/write/verify/erase/blank-check/chip-id/logic-test/config
        batch.ts               — batch programming state (chip counter, pass/fail, Next Chip flow, serial number injection config)
        logs.ts                — terminal log entries
        device.ts              — connected programmer + IC database
        settings.ts            — persisted app preferences (includes panel widths)
      components/
        HexViewer.svelte         — hex dump with offset/hex/ascii, save/open/clear, in-place editing, smart diff (Compare button)
        TerminalLog.svelte       — scrollable log panel with copy/clear
        DeviceSelector.svelte    — search + paginated IC list
        DiagnosticsPanel.svelte  — overcurrent, calibration, hardware check, firmware update, pin test (buttons collapsible)
        ZifSocketDiagram.svelte  — ZIF socket placement diagram (right sidebar, below terminal log; shown when icspMode is "zif"); highlights bad pins in red with "PIN N" labels when pin test results are active, good occupied pins in green on pass
        IcspConnectorDiagram.svelte — ICSP connector pin-numbering diagram (right sidebar; shown when icspMode is "icsp" or "icsp_no_vcc")
        FuseBitDecoder.svelte   — AVR fuse bit decoder (8-bit grid with named fields, shown in config panel when fuseBitDefs store is non-null)
        SettingsPanel.svelte     — theme, defaults, layout reset, custom database directory picker
        ProgressPanel.svelte     — operation progress + cancel
      file-dialog.ts             — Tauri dialog wrappers (file open/save, directory picker)
  src-tauri/
    src/
      commands.rs                — all Rust command handlers (includes set_custom_db_dir, get_db_status, get_fuse_bit_defs)
      fuse_defs.rs               — AVR fuse bit definitions (static data keyed by infoic.xml config name, with chip-prefix overrides for mixed configs)
      lib.rs                     — Tauri app builder + plugin init (reads saved customDbDir on startup)
      state.rs                   — AppState (USB handle, selected device, db_paths cache, db_dir_invalid flag)
    Cargo.toml
    tauri.conf.json
```

## Known Bugs & Fixes

### Custom database directory startup fallback
If the user sets a custom database directory in Settings and later moves or deletes that directory, the app cannot find `infoic.xml` / `logicic.xml` at the saved path on next launch. The startup code in `lib.rs` checks whether both files exist in the saved `customDbDir`; if not, it sets `db_dir_invalid` on `AppState`, logs a warning to stderr, and falls back to the standard search paths (CWD, exe dir, `MINIPRO_HOME`, platform data dirs, Tauri resources). The GUI reads `get_db_status` when opening Settings and shows an amber warning if `active` is false, prompting the user to browse for a new directory or reset to default. No popup or modal is shown — the warning is inline in the Settings panel only.

### `selectedDevice` store held string instead of object
`DeviceSelector.svelte` was doing `selectedDevice.set(name)` (a string), but the store is typed as `DeviceInfo | null`. Fixed by storing the full `DeviceInfo` object: `selectedDevice.set(selectedInfo)`.

### `do_write` called `erase_chip` before `begin_transaction`
The handle had no active device, so the firmware returned "Protocol error: no device selected". Fixed by calling `begin_transaction(device)` before `erase_chip`.

### Global `select-none` prevented text selection
Adding `select-none` to the root app container blocked selection everywhere including terminal logs. Fixed by only applying it conditionally during active drag operations.

### `verify_chip` panic when file smaller than device
`verify_chip` read the reference file but did not pad it to device size. When auto-verify ran after a write with a smaller file, `expected[offset..]` panicked at offsets beyond the file length. Fixed by resizing the expected buffer to `size` with blank_value padding, matching `write_chip` behavior.

### USB sleep/wake Code 10 (Windows)
When a Windows laptop goes to sleep with the programmer connected, the USB host controller suspends the port. On wake, the WinUSB driver sometimes fails to re-initialise the device, leaving it in a Code 10 state ("This device cannot start"). The device shows a yellow triangle in Device Manager and cannot be opened by the app until physically replugged.

**Root cause:** Windows USB power management (selective suspend). Not a bug in our code — the device is broken at the OS driver level.

**Workaround for users:**
1. Unplug the USB cable, wait 20-30 seconds, plug it back in
2. Click the reconnect button in the GUI (it retries for ~15 seconds)
3. To prevent recurrence: disable "USB selective suspend" in Windows Power Options, or uncheck "Allow the computer to turn off this device to save power" for the USB root hub in Device Manager

**App-side mitigation:** `force_reconnect` retries 8 times over ~15 seconds with increasing delays. The error message instructs the user to unplug, wait, and replug. The reconnect button tooltip also mentions the 20-30 second wait.

### Voltage display and overrides used wrong lookup tables (fixed)
`VoltagesDto` and `apply_voltage_overrides` in `commands.rs` converted raw database voltage values using a single hardcoded 16-entry table that only matched T48/T56 firmware encoding. TL866A and TL866II+ use different encodings (e.g. TL866A VPP code `0x00` = 12.5V, not 9V), so the GUI showed wrong voltages for those programmers.

**Fix:** Both now use `minipro_core::device::{vcc_voltage_table, vpp_voltage_table, voltage_name, lookup_voltage}` which select the correct table per `ProgrammerModel`. The model is read from `AppState::programmer_info`; when no programmer is connected, falls back to TL866II+ tables.

### GUI voltage dropdowns used hardcoded option lists (fixed)
The Advanced voltage override section in `App.svelte` used hardcoded `VPP_OPTIONS` and `VCC_OPTIONS` constants that only matched the XG (T48/T56) tables. TL866A and TL866II+ users saw invalid options, logic ICs showed VPP/VDD dropdowns that can't be used, and T56/T76 custom-protocol devices showed options when overrides aren't supported.

**Fix:** Added `get_voltage_options` Tauri command in `commands.rs` that returns `VoltageOptionsDto { vcc, vpp, is_logic }` from the per-model voltage tables. The frontend `voltageOptions` store in `device.ts` is loaded via `$effect` in `App.svelte` whenever `$programmer` or `$selectedDevice` changes. Dropdowns are populated from the backend response; VPP is hidden when null (logic ICs, custom protocol), VDD is hidden for logic ICs, and "Voltage overrides not supported for this device" is shown when both are null. Override values reset to empty on device/programmer change.

### CLI `--vcc`/`--vdd`/`--vpp` overrides used wrong voltage tables (fixed)
`apply_overrides` in `minipro-cli/src/main.rs` mapped voltage names to **sequential indices** of a single hardcoded 16-entry table for all programmer models. On TL866II+ (and TL866A, T48, T56, T76) the firmware expects model-specific **encoded** values, so overrides sent the wrong codes — e.g. a `--vcc` sweep on a logic IC produced 5 V on every run (verified on a scope). Upstream C minipro rejects `--vcc` entirely for logic ICs (their `vcc_table` is NULL for logic devices).

**Fix:** `minipro-core/src/device.rs` now has the full upstream table set (`TL866A_*`, `TL866II_*`, `XG_*`, `XG_PLD_VPP`, `T48_BB_*`, `LOGIC_VCC_VOLTAGES`) plus `vcc_voltage_table()` / `vpp_voltage_table()` (per-model selection, mirrors upstream `load_device()`) and `lookup_voltage()` (case-insensitive, tolerates trailing `V` and `.0`). `apply_overrides` takes the programmer model and validates against these tables.

**Logic-IC `--vcc` is now supported** (upstream advertises the voltages in device info but offers no way to select them): valid values are exactly `1.8`, `2.5`, `3.3`, `5` (encodings `0x03`/`0x02`/`0x01`/`0x00`, sent in `msg[1]` of the logic-test command). `--vpp`/`--vdd` on logic ICs are rejected. Caveats: the programmer drives logic inputs at ~3.3 V regardless of VCC, and its input thresholds don't scale — sub-3.3 V tests are stress indicators, not conformance tests.

**Related fix:** `build_logic_device` in `database.rs` matched the logicic.xml `voltage` attribute against `"5"`/`"3.3"` etc., but the XML stores `"5V"` — every entry fell through to the 5 V default. Now parsed via `lookup_voltage(LOGIC_VCC_VOLTAGES, …)`; unknown voltages are a hard error.

**Note:** the "Voltage display uses wrong lookup tables" entry above claims T48/T56 use sequential-index tables; upstream actually assigns the encoded `xg_*` tables to T48/T56/T76 as well, so that entry's table breakdown should be revisited when the GUI display bug is fixed.

### CLI warns when VCC override differs from database default (fixed)
When `--vcc` (or `-o vcc=...`) changes VCC away from the database default, the CLI prints a warning to stderr:
```
WARNING: VCC overridden from 5V to 3.3V; results may be unreliable for this chip.
  The database default is 5V. Reading or blank-checking at a different VCC may produce false results (e.g. all 0xFF).
```
This prevents silent false positives (e.g. blank-checking a 5V EPROM at 3.3V reports "BLANK" because the chip can't power up). The override is still applied — the warning is informational, not blocking. Logic ICs get the first line only (no "false results" explanation, since logic tests at different VCC are intentional stress tests).

### Chip ID read had wrong type byte, endianness, and length (fixed)
`get_chip_id` in all protocol implementations had three bugs compared to the upstream C minipro:
1. **Wrong type byte**: TL866II+ read `resp[1]` as the ID type; should be `resp[0]` (matching upstream `msg[0]`)
2. **Fixed 4-byte ID read**: Always read 4 bytes little-endian. Should read `chip_id_bytes_count` bytes (1-4) with endianness based on ID type (LE for type 3/4, BE otherwise)
3. **Overly strict minimum length**: Required 6 bytes minimum. Should only require `2 + chip_id_bytes_count`

**Impact:** Write operations failed with "Response too short: expected 6 bytes, got 4" on TL866II+ for chips with 2-byte IDs (e.g. 27512@DIP28). Blank check was unaffected (doesn't read chip ID).

**Fix:** Changed `get_chip_id` trait signature to take `&Device` so `chip_id_bytes_count` is available. All four protocol implementations (TL866A, TL866II+, T56, T76) now use the same logic: read `resp[0]` as type, read `chip_id_bytes_count` bytes from `resp[2..]` with correct endianness.

### Duplicate `--skip-id` / `--skip-device-id` flags diverged from upstream (fixed)
The CLI had two separate flags for skipping chip ID verification:
- `-x` / `--skip-id` — controlled the top-level `check_chip_id()` call before write/read
- `--skip-device-id` — controlled the per-operation `check_device_id` parameter passed to `erase_chip`, `write_chip`, `read_chip`, `verify_chip`

These looked identical in `--help` but gated different code paths. Passing `-x` alone skipped the top-level check but per-operation checks still ran. Passing `--skip-device-id` alone did the opposite. Neither matched upstream: the C minipro has a single `-x` / `--skip_id` flag that is **explicitly rejected** in write/erase mode (enforced at `main.c` lines 1062-1067).

**Fix:** Removed `--skip-device-id` entirely. `-x` / `--skip-id` now controls both code paths (top-level and per-operation). Write and erase actions with `-x` are rejected with an error message directing the user to `-y` / `--continue-id` (which reads the ID but warns instead of aborting on mismatch — matching upstream `--no_id_error`).

### `-y` / `--continue-id` didn't propagate to per-operation checks (fixed)
After consolidating `-x`/`--skip-device-id`, `-y` printed "WARNING: chip ID mismatch — continuing" at the top-level check, but the per-operation `check_chip_id` calls inside `write_chip`, `read_chip`, `verify_chip`, and `erase_chip` still ran and aborted with a hard error. The `continue_id` flag only gated the top-level check.

**Fix:** Consolidated to a single check point (matching upstream's architecture). The top-level `check_chip_id` call now covers write, read, erase, and verify. All per-operation `check_device_id` parameters are `false` — the top-level check handles `-x` (skip), `-y` (warn + continue), and the default (error) in one place. This also fixes batch mode: previously the ID was re-checked for every chip in a batch; now it's checked once at the start.

**Remaining cleanup:** The `check_device_id: bool` parameter is still in the core API signatures (`operations.rs`) and the GUI still passes `true` to per-operation calls (with its own separate `check_chip_id` calls before each op). Removing the parameter entirely is tracked in ROADMAP.md.

### `erase_chip` didn't check `can_erase` flag (fixed)
`erase_chip` in `operations.rs` unconditionally called `handle.protocol.erase()` without checking `device.flags.can_erase`. Upstream minipro checks this flag in `erase_device()` (main.c line 1738) and silently skips the erase for chips that don't support electrical erase (e.g. UV EPROMs like the 27512). Our code was sending erase commands to the programmer for UV EPROMs, which could apply VPP pulses to pins not meant for electrical erase — undefined behavior and a potential safety issue.

**Fix:** `erase_chip` now checks `device.flags.can_erase` and returns `Ok(())` early if false. The CLI also checks `can_erase` before showing the "Erasing..." spinner. For explicit `-E` on a non-erasable chip, the CLI prints "This chip does not support electrical erase (use UV light for UV EPROMs)." instead of silently succeeding. For auto-erase before write on a non-erasable chip, the erase step is silently skipped (matching upstream — the write proceeds without a pre-erase).

## Terminal Rendering (TerminalLog.svelte)

The GUI terminal simulates a real terminal using HTML. Column alignment depends on monospace fonts and preserved whitespace.

### Rules

1. **Use `white-space: pre` on per-entry elements** — HTML collapses whitespace by default. Without `pre`, leading spaces and column alignment are destroyed.

2. **Render each log entry as its own DOM node via `{#each}`** — Use a regular `<div>` (no `white-space: pre`) as the scroll container, and render each entry as `<div style="white-space:pre;">{@html renderEntry(entry)}</div>` inside `{#each}`. This satisfies two constraints simultaneously:
   - **Whitespace safety**: The outer container has normal HTML whitespace collapsing, so newlines/indentation in Svelte template source between `{#each}` blocks are collapsed (not preserved). Each inner div has `white-space:pre` which only applies to the entry's content, preserving column alignment within the log text.
   - **WebKitGTK repaint**: Replacing the entire `innerHTML` of a scrolled container via a single `{@html}` string causes content to become invisible after horizontal scrolling on Linux (WebKitGTK doesn't repaint the scrolled region). Individual DOM nodes created by `{#each}` repaint correctly.

   **Do NOT** use `{@html}` to replace the entire log as a single string inside a `<pre>` — this causes the WebKitGTK repaint bug.

   **Do NOT** put `{#each}` inside a `<pre white-space:pre>` container — template whitespace between elements leaks into the output and breaks alignment.

3. **Flush `{@html}` against the entry div tag** — No newlines or comments between `>` and `{@html}`:
   ```svelte
   <!-- BAD — whitespace leaks into output -->
   <div style="white-space:pre;">
     {@html content}
   </div>

   <!-- GOOD — no whitespace -->
   <div style="white-space:pre;">{@html content}</div>
   ```

4. **Use inline styles for ANSI colors** — Convert `\x1b[0;91m` (red) to `<span style="color:#ef4444;">` and `\x1b[0m` to `</span>`. Tailwind classes may not apply inside `white-space:pre` elements due to CSS scoping.

5. **Don't use `.trim()` on multi-line strings** — `String.trim()` strips leading spaces from the first line, breaking alignment. Use `.split('\n').map(l => l.trimEnd()).filter(l => l.length > 0)` instead.

6. **Force repaint on WebKitGTK** — After log entries change, toggle `opacity` via `requestAnimationFrame` to force the compositor to redraw. WebKitGTK has a bug where content in scrolled containers doesn't repaint after DOM changes — the content is in the DOM but invisible until an unrelated event (hover, scroll, resize) triggers a repaint.

### Rust format specifiers

- `{:<3}` = left-align in 3-char field (`"1  "`)
- `{:-3}` = fill with `-`, right-aligned (NOT left-align!) (`"  1"`)
- Always use `<` for left-align, `>` for right-align in Rust format strings.

## Release Versioning

### Keep GUI and CLI versions in sync

All version numbers in the repo must match for any given release:

| File | Field | Example |
|------|-------|---------|
| `Cargo.toml` (workspace root) | `version` | `0.2.4` |
| `gui/src-tauri/Cargo.toml` | `version` | `0.2.4` |
| `gui/src-tauri/tauri.conf.json` | `version` | `0.2.4` |
| `gui/package.json` | `version` | `0.2.4` |

**Why:** The project is a monorepo with a single tag (`v0.2.4`) that triggers builds for both the CLI and GUI. If versions drift:
- GUI installer filenames will show the wrong version (e.g., `MINIPRO-RS_0.2.0_x64.msi` inside a `v0.2.3` release)
- Users get confused about which version they have
- Changelogs become unreliable

**When bumping for a release:**
1. Update all four version fields above
2. Add a `## [X.Y.Z]` section to `CHANGELOG.md` (CI extracts this for release notes)
3. Run `cargo generate-lockfile` and stage `Cargo.lock` + `gui/src-tauri/Cargo.lock`
4. Run `cargo deny check` (supply-chain audit — see below)
5. Commit with message like `chore(release): bump version to X.Y.Z`
6. Create the tag `vX.Y.Z`
7. Push the commit and tag to `origin` (GitLab). The GitLab push mirror replicates branches and tags to GitHub automatically, but with a delay (seconds to minutes).
8. If GitHub Actions doesn't trigger within a few minutes, push the tag directly: `git push github vX.Y.Z`
9. Let CI build and release everything consistently

**Supply-chain audit (cargo-deny):**

Run `cargo deny check` during release prep to scan for known vulnerabilities, license issues, and non-crates.io sources. This is a **local-only** check — no CI job, to conserve compute minutes. The config is `deny.toml` at the repo root.

```bash
# One-time install:
cargo install cargo-deny

# Check CLI workspace (run from repo root):
cargo deny check

# Check GUI workspace (Tauri has its own Cargo.lock with 530+ crates):
cd gui/src-tauri && cargo deny --config ../../deny.toml check
```

This checks:
- **Advisories** — known vulnerabilities from the [RustSec advisory database](https://rustsec.org/advisories/)
- **Licenses** — all dependencies have GPL-3.0-or-later compatible licenses
- **Sources** — all crates come from crates.io (no git dependencies)

If `cargo deny check` reports advisories, evaluate each one:
- **Critical/high severity** — update the affected dependency before releasing
- **Low severity / unmaintained** — document in the release notes if accepted
- **False positive** — add the advisory ID to `ignore = []` in `deny.toml` with a comment explaining why

**Git remotes — GitLab is primary, GitHub is a push mirror:**

The repo has multiple remotes. `origin` is GitLab (primary). GitLab has a push mirror to GitHub (`github` remote). Both branches and tags mirror automatically, but the mirror has latency — it is not instant. If you need GitHub Actions to trigger immediately (e.g., for a release), push the tag directly to `github` instead of waiting for the mirror:

```bash
git push origin main          # GitLab (mirrors to GitHub with delay)
git push origin vX.Y.Z        # GitLab (mirrors to GitHub with delay)
git push github vX.Y.Z        # Immediate — triggers GitHub Actions right away
```

**CI version verification:** The GitHub Actions release workflow has a `verify-versions` job that runs before any builds. It checks that all four version fields match the tag name. If any file is out of sync, the build fails immediately with a clear error message. This prevents releasing a binary with a stale version badge or mismatched installer filename.

**Rebuild the GUI after bumping:** The version badge in the GUI reads from `gui/package.json` at Vite build time. If you bump `package.json` but only run `cargo build --release` (without `cargo tauri build`), the stale embedded frontend persists. Always run `cargo tauri build` after any frontend or version change.

---

### Pre-commit checks (prevent CI failures)

The GitLab CI runs `cargo fmt --all -- --check` and `cargo clippy`. Running these locally before pushing prevents red pipelines.

```bash
# Run from the repo root. Must pass before committing Rust changes:
cargo fmt --all          # auto-fixes formatting
cargo fmt --all -- --check   # verify (same as CI)
cargo clippy --all-targets -- -D warnings  # lint check
```

**Common `fmt` failure:** Inline comment alignment. `rustfmt` enforces exactly **one space** before `//`. This will fail:
```rust
msg[0x65] = 0x03;                              // comment  (too much space)
```
This passes:
```rust
msg[0x65] = 0x03; // comment
```

**When editing `.svelte` / `.ts` files:** Run `cargo tauri build` (not just `cargo build`), because the frontend is embedded into the Rust binary at build time.

**Commit `Cargo.lock` after dependency changes:** The CI runs `cargo test --all --locked`, which fails if `Cargo.lock` is out of sync with `Cargo.toml`. Always regenerate and commit the lockfile when adding/updating dependencies or bumping versions (run from the repo root):
```bash
cargo generate-lockfile   # updates Cargo.lock
git add Cargo.lock gui/src-tauri/Cargo.lock
```

---

### Linux cross-distro verification

Rust stdlib API stability and Linux package names both drift over time. CI runs on a single Rust version (`rust:1.93` in GitLab, `stable` in GitHub Actions) and a single Ubuntu version, so it does **not** catch MSRV regressions or stale package names on other distros.

**Before using a Rust stdlib API that was stabilized recently:**
- Check that the API is stable under the declared MSRV (`rust-version` in `Cargo.toml`, currently 1.85 for CLI/core, 1.88 for GUI)
- CI green on `rust:1.93` does NOT mean it builds on 1.85. A user on Debian Stable discovered `is_multiple_of()` was unstable in 1.85 despite passing CI (see GitLab work item #1)
- When in doubt, use the older equivalent (e.g. `x % y == 0` instead of `x.is_multiple_of(y)`)
- Clippy's `manual_is_multiple_of` lint respects `rust-version` — if it flags your `%` usage, the MSRV is probably not set correctly

**Before changing Linux system library dependencies or package install commands:**
- Verify package names against actual distro package databases — do NOT copy from Tauri docs or other projects, as package names drift (e.g. `libappindicator3-dev` was dropped in Debian Trixie / Ubuntu 24.04, replaced by `libayatana-appindicator3-dev`)
- Cover at minimum: Debian/Ubuntu (`packages.debian.org` / `packages.ubuntu.com`), Fedora (`packages.fedoraproject.org`), Arch (`archlinux.org/packages`), openSUSE (`software.opensuse.org`)
- Derived distros follow their upstream: Mint/Pop!_OS/Kali/Parrot → Debian/Ubuntu, Manjaro → Arch
- Update the package table in BOTH `README.md` and `gui/README.md` — they must stay in sync
- Also check `.github/workflows/release.yml` if it installs system packages

**MSRV check in CI:** The GitLab CI `msrv` job runs `cargo check --all --locked` on `rust:1.85` in the `check` stage, in parallel with `fmt` and `clippy`. This catches stdlib API usage that requires a newer Rust than the declared MSRV. If this job fails, replace the unstable API with an older equivalent (e.g. `x % y == 0` instead of `x.is_multiple_of(y)`).

---

### CI compute credit conservation

GitLab and GitHub have limited free CI minutes. Do not trigger pipelines unnecessarily.

**GitLab CI runs on:**
- Every push to `main`
- Every tag push
- Manual web triggers

**GitHub Actions runs on:**
- Tag pushes only

**Rules:**
- **Default: add `[skip ci]` to ALL commit and merge messages** — including build-affecting changes. CI is only run deliberately during release prep, not on every push. This conserves limited free CI minutes.
- **Remove `[skip ci]` only when explicitly preparing for a release** — the user will indicate when CI should run. At that point, run a full pipeline to verify fmt, clippy, tests, and builds before tagging.
- **Do NOT push tags casually** — each tag triggers both GitLab CI and GitHub Actions release builds. Only tag for actual releases.
- **Feature branch pushes are free** on both platforms — use feature branches for work-in-progress.
- **Run all checks locally** before pushing to `main` or tagging (from the repo root):
  ```bash
  cargo fmt --all -- --check
  cargo clippy --all-targets -- -D warnings
  cargo test --all --locked
  ```
- **Batch commits when possible** — one push with multiple commits is one pipeline run. Multiple pushes of one commit each are multiple pipeline runs.

**Local CI-equivalent checks (no CI minutes required):**

Since CI is skipped on most commits, run these locally to catch issues that would fail in CI:

| Check | Where | Command |
|-------|-------|---------|
| fmt | Windows | `cargo fmt --all -- --check` |
| clippy | Windows | `cargo clippy --all-targets -- -D warnings` |
| tests | Windows | `cargo test --all --locked` |
| MSRV (CLI/core) | WSL Ubuntu | `cargo +1.85 check --all --locked` |
| Linux build + tests | WSL Ubuntu | `cargo test --all --locked` |
| clippy (Linux) | WSL Ubuntu | `cargo clippy --all-targets -- -D warnings` |

WSL setup (one-time):
```bash
wsl -d Ubuntu -u root -- apt install -y build-essential pkg-config libssl-dev
wsl -d Ubuntu -- curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
wsl -d Ubuntu -- bash -c "source ~/.cargo/env && rustup install 1.85"
```

WSL checks (run from the project root):
```bash
wsl -d Ubuntu -- bash -c "source ~/.cargo/env && cd /mnt/<your-repo-path> && cargo +1.85 check --all --locked && cargo test --all --locked && cargo clippy --all-targets -- -D warnings"
```

**Note:** The GUI has its own workspace (`gui/src-tauri/`) and a separate MSRV (1.88 declared in `Cargo.toml`). The CI `msrv` job only checks the root workspace (`--all`), not the GUI. GUI MSRV compliance is not currently enforced.

**When bumping an MSRV (CLI or GUI), update ALL of these files in the same commit:**
1. `Cargo.toml` — root workspace `rust-version` (CLI)
2. `gui/src-tauri/Cargo.toml` — `rust-version` (GUI)
3. `README.md` — Linux support section + badge
4. `gui/README.md` — prerequisites section
5. `AGENTS.md` — this section (MSRV references in the cross-distro verification notes and the tech stack table)
6. `.gitlab-ci.yml` — the `msrv:` job's `image: rust:X.Y` tag (if the CLI MSRV changes)

Missing any of these causes user-reported discrepancies (see GitLab work item #5).

**Pre-commit checklist (mandatory before every commit):**
1. Does the change affect compiled code, tests, or build config? (Rust source, Cargo.toml, CI config, test files)
2. If YES → verify locally (fmt, clippy, test) before committing. For Linux-specific issues, also run WSL checks.
3. If NO → no local verification needed.
4. **Always add `[skip ci]` to the commit message** — regardless of whether the change is build-affecting or not. CI is only run deliberately during release prep.
5. Before writing the commit message, explicitly state aloud: "Build-affecting change: yes/no." If yes, run local checks first. Either way, add `[skip ci]`.

---

### Branching rules

- **All work goes on its own branch** — never commit directly to `main`. Use a descriptive branch name prefixed by type: `fix/...`, `feat/...`, `refactor/...`, `docs/...`, `chore/...`, `release/...`.
- **Branch from `main`** and merge back with `--no-ff` to preserve branch history.
- **Run pre-commit checks** (`cargo fmt`, `cargo clippy`, `cargo test`) on the branch before merging build-affecting changes.
- **Delete the branch after merging to `main`** — this applies to all short-lived branches (fix, feat, refactor, docs, chore, release). The only branches that persist are `main` and long-lived integration branches (e.g. `protocol-parity`) that span multiple sessions.
- **Merge commit messages** follow the pattern: `Merge <branch-name>: <short description> [skip ci]` (always include `[skip ci]` unless explicitly preparing for a release).
- **Branch pushes are free** on GitLab — they don't trigger CI pipelines. Only the merge to `main` triggers a pipeline.

---

### Commit message rules

- **Do NOT add any AI attribution footers** to commit messages
- **Do NOT add** `"Generated with [Devin]"` or similar watermarks
- **Do NOT add** `"Co-Authored-By"` lines for AI agents or bots
- **Commit messages must be written as if authored solely by the human developer**
- Keep commit messages concise: a short subject line, optional body, nothing else
