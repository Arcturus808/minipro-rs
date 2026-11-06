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
use minipro_core::zif::{device_to_zif, occupied_zif_pins, zif_spec, ZifInsertion};
use ratatui::widgets::ListState;

use crate::events::{AppEvent, OpKind, OpOptions, OpOutcome, SizeDiff};
use crate::worker::WorkerCmd;
use crate::zif::{Justify, Lever, ZifDiagram};

pub const OPS: [&str; 8] = [
    "Read", "Write", "Verify", "Erase", "Blank", "ID", "Logic", "Config",
];

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

/// Modal states — path prompt and destructive-op confirm swallow keys.
pub enum Modal {
    /// File path entry for Read/Write/Verify.
    Path {
        op: OpKind,
        title: &'static str,
        input: String,
        cursor: usize,
    },
    /// y/N confirmation for destructive ops (write, erase).
    Confirm {
        op: OpKind,
        path: Option<PathBuf>,
        lines: Vec<String>,
    },
}

/// Pin-test result mapped onto ZIF pin numbers for the diagram.
pub struct PinTestView {
    /// ZIF pins that failed contact.
    pub bad: std::collections::BTreeSet<u8>,
    /// ZIF pins that were tested (good or bad).
    pub tested: std::collections::BTreeSet<u8>,
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
    /// Last file path handed to a file op — for the buffer line.
    pub last_path: Option<String>,
    /// Last read/write stats — the buffer line + CRC display.
    pub last_stats: Option<OpStats>,
    /// Last chip-ID result (the ID tab also shows it in the log).
    pub chip_id: Option<ChipIdInfo>,
    /// Session favorites (device names) — `f` toggles in DEVICES.
    /// Persistence is P4 (`tui.toml`); favorites survive only per run.
    pub favorites: Vec<String>,
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
            last_path: None,
            last_stats: None,
            chip_id: None,
            favorites: Vec::new(),
            paths,
            model,
            cmd_tx: None,
        }
    }

    /// Database paths — the worker needs a clone at spawn time.
    pub fn paths(&self) -> &DatabasePaths {
        &self.paths
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
                all_open,
            } => self.set_pin_test(bad, expected, all_open),
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
    fn set_pin_test(&mut self, bad: Vec<u16>, expected: Vec<u16>, all_open: bool) {
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
            Ok(OpOutcome::Stats { stats }) => {
                self.last_stats = Some(stats);
                let verb = if op == OpKind::Write { "wrote" } else { "read" };
                let target = self
                    .last_path
                    .clone()
                    .map(|p| format!(" → {p}"))
                    .unwrap_or_default();
                self.push_log(
                    log::Level::Info,
                    format!(
                        "✓ {} {} bytes{} · CRC-32 {:08X}",
                        verb, stats.bytes, target, stats.crc32
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
            OpKind::Read => {
                let input = format!("{}.bin", device_name.replace(['/', '@'], "_"));
                self.modal = Some(Modal::Path {
                    op,
                    title: "Save read to",
                    cursor: input.len(),
                    input,
                });
            }
            OpKind::Write => {
                self.modal = Some(Modal::Path {
                    op,
                    title: "Write file",
                    input: String::new(),
                    cursor: 0,
                });
            }
            OpKind::Verify => {
                self.modal = Some(Modal::Path {
                    op,
                    title: "Verify against file",
                    input: String::new(),
                    cursor: 0,
                });
            }
            OpKind::Erase => {
                self.modal = Some(Modal::Confirm {
                    op,
                    path: None,
                    lines: vec![
                        format!("Erase {device_name}?"),
                        "this erases the whole chip".to_string(),
                    ],
                });
            }
            OpKind::Blank | OpKind::ChipId => self.send_run(op, None),
        }
    }

    /// Dispatch the op to the worker. `path` is `Some` for file ops.
    fn send_run(&mut self, op: OpKind, path: Option<PathBuf>) {
        let Some(tx) = self.cmd_tx.clone() else {
            self.push_log(log::Level::Error, "worker not started");
            return;
        };
        let Some(dev) = &self.selected else {
            return;
        };
        let device_name = dev.name.clone();
        self.last_path = path.as_ref().map(|p| {
            p.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        });
        self.op_running = Some(op);
        self.op_started = Some(Instant::now());
        self.progress = Some((0, 1));
        let options = Box::new(self.op_options());
        self.push_log(log::Level::Info, format!("{} {}…", op.name(), device_name));
        let _ = tx.send(WorkerCmd::Run {
            op,
            device_name,
            path,
            options,
        });
        self.modal = None;
    }

    /// `f` in the devices pane — toggle the cursor device as a favorite.
    fn toggle_favorite(&mut self) {
        let Some(&idx) = self.matches.get(self.cursor()) else {
            return;
        };
        let name = self.devices[idx].name.clone();
        if let Some(pos) = self.favorites.iter().position(|f| f == &name) {
            self.favorites.remove(pos);
        } else {
            self.favorites.push(name);
        }
    }

    // ── Modal handling ───────────────────────────────────────────────

    fn handle_modal_key(&mut self, k: KeyEvent) {
        match self.modal.take() {
            Some(Modal::Path {
                op,
                title,
                mut input,
                mut cursor,
            }) => match k.code {
                KeyCode::Esc => {} // dropped — cancel
                KeyCode::Enter => {
                    if input.trim().is_empty() {
                        self.modal = Some(Modal::Path {
                            op,
                            title,
                            input,
                            cursor,
                        });
                        return;
                    }
                    let path = PathBuf::from(input.trim());
                    if op.is_destructive() {
                        let name = self
                            .selected
                            .as_ref()
                            .map(|d| d.name.clone())
                            .unwrap_or_default();
                        self.modal = Some(Modal::Confirm {
                            op,
                            path: Some(path.clone()),
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
                    } else {
                        self.send_run(op, Some(path));
                    }
                }
                KeyCode::Char(c) => {
                    input.insert(cursor, c);
                    cursor += c.len_utf8();
                    self.modal = Some(Modal::Path {
                        op,
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
                        op,
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
                        op,
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
                        op,
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
                        op,
                        title,
                        input,
                        cursor,
                    });
                }
                KeyCode::Home => {
                    cursor = 0;
                    self.modal = Some(Modal::Path {
                        op,
                        title,
                        input,
                        cursor,
                    });
                }
                KeyCode::End => {
                    cursor = input.len();
                    self.modal = Some(Modal::Path {
                        op,
                        title,
                        input,
                        cursor,
                    });
                }
                _ => {
                    self.modal = Some(Modal::Path {
                        op,
                        title,
                        input,
                        cursor,
                    });
                }
            },
            Some(Modal::Confirm { op, path, .. }) => match k.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                    self.send_run(op, path);
                }
                _ => {} // any other key cancels — Esc, n, arrows
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
        let (bad, tested, show_test) = match &self.pin_test {
            Some(pt) => (pt.bad.clone(), pt.tested.clone(), true),
            None => (Default::default(), Default::default(), false),
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
            show_test,
            chip_top,
            chip_bot,
            interior: 10,
        }
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
        // aborted mid-transfer (same as the GUI's blocking dialog).
        if self.op_running.is_some() {
            match k.code {
                KeyCode::Tab => self.cycle_pane(1),
                KeyCode::BackTab => self.cycle_pane(-1),
                KeyCode::Char(']') => self.toggle_right(),
                KeyCode::Char('?') => self.show_help = true,
                KeyCode::Char('q') | KeyCode::Esc => self.push_log(
                    log::Level::Warn,
                    "operation in progress — it can't be aborted; wait for it to finish",
                ),
                _ => {}
            }
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
            KeyCode::Char(c @ '1'..='8') => self.op = (c as usize) - ('1' as usize),
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
        a.pane = Pane::Devices;
        a.handle_key(key(KeyCode::Char('f')));
        assert_eq!(a.favorites.len(), 1);
        a.handle_key(key(KeyCode::Char('f')));
        assert!(a.favorites.is_empty());
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
}
