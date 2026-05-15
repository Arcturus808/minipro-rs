//! `minipro` — CLI front-end for minipro-rs.
//!
//! Usage mirrors the upstream C `minipro` program:
//!
//!   minipro -p DEVICE -r file.bin        # read
//!   minipro -p DEVICE -w file.bin        # write
//!   minipro -p DEVICE -m file.bin        # verify
//!   minipro -p DEVICE -e                 # erase
//!   minipro -p DEVICE -b                 # blank check
//!   minipro -p DEVICE -D                 # read chip ID
//!   minipro -l [filter]                  # list devices
//!   minipro -I                           # show programmer info

use std::{
    path::PathBuf,
    process::ExitCode,
    sync::Arc,
};

use anyhow::{Context, Result};
use clap::{ArgAction, Parser};
use indicatif::{ProgressBar, ProgressStyle};
use minipro_core::{
    MiniproHandle,
    DatabasePaths,
    find_device,
    list_devices,
    error::MiniproError,
    operations::{
        blank_check, check_chip_id, check_ovc, erase_chip, read_chip, verify_chip, write_chip,
    },
};

#[derive(Debug, Parser)]
#[command(
    name    = "minipro",
    version,
    about   = "Open-source programmer for XGecu TL866xx/T48/T56/T76 chip programmers",
    long_about = None,
)]
struct Cli {
    /// Target device name (e.g. AT28C256, PIC16F628A, W25Q128)
    #[arg(short = 'p', long = "part", value_name = "DEVICE")]
    part: Option<String>,

    /// Read device memory to file
    #[arg(short = 'r', long = "read", value_name = "FILE")]
    read: Option<PathBuf>,

    /// Write file to device memory
    #[arg(short = 'w', long = "write", value_name = "FILE")]
    write: Option<PathBuf>,

    /// Verify file against device memory
    #[arg(short = 'm', long = "verify", value_name = "FILE")]
    verify: Option<PathBuf>,

    /// Erase device
    #[arg(short = 'e', long = "erase", action = ArgAction::SetTrue)]
    erase: bool,

    /// Blank-check device
    #[arg(short = 'b', long = "blank-check", action = ArgAction::SetTrue)]
    blank_check: bool,

    /// Read chip ID
    #[arg(short = 'D', long = "device-id", action = ArgAction::SetTrue)]
    device_id: bool,

    /// List supported devices (optional filter)
    #[arg(short = 'l', long = "list", value_name = "FILTER")]
    list: Option<Option<String>>,

    /// Show programmer info and exit
    #[arg(short = 'I', long = "info", action = ArgAction::SetTrue)]
    info: bool,

    /// Skip over-current check
    #[arg(long = "no-ovc-check", action = ArgAction::SetTrue)]
    no_ovc_check: bool,

    /// Enable ICSP mode
    #[arg(long = "icsp", action = ArgAction::SetTrue)]
    icsp: bool,

    /// Override path to infoic.xml
    #[arg(long = "infoic-path", value_name = "PATH")]
    infoic_path: Option<PathBuf>,

    /// Override path to logicic.xml
    #[arg(long = "logicic-path", value_name = "PATH")]
    logicic_path: Option<PathBuf>,

    /// Memory page: 0 = code (default), 1 = data
    #[arg(long = "page", default_value = "0", value_name = "N")]
    page: u8,

    /// Verbose output
    #[arg(short = 'v', long = "verbose", action = ArgAction::SetTrue)]
    verbose: bool,

    /// Skip erase before write
    #[arg(long = "no-erase", action = ArgAction::SetTrue)]
    no_erase: bool,

    /// Skip verify after write
    #[arg(long = "no-verify", action = ArgAction::SetTrue)]
    no_verify: bool,

    /// Disable write protection before operation
    #[arg(long = "protect-off", action = ArgAction::SetTrue)]
    protect_off: bool,

    /// Enable write protection after operation
    #[arg(long = "protect-on", action = ArgAction::SetTrue)]
    protect_on: bool,

    /// File format override: auto|bin|ihex|srec|jedec
    #[arg(long = "format", default_value = "auto", value_name = "FORMAT")]
    format: String,

    /// Skip chip ID verification
    #[arg(long = "skip-id", action = ArgAction::SetTrue)]
    skip_id: bool,

    /// Warn but continue on chip ID mismatch
    #[arg(long = "continue-id", action = ArgAction::SetTrue)]
    continue_id: bool,

    /// Test logic IC
    #[arg(long = "logic-test", action = ArgAction::SetTrue)]
    logic_test: bool,
}

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    // ── List devices ─────────────────────────────────────────────────────────
    if let Some(list_arg) = cli.list {
        let filter = list_arg.as_deref();
        let db_paths = DatabasePaths::resolve(
            cli.infoic_path.as_deref(),
            cli.logicic_path.as_deref(),
        )?;
        let names = list_devices(&db_paths, filter)?;
        for name in &names {
            println!("{name}");
        }
        println!("{} devices found.", names.len());
        return Ok(());
    }

    // ── Operations that need USB ──────────────────────────────────────────────
    let mut handle = MiniproHandle::open()
        .context("failed to open programmer")?;
    handle.icsp = cli.icsp;

    // ── Programmer info ───────────────────────────────────────────────────────
    if cli.info {
        handle.print_info();
        return Ok(());
    }

    // ── Device required from here on ─────────────────────────────────────────
    let part = cli.part.as_deref().context("no device specified (-p DEVICE)")?;

    let db_paths = DatabasePaths::resolve(
        cli.infoic_path.as_deref(),
        cli.logicic_path.as_deref(),
    )?;
    let device = Arc::new(
        find_device(&db_paths, part, handle.info.model)
            .with_context(|| format!("unknown device '{part}'"))?
    );

    handle.begin_transaction(device.clone())
        .context("begin_transaction failed")?;

    let result = do_operations(&cli, &mut handle, part);

    // Always send end_transaction regardless of success/failure
    let _ = handle.end_transaction();

    result
}

fn do_operations(cli: &Cli, handle: &mut MiniproHandle, _part: &str) -> Result<()> {
    // ── Chip ID ────────────────────────────────────────────────────────────────
    if cli.device_id {
        let (_, chip_id) = handle.protocol.get_chip_id(&handle.usb)?;
        println!("Chip ID: {:#010x}", chip_id);
        return Ok(());
    }

    // ── Logic IC test ─────────────────────────────────────────────────────────
    if cli.logic_test {
        eprint!("Testing logic IC... ");
        handle.protocol.logic_ic_test(&handle.usb)?;
        eprintln!("PASS.");
        return Ok(());
    }

    // ── Chip ID verification (before write/read ops) ───────────────────────────
    if !cli.skip_id && (cli.write.is_some() || cli.read.is_some()) {
        match check_chip_id(handle) {
            Ok(()) => {}
            Err(MiniproError::ChipIdMismatch { expected, actual }) if cli.continue_id => {
                eprintln!(
                    "WARNING: chip ID mismatch — expected {:#010x}, got {:#010x} — continuing",
                    expected, actual
                );
            }
            Err(e) => return Err(e.into()),
        }
    }

    // ── Protect off ───────────────────────────────────────────────────────────
    if cli.protect_off {
        handle.protocol.protect_off(&handle.usb)?;
    }

    // ── Erase ─────────────────────────────────────────────────────────────────
    if cli.erase {
        eprint!("Erasing... ");
        erase_chip(handle)?;
        eprintln!("done.");
    }

    // ── Blank check ───────────────────────────────────────────────────────────
    if cli.blank_check {
        eprint!("Checking blank... ");
        blank_check(handle)?;
        eprintln!("BLANK.");
    }

    // ── Write ─────────────────────────────────────────────────────────────────
    if let Some(ref path) = cli.write {
        // Auto-erase before write (unless suppressed)
        if !cli.no_erase {
            eprint!("Erasing... ");
            erase_chip(handle)?;
            eprintln!("done.");
        }

        let pb = ProgressBar::new(100);
        pb.set_style(
            ProgressStyle::with_template("Writing [{bar:40}] {percent}%")
                .unwrap_or_else(|_| ProgressStyle::default_bar()),
        );
        write_chip(handle, path, cli.page)?;
        pb.finish_and_clear();
        eprintln!("Written {:?}", path);

        if !cli.no_ovc_check {
            check_ovc(handle)?;
        }

        // Auto-verify after write (unless suppressed)
        if !cli.no_verify {
            eprint!("Verifying... ");
            verify_chip(handle, path, cli.page)?;
            eprintln!("OK.");
        }
    }

    // ── Read ──────────────────────────────────────────────────────────────────
    if let Some(ref path) = cli.read {
        let pb = ProgressBar::new(100);
        pb.set_style(
            ProgressStyle::with_template("Reading  [{bar:40}] {percent}%")
                .unwrap_or_else(|_| ProgressStyle::default_bar()),
        );
        read_chip(handle, path, cli.page)?;
        pb.finish_and_clear();
        eprintln!("Saved {:?}", path);

        if !cli.no_ovc_check {
            check_ovc(handle)?;
        }
    }

    // ── Verify ────────────────────────────────────────────────────────────────
    if let Some(ref path) = cli.verify {
        eprint!("Verifying {:?}... ", path);
        verify_chip(handle, path, cli.page)?;
        eprintln!("OK.");
    }

    // ── Protect on ────────────────────────────────────────────────────────────
    if cli.protect_on {
        handle.protocol.protect_on(&handle.usb)?;
    }

    Ok(())
}
