# GUI Internals — Feature Documentation

Feature-level internals for the Tauri GUI. Extracted from AGENTS.md — read
this before working on the hex viewer, pin test, logic test/identify,
config/fuse panel, or terminal rendering. Universal rules (build, stores,
Tauri gotchas, git/CI) remain in the root AGENTS.md.

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
        device.ts              — connected programmer + IC database + shared favorites store
        settings.ts            — persisted app preferences (includes panel widths)
      components/
        HexViewer.svelte         — hex dump with offset/hex/ascii, save/open/clear, in-place editing, smart diff (Compare button)
        TerminalLog.svelte       — scrollable log panel with copy/clear
        DeviceSelector.svelte    — search + paginated IC list (syncs from external selection via $effect)
        DiagnosticsPanel.svelte  — overcurrent, calibration, hardware check, firmware update, pin test (buttons collapsible)
        ZifSocketDiagram.svelte  — ZIF socket placement diagram (right sidebar, below terminal log; shown when icspMode is "zif"); highlights bad pins in red with "PIN N" labels when pin test results are active, good occupied pins in green on pass
        IcspConnectorDiagram.svelte — ICSP connector pin-numbering diagram (right sidebar; shown when icspMode is "icsp" or "icsp_no_vcc")
        FuseBitDecoder.svelte   — AVR fuse bit decoder (8-bit grid with named fields, shown in config panel when fuseBitDefs store is non-null)
        LogicTestGrid.svelte    — zoomable color-coded logic test result grid (Ctrl+Scroll zoom, copy-to-clipboard TSV)
        IdentifyResults.svelte  — logic IC identify results table (passing matches only, Select/favorite/deselect buttons)
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
| Ctrl+G | Go to offset (opens goto dialog) |
| Tab | Switch between hex and ASCII panes on the same byte |
| Ctrl+F | Open find dialog (hex or ASCII search) |
| Ctrl+Scroll | Increase/decrease font size (10-16px, 1px per notch) |
| F3 / Shift+F3 | Navigate find matches or diff results (whichever was last activated) |
| ? / F1 | Toggle help overlay |
| Escape | Close help overlay, find dialog, or goto dialog |

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

## Pin-contact test (GUI)

The Diagnostics panel has a "Pin Contact Test" button that runs the ZIF socket
contact test and highlights bad pins on the ZIF socket diagram. This
matches XGPro's "Pin Detect" feature.

**Pre-operation gate:** A "Pin Contact Check" checkbox in the operations
panel runs the pin test automatically before read, write, verify, erase,
blank check, chip ID, and config (fuse read/write) operations. When
checked (default, matching XGPro), the test runs after `begin_transaction`
and before the operation-specific command. If bad pins are found, the
operation is aborted and the bad pins are highlighted on the ZIF diagram.
The checkbox is disabled on unsupported models, in ICSP mode, or when
the device has no pin-map data. SPI autodetect runs the pin check
automatically on supported models (no checkbox) — autodetect is aborted
if bad pins are found, since poor contact produces garbage JEDEC IDs.

**Model support:** TL866II+ and T48 only. T48 inherits pin test
from TL866II+ via protocol alias (`T48Protocol = Tl866iiPlusProtocol`).
TL866A/CS, T56, and T76 are not supported — button is disabled with a
tooltip. The C `minipro` source does not define the bit-banging commands
(0x2D-0x36) for any of these models. The T76's `0x3E` command is an
adapter-init pin-driver configuration step, not a standalone contact
test — running it standalone returns meaningless data and can corrupt
subsequent reads. This was discovered by Matt Brown's t76-improvements
branch, whose t76_adapter_init() uses 0x3E to configure socket pin
drivers before bitstream upload. The upstream C minipro's `t76_pin_test`
receives the response but never parses it — `value` is initialized to 0
and never updated from the response buffer, so every pin reports as bad.
The xgecu-pro project (https://github.com/jfabienke/xgecu-pro) confirmed
on real hardware that it "measured nothing and corrupted every read."
The T56 defines the same `0x3E` command but has no `t56_pin_test`
function — it may share the T76's situation, but this has not been
verified on T56 hardware.

**Button disabled when:** no programmer connected, no device selected,
device has `pin_map == 0` (no contact-test data in database), ICSP mode
active, unsupported programmer model, or a test is already running.

**Checkbox disabled when:** no programmer connected, no device selected,
ICSP mode active, unsupported programmer model, or device has
`pin_map == 0`. The checkbox is located right of "Chip ID check" for
read/write/verify/erase, replaces "No options for this operation." for
blank check and chip ID, and appears in a thin options row above the
collapsible fuse editor for config. Logic test has no checkbox (uses
test vectors, not pin-contact mechanism).

**Backend:** `do_pin_test` Tauri command in `commands.rs` follows the
existing `try_acquire` / `spawn_blocking` / `take_handle` pattern with
a 10s timeout. Returns `PinTestResultDto { supported, pass, bad_pins,
message }`. Core `Protocol::pin_test()` returns `Result<PinTestResult>`
with `bad_pins: Vec<u16>` (device pin numbers, 1-based, empty = all
good). The CLI `-z` handler prints from the returned struct, preserving
the existing "Bad contact on pin: N" output format. The pre-operation
gate uses `run_pin_check_if_enabled` helper which silently skips on
unsupported models/ICSP/no-pin-map, and emits bad-pin data via
`pin-test-result` event for diagram highlighting on failure.

**`OperationOptions.pin_check`:** Added `pin_check: bool` field (serde
default `true`) to `OperationOptions` in `commands.rs`. Operations that
take `OperationOptions` (`do_read`, `read_chip_to_bytes`, `do_write`,
`do_write_bytes`, `do_batch_write_chip`, `do_verify`) pass it to the
helper. Operations that take `icspMode` directly (`do_erase`,
`do_blank_check`, `do_chip_id`, `read_fuses`, `write_fuses`) now also
take `pinCheck: bool` and `window: Window` parameters.

**`do_spi_autodetect`:** Now takes `window: Window`. On TL866II+/T48 in
ZIF mode, constructs a temporary `Device` with `pin_map` based on
`idType` (8-pin=0x01, 16-pin=0x03), sets `handle.device`, runs
`pin_contact_check`, then clears `handle.device` before proceeding with
autodetect. Matches upstream minipro's `auto_detect` function.

**Frontend stores:** `pinTestResult` and `pinTestRunning` in
`operations.ts`. `doPinTest()` invokes the Tauri command and logs
results. `clearPinTestResult()` is called in an `$effect` in App.svelte
when `$programmer` or `$selectedDevice` changes, preventing stale
bad-pin highlights from a previous device. `initPinTestListener()`
listens for `pin-test-result` events emitted by the backend during
pre-operation pin checks, updating the store for diagram highlighting.

**ZIF diagram highlighting:** `ZifSocketDiagram.svelte` accepts a
`badPins` prop (device pin numbers). Device pins are mapped to ZIF
socket positions using the same logic as `occupiedPins`. Bad ZIF slots
render red with "PIN N" labels. Good occupied slots keep their default
styling (no green). The "ZIF PIN 1" label is hidden when pin 1 is bad to
avoid overlap with the red "PIN 1" label. Chip pin stubs always use the
chip color (never change for bad/good — only the socket slots change).

**Result panel:** Below the ZIF diagram, a compact panel shows
"✓ All pins OK" (green) or "✗ Bad contact on N pin(s)" (red) with the
pin list and a "Clear" button to dismiss results.

### Logic IC tab gating (GUI)

When a logic IC is selected (`chip_type === "Logic"`), the Read, Write,
Verify, Erase, Blank Check, and Chip ID tabs are disabled and dimmed
with a "Not applicable for logic ICs" tooltip. Only Logic Test and
Config (self-gates via `!config`) remain enabled.

Conversely, when a non-Logic device is selected, the Logic Test tab is
disabled with a "Not applicable for this device type" tooltip. With no
device selected, Logic Test remains enabled (shows the identify panel).

A dedicated `$effect` auto-switches tabs when the device type changes
to an incompatible operation: switches to Logic Test when a logic IC is
selected with a non-applicable tab active, and switches to Read when a
non-Logic device is selected with Logic Test active. The effect is
separate from the results-clearing effect to avoid making
`$activeOperation` a dependency of it.

### Logic IC identify (GUI)

The Logic Test tab is always enabled. When no Logic device is selected,
a "Select a logic IC" panel shows pin count buttons (8/14/16/20/24/28/40),
a VCC selector, and an Identify button. Clicking Identify tests the
inserted chip against all `logicic.xml` entries with the matching pin
count via `logic_auto_find()` in `operations.rs`. The `do_logic_identify`
Tauri command emits `"progress"` events per candidate and returns sorted
`LogicIdentifyResultDto` entries (passing first, then by fewest errors,
then alphabetically).

**Results table** (`IdentifyResults.svelte`): shows only passing matches.
Each row has a favorite star (shared `favorites` store in `device.ts`),
device name, manufacturer, and a Select button. Selecting a result calls
`selectDevice(name)`, which syncs the DeviceSelector via an `$effect`
that searches for the device name and highlights it. The "✓ Selected"
indicator is clickable to deselect. A Clear button empties the results
contents (`clearIdentifyResultsContents()` sets `[]`) without hiding the
table. The full clear (`clearIdentifyResults()` sets `null`) is called
only on programmer disconnect.

**Identify results clearing**: Two separate functions —
`clearIdentifyResults()` (sets to `null`, used on programmer change) and
`clearIdentifyResultsContents()` (sets to `[]`, used by the Clear button).
The `$effect` that clears identify results fires on `$programmer` change
only, NOT on `$selectedDevice` change, so selecting a device from the
results list doesn't clear the results.

**With a Logic device selected**: The operations panel shows the normal
VCC selector, help button, and an "Identify unknown logic IC" link that
calls `deselectDevice()` to return to identify mode.

**DeviceSelector sync**: An `$effect` in `DeviceSelector.svelte` watches
`$selectedDevice`. When the name doesn't match local `selectedName`
(external selection), it syncs local state and sets `searchQuery` to the
device name, triggering the debounced search so the device appears in the
list with highlight and favorite controls.

### Logic test grid (GUI)

`LogicTestGrid.svelte` renders the logic test result as a color-coded
grid instead of raw ANSI text. The backend returns a structured
`LogicTestResult` DTO (`pinCount`, `vectorCount`, `vectors`, `step1`,
`step2`, `errors`, `pass`). Cell classification in `cellInfo()` maps
each cell to input (blue), output-pass (green), output-fail (red with
`-` suffix), or ignore (gray). Two-pass test data (pull-up/pull-down)
is checked for output validation.

**Zoom**: Ctrl+Scroll adjusts cell size (20–44px), persisted via
`settings.logicTestZoom`. Non-passive wheel listener with
`preventDefault`.

**Copy**: A Copy button exports the full grid as TSV (tab-separated
values) using `plugin:clipboard-manager|write_text`. Header row has
`Vec` + `Pin1..PinN`, one row per vector with symbols and `-` suffix
for mismatches. Pastes directly into spreadsheet cells.

**Layout**: Header is outside the scroll container in a `shrink-0` div
to prevent sticky-header overlap. Both header and body tables use
matching `<colgroup>` with `table-fixed` for column alignment.

### Shared favorites store

Device favorites were extracted from `DeviceSelector.svelte` to a shared
`favorites` writable store in `device.ts`. Both `DeviceSelector` and
`IdentifyResults` import `favorites`, `isFavorite`, and `toggleFavorite`.
The store auto-persists to `localStorage` via a `subscribe()` call.
Templates use a reactive `$derived` Set (`favNames`) for O(1) lookups
instead of calling `isFavorite()` directly (which uses `get()` and isn't
reactive).

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

## Terminal Rendering (TerminalLog.svelte)

The GUI terminal simulates a real terminal using HTML. Column alignment
depends on monospace fonts and preserved whitespace.

### Rules

1. **Use `white-space: pre` on per-entry elements** — HTML collapses
   whitespace by default. Without `pre`, leading spaces and column alignment
   are destroyed.

2. **Render each log entry as its own DOM node via `{#each}`** — Use a
   regular `<div>` (no `white-space: pre`) as the scroll container, and
   render each entry as `<div style="white-space:pre;">{@html renderEntry(entry)}</div>`
   inside `{#each}`. This satisfies two constraints simultaneously:
   - **Whitespace safety**: The outer container has normal HTML whitespace
     collapsing, so newlines/indentation in Svelte template source between
     `{#each}` blocks are collapsed (not preserved). Each inner div has
     `white-space:pre` which only applies to the entry's content, preserving
     column alignment within the log text.
   - **WebKitGTK repaint**: Replacing the entire `innerHTML` of a scrolled
     container via a single `{@html}` string causes content to become
     invisible after horizontal scrolling on Linux (WebKitGTK doesn't
     repaint the scrolled region). Individual DOM nodes created by `{#each}`
     repaint correctly.

   **Do NOT** use `{@html}` to replace the entire log as a single string
   inside a `<pre>` — this causes the WebKitGTK repaint bug.

   **Do NOT** put `{#each}` inside a `<pre white-space:pre>` container —
   template whitespace between elements leaks into the output and breaks
   alignment.

3. **Flush `{@html}` against the entry div tag** — No newlines or comments
   between `>` and `{@html}`:
   ```svelte
   <!-- BAD — whitespace leaks into output -->
   <div style="white-space:pre;">
     {@html content}
   </div>

   <!-- GOOD — no whitespace -->
   <div style="white-space:pre;">{@html content}</div>
   ```

4. **Use inline styles for ANSI colors** — Convert `\x1b[0;91m` (red) to
   `<span style="color:#ef4444;">` and `\x1b[0m` to `</span>`. Tailwind
   classes may not apply inside `white-space:pre` elements due to CSS
   scoping.

5. **Don't use `.trim()` on multi-line strings** — `String.trim()` strips
   leading spaces from the first line, breaking alignment. Use
   `.split('\n').map(l => l.trimEnd()).filter(l => l.length > 0)` instead.

6. **Force repaint on WebKitGTK** — After log entries change, toggle
   `opacity` via `requestAnimationFrame` to force the compositor to redraw.
   WebKitGTK has a bug where content in scrolled containers doesn't repaint
   after DOM changes — the content is in the DOM but invisible until an
   unrelated event (hover, scroll, resize) triggers a repaint.

### Rust format specifiers

- `{:<3}` = left-align in 3-char field (`"1  "`)
- `{:-3}` = fill with `-`, right-aligned (NOT left-align!) (`"  1"`)
- Always use `<` for left-align, `>` for right-align in Rust format strings.
