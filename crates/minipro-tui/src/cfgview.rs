//! Full-screen Config view — MCU fuse/lock fields and the SPI status
//! register, the TUI counterpart of the GUI's config panel.
//!
//! Rows render `name  value  mask` with the selected row highlighted and
//! the nibble-under-cursor in inverse video; dirty rows go amber. The
//! SPI layout adds the common SR1 bit decode (read-only bits marked) and
//! the non-volatile-BP warning the CLI/GUI both show.

use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::app::{App, CfgKind};
use crate::theme;

fn sp(s: impl Into<String>, style: ratatui::style::Style) -> Span<'static> {
    Span::styled(s.into(), style)
}

/// Draw the view into `area` (the whole pane region under the frame).
/// Header names the device + view kind; footer shows the cursor field.
pub fn draw(f: &mut Frame, area: Rect, app: &mut App) {
    let Some(cfg) = &app.cfg else {
        return;
    };
    let v = Layout::vertical([
        Constraint::Length(1), // header
        Constraint::Length(1), // gap
        Constraint::Min(0),    // sections
        Constraint::Length(1), // footer
    ])
    .split(area);

    let title = match cfg.kind {
        CfgKind::Fuses if cfg.n_uids > 0 => " CONFIG — fuses, user id & lock bits ",
        CfgKind::Fuses => " CONFIG — fuses & lock bits ",
        CfgKind::SpiSr => " CONFIG — SPI status register ",
    };
    let device = app
        .selected
        .as_ref()
        .map(|d| d.name.as_str())
        .unwrap_or("?");
    f.render_widget(
        Paragraph::new(Line::from(vec![
            sp(title, theme::accent_bold()),
            sp(device, theme::dim()),
        ])),
        v[0],
    );

    let mut lines: Vec<Line> = Vec::new();
    if cfg.kind == CfgKind::Fuses {
        // Upstream fuses-file order: fuses, user ids, locks.
        let lock_lo = cfg.n_fuses + cfg.n_uids;
        section(&mut lines, " FUSES", cfg, 0, cfg.n_fuses);
        if cfg.n_uids > 0 {
            lines.push(Line::from(""));
            section(&mut lines, " USER ID", cfg, cfg.n_fuses, lock_lo);
        }
        if lock_lo < cfg.rows.len() {
            lines.push(Line::from(""));
            section(&mut lines, " LOCKS", cfg, lock_lo, cfg.rows.len());
        }
    } else {
        section(&mut lines, " STATUS REGISTER", cfg, 0, cfg.rows.len());
        sr_decode(&mut lines, cfg);
    }
    f.render_widget(Paragraph::new(lines), v[2]);

    let row = &cfg.rows[cfg.cursor];
    f.render_widget(
        Paragraph::new(Line::from(vec![
            sp(
                format!(
                    " {} = 0x{:0w$X}",
                    row.name,
                    row.value,
                    w = cfg.row_nibbles(cfg.cursor)
                ),
                theme::dim(),
            ),
            if cfg.dirty.contains(&cfg.cursor) {
                sp("  (edited)", theme::amber())
            } else {
                sp("", theme::dim())
            },
        ])),
        v[3],
    );
}

/// One section's rows — cursor row highlighted, dirty rows amber, the
/// nibble-under-cursor rendered inverse.
fn section(
    lines: &mut Vec<Line<'static>>,
    title: &'static str,
    cfg: &crate::app::CfgView,
    from: usize,
    to: usize,
) {
    lines.push(Line::from(sp(title, theme::chip())));
    let name_w = cfg.rows[from..to]
        .iter()
        .map(|r| r.name.len())
        .max()
        .unwrap_or(0)
        .max(4);
    for i in from..to {
        let row = &cfg.rows[i];
        let w = cfg.row_nibbles(i);
        let sel = i == cfg.cursor;
        let dirty = cfg.dirty.contains(&i);
        let base = if dirty { theme::amber() } else { theme::text() };
        let mut spans = vec![
            sp(if sel { " > " } else { "   " }, theme::accent()),
            sp(
                format!("{:<w$}", row.name, w = name_w),
                if sel { theme::accent_bold() } else { base },
            ),
            sp("  0x", theme::dim()),
        ];
        // Value digits — the nibble-under-cursor gets inverse video.
        let digits = format!("{:0w$X}", row.value, w = w);
        for (j, ch) in digits.chars().enumerate() {
            let st = if sel && j == cfg.nibble {
                theme::text().add_modifier(Modifier::REVERSED)
            } else {
                base
            };
            spans.push(sp(ch.to_string(), st));
        }
        spans.push(sp(
            format!("   mask 0x{:0w$X}", row.mask, w = w),
            theme::faint(),
        ));
        if dirty {
            spans.push(sp("  *", theme::amber()));
        }
        lines.push(Line::from(spans));
    }
}

/// The common SR1 bit decode + warnings (SPI status register kind).
fn sr_decode(lines: &mut Vec<Line<'static>>, cfg: &crate::app::CfgView) {
    use minipro_core::operations::{SPI_SR1_BIT_NAMES, SPI_SR1_READ_ONLY};
    let byte = cfg.rows[0].value as u8;
    lines.push(Line::from(""));
    lines.push(Line::from(vec![sp(
        "   bit decode (common SR1 layout):",
        theme::dim(),
    )]));
    for bit in (0..8).rev() {
        let name = SPI_SR1_BIT_NAMES[bit];
        let set = byte & (1 << bit) != 0;
        let ro = SPI_SR1_READ_ONLY & (1 << bit) != 0;
        let style = if ro {
            theme::faint()
        } else if set {
            theme::amber()
        } else {
            theme::text()
        };
        lines.push(Line::from(vec![
            sp(format!("   bit{bit} "), theme::faint()),
            sp(format!("{name:<4}"), style),
            sp(if set { "set" } else { "—" }, style),
            if ro {
                sp("  (chip-controlled)", theme::faint())
            } else {
                sp("", theme::faint())
            },
        ]));
    }
    if !cfg.sr_extra.is_empty() {
        lines.push(Line::from(sp(
            format!(
                "   extra bytes: {}",
                cfg.sr_extra
                    .iter()
                    .map(|b| format!("{b:02X}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
            theme::faint(),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(sp(
        "   ⚠ BP/SRP bits are non-volatile — they persist until cleared",
        theme::amber(),
    )));
    lines.push(Line::from(sp(
        "     (protect-off clears BP bits; verified T76/TL866A/T48)",
        theme::faint(),
    )));
}
