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

`ConnState::{Disconnected, Connecting, Connected{info,fake},
Failed(msg)}`. Auto-connect at startup (`WorkerCmd::Connect`); **F5**
re-sends it. `MINIPRO_FAKE_PROGRAMMER=<MODEL>` produces a `fake`
connection (info exists, no USB handle): the badge renders amber with a
`(FAKE)` marker, and op dispatch warns instead of running. Op errors
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
variant/no-ID normalization matches the GUI.

## UI state & rendering invariants

- `App` is a plain struct; `handle_event`/`handle_key` are the only
  mutators (reducer). All crossterm `KeyEvent`s route through
  `handle_key`; Windows emits Press+Release, so only
  `Press | Repeat` act — tests cover this.
- `op_running.is_some()` swallows navigation keys; worker commands are
  also rejected at dispatch (double gate).
- Modals: `Modal::Path` (file ops — Read pre-fills
  `<sanitized-name>.bin`, Write/Verify start empty) and
  `Modal::Confirm` (Write after path entry, Erase). `Esc` cancels;
  nothing destructive runs without a `y`.
- Right pane auto-collapses under ~100 cols with hysteresis; `]`
  pins/unpins. Full layout switches to a too-small message under
  80×24, also hysteretic. `MINIPRO_TUI_DEBUG_PAINT` overlays draws/s.
- Resize flicker fixes are load-bearing — 256 KiB `BufWriter` (one
  write per frame), degenerate-size guard, bounded event drain. See
  `docs/KNOWN-BUGS.md`; don't reintroduce per-cell flushes.
- `ZifDiagram` is fed by core `zif.rs`: `zif_spec(model)` → socket,
  `device_to_zif`/`occupied_zif_pins` → occupied/justification/lever.
  Three pin states under a pin-test result: bad=red, tested=green,
  untested=faint (`show_test` + `tested` set); without a result,
  occupied pins are green and empties faint.
- Panic hook restores the terminal (raw mode off, leave alt screen)
  before the default panic message — Ctrl+P panics on debug builds to
  exercise it.

## Tests

`app.rs` tests drive the reducer directly: search/cursor, help modal,
pane hysteresis, option toggles, select cycling, run gating
(no device / disconnected / logic-IC warning), path→confirm flow for
Write, Erase confirm dispatch, key swallow during ops, `OpDone` state,
favorites. `worker.rs` tests cover `looks_like_disconnect` and the
`SizeDiff`→`SizeMismatch` map. The worker's op bodies need USB and are
covered by hardware validation, not unit tests.

## Dev mode

```powershell
$env:MINIPRO_FAKE_PROGRAMMER = "T48"
cargo run -p minipro-tui
# back to real hardware in the same shell:
Remove-Item Env:MINIPRO_FAKE_PROGRAMMER
```

Fake mode exercises connect state, search, selection, options, modals,
and rendering. Op dispatch logs a warning and returns — ops still
require real hardware.

## Deliberately out of MVP scope

Logic-vector test, config/fuse read+write, identify, diagnostics/health
check, batch ops, hex buffer pane (P2+), voltage overrides (needs the
deferred `OpOptions`/voltage-glue extraction), ICSP wiring diagrams.
The tabs render but warn "not in this MVP" — extend `OpKind`, `OPS`,
and the worker `match` arm together when adding phases.
