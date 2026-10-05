//! minipro-tui — scaffold.
//!
//! Renders the docs/TUI-PLAN.md mockup with real ratatui widgets over
//! hardcoded state: proves the layout, palette, glyph set, and the
//! custom `zif` widget before core wiring lands.

mod theme;
mod zif;

use std::{collections::BTreeSet, io, time::Duration};

use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, BorderType, Gauge, Paragraph, Tabs},
    Frame, Terminal,
};
use zif::{Justify, Lever, ZifDiagram};

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let mut term = Terminal::new(CrosstermBackend::new(stdout))?;

    let res = run(&mut term);

    disable_raw_mode()?;
    execute!(term.backend_mut(), LeaveAlternateScreen)?;
    res
}

fn run(term: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    let zif = ZifDiagram {
        socket: "ZIF-40",
        package: "DIP8",
        justify: Justify::Top,
        lever: Lever::Top,
        pins_per_col: 20,
        occupied: BTreeSet::from([1, 2, 3, 4, 37, 38, 39, 40]),
        bad: BTreeSet::from([3]),
        chip_top: 1,
        chip_bot: 4,
        interior: 10,
    };
    loop {
        term.draw(|f| draw(f, &zif))?;
        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(k) = event::read()? {
                match k.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => break,
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

fn draw(f: &mut Frame, zif: &ZifDiagram) {
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

    devices_pane(f, h[0]);
    center_pane(f, h[2]);
    right_pane(f, h[4], zif);
    keybar(f, v[2]);
}

fn sp(s: &'static str, style: Style) -> Span<'static> {
    Span::styled(s, style)
}

fn devices_pane(f: &mut Frame, area: Rect) {
    let w = area.width as usize;
    let lines = vec![
        Line::from(vec![Span::raw(" "), sp("DEVICES", theme::accent_bold())]),
        Line::from(vec![
            Span::raw("  "),
            sp("╭──────────────────────╮", theme::border()),
        ]),
        Line::from(vec![
            Span::raw("  "),
            sp("│", theme::border()),
            sp("/", theme::accent()),
            sp("25Q64", theme::text()),
            sp("▌", theme::accent()),
            Span::raw(" ".repeat(15)),
            sp("│", theme::border()),
        ]),
        Line::from(vec![
            Span::raw("  "),
            sp("╰──────────────────────╯", theme::border()),
        ]),
        Line::from(vec![Span::raw(" "), sp("★ FAVORITES", theme::faint())]),
        fav("AT28C256", "Atmel"),
        fav("PIC16F628A", "Microchip"),
        sep_line(w),
        Line::from(vec![
            sp(" 27 results", theme::dim()),
            sp("       scroll ↓", theme::faint()),
        ]),
        dev("W25Q32JV", "Winbond"),
        dev("W25Q64BV", "Winbond"),
        Line::from(Span::styled("  ▶W25Q64JV      Winbond  ", theme::sel())),
        dev("W25Q64JV-M", "Winbond"),
        dev("W25Q64FV", "Winbond"),
        dev("GD25Q64C", "GigaDev"),
        dev("MX25L6433F", "Macronix"),
        sep_line(w),
        Line::from(sp(" Selected", theme::accent())),
        Line::from(sp(" W25Q64JV", theme::text())),
        Line::from(sp(" Winbond · SPI NOR", theme::dim())),
        Line::from(sp(" 8 MB code · SOIC-8", theme::dim())),
        Line::from(vec![
            sp(" VCC 3.3V · Chip ID ", theme::dim()),
            sp("✓", theme::green()),
        ]),
        sep_line(w),
        Line::from(sp(" Diagnostics ▸", theme::faint())),
    ];
    f.render_widget(Paragraph::new(lines), area);
}

fn fav(name: &'static str, mfr: &'static str) -> Line<'static> {
    Line::from(vec![
        sp(" ★ ", theme::accent()),
        sp(name, theme::text()),
        Span::raw(" ".repeat(10usize.saturating_sub(name.len()))),
        sp(mfr, theme::dim()),
    ])
}

fn dev(name: &'static str, mfr: &'static str) -> Line<'static> {
    Line::from(vec![
        sp("   ", theme::text()),
        sp(name, theme::text()),
        Span::raw(" ".repeat(11usize.saturating_sub(name.len()))),
        sp(mfr, theme::dim()),
    ])
}

fn sep_line(w: usize) -> Line<'static> {
    Line::from(Span::styled(
        format!(" {}", "─".repeat(w.saturating_sub(3))),
        theme::faint(),
    ))
}

fn center_pane(f: &mut Frame, area: Rect) {
    let v = Layout::vertical([
        Constraint::Length(1), // op tabs
        Constraint::Min(0),    // options/buffer
        Constraint::Length(2), // progress
    ])
    .split(area);
    f.render_widget(
        Tabs::new([
            "Read", "Write", "Verify", "Erase", "Blank", "ID", "Logic", "Config",
        ])
        .select(1)
        .style(theme::dim())
        .highlight_style(theme::accent_bold())
        .divider(Span::raw("   ")),
        v[0],
    );
    let w = v[1].width as usize;
    let opt = |name: &'static str, val: &'static str| {
        Line::from(vec![
            Span::raw(format!(" {name:<13}")),
            sp(val, theme::text()),
            sp("  ▾", theme::accent()),
        ])
    };
    let chk = |a: &'static str, b: &'static str| {
        Line::from(vec![sp(a, theme::text()), sp(b, theme::dim())])
    };
    let lines = vec![
        Line::default(),
        opt("Interface", "ZIF socket"),
        opt("Page", "Code"),
        opt("Format", "Auto"),
        opt("Size diff", "Error"),
        Line::default(),
        chk(" [x] Chip ID check      ", "[x] Pin contact check"),
        chk(" [ ] Skip erase         ", "[x] Unprotect before"),
        chk(" [ ] Skip verify        ", "[ ] Re-protect after"),
        chk(" [ ] Skip blank pages   ", ""),
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
    ];
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

fn right_pane(f: &mut Frame, area: Rect, zif: &ZifDiagram) {
    let v = Layout::vertical([
        Constraint::Length(8), // LOG
        Constraint::Length(1), // sep
        Constraint::Min(0),    // ZIF diagram
        Constraint::Length(1), // sep
        Constraint::Length(1), // pin-test strip
    ])
    .split(area);
    let log = vec![
        Line::from(vec![Span::raw(" "), sp("LOG", theme::accent_bold())]),
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
    f.render_widget(Paragraph::new(sep_line(v[1].width as usize)), v[1]);
    f.render_widget(zif, v[2]);
    f.render_widget(Paragraph::new(sep_line(v[3].width as usize)), v[3]);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            sp(" ", theme::text()),
            sp("✗", theme::red()),
            sp(" Pin test: ", theme::text()),
            sp("bad contact on pin 3", theme::red()),
        ])),
        v[4],
    );
}

fn log_line(ts: &'static str, msg: &'static str, style: Style) -> Line<'static> {
    Line::from(vec![
        sp(" ", theme::text()),
        sp(ts, theme::faint()),
        Span::raw(" "),
        Span::styled(msg, style),
    ])
}

fn keybar(f: &mut Frame, area: Rect) {
    let line = Line::from(vec![
        Span::styled(" ⇥".to_string(), theme::accent_bold().bg(theme::KEY_BG)),
        Span::styled(
            " panes  ·  ".to_string(),
            Style::default().fg(theme::DIM).bg(theme::KEY_BG),
        ),
        Span::styled("1-8".to_string(), theme::accent_bold().bg(theme::KEY_BG)),
        Span::styled(
            " op  ·  ".to_string(),
            Style::default().fg(theme::DIM).bg(theme::KEY_BG),
        ),
        Span::styled("Space".to_string(), theme::accent_bold().bg(theme::KEY_BG)),
        Span::styled(
            " toggle  ·  ".to_string(),
            Style::default().fg(theme::DIM).bg(theme::KEY_BG),
        ),
        Span::styled("←→".to_string(), theme::accent_bold().bg(theme::KEY_BG)),
        Span::styled(
            " change  ·  ".to_string(),
            Style::default().fg(theme::DIM).bg(theme::KEY_BG),
        ),
        Span::styled("Enter".to_string(), theme::accent_bold().bg(theme::KEY_BG)),
        Span::styled(
            " run  ·  ".to_string(),
            Style::default().fg(theme::DIM).bg(theme::KEY_BG),
        ),
        Span::styled("/".to_string(), theme::accent_bold().bg(theme::KEY_BG)),
        Span::styled(
            " search  ·  ".to_string(),
            Style::default().fg(theme::DIM).bg(theme::KEY_BG),
        ),
        Span::styled("q".to_string(), theme::accent_bold().bg(theme::KEY_BG)),
        Span::styled(
            " quit ".to_string(),
            Style::default().fg(theme::DIM).bg(theme::KEY_BG),
        ),
    ]);
    f.render_widget(
        Paragraph::new(line).style(Style::default().bg(theme::KEY_BG)),
        area,
    );
}
