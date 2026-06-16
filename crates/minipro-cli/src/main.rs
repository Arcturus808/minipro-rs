//! `minipro` — CLI front-end for minipro-rs.
//!
//! Usage mirrors the upstream C `minipro` program:
//!
//!   minipro -p DEVICE -r file.bin        # read
//!   minipro -p DEVICE -w file.bin        # write
//!   minipro -p DEVICE -m file.bin        # verify
//!   minipro -p DEVICE -E                 # erase
//!   minipro -p DEVICE -b                 # blank check
//!   minipro -p DEVICE -D                 # read chip ID
//!   minipro -l [filter]                  # list devices
//!   minipro --info                       # show programmer info
//!   minipro --generate-completions bash  # print bash completions to stdout

use std::{path::PathBuf, process::ExitCode, sync::Arc};

use anyhow::{Context, Result};
use clap::{ArgAction, CommandFactory, Parser};
use clap_complete::{generate, shells};
use clap_mangen::Man;
use indicatif::{ProgressBar, ProgressStyle};
use minipro_core::{
    device::ProgrammerModel,
    error::MiniproError,
    find_device, find_device_any, list_devices, list_devices_for_model,
    operations::{
        blank_check, check_chip_id, check_ovc, erase_chip, firmware_update, hardware_check,
        logic_ic_test, pin_contact_check, read_chip, read_fuses, spi_autodetect, verify_chip,
        write_chip, write_fuses, FuseValue, SizeMismatch,
    },
    DatabasePaths, MiniproHandle,
};

// Cli struct is shared with build.rs for shell completion generation.
include!("cli.rs");

fn main() -> ExitCode {
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

    let default_level = if cli.verbose { "info" } else { "warn" };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(default_level))
        .init();

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

    // ── Man page ──────────────────────────────────────────────────────────────
    if cli.generate_man {
        generate_man_page()?;
        return Ok(());
    }

    // ── List devices ─────────────────────────────────────────────────────────
    if let Some(list_arg) = cli.list {
        let filter = list_arg.as_deref();
        let db_paths = DatabasePaths::resolve(
            cli.infoic_path.as_deref(),
            cli.logicic_path.as_deref(),
            cli.algorithms_path.as_deref(),
        )?;
        let names = if let Some(ref model_str) = cli.programmer {
            let model: ProgrammerModel =
                model_str.parse().map_err(|e: String| anyhow::anyhow!(e))?;
            list_devices_for_model(&db_paths, filter, model)?
        } else {
            list_devices(&db_paths, filter)?
        };
        for name in &names {
            println!("{name}");
        }
        println!("{} devices found.", names.len());
        return Ok(());
    }

    // ── Device info (no USB needed) ───────────────────────────────────────────
    if let Some(ref device_name) = cli.get_info {
        let db_paths = DatabasePaths::resolve(
            cli.infoic_path.as_deref(),
            cli.logicic_path.as_deref(),
            cli.algorithms_path.as_deref(),
        )?;
        let dev = find_device_any(&db_paths, device_name)
            .with_context(|| format!("unknown device '{device_name}'"))?;
        print_device_info(&dev);
        return Ok(());
    }

    // ── Query supported programmer models ─────────────────────────────────────
    if cli.query_supported {
        println!("Supported programmers:");
        for model in [
            ProgrammerModel::Tl866cs,
            ProgrammerModel::Tl866a,
            ProgrammerModel::Tl866iiPlus,
            ProgrammerModel::T48,
            ProgrammerModel::T56,
            ProgrammerModel::T76,
        ] {
            println!("  {model}");
        }
        return Ok(());
    }

    // ── Operations that need USB ──────────────────────────────────────────────
    let mut handle = MiniproHandle::open().context("failed to open programmer")?;
    handle.icsp = cli.icsp || cli.icsp_no_vcc;

    // ── Programmer info ───────────────────────────────────────────────────────
    if cli.info {
        handle.print_info();
        return Ok(());
    }

    // ── Presence check ────────────────────────────────────────────────────────
    if cli.presence_check {
        println!(
            "Found {} firmware {}",
            handle.info.model, handle.info.firmware_str
        );
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

    // ── Hardware self-test ────────────────────────────────────────────────────
    if cli.hardware_check {
        eprint!("Running hardware self-test... ");
        hardware_check(&mut handle)?;
        eprintln!("PASS");
        return Ok(());
    }

    // ── SPI autodetect (no device context needed) ────────────────────────────
    if let Some(id_type_opt) = cli.spi_autodetect {
        if cli.part.is_none() {
            let id_type = id_type_opt.unwrap_or(0);
            let jedec_id = spi_autodetect(&mut handle, id_type)?;
            println!("JEDEC ID: {:#08x}", jedec_id);
            return Ok(());
        }
    }

    // ── Device required from here on ─────────────────────────────────────────
    let part = cli
        .part
        .as_deref()
        .context("no device specified (-p DEVICE)")?;

    let db_paths = DatabasePaths::resolve(
        cli.infoic_path.as_deref(),
        cli.logicic_path.as_deref(),
        cli.algorithms_path.as_deref(),
    )?;
    let mut device = find_device(&db_paths, part, handle.info.model)
        .with_context(|| format!("unknown device '{part}'"))?;
    apply_overrides(&mut device, &collect_overrides(&cli))?;
    let device = Arc::new(device);

    handle
        .begin_transaction(device.clone())
        .context("begin_transaction failed")?;

    let result = do_operations(&cli, &mut handle, part, &db_paths);

    // Always send end_transaction regardless of success/failure
    let _ = handle.end_transaction();

    result
}

// ── Page type ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
enum PageType {
    Code,
    Data,
    Config,
    User,
    Calibration,
}

impl PageType {
    /// Returns the protocol page_type byte, or None for pages that use
    /// dedicated protocol commands (config = fuse ops, calibration = special).
    fn as_protocol_page(self) -> Option<u8> {
        match self {
            Self::Code => Some(0x00),
            Self::Data => Some(0x01),
            Self::User => Some(0x02),
            Self::Config | Self::Calibration => None,
        }
    }
}

fn parse_page(s: &str) -> Result<PageType> {
    match s.to_ascii_lowercase().as_str() {
        "0" | "code" => Ok(PageType::Code),
        "1" | "data" => Ok(PageType::Data),
        "2" | "config" => Ok(PageType::Config),
        "3" | "user" => Ok(PageType::User),
        "4" | "calibration" => Ok(PageType::Calibration),
        _ => anyhow::bail!(
            "unknown page type '{s}'; expected: code, data, config, user, calibration, or 0-4"
        ),
    }
}

/// Determine the effective page type, giving `--fuses`/`--uid`/`--lock` priority
/// over `--page` / `-c`.  Errors if more than one shortcut flag is set at once.
fn resolve_page(cli: &Cli) -> Result<PageType> {
    let shortcuts = [
        (cli.fuses, "config", "--fuses"),
        (cli.uid, "user", "--uid"),
        (cli.lock, "config", "--lock"),
    ];
    let active: Vec<&str> = shortcuts
        .iter()
        .filter(|(f, _, _)| *f)
        .map(|(_, _, n)| *n)
        .collect();
    match active.len() {
        0 => parse_page(&cli.page),
        1 => parse_page(shortcuts.iter().find(|(f, _, _)| *f).unwrap().1),
        _ => anyhow::bail!("{} cannot be used together", active.join(", ")),
    }
}

fn do_operations(
    cli: &Cli,
    handle: &mut MiniproHandle,
    _part: &str,
    db_paths: &DatabasePaths,
) -> Result<()> {
    let page = resolve_page(cli)?;
    let proto_page: u8 = page.as_protocol_page().unwrap_or(0x00);

    // ── Chip ID ────────────────────────────────────────────────────────────────
    if cli.device_id {
        let (_, chip_id) = handle.protocol.get_chip_id(&handle.usb)?;
        println!("Chip ID: {:#010x}", chip_id);
        return Ok(());
    }

    // ── Pin contact check ─────────────────────────────────────────────────────
    if cli.pin_check {
        eprint!("Running pin contact check... ");
        pin_contact_check(handle, &db_paths.infoic)?;
        return Ok(());
    }

    // ── Logic IC test ─────────────────────────────────────────────────────────
    if cli.logic_test {
        eprint!("Testing logic IC... ");
        if let Some(ref out_path) = cli.logicic_out {
            let mut f = std::fs::File::create(out_path).with_context(|| {
                format!("cannot create logicic output file '{}'", out_path.display())
            })?;
            logic_ic_test(handle, &mut f)?;
        } else {
            logic_ic_test(handle, &mut std::io::stdout())?;
        }
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
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::with_template("{spinner} Erasing...")
                .unwrap_or_else(|_| ProgressStyle::default_spinner()),
        );
        pb.enable_steady_tick(std::time::Duration::from_millis(80));
        erase_chip(handle, !cli.skip_device_id)?;
        pb.finish_with_message("Erasing... done.");
    }

    // ── Blank check ───────────────────────────────────────────────────────────
    if cli.blank_check {
        eprint!("Checking blank... ");
        blank_check(handle)?;
        eprintln!("BLANK.");
    }

    // ── Write ─────────────────────────────────────────────────────────────────
    if let Some(ref path) = cli.write {
        if page == PageType::Config {
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("cannot read config file {:?}", path))?;
            let values = parse_fuse_file(&text)?;
            write_fuses(handle, &values)?;
            eprintln!("Config written.");
        } else if page == PageType::Calibration {
            anyhow::bail!("calibration page is read-only");
        } else {
            // Auto-erase before write (unless suppressed)
            if !cli.no_erase {
                let pb = ProgressBar::new_spinner();
                pb.set_style(
                    ProgressStyle::with_template("{spinner} Erasing...")
                        .unwrap_or_else(|_| ProgressStyle::default_spinner()),
                );
                pb.enable_steady_tick(std::time::Duration::from_millis(80));
                erase_chip(handle, !cli.skip_device_id)?;
                pb.finish_with_message("Erasing... done.");
                // The firmware requires a transaction reset after erase before
                // writing (same as the C reference: end_transaction then
                // begin_transaction).
                let device_arc = handle
                    .device
                    .clone()
                    .expect("device is set during an active transaction");
                handle.end_transaction()?;
                handle.begin_transaction(device_arc)?;
            }

            let size_mismatch = if cli.size_ignore {
                SizeMismatch::Ignore
            } else if cli.size_warn {
                SizeMismatch::Warn
            } else {
                SizeMismatch::Error
            };
            let pb = ProgressBar::new(0);
            pb.set_style(
                ProgressStyle::with_template(
                    "Writing  [{bar:40}] {percent}%  {bytes}/{total_bytes}",
                )
                .unwrap_or_else(|_| ProgressStyle::default_bar()),
            );
            let stats = write_chip(
                handle,
                path,
                proto_page,
                &cli.format,
                size_mismatch,
                cli.skip_blank,
                !cli.skip_device_id,
                Some(&mut |done, total| {
                    pb.set_length(total as u64);
                    pb.set_position(done as u64);
                }),
            )?;
            pb.finish_and_clear();
            let src_label = if path.to_str() == Some("-") {
                "stdin".to_string()
            } else {
                format!("{:?}", path)
            };
            eprintln!(
                "Written {}  ({} bytes, CRC-32: {:#010x})",
                src_label, stats.bytes, stats.crc32
            );

            if !cli.no_ovc_check {
                check_ovc(handle)?;
            }

            // C write_page_file does end_transaction + begin_transaction between
            // write and verify so the firmware flushes/commits written data.
            {
                let device_arc = handle.device.clone().expect("device set");
                handle.end_transaction()?;
                handle.begin_transaction(device_arc)?;
            }

            // Auto-verify after write (unless suppressed)
            if !cli.no_verify {
                let pb = ProgressBar::new(0);
                pb.set_style(
                    ProgressStyle::with_template(
                        "Verifying [{bar:40}] {percent}%  {bytes}/{total_bytes}",
                    )
                    .unwrap_or_else(|_| ProgressStyle::default_bar()),
                );
                verify_chip(
                    handle,
                    path,
                    proto_page,
                    &cli.format,
                    !cli.skip_device_id,
                    Some(&mut |done, total| {
                        pb.set_length(total as u64);
                        pb.set_position(done as u64);
                    }),
                )?;
                pb.finish_and_clear();
                eprintln!("Verified OK.");
            }
        }
    }

    // ── Read ──────────────────────────────────────────────────────────────────
    if let Some(ref path) = cli.read {
        if page == PageType::Config {
            let values = read_fuses(handle)?;
            let mut text = String::new();
            for fv in &values {
                text.push_str(&format!("{}={:#04x}\n", fv.name, fv.value));
            }
            std::fs::write(path, &text)
                .with_context(|| format!("cannot write config file {:?}", path))?;
            eprintln!("Config saved to {:?}", path);
        } else if page == PageType::Calibration {
            anyhow::bail!("calibration bytes are read-only and not yet supported");
        } else {
            let pb = ProgressBar::new(0);
            pb.set_style(
                ProgressStyle::with_template(
                    "Reading  [{bar:40}] {percent}%  {bytes}/{total_bytes}",
                )
                .unwrap_or_else(|_| ProgressStyle::default_bar()),
            );
            let stats = read_chip(
                handle,
                path,
                proto_page,
                &cli.format,
                !cli.skip_device_id,
                Some(&mut |done, total| {
                    pb.set_length(total as u64);
                    pb.set_position(done as u64);
                }),
            )?;
            pb.finish_and_clear();
            let dst_label = if path.to_str() == Some("-") {
                "stdout".to_string()
            } else {
                format!("{:?}", path)
            };
            eprintln!(
                "Saved {}  ({} bytes, CRC-32: {:#010x})",
                dst_label, stats.bytes, stats.crc32
            );

            if !cli.no_ovc_check {
                check_ovc(handle)?;
            }
        }
    }

    // ── Verify ────────────────────────────────────────────────────────────────
    if let Some(ref path) = cli.verify {
        if matches!(page, PageType::Config | PageType::Calibration) {
            anyhow::bail!(
                "verify is not supported for the '{}' page; use -r to read and compare manually",
                cli.page
            );
        }
        let pb = ProgressBar::new(0);
        pb.set_style(
            ProgressStyle::with_template("Verifying [{bar:40}] {percent}%  {bytes}/{total_bytes}")
                .unwrap_or_else(|_| ProgressStyle::default_bar()),
        );
        verify_chip(
            handle,
            path,
            proto_page,
            &cli.format,
            !cli.skip_device_id,
            Some(&mut |done, total| {
                pb.set_length(total as u64);
                pb.set_position(done as u64);
            }),
        )?;
        pb.finish_and_clear();
        eprintln!("Verified OK.");
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

// ── Device info printer ───────────────────────────────────────────────────────

fn fmt_bytes(n: u32) -> String {
    if n == 0 {
        return "0 bytes".to_string();
    }
    if n.is_multiple_of(1024 * 1024) {
        format!("{} MB ({} bytes)", n / (1024 * 1024), n)
    } else if n.is_multiple_of(1024) {
        format!("{} KB ({} bytes)", n / 1024, n)
    } else {
        format!("{} bytes", n)
    }
}

fn print_device_info(dev: &minipro_core::Device) {
    println!("Device:       {}", dev.name);
    println!("Code memory:  {}", fmt_bytes(dev.code_memory_size));
    if dev.data_memory_size > 0 {
        println!("Data memory:  {}", fmt_bytes(dev.data_memory_size));
    }
    if dev.data_memory2_size > 0 {
        println!("Data memory2: {}", fmt_bytes(dev.data_memory2_size));
    }
    if dev.page_size > 0 {
        println!("Page size:    {} bytes", dev.page_size);
    }
    if dev.pages_per_block > 0 {
        println!("Pages/block:  {}", dev.pages_per_block);
    }
    if dev.chip_id != 0 {
        println!(
            "Chip ID:      {:#010x} ({} byte{})",
            dev.chip_id,
            dev.chip_id_bytes_count,
            if dev.chip_id_bytes_count == 1 {
                ""
            } else {
                "s"
            }
        );
    }
    println!("Protocol ID:  {:#04x}", dev.protocol_id);
}

// ── Fuse file parser ──────────────────────────────────────────────────────────
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

// ── Man page generation ───────────────────────────────────────────────────────

/// Apply `-o KEY=VALUE` overrides to a device before `begin_transaction`.
///
/// Supported keys:
/// - `vpp=V`   — VPP programming voltage (e.g. `"12.0"`)
/// - `vdd=V`   — VDD write voltage (e.g. `"5.0"`)
/// - `vcc=V`   — VCC verify voltage (e.g. `"3.3"`)
/// - `pulse=N` — write pulse delay in microseconds (0–65535)
/// - `spi_clock=N` — SPI clock index (raw u8)
/// - `address=N`   — I²C slave address (0–255)
///
/// Merge individual long-form override flags (--vpp, --vcc, etc.) with any
/// `-o KEY=VALUE` entries into a single list for `apply_overrides`.
fn collect_overrides(cli: &Cli) -> Vec<String> {
    let mut all = cli.overrides.clone();
    if let Some(ref v) = cli.vpp {
        all.push(format!("vpp={v}"));
    }
    if let Some(ref v) = cli.vcc {
        all.push(format!("vcc={v}"));
    }
    if let Some(ref v) = cli.vdd {
        all.push(format!("vdd={v}"));
    }
    if let Some(ref v) = cli.pulse {
        all.push(format!("pulse={v}"));
    }
    if let Some(ref v) = cli.spi_clock {
        all.push(format!("spi_clock={v}"));
    }
    if let Some(ref v) = cli.address {
        all.push(format!("address={v}"));
    }
    all
}

fn apply_overrides(device: &mut minipro_core::device::Device, overrides: &[String]) -> Result<()> {
    // VPP voltage table (index 0..15 → volts), from tl866iiplus.c
    static VPP_TABLE: &[&str] = &[
        "9.0", "9.5", "10.0", "11.0", "11.5", "12.0", "12.5", "13.0", "13.5", "14.0", "14.5",
        "15.5", "16.0", "16.5", "17.0", "18.0",
    ];
    // VCC / VDD voltage table (index 0..15 → volts), from tl866iiplus.c
    static VCC_TABLE: &[&str] = &[
        "1.9", "2.7", "3.0", "3.3", "3.6", "3.9", "4.1", "4.5", "4.8", "5.0", "5.3", "5.5", "6.0",
        "6.3", "6.5", "7.0",
    ];

    for raw in overrides {
        let (key, value) = raw
            .split_once('=')
            .with_context(|| format!("invalid override '{raw}': expected KEY=VALUE"))?;
        match key.to_ascii_lowercase().as_str() {
            "vpp" => {
                let idx = VPP_TABLE
                    .iter()
                    .position(|&v| v == value)
                    .with_context(|| {
                        format!(
                            "invalid vpp voltage '{value}'; valid values: {}",
                            VPP_TABLE.join(", ")
                        )
                    })?;
                device.voltages.vpp = idx as u8;
            }
            "vdd" => {
                let idx = VCC_TABLE
                    .iter()
                    .position(|&v| v == value)
                    .with_context(|| {
                        format!(
                            "invalid vdd voltage '{value}'; valid values: {}",
                            VCC_TABLE.join(", ")
                        )
                    })?;
                device.voltages.vdd = idx as u8;
            }
            "vcc" => {
                let idx = VCC_TABLE
                    .iter()
                    .position(|&v| v == value)
                    .with_context(|| {
                        format!(
                            "invalid vcc voltage '{value}'; valid values: {}",
                            VCC_TABLE.join(", ")
                        )
                    })?;
                device.voltages.vcc = idx as u8;
            }
            "pulse" => {
                let n: u32 = value
                    .parse()
                    .with_context(|| format!("invalid pulse value '{value}': expected integer 0–65535"))?;
                anyhow::ensure!(n <= 65535, "pulse value {n} out of range (max 65535)");
                device.pulse_delay = n;
            }
            "spi_clock" => {
                let n: u8 = value
                    .parse()
                    .with_context(|| format!("invalid spi_clock value '{value}': expected integer 0–255"))?;
                device.spi_clock = n;
            }
            "address" => {
                let n: u8 = if let Some(hex) = value.strip_prefix("0x").or_else(|| value.strip_prefix("0X")) {
                    u8::from_str_radix(hex, 16)
                        .with_context(|| format!("invalid address value '{value}': expected hex like 0xA0"))?
                } else {
                    value
                        .parse()
                        .with_context(|| format!("invalid address value '{value}': expected integer 0–255 or hex 0xNN"))?
                };
                device.i2c_address = n;
            }
            other => anyhow::bail!(
                "unknown override key '{other}'; valid keys: vpp, vdd, vcc, pulse, spi_clock, address"
            ),
        }
    }
    Ok(())
}

fn generate_man_page() -> Result<()> {
    use std::io::Write;

    let cmd = Cli::command();
    let man = Man::new(cmd).date("2026-05-18");

    let mut out = std::io::stdout();

    // Auto-generated sections: title, name, synopsis, description, options.
    man.render_title(&mut out)?;
    man.render_name_section(&mut out)?;
    man.render_synopsis_section(&mut out)?;
    man.render_description_section(&mut out)?;
    man.render_options_section(&mut out)?;

    // Extended sections adapted from the upstream DavidGriffith/minipro man page.
    out.write_all(
        br#"
.SH NOTES ON FILE FORMATS
If the
.B \-\-format
option is not used when reading, the resulting file will be saved as a
raw binary file.
.P
If the ihex format is chosen and the data size is 64 kilobytes or smaller,
the file will be saved in ihex8 format.
If the data size exceeds 64 kilobytes, the ihex32 format is used.
.P
When writing chips, the format is automatically detected.
It is therefore not necessary to use the
.B \-\-format
option.

.SH NOTES ON MEMORY TYPES
The
.B \-\-page
option selects which memory region to operate on:
.TP
.B \-\-page 0
Code (flash/ROM) memory \(em the default.
.TP
.B \-\-page 1
Data (EEPROM) memory, where available.
.P
When
.B \-\-page
is omitted,
.B \-r
reads code memory and
.B \-w
writes code memory.
.P
Fuse and configuration bits are handled separately via
.B \-\-read\-fuses
and
.B \-\-write\-fuses .
.P
The following shorthand options select a named page without specifying a number:
.TP
.B \-\-fuses
Equivalent to
.BR "\-\-page config" .
Selects the fuse/configuration byte region.
.TP
.B \-\-uid
Equivalent to
.BR "\-\-page user" .
Selects the user/UID byte region (where available).
.TP
.B \-\-lock
Equivalent to
.BR "\-\-page config" .
Selects the lock-bit region.
Only one of
.BR \-\-fuses ", " \-\-uid ", " \-\-lock ", or " \-\-page
may be used at a time.

.SH DATABASE FILES
.I minipro
reads chip definitions from three XML files:
.TP
.B infoic.xml
Chip database (MCUs, memory chips, etc.).
.TP
.B logicic.xml
Logic IC database (for logic IC testing with
.BR \-\-logic\-test ).
.TP
.B algorithm.xml
FPGA bitstream algorithm descriptions (T56/T76 only).
.P
File paths can be overridden explicitly with
.BR \-\-infoic\-path ,
.BR \-\-logicic\-path ,
and
.BR \-\-algorithms .
Otherwise, files are searched in the following order:
.RS
.IP 1. 4
Current working directory.
.IP 2. 4
Directory containing the
.I minipro
executable.
.IP 3. 4
.B MINIPRO_HOME
environment variable.
.IP 4. 4
.B %PROGRAMDATA%\\eminipro\\e
(Windows) or
.B /usr/share/minipro/
(Unix).
.RE

.SH ALGORITHMS
The
.B \-\-algorithms
option specifies the path to
.IR algorithm.xml ,
which describes the FPGA bitstream algorithms used by the T56 and T76 programmers.
This file is only required for devices that use algorithm-based programming;
it is ignored for all other programmer models.
If not specified, the file is searched in the same four locations as
.I infoic.xml
(see
.B DATABASE FILES
above).

.SH UPDATING FIRMWARE
Firmware update files can be obtained from the manufacturer's website:
.nf
.B http://www.xgecu.com/en/
.fi
.P
For the TL866A/CS, use the "update.dat" file.
.P
For the TL866II+, use the "updateII.dat" file.
.P
For the T48, use the "UpdateT48.dat" file.
.P
For the T56, use the "updateT56.dat" file.
.P
For the T76, use the "updateT76.dat" file.

.SH EXAMPLES
.TP
.B minipro \-p ATMEGA48 \-D
Read the chip ID from the device inserted in the ZIF socket.
The
.B \-p
option is required because the programmer must configure socket voltages and
pin mapping before it can issue the ID read sequence.
.TP
.B minipro \-p ATMEGA48 \-r atmega48.bin
Read the contents of an ATmega48 into a file.
.TP
.B minipro \-p ATMEGA48 \-w atmega48.bin
Write the contents of a file to an ATmega48.
.TP
.B minipro \-p \(dqAT29C256@DIP28\(dq \-w foobar.bin
Write to an AT29C256 EEPROM.
Remember to put quotes around device names containing the @ sign.
.TP
.B minipro \-p \(dqW25Q128@SOIC8\(dq \-r flash.bin
Read a 16 MiB SPI NOR flash chip.
.TP
.B minipro \-p 7404 \-\-logic\-test
Check whether a 74(LS/HC/...)04 hex NOT gate chip works correctly.
.TP
.B minipro \-p ATMEGA48 \-r fuses.bin \-\-fuses
Read the fuse/configuration bytes of an ATmega48 into a file.
.TP
.B minipro \-p ATMEGA48 \-r uid.bin \-\-uid
Read the user/UID byte region of an ATmega48.
.TP
.B minipro \-p ATMEGA48 \-w flash.bin \-\-vpp 12.0 \-\-vcc 5.0
Write to an ATmega48 with explicit programming and supply voltages.
.TP
.B minipro \-p W25Q128@SOIC8 \-r flash.bin \-\-spi_clock 2
Read a SPI NOR flash with a lower SPI clock divisor.
.TP
.B minipro \-p 7404 \-\-logic\-test \-\-logicic\-out results.txt
Test a 74xx04 hex inverter and save the result table to a file.
.TP
.B minipro \-p ATMEGA48 \-r dump.bin \-I
Read an ATmega48 using ICSP without supplying VCC \(em the target board
provides its own power.
.TP
.B minipro \-\-info
Show programmer model, device code, serial number, firmware version, and hardware version.
.TP
.B minipro \-l AT89
List all devices whose name contains "AT89".
.TP
.B minipro \-\-info
Show programmer model, device code, serial number, firmware version, and hardware version.

.SH CAVEATS
The TL866A and TL866CS programmers appear to have a firmware bug such that
if not quite enough current is provided to them from a USB port, then the
programmer will fail to initialize itself or reset itself after an operation.
This problem seems to go hand\-in\-hand with newer USB 3.0 / xHCI ports and
can be avoided by using a powered hub.
.P
On Windows, the WinUSB driver must be installed via Zadig before the
programmer can be used.
Replug the device after driver installation.

.SH AUTHOR
.I minipro
was created by Valentin Dudouyt in 2014.
Many others have contributed code and bug reports.
Development of the original C project is coordinated by David Griffith.
.I minipro\-rs
is a Rust reimplementation by the minipro\-rs contributors.

.SH DISTRIBUTION
The canonical repository for
.I minipro\-rs
is at GitLab:
.nf
.B https://gitlab.com/arcturus8081/minipro\-rs/
.fi
.P
It is distributed under the GNU General Public License version 3 or
(at your option) any later version.
.nf
.B https://www.gnu.org/licenses/gpl\-3.0.en.html
.fi
"#,
    )?;

    Ok(())
}
