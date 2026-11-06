//! Shared protocol between the UI thread and the worker thread.
//!
//! `WorkerCmd` goes UI → worker (worker.rs); `AppEvent` comes back.
//! The `log` crate's records are forwarded as `AppEvent::Log` too, so
//! `info!`/`warn!` inside minipro-core (chip-ID OK, skipped-blank counts,
//! per-op device lines) lands in the log pane like the GUI's app-log.

use std::sync::mpsc::Sender;

use minipro_core::device::ProgrammerInfo;
use minipro_core::operations::{ChipIdInfo, OpStats};

/// The six P1 operations, in tab order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpKind {
    Read,
    Write,
    Verify,
    Erase,
    Blank,
    ChipId,
}

impl OpKind {
    /// Tab index → op. Tabs 6 (Logic) and 7 (Config) are P3 — no op yet.
    pub fn from_tab(i: usize) -> Option<Self> {
        Some(match i {
            0 => Self::Read,
            1 => Self::Write,
            2 => Self::Verify,
            3 => Self::Erase,
            4 => Self::Blank,
            5 => Self::ChipId,
            _ => return None,
        })
    }

    /// Display name for logs/progress.
    pub fn name(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Verify => "verify",
            Self::Erase => "erase",
            Self::Blank => "blank check",
            Self::ChipId => "chip ID",
        }
    }

    /// True for ops that alter the chip (get a confirm modal before run).
    pub fn is_destructive(self) -> bool {
        matches!(self, Self::Write | Self::Erase)
    }
}

/// "Size diff" select value — mirrors `operations::SizeMismatch`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SizeDiff {
    Error,
    Warn,
    Ignore,
}

/// Options pane snapshot taken when an op starts.
#[derive(Clone)]
pub struct OpOptions {
    pub check_device_id: bool,
    pub pin_check: bool,
    pub skip_erase: bool,
    pub skip_verify: bool,
    pub skip_blank: bool,
    pub unprotect_before: bool,
    pub protect_after: bool,
    /// 0x00 = code memory, 0x01 = data memory.
    pub page: u8,
    /// "auto" | "bin" | "ihex" | "srec".
    pub format: &'static str,
    pub size_diff: SizeDiff,
}

/// Successful outcome of a completed operation.
pub enum OpOutcome {
    /// Read or write finished — byte count + CRC for the buffer line.
    Stats {
        stats: OpStats,
    },
    Verified,
    /// `supported = false` means the device has no electrical erase and
    /// `erase_chip` was a no-op (UV EPROMs etc.) — worth telling the user.
    Erased {
        supported: bool,
    },
    Blank,
    NotBlank {
        address: u32,
    },
    ChipId(Box<ChipIdInfo>),
    /// The op never ran — a pre-op gate stopped it. The gate already
    /// logged the specific reason; `on_op_done` prints only the short
    /// status line. (Not an `Err`: the op didn't fail, it didn't run.)
    Aborted {
        gate: &'static str,
    },
}

/// Everything the worker thread can tell the app.
pub enum AppEvent {
    /// Programmer opened (or faked via MINIPRO_FAKE_PROGRAMMER).
    Connected {
        info: Box<ProgrammerInfo>,
        fake: bool,
    },
    /// Open attempt failed after all retries.
    ConnectFailed(String),
    /// Log line — from the log-crate forwarder or the worker itself.
    Log { level: log::Level, msg: String },
    /// Operation progress `(done, total)` for the gauge.
    Progress { done: usize, total: usize },
    /// Pin-contact result for ZIF highlighting — device pin numbers.
    PinTest {
        bad: Vec<u16>,
        expected: Vec<u16>,
        all_open: bool,
    },
    /// The programmer disappeared mid-op (USB error signature).
    Disconnected,
    /// An operation finished — `Err` carries the user-facing message.
    OpDone {
        op: OpKind,
        result: Result<OpOutcome, String>,
    },
}

/// `log::Log` impl forwarding records into the event channel.
///
/// Core's `info!` is mostly op-level lines ("Reading N bytes", "Chip ID
/// OK") — the right density for the pane. The two per-block `blank_check`
/// debug traces are filtered by prefix; everything else passes at Info.
struct TuiLogger(Sender<AppEvent>);

impl log::Log for TuiLogger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Info
    }

    fn log(&self, r: &log::Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        let msg = r.args().to_string();
        // Per-block debug traces — hundreds of lines per blank check.
        if msg.starts_with("blank_check:") {
            return;
        }
        let _ = self.0.send(AppEvent::Log {
            level: r.level(),
            msg,
        });
    }

    fn flush(&self) {}
}

/// Install the forwarding logger. Errors are ignored — logging is nice
/// to have, never a startup failure.
pub fn install_logger(tx: Sender<AppEvent>) {
    let _ = log::set_logger(Box::leak(Box::new(TuiLogger(tx))));
    log::set_max_level(log::LevelFilter::Info);
}
