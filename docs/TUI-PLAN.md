# TUI Plan — `minipro-tui`

Design for a third frontend: an interactive terminal UI built on ratatui,
sitting alongside `minipro` (CLI) and the Tauri GUI. Read this before
writing any TUI code; update it when decisions change.

## Decisions (locked)

| Decision | Resolution |
|----------|------------|
| Binary shape | **Separate `minipro-tui` binary** in `crates/minipro-tui` (workspace member). No `minipro tui` subcommand — the CLI's flat clap flags don't accommodate subcommands without breaking compat, and it keeps ratatui/crossterm out of the CLI dependency tree. |
| Shared logic | **Extract orchestration glue into `minipro-core` first.** The TUI must not reimplement the pin-check gate, voltage-override resolution, or ICSP auto-activation — those encode upstream C minipro behavior (see KNOWN-BUGS.md). |
| MVP scope | **P0 + P1 only** — connect, device search/select, core ops, options, progress, log. No hex editor, no fuses in the first ship. |
| UI library | **ratatui + crossterm.** De-facto standard, actively maintained (0.30.x), immediate-mode rendering suits the custom widgets (hex grid, ZIF diagram, logic vector grid). |

## Rationale

Headless/SSH/bench-Pi workflows where WebView2/WebKitGTK is unavailable or
heavy, and where the CLI's weakest point — searching ~30k devices — benefits
from an interactive fuzzy list. Precedent: `jfabienke/xgecu-pro` ships a TUI
mode for the same hardware.

## Architecture

### Crate

```
crates/minipro-tui/
  Cargo.toml            — package "minipro-tui", [[bin]] minipro-tui,
                          rust-version = "1.88"  (ratatui 0.30.x MSRV)
  src/
    main.rs             — terminal setup/restore, panic hook, run loop
    app.rs              — App state + update() reducer
    event.rs            — AppEvent enum, input thread, tick timer
    worker.rs           — WorkerCmd enum, worker thread owning MiniproHandle
    session.rs          — selected device, op options, favorites
    config.rs           — TOML persistence via `directories`
    ui/
      mod.rs status_bar.rs devices.rs operations.rs log.rs
      zif.rs icsp.rs dialogs.rs hexview.rs (P2)
      fuses.rs logic.rs batch.rs diagnostics.rs settings.rs (P3/P4)
```

Dependencies (keep the tree lean — every dep is compiled by every CLI user
on `cargo install` only if they're in this crate, so they stay here):
`ratatui` (0.30), `crossterm`, `minipro-core`, `serde` + `toml`,
`directories`, `log`. Phase-optional: `arboard` (clipboard), `unicode-width`.

### Threading model — no tokio

`minipro-core` is synchronous/blocking (`MiniproHandle` is `Send` — the GUI
already moves it across `spawn_blocking`). Three threads:

```
input thread ──┐   crossterm::event::read() ──> Event::Terminal
tick thread ───┼──> mpsc::channel<AppEvent> ──> main loop
worker thread ─┘   WorkerCmd via second channel
                   owns MiniproHandle, replies AppEvent::{Progress,OpDone,...}
```

- **Worker owns the handle.** Ops are serialized by construction — one
  `WorkerCmd` at a time, no `is_running` lock needed. UI simply ignores
  start-requests while `op_running`.
- **Progress:** core ops already take `Option<&mut dyn FnMut(usize, usize)>`
  — the closure captures a cloned `Sender<AppEvent>` and forwards
  `(done, total)`.
- **Logs:** install a `log::Log` impl that forwards records as
  `AppEvent::Log` — every `info!`/`warn!` inside core (chip-ID OK, skip-blank
  counts) lands in the log pane automatically, matching the GUI's `app-log`
  event pattern.
- **Main loop:** `rx.recv() → update(&mut app, event) → terminal.draw(...)`.
  Tick (~4 Hz) drives spinners and elapsed-time display.
- **Terminal lifecycle:** raw mode + alternate screen on start; restore on
  drop, on error, and via `std::panic::set_hook` — a panic must never leave
  the user's terminal scrambled. This is the #1 TUI hygiene rule.

## Shared-code extraction (into `minipro-core`)

These currently live in `gui/src-tauri/` and encode upstream-parity
behavior — extracting them is a precondition for P0/P1, not a refactor for
its own sake:

| Item | From | To in core | Needed by |
|------|------|-----------|-----------|
| Fake programmer info (`MINIPRO_FAKE_PROGRAMMER` → synthetic `ProgrammerInfo`) | `commands.rs` | `device::ProgrammerInfo::fake(model)` or `usb::open_or_fake()` | P0 |
| Voltage-override resolution (`apply_voltage_overrides`, table lookup + errors) | `commands.rs` | `operations`/`device` helper | P1 |
| Pin-contact pre-op gate (supported models, ZIF-only, pin_map != 0, emits result) | `commands.rs` `run_pin_check_if_enabled` | `operations::pin_check_gate()` | P1 |
| ICSP mode mapping incl. `MP_ICSP_ONLY`/`MP_ZIF_ONLY` auto-activation | `commands.rs` `set_icsp_from_mode` | `device`/`handle` method | P1 |
| `OperationOptions` vocabulary struct | `commands.rs` | `operations::OpOptions` (rename ok) | P1 |
| `fuse_defs.rs` (5k lines of static bit definitions) | `gui/src-tauri` | `minipro_core::fuse_defs` | P3 |
| Pre-op warning helpers (lock-protection check, `@`-variant warning, no-chip-ID warning) | `commands.rs`/`App.svelte` | `session` helpers or documented recipe | P1 |

Keep the GUI working unchanged: extract = move + re-export, then thin the
Tauri commands down to DTO mapping. Commit extraction separately from TUI
work (`refactor/` branch) so each is reviewable.

## Layout

Mirror the GUI's proven three-column design. Rounded borders
(`BorderType::Rounded`), dim chrome, one accent hue + green/red semantics,
portable glyphs only (● ✓ ✗ ★ ▲▼ ◄) with an `ascii = true` config fallback.

```
╭ minipro-rs 0.9.0 ──── ● TL866II+ · FW 04.2.86 ─────────────── [?] help ╮
│ DEVICES            │ WRITE                          │ LOG              │
│ /search___________ │ Interface  [ZIF socket     ▾]  │ 09:12 connected  │
│ ★ Favorites        │ Page       [Code           ▾]  │ 09:13 AT28C256   │
│   AT28C256         │ Format     [Auto           ▾]  │ 09:14 Read 32KB  │
│   W25Q64JV         │ Size diff  [Error          ▾]  │ 09:14 CRC A1B2.. │
│ ──────────────     │ [x] Chip ID check              │ ──────────────── │
│ 27 results         │ [x] Pin contact check          │ ZIF-40 · DIP8    │
│   AT28C256         │ [ ] Skip erase                 │   ═════════ lever│
│   AT28C64B         │ [x] Unprotect before write     │    ┌────∪────┐   │
│   AT28BV64B        │ [ ] Re-protect after write     │  1 │▓│ ███ │▓│ 40│
│                    │ Advanced ▸                     │  3►│▓│ ███ │▓│ 38│
│ Selected:          │ ────────────────────────────── │  ⋮ │⋮│     │⋮│ ⋮ │
│ AT28C256 · Atmel   │ Buffer  32KB · CRC A1B2C3D4    │ 20 │·│     │·│ 21│
│ 32KB code · DIP28  │ Write source: (f)ile (b)uffer  │    └─────────┘   │
│ ──────────────     │ ▶▶▶ Writing ████████░░  72%    │ Pin test ✗ pin 3 │
│ Diagnostics ▸      │                                │                  │
├────────────────────┴────────────────────────────────┴──────────────────┤
│ ⇥ panes · 1-8 op · Space toggle · ←/→ change · Enter run · / · q quit  │
╰────────────────────────────────────────────────────────────────────────╯
```

(Alignment note: this sketch is exact by character count, but markdown
previewers may render fallback glyphs like `▓ ░ ⋮ ► ∪ ▸ ▾ ✗` at non-uniform
widths — view the raw file in a monospace font for the intended geometry.)

Panes: **Devices** (search field, favorites, filtered results, selected
summary; collapsible Diagnostics below), **Center** (op tabs → options form
→ buffer/progress region), **Right** (log + context diagram — ZIF in ZIF
mode, ICSP table otherwise — + pin-test strip), plus a **context-sensitive
keybar** on the bottom that shows the keys valid for the focused pane
(lazygit-style — this is the TUI replacement for tooltips).

Responsive: ≥100 cols = three columns; 80–100 = collapse right column
behind a toggle; <80×24 = "resize terminal" screen.

## Keymap (proposal)

Global: `q`/`Ctrl+C` quit · `?`/`F1` help overlay · `Tab`/`Shift-Tab` cycle
panes · `F5` reconnect · `,` settings · `Esc` close modal/backs out.

| Context | Keys |
|---------|------|
| Devices | type or `/` to search · `↑/↓` navigate · `Enter` select/deselect · `f` favorite · `a`/`A` SPI autodetect 8/16-pin |
| Operations | `1`–`8` pick op (Read Write Verify Erase Blank Chip-ID Logic Config) · `Enter` start · `Space` toggle · `←/→` cycle select · `o` open file · `s` save buffer |
| Hex (P2) | `hjkl`/arrows · `g`/`G` first/last · `/` find, `n`/`N` next/prev · `v` select · hex digits or chars edit in place · `u`/`Ctrl-r` undo/redo · `+`/`-` bytes-per-row |
| Log | `PgUp/PgDn`/`j`/`k` scroll · `c` copy · `x` clear |
| Batch (P4) | `Enter` next chip · `r` retry · `s` stop |

Disabled actions stay visible but greyed; focusing them shows the reason in
the keybar ("no programmer connected", "not supported on T56"). Op names and
gating rules mirror the GUI (logic ICs disable all ops except Logic Test,
etc.).

## Feature map — GUI → TUI, including idiom translations

| GUI element | TUI treatment | Idiom changes |
|-------------|---------------|---------------|
| Native file dialogs | Path prompt input; P2 mini file-browser popup | **Do not shell out to `rfd`** — jarring over SSH |
| Drag-resize splitters | Fixed ratios; optional `[`/`]` resize focused pane | Usually unnecessary |
| Hover tooltips | Keybar hints + `?` overlay + inline dimmed reasons | |
| Op buttons | `Tabs` widget, `1`–`8` accelerators | Same enable/disable logic |
| Options panel | Form rows: `Space` toggles, `←/→` cycles selects | Avoids nested dropdown popups |
| Hex viewer/editor | Custom widget (hard part, P2) | Overtype model (see ROADMAP — insert mode is won't-fix). `v`+arrows replaces drag-select; `+`/`-` bytes-per-row replaces font zoom |
| Entropy bar | Colored `▌` gutter char per row | Same 4-tier mapping |
| Compare/diff | Toggle diff coloring on buffer + `n`/`N` | Same `smart_diff` core call |
| TerminalLog | Scrolling pane — render ANSI natively | The HTML/whitespace rules from GUI-INTERNALS don't apply |
| ZIF diagram | Box-drawing widget over `core::zif` pin map | Rendering rules proven in `tui_mockup_preview.py`: pin numbers live in fixed label columns; bad-pin markers hug *that pin's own* column (`N►` left / `◄N` right) — never a callout at the row's far end. Chip = `█` block in the socket channel over occupied rows only; `∪` notch centered on the channel; lever = `═` bar flush against the socket's lever end (top on all models except T48 = bottom). `▓` occupied green / red bad |
| ICSP wiring | `Table`: header pin → signal → chip pin + notes | Verified tables already in `core::icsp` |
| Logic test grid | `Table` of colored cells; `+`/`-` cell width | `c` copies TSV via `arboard` |
| Identify results | `Table`, `Enter` select, `f` favorite | Same sort order (pass, fewest errors, alpha) |
| Fuse/config panel | Form table of hex fields + optional bit grid | Needs `fuse_defs` move; keep dangerous-fuse confirm modal |
| Batch + serial | Status bar: `Enter`=next, `r`=retry, `s`=stop | CLI already proves the terminal flow |
| Theme picker | Palette presets (default/dim/high-contrast) | The terminal owns colors — don't fight it |
| Window-size persist | N/A | `directories` config holds everything else |
| `localStorage` settings | `~/.config`-style `tui.toml` | Same key names where they overlap |
| Modals (help, confirms) | Centered modal over dimmed backdrop | `y`/`n`/`Esc`; type-to-confirm is overkill here |
| SPI autodetect buttons | `a`/`A` actions in Devices pane | Same gating (not T56/T76) |
| Programmer badge click | `F5` reconnect + status-line hint | Same retry/timeout behavior |

**Known limitation (design honestly):** core ops are not cancellable — the
GUI's progress panel has no cancel button either, and the progress callback
returns `()`. `Esc` during an op cannot abort a USB transfer. Match the
GUI: block until done. A cooperative abort flag in core's callbacks is a
possible later enhancement — do not promise it.

## Phasing

- **P0 — foundation:** crate skeleton, terminal lifecycle + panic hook,
  layout + keybar, log pane, device search/select/favorites, connect with
  `MINIPRO_FAKE_PROGRAMMER` parity. Core extraction step 1 (fake info).
- **P1 — core workflow (MVP cut):** read/write/verify/erase/blank-check/
  chip-id, options form, path prompt, progress, pin-check gate, lock/variant
  warnings. Core extraction step 2 (op-runner glue). **Ship here first.**
- **P2 — buffer pane:** hex viewer view-only + save, then edit/find/goto/
  diff; ZIF + ICSP diagrams (pure render, high value).
- **P3 — specialist ops:** config/fuse editor (+ `fuse_defs` move), logic
  test + identify, SPI autodetect, pin-test diagram highlighting,
  diagnostics menu (OVC/calibration/hw-check/firmware update).
- **P4 — production/polish:** batch + serial injection, settings screen,
  file-browser popup, mouse support, entropy bar.

## Testing

- `update()` as pure reducers: `AppState + Event → AppState` — unit-test
  all key handling without a terminal.
- `ratatui::backend::TestBackend` for render assertions on widgets
  (hex rows, ZIF diagram, forms).
- `MINIPRO_FAKE_PROGRAMMER=<MODEL>` for end-to-end exercise without hardware
  — same env var as the GUI (this is why the fake-info helper moves to core).

## CI / MSRV / packaging impact

- `crates/minipro-tui` sets `rust-version = "1.88"` (ratatui 0.30.x). The
  GitLab `msrv` job currently runs `cargo check --all --locked` on
  `rust:1.85` — change it to `-p minipro -p minipro-core` (or
  `--exclude minipro-tui`) **in the same commit that adds the crate**, and
  update the MSRV section of AGENTS.md.
- Same gates apply: `cargo fmt --all -- --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test --all --locked`.
- `Cargo.lock` must be regenerated and committed (`cargo generate-lockfile`).
- Release packaging (tarballs/deb/rpm in `docs/RELEASE.md`) gains a second
  binary when the TUI ships — add then, not at P0.

## Doc trail

- This file: `docs/TUI-PLAN.md` (living plan).
- `docs/TUI-INTERNALS.md` — create when P1 lands (keymap table, event
  protocol, widget notes) and register it under "Feature internals" in
  AGENTS.md.
- `CHANGELOG.md` `[Unreleased]` entry on first user-visible merge.
- `ROADMAP.md` entry updated (replaces the old parked stub).
