//! minipro-tui — terminal UI for minipro-rs (P0+P1 MVP).
//!
//! Three-pane ratatui layout per docs/TUI-PLAN.md: device browser,
//! operation tabs/options/progress, log + ZIF diagram. A worker thread
//! owns `MiniproHandle` and runs the six core ops (read/write/verify/
//! erase/blank/chip-id) behind `WorkerCmd`/`AppEvent` channels; the log
//! crate is forwarded into the log pane. Modals cover file paths and
//! destructive-op confirms — no native dialogs.

mod app;
mod events;
mod hexview;
mod theme;
mod worker;
mod zif;

use std::{
    error::Error,
    io,
    sync::mpsc,
    time::{Duration, Instant},
};

use app::{wrap_log, App, ConnState, Modal, OptZone, Pane, OPS, OPT_COLS, SEL_DEFS};
#[cfg(debug_assertions)]
use crossterm::event::{KeyCode, KeyModifiers};
use crossterm::{
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use events::AppEvent;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Gauge, HighlightSpacing, List, ListItem, Paragraph, Tabs},
    Frame, Terminal,
};
use worker::WorkerCmd;

fn main() -> Result<(), Box<dyn Error>> {
    // Load the device DB before touching the terminal — a resolution
    // failure prints to the normal screen, not a raw-mode alt buffer.
    let mut app = App::new()?;
    install_panic_hook();

    // Worker thread + event channel. The logger forwards core's
    // info!/warn! records as AppEvent::Log; PRINT_TO_LOG routes core's
    // outln! diagnostics there too instead of leaking to stderr.
    let (ev_tx, ev_rx) = mpsc::channel::<AppEvent>();
    events::install_logger(ev_tx.clone());
    minipro_core::PRINT_TO_LOG.store(true, std::sync::atomic::Ordering::Relaxed);
    let cmd_tx = worker::spawn(app.paths().clone(), ev_tx);
    app.cmd_tx = Some(cmd_tx.clone());

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    // io::Stdout's LineWriter flushes mid-frame once its ~8KB buffer
    // fills — a resize repaint rewrites every cell (tens of KB), so the
    // terminal composites a torn half-painted screen per repaint and the
    // contents read as jumping vertically. A big BufWriter lands each
    // draw as a single write — one atomic frame.
    let stdout = io::BufWriter::with_capacity(256 * 1024, stdout);
    let mut term = Terminal::new(CrosstermBackend::new(stdout))?;

    // Auto-connect at startup (F5 re-sends this).
    let _ = cmd_tx.send(WorkerCmd::Connect);

    let res = run(&mut term, &mut app, &ev_rx);

    disable_raw_mode()?;
    execute!(term.backend_mut(), LeaveAlternateScreen)?;
    Ok(res?)
}

/// If the app panics, restore the terminal before the panic message
/// prints — otherwise the user's shell is left in raw mode on the
/// alternate screen.
fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        default(info);
    }));
}

fn run<W: io::Write>(
    term: &mut Terminal<CrosstermBackend<W>>,
    app: &mut App,
    ev_rx: &mpsc::Receiver<AppEvent>,
) -> io::Result<()> {
    // MINIPRO_TUI_DEBUG_PAINT (any value) overlays draws-per-second and
    // the painted size — diagnoses whether mid-drag blank frames are
    // starved repaints (0 d/s) or terminal-side compositing.
    let debug = std::env::var_os("MINIPRO_TUI_DEBUG_PAINT").is_some();
    let mut paints = 0u32;
    let mut meter = Instant::now();
    loop {
        // Worker/logger events first — they're cheap and drive progress
        // + log lines. try_recv drains without blocking.
        while let Ok(ev) = ev_rx.try_recv() {
            app.handle_event(ev);
        }
        // Spinner advances once per paint while an op runs (~4 Hz at the
        // poll interval below — the plan's tick cadence).
        if app.op_running.is_some() {
            app.spin = app.spin.wrapping_add(1);
        }
        // Transient degenerate sizes (0 rows/cols) surface mid-drag on
        // conpty — drawing at one clears the screen to blank.
        let (w, h) = crossterm::terminal::size().unwrap_or((0, 0));
        if w > 0 && h > 0 {
            term.draw(|f| draw(f, app))?;
            paints += 1;
        }
        if debug && meter.elapsed() >= Duration::from_secs(1) {
            app.debug_paint = Some((paints, w, h));
            paints = 0;
            meter = Instant::now();
        }
        // 100 ms poll: worker events don't wake it, but progress is
        // drained every loop anyway — ~10 Hz worst-case event latency.
        if !event::poll(Duration::from_millis(100))? {
            continue;
        }
        // Drain the queued events before the next draw — a resize drag
        // streams them — but bound the drain so a continuous flood
        // can't starve repaints entirely (the mid-drag "contents
        // disappear"). No further paint throttling: delayed paints
        // leave a stale too-wide frame that reflow-wraps into a scroll.
        let mut quit = false;
        let deadline = Instant::now() + Duration::from_millis(8);
        loop {
            if let Event::Key(k) = event::read()? {
                // Debug-only panic trigger: verifies the panic hook
                // restores the terminal. Ctrl+P → panic.
                #[cfg(debug_assertions)]
                if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('p') {
                    panic!("minipro-tui: debug panic trigger (terminal restore test)");
                }
                if app.handle_key(k) {
                    quit = true;
                }
            }
            if Instant::now() >= deadline || !event::poll(Duration::ZERO)? {
                break;
            }
        }
        if quit {
            break;
        }
    }
    Ok(())
}

/// Header badge for the current connection state.
fn conn_badge(app: &App) -> Line<'static> {
    match &app.conn {
        ConnState::Connected { info, fake } => Line::from(vec![
            sp(" ● ", theme::green()),
            sp(
                format!(
                    "{} · FW {} · S/N {}",
                    info.model, info.firmware_str, info.serial_number
                ),
                if *fake { theme::amber() } else { theme::dim() },
            ),
            if *fake {
                sp(" (FAKE)", theme::amber())
            } else {
                Span::raw("")
            },
            Span::raw(" "),
        ]),
        ConnState::Connecting => Line::from(vec![
            sp(" … ", theme::amber()),
            sp("connecting", theme::dim()),
            Span::raw(" "),
        ]),
        ConnState::Failed(e) => Line::from(vec![
            sp(" ○ ", theme::red()),
            sp(
                format!("{} — F5", e.chars().take(40).collect::<String>()),
                theme::dim(),
            ),
            Span::raw(" "),
        ]),
        ConnState::Disconnected => Line::from(vec![
            sp(" ○ ", theme::dim()),
            sp("disconnected — F5", theme::faint()),
            Span::raw(" "),
        ]),
    }
}

fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    app.note_size(area.width, area.height);
    if !app.right_visible() && app.pane == Pane::Right {
        app.pane = Pane::Options;
    }
    // ~80×24 minimum per the plan's responsive rule (dead-banded in
    // note_size) — a message screen, not a squashed layout. handle_key
    // still runs, so q/Ctrl+C quit.
    if app.too_small {
        resize_screen(f, area);
        return;
    }
    let outer = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::border())
        .title(Span::styled(
            format!(" minipro-rs {} ", env!("CARGO_PKG_VERSION")),
            theme::text(),
        ))
        .title(conn_badge(app).alignment(Alignment::Center))
        // In the hex view's ASCII mode '?' types 0x3F, so the hint
        // switches to the always-live F1 at the moment it matters.
        .title(
            Line::from(Span::styled(
                if app.hex_view && app.hex.as_ref().is_some_and(|h| h.ascii) {
                    " [F1] help "
                } else {
                    " [?] help "
                },
                theme::dim(),
            ))
            .alignment(Alignment::Right),
        );
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    let v = Layout::vertical([
        Constraint::Length(1), // pane separator
        Constraint::Min(0),    // panes
        Constraint::Length(1), // keybar
    ])
    .split(inner);

    // 80–100 cols: the right pane collapses behind `]` (app decides via
    // right_visible). Options expands to fill when it's hidden.
    let right_visible = app.right_visible();
    // DEVICES grows with the window: 26-col floor for narrow terminals,
    // quarter-width capped at 44 — the longest name in the database is 39
    // chars (Micron MT35X parts), so most rows render unellipsized on
    // wide terminals instead of leaving the spare columns to the center.
    let dev_w = (v[1].width / 4).clamp(26, 44);
    let h = Layout::horizontal(if right_visible {
        vec![
            Constraint::Length(dev_w), // DEVICES
            Constraint::Length(1),     // sep
            Constraint::Min(0),        // options/buffer/progress
            Constraint::Length(1),     // sep
            Constraint::Length(34),    // LOG + ZIF
        ]
    } else {
        vec![
            Constraint::Length(dev_w),
            Constraint::Length(1),
            Constraint::Min(0),
        ]
    })
    .split(v[1]);
    let right_area = if right_visible { Some(h[4]) } else { None };

    // Pane-top separator — part of the frame look in every view.
    {
        let buf = f.buffer_mut();
        let y = v[0].y;
        for x in area.x..area.x + area.width {
            buf.set_string(x, y, "─", theme::border());
        }
        buf.set_string(area.x, y, "├", theme::border());
        buf.set_string(area.x + area.width - 1, y, "┤", theme::border());
    }

    // The hex-buffer view owns the pane region when open — frame,
    // separator, keybar, and overlays still draw around it.
    if app.hex_view {
        hexview::draw(f, v[1], app);
        keybar(f, v[2], app);
        if app.show_help {
            help_overlay(f, area);
        }
        if let Some(m) = &app.modal {
            modal_overlay(f, area, m);
        }
        return;
    }

    let buf = f.buffer_mut();
    let y = v[0].y;
    let mut seps = vec![h[1]];
    if right_visible {
        seps.push(h[3]);
    }
    for col in seps {
        buf.set_string(col.x, y, "┬", theme::border());
        for yy in col.y..col.y + col.height {
            buf.set_string(col.x, yy, "│", theme::border());
        }
    }

    // Focus cue: accent the focused pane's left + right + top edges.
    // Edge panes borrow the outer frame column as one side. Recolour
    // existing cells only — junctions (├ ┬ ┤ ╭ ╰) keep their glyphs.
    let frame_r = area.x + area.width - 1;
    let (lx, rx) = match app.pane {
        Pane::Devices => (area.x, h[1].x),
        Pane::Options => (h[1].x, if right_visible { h[3].x } else { frame_r }),
        Pane::Right => (h[3].x, frame_r),
    };
    for x in lx..=rx {
        buf[(x, y)].set_style(theme::accent());
    }
    for yy in v[1].y..v[1].y + v[1].height {
        buf[(lx, yy)].set_style(theme::accent());
        buf[(rx, yy)].set_style(theme::accent());
    }

    // Collapsed affordance: ◄ on the frame edge — "] pulls it open".
    if !right_visible {
        buf.set_string(frame_r, v[1].y + v[1].height / 2, "◄", theme::accent());
    }

    devices_pane(f, h[0], app);
    center_pane(f, h[2], app);
    if let Some(ra) = right_area {
        right_pane(f, ra, app);
    }
    keybar(f, v[2], app);

    if app.show_help {
        help_overlay(f, area);
    }
    if let Some(m) = &app.modal {
        modal_overlay(f, area, m);
    }

    // Debug paint meter (MINIPRO_TUI_DEBUG_PAINT) — right end of keybar.
    if let Some((paints, w, h)) = app.debug_paint {
        let s = format!(" {paints}d/s {w}×{h} ");
        let x = area.x + area.width.saturating_sub(s.len() as u16 + 1);
        f.buffer_mut().set_string(x, v[2].y, s, theme::accent());
    }
}

/// <80×24 fallback — the plan's "resize terminal" screen.
fn resize_screen(f: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from(sp("minipro-tui needs at least 80 × 24", theme::text())),
        Line::from(sp(
            format!("current: {} × {}", area.width, area.height),
            theme::dim(),
        )),
        Line::from(sp("resize the terminal · q to quit", theme::faint())),
    ];
    let r = Rect::new(
        area.x,
        area.y + area.height.saturating_sub(3) / 2,
        area.width,
        3,
    );
    f.render_widget(Paragraph::new(lines).alignment(Alignment::Center), r);
}

fn sp(s: impl Into<String>, style: Style) -> Span<'static> {
    Span::styled(s.into(), style)
}

fn hdr(title: &'static str, focused: bool) -> Line<'static> {
    Line::from(vec![
        Span::raw(" "),
        sp(
            title,
            if focused {
                theme::accent_bold()
            } else {
                theme::dim()
            },
        ),
    ])
}

fn devices_pane(f: &mut Frame, area: Rect, app: &mut App) {
    let focused = app.pane == Pane::Devices;
    let v = Layout::vertical([
        Constraint::Length(1), // header
        Constraint::Length(3), // search box
        Constraint::Length(1), // sep
        Constraint::Length(1), // results header
        Constraint::Min(0),    // results list (favorites pinned on top)
        Constraint::Length(1), // sep
        Constraint::Length(5), // selected
        Constraint::Length(1), // sep
        Constraint::Length(1), // diagnostics
    ])
    .split(area);

    f.render_widget(Paragraph::new(hdr("DEVICES", focused)), v[0]);

    // Search box — accent border while editing; ▌ marks the text cursor.
    let editing = app.search_editing;
    let box_border = if editing {
        theme::accent()
    } else {
        theme::border()
    };
    let mut search_spans = vec![sp("/", theme::accent())];
    if editing {
        search_spans.push(sp(&app.search[..app.search_cursor], theme::text()));
        search_spans.push(sp("▌", theme::accent()));
        search_spans.push(sp(&app.search[app.search_cursor..], theme::text()));
    } else {
        search_spans.push(sp(app.search.clone(), theme::text()));
        if app.search.is_empty() {
            search_spans.push(sp("type / to search", theme::faint()));
        }
    }
    f.render_widget(
        Paragraph::new(Line::from(search_spans)).block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(box_border),
        ),
        v[1],
    );

    f.render_widget(Paragraph::new(sep_line(area.width)), v[2]);

    // Results header — filtered count plus the pinned-favorites count
    // and a scroll hint driven by ListState.
    let off = app.list.offset();
    let more_below = off + v[4].height as usize + 1 < app.matches.len();
    let hint = match (off > 0, more_below) {
        (true, true) => "↑↓",
        (true, false) => "↑",
        (false, true) => "↓",
        (false, false) => "",
    };
    let mut rh = vec![sp(
        format!(" {} results", app.matches.len() - app.fav_rows),
        theme::dim(),
    )];
    if app.fav_rows > 0 {
        rh.push(sp(format!(" · ★{} pinned", app.fav_rows), theme::amber()));
    }
    if !hint.is_empty() {
        rh.push(sp(format!("{:>10}", hint), theme::faint()));
    }
    f.render_widget(Paragraph::new(Line::from(rh)), v[3]);

    let list_area = Rect {
        x: v[4].x + 2,
        width: v[4].width.saturating_sub(2),
        ..v[4]
    };
    let row_w = list_area.width.saturating_sub(1) as usize; // star col
    let items: Vec<ListItem> = app
        .matches
        .iter()
        .map(|&i| {
            let d = &app.devices[i];
            let is_fav = app.favorites.iter().any(|f| f.name == d.name);
            let nl = d.name.chars().count();
            let mw = d.manufacturer.chars().count();
            // star + name + right-aligned mfr. Priority: the full name
            // over the mfr — the Selected panel always shows both.
            // A name that still overflows gets a middle ellipsis: the
            // tail carries the discriminator (@DIP28, (OTP), @TSSOP8).
            let show_mfr = nl + mw < row_w;
            let name_w = if show_mfr { row_w - mw - 1 } else { row_w };
            let mut spans = vec![
                sp(if is_fav { "★" } else { " " }, theme::amber()),
                sp(
                    format!("{:<w$}", ellipsize(&d.name, name_w), w = name_w),
                    theme::text(),
                ),
            ];
            if show_mfr {
                spans.push(sp(d.manufacturer.clone(), theme::dim()));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();
    let list = List::new(items)
        .highlight_style(theme::sel())
        .highlight_symbol("▶")
        .highlight_spacing(HighlightSpacing::Always);
    f.render_stateful_widget(list, list_area, &mut app.list);

    f.render_widget(Paragraph::new(sep_line(area.width)), v[5]);
    f.render_widget(Paragraph::new(selected_lines(app)), v[6]);
    f.render_widget(Paragraph::new(sep_line(area.width)), v[7]);
    f.render_widget(
        Paragraph::new(Line::from(sp(" Diagnostics ▸", theme::faint()))),
        v[8],
    );
}

/// Selected panel: cursor item's name/mfr always; Enter resolves the
/// full record and adds size, pin count, and chip-ID support.
fn selected_lines(app: &App) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(vec![
        Span::raw(" "),
        sp("Selected", theme::accent()),
    ])];
    let Some(&idx) = app.matches.get(app.list.selected().unwrap_or(0)) else {
        lines.push(Line::from(sp(" (no match)", theme::dim())));
        return lines;
    };
    let item = &app.devices[idx];
    lines.push(Line::from(sp(format!(" {}", item.name), theme::text())));
    lines.push(Line::from(sp(
        format!(" {}", item.manufacturer),
        theme::dim(),
    )));
    match &app.selected {
        Some(d) if d.name == item.name => {
            if d.chip_type == minipro_core::device::ChipType::Logic as u32 {
                lines.push(Line::from(sp(" logic IC — Logic op only", theme::amber())));
            } else {
                lines.push(Line::from(vec![
                    Span::raw(" "),
                    sp(
                        format!(
                            "{} code · {}-pin",
                            human_size(d.code_memory_bytes()),
                            d.package_details.pin_count
                        ),
                        theme::dim(),
                    ),
                ]));
                lines.push(Line::from(vec![
                    Span::raw(" "),
                    sp("Chip ID ", theme::dim()),
                    if d.chip_id_bytes_count > 0 {
                        sp("✓", theme::green())
                    } else {
                        sp("✗", theme::faint())
                    },
                ]));
            }
        }
        _ => lines.push(Line::from(sp(" Enter — details", theme::faint()))),
    }
    lines
}

fn human_size(b: usize) -> String {
    if b >= 1 << 20 {
        format!("{} MB", b >> 20)
    } else if b >= 1 << 10 {
        format!("{} KB", b >> 10)
    } else {
        format!("{b} B")
    }
}

/// Middle-ellipsize to `max` display chars — keeps the tail, where
/// device names carry their discriminator (`@DIP28`, `(OTP)`,
/// `@TSSOP8`). Char-safe; head gets the extra char on odd budgets.
fn ellipsize(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    if max <= 1 {
        return "…".into();
    }
    let keep = max - 1;
    // Tail-weighted: device names front-load family (AUTOEMMC, W25Q) and
    // back-load the discriminator (@TSSOP8, (OTP), (recommend)), so the
    // tail gets ~2/3 of the budget.
    let tail = (keep * 2).div_ceil(3);
    let head = keep - tail;
    let head: String = s.chars().take(head).collect();
    let tail: String = s
        .chars()
        .rev()
        .take(tail)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{head}…{tail}")
}

fn sep_line(w: u16) -> Line<'static> {
    Line::from(Span::styled(
        format!(" {}", "─".repeat((w as usize).saturating_sub(3))),
        theme::faint(),
    ))
}

fn center_pane(f: &mut Frame, area: Rect, app: &App) {
    let focused = app.pane == Pane::Options;
    let v = Layout::vertical([
        Constraint::Length(1), // op tabs
        Constraint::Min(0),    // options/buffer
        Constraint::Length(2), // progress
    ])
    .split(area);
    let mut hi = theme::accent_bold();
    if focused {
        hi = hi.add_modifier(Modifier::UNDERLINED);
    }
    // Underline each tab's mnemonic letter — the key runs that op.
    let items = OPS.iter().map(|label| {
        let (mn, rest) = label.split_at(1);
        Line::from(vec![
            Span::styled(
                mn.to_string(),
                theme::accent().add_modifier(Modifier::UNDERLINED),
            ),
            Span::raw(rest.to_string()),
        ])
    });
    f.render_widget(
        Tabs::new(items)
            .select(app.op)
            .style(theme::dim())
            .highlight_style(hi)
            .divider(Span::raw("   ")),
        v[0],
    );
    let w = v[1].width;
    // Select rows — real values, cursor per opt_zone.
    let mut lines = vec![Line::default()];
    for (i, (name, vals)) in SEL_DEFS.iter().enumerate() {
        let cur = focused && app.opt_zone == OptZone::Selects && app.sel_row == i;
        let val = vals[app.selects[i].min(vals.len() - 1)];
        let mut spans = vec![Span::raw(format!(" {name:<13}"))];
        if cur {
            spans.push(sp(format!("[{val:<14}]"), theme::sel()));
            spans.push(sp("▾", theme::accent()));
        } else {
            spans.push(sp(format!("[{val:<14}]",), theme::text()));
            spans.push(sp("▾", theme::accent()));
        }
        lines.push(Line::from(spans));
    }
    lines.push(Line::default());
    // Two checkbox columns; the option cursor is row+col per OPT_COLS.
    for row in 0..OPT_COLS[0].len().max(OPT_COLS[1].len()) {
        let mut spans = Vec::new();
        for (col, labels) in OPT_COLS.iter().enumerate() {
            let base = if col == 0 { 0 } else { OPT_COLS[0].len() };
            if let Some(&label) = labels.get(row) {
                let i = base + row;
                let cur = focused
                    && app.opt_zone == OptZone::Checks
                    && app.opt_col == col
                    && app.opt_row == row;
                if cur {
                    spans.push(sp(" ▸", theme::accent()));
                } else {
                    spans.push(Span::raw("  "));
                }
                let st = if cur { theme::sel() } else { theme::text() };
                spans.push(sp(
                    format!("[{}] {:<20}", if app.options[i] { "x" } else { " " }, label),
                    st,
                ));
            } else {
                spans.push(Span::raw(" ".repeat(25)));
            }
        }
        lines.push(Line::from(spans));
    }
    lines.push(Line::default());
    lines.push(Line::from(vec![
        sp(" ▸ Advanced", theme::faint()),
        sp("  (VPP / VCC / VDD overrides — later)", theme::faint()),
    ]));
    lines.push(sep_line(w));
    // Buffer line — the hex buffer's name/size/CRC, or the empty state.
    match (&app.hex, &app.last_path) {
        (Some(hex), path) => {
            let name = path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_else(|| "(buffer)".to_string());
            lines.push(Line::from(vec![
                sp(" Buffer  ", theme::dim()),
                sp(name, theme::text()),
                sp(format!("  · {} B", hex.data.len()), theme::dim()),
                if hex.edited.is_empty() {
                    Span::raw("")
                } else {
                    sp(format!("  · {} edit(s)", hex.edited.len()), theme::amber())
                },
            ]));
            lines.push(Line::from(vec![
                Span::raw("          "),
                sp(format!("CRC-32 {:08X}", hex.crc32), theme::cyan()),
                sp("  ·  o view", theme::faint()),
            ]));
        }
        _ => {
            lines.push(Line::from(vec![
                sp(" Buffer  ", theme::dim()),
                sp("—", theme::faint()),
            ]));
        }
    }
    lines.push(sep_line(w));
    lines.push(Line::from(vec![
        sp(" Enter", theme::accent()),
        sp(" run op", theme::text()),
        sp("    Space", theme::accent()),
        sp(" toggle", theme::text()),
        sp("    ←→", theme::accent()),
        sp(" change", theme::text()),
    ]));
    lines.push(Line::default());
    f.render_widget(Paragraph::new(lines), v[1]);
    let p = Layout::horizontal([
        Constraint::Length(13),
        Constraint::Min(0),
        Constraint::Length(5),
    ])
    .split(v[2]);
    // Progress: live while an op runs, idle otherwise.
    const SPINNER: [char; 4] = ['◐', '◓', '◑', '◒'];
    match (app.op_running, app.progress) {
        (Some(op), Some((done, total))) => {
            let ratio = if total > 0 {
                done as f64 / total as f64
            } else {
                0.0
            };
            f.render_widget(
                Paragraph::new(sp(
                    format!(" {} {}", SPINNER[app.spin as usize % 4], op.name()),
                    theme::accent(),
                )),
                p[0],
            );
            f.render_widget(
                Gauge::default()
                    .ratio(ratio.clamp(0.0, 1.0))
                    .gauge_style(Style::default().fg(theme::GREEN).bg(theme::PROG_EMPTY)),
                p[1],
            );
            f.render_widget(
                Paragraph::new(sp(format!(" {:>3.0}%", ratio * 100.0), theme::dim())),
                p[2],
            );
        }
        _ => {
            f.render_widget(Paragraph::new(sp(" · idle", theme::faint())), p[0]);
            f.render_widget(
                Gauge::default()
                    .ratio(0.0)
                    .gauge_style(Style::default().fg(theme::GREEN).bg(theme::PROG_EMPTY)),
                p[1],
            );
            f.render_widget(Paragraph::new(sp("", theme::dim())), p[2]);
        }
    }
}

fn right_pane(f: &mut Frame, area: Rect, app: &mut App) {
    let focused = app.pane == Pane::Right;
    let v = Layout::vertical([
        Constraint::Length(8), // LOG
        Constraint::Length(1), // sep
        Constraint::Min(0),    // ZIF diagram
        Constraint::Length(1), // sep
        Constraint::Length(1), // pin-test strip
    ])
    .split(area);

    // Log pane — newest at the bottom; log_scroll shifts the window up.
    // GUI-style rows: colored [LEVEL] tag + message, word-wrapped to the
    // pane width (no horizontal scroll exists, so wrap or lose text).
    let log_h = v[0].height.saturating_sub(1) as usize;
    let width = v[0].width as usize;
    let mut rows: Vec<Line> = Vec::new();
    for l in &app.log_lines {
        let (tag, tag_style) = match l.level {
            log::Level::Error => ("[ERROR]", theme::red()),
            log::Level::Warn => ("[WARN]", theme::amber()),
            _ => ("[INFO]", theme::green()),
        };
        let indent = 1 + tag.len() + 1;
        let pad = " ".repeat(indent);
        for (i, chunk) in wrap_log(&l.msg, width.saturating_sub(indent))
            .iter()
            .enumerate()
        {
            if i == 0 {
                rows.push(Line::from(vec![
                    Span::raw(" "),
                    Span::styled(tag, tag_style),
                    Span::raw(" "),
                    Span::styled(chunk.clone(), theme::text()),
                ]));
            } else {
                rows.push(Line::from(vec![
                    Span::raw(pad.clone()),
                    Span::styled(chunk.clone(), theme::text()),
                ]));
            }
        }
    }
    app.log_scroll_max = rows.len().saturating_sub(log_h);
    app.log_scroll = app.log_scroll.min(app.log_scroll_max);
    let total = rows.len();
    let end = total.saturating_sub(app.log_scroll);
    let start = end.saturating_sub(log_h);
    let mut log: Vec<Line> = vec![hdr("LOG", focused)];
    log.extend(rows.into_iter().skip(start).take(end - start));
    if total == 0 {
        log.push(Line::from(sp(" —", theme::faint())));
    }
    f.render_widget(Paragraph::new(log), v[0]);
    f.render_widget(Paragraph::new(sep_line(v[1].width)), v[1]);

    let zif = app.zif_diagram();
    f.render_widget(&zif, v[2]);
    f.render_widget(Paragraph::new(sep_line(v[3].width)), v[3]);

    // Pin-test strip — mirrors the ZIF colors.
    let strip = match &app.pin_test {
        Some(pt) if !pt.bad_device_pins.is_empty() => {
            let pins = pt
                .bad_device_pins
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join(",");
            if pt.all_open {
                Line::from(vec![
                    Span::raw(" "),
                    sp("✗", theme::amber()),
                    sp(" Pin test: ", theme::text()),
                    sp("all pins open — chip seated?", theme::amber()),
                ])
            } else {
                Line::from(vec![
                    Span::raw(" "),
                    sp("✗", theme::red()),
                    sp(" Pin test: ", theme::text()),
                    sp(format!("bad contact pin {pins}"), theme::red()),
                ])
            }
        }
        Some(pt) => Line::from(vec![
            Span::raw(" "),
            sp("✓", theme::green()),
            sp(
                if pt.untestable.is_empty() {
                    " Pin test: all pins OK"
                } else {
                    " Pin test: all testable pins OK"
                },
                theme::green(),
            ),
        ]),
        None => Line::from(vec![Span::raw(" "), sp("Pin test: —", theme::faint())]),
    };
    f.render_widget(Paragraph::new(strip), v[4]);
}

/// Centered help modal — the first overlay; path/confirm dialogs reuse
/// this pattern. Lists only keys that are actually bound.
fn help_overlay(f: &mut Frame, area: Rect) {
    let w = 68u16.min(area.width.saturating_sub(4));
    let h = 42u16.min(area.height.saturating_sub(2));
    let r = Rect::new(
        area.x + area.width.saturating_sub(w) / 2,
        area.y + area.height.saturating_sub(h) / 2,
        w,
        h,
    );
    f.render_widget(Clear, r);
    let row = |keys: &'static str, desc: &'static str| {
        Line::from(vec![
            Span::raw("   "),
            sp(format!("{keys:<16}"), theme::accent()),
            sp(desc, theme::text()),
        ])
    };
    let section = |t: &'static str| Line::from(vec![Span::raw(" "), sp(t, theme::accent_bold())]);
    let lines = vec![
        Line::default(),
        section("Global"),
        row("q / Ctrl+C", "quit (blocked while an op runs)"),
        row("? / F1", "this help · close"),
        row("Tab / S-Tab", "cycle panes"),
        row("F5", "(re)connect programmer"),
        row("Esc", "back / quit"),
        Line::default(),
        section("Devices"),
        row("/", "edit search (chars insert)"),
        row("↑ ↓ PgUp PgDn", "move cursor (search results too)"),
        row("Home · End", "first · last match"),
        row("Enter", "select (search: select + exit)"),
        row("Esc", "cancel search · quit"),
        row("f · Ctrl-F", "favorite (Ctrl-F while typing too)"),
        Line::default(),
        section("Operations"),
        row("r/w/v/e/b/i", "run op (read → buffer; else prompts)"),
        row("l · c", "logic / config — not in this MVP"),
        row("1-8", "arm op, Enter runs"),
        row("Space", "toggle option · ←→ cycle selects"),
        row("o", "hex buffer view / load file"),
        Line::default(),
        section("Hex buffer view"),
        row("←→↑↓ PgUp/PgDn", "move cursor · g/G first/last"),
        row("Home · End · :", "row start/end · goto offset"),
        row("0-9a-f", "overtype byte (half-typed = x·)"),
        row("Tab · Esc", "hex/ASCII toggle (i/Ins enter too)"),
        row("/ · n/N · F3", "find (x: = hex) · repeat"),
        row("Backspace", "undo previous edit keystroke"),
        row("o · s · w · v", "load · save · write chip · verify"),
        row("Esc / q / x", "back to panes"),
        Line::default(),
        section("Log (right pane)"),
        row("j/k · PgUp/PgDn", "scroll · x clear"),
        Line::default(),
        Line::from(Span::styled(
            "  Esc / q / ? / Enter / F1 — close",
            theme::faint(),
        )),
    ];
    f.render_widget(
        Paragraph::new(lines).block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(theme::accent())
                .title(Span::styled(" Help — keys ", theme::accent_bold())),
        ),
        r,
    );
}

/// Centered modal for path prompts and destructive-op confirms.
fn modal_overlay(f: &mut Frame, area: Rect, m: &Modal) {
    let (title, body): (&str, Vec<Line>) = match m {
        Modal::Path {
            title,
            input,
            cursor,
            ..
        } => {
            let mut spans = vec![
                sp(&input[..*cursor], theme::text()),
                sp("▌", theme::accent()),
            ];
            spans.push(sp(&input[*cursor..], theme::text()));
            // Resolved-path preview — where the file actually lands.
            let resolved = App::resolve_path(input.trim()).display().to_string();
            (
                title,
                vec![
                    Line::default(),
                    Line::from(spans),
                    Line::from(vec![sp(" → ", theme::faint()), sp(resolved, theme::dim())]),
                    Line::default(),
                    Line::from(vec![
                        sp(" Enter", theme::accent()),
                        sp(" run  ·  ", theme::dim()),
                        sp("Esc", theme::accent()),
                        sp(" cancel", theme::dim()),
                    ]),
                ],
            )
        }
        Modal::Goto { input, cursor } => {
            let mut spans = vec![
                sp(&input[..*cursor], theme::text()),
                sp("▌", theme::accent()),
            ];
            spans.push(sp(&input[*cursor..], theme::text()));
            (
                "Go to offset (hex)",
                vec![
                    Line::default(),
                    Line::from(spans),
                    Line::default(),
                    Line::from(vec![
                        sp(" Enter", theme::accent()),
                        sp(" jump  ·  ", theme::dim()),
                        sp("Esc", theme::accent()),
                        sp(" cancel", theme::dim()),
                    ]),
                ],
            )
        }
        Modal::Find { input, cursor } => {
            let mut spans = vec![
                sp(&input[..*cursor], theme::text()),
                sp("▌", theme::accent()),
            ];
            spans.push(sp(&input[*cursor..], theme::text()));
            (
                "Find (text · x:hex pairs)",
                vec![
                    Line::default(),
                    Line::from(spans),
                    Line::default(),
                    Line::from(vec![
                        sp(" Enter", theme::accent()),
                        sp(" find next  ·  ", theme::dim()),
                        sp("Esc", theme::accent()),
                        sp(" cancel", theme::dim()),
                    ]),
                ],
            )
        }
        Modal::Confirm { lines, .. } => {
            let mut body: Vec<Line> = vec![Line::default()];
            for (i, l) in lines.iter().enumerate() {
                body.push(Line::from(sp(
                    l.clone(),
                    if i == 0 { theme::amber() } else { theme::dim() },
                )));
            }
            body.push(Line::default());
            body.push(Line::from(vec![
                sp(" y", theme::accent()),
                sp(" confirm  ·  ", theme::dim()),
                sp("n/Esc", theme::accent()),
                sp(" cancel", theme::dim()),
            ]));
            ("Confirm", body)
        }
    };
    let w = 52u16.min(area.width.saturating_sub(4));
    let h = (body.len() as u16 + 2).min(area.height.saturating_sub(2));
    let r = Rect::new(
        area.x + area.width.saturating_sub(w) / 2,
        area.y + area.height.saturating_sub(h) / 2,
        w,
        h,
    );
    f.render_widget(Clear, r);
    f.render_widget(
        Paragraph::new(body).block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(theme::amber())
                .title(Span::styled(format!(" {title} "), theme::accent_bold())),
        ),
        r,
    );
}

fn keybar(f: &mut Frame, area: Rect, app: &App) {
    let key = |s: &'static str| Span::styled(s.to_string(), theme::accent_bold().bg(theme::KEY_BG));
    let sep = |s: &'static str| Span::styled(s.to_string(), theme::dim().bg(theme::KEY_BG));
    // While an op runs, the bar explains why keys are dead — the GUI's
    // modal-progress equivalent. (First: it also wins inside the hex
    // view, where edits/actions are blocked but nav stays live.)
    if app.op_running.is_some() {
        let line = Line::from(vec![
            key(" ◐"),
            sep(" running — ops can't be aborted mid-transfer "),
        ]);
        f.render_widget(
            Paragraph::new(line).style(Style::default().bg(theme::KEY_BG)),
            area,
        );
        return;
    }
    // Hex-buffer view context — its own key set (hex digits are data).
    if app.hex_view {
        let mut spans = vec![
            key(" ←→↑↓"),
            sep(" move  ·  "),
            key("g/G"),
            sep(" top/end  ·  "),
            key(":"),
            sep(" goto  ·  "),
            key("/"),
            sep(" find  ·  "),
            key("0-9a-f"),
            sep(" edit  ·  "),
            key("Tab"),
            sep(" ascii  ·  "),
        ];
        if area.width >= 100 {
            spans.extend([key("o s w v"), sep(" open/save/write/verify  ·  ")]);
        } else {
            spans.extend([key("o s w v"), sep(" buf  ·  ")]);
        }
        spans.extend([key("Esc"), sep(" back ")]);
        let line = Line::from(spans);
        f.render_widget(
            Paragraph::new(line).style(Style::default().bg(theme::KEY_BG)),
            area,
        );
        return;
    }
    let mut spans = vec![
        key(" ⇥"),
        sep(" panes  ·  "),
        key("r w v e b i"),
        sep(" op  ·  "),
    ];
    // Compact variant below 100 cols — the full set would clip mid-hint.
    if area.width >= 100 {
        spans.extend([
            key("Enter"),
            sep(" run  ·  "),
            key("Space"),
            sep(" toggle  ·  "),
            key("←→"),
            sep(" change  ·  "),
            key("F5"),
            sep(" conn  ·  "),
        ]);
    }
    spans.extend([
        key("/"),
        sep(" search  ·  "),
        key("]"),
        sep(" right  ·  "),
        key("q"),
        sep(" quit "),
    ]);
    let line = Line::from(spans);
    f.render_widget(
        Paragraph::new(line).style(Style::default().bg(theme::KEY_BG)),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::ellipsize;

    #[test]
    fn ellipsize_middle() {
        // Fits → unchanged.
        assert_eq!(ellipsize("W25Q32JV", 20), "W25Q32JV");
        assert_eq!(ellipsize("W25Q32JV", 8), "W25Q32JV");
        // Overflows → middle cut, tail-weighted (~2/3 tail): the
        // @package/(qualifier) suffix is where device names carry their
        // discriminator.
        assert_eq!(ellipsize("ACE25C512C(OTP)@TSSOP8", 16), "ACE25…TP)@TSSOP8");
        assert_eq!(ellipsize("ATMEGA328P@TQFP32", 11), "ATM…@TQFP32");
        // Whole trailing tokens survive when they fit the tail budget.
        assert_eq!(
            ellipsize("AUTOEMMC(ISP)_1B_1.8V(recommend)", 23),
            "AUTOEMM…1.8V(recommend)"
        );
        assert_eq!(ellipsize("ABCDEFGHIJ", 7), "AB…GHIJ");
        // Degenerate widths.
        assert_eq!(ellipsize("ABCDEF", 1), "…");
    }
}
