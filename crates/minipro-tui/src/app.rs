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
//!
//! `handle_event` is the reducer for `AppEvent`s coming off the worker
//! thread — connect results, op progress/completion, pin-test data, and
//! forwarded `log` records. `handle_key` owns terminal input. Modal
//! prompts (path entry, destructive-op confirm) swallow keys before
//! panes see them; while an op is running, only help/pane navigation
//! pass (matching the GUI's blocking progress dialog — core ops can't
//! be aborted mid-transfer).

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use minipro_core::database::{find_device, list_devices_for_model, DatabasePaths, DeviceListItem};
use minipro_core::device::{ChipType, Device, ProgrammerInfo, ProgrammerModel};
use minipro_core::error::MiniproError;
use minipro_core::operations::{ChipIdInfo, OpStats};
use minipro_core::prefs::{self, Favorite};
use minipro_core::zif::{device_to_zif, occupied_zif_pins, zif_spec, ZifInsertion};
use ratatui::widgets::ListState;

use crate::events::{AppEvent, OpInput, OpKind, OpOptions, OpOutcome, SizeDiff};
use crate::worker::WorkerCmd;
use crate::zif::{Justify, Lever, ZifDiagram};

pub const OPS: [&str; 8] = [
    "Read", "Write", "Verify", "Erase", "Blank", "ID", "Logic", "Config",
];

/// Mnemonic dispatch key per op tab (lazygit convention: press → run).
/// Every mnemonic is the tab's first letter — the renderer underlines it.
/// Digits `1-8` remain as positional aliases that arm the tab for Enter.
pub const OP_KEYS: [char; 8] = ['r', 'w', 'v', 'e', 'b', 'i', 'l', 'c'];

/// Option checkboxes per column (rendered two-up in the options pane).
/// Index order is col-major: options[0..4] = col 0, options[4..7] = col 1.
pub const OPT_COLS: [&[&str]; 2] = [
    &[
        "Chip ID check",
        "Skip erase",
        "Skip verify",
        "Skip blank pages",
    ],
    &["Pin contact check", "Unprotect before", "Re-protect after"],
];

/// Select rows rendered above the checkboxes: (label, values). Interface
/// is ZIF-only in the MVP — ICSP-only devices auto-activate in the worker.
pub const SEL_DEFS: [(&str, &[&str]); 4] = [
    ("Interface", &["ZIF socket"]),
    ("Page", &["Code", "Data"]),
    ("Format", &["Auto", "bin", "ihex", "srec"]),
    ("Size diff", &["Error", "Warn", "Ignore"]),
];

/// Format select index → `read_file`/`write_file` format string.
const FORMATS: [&str; 4] = ["auto", "bin", "ihex", "srec"];
/// Number of select rows above the checkbox grid.
const N_SELS: usize = SEL_DEFS.len();
/// Log pane ring capacity.
const LOG_CAP: usize = 1000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pane {
    Devices,
    Options,
    Right,
}

/// Programmer connection state — drives the header badge and op gating.
pub enum ConnState {
    Disconnected,
    Connecting,
    Connected { info: ProgrammerInfo, fake: bool },
    Failed(String),
}

/// One log-pane line. Rendered GUI-style (`[LEVEL] message`); wrapping
/// happens at draw time against the actual pane width.
pub struct LogLine {
    pub level: log::Level,
    pub msg: String,
}

/// Word-wrap `msg` to `width` columns (char count). Words longer than
/// the width are hard-broken so nothing is ever clipped.
pub fn wrap_log(msg: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![msg.to_string()];
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut cur_w = 0usize;
    for word in msg.split(' ') {
        let w = word.chars().count();
        if w > width {
            // Hard-break an over-long word — flush the line, then chunk it.
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
                cur_w = 0;
            }
            let mut chunk = String::new();
            for c in word.chars() {
                if cur_w == width {
                    out.push(std::mem::take(&mut chunk));
                    cur_w = 0;
                }
                chunk.push(c);
                cur_w += 1;
            }
            cur = chunk; // tail resumes the line — a following short word may join
        } else if cur_w == 0 {
            cur.push_str(word);
            cur_w = w;
        } else if cur_w + 1 + w <= width {
            cur.push(' ');
            cur.push_str(word);
            cur_w += 1 + w;
        } else {
            out.push(std::mem::take(&mut cur));
            cur.push_str(word);
            cur_w = w;
        }
    }
    if !cur.is_empty() || out.is_empty() {
        out.push(cur);
    }
    out
}

/// What a path prompt submits to — an op's file operand or the
/// hex buffer's save/load actions.
pub enum PathAction {
    Op(OpKind),
    /// Write the hex buffer to the entered path.
    SaveBuffer,
    /// Read a file into the hex buffer.
    LoadBuffer,
}

/// Modal states — path/goto prompts and destructive-op confirms
/// swallow keys.
pub enum Modal {
    /// File path entry for file ops (Write/Verify) or buffer save/load.
    Path {
        action: PathAction,
        title: &'static str,
        input: String,
        cursor: usize,
    },
    /// Hex-view goto-offset prompt (`:` key) — input parsed as hex.
    Goto { input: String, cursor: usize },
    /// Hex-view find prompt (`/`/Ctrl-F) — literal text, or an `x:`
    /// prefix for hex pairs (e.g. `x:de ad be ef`).
    Find { input: String, cursor: usize },
    /// y/N confirmation for destructive ops (write, erase).
    Confirm {
        op: OpKind,
        path: Option<PathBuf>,
        /// Write-from-buffer payload (hex-view `w`) — `path` is `None`.
        buffer: Option<Vec<u8>>,
        lines: Vec<String>,
    },
}

/// The hex-buffer view's state: data plus cursor/scroll/edit marks.
/// `r` reads land here instead of a file; `s` saves, `o` loads, `w`
/// writes it back to the chip, `v` verifies the chip against it.
pub struct HexBuf {
    pub data: Vec<u8>,
    /// Addresses edited since load/save — rendered amber, cleared on save.
    pub edited: std::collections::BTreeSet<usize>,
    /// Byte offset of the cursor.
    pub cursor: usize,
    /// First visible row (scroll position), in 16-byte rows.
    pub scroll: u16,
    /// High nibble of an in-progress overtype at `cursor`, if any.
    pub nibble: Option<u8>,
    /// ASCII-entry mode (`i` toggles): printable chars write their byte
    /// value directly. A mode is required because a-f/0-9 are ambiguous
    /// — hex nibble or literal character.
    pub ascii: bool,
    /// Rows visible in the last draw — PgUp/PgDn need it (renderer sets).
    pub view_rows: u16,
    /// CRC-32 shown in the header — from the read stats or computed on load.
    pub crc32: u32,
    /// Buffer contents at load/save — cloned lazily on the first edit so
    /// the dirty marks can be recomputed when undo restores a byte.
    original: Option<Vec<u8>>,
    /// Per-keystroke undo stack: (offset, pre-edit byte). Capped — the
    /// oldest entries drop off (hexedit-style minimal undo).
    undo: std::collections::VecDeque<(usize, u8)>,
    /// Last `/` find pattern — F3/n/N repeats need it after the modal
    /// closes.
    pub last_find: Vec<u8>,
    /// One-line status shown in the footer (find results) — cleared on
    /// the next keypress.
    pub note: Option<String>,
}

/// Max undo entries retained — 8K × 9 bytes is trivial and far past
/// any realistic interactive edit session.
const HEX_UNDO_CAP: usize = 8192;

impl HexBuf {
    pub fn new(data: Vec<u8>, crc32: u32) -> Self {
        Self {
            data,
            edited: Default::default(),
            cursor: 0,
            scroll: 0,
            nibble: None,
            ascii: false,
            view_rows: 1,
            crc32,
            original: None,
            undo: Default::default(),
            last_find: Vec::new(),
            note: None,
        }
    }

    fn last(&self) -> usize {
        self.data.len().saturating_sub(1)
    }

    /// Record the pre-edit byte for undo and snapshot the pristine
    /// buffer (once) so dirty marks can be recomputed on undo.
    fn begin_edit(&mut self) {
        if self.original.is_none() {
            self.original = Some(self.data.clone());
        }
        self.undo.push_back((self.cursor, self.data[self.cursor]));
        if self.undo.len() > HEX_UNDO_CAP {
            self.undo.pop_front();
        }
    }

    /// Recompute the dirty mark at `pos` against the load-time buffer.
    fn mark(&mut self, pos: usize) {
        if self
            .original
            .as_ref()
            .is_some_and(|o| o[pos] == self.data[pos])
        {
            self.edited.remove(&pos);
        } else {
            self.edited.insert(pos);
        }
    }

    /// Backspace — undo the previous edit keystroke: restores the byte
    /// and moves the cursor there (hexedit's undo-previous-character).
    fn undo_last(&mut self) {
        if let Some((pos, old)) = self.undo.pop_back() {
            self.data[pos] = old;
            self.mark(pos);
            self.cursor = pos;
            self.nibble = None;
            self.ensure_visible();
        }
    }

    /// First match of `pat` at-or-after `from`, wrapping to the start.
    fn find_next(&self, pat: &[u8], from: usize) -> Option<usize> {
        if pat.is_empty() || self.data.is_empty() {
            return None;
        }
        let from = from.min(self.data.len());
        self.data[from..]
            .windows(pat.len())
            .position(|w| w == pat)
            .map(|i| i + from)
            .or_else(|| self.data[..from].windows(pat.len()).position(|w| w == pat))
    }

    /// Match of `pat` starting strictly before `to`, nearest first,
    /// wrapping to the tail. Starts < `to` count even when the match
    /// itself overlaps the cursor.
    fn find_prev(&self, pat: &[u8], to: usize) -> Option<usize> {
        let len = pat.len();
        if len == 0 || self.data.is_empty() {
            return None;
        }
        let to = to.min(self.data.len());
        // Window starts < to need a slice reaching to+len-1.
        let head = (to + len - 1).min(self.data.len());
        self.data[..head]
            .windows(len)
            .rposition(|w| w == pat)
            .or_else(|| {
                self.data[to..]
                    .windows(len)
                    .rposition(|w| w == pat)
                    .map(|i| i + to)
            })
    }

    /// Overtype one hex digit at the cursor: first digit sets the high
    /// nibble (shown half-typed), the second commits the byte, marks it
    /// edited, and auto-advances.
    fn type_digit(&mut self, d: u8) {
        if self.data.is_empty() {
            return;
        }
        self.begin_edit();
        match self.nibble {
            None => {
                self.data[self.cursor] = d << 4;
                self.nibble = Some(d);
                // The byte already changed — a half-typed edit abandoned
                // by moving away is still an edit.
                self.mark(self.cursor);
            }
            Some(hi) => {
                self.data[self.cursor] = (hi << 4) | d;
                self.nibble = None;
                self.mark(self.cursor);
                self.cursor = (self.cursor + 1).min(self.last());
            }
        }
    }

    /// Write one byte directly (ASCII-entry mode) and auto-advance.
    /// Supersedes any half-typed hex nibble at the cursor.
    fn type_byte(&mut self, b: u8) {
        if self.data.is_empty() {
            return;
        }
        self.begin_edit();
        self.data[self.cursor] = b;
        self.nibble = None;
        self.mark(self.cursor);
        self.cursor = (self.cursor + 1).min(self.last());
    }

    fn move_cursor(&mut self, to: usize) {
        self.cursor = to.min(self.last());
        self.nibble = None; // leaving the byte abandons a half-typed edit
    }

    /// Keep the cursor row inside [scroll, scroll+view_rows).
    pub fn ensure_visible(&mut self) {
        let row = (self.cursor / 16) as u16;
        if row < self.scroll {
            self.scroll = row;
        } else if row >= self.scroll + self.view_rows.max(1) {
            self.scroll = row + 1 - self.view_rows.max(1);
        }
    }

    /// Pure-navigation keys — shared between the idle handler and the
    /// op-running gate (browsing the buffer mid-op is safe; edits are
    /// not). Returns true when the key was a navigation key.
    fn nav(&mut self, code: KeyCode) -> bool {
        let page = (self.view_rows.max(1) as usize) * 16;
        match code {
            KeyCode::Left => self.move_cursor(self.cursor.saturating_sub(1)),
            KeyCode::Right => self.move_cursor(self.cursor + 1),
            KeyCode::Up => self.move_cursor(self.cursor.saturating_sub(16)),
            KeyCode::Down => self.move_cursor(self.cursor + 16),
            KeyCode::PageUp => self.move_cursor(self.cursor.saturating_sub(page)),
            KeyCode::PageDown => self.move_cursor(self.cursor + page),
            KeyCode::Home => self.move_cursor(self.cursor & !15),
            KeyCode::End => self.move_cursor((self.cursor | 15).min(self.last())),
            KeyCode::Char('g') => self.move_cursor(0),
            KeyCode::Char('G') => self.move_cursor(self.last()),
            _ => return false,
        }
        self.ensure_visible();
        true
    }
}

/// Pin-test result mapped onto ZIF pin numbers for the diagram.
pub struct PinTestView {
    /// ZIF pins that failed contact.
    pub bad: std::collections::BTreeSet<u8>,
    /// ZIF pins that were tested (good or bad).
    pub tested: std::collections::BTreeSet<u8>,
    /// ZIF pins untestable by design (GND — held low by the die).
    pub untestable: std::collections::BTreeSet<u8>,
    /// Device pins that failed (for the strip text).
    pub bad_device_pins: Vec<u16>,
    /// True when every expected pin was open — probably an empty socket.
    pub all_open: bool,
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
    /// Select-row values (index into `SEL_DEFS[i].1` per row).
    pub selects: [usize; N_SELS],
    /// Which options zone the cursor is in: select rows vs checkbox grid.
    pub opt_zone: OptZone,
    /// Option cursor: column and row within the checkbox grid.
    pub opt_col: usize,
    pub opt_row: usize,
    /// Select-row cursor (0..N_SELS) — used while `opt_zone == Selects`.
    pub sel_row: usize,
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
    /// Debug paint meter: (draws in the last second, painted w, h) —
    /// set by `run` while MINIPRO_TUI_DEBUG_PAINT is in the environment.
    pub debug_paint: Option<(u32, u16, u16)>,
    /// Full device record, resolved on Enter; cleared when the cursor
    /// moves to a different device.
    pub selected: Option<Device>,
    /// Connection state.
    pub conn: ConnState,
    /// Currently-running operation, if any.
    pub op_running: Option<OpKind>,
    /// When the running op started — elapsed display.
    pub op_started: Option<Instant>,
    /// Latest progress `(done, total)`.
    pub progress: Option<(usize, usize)>,
    /// Spinner frame — advances each draw while an op runs.
    pub spin: u8,
    /// Log pane lines (ring buffer, newest at back).
    pub log_lines: VecDeque<LogLine>,
    /// Scroll offset from the newest *wrapped row*; 0 = follow the tail.
    pub log_scroll: usize,
    /// Max legal `log_scroll` — wrapped-row count minus view height.
    /// Recomputed every draw by the renderer; keys clamp against it.
    pub log_scroll_max: usize,
    /// Active modal (path prompt / confirm), if any.
    pub modal: Option<Modal>,
    /// Latest pin-test result for ZIF highlighting.
    pub pin_test: Option<PinTestView>,
    /// In-memory data buffer — `r` reads land here (hex view), `o`
    /// loads files into it, `s` saves it out, `w`/`v` write/verify it.
    pub hex: Option<HexBuf>,
    /// Full-screen hex view is open — replaces the three-pane layout
    /// and routes all keys to `handle_hex_key`.
    pub hex_view: bool,
    /// Last file path handed to a file op — for the buffer line.
    /// Last file op's resolved path — shown full in the log, `file_name`
    /// in the status strip.
    pub last_path: Option<PathBuf>,
    /// Last read/write stats — the buffer line + CRC display.
    pub last_stats: Option<OpStats>,
    /// Last chip-ID result (the ID tab also shows it in the log).
    pub chip_id: Option<ChipIdInfo>,
    /// Device favorites — `f` toggles in DEVICES. Persisted to
    /// `favorites.txt` in the per-user data dir on every toggle.
    pub favorites: Vec<Favorite>,
    /// Where favorites persist — `None` disables write-through (tests
    /// and data-dir-less environments stay session-scoped).
    pub favorites_path: Option<PathBuf>,
    /// Leading rows of `matches` that are pinned favorites — the
    /// results header splits the count at this boundary.
    pub fav_rows: usize,
    paths: DatabasePaths,
    /// Model used for list filtering — follows the connected programmer.
    pub model: ProgrammerModel,
    /// Command channel to the worker thread (set once at startup).
    pub cmd_tx: Option<Sender<WorkerCmd>>,
}

/// Which part of the options pane the cursor is in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OptZone {
    /// The select rows (Interface / Page / Format / Size diff).
    Selects,
    /// The two-column checkbox grid.
    Checks,
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

/// Favorites resolvable in the connected model's device list — pinned
/// to the top of `matches` in stored order, regardless of the query
/// (GUI parity: "always visible regardless of the search query").
fn pinned(devices: &[DeviceListItem], favorites: &[Favorite]) -> Vec<usize> {
    favorites
        .iter()
        .filter_map(|f| devices.iter().position(|d| d.name == f.name))
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
        let mut list = ListState::default();
        list.select(Some(0));
        let mut app = Self {
            pane: Pane::Devices,
            op: 1, // Write — matches the mockup
            search_editing: false,
            search: String::new(),
            search_cursor: 0,
            devices,
            matches: Vec::new(),
            list,
            options: [true, false, false, false, true, true, false],
            selects: [0; N_SELS],
            opt_zone: OptZone::Checks,
            opt_col: 0,
            opt_row: 0,
            sel_row: 0,
            show_help: false,
            right_hidden: None,
            right_auto: true,
            too_small: false,
            debug_paint: None,
            selected: None,
            conn: ConnState::Disconnected,
            op_running: None,
            op_started: None,
            progress: None,
            spin: 0,
            log_lines: VecDeque::new(),
            log_scroll: 0,
            log_scroll_max: 0,
            modal: None,
            pin_test: None,
            hex: None,
            hex_view: false,
            last_path: None,
            last_stats: None,
            chip_id: None,
            favorites: prefs::load_favorites(),
            favorites_path: prefs::favorites_path(),
            fav_rows: 0,
            paths,
            model,
            cmd_tx: None,
        };
        app.refilter(); // fills matches + fav_rows + clamps the cursor
        app
    }

    /// Database paths — the worker needs a clone at spawn time.
    pub fn paths(&self) -> &DatabasePaths {
        &self.paths
    }

    /// Minimal app with a loaded hex buffer — render tests outside
    /// `app`'s own test module (hexview.rs) build state through this.
    #[cfg(test)]
    pub(crate) fn test_fixture(buf: Vec<u8>) -> Self {
        let mut a = Self::from_devices(
            Vec::new(),
            DatabasePaths {
                infoic: "n".into(),
                logicic: "n".into(),
                algorithms: None,
            },
            ProgrammerModel::Tl866iiPlus,
        );
        a.hex = Some(HexBuf::new(buf, 0));
        a
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
        // Favorites pin to the top in stored order regardless of the
        // query; the rest is the name-filtered remainder minus them.
        let mut m = pinned(&self.devices, &self.favorites);
        self.fav_rows = m.len();
        let favs = &self.favorites;
        m.extend(
            filtered(&self.devices, &self.search)
                .into_iter()
                .filter(|&i| !favs.iter().any(|f| f.name == self.devices[i].name)),
        );
        self.matches = m;
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
            match find_device(&self.paths, &name, self.model) {
                Ok(dev) => {
                    if dev.name.contains('@') {
                        self.push_log(
                            log::Level::Warn,
                            format!(
                                "{} is a package-variant entry — the chip ID may differ from the base chip",
                                dev.name
                            ),
                        );
                    }
                    self.selected = Some(dev);
                    self.pin_test = None;
                    self.chip_id = None;
                }
                Err(e) => self.push_log(log::Level::Error, format!("{e}")),
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

    // ── Logging ──────────────────────────────────────────────────────

    /// Append a log line and cap the ring at LOG_CAP.
    pub fn push_log(&mut self, level: log::Level, msg: impl Into<String>) {
        self.log_lines.push_back(LogLine {
            level,
            msg: msg.into(),
        });
        while self.log_lines.len() > LOG_CAP {
            self.log_lines.pop_front();
        }
        if self.log_scroll > 0 {
            // Keep the user's scroll position stable while lines arrive.
            self.log_scroll = self.log_scroll.min(self.log_scroll_max);
        }
    }

    // ── Event reducer ────────────────────────────────────────────────

    /// Handle one `AppEvent` from the worker/logger.
    pub fn handle_event(&mut self, ev: AppEvent) {
        match ev {
            AppEvent::Connected { info, fake } => {
                self.model = info.model;
                self.push_log(
                    log::Level::Info,
                    format!(
                        "● {} connected · FW {} · S/N {}{}",
                        info.model,
                        info.firmware_str,
                        info.serial_number,
                        if fake { " (fake)" } else { "" }
                    ),
                );
                self.conn = ConnState::Connected { info: *info, fake };
                self.reload_devices();
            }
            AppEvent::ConnectFailed(e) => {
                self.push_log(
                    log::Level::Warn,
                    format!("connect failed: {e} — plug in and press F5"),
                );
                self.conn = ConnState::Failed(e);
            }
            AppEvent::Disconnected => {
                self.conn = ConnState::Disconnected;
                self.push_log(
                    log::Level::Warn,
                    "programmer disconnected — press F5 to reconnect",
                );
            }
            AppEvent::Log { level, msg } => self.push_log(level, msg),
            AppEvent::Progress { done, total } => self.progress = Some((done, total)),
            AppEvent::PinTest {
                bad,
                expected,
                untestable,
                all_open,
            } => self.set_pin_test(bad, expected, untestable, all_open),
            AppEvent::OpDone { op, result } => self.on_op_done(op, result),
        }
    }

    /// Reload the device list for the connected model — a T56/T76 shows
    /// a different device set than the TL866II+ default.
    fn reload_devices(&mut self) {
        match list_devices_for_model(&self.paths, None, self.model) {
            Ok(devices) => {
                self.devices = devices;
                self.refilter();
                self.list.select(if self.matches.is_empty() {
                    None
                } else {
                    Some(0)
                });
            }
            Err(e) => self.push_log(log::Level::Error, format!("device list reload: {e}")),
        }
    }

    /// Map a pin-test result (device pin numbers) onto ZIF pins for the
    /// diagram, using the selected device's package and the model's spec.
    fn set_pin_test(
        &mut self,
        bad: Vec<u16>,
        expected: Vec<u16>,
        untestable: Vec<u16>,
        all_open: bool,
    ) {
        let spec = zif_spec(self.model);
        let pin_count = self
            .selected
            .as_ref()
            .map(|d| d.package_details.pin_count as usize)
            .unwrap_or(0);
        let map = |pins: &[u16]| {
            pins.iter()
                .filter_map(|&p| device_to_zif(&spec, p as usize, pin_count))
                .map(|z| z as u8)
                .collect()
        };
        self.pin_test = Some(PinTestView {
            bad: map(&bad),
            tested: map(&expected),
            untestable: map(&untestable),
            bad_device_pins: bad,
            all_open,
        });
    }

    /// Operation finished — log the outcome, update buffer stats.
    fn on_op_done(&mut self, op: OpKind, result: Result<OpOutcome, String>) {
        self.op_running = None;
        self.op_started = None;
        self.progress = None;
        match result {
            Ok(OpOutcome::Read { stats, data }) => {
                self.last_stats = Some(stats);
                self.hex = Some(HexBuf::new(data, stats.crc32));
                self.hex_view = true;
                self.push_log(
                    log::Level::Info,
                    format!(
                        "✓ read {} bytes → buffer · CRC-32 {:08X} — o/s/w/v/Esc in hex view",
                        stats.bytes, stats.crc32
                    ),
                );
            }
            Ok(OpOutcome::Stats { stats }) => {
                self.last_stats = Some(stats);
                let target = self
                    .last_path
                    .clone()
                    .map(|p| format!(" → {}", p.display()))
                    .unwrap_or_default();
                self.push_log(
                    log::Level::Info,
                    format!(
                        "✓ wrote {} bytes{} · CRC-32 {:08X}",
                        stats.bytes, target, stats.crc32
                    ),
                );
            }
            Ok(OpOutcome::Verified) => {
                self.push_log(log::Level::Info, "✓ verify passed");
            }
            Ok(OpOutcome::Erased { supported }) => {
                if supported {
                    self.push_log(log::Level::Info, "✓ erase complete");
                } else {
                    self.push_log(
                        log::Level::Warn,
                        "device has no electrical erase (UV EPROM?) — nothing to do",
                    );
                }
            }
            Ok(OpOutcome::Blank) => {
                self.push_log(log::Level::Info, "✓ chip is blank");
            }
            Ok(OpOutcome::NotBlank { address }) => {
                self.push_log(
                    log::Level::Warn,
                    format!("✗ chip is NOT blank — first non-blank byte at 0x{address:04x}"),
                );
            }
            Ok(OpOutcome::ChipId(info)) => {
                let mask = info.mask();
                let width = (info.bytes * 2) as usize;
                if info.is_match {
                    self.push_log(
                        log::Level::Info,
                        format!(
                            "✓ chip ID 0x{:0width$X} (expected 0x{:0width$X}) — match",
                            info.id & mask,
                            info.expected & mask,
                            width = width
                        ),
                    );
                } else if info.is_variant {
                    self.push_log(
                        log::Level::Warn,
                        format!(
                            "chip ID 0x{:0width$X} vs expected 0x{:0width$X} — package-variant entry may report a different ID",
                            info.id & mask,
                            info.expected & mask,
                            width = width
                        ),
                    );
                } else {
                    self.push_log(
                        log::Level::Error,
                        format!(
                            "✗ chip ID 0x{:0width$X} vs expected 0x{:0width$X} — MISMATCH",
                            info.id & mask,
                            info.expected & mask,
                            width = width
                        ),
                    );
                }
                self.chip_id = Some(*info);
            }
            Ok(OpOutcome::Aborted { gate }) => {
                self.push_log(
                    log::Level::Warn,
                    format!("✗ {} aborted ({gate})", op.name()),
                );
            }
            Err(e) => {
                self.push_log(log::Level::Error, format!("✗ {} failed: {e}", op.name()));
            }
        }
    }

    // ── Operation dispatch ───────────────────────────────────────────

    /// Snapshot the options pane into the worker's `OpOptions`.
    fn op_options(&self) -> OpOptions {
        OpOptions {
            check_device_id: self.options[0],
            skip_erase: self.options[1],
            skip_verify: self.options[2],
            skip_blank: self.options[3],
            pin_check: self.options[4],
            unprotect_before: self.options[5],
            protect_after: self.options[6],
            page: if self.selects[1] == 1 { 0x01 } else { 0x00 },
            format: FORMATS[self.selects[2]],
            size_diff: match self.selects[3] {
                1 => SizeDiff::Warn,
                2 => SizeDiff::Ignore,
                _ => SizeDiff::Error,
            },
        }
    }

    /// Enter = start the selected op — gated, then prompted/confirmed
    /// or dispatched straight to the worker.
    fn start_op(&mut self) {
        if self.op_running.is_some() {
            return;
        }
        let Some(op) = OpKind::from_tab(self.op) else {
            self.push_log(
                log::Level::Warn,
                "logic test / config ops are not in this MVP — use the CLI or GUI",
            );
            return;
        };
        let Some(dev) = &self.selected else {
            self.push_log(
                log::Level::Warn,
                "select a device first — Enter in the devices pane",
            );
            return;
        };
        let device_name = dev.name.clone();
        let is_logic = dev.chip_type == ChipType::Logic as u32;
        match &self.conn {
            ConnState::Connected { fake: true, .. } => {
                self.push_log(
                    log::Level::Warn,
                    "MINIPRO_FAKE_PROGRAMMER is set — operations need real hardware",
                );
                return;
            }
            ConnState::Connected { .. } => {}
            ConnState::Connecting => {
                self.push_log(log::Level::Warn, "still connecting…");
                return;
            }
            _ => {
                self.push_log(
                    log::Level::Warn,
                    "no programmer connected — press F5 to connect",
                );
                return;
            }
        }
        if is_logic {
            self.push_log(
                log::Level::Warn,
                "logic ICs support only the Logic test — not in this MVP",
            );
            return;
        }
        match op {
            // Reads land in the hex-buffer view — no file prompt.
            // `s` inside the view saves to a path.
            OpKind::Read => self.send_run(op, None, None),
            OpKind::Write => {
                self.modal = Some(Modal::Path {
                    action: PathAction::Op(op),
                    title: "Write file",
                    input: String::new(),
                    cursor: 0,
                });
            }
            OpKind::Verify => {
                self.modal = Some(Modal::Path {
                    action: PathAction::Op(op),
                    title: "Verify against file",
                    input: String::new(),
                    cursor: 0,
                });
            }
            OpKind::Erase => {
                self.modal = Some(Modal::Confirm {
                    op,
                    path: None,
                    buffer: None,
                    lines: vec![
                        format!("Erase {device_name}?"),
                        "this erases the whole chip".to_string(),
                    ],
                });
            }
            OpKind::Blank | OpKind::ChipId => self.send_run(op, None, None),
        }
    }

    /// Dispatch the op to the worker. `path` is `Some` for file ops,
    /// `buffer` `Some` for write/verify issued from the hex view.
    fn send_run(&mut self, op: OpKind, path: Option<PathBuf>, buffer: Option<Vec<u8>>) {
        let Some(tx) = self.cmd_tx.clone() else {
            self.push_log(log::Level::Error, "worker not started");
            return;
        };
        let Some(dev) = &self.selected else {
            return;
        };
        let device_name = dev.name.clone();
        self.last_path = path.clone();
        self.op_running = Some(op);
        self.op_started = Some(Instant::now());
        self.progress = Some((0, 1));
        let options = Box::new(self.op_options());
        self.push_log(log::Level::Info, format!("{} {}…", op.name(), device_name));
        let input = match (path, buffer) {
            (Some(p), _) => OpInput::File(p),
            (None, Some(buf)) => OpInput::Buffer(buf),
            (None, None) => OpInput::None,
        };
        let _ = tx.send(WorkerCmd::Run {
            op,
            device_name,
            input,
            options,
        });
        self.modal = None;
    }

    /// `f` in the devices pane — toggle the cursor device as a
    /// favorite, then write-through to disk.
    fn toggle_favorite(&mut self) {
        let Some(&idx) = self.matches.get(self.cursor()) else {
            return;
        };
        let name = self.devices[idx].name.clone();
        if let Some(pos) = self.favorites.iter().position(|f| f.name == name) {
            self.favorites.remove(pos);
        } else {
            self.favorites.push(Favorite {
                name,
                manufacturer: self.devices[idx].manufacturer.clone(),
            });
        }
        if let Some(path) = &self.favorites_path {
            if let Err(e) = prefs::save_favorites_to(path, &self.favorites) {
                self.push_log(log::Level::Warn, format!("favorites not saved: {e}"));
            }
        }
        // Re-pin: the toggled device moves between the pinned block
        // and the filtered results — keep the cursor on it.
        self.refilter();
        if let Some(pos) = self.matches.iter().position(|&i| i == idx) {
            self.list.select(Some(pos));
        }
    }

    // ── Modal handling ───────────────────────────────────────────────

    /// Resolve a modal path input: absolute stays as-is, relative joins
    /// onto the process CWD so the save location is explicit everywhere
    /// it's displayed.
    pub fn resolve_path(input: &str) -> PathBuf {
        let p = PathBuf::from(input);
        if p.is_absolute() {
            p
        } else {
            std::env::current_dir().unwrap_or_default().join(p)
        }
    }

    /// Write the hex buffer to `path` (hex-view `s`) — honors the
    /// Format select like a file read would.
    fn save_buffer(&mut self, path: PathBuf) {
        let dev_name = self.selected.as_ref().map(|d| d.name.clone());
        let fmt = FORMATS[self.selects[2]];
        let result = match &self.hex {
            Some(hex) => {
                minipro_core::operations::write_file(&path, fmt, &hex.data, dev_name.as_deref())
                    .map(|_| hex.data.len())
            }
            None => return,
        };
        match result {
            Ok(n) => {
                if let Some(hex) = &mut self.hex {
                    // Saved contents become the clean baseline — dirty
                    // marks and the undo stack reset together.
                    hex.edited.clear();
                    hex.original = None;
                    hex.undo.clear();
                }
                self.last_path = Some(path.clone());
                self.push_log(
                    log::Level::Info,
                    format!("✓ buffer → {} ({} bytes)", path.display(), n),
                );
            }
            Err(e) => self.push_log(log::Level::Error, format!("✗ save failed: {e}")),
        }
    }

    /// Read a file into the hex buffer (hex-view `o`) — same
    /// `read_file` the write op uses, so ihex/srec decode too.
    fn load_buffer(&mut self, path: PathBuf) {
        let fmt = FORMATS[self.selects[2]];
        let (size, blank) = self
            .selected
            .as_ref()
            .map(|d| (d.code_memory_bytes(), d.blank_value as u8))
            .unwrap_or((0, 0xFF));
        match minipro_core::operations::read_file(&path, fmt, size, blank) {
            Ok(data) => {
                let n = data.len();
                let crc = OpStats::from_buffer(&data).crc32;
                self.hex = Some(HexBuf::new(data, crc));
                self.hex_view = true;
                self.last_path = Some(path.clone());
                self.push_log(
                    log::Level::Info,
                    format!(
                        "✓ {} → buffer ({} bytes · CRC-32 {:08X})",
                        path.display(),
                        n,
                        crc
                    ),
                );
            }
            Err(e) => self.push_log(log::Level::Error, format!("✗ load failed: {e}")),
        }
    }

    /// Hex-view `w`/`v`: write or verify the buffer against the chip.
    /// Same gates as `start_op` — device selected + real connection.
    fn hex_dispatch(&mut self, op: OpKind) {
        if self.op_running.is_some() {
            return;
        }
        let Some(buf) = self.hex.as_ref().map(|h| h.data.clone()) else {
            return;
        };
        let Some(dev) = &self.selected else {
            self.push_log(
                log::Level::Warn,
                "select a device first — Enter in the devices pane",
            );
            return;
        };
        let device_name = dev.name.clone();
        match &self.conn {
            ConnState::Connected { fake: true, .. } => {
                self.push_log(
                    log::Level::Warn,
                    "MINIPRO_FAKE_PROGRAMMER is set — operations need real hardware",
                );
                return;
            }
            ConnState::Connected { .. } => {}
            ConnState::Connecting => {
                self.push_log(log::Level::Warn, "still connecting…");
                return;
            }
            _ => {
                self.push_log(
                    log::Level::Warn,
                    "no programmer connected — press F5 to connect",
                );
                return;
            }
        }
        match op {
            OpKind::Write => {
                self.modal = Some(Modal::Confirm {
                    op,
                    path: None,
                    buffer: Some(buf),
                    lines: vec![
                        format!("Write buffer to {device_name}?"),
                        if self.options[1] {
                            "writes over current contents (skip-erase is set)"
                        } else {
                            "erases the chip first"
                        }
                        .to_string(),
                    ],
                });
            }
            OpKind::Verify => self.send_run(op, None, Some(buf)),
            _ => {}
        }
    }

    fn handle_modal_key(&mut self, k: KeyEvent) {
        match self.modal.take() {
            Some(Modal::Path {
                action,
                title,
                mut input,
                mut cursor,
            }) => match k.code {
                KeyCode::Esc => {} // dropped — cancel
                KeyCode::Enter => {
                    if input.trim().is_empty() {
                        self.modal = Some(Modal::Path {
                            action,
                            title,
                            input,
                            cursor,
                        });
                        return;
                    }
                    let path = Self::resolve_path(input.trim());
                    match action {
                        PathAction::Op(op) if op.is_destructive() => {
                            let name = self
                                .selected
                                .as_ref()
                                .map(|d| d.name.clone())
                                .unwrap_or_default();
                            self.modal = Some(Modal::Confirm {
                                op,
                                path: Some(path.clone()),
                                buffer: None,
                                lines: vec![
                                    format!("Write {} to {name}?", path.display()),
                                    if self.options[1] {
                                        "writes over current contents (skip-erase is set)"
                                    } else {
                                        "erases the chip first"
                                    }
                                    .to_string(),
                                ],
                            });
                        }
                        PathAction::Op(op) => self.send_run(op, Some(path), None),
                        PathAction::SaveBuffer => self.save_buffer(path),
                        PathAction::LoadBuffer => self.load_buffer(path),
                    }
                }
                KeyCode::Char(c) => {
                    input.insert(cursor, c);
                    cursor += c.len_utf8();
                    self.modal = Some(Modal::Path {
                        action,
                        title,
                        input,
                        cursor,
                    });
                }
                KeyCode::Backspace => {
                    if cursor > 0 {
                        let prev = input[..cursor]
                            .char_indices()
                            .last()
                            .map(|(i, _)| i)
                            .unwrap_or(0);
                        input.remove(prev);
                        cursor = prev;
                    }
                    self.modal = Some(Modal::Path {
                        action,
                        title,
                        input,
                        cursor,
                    });
                }
                KeyCode::Delete => {
                    if cursor < input.len() {
                        input.remove(cursor);
                    }
                    self.modal = Some(Modal::Path {
                        action,
                        title,
                        input,
                        cursor,
                    });
                }
                KeyCode::Left => {
                    if cursor > 0 {
                        cursor = input[..cursor]
                            .char_indices()
                            .last()
                            .map(|(i, _)| i)
                            .unwrap_or(0);
                    }
                    self.modal = Some(Modal::Path {
                        action,
                        title,
                        input,
                        cursor,
                    });
                }
                KeyCode::Right => {
                    if cursor < input.len() {
                        cursor += input[cursor..].chars().next().unwrap().len_utf8();
                    }
                    self.modal = Some(Modal::Path {
                        action,
                        title,
                        input,
                        cursor,
                    });
                }
                KeyCode::Home => {
                    cursor = 0;
                    self.modal = Some(Modal::Path {
                        action,
                        title,
                        input,
                        cursor,
                    });
                }
                KeyCode::End => {
                    cursor = input.len();
                    self.modal = Some(Modal::Path {
                        action,
                        title,
                        input,
                        cursor,
                    });
                }
                _ => {
                    self.modal = Some(Modal::Path {
                        action,
                        title,
                        input,
                        cursor,
                    });
                }
            },
            Some(Modal::Goto {
                mut input,
                mut cursor,
            }) => {
                let mut keep = |input, cursor| {
                    self.modal = Some(Modal::Goto { input, cursor });
                };
                match k.code {
                    KeyCode::Esc => {} // dropped — cancel
                    KeyCode::Enter => {
                        let s = input
                            .trim()
                            .trim_start_matches("0x")
                            .trim_start_matches("0X");
                        match usize::from_str_radix(s, 16) {
                            Ok(off) => {
                                if let Some(hex) = &mut self.hex {
                                    hex.move_cursor(off);
                                    hex.ensure_visible();
                                }
                            }
                            // Bad input: clear the field, keep the prompt —
                            // the log is hidden behind the hex view, so a
                            // logged error wouldn't be seen.
                            Err(_) => keep(String::new(), 0),
                        }
                    }
                    // Hex digits + an optional 0x prefix; anything else is
                    // ignored rather than closing the prompt.
                    KeyCode::Char(c) if c.is_ascii_hexdigit() || c == 'x' || c == 'X' => {
                        input.insert(cursor, c);
                        cursor += c.len_utf8();
                        keep(input, cursor);
                    }
                    KeyCode::Backspace => {
                        if cursor > 0 {
                            input.remove(cursor - 1);
                            cursor -= 1;
                        }
                        keep(input, cursor);
                    }
                    KeyCode::Left => {
                        cursor = cursor.saturating_sub(1);
                        keep(input, cursor);
                    }
                    KeyCode::Right => {
                        if cursor < input.len() {
                            cursor += 1;
                        }
                        keep(input, cursor);
                    }
                    KeyCode::Home => keep(input, 0),
                    KeyCode::End => {
                        let end = input.len();
                        keep(input, end);
                    }
                    _ => keep(input, cursor),
                }
            }
            Some(Modal::Find {
                mut input,
                mut cursor,
            }) => {
                let mut keep = |input, cursor| {
                    self.modal = Some(Modal::Find { input, cursor });
                };
                match k.code {
                    KeyCode::Esc => {} // dropped — cancel
                    KeyCode::Enter => match Self::parse_find(&input) {
                        Some(pat) => {
                            if let Some(hex) = &mut self.hex {
                                let found = hex.find_next(&pat, hex.cursor);
                                hex.last_find = pat;
                                hex.note = Some(match found {
                                    Some(i) => {
                                        hex.move_cursor(i);
                                        format!("found @ 0x{i:08X}")
                                    }
                                    None => "not found".to_string(),
                                });
                                hex.ensure_visible();
                            }
                        }
                        // Unparseable pattern: clear the field, keep the
                        // prompt — the log is hidden behind the hex view.
                        None => keep(String::new(), 0),
                    },
                    // Any printable char is pattern text — an `x:`
                    // prefix switches the whole input to hex pairs at
                    // submit time, so the field itself stays free-form.
                    KeyCode::Char(c) if c.is_ascii() && !c.is_control() => {
                        input.insert(cursor, c);
                        cursor += c.len_utf8();
                        keep(input, cursor);
                    }
                    KeyCode::Backspace => {
                        if cursor > 0 {
                            input.remove(cursor - 1);
                            cursor -= 1;
                        }
                        keep(input, cursor);
                    }
                    KeyCode::Left => {
                        cursor = cursor.saturating_sub(1);
                        keep(input, cursor);
                    }
                    KeyCode::Right => {
                        if cursor < input.len() {
                            cursor += 1;
                        }
                        keep(input, cursor);
                    }
                    KeyCode::Home => keep(input, 0),
                    KeyCode::End => {
                        let end = input.len();
                        keep(input, end);
                    }
                    _ => keep(input, cursor),
                }
            }
            Some(Modal::Confirm {
                op,
                path,
                buffer,
                lines,
            }) => match k.code {
                // Only an explicit y confirms — Enter is deliberately
                // excluded: key-repeat on the path prompt's submit could
                // carry a held Enter into this dialog and fire the op.
                KeyCode::Char('y') | KeyCode::Char('Y') => self.send_run(op, path, buffer),
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {} // cancel
                // Stray keys are ignored — a bumped key must not silently
                // abandon a destructive-op confirm.
                _ => {
                    self.modal = Some(Modal::Confirm {
                        op,
                        path,
                        buffer,
                        lines,
                    })
                }
            },
            None => {}
        }
    }

    /// ZIF diagram for the right pane — real placement from core::zif
    /// (socket size, justification, lever end, occupied/tested/bad pins).
    pub fn zif_diagram(&self) -> ZifDiagram {
        let spec = zif_spec(self.model);
        let pin_count = self
            .selected
            .as_ref()
            .map(|d| d.package_details.pin_count as usize)
            .unwrap_or(0);
        let occupied: std::collections::BTreeSet<u8> = occupied_zif_pins(&spec, pin_count)
            .into_iter()
            .map(|z| z as u8)
            .collect();
        let left: Vec<u8> = occupied
            .iter()
            .copied()
            .filter(|&p| p as usize <= spec.pins / 2)
            .collect();
        let (chip_top, chip_bot) = match (left.first(), left.last()) {
            (Some(&t), Some(&b)) => (t, b),
            _ => (0, 0),
        };
        let (bad, tested, untestable, show_test) = match &self.pin_test {
            Some(pt) => (
                pt.bad.clone(),
                pt.tested.clone(),
                pt.untestable.clone(),
                true,
            ),
            None => (
                Default::default(),
                Default::default(),
                Default::default(),
                false,
            ),
        };
        ZifDiagram {
            socket: if spec.pins == 48 { "ZIF-48" } else { "ZIF-40" },
            package: self
                .selected
                .as_ref()
                .map(|d| format!("DIP{}", d.package_details.pin_count))
                .unwrap_or_else(|| "—".into()),
            justify: match spec.insertion {
                ZifInsertion::Top => Justify::Top,
                ZifInsertion::Bottom => Justify::Bottom,
            },
            lever: if spec.lever_top {
                Lever::Top
            } else {
                Lever::Bottom
            },
            pins_per_col: (spec.pins / 2) as u8,
            occupied,
            bad,
            tested,
            untestable,
            show_test,
            chip_top,
            chip_bot,
            interior: 10,
        }
    }

    /// Parse a find-modal pattern: `x:de ad` → hex pairs (spaces
    /// ignored); anything else → literal text bytes. `None` rejects
    /// empty input and malformed hex.
    // `% 2` stays: is_multiple_of() is unstable at our 1.85 MSRV.
    #[allow(clippy::manual_is_multiple_of)]
    fn parse_find(input: &str) -> Option<Vec<u8>> {
        let s = input.trim();
        let Some(h) = s.strip_prefix("x:").or_else(|| s.strip_prefix("X:")) else {
            return if s.is_empty() {
                None
            } else {
                Some(s.as_bytes().to_vec())
            };
        };
        let digits: String = h.chars().filter(|c| !c.is_whitespace()).collect();
        if digits.is_empty() || digits.len() % 2 != 0 {
            return None;
        }
        (0..digits.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&digits[i..i + 2], 16).ok())
            .collect()
    }

    /// Repeat the last `/` find — F3/Shift-F3 (works in ASCII mode too)
    /// and n/N (hex mode only: n/N are data in ASCII mode).
    fn find_repeat(&mut self, backward: bool) {
        let Some(hex) = &mut self.hex else {
            return;
        };
        if hex.last_find.is_empty() {
            return;
        }
        let pat = hex.last_find.clone();
        let found = if backward {
            hex.find_prev(&pat, hex.cursor)
        } else {
            hex.find_next(&pat, hex.cursor.saturating_add(1))
        };
        hex.note = Some(match found {
            Some(i) => {
                hex.move_cursor(i);
                format!("found @ 0x{i:08X}")
            }
            None => "not found".to_string(),
        });
        hex.ensure_visible();
    }

    /// Keys while the hex-buffer view is open — its own input context:
    /// hex digits overtype bytes (so the b/e op mnemonics are data
    /// here), Esc/q/x/Tab return to the panes, and s/o/w/v/: run the
    /// buffer actions.
    fn handle_hex_key(&mut self, k: KeyEvent) -> bool {
        if self.hex.is_none() {
            self.hex_view = false; // defensive: view without a buffer
            return false;
        }
        if let Some(hex) = &mut self.hex {
            hex.note = None; // one-shot status (find results)
        }
        // ASCII-entry mode: every printable char is data — even
        // q/x/Tab — so only Esc, Insert (the real toggle — 'i' can't
        // exit, it would type 0x69), and navigation keys stay live.
        if self.hex.as_ref().is_some_and(|h| h.ascii) {
            match k.code {
                // Tab/Insert toggle back to hex mode — Tab is the
                // hex-editor convention (hexedit, Okteta, bvi).
                KeyCode::Esc | KeyCode::Tab | KeyCode::Insert => {
                    if let Some(hex) = &mut self.hex {
                        hex.ascii = false;
                    }
                }
                // Non-printable keys stay live in ASCII mode: find,
                // find-repeat, undo.
                KeyCode::Char('f') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.modal = Some(Modal::Find {
                        input: String::new(),
                        cursor: 0,
                    });
                }
                KeyCode::F(3) => {
                    let back = k.modifiers.contains(KeyModifiers::SHIFT);
                    self.find_repeat(back);
                }
                KeyCode::Backspace => {
                    if let Some(hex) = &mut self.hex {
                        hex.undo_last();
                    }
                }
                KeyCode::Char(c) if !c.is_control() && c.is_ascii() => {
                    if let Some(hex) = &mut self.hex {
                        hex.type_byte(c as u8);
                        hex.ensure_visible();
                    }
                }
                _ => {
                    if let Some(hex) = &mut self.hex {
                        hex.nav(k.code);
                    }
                }
            }
            return false;
        }
        match k.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('x') | KeyCode::BackTab => {
                self.hex_view = false
            }
            // Tab switches which column receives input — the standard
            // hex-editor binding (hexedit/Okteta); i/Ins are aliases.
            KeyCode::Tab => {
                if let Some(hex) = &mut self.hex {
                    hex.ascii = true;
                    hex.nibble = None;
                }
            }
            KeyCode::Char('?') => self.show_help = true,
            KeyCode::Char(':') => {
                self.modal = Some(Modal::Goto {
                    input: String::new(),
                    cursor: 0,
                });
            }
            KeyCode::Char('s') => {
                let input = self
                    .last_path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map(|f| f.to_string_lossy().into_owned())
                    .or_else(|| {
                        self.selected
                            .as_ref()
                            .map(|d| format!("{}.bin", d.name.replace(['/', '@'], "_")))
                    })
                    .unwrap_or_else(|| "buffer.bin".to_string());
                self.modal = Some(Modal::Path {
                    action: PathAction::SaveBuffer,
                    title: "Save buffer to",
                    cursor: input.len(),
                    input,
                });
            }
            KeyCode::Char('o') => {
                self.modal = Some(Modal::Path {
                    action: PathAction::LoadBuffer,
                    title: "Load file into buffer",
                    input: String::new(),
                    cursor: 0,
                });
            }
            // 'i'/Insert enter ASCII mode as aliases — 'i' can't exit
            // (it would type 0x69), so Tab/Insert are the way back.
            KeyCode::Char('i') | KeyCode::Insert => {
                if let Some(hex) = &mut self.hex {
                    hex.ascii = true;
                    hex.nibble = None;
                }
            }
            KeyCode::Char('w') => self.hex_dispatch(OpKind::Write),
            KeyCode::Char('v') => self.hex_dispatch(OpKind::Verify),
            // Find — '/' opens the prompt; Ctrl-F must come before the
            // hexdigit arm ('f' is a hex digit). Repeats: n/N here,
            // F3/Shift-F3 in both modes.
            KeyCode::Char('/') => {
                self.modal = Some(Modal::Find {
                    input: String::new(),
                    cursor: 0,
                });
            }
            KeyCode::Char('f') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                self.modal = Some(Modal::Find {
                    input: String::new(),
                    cursor: 0,
                });
            }
            KeyCode::F(3) => {
                let back = k.modifiers.contains(KeyModifiers::SHIFT);
                self.find_repeat(back);
            }
            KeyCode::Char('n') => self.find_repeat(false),
            KeyCode::Char('N') => self.find_repeat(true),
            // Backspace undoes the previous edit keystroke (hexedit's
            // undo-previous-character).
            KeyCode::Backspace => {
                if let Some(hex) = &mut self.hex {
                    hex.undo_last();
                }
            }
            KeyCode::Char(c) if c.is_ascii_hexdigit() => {
                if let Some(d) = c.to_digit(16) {
                    if let Some(hex) = &mut self.hex {
                        hex.type_digit(d as u8);
                        hex.ensure_visible();
                    }
                }
            }
            _ => {
                if let Some(hex) = &mut self.hex {
                    hex.nav(k.code);
                }
            }
        }
        false
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
        // Modals (path prompt / confirm) swallow all keys.
        if self.modal.is_some() {
            self.handle_modal_key(k);
            return false;
        }
        // While an op runs, navigation + help stay live but anything
        // that could start/toggle state is blocked — core ops can't be
        // aborted mid-transfer (same as the GUI's blocking dialog). The
        // hex view stays browsable (nav only — no edits/actions).
        if self.op_running.is_some() {
            match k.code {
                KeyCode::Tab => self.cycle_pane(1),
                KeyCode::BackTab => self.cycle_pane(-1),
                KeyCode::Char(']') => self.toggle_right(),
                KeyCode::Char('?') => self.show_help = true,
                KeyCode::Char('q') | KeyCode::Char('x') | KeyCode::Esc => {
                    if self.hex_view {
                        self.hex_view = false;
                    } else {
                        self.push_log(
                            log::Level::Warn,
                            "operation in progress — it can't be aborted; wait for it to finish",
                        );
                    }
                }
                _ => {
                    if self.hex_view {
                        if let Some(hex) = &mut self.hex {
                            hex.nav(k.code);
                        }
                    }
                }
            }
            return false;
        }
        // The hex view owns all keys while open — its own input context.
        if self.hex_view {
            return self.handle_hex_key(k);
        }
        if self.search_editing {
            match k.code {
                // Ctrl-F favorites the cursor result without leaving
                // typing mode — plain 'f' is text here, so a
                // non-printable key carries the action (same rule as
                // the hex view's F3 find-repeat).
                KeyCode::Char('f') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.toggle_favorite()
                }
                // Plain/shifted chars only — Ctrl/Alt combos are
                // commands, not text (previously Ctrl+<letter>
                // inserted the letter into the query).
                KeyCode::Char(c)
                    if !k
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    self.insert(c)
                }
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
                // While the filter input is focused, arrows navigate the
                // results (fzf convention) — otherwise typing a query
                // dead-ends the list until Esc/Enter.
                KeyCode::Up => self.move_cursor(self.cursor().saturating_sub(1)),
                KeyCode::Down => self.move_cursor(self.cursor() + 1),
                KeyCode::PageUp => self.move_cursor(self.cursor().saturating_sub(10)),
                KeyCode::PageDown => self.move_cursor(self.cursor() + 10),
                KeyCode::Esc => self.search_editing = false,
                KeyCode::Enter => {
                    self.search_editing = false;
                    self.activate();
                }
                // Tab always means "leave this field" — exit the input
                // AND cycle the pane; a dead Tab mid-search reads as a hang.
                KeyCode::Tab => {
                    self.search_editing = false;
                    self.cycle_pane(1);
                }
                KeyCode::BackTab => {
                    self.search_editing = false;
                    self.cycle_pane(-1);
                }
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
            // `o` = buffer view: open it when one exists, else prompt for
            // a file to load into it. No programmer needed for viewing.
            KeyCode::Char('o') => {
                if self.hex.is_some() {
                    self.hex_view = true;
                } else {
                    self.modal = Some(Modal::Path {
                        action: PathAction::LoadBuffer,
                        title: "Load file into buffer",
                        input: String::new(),
                        cursor: 0,
                    });
                }
            }
            KeyCode::Char(c @ '1'..='8') => self.op = (c as usize) - ('1' as usize),
            // Mnemonic dispatch: arm the tab AND run it in one key. Path
            // and confirm modals still gate file/destructive ops. Only the
            // eight mnemonic letters are captured — everything else falls
            // through to the pane-scoped arms below.
            KeyCode::Char(c) if OP_KEYS.contains(&c) => {
                self.op = OP_KEYS.iter().position(|&k| k == c).unwrap();
                self.start_op();
            }
            KeyCode::F(5) => {
                self.conn = ConnState::Connecting;
                self.push_log(log::Level::Info, "connecting…");
                if let Some(tx) = &self.cmd_tx {
                    let _ = tx.send(WorkerCmd::Connect);
                }
            }
            _ => match self.pane {
                Pane::Devices => match k.code {
                    KeyCode::Up => self.move_cursor(self.cursor().saturating_sub(1)),
                    KeyCode::Down => self.move_cursor(self.cursor() + 1),
                    KeyCode::PageUp => self.move_cursor(self.cursor().saturating_sub(10)),
                    KeyCode::PageDown => self.move_cursor(self.cursor() + 10),
                    KeyCode::Home => self.move_cursor(0),
                    KeyCode::End => self.move_cursor(usize::MAX),
                    KeyCode::Enter => self.activate(),
                    KeyCode::Char('f') => self.toggle_favorite(),
                    KeyCode::Char('/') => self.search_editing = true,
                    _ => {}
                },
                Pane::Options => match k.code {
                    KeyCode::Enter => self.start_op(),
                    KeyCode::Char(' ') => {
                        if self.opt_zone == OptZone::Checks {
                            let base = if self.opt_col == 0 {
                                0
                            } else {
                                OPT_COLS[0].len()
                            };
                            let i = base + self.opt_row;
                            self.options[i] = !self.options[i];
                        }
                    }
                    KeyCode::Up => match self.opt_zone {
                        OptZone::Selects => {
                            if self.sel_row > 0 {
                                self.sel_row -= 1;
                            }
                        }
                        OptZone::Checks => {
                            if self.opt_row == 0 {
                                self.opt_zone = OptZone::Selects;
                                self.sel_row = N_SELS - 1;
                            } else {
                                self.opt_row -= 1;
                            }
                        }
                    },
                    KeyCode::Down => match self.opt_zone {
                        OptZone::Selects => {
                            if self.sel_row + 1 >= N_SELS {
                                self.opt_zone = OptZone::Checks;
                                self.opt_row = 0;
                            } else {
                                self.sel_row += 1;
                            }
                        }
                        OptZone::Checks => {
                            self.opt_row = (self.opt_row + 1).min(OPT_COLS[self.opt_col].len() - 1)
                        }
                    },
                    KeyCode::Left => match self.opt_zone {
                        OptZone::Selects => {
                            let n = SEL_DEFS[self.sel_row].1.len();
                            if n > 1 {
                                self.selects[self.sel_row] =
                                    (self.selects[self.sel_row] + n - 1) % n;
                            }
                        }
                        OptZone::Checks => {
                            self.opt_col = 0;
                            self.opt_row = self.opt_row.min(OPT_COLS[0].len() - 1);
                        }
                    },
                    KeyCode::Right => match self.opt_zone {
                        OptZone::Selects => {
                            let n = SEL_DEFS[self.sel_row].1.len();
                            if n > 1 {
                                self.selects[self.sel_row] = (self.selects[self.sel_row] + 1) % n;
                            }
                        }
                        OptZone::Checks => {
                            self.opt_col = 1;
                            self.opt_row = self.opt_row.min(OPT_COLS[1].len() - 1);
                        }
                    },
                    _ => {}
                },
                Pane::Right => match k.code {
                    KeyCode::PageUp | KeyCode::Char('k') => {
                        self.log_scroll = (self.log_scroll + 5).min(self.log_scroll_max);
                    }
                    KeyCode::PageDown | KeyCode::Char('j') => {
                        self.log_scroll = self.log_scroll.saturating_sub(5);
                    }
                    KeyCode::Char('x') => self.log_lines.clear(),
                    _ => {}
                },
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
    fn arrows_navigate_results_while_searching() {
        let mut a = app();
        a.search_editing = true;
        a.handle_key(key(KeyCode::Down));
        assert_eq!(a.cursor(), 1);
        a.handle_key(key(KeyCode::Down));
        assert_eq!(a.cursor(), 2);
        a.handle_key(key(KeyCode::Up));
        assert_eq!(a.cursor(), 1);
        a.handle_key(key(KeyCode::PageDown));
        assert_eq!(a.cursor(), 3); // clamped to last
        a.handle_key(key(KeyCode::PageUp));
        assert_eq!(a.cursor(), 0);
        assert!(a.search_editing); // still editing — only Esc/Enter leave
    }

    #[test]
    fn enter_in_search_exits_and_activates() {
        let mut a = app();
        a.search_editing = true;
        a.handle_key(key(KeyCode::Enter));
        assert!(!a.search_editing);
        // activate() ran: pane jumped to Options (find_device fails on
        // the fake paths, but the dispatch still happened).
        assert_eq!(a.pane, Pane::Options);
    }

    #[test]
    fn tab_in_search_exits_and_cycles_pane() {
        let mut a = app();
        a.note_size(120, 40); // all three panes visible → Tab moves Options
        a.search_editing = true;
        a.handle_key(key(KeyCode::Tab));
        assert!(!a.search_editing);
        assert_eq!(a.pane, Pane::Options);
        a.search_editing = true;
        a.handle_key(key(KeyCode::BackTab));
        assert!(!a.search_editing);
        assert_eq!(a.pane, Pane::Devices);
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
        a.opt_zone = OptZone::Checks;
        a.opt_col = 1;
        a.opt_row = 1; // "Unprotect before" = index 5
        a.handle_key(key(KeyCode::Char(' ')));
        assert!(!a.options[5]);
    }

    #[test]
    fn select_rows_cycle_and_bound() {
        let mut a = app();
        a.pane = Pane::Options;
        a.opt_zone = OptZone::Selects;
        // Format row (index 2): Auto → bin → ihex → srec → Auto.
        a.sel_row = 2;
        a.handle_key(key(KeyCode::Right));
        assert_eq!(a.selects[2], 1);
        a.handle_key(key(KeyCode::Right));
        a.handle_key(key(KeyCode::Right));
        a.handle_key(key(KeyCode::Right));
        assert_eq!(a.selects[2], 0);
        // Interface row has one value — cycling is a no-op.
        a.sel_row = 0;
        a.handle_key(key(KeyCode::Right));
        assert_eq!(a.selects[0], 0);
        // Down from the last select lands on the checkbox grid.
        a.sel_row = 3;
        a.handle_key(key(KeyCode::Down));
        assert_eq!(a.opt_zone, OptZone::Checks);
        assert_eq!(a.opt_row, 0);
        // Up from checkbox row 0 returns to the last select.
        a.handle_key(key(KeyCode::Up));
        assert_eq!(a.opt_zone, OptZone::Selects);
        assert_eq!(a.sel_row, 3);
    }

    #[test]
    fn enter_run_blocked_without_device() {
        let mut a = app();
        a.pane = Pane::Options;
        a.conn = ConnState::Connected {
            info: ProgrammerInfo {
                model: ProgrammerModel::Tl866iiPlus,
                status: minipro_core::device::ProgrammerStatus::Normal,
                firmware: 0,
                firmware_str: "04.2.86".into(),
                device_code: "00".into(),
                serial_number: "TEST".into(),
                hardware_version: 0,
            },
            fake: false,
        };
        // No device selected → warns, no op starts.
        a.handle_key(key(KeyCode::Enter));
        assert!(a.op_running.is_none());
        assert!(a.log_lines.back().unwrap().msg.contains("select a device"));
    }

    #[test]
    fn enter_run_blocked_when_disconnected() {
        let mut a = app();
        a.selected = Some(Device::default());
        a.pane = Pane::Options;
        a.handle_key(key(KeyCode::Enter));
        assert!(a
            .log_lines
            .back()
            .unwrap()
            .msg
            .contains("no programmer connected"));
    }

    #[test]
    fn enter_on_logic_tab_warns_not_mvp() {
        let mut a = app();
        a.pane = Pane::Options;
        a.op = 6; // Logic tab
        a.handle_key(key(KeyCode::Enter));
        assert!(a.log_lines.back().unwrap().msg.contains("MVP"));
    }

    #[test]
    fn modal_path_submit_then_confirm_for_write() {
        let mut a = app();
        a.pane = Pane::Options;
        a.op = 1; // Write
        a.conn = ConnState::Connected {
            info: ProgrammerInfo {
                model: ProgrammerModel::Tl866iiPlus,
                status: minipro_core::device::ProgrammerStatus::Normal,
                firmware: 0,
                firmware_str: "04.2.86".into(),
                device_code: "00".into(),
                serial_number: "TEST".into(),
                hardware_version: 0,
            },
            fake: false,
        };
        // Fake a selected device — from_devices has no real Device; the
        // modal flow only needs a name, so inject one via a match arm.
        a.selected = Some(Device::default());
        a.handle_key(key(KeyCode::Enter));
        assert!(matches!(a.modal, Some(Modal::Path { .. })));
        // Type a filename.
        for c in "a.bin".chars() {
            a.handle_key(key(KeyCode::Char(c)));
        }
        a.handle_key(key(KeyCode::Enter));
        // Write is destructive → confirm modal, not a dispatch.
        assert!(matches!(a.modal, Some(Modal::Confirm { .. })));
        // 'n' cancels — no op starts.
        a.handle_key(key(KeyCode::Char('n')));
        assert!(a.modal.is_none());
        assert!(a.op_running.is_none());
    }

    #[test]
    fn erase_confirm_y_dispatches() {
        let mut a = app();
        a.pane = Pane::Options;
        a.op = 3; // Erase
        a.conn = ConnState::Connected {
            info: ProgrammerInfo {
                model: ProgrammerModel::Tl866iiPlus,
                status: minipro_core::device::ProgrammerStatus::Normal,
                firmware: 0,
                firmware_str: "04.2.86".into(),
                device_code: "00".into(),
                serial_number: "TEST".into(),
                hardware_version: 0,
            },
            fake: false,
        };
        let (tx, rx) = std::sync::mpsc::channel();
        a.cmd_tx = Some(tx);
        a.selected = Some(Device::default());
        a.handle_key(key(KeyCode::Enter));
        assert!(matches!(a.modal, Some(Modal::Confirm { .. })));
        a.handle_key(key(KeyCode::Char('y')));
        assert!(a.op_running == Some(OpKind::Erase));
        assert!(matches!(rx.try_recv(), Ok(WorkerCmd::Run { .. })));
    }

    #[test]
    fn keys_swallowed_while_op_running() {
        let mut a = app();
        a.op_running = Some(OpKind::Read);
        a.handle_key(key(KeyCode::Enter));
        a.handle_key(key(KeyCode::Char('2')));
        assert_eq!(a.op, 1); // tab didn't change
        a.handle_key(key(KeyCode::Char('q')));
        assert!(a.op_running.is_some()); // quit blocked
        assert!(a.log_lines.back().unwrap().msg.contains("can't be aborted"));
    }

    /// Connected+selected fixture shared by the mnemonic-dispatch tests.
    fn op_ready_app() -> App {
        let mut a = app();
        a.conn = ConnState::Connected {
            info: ProgrammerInfo {
                model: ProgrammerModel::Tl866iiPlus,
                status: minipro_core::device::ProgrammerStatus::Normal,
                firmware: 0,
                firmware_str: "04.2.86".into(),
                device_code: "00".into(),
                serial_number: "TEST".into(),
                hardware_version: 0,
            },
            fake: false,
        };
        a.selected = Some(Device::default());
        a
    }

    #[test]
    fn mnemonic_dispatches_immediate_op() {
        let mut a = op_ready_app();
        let (tx, rx) = std::sync::mpsc::channel();
        a.cmd_tx = Some(tx);
        a.handle_key(key(KeyCode::Char('i')));
        assert_eq!(a.op, 5); // armed on the ID tab too
        assert_eq!(a.op_running, Some(OpKind::ChipId));
        assert!(matches!(rx.try_recv(), Ok(WorkerCmd::Run { .. })));
    }

    #[test]
    fn mnemonic_opens_path_and_confirm_modals() {
        let mut a = op_ready_app();
        a.handle_key(key(KeyCode::Char('w')));
        assert!(matches!(
            a.modal,
            Some(Modal::Path {
                action: PathAction::Op(OpKind::Write),
                ..
            })
        ));
        a.handle_key(key(KeyCode::Esc));
        a.handle_key(key(KeyCode::Char('e')));
        assert!(matches!(
            a.modal,
            Some(Modal::Confirm {
                op: OpKind::Erase,
                ..
            })
        ));
    }

    #[test]
    fn mnemonic_warns_for_gated_tabs() {
        let mut a = op_ready_app();
        a.handle_key(key(KeyCode::Char('l')));
        assert_eq!(a.op, 6);
        assert!(a.log_lines.back().unwrap().msg.contains("MVP"));
    }

    #[test]
    fn mnemonic_blocked_in_search_and_modal() {
        let mut a = op_ready_app();
        // Search editing: 'r' inserts into the filter, not a dispatch.
        a.search_editing = true;
        a.handle_key(key(KeyCode::Char('r')));
        assert_eq!(a.search, "r");
        assert!(a.modal.is_none());
        assert!(a.op_running.is_none());
        // Path modal: letters go to the filename input. (Typing in the
        // filter cleared `selected` — re-inject the fixture device.)
        a.search_editing = false;
        a.search.clear();
        a.selected = Some(Device::default());
        a.handle_key(key(KeyCode::Char('w')));
        assert!(matches!(a.modal, Some(Modal::Path { .. })));
        a.handle_key(key(KeyCode::Char('e')));
        if let Some(Modal::Path { input, .. }) = &a.modal {
            assert_eq!(input, "e");
        } else {
            panic!("path modal vanished");
        }
    }

    #[test]
    fn digits_still_arm_for_enter() {
        let mut a = op_ready_app();
        a.handle_key(key(KeyCode::Char('4')));
        assert_eq!(a.op, 3); // armed, not dispatched
        assert!(a.modal.is_none());
        assert!(a.op_running.is_none());
        a.pane = Pane::Options;
        a.handle_key(key(KeyCode::Enter));
        assert!(matches!(
            a.modal,
            Some(Modal::Confirm {
                op: OpKind::Erase,
                ..
            })
        ));
    }

    #[test]
    fn confirm_ignores_stray_keys() {
        let mut a = op_ready_app();
        a.handle_key(key(KeyCode::Char('e'))); // opens the erase confirm
        assert!(matches!(a.modal, Some(Modal::Confirm { .. })));
        // Stray keys must not dismiss a destructive-op confirm — and
        // Enter must not confirm it (repeat events can carry a held
        // Enter in from the path prompt).
        a.handle_key(key(KeyCode::Char('e')));
        a.handle_key(key(KeyCode::Char('5')));
        a.handle_key(key(KeyCode::Tab));
        a.handle_key(key(KeyCode::Enter));
        assert!(matches!(a.modal, Some(Modal::Confirm { .. })));
        a.handle_key(key(KeyCode::Char('n'))); // explicit cancel still works
        assert!(a.modal.is_none());
        assert!(a.op_running.is_none());
    }

    #[test]
    fn aborted_op_logs_short_status() {
        let mut a = app();
        a.op_running = Some(OpKind::Read);
        a.handle_event(AppEvent::OpDone {
            op: OpKind::Read,
            result: Ok(OpOutcome::Aborted {
                gate: "pin contact check",
            }),
        });
        assert!(a.op_running.is_none());
        let msg = &a.log_lines.back().unwrap().msg;
        assert!(msg.contains("read aborted (pin contact check)"));
        // The gate's detailed reason is NOT repeated here.
        assert!(!msg.contains("failed"));
    }

    #[test]
    fn relative_path_resolves_to_absolute() {
        let mut a = op_ready_app();
        let (tx, rx) = std::sync::mpsc::channel();
        a.cmd_tx = Some(tx);
        a.handle_key(key(KeyCode::Char('v'))); // verify opens a file path prompt
        a.handle_key(key(KeyCode::Char('x')));
        a.handle_key(key(KeyCode::Enter));
        match rx.try_recv() {
            Ok(WorkerCmd::Run {
                input: OpInput::File(p),
                op,
                ..
            }) => {
                assert_eq!(op, OpKind::Verify);
                assert!(p.is_absolute(), "worker must get an absolute path");
            }
            _ => panic!("expected Run with path"),
        }
    }

    #[test]
    fn opdone_clears_running_and_logs() {
        let mut a = app();
        a.op_running = Some(OpKind::Read);
        a.handle_event(AppEvent::OpDone {
            op: OpKind::Read,
            result: Ok(OpOutcome::Stats {
                stats: OpStats {
                    bytes: 4096,
                    crc32: 0xDEADBEEF,
                },
            }),
        });
        assert!(a.op_running.is_none());
        assert_eq!(a.last_stats.unwrap().bytes, 4096);
        assert!(a.log_lines.back().unwrap().msg.contains("4096 bytes"));
    }

    #[test]
    fn favorites_toggle() {
        let mut a = app();
        // Hermetic: no write-through to the real user data dir.
        a.favorites.clear();
        a.favorites_path = None;
        a.pane = Pane::Devices;
        a.handle_key(key(KeyCode::Char('f')));
        assert_eq!(a.favorites.len(), 1);
        a.handle_key(key(KeyCode::Char('f')));
        assert!(a.favorites.is_empty());
    }

    #[test]
    fn favorites_persist_write_through() {
        let mut a = app();
        a.favorites.clear();
        // Point persistence at a temp file, toggle, and confirm the
        // file round-trips the entry (name + manufacturer).
        let dir = std::env::temp_dir().join(format!("minipro-fav-test-{}", std::process::id()));
        let path = dir.join("favorites.txt");
        a.favorites_path = Some(path.clone());
        a.pane = Pane::Devices;
        a.handle_key(key(KeyCode::Char('f')));
        let loaded = prefs::load_favorites_from(&path);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, a.favorites[0].name);
        // Toggle off → file rewritten with zero favorites.
        a.handle_key(key(KeyCode::Char('f')));
        assert!(prefs::load_favorites_from(&path).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn search_ctrl_f_favorites_without_leaving_typing() {
        let mut a = app();
        a.favorites.clear();
        a.favorites_path = None;
        a.search_editing = true;
        a.search = "w25".into();
        a.refilter();
        // Ctrl-F favorites the cursor result while typing stays live.
        a.handle_key(KeyEvent::new_with_kind(
            KeyCode::Char('f'),
            KeyModifiers::CONTROL,
            KeyEventKind::Press,
        ));
        assert_eq!(a.favorites.len(), 1);
        assert!(a.search_editing);
        assert_eq!(a.search, "w25"); // 'f' was not inserted
                                     // Ctrl/Alt-modified chars are commands, not query text —
                                     // previously they inserted the letter.
        a.handle_key(KeyEvent::new_with_kind(
            KeyCode::Char('x'),
            KeyModifiers::CONTROL,
            KeyEventKind::Press,
        ));
        assert_eq!(a.search, "w25");
        // Plain and shifted chars still type (cursor was at 0).
        a.handle_key(key(KeyCode::Char('q')));
        assert_eq!(a.search, "qw25");
    }

    #[test]
    fn favorites_pin_top_and_ignore_query() {
        let mut a = app();
        a.favorites.clear();
        a.favorites_path = None;
        // Favorite a mid-list device → it pins to matches[0] and is
        // removed from the filtered body (no duplicate row).
        a.favorites.push(Favorite {
            name: "AT28C256".into(),
            manufacturer: "X".into(),
        });
        a.refilter();
        let at = a.devices.iter().position(|d| d.name == "AT28C256").unwrap();
        assert_eq!(a.matches[0], at);
        assert_eq!(a.fav_rows, 1);
        assert!(!a.matches[1..].contains(&at));
        // A query that doesn't match the favorite keeps it pinned —
        // "always visible regardless of the search query" (GUI parity).
        a.search = "w25q64".into();
        a.refilter();
        assert_eq!(a.matches[0], at);
        assert_eq!(a.fav_rows, 1);
        // 'f' on the pinned row unfavorites → it rejoins the filtered
        // list rather than vanishing, and the cursor stays on it.
        a.search.clear();
        a.refilter();
        a.list.select(Some(0));
        a.pane = Pane::Devices;
        a.handle_key(key(KeyCode::Char('f')));
        assert!(a.favorites.is_empty());
        assert_eq!(a.fav_rows, 0);
        assert!(a.matches.contains(&at));
        assert_eq!(a.matches[a.cursor()], at);
        // A favorite that isn't in this model's list doesn't pin —
        // availability rule mirrors the GUI.
        a.favorites.push(Favorite {
            name: "NOT-A-DEVICE".into(),
            manufacturer: "X".into(),
        });
        a.refilter();
        assert_eq!(a.fav_rows, 0);
        assert_eq!(a.matches.len(), 4);
    }

    #[test]
    fn wrap_log_fits_and_splits() {
        // Short message → one row.
        assert_eq!(wrap_log("hello world", 20), vec!["hello world"]);
        // Greedy fill — second word bumped to the next row.
        assert_eq!(wrap_log("aaa bbb ccc", 7), vec!["aaa bbb", "ccc"]);
        // Exact fit stays on one row.
        assert_eq!(wrap_log("aa bb", 5), vec!["aa bb"]);
    }

    #[test]
    fn wrap_log_hard_breaks_long_words() {
        assert_eq!(
            wrap_log("x abcdefghij y", 5),
            vec!["x", "abcde", "fghij", "y"]
        );
        // Tail of a hard break can't join a word that wouldn't fit whole.
        assert_eq!(wrap_log("abcdefghij zz", 6), vec!["abcdef", "ghij", "zz"]);
    }

    #[test]
    fn wrap_log_edges() {
        assert_eq!(wrap_log("", 10), vec![""]);
        assert_eq!(wrap_log("word", 0), vec!["word"]);
        // A word that exactly fills the width doesn't split.
        assert_eq!(wrap_log("abcde", 5), vec!["abcde"]);
    }

    // ── Hex-buffer view ─────────────────────────────────────────────

    #[test]
    fn read_dispatches_without_modal_and_read_fills_buffer() {
        let mut a = op_ready_app();
        let (tx, rx) = std::sync::mpsc::channel();
        a.cmd_tx = Some(tx);
        a.handle_key(key(KeyCode::Char('r')));
        // No path prompt — the read goes straight to the worker.
        assert!(a.modal.is_none());
        assert_eq!(a.op_running, Some(OpKind::Read));
        match rx.try_recv() {
            Ok(WorkerCmd::Run {
                op: OpKind::Read,
                input: OpInput::None,
                ..
            }) => {}
            _ => panic!("expected input-less Read"),
        }
        // OpDone::Read lands the bytes in the buffer and opens the view.
        a.handle_event(AppEvent::OpDone {
            op: OpKind::Read,
            result: Ok(OpOutcome::Read {
                stats: OpStats {
                    bytes: 32,
                    crc32: 0x1234,
                },
                data: (0u8..32).collect(),
            }),
        });
        assert!(a.hex_view);
        assert_eq!(a.hex.as_ref().unwrap().data.len(), 32);
        assert_eq!(a.hex.as_ref().unwrap().crc32, 0x1234);
    }

    #[test]
    fn o_opens_view_or_load_prompt() {
        let mut a = app();
        // No buffer yet → load-file prompt.
        a.handle_key(key(KeyCode::Char('o')));
        assert!(matches!(
            a.modal,
            Some(Modal::Path {
                action: PathAction::LoadBuffer,
                ..
            })
        ));
        a.handle_key(key(KeyCode::Esc));
        // With a buffer → straight into the view.
        a.hex = Some(HexBuf::new(vec![0xAA; 16], 0));
        a.handle_key(key(KeyCode::Char('o')));
        assert!(a.hex_view);
        assert!(a.modal.is_none());
    }

    #[test]
    fn hex_digits_overtype_and_advance() {
        let mut a = app();
        a.hex = Some(HexBuf::new(vec![0x00; 32], 0));
        a.hex_view = true;
        a.handle_key(key(KeyCode::Char('d')));
        a.handle_key(key(KeyCode::Char('e')));
        assert_eq!(a.hex.as_ref().unwrap().data[0], 0xDE);
        assert_eq!(a.hex.as_ref().unwrap().cursor, 1);
        assert!(a.hex.as_ref().unwrap().edited.contains(&0));
        a.handle_key(key(KeyCode::Char('a')));
        a.handle_key(key(KeyCode::Char('d')));
        assert_eq!(a.hex.as_ref().unwrap().data[1], 0xAD);
        assert_eq!(a.hex.as_ref().unwrap().cursor, 2);
    }

    #[test]
    fn hex_b_and_e_are_digits_not_ops() {
        // 'b' = blank check and 'e' = erase as mnemonics — inside the
        // hex view they must overtype nibbles, not dispatch ops.
        let mut a = op_ready_app();
        a.hex = Some(HexBuf::new(vec![0x00; 16], 0));
        a.hex_view = true;
        a.handle_key(key(KeyCode::Char('b')));
        a.handle_key(key(KeyCode::Char('e')));
        assert_eq!(a.hex.as_ref().unwrap().data[0], 0xBE);
        assert!(a.op_running.is_none());
        assert!(a.modal.is_none());
    }

    #[test]
    fn hex_nav_and_clamps() {
        let mut a = app();
        a.hex = Some(HexBuf::new(vec![0u8; 48], 0));
        a.hex_view = true;
        a.handle_key(key(KeyCode::Right));
        a.handle_key(key(KeyCode::Down));
        assert_eq!(a.hex.as_ref().unwrap().cursor, 17);
        a.handle_key(key(KeyCode::End)); // row end
        assert_eq!(a.hex.as_ref().unwrap().cursor, 31);
        a.handle_key(key(KeyCode::Char('G'))); // buffer end
        assert_eq!(a.hex.as_ref().unwrap().cursor, 47);
        a.handle_key(key(KeyCode::Right)); // clamped
        assert_eq!(a.hex.as_ref().unwrap().cursor, 47);
        a.handle_key(key(KeyCode::Char('g'))); // top
        assert_eq!(a.hex.as_ref().unwrap().cursor, 0);
    }

    #[test]
    fn hex_goto_modal_jumps_and_rejects() {
        let mut a = app();
        a.hex = Some(HexBuf::new(vec![0u8; 512], 0));
        a.hex_view = true;
        a.handle_key(key(KeyCode::Char(':')));
        assert!(matches!(a.modal, Some(Modal::Goto { .. })));
        for c in "0x40".chars() {
            a.handle_key(key(KeyCode::Char(c)));
        }
        a.handle_key(key(KeyCode::Enter));
        assert!(a.modal.is_none());
        assert_eq!(a.hex.as_ref().unwrap().cursor, 0x40);
        // Non-hex input can't be typed — 'z' is ignored.
        a.handle_key(key(KeyCode::Char(':')));
        a.handle_key(key(KeyCode::Char('z')));
        if let Some(Modal::Goto { input, .. }) = &a.modal {
            assert_eq!(input, "");
        } else {
            panic!("goto modal vanished");
        }
        a.handle_key(key(KeyCode::Esc));
    }

    #[test]
    fn hex_save_and_write_flows() {
        let mut a = op_ready_app();
        let (tx, rx) = std::sync::mpsc::channel();
        a.cmd_tx = Some(tx);
        a.hex = Some(HexBuf::new(vec![0x55; 64], 0xC0DE));
        a.hex_view = true;
        // 's' → save path prompt.
        a.handle_key(key(KeyCode::Char('s')));
        assert!(matches!(
            a.modal,
            Some(Modal::Path {
                action: PathAction::SaveBuffer,
                ..
            })
        ));
        a.handle_key(key(KeyCode::Esc));
        // 'w' → destructive confirm carrying the buffer payload.
        a.handle_key(key(KeyCode::Char('w')));
        assert!(matches!(a.modal, Some(Modal::Confirm { .. })));
        a.handle_key(key(KeyCode::Char('y')));
        match rx.try_recv() {
            Ok(WorkerCmd::Run {
                op: OpKind::Write,
                input: OpInput::Buffer(buf),
                ..
            }) => assert_eq!(buf.len(), 64),
            _ => panic!("expected buffer Write"),
        }
    }

    #[test]
    fn hex_view_nav_only_during_op() {
        let mut a = op_ready_app();
        a.hex = Some(HexBuf::new(vec![0u8; 64], 0));
        a.hex_view = true;
        a.op_running = Some(OpKind::Write);
        // Navigation still works — browsing during a write is safe.
        a.handle_key(key(KeyCode::Down));
        assert_eq!(a.hex.as_ref().unwrap().cursor, 16);
        // Edits are blocked: 'f' is a hex digit but must not overtype.
        a.handle_key(key(KeyCode::Char('f')));
        assert_eq!(a.hex.as_ref().unwrap().data[16], 0x00);
        // Esc leaves the view rather than warning.
        a.handle_key(key(KeyCode::Esc));
        assert!(!a.hex_view);
    }

    #[test]
    fn hex_ascii_mode_types_bytes() {
        let mut a = app();
        a.hex = Some(HexBuf::new(vec![0x00; 16], 0));
        a.hex_view = true;
        // 'i' enters ASCII-entry mode.
        a.handle_key(key(KeyCode::Char('i')));
        assert!(a.hex.as_ref().unwrap().ascii);
        // 'A' writes 0x41 — not nibble 0xA. 'b'/'e' are literal chars too.
        for c in "Abe".chars() {
            a.handle_key(key(KeyCode::Char(c)));
        }
        let h = a.hex.as_ref().unwrap();
        assert_eq!(&h.data[..3], &[0x41, 0x62, 0x65]);
        assert_eq!(h.cursor, 3);
        assert!(h.edited.contains(&1));
        // 'q' in ASCII mode types 0x71 — it must not close the view.
        a.handle_key(key(KeyCode::Char('q')));
        assert!(a.hex_view);
        assert_eq!(a.hex.as_ref().unwrap().data[3], 0x71);
        // 'i' in ASCII mode types 0x69 — it can never exit the mode.
        a.handle_key(key(KeyCode::Char('i')));
        assert_eq!(a.hex.as_ref().unwrap().data[4], 0x69);
        // Tab toggles back to hex mode (the hexedit/Okteta convention)
        // — and it must NOT close the view.
        a.handle_key(key(KeyCode::Tab));
        assert!(a.hex_view);
        assert!(!a.hex.as_ref().unwrap().ascii);
        // Tab toggles back in; Insert exits too; Esc exits mode, then
        // view.
        a.handle_key(key(KeyCode::Tab));
        assert!(a.hex.as_ref().unwrap().ascii);
        a.handle_key(key(KeyCode::Insert));
        assert!(!a.hex.as_ref().unwrap().ascii);
        a.handle_key(key(KeyCode::Insert));
        assert!(a.hex.as_ref().unwrap().ascii);
        a.handle_key(key(KeyCode::Esc));
        assert!(a.hex_view);
        assert!(!a.hex.as_ref().unwrap().ascii);
        a.handle_key(key(KeyCode::Esc));
        assert!(!a.hex_view);
    }

    #[test]
    fn hex_esc_returns_to_panes() {
        let mut a = app();
        a.hex = Some(HexBuf::new(vec![0u8; 16], 0));
        a.hex_view = true;
        a.handle_key(key(KeyCode::Esc));
        assert!(!a.hex_view);
        // A second Esc quits — the view consumed only the first.
        assert!(a.handle_key(key(KeyCode::Esc)));
    }

    #[test]
    fn hex_undo_restores_byte_and_clears_dirty() {
        let mut a = app();
        a.hex = Some(HexBuf::new(vec![0x00; 16], 0));
        a.hex_view = true;
        // Overtype byte 0 with 0xAB — two keystrokes, two undo entries.
        a.handle_key(key(KeyCode::Char('a')));
        a.handle_key(key(KeyCode::Char('b')));
        let h = a.hex.as_ref().unwrap();
        assert_eq!(h.data[0], 0xAB);
        assert!(h.edited.contains(&0));
        assert_eq!(h.cursor, 1);
        // First Backspace: back to the half-typed 0xA0 at cursor 0 —
        // still dirty.
        a.handle_key(key(KeyCode::Backspace));
        let h = a.hex.as_ref().unwrap();
        assert_eq!(h.data[0], 0xA0);
        assert_eq!(h.cursor, 0);
        assert!(h.edited.contains(&0));
        // Second: original 0x00 restored and the dirty mark cleared —
        // the byte equals the load-time buffer again.
        a.handle_key(key(KeyCode::Backspace));
        let h = a.hex.as_ref().unwrap();
        assert_eq!(h.data[0], 0x00);
        assert!(h.edited.is_empty());
        // Undo of nothing is a no-op, not a panic.
        a.handle_key(key(KeyCode::Backspace));
        assert_eq!(a.hex.as_ref().unwrap().data[0], 0x00);
    }

    #[test]
    fn hex_undo_in_ascii_mode() {
        let mut a = app();
        a.hex = Some(HexBuf::new(vec![0x00; 16], 0));
        a.hex_view = true;
        a.handle_key(key(KeyCode::Char('i')));
        a.handle_key(key(KeyCode::Char('A')));
        assert_eq!(a.hex.as_ref().unwrap().data[0], 0x41);
        a.handle_key(key(KeyCode::Backspace));
        let h = a.hex.as_ref().unwrap();
        assert_eq!(h.data[0], 0x00);
        assert_eq!(h.cursor, 0);
        assert!(h.edited.is_empty());
        assert!(h.ascii); // undo must not drop the entry mode
    }

    fn hex_find_typed(a: &mut App, pat: &str) {
        a.handle_key(key(KeyCode::Char('/')));
        for c in pat.chars() {
            a.handle_key(key(KeyCode::Char(c)));
        }
        a.handle_key(key(KeyCode::Enter));
    }

    #[test]
    fn hex_find_text_and_hex() {
        let mut a = app();
        let mut buf = vec![0x00; 64];
        buf[20] = 0xDE;
        buf[21] = 0xAD;
        buf[50] = b'h';
        buf[51] = b'i';
        a.hex = Some(HexBuf::new(buf, 0));
        a.hex_view = true;
        // Hex pattern via the x: prefix (spaces ignored).
        hex_find_typed(&mut a, "x:de ad");
        let h = a.hex.as_ref().unwrap();
        assert_eq!(h.cursor, 20);
        assert_eq!(h.last_find, vec![0xDE, 0xAD]);
        assert_eq!(h.note.as_deref(), Some("found @ 0x00000014"));
        // Literal text pattern.
        hex_find_typed(&mut a, "hi");
        assert_eq!(a.hex.as_ref().unwrap().cursor, 50);
        // Find is inclusive of the cursor on Enter — repeating moves on.
        hex_find_typed(&mut a, "hi");
        assert_eq!(a.hex.as_ref().unwrap().cursor, 50);
        a.handle_key(key(KeyCode::Char('n')));
        assert_eq!(a.hex.as_ref().unwrap().cursor, 50); // wraps to same single match
    }

    #[test]
    fn hex_find_repeat_and_not_found() {
        let mut a = app();
        let mut buf = vec![0x00; 64];
        buf[10] = 0x77;
        buf[40] = 0x77;
        a.hex = Some(HexBuf::new(buf, 0));
        a.hex_view = true;
        hex_find_typed(&mut a, "x:77");
        assert_eq!(a.hex.as_ref().unwrap().cursor, 10);
        // n repeats forward (exclusive), wraps at the end.
        a.handle_key(key(KeyCode::Char('n')));
        assert_eq!(a.hex.as_ref().unwrap().cursor, 40);
        a.handle_key(key(KeyCode::Char('n')));
        assert_eq!(a.hex.as_ref().unwrap().cursor, 10);
        // N repeats backward.
        a.handle_key(key(KeyCode::Char('N')));
        assert_eq!(a.hex.as_ref().unwrap().cursor, 40);
        // F3/Shift-F3 repeat in both modes — n/N are data in ASCII mode.
        a.handle_key(key(KeyCode::Char('i')));
        a.handle_key(KeyEvent::new_with_kind(
            KeyCode::F(3),
            KeyModifiers::NONE,
            KeyEventKind::Press,
        ));
        assert_eq!(a.hex.as_ref().unwrap().cursor, 10);
        a.handle_key(KeyEvent::new_with_kind(
            KeyCode::F(3),
            KeyModifiers::SHIFT,
            KeyEventKind::Press,
        ));
        assert_eq!(a.hex.as_ref().unwrap().cursor, 40);
        // Ctrl-F opens the find modal in ASCII mode too.
        a.handle_key(KeyEvent::new_with_kind(
            KeyCode::Char('f'),
            KeyModifiers::CONTROL,
            KeyEventKind::Press,
        ));
        assert!(matches!(a.modal, Some(Modal::Find { .. })));
        a.handle_key(key(KeyCode::Esc)); // close modal
        a.handle_key(key(KeyCode::Esc)); // leave ASCII mode
                                         // No match → the footer note reports it (log pane is hidden).
        a.hex.as_mut().unwrap().last_find = vec![0x99, 0x99];
        a.handle_key(key(KeyCode::Char('n')));
        assert_eq!(a.hex.as_ref().unwrap().note.as_deref(), Some("not found"));
    }

    #[test]
    fn hex_find_bad_pattern_keeps_modal() {
        let mut a = app();
        a.hex = Some(HexBuf::new(vec![0x00; 16], 0));
        a.hex_view = true;
        // Odd-length hex → cleared field, prompt stays open.
        hex_find_typed(&mut a, "x:abc");
        assert!(matches!(a.modal, Some(Modal::Find { .. })));
        a.handle_key(key(KeyCode::Esc));
        // Non-hex digits after x: → same.
        hex_find_typed(&mut a, "x:zz");
        assert!(matches!(a.modal, Some(Modal::Find { .. })));
        a.handle_key(key(KeyCode::Esc));
        // Empty input → same.
        a.handle_key(key(KeyCode::Char('/')));
        a.handle_key(key(KeyCode::Enter));
        assert!(matches!(a.modal, Some(Modal::Find { .. })));
        a.handle_key(key(KeyCode::Esc));
    }
}
