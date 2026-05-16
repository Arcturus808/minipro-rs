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
//!   minipro --generate-completions bash  # print bash completions to stdout

use std::{path::PathBuf, process::ExitCode, sync::Arc};

use anyhow::{Context, Result};
use clap::{ArgAction, CommandFactory, Parser};
use clap_complete::{generate, shells};
use indicatif::{ProgressBar, ProgressStyle};
use minipro_core::{
    error::MiniproError,
    find_device, list_devices,
    operations::{
        blank_check, check_chip_id, check_ovc, erase_chip, firmware_update, logic_ic_test,
        read_chip, read_fuses, spi_autodetect, verify_chip, write_chip, write_fuses, FuseValue,
    },
    DatabasePaths, MiniproHandle,
};

// Cli struct is shared with build.rs for shell completion generation.
include!("cli.rs");

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

    // ── Shell completions ─────────────────────────────────────────────────────
    if let Some(ref shell_name) = cli.generate_completions {
        let mut cmd = Cli::command();
        let mut stdout = std::io::stdout();
        match shell_name.to_ascii_lowercase().as_str() {
            "bash" => generate(shells::Bash, &mut cmd, "minipro", &mut stdout),
            "zsh" => generate(shells::Zsh, &mut cmd, "minipro", &mut stdout),
            "fish" => generate(shells::Fish, &mut cmd, "minipro", &mut stdout),
            "powershell" => generate(shells::PowerShell, &mut cmd, "minipro", &mut stdout),
            other => {
                anyhow::bail!("unknown shell '{other}'; supported: bash, zsh, fish, powershell")
            }
        }
        return Ok(());
    }

    // ── List devices ─────────────────────────────────────────────────────────
    if let Some(list_arg) = cli.list {
        let filter = list_arg.as_deref();
        let db_paths =
            DatabasePaths::resolve(cli.infoic_path.as_deref(), cli.logicic_path.as_deref())?;
        let names = list_devices(&db_paths, filter)?;
        for name in &names {
            println!("{name}");
        }
        println!("{} devices found.", names.len());
        return Ok(());
    }

    // ── Operations that need USB ──────────────────────────────────────────────
    let mut handle = MiniproHandle::open().context("failed to open programmer")?;
    handle.icsp = cli.icsp;

    // ── Programmer info ───────────────────────────────────────────────────────
    if cli.info {
        handle.print_info();
        return Ok(());
    }

    // ── Firmware update (no device / begin_transaction needed) ────────────────
    if let Some(ref fw_path) = cli.firmware_update {
        let fw_data = std::fs::read(fw_path)
            .with_context(|| format!("cannot read firmware file {:?}", fw_path))?;
        eprintln!(
            "Updating firmware from {:?} ({} bytes)...",
            fw_path,
            fw_data.len()
        );
        firmware_update(&mut handle, &fw_data)?;
        return Ok(());
    }

    // ── Device required from here on ─────────────────────────────────────────
    let part = cli
        .part
        .as_deref()
        .context("no device specified (-p DEVICE)")?;

    let db_paths = DatabasePaths::resolve(cli.infoic_path.as_deref(), cli.logicic_path.as_deref())?;
    let device = Arc::new(
        find_device(&db_paths, part, handle.info.model)
            .with_context(|| format!("unknown device '{part}'"))?,
    );

    handle
        .begin_transaction(device.clone())
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
        logic_ic_test(handle)?;
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
        write_chip(handle, path, cli.page, &cli.format)?;
        pb.finish_and_clear();
        eprintln!("Written {:?}", path);

        if !cli.no_ovc_check {
            check_ovc(handle)?;
        }

        // Auto-verify after write (unless suppressed)
        if !cli.no_verify {
            eprint!("Verifying... ");
            verify_chip(handle, path, cli.page, &cli.format)?;
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
        read_chip(handle, path, cli.page, &cli.format)?;
        pb.finish_and_clear();
        eprintln!("Saved {:?}", path);

        if !cli.no_ovc_check {
            check_ovc(handle)?;
        }
    }

    // ── Verify ────────────────────────────────────────────────────────────────
    if let Some(ref path) = cli.verify {
        eprint!("Verifying {:?}... ", path);
        verify_chip(handle, path, cli.page, &cli.format)?;
        eprintln!("OK.");
    }

    // ── Read fuses ────────────────────────────────────────────────────────────
    if let Some(ref out_path) = cli.read_fuses {
        let values = read_fuses(handle)?;
        let mut text = String::new();
        for fv in &values {
            text.push_str(&format!("{}={:#04x}\n", fv.name, fv.value));
        }
        match out_path {
            Some(path) => {
                std::fs::write(path, &text)?;
                eprintln!("Fuses written to {:?}", path);
            }
            None => print!("{text}"),
        }
    }

    // ── Write fuses ───────────────────────────────────────────────────────────
    if let Some(ref path) = cli.write_fuses {
        let text = std::fs::read_to_string(path)?;
        let values = parse_fuse_file(&text)?;
        write_fuses(handle, &values)?;
        eprintln!("Fuses written.");
    }

    // ── SPI autodetect ────────────────────────────────────────────────────────
    if let Some(id_type_opt) = cli.spi_autodetect {
        let id_type = id_type_opt.unwrap_or(0);
        let jedec_id = spi_autodetect(handle, id_type)?;
        println!("JEDEC ID: {:#08x}", jedec_id);
    }

    // ── Protect on ────────────────────────────────────────────────────────────
    if cli.protect_on {
        handle.protocol.protect_on(&handle.usb)?;
    }

    Ok(())
}

// ── Fuse file parser ──────────────────────────────────────────────────────────

/// Parse a `key=value` fuse text file as produced by `--read-fuses`.
///
/// Each non-blank, non-comment line must have the form `NAME=VALUE` where
/// VALUE is a decimal or `0x`-prefixed hex integer.
fn parse_fuse_file(text: &str) -> anyhow::Result<Vec<FuseValue>> {
    let mut values = Vec::new();
    for (lineno, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (name, raw) = line.split_once('=').ok_or_else(|| {
            anyhow::anyhow!(
                "fuse file line {}: expected NAME=VALUE, got {:?}",
                lineno + 1,
                line
            )
        })?;
        let raw = raw.trim();
        let value = if let Some(hex) = raw.strip_prefix("0x").or_else(|| raw.strip_prefix("0X")) {
            u8::from_str_radix(hex, 16).map_err(|_| {
                anyhow::anyhow!("fuse file line {}: invalid hex value {:?}", lineno + 1, raw)
            })?
        } else {
            raw.parse::<u8>().map_err(|_| {
                anyhow::anyhow!(
                    "fuse file line {}: invalid decimal value {:?}",
                    lineno + 1,
                    raw
                )
            })?
        };
        values.push(FuseValue {
            name: name.trim().to_string(),
            value,
        });
    }
    Ok(values)
}
