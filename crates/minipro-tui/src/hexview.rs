//! Full-screen hex-buffer view — the TUI counterpart of the GUI's
//! `HexViewer.svelte`.
//!
//! Layout per row, mirroring the GUI: offset column · 1-cell entropy
//! gutter (same Shannon + 4-tier mapping) · 16 hex bytes · ASCII pane.
//! Cursor byte is inverse-video; edited bytes are amber; a half-typed
//! nibble shows as `x·`.

use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::app::{App, HexBuf};
use crate::theme;

fn sp(s: impl Into<String>, style: ratatui::style::Style) -> Span<'static> {
    Span::styled(s.into(), style)
}

/// Shannon entropy of a byte slice, normalized to 0.0–1.0 — same
/// formula the GUI's `rowEntropy` uses (`entropy / log2(len)`).
pub fn row_entropy(row: &[u8]) -> f64 {
    let n = row.len();
    if n <= 1 {
        return 0.0;
    }
    let mut counts = [0u32; 256];
    for &b in row {
        counts[b as usize] += 1;
    }
    let mut e = 0.0f64;
    for &c in &counts {
        if c > 0 {
            let p = c as f64 / n as f64;
            e -= p * p.log2();
        }
    }
    e / (n as f64).log2()
}

/// Same 4-tier mapping as the GUI's `entropyColor` — green/lime/amber/
/// red as entropy rises.
fn entropy_style(e: f64) -> ratatui::style::Style {
    if e < 0.25 {
        theme::green()
    } else if e < 0.50 {
        theme::lime()
    } else if e < 0.75 {
        theme::amber()
    } else {
        theme::red()
    }
}

/// Draw the view into `area` (the whole pane region under the frame).
/// Header shows size/CRC/edit count; footer shows cursor position and
/// the byte value under it.
pub fn draw(f: &mut Frame, area: Rect, app: &mut App) {
    let v = Layout::vertical([
        Constraint::Length(1), // header
        Constraint::Length(1), // column header
        Constraint::Min(0),    // rows
        Constraint::Length(1), // sep
        Constraint::Length(1), // status
    ])
    .split(area);

    let Some(hex) = &mut app.hex else {
        f.render_widget(
            Paragraph::new(sp(
                " no buffer — o loads a file, r reads the chip",
                theme::faint(),
            )),
            area,
        );
        return;
    };

    // Header — name/size/CRC, GUI "buffer bar" equivalent.
    let edits = hex.edited.len();
    let hdr = Line::from(vec![
        Span::raw(" "),
        sp("BUFFER", theme::accent_bold()),
        sp(format!("  {} bytes", hex.data.len()), theme::text()),
        sp(format!("  ·  CRC-32 {:08X}", hex.crc32), theme::cyan()),
        if edits > 0 {
            sp(format!("  ·  {edits} unsaved edit(s)",), theme::amber())
        } else {
            sp("", theme::dim())
        },
    ]);
    f.render_widget(Paragraph::new(hdr), v[0]);

    // Column header — the 16 byte offsets, grouped in 8s like the hex
    // area below (extra space in the middle keeps the two halves
    // legible). Pad must match the row prefix exactly: " " + 8-char
    // offset + "  " + "▌" = 12 cells before byte 0.
    let mut cols = String::from("            "); // offset + gutter pad
    for i in 0..16 {
        if i == 8 {
            cols.push(' ');
        }
        cols.push_str(&format!("{i:02X} "));
    }
    cols.push_str(" ASCII");
    f.render_widget(Paragraph::new(sp(cols, theme::faint())), v[1]);

    // Rows — the renderer reports the real viewport height back into
    // view_rows for PgUp/PgDn and ensure_visible (same pattern as
    // log_scroll_max).
    let rows_area = v[2];
    hex.view_rows = rows_area.height.max(1);
    hex.ensure_visible();
    let hex = &*hex; // reborrow immutable for the row loop
    let n_rows = hex.data.len().div_ceil(16);
    let mut lines: Vec<Line> = Vec::with_capacity(rows_area.height as usize);
    for r in (hex.scroll as usize)..n_rows.min(hex.scroll as usize + rows_area.height as usize) {
        lines.push(row_line(hex, r));
    }
    if hex.data.is_empty() {
        lines.push(Line::from(sp(" (empty)", theme::faint())));
    }
    f.render_widget(Paragraph::new(lines), rows_area);

    // Footer — cursor position + byte value; while an op runs it shows
    // the op instead, since the log pane is hidden in this view.
    let pos = hex.cursor.min(hex.data.len().saturating_sub(1));
    let val = hex.data.get(pos).copied().unwrap_or(0);
    let status = match app.op_running {
        Some(op) => Line::from(vec![
            Span::raw(" "),
            sp(format!("◐ {} running…", op.name()), theme::amber()),
            sp("  (result logs when you Esc back)", theme::faint()),
        ]),
        None => Line::from(vec![
            Span::raw(" "),
            if hex.ascii {
                sp("ASCII ", theme::accent_bold())
            } else {
                sp("HEX   ", theme::faint())
            },
            sp(format!("0x{pos:08X}",), theme::accent()),
            sp(
                format!("  0x{val:02X}  {}/{}", pos + 1, hex.data.len()),
                theme::dim(),
            ),
        ]),
    };
    f.render_widget(
        Paragraph::new(Line::from(sp(
            format!(" {}", "─".repeat(area.width.saturating_sub(3) as usize)),
            theme::faint(),
        ))),
        v[3],
    );
    f.render_widget(Paragraph::new(status), v[4]);
}

/// One 16-byte row: `00000010  ▌ xx xx …  xx …  |ascii`.
fn row_line(hex: &HexBuf, r: usize) -> Line<'static> {
    let base = r * 16;
    let row = &hex.data[base..(base + 16).min(hex.data.len())];
    let mut spans = vec![
        sp(format!(" {base:08X}  ",), theme::dim()),
        sp("▌", entropy_style(row_entropy(row))),
    ];

    for i in 0..16 {
        if i == 8 {
            spans.push(Span::raw(" "));
        }
        let addr = base + i;
        match hex.data.get(addr) {
            Some(&b) => {
                let cur = addr == hex.cursor;
                let half = cur && hex.nibble.is_some();
                let txt = if half {
                    format!("{:X}·", hex.nibble.unwrap())
                } else {
                    format!("{b:02X}")
                };
                let mut st = if hex.edited.contains(&addr) {
                    theme::amber()
                } else {
                    theme::text()
                };
                if cur {
                    st = st.bg(theme::SEL_BG).add_modifier(Modifier::BOLD);
                }
                spans.push(Span::styled(format!("{txt:>2}"), st));
                spans.push(Span::raw(" "));
            }
            None => spans.push(Span::raw("   ")),
        }
    }

    // ASCII pane — cursor's char is inverse too.
    spans.push(Span::raw(" "));
    for i in 0..16 {
        let addr = base + i;
        let ch = hex
            .data
            .get(addr)
            .map(|&b| {
                if (0x20..0x7f).contains(&b) {
                    b as char
                } else {
                    '·'
                }
            })
            .unwrap_or(' ');
        let st = if addr == hex.cursor {
            theme::sel()
        } else {
            theme::dim()
        };
        spans.push(Span::styled(ch.to_string(), st));
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn entropy_uniform_is_zero_distinct_is_one() {
        assert_eq!(row_entropy(&[0xAA; 16]), 0.0);
        // All 16 byte values distinct → entropy saturates at 1.0.
        let all: Vec<u8> = (0..16).collect();
        assert_eq!(row_entropy(&all), 1.0);
        // Degenerate lengths can't divide by log2(len).
        assert_eq!(row_entropy(&[]), 0.0);
        assert_eq!(row_entropy(&[0x42]), 0.0);
    }

    #[test]
    fn draw_does_not_panic_and_shows_header() {
        let mut app = crate::app::App::test_fixture(vec![0xAB; 64]);
        app.hex_view = true;
        let backend = TestBackend::new(100, 30);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| draw(f, f.area(), &mut app)).unwrap();
        let buf = term.backend().buffer().clone();
        let text: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(text.contains("BUFFER"), "header missing: {text:?}");
        assert!(text.contains("00000000"), "offset column missing");
    }

    #[test]
    fn column_header_aligns_with_data_bytes() {
        // Regression: the header pad was one cell short of the row
        // prefix (" " + 8-char offset + "  " + "▌"), so the 00 column
        // sat left of byte 0. Assert the header's "00" starts exactly
        // above the first data byte.
        let mut app = crate::app::App::test_fixture(vec![0xAB; 64]);
        app.hex_view = true;
        let backend = TestBackend::new(100, 30);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| draw(f, f.area(), &mut app)).unwrap();
        let buf = term.backend().buffer().clone();
        // Collect per-cell chars — NOT a String: find() would return
        // byte offsets and '▌' is 3 bytes UTF-8, skewing the compare.
        let row_text = |y: u16| -> Vec<char> {
            (0..buf.area.width)
                .flat_map(|x| buf[(x, y)].symbol().chars())
                .collect()
        };
        // Header row is line 1 (below the BUFFER title); first data row
        // is line 2.
        let hdr = row_text(1);
        let data = row_text(2);
        let hdr_x = hdr.windows(2).position(|w| w == ['0', '0']).expect("no 00");
        let data_x = data
            .windows(2)
            .position(|w| w == ['A', 'B'])
            .expect("no byte");
        let h: String = hdr.iter().collect();
        let d: String = data.iter().collect();
        assert_eq!(hdr_x, data_x, "header/data columns misaligned:\n{h}\n{d}");
        // ASCII label sits above the ASCII pane start too.
        let h_asc = hdr.windows(5).position(|w| w == ['A', 'S', 'C', 'I', 'I']);
        let d_asc = data.iter().position(|&c| c == '·');
        assert_eq!(h_asc, d_asc);
    }
}
