//! Worker thread — owns `MiniproHandle`, runs ops serialized by the
//! channel, and replies `AppEvent`s. Core ops are blocking, so this must
//! live off the UI thread (same reason the GUI uses `spawn_blocking`).
//!
//! Op sequences mirror `gui/src-tauri/src/commands.rs` exactly: ICSP
//! mode → begin_transaction → pin-check gate → chip-ID check → the op
//! body → end_transaction (always). Diverging here is how upstream-
//! parity bugs are born — when the GUI's sequence changes, change this.

use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use minipro_core::database::{find_device, DatabasePaths};
use minipro_core::device::{ChipType, ProgrammerInfo, ProgrammerModel};
use minipro_core::error::MiniproError;
use minipro_core::handle::MiniproHandle;
use minipro_core::operations::{
    blank_check, check_chip_id, chip_id_info, erase_chip, pin_check_gate_applies,
    pin_contact_check, read_chip_buffer, verify_chip, verify_chip_bytes, write_chip,
    write_chip_bytes, SizeMismatch,
};

use crate::events::{AppEvent, OpInput, OpKind, OpOptions, OpOutcome, SizeDiff};

/// UI → worker commands.
pub enum WorkerCmd {
    /// (Re)open the programmer — also the F5 reconnect path.
    Connect,
    /// Run one operation against the named database device.
    Run {
        op: OpKind,
        device_name: String,
        /// File or hex-buffer operand — `None` for read/erase/blank/id.
        input: OpInput,
        options: Box<OpOptions>,
    },
}

/// Spawn the worker; returns its command sender. The worker exits when
/// all senders drop (app shutdown).
pub fn spawn(paths: DatabasePaths, tx: Sender<AppEvent>) -> Sender<WorkerCmd> {
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    thread::spawn(move || worker(paths, cmd_rx, tx));
    cmd_tx
}

fn worker(paths: DatabasePaths, rx: Receiver<WorkerCmd>, tx: Sender<AppEvent>) {
    // Handle state: `Some` = real USB handle, `None` + `fake` = the
    // MINIPRO_FAKE_PROGRAMMER dev mode (info exists, no USB).
    let mut handle: Option<MiniproHandle> = None;
    let mut fake = false;

    while let Ok(cmd) = rx.recv() {
        match cmd {
            WorkerCmd::Connect => {
                handle = None;
                fake = false;
                match connect(&paths) {
                    Ok((h, info, is_fake)) => {
                        handle = h;
                        fake = is_fake;
                        let _ = tx.send(AppEvent::Connected {
                            info: Box::new(info),
                            fake: is_fake,
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(AppEvent::ConnectFailed(e.to_string()));
                    }
                }
            }
            WorkerCmd::Run {
                op,
                device_name,
                input,
                options,
            } => {
                let result = if fake {
                    Err("MINIPRO_FAKE_PROGRAMMER is set — operations need real hardware".into())
                } else {
                    match handle.as_mut() {
                        Some(h) => run_op(h, &paths, &tx, op, &device_name, input, &options),
                        None => Err("no programmer connected — press F5".into()),
                    }
                };
                // A dead-programmer signature drops the handle so the
                // next Run can't touch a stale USB endpoint — mirrors
                // the GUI's handle_usb_error → clear_programmer.
                if let Err(ref e) = result {
                    if looks_like_disconnect(e) {
                        handle = None;
                        fake = false;
                        let _ = tx.send(AppEvent::Disconnected);
                    }
                }
                let _ = tx.send(AppEvent::OpDone { op, result });
            }
        }
    }
}

/// Open the programmer, honoring MINIPRO_FAKE_PROGRAMMER first.
///
/// Returns `(handle, info, is_fake)` — `handle` is `None` in fake mode.
/// Real opens retry like the GUI: Windows USB enumeration can lag the
/// plug event by seconds.
fn connect(
    paths: &DatabasePaths,
) -> Result<(Option<MiniproHandle>, ProgrammerInfo, bool), MiniproError> {
    if let Some(info) = ProgrammerInfo::fake_from_env()? {
        log::warn!(
            "MINIPRO_FAKE_PROGRAMMER={}: no hardware, model faked",
            info.model
        );
        return Ok((None, info, true));
    }
    let mut last = None;
    for (i, delay) in [0u64, 500, 1000, 1500].iter().enumerate() {
        if *delay > 0 {
            thread::sleep(Duration::from_millis(*delay));
        }
        match MiniproHandle::open() {
            Ok(mut h) => {
                // T56/T76 algorithm lookup needs the database paths.
                h.db_paths = Some(paths.clone());
                let info = h.info.clone();
                return Ok((Some(h), info, false));
            }
            Err(e) => {
                if i + 1 < 4 {
                    log::info!("connect attempt {} failed: {}", i + 1, e);
                }
                last = Some(e);
            }
        }
    }
    Err(last.unwrap())
}

/// USB-error keyword table — mirrors `handle_usb_error` in commands.rs.
/// When an op error contains one of these, the programmer is gone.
fn looks_like_disconnect(err: &str) -> bool {
    const KEYS: [&str; 11] = [
        "STALL",
        "NoDevice",
        "LIBUSB_ERROR_NO_DEVICE",
        "LIBUSB_ERROR_IO",
        "LIBUSB_ERROR_PIPE",
        "DeviceNotFound",
        "endpoint",
        "USB error",
        "No programmer connected",
        "unknown error",
        "timed out",
    ];
    KEYS.iter().any(|k| err.contains(k))
}

fn log(tx: &Sender<AppEvent>, level: log::Level, msg: impl Into<String>) {
    let _ = tx.send(AppEvent::Log {
        level,
        msg: msg.into(),
    });
}

/// Resolve the device, set up the transaction, run the op, and always
/// end the transaction — the shared op skeleton.
fn run_op(
    h: &mut MiniproHandle,
    paths: &DatabasePaths,
    tx: &Sender<AppEvent>,
    op: OpKind,
    device_name: &str,
    input: OpInput,
    o: &OpOptions,
) -> Result<OpOutcome, String> {
    let model = h.info.model;
    let device = find_device(paths, device_name, model).map_err(|e| e.to_string())?;
    if device.chip_type == ChipType::Logic as u32 {
        return Err(
            "logic ICs support only the Logic test — not in this build (use CLI or GUI)".into(),
        );
    }
    let device = Arc::new(device);

    // ZIF mode is the MVP default; set_icsp_mode still auto-activates
    // ICSP for MP_ICSP_ONLY devices (upstream parity).
    h.set_icsp_mode("zif", &device);
    if h.icsp != 0 {
        log(
            tx,
            log::Level::Warn,
            format!("{} is ICSP-only — running with ICSP enabled", device.name),
        );
    }
    h.begin_transaction(device).map_err(|e| e.to_string())?;

    let r = run_in_transaction(h, paths, tx, op, input, o, model);
    let _ = h.end_transaction();
    r
}

/// Everything inside the transaction: pin check → ID check → op body.
fn run_in_transaction(
    h: &mut MiniproHandle,
    paths: &DatabasePaths,
    tx: &Sender<AppEvent>,
    op: OpKind,
    input: OpInput,
    o: &OpOptions,
    model: ProgrammerModel,
) -> Result<OpOutcome, String> {
    let device = h.device().map_err(|e| e.to_string())?.clone();

    // ── Pin contact check (pre-op gate) ─────────────────────────────
    if o.pin_check && pin_check_gate_applies(model, h.icsp != 0, device.pin_map) {
        log(tx, log::Level::Info, "Running pin contact check...");
        let res = pin_contact_check(h, &paths.infoic).map_err(|e| e.to_string())?;
        let _ = tx.send(AppEvent::PinTest {
            bad: res.bad_pins.clone(),
            expected: res.expected_pins.clone(),
            untestable: res.untestable_pins.clone(),
            all_open: res.all_expected_open(),
        });
        if !res.bad_pins.is_empty() {
            // All-open almost always means an empty socket, not N bad pins.
            let detail = if res.all_expected_open() {
                format!(
                    "all {} expected pins open — is a chip seated in the socket?",
                    res.bad_pins.len()
                )
            } else {
                let pins = res
                    .bad_pins
                    .iter()
                    .map(|p| p.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("bad contact on pin(s) {pins}")
            };
            log(
                tx,
                log::Level::Warn,
                format!("Pin contact check failed: {detail}"),
            );
            return Ok(OpOutcome::Aborted {
                gate: "pin contact check",
            });
        }
        log(tx, log::Level::Info, "Pin contact check passed");
    }

    // ── Chip-ID check (single top-level gate — the ID op reads it below) ──
    if o.check_device_id && op != OpKind::ChipId {
        match check_chip_id(h) {
            Ok(()) => log(tx, log::Level::Info, "Chip ID check passed"),
            Err(e) => {
                log(tx, log::Level::Error, format!("Chip ID check failed: {e}"));
                return Ok(OpOutcome::Aborted {
                    gate: "chip ID check",
                });
            }
        }
    }

    let mut progress = |done: usize, total: usize| {
        let _ = tx.send(AppEvent::Progress { done, total });
    };

    match op {
        OpKind::Read => {
            let data =
                read_chip_buffer(h, o.page, Some(&mut progress)).map_err(|e| e.to_string())?;
            let stats = minipro_core::operations::OpStats::from_buffer(&data);
            Ok(OpOutcome::Read { stats, data })
        }
        OpKind::Verify => {
            match input {
                OpInput::Buffer(buf) => verify_chip_bytes(h, buf, o.page, Some(&mut progress))
                    .map_err(|e| e.to_string())?,
                OpInput::File(p) => verify_chip(h, &p, o.page, o.format, Some(&mut progress))
                    .map_err(|e| e.to_string())?,
                OpInput::None => return Err("verify needs a file path or a buffer".into()),
            }
            Ok(OpOutcome::Verified)
        }
        OpKind::Erase => {
            let supported = device.flags.can_erase;
            if supported {
                erase_chip(h).map_err(|e| e.to_string())?;
            }
            Ok(OpOutcome::Erased { supported })
        }
        OpKind::Blank => match blank_check(h) {
            Ok(()) => Ok(OpOutcome::Blank),
            Err(MiniproError::NotBlank { address }) => Ok(OpOutcome::NotBlank { address }),
            Err(e) => Err(e.to_string()),
        },
        OpKind::ChipId => {
            let info = chip_id_info(h).map_err(|e| e.to_string())?;
            Ok(OpOutcome::ChipId(Box::new(info)))
        }
        OpKind::Write => {
            if matches!(input, OpInput::None) {
                return Err("write needs a file path or a buffer".into());
            }
            // Protect-off before erase/write — T76 always honors
            // off_protect_before; other models need the user's checkbox.
            // Mirrors the GUI's do_write.
            let is_t76 = model == ProgrammerModel::T76;
            if device.flags.off_protect_before && (is_t76 || o.unprotect_before) {
                log(tx, log::Level::Info, "Protect off...");
                h.protocol.protect_off(&h.usb).map_err(|e| e.to_string())?;
                log(tx, log::Level::Info, "Protect off...OK");
                if is_t76 {
                    h.end_transaction().map_err(|e| e.to_string())?;
                    h.begin_transaction(Arc::new(device.clone()))
                        .map_err(|e| e.to_string())?;
                }
            }
            // Auto-erase before write unless suppressed — gated on
            // can_erase like the CLI (UV EPROMs can't electrically erase).
            // The firmware requires a transaction reset after erase.
            if !o.skip_erase && device.flags.can_erase {
                erase_chip(h).map_err(|e| e.to_string())?;
                h.end_transaction().map_err(|e| e.to_string())?;
                h.begin_transaction(Arc::new(device.clone()))
                    .map_err(|e| e.to_string())?;
            }
            let stats = match &input {
                OpInput::Buffer(buf) => write_chip_bytes(
                    h,
                    buf.clone(),
                    o.page,
                    size_diff(o.size_diff),
                    o.skip_blank,
                    Some(&mut progress),
                )
                .map_err(|e| e.to_string())?,
                OpInput::File(p) => write_chip(
                    h,
                    p,
                    o.page,
                    o.format,
                    size_diff(o.size_diff),
                    o.skip_blank,
                    Some(&mut progress),
                )
                .map_err(|e| e.to_string())?,
                OpInput::None => unreachable!("checked above"),
            };
            if !o.skip_verify {
                match input {
                    OpInput::Buffer(buf) => verify_chip_bytes(h, buf, o.page, Some(&mut progress))
                        .map_err(|e| e.to_string())?,
                    OpInput::File(p) => verify_chip(h, &p, o.page, o.format, Some(&mut progress))
                        .map_err(|e| e.to_string())?,
                    OpInput::None => unreachable!("checked above"),
                }
            }
            if o.protect_after && device.flags.protect_after {
                log(tx, log::Level::Info, "Protect on...");
                h.protocol.protect_on(&h.usb).map_err(|e| e.to_string())?;
                log(tx, log::Level::Info, "Protect on...OK");
            }
            Ok(OpOutcome::Stats { stats })
        }
    }
}

fn size_diff(d: SizeDiff) -> SizeMismatch {
    match d {
        SizeDiff::Error => SizeMismatch::Error,
        SizeDiff::Warn => SizeMismatch::Warn,
        SizeDiff::Ignore => SizeMismatch::Ignore,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disconnect_keywords_match_usb_failures() {
        for msg in [
            "USB error: STALL",
            "LIBUSB_ERROR_NO_DEVICE",
            "No programmer connected",
            "read timed out",
        ] {
            assert!(looks_like_disconnect(msg), "{msg}");
        }
        for msg in [
            "chip ID mismatch",
            "verify failed at 0x10",
            "file is larger than the chip",
        ] {
            assert!(!looks_like_disconnect(msg), "{msg}");
        }
    }

    #[test]
    fn size_diff_maps_all_variants() {
        assert!(matches!(size_diff(SizeDiff::Error), SizeMismatch::Error));
        assert!(matches!(size_diff(SizeDiff::Warn), SizeMismatch::Warn));
        assert!(matches!(size_diff(SizeDiff::Ignore), SizeMismatch::Ignore));
    }
}
