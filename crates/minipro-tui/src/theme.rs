use ratatui::style::{Color, Modifier, Style};

// Truecolor palette mirrored from tui_mockup_preview.py — the canonical
// render. Keep glyph choices to font-vetted sets (Box Drawing + proven
// Geometric Shapes; see docs/TUI-PLAN.md for the fallback lesson).
pub const BORDER: Color = Color::Rgb(76, 82, 102);
pub const TEXT: Color = Color::Rgb(200, 205, 222);
pub const DIM: Color = Color::Rgb(112, 120, 145);
pub const FAINT: Color = Color::Rgb(88, 95, 118);
pub const ACCENT: Color = Color::Rgb(126, 162, 247);
pub const CHIP: Color = Color::Rgb(187, 154, 247);
pub const GREEN: Color = Color::Rgb(90, 205, 145);
pub const RED: Color = Color::Rgb(242, 96, 96);
pub const AMBER: Color = Color::Rgb(232, 184, 90);
pub const LIME: Color = Color::Rgb(132, 204, 22);
pub const CYAN: Color = Color::Rgb(96, 200, 210);
pub const WHITE: Color = Color::Rgb(235, 238, 248);
pub const SEL_BG: Color = Color::Rgb(44, 55, 100);
pub const KEY_BG: Color = Color::Rgb(32, 37, 56);
pub const PROG_EMPTY: Color = Color::Rgb(52, 58, 80);
pub const MARK: Color = Color::Rgb(40, 44, 64);

pub fn text() -> Style {
    Style::default().fg(TEXT)
}
pub fn dim() -> Style {
    Style::default().fg(DIM)
}
pub fn faint() -> Style {
    Style::default().fg(FAINT)
}
pub fn accent() -> Style {
    Style::default().fg(ACCENT)
}
pub fn accent_bold() -> Style {
    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
}
pub fn chip() -> Style {
    Style::default().fg(CHIP)
}
pub fn green() -> Style {
    Style::default().fg(GREEN)
}
pub fn red() -> Style {
    Style::default().fg(RED)
}
pub fn amber() -> Style {
    Style::default().fg(AMBER)
}
pub fn lime() -> Style {
    Style::default().fg(LIME)
}
pub fn cyan() -> Style {
    Style::default().fg(CYAN)
}
pub fn white() -> Style {
    Style::default().fg(WHITE)
}
pub fn border() -> Style {
    Style::default().fg(BORDER)
}
pub fn sel() -> Style {
    Style::default().fg(TEXT).bg(SEL_BG)
}
/// Marks painted inside the chip fill — dark fg on the chip colour, or
/// the cell punches a hole in the body.
pub fn on_chip() -> Style {
    Style::default().fg(MARK).bg(CHIP)
}
