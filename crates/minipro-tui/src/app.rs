//! Application state and key handling.
//!
//! Focus model (per docs/TUI-PLAN.md): `Tab`/`Shift-Tab` cycles panes;
//! in DEVICES, `/` enters the search box, `↑↓`/`PgUp`/`PgDn`/`Home`/`End`
//! move the result cursor, `Enter` resolves the highlighted device to a
//! full `Device` record and advances to the options pane. `q`/`Esc`
//! quits whenever the search box isn't editing — while editing, keys go
//! to the text field and `Esc`/`Enter` return to the list.
//!
//! The device list is loaded once via `list_devices_for_model` and
//! filtered in-memory — reparsing the XML database per keystroke is far
//! too slow. `find_device` runs only on `Enter` (a full-file scan is fine
//! once per selection, not per cursor move).

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use minipro_core::database::{find_device, list_devices_for_model, DatabasePaths, DeviceListItem};
use minipro_core::device::{Device, ProgrammerModel};
use minipro_core::error::MiniproError;
use ratatui::widgets::ListState;

pub const OPS: [&str; 8] = [
    "Read", "Write", "Verify", "Erase", "Blank", "ID", "Logic", "Config",
];

/// Option checkboxes per column (rendered two-up in the options pane).
pub const OPT_COLS: [&[&str]; 2] = [
    &[
        "Chip ID check",
        "Skip erase",
        "Skip verify",
        "Skip blank pages",
    ],
    &["Pin contact check", "Unprotect before", "Re-protect after"],
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pane {
    Devices,
    Options,
    Right,
}

pub struct App {
    pub pane: Pane,
    /// Selected operation tab index into `OPS`.
    pub op: usize,
    /// Search box is being edited (keys route to it).
    pub search_editing: bool,
    pub search: String,
    /// Byte offset of the text cursor inside `search`.
    pub search_cursor: usize,
    /// Full model-filtered device list (loaded once).
    pub devices: Vec<DeviceListItem>,
    /// Indices into `devices` matching `search`.
    pub matches: Vec<usize>,
    /// Result-list cursor + scroll (drives `render_stateful_widget`).
    pub list: ListState,
    /// Checkbox states, col-major per `OPT_COLS`.
    pub options: [bool; 7],
    /// Option cursor: column and row within it.
    pub opt_col: usize,
    pub opt_row: usize,
    /// Help overlay is open — swallows all keys until dismissed.
    pub show_help: bool,
    /// Right pane visibility override: None = auto (collapsed at narrow
    /// widths per the plan's responsive rule); Some = the user's `]`
    /// toggle, in either direction.
    pub right_hidden: Option<bool>,
    /// Auto-collapse state for the right pane — the last width regime
    /// seen outside the 98–102 dead band. `right_hidden` overrides it.
    right_auto: bool,
    /// Below-minimum-layout flag with hysteresis: enters at <80×24,
    /// exits at ≥82×25, so a drag parked on the boundary can't flap
    /// between message screen and full layout every repaint.
    pub too_small: bool,
    /// Full device record, resolved on Enter; cleared when the cursor
    /// moves to a different device.
    pub selected: Option<Device>,
    paths: DatabasePaths,
    model: ProgrammerModel,
}

fn filtered(devices: &[DeviceListItem], query: &str) -> Vec<usize> {
    let q = query.to_lowercase();
    devices
        .iter()
        .enumerate()
        .filter(|(_, d)| q.is_empty() || d.name.to_lowercase().contains(&q))
        .map(|(i, _)| i)
        .collect()
}

impl App {
    pub fn new() -> Result<Self, MiniproError> {
        let paths = DatabasePaths::resolve(None, None, None)?;
        let model = ProgrammerModel::Tl866iiPlus;
        let devices = list_devices_for_model(&paths, None, model)?;
        Ok(Self::from_devices(devices, paths, model))
    }

    fn from_devices(
        devices: Vec<DeviceListItem>,
        paths: DatabasePaths,
        model: ProgrammerModel,
    ) -> Self {
        let matches = filtered(&devices, "");
        let mut list = ListState::default();
        list.select(if matches.is_empty() { None } else { Some(0) });
        Self {
            pane: Pane::Devices,
            op: 1, // Write — matches the mockup
            search_editing: false,
            search: String::new(),
            search_cursor: 0,
            devices,
            matches,
            list,
            options: [true, false, false, false, true, true, false],
            opt_col: 0,
            opt_row: 0,
            show_help: false,
            right_hidden: None,
            right_auto: true,
            too_small: false,
            selected: None,
            paths,
            model,
        }
    }

    /// Right pane: auto-follows the dead-banded width regime; `]` sets
    /// `right_hidden` to override it in either direction.
    pub fn right_visible(&self) -> bool {
        self.right_hidden.unwrap_or(self.right_auto)
    }

    /// Track the terminal size each draw. Both responsive breakpoints
    /// use a dead band — leaving a regime takes a couple more cells than
    /// entering it — so a resize drag parked on a boundary can't flap
    /// between structurally different layouts every repaint. A real
    /// regime crossing still clears a `]` override: the toggle tweaks
    /// the pane within a regime; the responsive rule owns crossings.
    pub fn note_size(&mut self, w: u16, h: u16) {
        self.too_small = if self.too_small {
            !(w >= 82 && h >= 25)
        } else {
            w < 80 || h < 24
        };
        let auto = if w >= 103 {
            Some(true)
        } else if w <= 97 {
            Some(false)
        } else {
            None
        };
        if let Some(a) = auto {
            if self.right_auto != a {
                self.right_hidden = None;
                self.right_auto = a;
            }
        }
    }

    /// `Tab`/`Shift-Tab` cycling; Right is skipped while collapsed, and
    /// a focused Right falls back to Options when it collapses.
    fn cycle_pane(&mut self, dir: i32) {
        const DEV_OPT: [Pane; 2] = [Pane::Devices, Pane::Options];
        const ALL: [Pane; 3] = [Pane::Devices, Pane::Options, Pane::Right];
        let order: &[Pane] = if self.right_visible() { &ALL } else { &DEV_OPT };
        let i = order.iter().position(|&p| p == self.pane).unwrap_or(0);
        self.pane = order[(i as i32 + dir).rem_euclid(order.len() as i32) as usize];
    }

    fn toggle_right(&mut self) {
        self.right_hidden = Some(!self.right_visible());
        if !self.right_visible() && self.pane == Pane::Right {
            self.pane = Pane::Options;
        }
    }

    fn refilter(&mut self) {
        self.matches = filtered(&self.devices, &self.search);
        let n = self.matches.len();
        let cur = self.list.selected().unwrap_or(0).min(n.saturating_sub(1));
        self.list.select(if n == 0 { None } else { Some(cur) });
        self.clear_selection_if_moved();
    }

    fn cursor(&self) -> usize {
        self.list.selected().unwrap_or(0)
    }

    /// Full record of the highlighted device, fetched on Enter only.
    fn activate(&mut self) {
        if let Some(&idx) = self.matches.get(self.cursor()) {
            let name = self.devices[idx].name.clone();
            if let Ok(dev) = find_device(&self.paths, &name, self.model) {
                self.selected = Some(dev);
            }
            self.pane = Pane::Options;
        }
    }

    fn clear_selection_if_moved(&mut self) {
        let cur_name = self
            .matches
            .get(self.cursor())
            .map(|&i| self.devices[i].name.as_str());
        if let Some(sel) = &self.selected {
            if Some(sel.name.as_str()) != cur_name {
                self.selected = None;
            }
        }
    }

    fn move_cursor(&mut self, to: usize) {
        if self.matches.is_empty() {
            return;
        }
        let to = to.min(self.matches.len() - 1);
        self.list.select(Some(to));
        self.clear_selection_if_moved();
    }

    fn insert(&mut self, c: char) {
        self.search.insert(self.search_cursor, c);
        self.search_cursor += c.len_utf8();
        self.refilter();
    }

    fn backspace(&mut self) {
        if self.search_cursor == 0 {
            return;
        }
        let prev = self.search[..self.search_cursor]
            .char_indices()
            .last()
            .map(|(i, _)| i)
            .unwrap_or(0);
        self.search.remove(prev);
        self.search_cursor = prev;
        self.refilter();
    }

    /// Returns true when the app should quit.
    pub fn handle_key(&mut self, k: KeyEvent) -> bool {
        // Windows emits Press + Release (+ Repeat for held keys); acting
        // on every event types each character twice. Unix is Press-only.
        if !matches!(k.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            return false;
        }
        if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
            return true;
        }
        // Modal: help swallows every key except the close set.
        if self.show_help {
            if matches!(
                k.code,
                KeyCode::Esc
                    | KeyCode::Enter
                    | KeyCode::F(1)
                    | KeyCode::Char('?')
                    | KeyCode::Char('q')
            ) {
                self.show_help = false;
            }
            return false;
        }
        // F1 opens help even while the search box is editing — it can't
        // be typed, so there's no ambiguity with query text.
        if k.code == KeyCode::F(1) {
            self.show_help = true;
            return false;
        }
        if self.search_editing {
            match k.code {
                KeyCode::Char(c) => self.insert(c),
                KeyCode::Backspace => {
                    if self.search.is_empty() {
                        self.search_editing = false;
                    } else {
                        self.backspace();
                    }
                }
                KeyCode::Delete => {
                    if self.search_cursor < self.search.len() {
                        self.search.remove(self.search_cursor);
                        self.refilter();
                    }
                }
                KeyCode::Left => {
                    if self.search_cursor > 0 {
                        self.search_cursor = self.search[..self.search_cursor]
                            .char_indices()
                            .last()
                            .map(|(i, _)| i)
                            .unwrap_or(0);
                    }
                }
                KeyCode::Right => {
                    if self.search_cursor < self.search.len() {
                        self.search_cursor += self.search[self.search_cursor..]
                            .chars()
                            .next()
                            .unwrap()
                            .len_utf8();
                    }
                }
                KeyCode::Home => self.search_cursor = 0,
                KeyCode::End => self.search_cursor = self.search.len(),
                KeyCode::Esc | KeyCode::Enter => self.search_editing = false,
                _ => {}
            }
            return false;
        }
        match k.code {
            KeyCode::Char('q') | KeyCode::Esc => return true,
            KeyCode::Char('?') => self.show_help = true,
            KeyCode::Tab => self.cycle_pane(1),
            KeyCode::BackTab => self.cycle_pane(-1),
            KeyCode::Char(']') => self.toggle_right(),
            KeyCode::Char(c @ '1'..='8') => self.op = (c as usize) - ('1' as usize),
            _ => match self.pane {
                Pane::Devices => match k.code {
                    KeyCode::Up => self.move_cursor(self.cursor().saturating_sub(1)),
                    KeyCode::Down => self.move_cursor(self.cursor() + 1),
                    KeyCode::PageUp => self.move_cursor(self.cursor().saturating_sub(10)),
                    KeyCode::PageDown => self.move_cursor(self.cursor() + 10),
                    KeyCode::Home => self.move_cursor(0),
                    KeyCode::End => self.move_cursor(usize::MAX),
                    KeyCode::Enter => self.activate(),
                    KeyCode::Char('/') => self.search_editing = true,
                    _ => {}
                },
                Pane::Options => match k.code {
                    KeyCode::Up => self.opt_row = self.opt_row.saturating_sub(1),
                    KeyCode::Down => {
                        self.opt_row = (self.opt_row + 1).min(OPT_COLS[self.opt_col].len() - 1)
                    }
                    KeyCode::Left => {
                        self.opt_col = 0;
                        self.opt_row = self.opt_row.min(OPT_COLS[0].len() - 1);
                    }
                    KeyCode::Right => {
                        self.opt_col = 1;
                        self.opt_row = self.opt_row.min(OPT_COLS[1].len() - 1);
                    }
                    KeyCode::Char(' ') | KeyCode::Enter => {
                        let base = if self.opt_col == 0 {
                            0
                        } else {
                            OPT_COLS[0].len()
                        };
                        let i = base + self.opt_row;
                        self.options[i] = !self.options[i];
                    }
                    _ => {}
                },
                Pane::Right => {}
            },
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(name: &str) -> DeviceListItem {
        DeviceListItem {
            name: name.to_string(),
            manufacturer: "X".to_string(),
        }
    }

    fn app() -> App {
        let devices = vec![
            dev("W25Q32JV"),
            dev("W25Q64BV"),
            dev("W25Q64JV"),
            dev("AT28C256"),
        ];
        let paths = DatabasePaths {
            infoic: "n".into(),
            logicic: "n".into(),
            algorithms: None,
        };
        App::from_devices(devices, paths, ProgrammerModel::Tl866iiPlus)
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn search_filters_and_reopens() {
        let mut a = app();
        assert_eq!(a.matches.len(), 4);
        a.search_editing = true;
        a.handle_key(key(KeyCode::Char('2')));
        a.handle_key(key(KeyCode::Char('5')));
        a.handle_key(key(KeyCode::Char('q')));
        a.handle_key(key(KeyCode::Char('6')));
        a.handle_key(key(KeyCode::Char('4')));
        assert_eq!(a.search, "25q64");
        assert_eq!(a.matches.len(), 2);
        // 'q' typed while editing must not quit.
        assert!(!a.handle_key(key(KeyCode::Char('x'))));
    }

    #[test]
    fn cursor_clamps_when_filter_shrinks() {
        let mut a = app();
        a.move_cursor(3);
        a.search_editing = true;
        a.handle_key(key(KeyCode::Char('A')));
        a.handle_key(key(KeyCode::Char('T')));
        assert_eq!(a.cursor(), 0);
    }

    #[test]
    fn slash_enters_and_esc_leaves_search() {
        let mut a = app();
        a.handle_key(key(KeyCode::Char('/')));
        assert!(a.search_editing);
        a.handle_key(key(KeyCode::Esc));
        assert!(!a.search_editing);
        assert!(a.handle_key(key(KeyCode::Esc)));
    }

    #[test]
    fn release_events_are_ignored() {
        // Windows reports Press+Release per keystroke; only Press may act.
        let mut a = app();
        a.search_editing = true;
        let press =
            KeyEvent::new_with_kind(KeyCode::Char('x'), KeyModifiers::NONE, KeyEventKind::Press);
        let release = KeyEvent::new_with_kind(
            KeyCode::Char('x'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        );
        a.handle_key(press);
        a.handle_key(release);
        assert_eq!(a.search, "x");
    }

    #[test]
    fn help_opens_swallows_and_closes() {
        let mut a = app();
        a.handle_key(key(KeyCode::Char('?')));
        assert!(a.show_help);
        // While open, navigation keys are swallowed — cursor must not move.
        a.handle_key(key(KeyCode::Down));
        assert_eq!(a.cursor(), 0);
        // 'q' closes the overlay rather than quitting.
        assert!(!a.handle_key(key(KeyCode::Char('q'))));
        assert!(!a.show_help);
    }

    #[test]
    fn f1_opens_help_while_search_editing() {
        let mut a = app();
        a.search_editing = true;
        a.handle_key(key(KeyCode::F(1)));
        assert!(a.show_help);
        a.handle_key(key(KeyCode::Esc));
        assert!(!a.show_help);
        // Search is still editing afterwards — F1 didn't leave the box.
        assert!(a.search_editing);
    }

    #[test]
    fn right_pane_auto_collapses_under_100() {
        let mut a = app();
        a.note_size(90, 40);
        assert!(!a.right_visible());
        a.note_size(120, 40);
        assert!(a.right_visible());
        // `]` overrides in both directions.
        a.note_size(90, 40);
        a.handle_key(key(KeyCode::Char(']')));
        assert!(a.right_visible());
        a.note_size(120, 40);
        a.handle_key(key(KeyCode::Char(']')));
        assert!(!a.right_visible());
    }

    #[test]
    fn manual_override_resets_on_threshold_crossing() {
        // Reported bug: ] at wide pinned the pane forever — narrowing
        // must return to auto-collapse.
        let mut a = app();
        a.note_size(120, 40);
        a.handle_key(key(KeyCode::Char(']'))); // hide manually
        assert!(!a.right_visible());
        a.handle_key(key(KeyCode::Char(']'))); // show manually
        assert!(a.right_visible());
        a.note_size(90, 40); // narrow → back to auto: hidden
        assert!(!a.right_visible());
        // Same for a narrow-time override.
        a.handle_key(key(KeyCode::Char(']'))); // show manually at 90
        assert!(a.right_visible());
        a.note_size(120, 40); // widen → auto: shown
        assert!(a.right_visible());
        a.note_size(90, 40); // narrow again → hidden
        assert!(!a.right_visible());
    }

    #[test]
    fn tab_cycle_skips_collapsed_right() {
        let mut a = app();
        a.note_size(90, 40); // right hidden → two-pane cycle
        a.handle_key(key(KeyCode::Tab));
        assert_eq!(a.pane, Pane::Options);
        a.handle_key(key(KeyCode::Tab));
        assert_eq!(a.pane, Pane::Devices);
        // At ≥100 all three panes cycle.
        a.note_size(120, 40);
        a.handle_key(key(KeyCode::Tab));
        assert_eq!(a.pane, Pane::Options);
        a.handle_key(key(KeyCode::Tab));
        assert_eq!(a.pane, Pane::Right);
        a.handle_key(key(KeyCode::Tab));
        assert_eq!(a.pane, Pane::Devices);
    }

    #[test]
    fn toggle_off_falls_back_to_options() {
        let mut a = app();
        a.note_size(120, 40);
        a.pane = Pane::Right;
        a.handle_key(key(KeyCode::Char(']')));
        assert_eq!(a.pane, Pane::Options);
    }

    #[test]
    fn option_toggle_uses_col_major_index() {
        let mut a = app();
        a.pane = Pane::Options;
        a.opt_col = 1;
        a.opt_row = 1; // "Unprotect before" = index 5
        a.handle_key(key(KeyCode::Char(' ')));
        assert!(!a.options[5]);
    }
}
