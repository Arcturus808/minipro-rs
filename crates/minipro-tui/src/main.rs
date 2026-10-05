//! minipro-tui — scaffold.
//!
//! Renders the docs/TUI-PLAN.md mockup with real ratatui widgets. The
//! device list and search run against the real `minipro-core` database;
//! operation options track real state. Hardware operations are not wired
//! yet — see docs/TUI-PLAN.md for the roadmap.

mod app;
mod theme;
mod zif;

use std::{error::Error, io, time::Duration};

use app::{App, Pane, OPS, OPT_COLS};
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Gauge, HighlightSpacing, List, ListItem, Paragraph, Tabs},
    Frame, Terminal,
};
use zif::{Justify, Lever, ZifDiagram};

fn main() -> Result<(), Box<dyn Error>> {
    // Load the device DB before touching the terminal — a resolution
    // failure prints to the normal screen, not a raw-mode alt buffer.
    let mut app = App::new()?;
    install_panic_hook();

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let mut term = Terminal::new(CrosstermBackend::new(stdout))?;

    let res = run(&mut term, &mut app);

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

fn run(term: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> io::Result<()> {
    loop {
        term.draw(|f| draw(f, app))?;
        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(k) = event::read()? {
                // Debug-only panic trigger: verifies the panic hook
                // restores the terminal. Ctrl+P → panic.
                #[cfg(debug_assertions)]
                if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('p') {
                    panic!("minipro-tui: debug panic trigger (terminal restore test)");
                }
                if app.handle_key(k) {
                    break;
                }
            }
        }
    }
    Ok(())
}

fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let outer = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::border())
        .title(Span::styled(" minipro-rs 0.9.0 ", theme::text()))
        .title(
            Line::from(Span::styled(
                " ● TL866II+ · FW 04.2.86 · S/N 5A3F ",
                theme::dim(),
            ))
            .alignment(Alignment::Center),
        )
        .title(Line::from(Span::styled(" [?] help ", theme::dim())).alignment(Alignment::Right));
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    let v = Layout::vertical([
        Constraint::Length(1), // pane separator
        Constraint::Min(0),    // panes
        Constraint::Length(1), // keybar
    ])
    .split(inner);

    let h = Layout::horizontal([
        Constraint::Length(26), // DEVICES
        Constraint::Length(1),  // sep
        Constraint::Min(0),     // options/buffer/progress
        Constraint::Length(1),  // sep
        Constraint::Length(34), // LOG + ZIF
    ])
    .split(v[1]);

    let buf = f.buffer_mut();
    let y = v[0].y;
    for x in area.x..area.x + area.width {
        buf.set_string(x, y, "─", theme::border());
    }
    buf.set_string(area.x, y, "├", theme::border());
    buf.set_string(area.x + area.width - 1, y, "┤", theme::border());
    for col in [h[1], h[3]] {
        buf.set_string(col.x, y, "┬", theme::border());
        for yy in col.y..col.y + col.height {
            buf.set_string(col.x, yy, "│", theme::border());
        }
    }

    // Focus cue: accent the focused pane's left + right + top edges.
    // Edge panes borrow the outer frame column as one side. Recolour
    // existing cells only — junctions (├ ┬ ┤ ╭ ╰) keep their glyphs.
    let (lx, rx) = match app.pane {
        Pane::Devices => (area.x, h[1].x),
        Pane::Options => (h[1].x, h[3].x),
        Pane::Right => (h[3].x, area.x + area.width - 1),
    };
    for x in lx..=rx {
        buf[(x, y)].set_style(theme::accent());
    }
    for yy in v[1].y..v[1].y + v[1].height {
        buf[(lx, yy)].set_style(theme::accent());
        buf[(rx, yy)].set_style(theme::accent());
    }

    devices_pane(f, h[0], app);
    center_pane(f, h[2], app);
    right_pane(f, h[4], app);
    keybar(f, v[2]);
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
        Constraint::Length(1), // favorites header
        Constraint::Length(2), // favorites
        Constraint::Length(1), // sep
        Constraint::Length(1), // results header
        Constraint::Min(0),    // results list
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

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw(" "),
            sp("★ FAVORITES", theme::faint()),
        ])),
        v[2],
    );
    let favs = Layout::vertical([Constraint::Length(1); 2]).split(v[3]);
    f.render_widget(Paragraph::new(fav("AT28C256", "Atmel")), favs[0]);
    f.render_widget(Paragraph::new(fav("PIC16F628A", "Microchip")), favs[1]);
    f.render_widget(Paragraph::new(sep_line(area.width)), v[4]);

    // Results header — count plus a scroll hint driven by ListState.
    let off = app.list.offset();
    let more_below = off + v[6].height as usize + 1 < app.matches.len();
    let hint = match (off > 0, more_below) {
        (true, true) => "↑↓",
        (true, false) => "↑",
        (false, true) => "↓",
        (false, false) => "",
    };
    let mut rh = vec![sp(format!(" {} results", app.matches.len()), theme::dim())];
    if !hint.is_empty() {
        rh.push(sp(format!("{:>10}", hint), theme::faint()));
    }
    f.render_widget(Paragraph::new(Line::from(rh)), v[5]);

    let list_area = Rect {
        x: v[6].x + 2,
        width: v[6].width.saturating_sub(2),
        ..v[6]
    };
    let items: Vec<ListItem> = app
        .matches
        .iter()
        .map(|&i| {
            let d = &app.devices[i];
            ListItem::new(Line::from(vec![
                sp(format!("{:<11}", d.name), theme::text()),
                sp(d.manufacturer.clone(), theme::dim()),
            ]))
        })
        .collect();
    let list = List::new(items)
        .highlight_style(theme::sel())
        .highlight_symbol("▶")
        .highlight_spacing(HighlightSpacing::Always);
    f.render_stateful_widget(list, list_area, &mut app.list);

    f.render_widget(Paragraph::new(sep_line(area.width)), v[7]);
    f.render_widget(Paragraph::new(selected_lines(app)), v[8]);
    f.render_widget(Paragraph::new(sep_line(area.width)), v[9]);
    f.render_widget(
        Paragraph::new(Line::from(sp(" Diagnostics ▸", theme::faint()))),
        v[10],
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

fn fav(name: &'static str, mfr: &'static str) -> Line<'static> {
    Line::from(vec![
        sp(" ★ ", theme::accent()),
        sp(name, theme::text()),
        Span::raw(" ".repeat(10usize.saturating_sub(name.len()))),
        sp(mfr, theme::dim()),
    ])
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
    f.render_widget(
        Tabs::new(OPS)
            .select(app.op)
            .style(theme::dim())
            .highlight_style(hi)
            .divider(Span::raw("   ")),
        v[0],
    );
    let w = v[1].width;
    let opt = |name: &'static str, val: &'static str| {
        Line::from(vec![
            Span::raw(format!(" {name:<13}")),
            sp(val, theme::text()),
            sp("  ▾", theme::accent()),
        ])
    };
    let mut lines = vec![
        Line::default(),
        opt("Interface", "ZIF socket"),
        opt("Page", "Code"),
        opt("Format", "Auto"),
        opt("Size diff", "Error"),
        Line::default(),
    ];
    // Two checkbox columns; the option cursor is row+col per OPT_COLS.
    for row in 0..OPT_COLS[0].len().max(OPT_COLS[1].len()) {
        let mut spans = Vec::new();
        for (col, labels) in OPT_COLS.iter().enumerate() {
            let base = if col == 0 { 0 } else { OPT_COLS[0].len() };
            if let Some(&label) = labels.get(row) {
                let i = base + row;
                let cur = focused && app.opt_col == col && app.opt_row == row;
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
    lines.extend([
        Line::default(),
        Line::from(vec![
            sp(" ▸ Advanced", theme::accent()),
            sp("  (VPP / VCC / VDD overrides)", theme::faint()),
        ]),
        sep_line(w),
        Line::from(vec![
            sp(" Buffer  ", theme::dim()),
            sp("W25Q64JV_dump.bin", theme::text()),
            sp("  · 8,388,608 B", theme::dim()),
        ]),
        Line::from(vec![
            Span::raw("          "),
            sp("CRC-32 A1B2C3D4", theme::cyan()),
            sp("  · format bin", theme::dim()),
        ]),
        sep_line(w),
        Line::from(vec![
            sp(" [f]", theme::accent()),
            sp(" write from file", theme::text()),
            sp("    [b]", theme::accent()),
            sp(" write buffer", theme::text()),
        ]),
        Line::default(),
    ]);
    f.render_widget(Paragraph::new(lines), v[1]);
    let p = Layout::horizontal([
        Constraint::Length(13),
        Constraint::Min(0),
        Constraint::Length(5),
    ])
    .split(v[2]);
    f.render_widget(Paragraph::new(sp(" ◐ Writing", theme::accent())), p[0]);
    f.render_widget(
        Gauge::default()
            .ratio(0.72)
            .gauge_style(Style::default().fg(theme::GREEN).bg(theme::PROG_EMPTY)),
        p[1],
    );
    f.render_widget(Paragraph::new(sp(" 72%", theme::dim())), p[2]);
}

fn right_pane(f: &mut Frame, area: Rect, app: &App) {
    let focused = app.pane == Pane::Right;
    let v = Layout::vertical([
        Constraint::Length(8), // LOG
        Constraint::Length(1), // sep
        Constraint::Min(0),    // ZIF diagram
        Constraint::Length(1), // sep
        Constraint::Length(1), // pin-test strip
    ])
    .split(area);
    let log = vec![
        hdr("LOG", focused),
        log_line("09:12:04", "● TL866II+ connected", theme::green()),
        log_line("09:12:31", "device W25Q64JV", theme::dim()),
        Line::from(vec![
            sp(" 09:13:02 ", theme::faint()),
            sp("Chip ID OK ", theme::text()),
            sp("0xEF4017", theme::cyan()),
        ]),
        log_line("09:13:02", "Writing 8388608 B…", theme::text()),
        log_line("09:13:09", "OVC check ok", theme::amber()),
        log_line("09:13:14", "…", theme::faint()),
        Line::default(),
    ];
    f.render_widget(Paragraph::new(log), v[0]);
    f.render_widget(Paragraph::new(sep_line(v[1].width)), v[1]);

    // TODO(scaffold): once `selected` drives this, occupied/chip rows
    // come from core::zif's pin map for the device + model.
    let zif = ZifDiagram {
        socket: "ZIF-40",
        package: "DIP8",
        justify: Justify::Top,
        lever: Lever::Top,
        pins_per_col: 20,
        occupied: [1, 2, 3, 4, 37, 38, 39, 40].into_iter().collect(),
        bad: [3].into_iter().collect(),
        chip_top: 1,
        chip_bot: 4,
        interior: 10,
    };
    f.render_widget(&zif, v[2]);
    f.render_widget(Paragraph::new(sep_line(v[3].width)), v[3]);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw(" "),
            sp("✗", theme::red()),
            sp(" Pin test: ", theme::text()),
            sp("bad contact on pin 3", theme::red()),
        ])),
        v[4],
    );
}

fn log_line(ts: &'static str, msg: &'static str, style: Style) -> Line<'static> {
    Line::from(vec![
        Span::raw(" "),
        sp(ts, theme::faint()),
        Span::raw(" "),
        Span::styled(msg, style),
    ])
}

fn keybar(f: &mut Frame, area: Rect) {
    let key = |s: &'static str| Span::styled(s.to_string(), theme::accent_bold().bg(theme::KEY_BG));
    let sep = |s: &'static str| Span::styled(s.to_string(), theme::dim().bg(theme::KEY_BG));
    let line = Line::from(vec![
        key(" ⇥"),
        sep(" panes  ·  "),
        key("1-8"),
        sep(" op  ·  "),
        key("Space"),
        sep(" toggle  ·  "),
        key("←→"),
        sep(" change  ·  "),
        key("Enter"),
        sep(" sel  ·  "),
        key("/"),
        sep(" search  ·  "),
        key("q"),
        sep(" quit "),
    ]);
    f.render_widget(
        Paragraph::new(line).style(Style::default().bg(theme::KEY_BG)),
        area,
    );
}
