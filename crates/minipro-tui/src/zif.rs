//! ZIF socket placement diagram.
//!
//! The visual spec lives in docs/TUI-PLAN.md and tui_mockup_preview.py —
//! the Python `pinrow()` is this `render()`; keep them in sync.
//!
//! Justification is pure data: `occupied` + `chip_top`/`chip_bot` decide
//! every cell, and `⋮` collapses only empty spans (a run keeps its first
//! and last row). Glyph vocabulary is font-vetted Box Drawing + proven
//! shapes; marks inside the chip fill paint their cell background or the
//! glyph punches a hole in the body.

use std::collections::BTreeSet;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use crate::theme;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Justify {
    Top,
    Bottom,
}

/// Lever-end corner — top-left on most models, bottom-right on T48.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Lever {
    Top,
    Bottom,
}

pub struct ZifDiagram {
    /// e.g. "ZIF-40".
    pub socket: &'static str,
    /// e.g. "DIP8".
    pub package: &'static str,
    pub justify: Justify,
    pub lever: Lever,
    /// Pins per column — ZIF-40 → 20.
    pub pins_per_col: u8,
    /// ZIF pins holding a chip pin, resolved from the device pin map.
    pub occupied: BTreeSet<u8>,
    /// ZIF pins that failed the pin-contact check.
    pub bad: BTreeSet<u8>,
    /// Left-column pin rows spanning the chip body — pin-1 end = `chip_top`.
    pub chip_top: u8,
    pub chip_bot: u8,
    /// Cells between the socket walls — 10 in the proven mockup.
    pub interior: usize,
}

enum Row {
    Pin(u8),
    Dots,
}

impl ZifDiagram {
    fn right_pin(&self, lp: u8) -> u8 {
        self.pins_per_col * 2 + 1 - lp
    }

    fn empty(&self, lp: u8) -> bool {
        !self.occupied.contains(&lp) && !self.occupied.contains(&self.right_pin(lp))
    }

    /// Rows to draw: every left-column pin, with empty runs of 4+
    /// collapsed to a `⋮` row keeping the run's first and last rows.
    fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        let mut run: Vec<u8> = Vec::new();
        let flush = |rows: &mut Vec<Row>, run: &mut Vec<u8>| {
            if run.len() >= 4 {
                rows.push(Row::Pin(run[0]));
                rows.push(Row::Dots);
                rows.push(Row::Pin(*run.last().unwrap()));
            } else {
                rows.extend(run.iter().map(|&p| Row::Pin(p)));
            }
            run.clear();
        };
        for lp in 1..=self.pins_per_col {
            if self.empty(lp) {
                run.push(lp);
            } else {
                flush(&mut rows, &mut run);
                rows.push(Row::Pin(lp));
            }
        }
        flush(&mut rows, &mut run);
        rows
    }

    fn slot_style(&self, bad: bool, occupied: bool) -> ratatui::style::Style {
        if bad {
            theme::red()
        } else if occupied {
            theme::green()
        } else {
            theme::faint()
        }
    }

    /// Spans for the `mid` region (interior - 2 cells): the chip body on
    /// pin rows, blank elsewhere. `▪` pin tips flank `█` fill; the
    /// `chip_top` row carries the painted `●` dimple + `╰╯` notch.
    fn mid_spans(&self, lp: u8, lbad: bool, rbad: bool) -> Vec<Span<'static>> {
        if !(self.chip_top..=self.chip_bot).contains(&lp) {
            return vec![Span::raw(" ".repeat(self.interior - 2))];
        }
        let lj = if lbad { theme::red() } else { theme::green() };
        let rj = if rbad { theme::red() } else { theme::green() };
        let body_w = self.interior - 4;
        let mut body = Vec::new();
        for i in 0..body_w {
            let (glyph, style) = if lp == self.chip_top && i == 0 {
                ("●", theme::on_chip())
            } else if lp == self.chip_top && i == (body_w - 2) / 2 {
                ("╰", theme::on_chip())
            } else if lp == self.chip_top && i == (body_w - 2) / 2 + 1 {
                ("╯", theme::on_chip())
            } else {
                ("█", theme::chip())
            };
            body.push(Span::styled(glyph.to_string(), style));
        }
        let mut spans = vec![Span::styled("▪", lj)];
        spans.extend(body);
        spans.push(Span::styled("▪", rj));
        spans
    }

    /// One pin row: `[llab][lsep][│ slot mid slot │][rsep][rlab]`.
    fn pin_line(&self, pad: &str, lp: u8) -> Line<'static> {
        let rp = self.right_pin(lp);
        let (lbad, rbad) = (self.bad.contains(&lp), self.bad.contains(&rp));
        let (locc, rocc) = (self.occupied.contains(&lp), self.occupied.contains(&rp));
        let ll = if lbad { theme::red() } else { theme::dim() };
        let rl = if rbad { theme::red() } else { theme::dim() };
        let mut spans = vec![
            Span::raw(pad.to_string()),
            Span::styled(format!("{lp:>2}"), ll),
            Span::styled(if lbad { "►" } else { " " }, theme::red()),
            Span::styled("│", theme::border()),
            Span::styled("▭", self.slot_style(lbad, locc)),
        ];
        spans.extend(self.mid_spans(lp, lbad, rbad));
        spans.extend([
            Span::styled("▭", self.slot_style(rbad, rocc)),
            Span::styled("│", theme::border()),
            Span::styled(if rbad { "◄" } else { " " }, theme::red()),
            Span::styled(rp.to_string(), rl),
        ]);
        Line::from(spans)
    }

    fn dots_line(&self, pad: &str) -> Line<'static> {
        Line::from(vec![
            Span::raw(pad.to_string()),
            Span::styled(" ⋮", theme::faint()),
            Span::raw(" "),
            Span::styled("│", theme::border()),
            Span::styled("⋮", theme::faint()),
            Span::raw(" ".repeat(self.interior - 2)),
            Span::styled("⋮", theme::faint()),
            Span::styled("│", theme::border()),
            Span::raw(" "),
            Span::styled("⋮", theme::faint()),
        ])
    }

    fn edge(&self, pad: &str, open: bool) -> Line<'static> {
        let (l, r) = if open { ("┌", "┐") } else { ("└", "┘") };
        Line::from(vec![
            Span::raw(format!("{pad}   ")),
            Span::styled(
                l.to_string() + &"─".repeat(self.interior) + r,
                theme::border(),
            ),
        ])
    }
}

impl Widget for &ZifDiagram {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block_w = self.interior + 8;
        let pad = " ".repeat((area.width as usize).saturating_sub(block_w) / 2);
        let mut lines = vec![Line::from(vec![
            Span::raw(" "),
            Span::styled(self.socket, theme::accent_bold()),
            Span::styled(
                format!(
                    " · {} · {}-justified",
                    self.package,
                    match self.justify {
                        Justify::Top => "top",
                        Justify::Bottom => "bottom",
                    }
                ),
                theme::dim(),
            ),
        ])];
        if self.lever == Lever::Top {
            lines.push(Line::from(vec![
                Span::raw(format!("{pad}   ")),
                Span::styled("lever", theme::dim()),
            ]));
        }
        lines.push(self.edge(&pad, true));
        for row in self.rows() {
            lines.push(match row {
                Row::Pin(lp) => self.pin_line(&pad, lp),
                Row::Dots => self.dots_line(&pad),
            });
        }
        lines.push(self.edge(&pad, false));
        if self.lever == Lever::Bottom {
            lines.push(Line::from(vec![
                Span::raw(format!("{pad}{}", " ".repeat(self.interior))),
                Span::styled("lever", theme::dim()),
            ]));
        }
        Paragraph::new(lines).render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn diagram() -> ZifDiagram {
        ZifDiagram {
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
        }
    }

    fn rendered_lines(zif: &ZifDiagram, w: u16, h: u16) -> (Vec<String>, Buffer) {
        let backend = TestBackend::new(w, h);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| f.render_widget(zif, f.area())).unwrap();
        let buf = term.backend().buffer().clone();
        let lines = (0..h)
            .map(|y| {
                (0..w)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect();
        (lines, buf)
    }

    #[test]
    fn top_justified_dip8_matches_mockup() {
        let zif = diagram();
        // 30 cols: block is 18 wide → 6 cells of pad on each side.
        let (lines, _buf) = rendered_lines(&zif, 30, 11);
        assert_eq!(lines[0], " ZIF-40 · DIP8 · top-justified");
        assert_eq!(lines[1], "         lever");
        assert_eq!(lines[2], "         ┌──────────┐");
        assert_eq!(lines[3], "       1 │▭▪●█╰╯██▪▭│ 40");
        assert_eq!(lines[4], "       2 │▭▪██████▪▭│ 39");
        assert_eq!(lines[5], "       3►│▭▪██████▪▭│ 38");
        assert_eq!(lines[6], "       4 │▭▪██████▪▭│ 37");
        assert_eq!(lines[7], "       5 │▭        ▭│ 36");
        assert_eq!(lines[8], "       ⋮ │⋮        ⋮│ ⋮");
        assert_eq!(lines[9], "      20 │▭        ▭│ 21");
        assert_eq!(lines[10], "         └──────────┘");
    }

    #[test]
    fn bad_pin_marks_own_label_and_slot() {
        let zif = diagram();
        let (_lines, buf) = rendered_lines(&zif, 30, 11);
        // Pin 3's row: label, ► marker, slot, and pin tip are all red.
        assert_eq!(buf[(7, 5)].fg, theme::RED); // "3"
        assert_eq!(buf[(8, 5)].fg, theme::RED); // "►"
        assert_eq!(buf[(10, 5)].fg, theme::RED); // left slot ▭
        assert_eq!(buf[(11, 5)].fg, theme::RED); // left pin tip ▪
        assert_eq!(buf[(7, 4)].fg, theme::DIM); // "2" (pin 2 stays dim/green)
        assert_eq!(buf[(10, 4)].fg, theme::GREEN); // pin 2 slot ▭
    }

    #[test]
    fn bottom_lever_sits_under_close_corner() {
        let mut zif = diagram();
        zif.lever = Lever::Bottom;
        // No top lever row: ┌ on line 1, └ on line 9, lever below it.
        let (lines, _buf) = rendered_lines(&zif, 30, 11);
        assert_eq!(lines[9], "         └──────────┘");
        assert_eq!(lines[10], "                lever");
    }

    #[test]
    fn bottom_justified_collapses_top_span() {
        let mut zif = diagram();
        zif.justify = Justify::Bottom;
        zif.occupied = BTreeSet::from([17, 18, 19, 20, 21, 22, 23, 24]);
        zif.bad = BTreeSet::from([19]);
        zif.chip_top = 17;
        zif.chip_bot = 20;
        let (lines, _buf) = rendered_lines(&zif, 30, 11);
        assert_eq!(lines[3], "       1 │▭        ▭│ 40");
        assert_eq!(lines[4], "       ⋮ │⋮        ⋮│ ⋮");
        assert_eq!(lines[5], "      16 │▭        ▭│ 25");
        assert_eq!(lines[6], "      17 │▭▪●█╰╯██▪▭│ 24");
        assert_eq!(lines[8], "      19►│▭▪██████▪▭│ 22");
    }
}
