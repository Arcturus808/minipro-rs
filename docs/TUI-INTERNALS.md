# TUI Internals — `minipro-tui`

Developer notes for the ratatui terminal UI (`crates/minipro-tui`). The
user-facing design/rationale lives in `docs/TUI-PLAN.md`; this file
records how the shipped MVP is actually built and the invariants to
preserve.

Requires **Rust 1.88** (ratatui 0.30); the crate carries its own
`rust-version` and is excluded from the 1.85 MSRV CI job — do not lower
it or let it creep back into `--workspace` MSRV checks.

## Crate map

| File | Role |
|------|------|
| `main.rs` | terminal setup, event loop, all `draw` code (frames/widgets) |
| `app.rs` | `App` state + `handle_event`/`handle_key` reducer; modals; tests |
| `events.rs` | `AppEvent`/`WorkerCmd` protocol + `TuiLogger` (`log::Log` impl) |
| `worker.rs` | worker thread: owns `MiniproHandle`, runs ops, reports events |
| `zif.rs` | `ZifDiagram` widget — ZIF socket + chip overlay + pin states |
| `hexview.rs` | full-screen hex-buffer view — rows, entropy gutter, cursor |
| `cfgview.rs` | full-screen Config view — fuse/lock rows, SPI SR decode |
| `theme.rs` | color/style palette used by every pane |

## Threading & event flow

Two threads, two channels:

```
crossterm ──poll(100ms)──► main loop ──WorkerCmd──► worker thread
                            ▲    │                     │
                            │    └─► App reducer       │ MiniproHandle
                            │                          │ (blocking ops)
                            └────── AppEvent ◄─────────┘
                                 ◄── TuiLogger (log crate)
```

- **Main loop** (`run` in `main.rs`): drains `ev_rx` non-blocking every
  iteration, draws, then `event::poll(100ms)` for terminal input. The
  poll doubles as the tick — no tick thread; the spinner advances per
  paint while an op runs. A bounded 8 ms event-drain follows `poll` so
  resize floods can't starve repaints.
- **Worker** (`worker.rs`): owns the `MiniproHandle` — ops serialize by
  construction, matching the GUI's non-cancellable model. Receives
  `WorkerCmd::{Connect, Run{..}}`, emits `AppEvent::{Connected,
  ConnectFailed, OpDone, Progress, Log, PinTest}`.
- **Logging**: `events::install_logger` installs a `log::Log` impl
  (leaked `&'static`, `set_logger`) that forwards records as
  `AppEvent::Log`. Core emits `info!` per block in `blank_check` — the
  logger filters `blank_check` progress spam before it reaches the pane.
  Rows render GUI-style (`[LEVEL]` colored tag + message, no timestamp)
  and word-wrap to the pane width via `app::wrap_log` — the pane has no
  horizontal scroll, so wrapping is the only way long messages stay
  readable. `log_scroll`/`log_scroll_max` are in *wrapped rows*, the
  latter cached by the renderer each frame.
- **stderr ownership**: core prints some diagnostics with `eprintln!`
  (algorithm names, pin-test progress, OVC results) — fine for the CLI,
  but raw stderr writes corrupt the TUI's frame. Core's `outln!` macro
  routes them to `log::info!` when `PRINT_TO_LOG` is set; `main` sets it
  at startup so those lines reach the log pane. New library diagnostics
  must use `outln!`, never `eprintln!`.

## Connection state

`ConnState::{Disconnected, Connecting, Connected{info,simulated},
Failed(msg)}`. Auto-connect at startup (`WorkerCmd::Connect`); **F5**
re-sends it. `MINIPRO_SIMULATE_PROGRAMMER=<MODEL>` (or `--simulate
<MODEL>`) produces a `simulated` connection (info exists, no USB
handle): the badge renders amber with a `(SIM)` marker, and op dispatch
warns instead of running. Op errors
containing the USB keyword table (`looks_like_disconnect` — mirrors the
GUI's `handle_usb_error`) drop `Connected` back to `Disconnected`.

## Op orchestration (worker)

`run_op` → `run_in_transaction` mirrors the GUI command sequences
exactly — keep them in sync:

1. `find_device` → reject logic ICs (MVP scope gate)
2. `set_icsp_mode("zif", dev)` — auto-activates ICSP for `MP_ICSP_ONLY`
   devices (warns when it does)
3. `begin_transaction`
4. **Pin-contact gate** — `pin_check_gate_applies(model, icsp,
   pin_map)`; failure aborts the op and emits `AppEvent::PinTest` so
   the ZIF diagram colors bad/tested/untested pins. All-open gets the
   "is a chip seated?" hint.
5. **Chip-ID gate** — explicit `check_chip_id` when the option is on
   (skipped for the ChipId op itself, which reads the ID as its body)
6. Op body: `read_chip` / `verify_chip` / `blank_check` /
   `chip_id_info` / `erase_chip` / `write_chip`
7. `end_transaction` — always, even on error

Write-specific sequencing (mirrors `do_write` + the CLI): `protect_off`
first when `off_protect_before` is set (unconditional on T76, else the
"Unprotect before" checkbox); auto-erase unless Skip-erase, **gated on
`device.flags.can_erase`** (UV EPROMs) — each erase is followed by an
`end`/`begin` transaction reset, which the firmware requires; verify
unless Skip-verify; `protect_on` when "Re-protect after" is set and the
chip supports it. Standalone `Erase` also gates on `can_erase` and
reports `Erased{supported:false}` rather than calling the op.

`OpOutcome` variants carry result data back to the reducer: `Stats`,
`Verified`, `Erased{supported}`, `Blank`, `NotBlank{address}`,
`ChipId(info)` — chip-ID results reuse core `chip_id_info` so
variant/no-ID normalization matches the GUI — plus the Config
outcomes: `ConfigFuses{rows,n_fuses,n_uids,nibbles,uid_nibbles}` (each
row carries name+value+mask; `n_fuses`/`n_uids` mark the fuse →
user-id → lock section boundaries — upstream's fuses-file order — and
the uid section can be wider than fuse rows since it follows
`data_org`), `ConfigSr{bytes}` (read *and* write read-back, so the view
always reflects chip state), and `ConfigWritten`.

## Config view (fuses/locks + SPI status register)

The `c` mnemonic (tab 7) dispatches `OpKind::Config` through the normal
transaction machinery — pin gate and chip-ID gate apply unchanged. On
`OpDone` the reducer opens `App::cfg`, a dedicated input context like
the hex view (its own key space; hex digits are data, not op mnemonics).

- **Device split** (worker, `OpKind::Config` arm): `protocol_id 0x03`
  (SPI NOR) → `spi_status_read` — no `ChipConfig` exists for those
  parts; the fuse-0x01 path reaches the physical SR. `ChipConfig::Mcu`
  → `read_fuses`, split against `config.fuses`/`num_uids`/
  `config.locks` to recover masks and section boundaries (user-id rows
  get `device.compare_mask` — they have no field mask). Neither → "no
  configuration available" error (GUI parity).
- **Editing**: `↑↓` move the row cursor (resets `nibble`), `←→` move
  the nibble, hex digits overtype at `nibble` (position 0 = most
  significant, auto-advances, clamps at the row's width — 2 nibbles for
  byte fuses, 4 for `pic_*` config words, and `uid_nibbles` for
  user-id rows since those follow `data_org`, not
  `fuse_element_size`; `row_nibbles(i)` resolves per row). Per-row
  `dirty` marks render amber.
- **Write**: `w` requires a dirty row, then opens the mandatory
  `Modal::Confirm` (`OpKind::ConfigWrite` is `is_destructive`) with
  context warnings — lock-section edits add "lock bits can permanently
  lock the chip", SR edits add the non-volatile-BP note. `y` dispatches
  `OpInput::Fuses(Vec<FuseValue>)` or `OpInput::SpiSr(u8)`; fuse writes
  clear dirty on `ConfigWritten`, SR writes refresh the row from the
  firmware's post-write read-back.
- The view stays open across re-reads (`r`) and refreshes in place for
  SR; fuse reads rebuild it. `Esc`/`q`/`x` close.

## UI state & rendering invariants

- `App` is a plain struct; `handle_event`/`handle_key` are the only
  mutators (reducer). All crossterm `KeyEvent`s route through
  `handle_key`; Windows emits Press+Release, so only
  `Press | Repeat` act — tests cover this.
- `op_running.is_some()` swallows navigation keys; worker commands are
  also rejected at dispatch (double gate).
- Op dispatch: mnemonic keys `r w v e b i` (`l`/`c` warn) call
  `start_op()` directly — arm + run in one key. `OP_KEYS` maps letter →
  tab index; digits `1`–`8` only arm. The mnemonic arm lives in the
  global key match *after* the modal and search-editing intercepts, so
  letters still type into inputs and can't fire inside modals — keep
  that ordering when adding global keys.
- Modals: `Modal::Path` (its `PathAction` picks the submit action —
  `Op(op)` for Write/Verify file ops, `SaveBuffer`/`LoadBuffer` for the
  hex view; the dialog shows a live `→ resolved` preview and submits
  the absolutized path so every later display names the real
  location), `Modal::Goto` (hex-view `:` offset prompt — hex digits
  only, `0x` prefix optional; invalid input clears the field rather
  than closing, since the log is hidden behind the view), and
  `Modal::Confirm` (Write after path entry, Write-from-buffer, Erase).
  Only `y`/`Y` confirm — `Enter` is deliberately excluded (a held
  Enter's repeat events from the path prompt could otherwise carry in
  and fire the op); `n`/`N`/`Esc` cancel; every other key is ignored so
  a stray keypress can't silently abandon the confirm.
- **Hex-buffer view** (`hexview.rs`, `App::hex`/`hex_view`): reads land
  in memory, not a file — `r` dispatches `read_chip_buffer` and the
  `OpDone::Read` payload fills `App::hex` and opens the view. It's a
  dedicated input context (like modals/search): while open every key
  routes to `handle_hex_key`, so hex digits `0-9a-f` overtype bytes —
  `b`/`e` are data here, not the blank/erase mnemonics — and
  Esc/q/x return to the panes. `Tab` toggles between hex-entry and
  ASCII-entry mode — the standard hex-editor binding (hexedit, Okteta,
  bvi); `i`/Insert are aliases to enter ASCII mode. A mode is required
  since a-f/0-9 are ambiguous, and `i` can never exit (it would type
  0x69) — non-printable keys only. Esc exits the mode first, then the
  view.
  `o` enters the view globally (or
  opens a file→buffer prompt when empty); inside, `o` loads a file,
  `s` saves (`read_file`/`write_file`, so the Format select applies),
  `w`/`v` write/verify the buffer via `WorkerCmd::Run{OpInput::Buffer}`.
  Navigation: arrows/PgUp/PgDn/Home/End/`g`/`G` + `:` goto; edits mark
  `HexBuf::edited` (amber) until a save clears the set. `/`/`Ctrl-F`
  opens `Modal::Find` — literal text, or `x:`-prefixed hex pairs —
  searched from the cursor with wraparound (`n`/`N`, `F3`/`Shift-F3`
  repeat; F3 works in ASCII mode where n/N are data). Results land on
  `HexBuf::note`, a one-shot footer status, because the log pane is
  hidden in this view. `Backspace` undoes the previous edit keystroke
  (hexedit's undo-previous-character): `HexBuf::undo` is a capped
  per-keystroke stack of (offset, pre-edit byte), and the lazily
  cloned `HexBuf::original` lets `mark()` recompute dirty marks when
  an undo returns a byte to its load-time value. Mid-op the view
  stays browsable — `HexBuf::nav` runs read-only while `op_running`
  blocks edits/actions. Rows carry a 1-cell `▌` entropy gutter using
  the GUI's exact Shannon formula (`entropy / log2(len)`) and 4-tier
  color mapping (green/lime/amber/red). The center-pane Buffer line
  mirrors the buffer's name/size/CRC/edit count.
- Favorites (`f` in DEVICES) persist to `favorites.txt` in the
  per-user data dir via `minipro_core::prefs` — `name<TAB>mfr` lines,
  tolerant parser, atomic tmp+rename writes, write-through on every
  toggle (`App::favorites_path`; `None` disables persistence — tests).
  They **pin to the top of `matches`** in stored order regardless of
  the query — same list, same cursor, `Enter` selects directly (GUI
  parity: "always visible regardless of the search query");
  `App::fav_rows` marks the boundary for the header count. `refilter`
  excludes favorited names from the filtered body so no row
  duplicates; `toggle_favorite` re-filters and keeps the cursor on
  the toggled device. `Ctrl-F` toggles a favorite while the search
  input is editing — plain `f` is text there, so a non-printable key
  carries the action; the search `Char` arm also now guards on
  Ctrl/Alt so modified keys can't leak in as text. Only favorites
  resolvable in `app.devices` pin — a favorite saved under another
  model is hidden, mirroring the GUI's availability rule.
  Unifying with the GUI's `localStorage` favorites store is deferred —
  shared-file costs + the import/export interim are analyzed in ROADMAP
  ("Unify GUI/TUI favorites").
- Right pane auto-collapses under ~100 cols with hysteresis; `]`
  pins/unpins. Full layout switches to a too-small message under
  80×24, also hysteretic. `MINIPRO_TUI_DEBUG_PAINT` overlays draws/s.
- Resize flicker fixes are load-bearing — 256 KiB `BufWriter` (one
  write per frame), degenerate-size guard, bounded event drain. See
  `docs/KNOWN-BUGS.md`; don't reintroduce per-cell flushes.
- `ZifDiagram` is fed by core `zif.rs`: `zif_spec(model)` → socket,
  `device_to_zif`/`occupied_zif_pins` → occupied/justification/lever.
  Pin states under a pin-test result: bad=red, tested=green,
  GND=white (`untestable` — held low by the die so contact can't be
  distinguished; legend row appears under the socket while `show_test`
  is set), occupied-but-untested=faint. Without a result, occupied
  pins are green and empties faint.
- Panic hook restores the terminal (raw mode off, leave alt screen)
  before the default panic message — Ctrl+P panics on debug builds to
  exercise it.

## Tests

`app.rs` tests drive the reducer directly: search/cursor, help modal,
pane hysteresis, option toggles, select cycling, run gating
(no device / disconnected / logic-IC warning), path→confirm flow for
Write, Erase confirm dispatch, key swallow during ops, `OpDone` state,
favorites, and the hex-buffer view (read→buffer dispatch, `o` entry,
nibble overtype, b/e-are-digits, nav clamps, goto parse, save/write
flows, nav-only-during-op). `worker.rs` tests cover
`looks_like_disconnect` and the `SizeDiff`→`SizeMismatch` map;
`hexview.rs` tests cover the entropy edge cases. The worker's op
bodies need USB and are covered by hardware validation, not unit
tests.

## Dev mode

```powershell
cargo run -p minipro-tui -- --simulate T48
# or via env var (MINIPRO_FAKE_PROGRAMMER still works, deprecated):
$env:MINIPRO_SIMULATE_PROGRAMMER = "T48"
cargo run -p minipro-tui
Remove-Item Env:MINIPRO_SIMULATE_PROGRAMMER
```

Simulated mode exercises connect state, search, selection, options, modals,
and rendering. Op dispatch logs a warning and returns — ops still
require real hardware.

## Deliberately out of MVP scope

Logic-vector test, identify, diagnostics/health
check, batch ops, voltage overrides (needs the
deferred `OpOptions`/voltage-glue extraction), ICSP wiring diagrams.
The Logic tab renders but warns "not in this MVP" — extend `OpKind`,
`OPS`, and the worker `match` arm together when adding phases. The hex
view
shipped with the P2 subset: find, undo/redo, selection, diff/compare,
bytes-per-row, and mouse remain future work.
