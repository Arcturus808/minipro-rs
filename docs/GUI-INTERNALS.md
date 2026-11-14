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
        DiagnosticsPanel.svelte  — overcurrent, calibration, hardware check, firmware update, pin test, chip health check (collapsible; actions render as borderless menu rows via the shared `actionClass` string)
        ZifSocketDiagram.svelte  — ZIF socket placement diagram (right sidebar, below terminal log; shown when icspMode is "zif"); highlights bad pins in red with "PIN N" labels when pin test results are active, good occupied pins in green on pass
        IcspConnectorDiagram.svelte — ICSP connector diagram (right sidebar; shown when icspMode is "icsp" or "icsp_no_vcc"); renders pin numbering plus per-class signal wiring when a verified table exists
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
      lib.rs                     — Tauri app builder + plugin init (reads saved customDbDir on startup); registers tauri-plugin-single-instance first — a second launch focuses the existing "main" window (unminimize/show/set_focus) and exits, preventing shared-state races (favorites.txt, settings.json, algorithm cache) and dead USB-claim attempts. Dev/--no-bundle exes find the XMLs via the Tauri resource dir (gui/src-tauri/resources/) — NOT the shared resolver's exe_dir/../../data candidate, which only covers root-workspace binaries (target/release/minipro, minipro-tui)
      state.rs                   — AppState (USB handle, selected device, db_paths cache, db_dir_invalid flag); `search_device_names` returns every substring match — no result cap, so broad queries can't silently drop late-file manufacturers
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

**Modal convention:** every fullscreen overlay must cap its height and
scroll — `max-height: 80vh`/`max-h-[85vh]` + `overflow-y: auto` — because
an unbounded card overflows the window once content outgrows it (the
Settings modal shipped without a cap and clipped its own Close button;
fixed via a flex-column card, pinned header, scrollable body). Follow the
pinned-header + scrollable-body pattern whenever the content can grow.

## Pin-contact test (GUI)

The Diagnostics panel has a "Pin Contact Test" button that runs the ZIF socket
contact test and highlights bad pins on the ZIF socket diagram. This
matches XGPro's "Pin Detect" feature.

**Pre-operation gate:** A "Pin Contact Check" checkbox in the operations
panel runs the pin test automatically before read, write, verify, erase,
blank check, chip ID, and config (fuse read/write) operations. When
checked (default, matching XGPro), the test runs after `begin_transaction`
and before the operation-specific command. The result is emitted to the
ZIF diagram on both pass and fail (`emit_pin_test_result` is
unconditional — a pass proves the check ran and shows which pins were
measured). If bad pins are found, the operation is aborted.
The checkbox is disabled on unsupported models, in ICSP mode, or when
the device has no pin-map data. SPI autodetect runs the pin check
automatically on supported models (no checkbox) — autodetect is aborted
if bad pins are found, since poor contact produces garbage JEDEC IDs.

**Abort reporting:** gate failures (pin check, chip-ID check) emit their
detail once at warn/error level, then return a terse
`"aborted (<gate>)"` marker string. `runOp` in `operations.ts` detects
the `aborted (` prefix and logs `<op> aborted (<gate>)` at warn level —
without it the frontend would re-log the whole gate reason as
`<op> failed: …`, double-reporting and mislabeling a never-ran op as a
failure. Keep new pre-op gates on the same marker convention.

The abort path also skips `refreshProgrammer()`. That call exists to
sync the badge when a programmer is yanked mid-op, but it unconditionally
writes the `programmer` store — which trips the App.svelte
device/programmer-change `$effect` that clears `pinTestResult`, wiping
the bad-pin highlighting the gate just emitted. A gate abort means the
session is provably alive (the gate command itself succeeded), so the
refresh is both harmful and pointless there.

**`runOp` contract:** returns the op's result (a truthy sentinel for
void invokes) or `null` on error — errors are logged inside `runOp`,
not rethrown. Any post-op log/UI that asserts success MUST gate on the
return value (`if (await doErase(...))`), never on the await resolving
— an aborted op resolves `null`, so unconditional post-await messages
lie. Do not log success lines the backend can't verify (the former
`logChipIdCheck` printed "Chip ID check passed" purely from the option
checkbox + `has_chip_id` flag — including after gate aborts; the backend
`emit_log` at the real gate is the only honest source). Also note the
ordering quirk: `runOp`'s own lines are `deferLog` (rAF-deferred) while
backend `app-log` events and direct `logs.*` calls write immediately —
an immediate line can land between an op's start and end bracket.

**Model support:** TL866II+ and T48. The backend returns
`UnsupportedOperation` on other models and the button is disabled with
a tooltip.

`PinTestResult.untestable_pins` carries the `<gnd>` table (device-pin
numbering): ground pins are held low by the die so contact can't be
distinguished — they're excluded by design, not "passed". The diagram
renders them neutral gray with a `GND — not testable` legend entry;
the same set drives the TUI's white cells. Note coverage is still implicit —
upstream's T48 pin-test MR (!315) makes the same point: a floating GND
fails every other pin anyway.

The T48 path (`pin_test_t48` in `tl866iiplus.rs`) uses XGPro's own
single-command protocol — `0x3E` carrying the device `variant` as chip
ID (u16 LE — the field is ignored by firmware; the response carries
constant fields, not an echo), returning a contact bitmap (ZIF1-40 in
`resp[8..13]` LSB-first + ISP1-16 in `resp[13..15]`), documented with
USB captures at https://proghq.org/wiki/XGecu_Protocol. Upstream never
implemented it; this is a reverse-engineered port — bit set = contact,
compared against the device's `pin_map` mask. Hardware-validated on a
fully seated DIP-8: bitmap `0F 00 00 00 F0` = ZIF 1-4 + 37-40.

**The bitmap is only valid with the pin drivers idle** — inside a
transaction, configured/driven pins read as open (the same seated DIP-8
read `01 00 00 00 C0` in-transaction, reporting half its pins bad).
XGPro runs pin detect before beginning the transaction, so
`pin_test_t48` ends the transaction, measures, then re-begins — callers
stay inside a transaction transparently. See KNOWN-BUGS.md.

The T76 path (`pin_test_t76` in `t76.rs`) uses `0x3E` too but with a
different protocol: 16-byte bare request → 32-byte reply with the
contact bitmap at `resp[16..21]` (LE, bit n = ZIF pin n+1 — the T76 ZIF
has **48** positions, not 40). A DIP pin d of an N-pin part is ZIF pin
`d + 24 − N/2` (bottom-seated). Format from XGPro_T76 V13.21 USB
captures documented in upstream MR !304 (unmerged as of writing —
implemented from its capture tables; reply verified on firmware
00.1.18, other firmware prints a note). DIP-only: `@DIPn`/`@PDIPn`,
n even ≤ 40, non-ICSP — enforced by `t76_pin_test_supported` in
`protocol/mod.rs` (mirrored frontend-side in DiagnosticsPanel) and
inside `pin_test_t76` itself. "Non-ICSP" tests
`flags.prog_support != MP_ICSP_ONLY` — NOT `package_details.icsp`,
which is a wiring-class index (e.g. ATMEGA328P@DIP28 carries class 7
yet is fully ZIF-programmable); the DTO exposes it as `icsp_only`.

History: we previously believed T76 `0x3E` was adapter-init only —
Matt Brown's t76-improvements branch found it configures socket pin
drivers (true — our own `t76_adapter_init`/`t76_emmc_adapter_init` use
it that way), upstream's stub never parsed the reply so every pin
reported bad, and xgecu-pro saw it "corrupt every read". The MR's
captures resolve the contradiction: the reply *does* carry measured
contact state, and the read corruption was the undrained 32-byte reply
desyncing the next response — `pin_test_t76` drains it.
The T56 also defines `0x3E`, labelled pin detect on proghq like the
T48's — but whether it is the pin-detect variant or an adapter-init is
still unverified on hardware.

**Result semantics:** the backend's `PinTestResult` carries both
`bad_pins` and `expected_pins` (every device pin the `pin_map` mask
actually tested — the GND pin on SPI flashes is excluded and never
measured). The ZIF diagram renders three states: tested-bad (red +
label), tested-good (green), and untested/unoccupied (neutral gray) —
so untested pins are never implied "good". When every expected pin is
open (`bad == expected`), the surfaces treat it as an empty socket:
the log/card say "is a chip seated?" instead of implying N bad
contacts. The CLI `-z` prints the same hint.

**Button disabled when:** no programmer connected, no device selected,
device has `pin_map == 0` (no contact-test data in database), ICSP mode
active, unsupported programmer model, a non-DIP package on T76
(`t76PinTestOk` mirrors the core check), or a test is already running.

**Checkbox disabled when:** no programmer connected, no device selected,
ICSP mode active, unsupported programmer model, a non-DIP package on
T76, or device has `pin_map == 0`. Model/package eligibility for both
the standalone button and this checkbox comes from `lib/pinTest.ts`
(`PIN_TEST_SUPPORTED`, `t76PinTestOk`, `pinCheckModelOk`) — a single
frontend mirror of the core checks, after the two sites drifted apart. The checkbox is located right of "Chip ID check" for
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

**Per-model socket rules:** `minipro_core::zif` encodes three
model-specific facts observed from XGPro's own diagrams: socket size
(40-pin on TL866A/CS/II+/T48, 48-pin on T56/T76), lever position
(top-left on every model except T48, which is bottom-right), and
insertion rule (most models top-justify — chip pin 1 at ZIF pin 1;
T56/T76 bottom-justify — the chip's lower-left pin N/2 sits at ZIF pin
24, so a DIP-8 occupies ZIF 21–24 + 25–28). The GUI fetches the resolved
layout via `get_zif_layout(pinCount)` — a `zif_pins[]` map of device
pin → ZIF pin that `occupiedPins`, `badZifPins`, and the "PIN N" labels
all consume — so the drawing and highlighting can never diverge from
the CLI's placement hints (`-d` output, `-z` bad-pin annotation).

**Result panel:** Below the ZIF diagram, a compact panel shows
"✓ All pins OK" (green) or "✗ Bad contact on N pin(s)" (red) with the
pin list and a "Clear" button to dismiss results.

## Chip health check (GUI)

Diagnostics → "Chip Health Check" invokes `run_health_check` (enabled
whenever a device is selected — it needs `-p`-equivalent context for the
transaction). Params: `icspMode`, `pinCheck: true` (pre-op pin gate like
other operations), `passes: 5`, and optional `referencePath` — enabled via the "Compare
against file" checkbox under the button (opt-in modifier, matching the
app's checkbox convention; reveals the file picker only when checked and
disables the run button while a file is unchosen); the backend loads it through
`operations::read_file` (auto format, padded to device size) and compares
the effective image against it (`referenceMismatches` renders in the
card — amber when >0). The backend runs `operations::health_check`
(20 chip-ID reads + N full reads), emits progress on the standard
`"progress"` channel under the "Health check" operation name, and logs a
verdict line + notes via `app-log`.

The returned `HealthCheckResultDto` renders as a result card inside the
Diagnostics disclosure: colored verdict (`Stable` green, `Transport error`
red, others amber), pass/ID-run counts, unique chip IDs seen, unstable-byte
count with `0→1`/`1→0` flip split and dominant periodic spacing, top
instability regions (4 KiB buckets), analysis notes, and — when
`consensusB64` is present — a "Save consensus dump" button routing through
`pickSaveFile` + `save_bytes_to_file` (absent when passes all agree —
nothing to reconstruct). A "Clear" button in the card header dismisses
the result. Read-only: the command never writes
to the chip. See `docs/CLI-INTERNALS.md` "Health check" for the analysis
semantics and transport limitations.

**Read-panel "Stability check"** (`OperationOptions.stability_check`,
serde default `false`): a checkbox in the read operation's options row
applies the same analysis inline — `do_read` and `read_chip_to_bytes`
route through `read_chip_stable_logged` → `operations::read_chip_stable`
(5 passes, cumulative progress across all passes). Stable → the consistent
buffer is saved/loaded as usual; unstable → the majority-vote consensus
is used and a warn log reports unstable-byte count, pass errors, and
notes. Core op functions no longer take a `check_device_id` parameter
(the single top-level `check_chip_id` handles it); the GUI's
`check_device_id` *option* remains, gating the explicit pre-operation
ID check — `read_chip_to_bytes` previously relied on the parameter and
now runs the same explicit pre-check as `do_read`.

## ICSP wiring diagrams (IcspConnectorDiagram.svelte)

The `package_details` field in `infoic.xml` encodes a *wiring-class*
index in bits 8–15 (`PackageDetails.icsp` in `device.rs`). It is a
per-device/per-family diagram selector — upstream C minipro prints it
as `ICP%03d.JPG`, the filename of a canned Xgpro image. We instead map
each verified `(ProgrammerModel, class)` pair to an explicit connection
list in `crates/minipro-core/src/icsp.rs` and render our own SVG.

**Important:** the class byte is *family-relative* — the same value can
mean different chip pinouts in different `<database>` sections (e.g.
class 0x05 is AT45DB DataFlash in the legacy INFOIC section but SPI
NAND in INFOIC2PLUS/INFOICT76). The legacy INFOIC section (TL866A/CS)
also has no class 0x09 at all — SPI NOR ICSP was never offered on that
hardware, so `icsp == 0` on a TL866A is correct vendor behavior, not a
missing table.

**Architecture:**

- `icsp::icsp_wiring(model, class)` → `Option<&'static IcspWiring>`;
  `None` means unverified — callers must not guess.
- `IcspWiring` = `{ title, numbered, chip_labels, wires, notes }`.
  `chip_labels` is indexed by chip pin − 1 so unconnected pins still
  render (dimmed). When `numbered` is false the target is a generic MCU
  whose pin positions vary by package — labels are signal names and the
  row index is not a pin number (GUI hides it; CLI omits "chip pin N").
- `DeviceInfoDto.icsp` carries the raw class byte; the stateless
  `get_icsp_wiring(model, icspClass)` command resolves the table. The
  component fetches in an `$effect` keyed on `$programmer.model` +
  `$selectedDevice.icsp` — no AppState timing issues.
- CLI `minipro -d NAME` prints `ICSP: ICP%03d.JPG` (upstream parity)
  plus an ASCII wiring table when `--programmer MODEL` is given.

**Rendering:** header pin-number groups on the left (right-aligned at a
fixed x), chip pin rows on the right in pin order, horizontal wires —
no crossings. Unconnected chip pins render dimmed. `notes` render as
bullet warnings under the SVG.

**Panel behavior:** the header is a collapse toggle (state persisted in
`localStorage["minipro_icsp_collapsed"]`); expanded content is capped at
420 px and scrolls in both axes so tall tables (T76 eMMC) can't starve
the terminal log. SVGs render at natural pixel size (no `w-full`
scaling — that made labels unreadable) inside an `align-items:
safe center` flex container, so wide diagrams scroll from the left edge
instead of clipping both sides. Zigzag headers (T48/T76) use compact
geometry (18 px pads) to fit the default sidebar width. The pin-1 dot
sits above pin 1 on linear headers and below pin 1 on zigzag headers
(odd pins are the bottom row); it is suppressed on `numbered: false`
targets where it would imply a nonexistent physical pin 1. Pin numbers
render in the same indigo on both diagrams (`LABEL_FILL`); numbered chip
labels get a bold indigo pin number + `dx="7"` gap before the signal
name. Text opacity convention: notes and actionable fallbacks at 80%,
pure placeholders at 50–60%.

**Testing without hardware:** `MINIPRO_SIMULATE_PROGRAMMER=<MODEL>`
(deprecated alias `MINIPRO_FAKE_PROGRAMMER`; see AGENTS.md) makes
`get_programmer_info`/`force_reconnect` return a synthetic programmer —
device search, device info, and both diagram panels work with no USB
device attached. The same mode is exposed in-app via Settings →
Simulation (persists `simulatedModel`, calls
`set_simulated_programmer`, and marks the status badge "simulated";
`ProgrammerInfoDto.simulated` distinguishes it from a real connect).
Op paths that find no USB handle emit "simulated programmer — operations
need real hardware" (via `state::no_programmer_error`) instead of
"No programmer connected", and both the settings toggle and the
badge-reconnect path log the transition so the log can't end on a
simulated-model line while the badge shows real hardware.

**Adding a new class:** verify against official docs or hardware, then
add a `static` table in `icsp.rs` and a match arm. Never ship a
pinout from a class-number guess — a wrong VCC/VPP line can damage
hardware.

**Verified tables so far**, per programmer header:

- TL866A + TL866II+ (1×6, identical pinout): 0x01 Atmel SPI, 0x02 PIC
  (ICD2), 0x06/0x07/0x08 AVR variants, 0x09 SPI NOR; plus TL866A-only
  0x05 AT45DB, and II+-only 0x05 SPI NAND, 0x0a Microwire, 0x0b I²C,
  0x0c KB90xx.
- T48 (2×8 zigzag): 0x01/0x02/0x06–0x09, 0x0a, 0x0b, 0x0c (VCC = pin 13,
  GND = pin 16; 0x40 eMMC uses an external driver board — stays on
  fallback).
- T56 (1×8): all of the above plus 0x0e AT17LV and 0x40 eMMC (direct
  8-pin wiring).
- T76 (2×14 zigzag): all of the above plus 0x0e and full 8-bit 0x40
  eMMC (VCC = pins 20/22/24, GND = 11/21/26/28, SGND = 1/15/27).

Sources: legacy MiniPro `ICP001`–`ICP008` (TL866A) and the Xgpro V13.17
`img/` set (`ICPnnn` = TL866II+, `T48ICPnnn`/`T56ICPnnn`/`T76ICPnnn` per
model). The MCU classes are `numbered: false` generic-signal targets.
Still unverified (fallback): 0x03/0x04 SyncMos (no DB devices reference
them) and T48 0x0e/0x40 (adapter-board based, not a direct pinout).

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
`$selectedDevice`. A `lastSyncedDevice` guard distinguishes external
selections (e.g. IdentifyResults → `selectDevice()`): for those, the
effect syncs local state and sets `searchQuery` to the device name,
triggering the debounced search so the device appears in the list with
highlight and favorite controls. Selections made inside the component via
`onSelect` set `lastSyncedDevice` before publishing to the store, so the
effect skips the `searchQuery` rewrite — otherwise the full device name
would re-run the search and collapse the results list to just the
selection.

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
reactive). The TUI keeps a separate `favorites.txt` file store —
unifying the two is deferred pending demand; see ROADMAP ("Unify
GUI/TUI favorites") for the shared-file cost analysis and the
import/export interim candidate.

### Favorites vs. programmer-model device lists

Favorite names are not portable across `infoic.xml` database sections: the
TL866A section has bare names (`PIC16F628A`) while the T76 section only has
package-qualified names (`PIC16F628A@DIP18`). A favorite saved under one
model can therefore fail `select_device` under another.

`DeviceSelector` calls `check_favorite_devices` whenever `$favorites` or
`$programmer` changes. The command does a single-pass
`list_devices_by_model()` scan of the database and reports whether each
favorite's exact name resolves for the connected model (or any model when
none is connected). `favoriteItems` then filters out unavailable names —
hidden favorites still exist in the store and reappear when a compatible
programmer is connected. While the check is in flight, unknown entries are
shown (`available !== false`).

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

### User-id bytes in the config DTO

`read_fuses` (the core op) returns fuses, then `user_idN` entries, then
locks — upstream's fuses-file order. The `read_fuses` command splits the
flat list by `fuse_len` plus a `user_id`-name count: `cfg_fuses` /
`lock_bits` keep their `FuseValueDto` shape while `user_fuses` carries
the raw user-id bytes (LE at the `data_org` element width) that the
frontend shows as a hex list. The GUI's `write_fuses` sends only
cfg+lock values — the core write keeps the whole user-id section
optional for exactly this reason. TL866A/CS carry no user-id rows at
all (their firmware hangs on `MP_FUSE_USER` — see KNOWN-BUGS).

### SPI status register panel (25-series NOR)

For devices with `protocol_id === 0x03` (exposed as `protocol_id` on
`DeviceInfoDto`) and no MCU config, the Config operation shows a status
register panel instead of the fuse editor — the Config button stays
enabled via the `isSpiNor` derived flag, and the "Read Config from Chip"
Start button routes to `readSpiStatus()` rather than `readFuses()`.

- Backend: `spi_status_read`/`spi_status_write` commands
  (`commands.rs`), thin wrappers over `minipro_core::spi_status_*`
  inside the usual take_handle → begin_transaction → end_transaction →
  store_handle flow via the shared `spi_status_op` helper.
  `UnsupportedOperation` maps to `{ supported: false }` so the frontend
  can warn instead of failing — the ops layer itself gates on
  `protocol_id`.
- Frontend: `spiStatus` state (`number | null`, reset on device change),
  a hex input synced with eight bit-toggle checkboxes in MSB-first order
  (`SR_BITS` in `App.svelte` — the common SR1 layout; WEL/WIP are
  disabled read-only), and Read/Write buttons. `writeSpiStatus` logs a
  non-volatile warning when BP bits are being set. The `showSpiSrHelp`
  modal documents bit semantics, layout variance (MX25L8006E BP0–BP3),
  and the protect-off recovery path.
- The SR byte read/written here is the real on-chip status register —
  verified T76/TL866A/T48, see `docs/T76-SUPPORT-STATUS.md`. BP/SRP
  bits are non-volatile; a `0x00` write or protect-off recovers
  (protect-off verified T76/TL866A/T48).

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

## Algorithm Downloader (Settings → Database)

The "Download algorithm.xml + firmware…" button in SettingsPanel invokes
`download_algorithms`, which wraps `minipro_core::xgpro_fetch::fetch` in
`spawn_blocking` behind the shared `try_acquire` gate — it cannot run
concurrently with chip operations.

- **Output dir:** the command takes an optional `outputDir`. The frontend
  passes `$settings.customDbDir` so generated files land next to a custom
  database when one is configured; `None` → the per-user data dir that
  `resolve_one()` already searches (see CLI-INTERNALS.md).
- **Progress:** core `Progress` events are mapped onto the standard
  `"progress"` channel (`{done, total, operation}` — `operation` is
  `"download"` or `"extract"`) and stage/file lines go through
  `emit_log` → `"app-log"`, so ProgressPanel and the terminal show them
  with no new plumbing.
- **Cancel:** `AppState.download_cancel` is an `AtomicBool` flipped by the
  `cancel_algo_download` command; the core progress callback returns
  `false` when set, and `fetch` aborts with `MiniproError::Cancelled`
  (displayed as `"Operation cancelled"` — the command matches on that
  string, so keep them in sync). The flag is reset at the start of each
  download; cancelling is fire-and-forget.
- **Post-download refresh:** the cached `DatabasePaths.algorithms` is
  re-resolved only when absent or stale — an existing still-present path
  wins, so an explicit/custom `algorithm.xml` is never shadowed by the
  downloaded one. `DatabasePaths::resolve` semantics mean this also picks
  up the per-user dir automatically.
- **Connected handle caveat:** an open `MiniproHandle` snapshots its db
  paths at connect time; the refresh only updates `state.db_paths`. The
  command logs "reconnect to use the new algorithm.xml" on success.
- **Open folder:** the adjacent "Open folder" button calls
  `open_download_dir(dir)` — resolves the same output dir (custom db dir
  or per-user data dir), `create_dir_all`s it so it works before any
  download, and opens it in the OS file manager. `open_folder` gained an
  `is_dir()` branch via the shared `open_dir_in_manager` helper, so it
  accepts directory paths too (previously it opened the *parent*).

## Firmware update — downloaded-image resolution

Diagnostics → Firmware Update first calls `find_downloaded_firmware`
(`commands.rs`), which scans `customDir` then `user_data_dir()` for
`firmware-<model>-*.dat`, parses `a.b.c` versions out of filenames,
sorts newest-first and dedups by version. Match by exact
`firmware-<model>-` prefix — model-safe by construction, which matters
because flashing a wrong-model image is the brick path. TL866CS maps
to the TL866A prefix: one `update.dat` covers both models and the
bundled file is named for the A (the flasher selects the CS image
internally).

Exactly one match is used directly. **Multiple matches open a chooser
modal** — never a silent newest-first pick, because for TL866A/CS the
newest downloaded image is the clone-bricking one (see below). Each
row shows version, filename, and a per-version safety label
(`FW_VERSION_LABELS`); clone-risk models preselect the *oldest* image
(the reported clone-tolerant one), other models preselect newest. The
modal also offers "Choose different file…" (the native picker) and
Cancel. Zero matches falls through to the same picker — the manual
path always remains reachable.

The confirm dialog appends a version note comparing installed vs
candidate. Comparison is on `{major}.{minor}` — the low u16 of the file
version, which is what the device actually reports. The
hardware-revision warning (first-component mismatch) only applies when
the installed side reports a real hw byte — `installed[0] !== 0`, i.e.
TL866A/II+; T48/T56/T76 hardcode hw to 0 so their `00` display prefix is
a constant and file prefixes like `01.1.35` are product-line labels, not
revisions (see `docs/FIRMWARE-FORMATS.md`). Warn only, never a hard
block — upstream posture; strictly-lower = explicit DOWNGRADE note;
equal = "reflash only to recover".

**TL866A/CS clone warning:** `CLONE_RISK_MODELS` in DiagnosticsPanel
adds a version-aware paragraph for those models on *every* path
(auto-found, chooser, or manual pick) via `cloneNoteFor(model,
fileVersion)`. The generic counterfeit-existence warning always shows;
the hard "reported brick vector" line keys on the *selected file's*
version — ≥03.2.86 (MiniPro 6.85) gets the brick warning, 03.2.85
(MiniPro 6.82) gets "widely reported clone-tolerant, but not
certified", and unknown/no-version files get a neutral caution. A
brick is recoverable only by forcing boot mode with a resistor on the
PCB test points.

Both images download via Settings → Download (vendored `libarchive-rs`
reads the MiniPro installers' RAR4 layers — see
`docs/CLI-INTERNALS.md` and `docs/LIBARCHIVE-INTERNALS.md`), so the
chooser modal is the normal TL866A/CS path once a download has run.

`do_firmware_update` runs the synchronous `operations::firmware_update`
inside `tokio::task::spawn_blocking` — the update blocks for a minute
or more (erase + ~1900 block writes + two re-enumeration waits) and
would otherwise starve a tokio worker thread for its whole duration.
Progress still rides `window.emit("progress")` from inside the
callback; the captured `out` log is emitted only after the call
returns, so a stall shows as 0% progress until the (bounded) timeout
fires — a live-streaming log would need the op's output pumped through
a channel rather than captured.

**TL866A/CS ≥03.2.84 caveat:** the software-entered boot session is
unreliable (see CLI-INTERNALS "Firmware update" and KNOWN-BUGS); the
core now warns, probes the session before erase, bounds the erase
wait, and appends unplug/replug recovery instructions to failures —
all of which surface in the GUI's log panel and error dialog
verbatim. Because the core's warning text only reaches the log *after*
the op returns, the confirm dialog additionally shows
`unstableSessionNote` up front on TL866A/CS units running ≥03.2.84 —
the user needs the "replug and retry" instruction *before* clicking
yes, not post-mortem. A boot-mode unit still can't be *connected* in
the GUI (strict open), so the recovery path there is replug → CLI `-F`.

### Rust format specifiers

- `{:<3}` = left-align in 3-char field (`"1  "`)
- `{:-3}` = fill with `-`, right-aligned (NOT left-align!) (`"  1"`)
- Always use `<` for left-align, `>` for right-align in Rust format strings.
