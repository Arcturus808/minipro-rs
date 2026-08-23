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
        logic_ic_test, pin_contact_check, read_chip, read_chip_calibration, read_fuses,
        spi_autodetect_and_lookup, verify_chip, write_chip, write_fuses, FuseValue, SizeMismatch,
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

/// Parse an erase-value string: accepts "0xFF", "0x00", "255", "0", etc.
fn parse_erase_value(s: &str) -> Result<u8> {
    let s = s.trim();
    let val = if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u8::from_str_radix(hex, 16)
    } else {
        s.parse::<u8>()
    }
    .map_err(|e| anyhow::anyhow!("invalid erase-value '{s}': {e}"))?;
    Ok(val)
}

/// Parse a u64 from decimal or hex (0x-prefixed).
fn parse_u64(s: &str) -> Result<u64> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16)
    } else {
        s.parse::<u64>()
    }
    .map_err(|e| anyhow::anyhow!("invalid value '{s}': {e}"))
}

/// Parse a usize from decimal or hex (0x-prefixed).
fn parse_usize(s: &str) -> Result<usize> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        usize::from_str_radix(hex, 16)
    } else {
        s.parse::<usize>()
    }
    .map_err(|e| anyhow::anyhow!("invalid value '{s}': {e}"))
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
        for item in &names {
            println!("{}", item.name);
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

    // ── Diff two firmware files (no USB needed) ──────────────────────────────
    if let Some(ref diff_files) = cli.diff {
        let files = diff_files;
        let file_a = &files[0];
        let file_b = &files[1];

        // Parse erase value (hex or decimal)
        let erase_value = parse_erase_value(&cli.erase_value)?;

        // Read both files as raw binary (no format parsing — diff compares bytes)
        let buf_a = std::fs::read(file_a).with_context(|| format!("cannot read {:?}", file_a))?;
        let buf_b = std::fs::read(file_b).with_context(|| format!("cannot read {:?}", file_b))?;

        let result = minipro_core::smart_diff(&buf_a, &buf_b, erase_value);
        let report = minipro_core::format_diff_report(&result, erase_value);
        print!("{report}");

        return if result.summary.is_equal {
            Ok(())
        } else {
            // Exit code 1 on mismatch (like diff(1))
            Err(anyhow::anyhow!("files differ"))
        };
    }

    // ── Operations that need USB ──────────────────────────────────────────────
    let mut handle = MiniproHandle::open().context("failed to open programmer")?;
    // ICSP mode: -i enables ICSP with VCC, -I enables ICSP without VCC.
    // The bitmask (0x80=enable, 0x01=VCC) is sent in begin_transaction.
    if cli.icsp {
        handle.set_icsp(true);
    } else if cli.icsp_no_vcc {
        handle.set_icsp(false);
    }

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
        eprintln!(
            "WARNING: Firmware update is experimental and has not been validated on real hardware."
        );
        eprintln!("Do NOT disconnect the device during the update. Use at your own risk.");
        let fw_data = std::fs::read(fw_path)
            .with_context(|| format!("cannot read firmware file {:?}", fw_path))?;
        eprintln!(
            "Updating firmware from {:?} ({} bytes)...",
            fw_path,
            fw_data.len()
        );
        firmware_update(&mut handle, &fw_data, &mut std::io::stderr(), None)?;
        return Ok(());
    }

    // ── Hardware self-test ────────────────────────────────────────────────────
    if cli.hardware_check {
        if cli.verbose {
            eprintln!("Running hardware self-test...");
        } else {
            eprint!("Running hardware self-test... ");
        }
        hardware_check(&mut handle)?;
        eprintln!("PASS");
        return Ok(());
    }

    // ── SPI autodetect (no device context needed) ────────────────────────────
    if let Some(id_type_opt) = cli.spi_autodetect {
        if cli.part.is_none() {
            let id_type = id_type_opt.unwrap_or(0);
            let db_paths = DatabasePaths::resolve(
                cli.infoic_path.as_deref(),
                cli.logicic_path.as_deref(),
                cli.algorithms_path.as_deref(),
            )?;
            let result = spi_autodetect_and_lookup(&mut handle, &db_paths, id_type)?;
            eprintln!("Autodetecting device (ID:0x{:04X})", result.jedec_id);
            if result.matches.is_empty() {
                if result.jedec_id == 0 {
                    eprintln!("No SPI chip detected.");
                } else {
                    eprintln!("No device found.");
                }
            } else {
                for item in &result.matches {
                    println!("{}", item.name);
                }
                eprintln!("{} device(s) found.", result.matches.len());
            }
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

    // Capture the database-default VCC code so we can warn if the user
    // overrides it to a value that may produce unreliable results.
    let default_vcc = device.voltages.vcc;
    let is_logic = device.chip_type == minipro_core::device::ChipType::Logic as u32;
    let model = handle.info.model;

    apply_overrides(&mut device, &collect_overrides(&cli), model)?;

    // Warn if VCC was overridden away from the database default.
    if device.voltages.vcc != default_vcc {
        let table = minipro_core::device::vcc_voltage_table(
            model,
            device.chip_type,
            device.flags.custom_protocol,
        );
        let default_name = table
            .and_then(|t| minipro_core::device::voltage_name(t, default_vcc))
            .unwrap_or("?");
        let override_name = table
            .and_then(|t| minipro_core::device::voltage_name(t, device.voltages.vcc))
            .unwrap_or("?");
        eprintln!(
            "WARNING: VCC overridden from {default_name}V to {override_name}V; \
             results may be unreliable for this chip."
        );
        if !is_logic {
            eprintln!(
                "  The database default is {default_name}V. Reading or blank-checking \
                 at a different VCC may produce false results (e.g. all 0xFF)."
            );
        }
    }

    let device = Arc::new(device);

    // Auto-activate ICSP for ICSP-only chips; force ZIF for ZIF-only chips.
    // Matches upstream C minipro main.c logic.
    use minipro_core::device::{MP_ICSP_ONLY, MP_ZIF_ONLY};
    if device.flags.prog_support == MP_ICSP_ONLY {
        handle.set_icsp(true);
        eprintln!("Activating ICSP...");
    } else if device.flags.prog_support == MP_ZIF_ONLY {
        if handle.icsp != 0 {
            eprintln!("Warning: ICSP is not supported by this chip.");
        }
        handle.icsp = 0;
    }

    // Populate db_paths on the handle so begin_transaction can look up
    // T56/T76 FPGA algorithm bitstreams from algorithm.xml.
    handle.db_paths = Some(db_paths.clone());

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

    // ── Reject -x/--skip-id in write/erase mode (upstream parity) ────────────
    // Upstream minipro explicitly forbids skipping the ID check for write and
    // erase actions.  Use -y/--continue-id to warn-but-continue on mismatch.
    if cli.skip_id && (cli.write.is_some() || cli.erase) {
        if cli.continue_id {
            anyhow::bail!(
                "-x / --skip-id is not permitted for write or erase actions.\n\
                 Remove -x / --skip-id from the command; -y / --continue-id\n\
                 is already set and will handle the ID mismatch."
            );
        } else {
            anyhow::bail!(
                "-x / --skip-id is not permitted for write or erase actions.\n\
                 Remove -x / --skip-id from the command. To continue despite a\n\
                 chip ID mismatch, use -y / --continue-id."
            );
        }
    }

    // ── Chip ID ────────────────────────────────────────────────────────────────
    if cli.device_id {
        let device = handle
            .device
            .as_ref()
            .context("no device selected")?
            .clone();
        // begin_transaction is required before get_chip_id on T76 NAND/eMMC
        // devices — it powers the socket adapter, uploads the FPGA bitstream,
        // and sends NAND geometry setup. Without it, READID returns 0x00000000.
        // Matches the C minipro flow where -D falls through to check_chip_id,
        // which calls begin_transaction before get_chip_id.
        handle.protocol.begin_transaction(&handle.usb, &device, 0)?;
        let (_, chip_id) = handle.protocol.get_chip_id(&handle.usb, &device)?;
        handle.protocol.end_transaction(&handle.usb)?;
        println!("Chip ID: {:#010x}", chip_id);
        return Ok(());
    }

    // ── Pin contact check ─────────────────────────────────────────────────────
    if cli.pin_check {
        if handle.icsp != 0 {
            eprintln!("Pin test is not supported in ICSP mode.");
            return Ok(());
        }
        if !matches!(
            handle.info.model,
            ProgrammerModel::Tl866iiPlus | ProgrammerModel::T48
        ) {
            eprintln!("Pin test is not supported on this programmer model.");
            return Ok(());
        }
        if cli.verbose {
            eprintln!("Running pin contact check...");
        } else {
            eprint!("Running pin contact check... ");
        }
        let result = pin_contact_check(handle, &db_paths.infoic)?;
        if result.bad_pins.is_empty() {
            if cli.verbose {
                eprintln!("Pin test passed.");
            } else {
                eprintln!("OK");
            }
        } else {
            if !cli.verbose {
                eprintln!();
            }
            for pin in &result.bad_pins {
                eprintln!("Bad contact on pin: {}", pin);
            }
            return Err(anyhow::anyhow!(
                "Pin contact test failed: {} bad pin(s)",
                result.bad_pins.len()
            ));
        }
        return Ok(());
    }

    // ── Logic IC test ─────────────────────────────────────────────────────────
    if cli.logic_test {
        let result = if let Some(ref out_path) = cli.logicic_out {
            let mut f = std::fs::File::create(out_path).with_context(|| {
                format!("cannot create logicic output file '{}'", out_path.display())
            })?;
            logic_ic_test(handle, &mut f)
        } else {
            logic_ic_test(handle, &mut std::io::stdout())
        };
        // The function prints its own "Logic test successful." or
        // "Logic test failed: N errors encountered." to stderr.
        // Suppress the default error handler to avoid a duplicate line.
        if let Err(e) = result {
            if let MiniproError::Protocol(ref msg) = e {
                if msg.starts_with("Logic test failed") {
                    std::process::exit(1);
                }
            }
            return Err(e.into());
        }
        return Ok(());
    }

    // ── Chip ID verification (single check before all ops) ───────────────────
    // Upstream minipro has one check_chip_id call in the main flow (main.c
    // line 3563), not per-operation checks.  We do the same here: one check
    // that handles -x (skip), -y (warn + continue), and the default (error).
    // Per-operation check_device_id params are all false (see below).
    if !cli.skip_id
        && (cli.write.is_some() || cli.read.is_some() || cli.erase || cli.verify.is_some())
    {
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

    // ── Erase ─────────────────────────────────────────────────────────────────
    if cli.erase {
        let can_erase = handle.device().map(|d| d.flags.can_erase).unwrap_or(false);
        if can_erase {
            let pb = ProgressBar::new_spinner();
            pb.set_style(
                ProgressStyle::with_template("{spinner} Erasing...")
                    .unwrap_or_else(|_| ProgressStyle::default_spinner()),
            );
            pb.enable_steady_tick(std::time::Duration::from_millis(80));
            erase_chip(handle, false)?;
            pb.finish_with_message("Erasing... done.");
        } else {
            eprintln!("This chip does not support electrical erase (use UV light for UV EPROMs).");
        }
    }

    // ── Blank check ───────────────────────────────────────────────────────────
    if cli.blank_check {
        if cli.verbose {
            eprintln!("Checking blank...");
        } else {
            eprint!("Checking blank... ");
        }
        blank_check(handle)?;
        eprintln!("BLANK.");
    }

    // ── Write ─────────────────────────────────────────────────────────────────
    if let Some(ref path) = cli.write {
        // ── Batch mode ───────────────────────────────────────────────────────
        if let Some(batch_count) = cli.batch {
            if page == PageType::Config || page == PageType::Calibration {
                anyhow::bail!("batch mode is not supported for config/calibration pages");
            }

            let count = if batch_count == 0 {
                None
            } else {
                Some(batch_count)
            };

            let size_mismatch = if cli.size_ignore {
                SizeMismatch::Ignore
            } else if cli.size_warn {
                SizeMismatch::Warn
            } else {
                SizeMismatch::Error
            };

            let config = minipro_core::BatchConfig {
                path: path.clone(),
                page: proto_page,
                format: cli.format.clone(),
                size_mismatch,
                skip_blank: cli.skip_blank,
                check_device_id: false,
                erase: !cli.no_erase,
                verify: !cli.no_verify,
                count,
                unprotect_before: cli.protect_off,
                protect_after_op: cli.protect_on,
            };

            // ── Parse serial number config (if --serial-start is given) ──────
            let serial_cfg = if let Some(ref start_str) = cli.serial_start {
                let start = parse_u64(start_str)
                    .with_context(|| format!("invalid --serial-start value '{start_str}'"))?;
                let addr_str = cli.serial_addr.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("--serial-addr is required when --serial-start is given")
                })?;
                let address = parse_usize(addr_str)
                    .with_context(|| format!("invalid --serial-addr value '{addr_str}'"))?;
                let cfg = minipro_core::SerialConfig {
                    start,
                    address,
                    width: cli.serial_width,
                    format: minipro_core::SerialFormat::parse(&cli.serial_format)?,
                    endian: minipro_core::SerialEndian::parse(&cli.serial_endian)?,
                    step: cli.serial_step,
                    checksum: minipro_core::SerialChecksum::parse(&cli.serial_checksum)?,
                };
                eprintln!(
                    "Serial: start={}, addr=0x{:X}, width={}, format={:?}, step={}",
                    cfg.start, cfg.address, cfg.width, cfg.format, cfg.step
                );

                // Check for serial overflow before starting the batch
                if let Some((chip, value)) = cfg.check_overflow(count) {
                    return Err(anyhow::anyhow!(
                        "serial overflow: chip {} value 0x{:X} exceeds {}-byte max 0x{:X} — reduce count, lower start, increase width, or decrease step",
                        chip, value, cfg.width, cfg.max_value()
                    ));
                }

                Some(cfg)
            } else {
                None
            };

            let serial_cfg_ref = serial_cfg.as_ref();
            let mut on_ready = |chip_num: usize| -> bool {
                eprint!(
                    "\nInsert chip {} and press Enter (Ctrl+C to abort)... ",
                    chip_num
                );
                let mut line = String::new();
                match std::io::stdin().read_line(&mut line) {
                    Ok(0) => false, // EOF — abort
                    Ok(_) => true,
                    Err(_) => false,
                }
            };
            let mut on_patch_buffer = |chip_num: usize, buf: &mut Vec<u8>| {
                if let Some(sc) = serial_cfg_ref {
                    let value = sc.value_for_chip(chip_num);
                    match minipro_core::patch_serial(buf, sc, chip_num) {
                        Ok(()) => {
                            eprintln!(
                                "  Chip {}: serial = 0x{:0>width$X}",
                                chip_num,
                                value,
                                width = sc.width * 2
                            );
                        }
                        Err(e) => {
                            eprintln!("  Chip {}: serial patch failed: {}", chip_num, e);
                        }
                    }
                }
            };
            let mut callbacks = minipro_core::BatchCallbacks {
                on_progress: None,
                on_chip_complete: None,
                on_ready: Some(&mut on_ready),
                on_patch_buffer: if serial_cfg_ref.is_some() {
                    Some(&mut on_patch_buffer)
                } else {
                    None
                },
            };

            eprintln!(
                "Batch mode: programming {} chip(s) with {}",
                count
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "unlimited".into()),
                path.display()
            );

            let summary = minipro_core::batch_write(handle, &config, &mut callbacks)?;

            eprintln!("\n{}", "─".repeat(40));
            eprintln!(
                "Batch complete: {} programmed, {} passed, {} failed{}",
                summary.total,
                summary.passed,
                summary.failed,
                if summary.aborted { " (aborted)" } else { "" }
            );

            for r in &summary.results {
                if r.success {
                    eprintln!("  Chip {}: PASS", r.chip_number);
                } else if let Some(ref err) = r.error {
                    eprintln!("  Chip {}: FAIL — {}", r.chip_number, err);
                }
            }

            return Ok(());
        }

        if page == PageType::Config {
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("cannot read config file {:?}", path))?;
            let values = parse_fuse_file(&text)?;
            write_fuses(handle, &values)?;
            eprintln!("Config written.");
        } else if page == PageType::Calibration {
            anyhow::bail!("calibration page is read-only");
        } else {
            // ── Protect off (before erase/write) ──────────────────────────────
            // T76 + off_protect_before: auto-unprotect regardless of -u flag.
            // The firmware needs this before erase for protected parallel-NOR.
            // Non-T76: only unprotect if -u flag AND off_protect_before.
            let off_protect = handle
                .device()
                .map(|d| d.flags.off_protect_before)
                .unwrap_or(false);
            let is_t76 = handle.info.model == ProgrammerModel::T76;
            if off_protect && (is_t76 || cli.protect_off) {
                eprint!("Protect off... ");
                handle.protocol.protect_off(&handle.usb)?;
                eprintln!("OK");
                // T76 needs a transaction reset after protect_off for the
                // change to take effect before erase.
                if is_t76 {
                    let device_arc = handle
                        .device
                        .clone()
                        .expect("device is set during an active transaction");
                    handle.end_transaction()?;
                    handle.begin_transaction(device_arc)?;
                }
            }

            // Warning if chip may be write-protected and user didn't request unprotect.
            if off_protect && !cli.protect_off && !is_t76 {
                eprintln!(
                    "Note: This chip may be write-protected. Use -u to unprotect before writing."
                );
            }

            // Auto-erase before write (unless suppressed or chip doesn't
            // support electrical erase — e.g. UV EPROMs).
            if !cli.no_erase {
                let can_erase = handle.device().map(|d| d.flags.can_erase).unwrap_or(false);
                if can_erase {
                    let pb = ProgressBar::new_spinner();
                    pb.set_style(
                        ProgressStyle::with_template("{spinner} Erasing...")
                            .unwrap_or_else(|_| ProgressStyle::default_spinner()),
                    );
                    pb.enable_steady_tick(std::time::Duration::from_millis(80));
                    erase_chip(handle, false)?;
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
                false,
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
                    false,
                    Some(&mut |done, total| {
                        pb.set_length(total as u64);
                        pb.set_position(done as u64);
                    }),
                )?;
                pb.finish_and_clear();
                eprintln!("Verified OK.");
            }

            // ── Protect on (after write + verify) ─────────────────────────────
            // Only if -P flag AND protect_after flag are both set.
            let protect_after = handle
                .device()
                .map(|d| d.flags.protect_after)
                .unwrap_or(false);
            if cli.protect_on && protect_after {
                eprint!("Protect on...");
                handle.protocol.protect_on(&handle.usb)?;
                eprintln!("OK");
            } else if protect_after && !cli.protect_on {
                eprintln!("Note: Use -P if you want to write-protect this chip.");
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
            let calib = read_chip_calibration(handle)?;
            if calib.is_empty() {
                anyhow::bail!("this device does not have chip calibration data");
            }
            std::fs::write(path, &calib)
                .with_context(|| format!("cannot write calibration file {:?}", path))?;
            eprintln!(
                "Calibration bytes saved to {:?}: {}",
                path,
                calib
                    .iter()
                    .map(|b| format!("{:02x}", b))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
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
                false,
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
            false,
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
        let result = spi_autodetect_and_lookup(handle, db_paths, id_type)?;
        eprintln!("Autodetecting device (ID:0x{:04X})", result.jedec_id);
        if result.matches.is_empty() {
            if result.jedec_id == 0 {
                eprintln!("No SPI chip detected.");
            } else {
                eprintln!("No device found.");
            }
        } else {
            for item in &result.matches {
                println!("{}", item.name);
            }
            eprintln!("{} device(s) found.", result.matches.len());
        }
    }

    Ok(())
}

// ── Device info printer ───────────────────────────────────────────────────────

fn fmt_bytes(n: u32) -> String {
    if n == 0 {
        return "0 bytes".to_string();
    }
    if n % (1024 * 1024) == 0 {
        format!("{} MB ({} bytes)", n / (1024 * 1024), n)
    } else if n % 1024 == 0 {
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
/// - `vcc=V`   — VCC verify voltage (e.g. `"3.3"`); for logic ICs this sets the
///   logic-test supply voltage (valid: 1.8, 2.5, 3.3, 5)
/// - `pulse=N` — write pulse delay in microseconds (0–65535)
/// - `spi_clock=N` — SPI clock index (raw u8)
/// - `address=N`   — I²C slave address (0–255)
///
/// Voltage names are validated against the firmware encoding table for the
/// connected programmer model (see `vcc_voltage_table`/`vpp_voltage_table` in
/// minipro-core).
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

fn apply_overrides(
    device: &mut minipro_core::device::Device,
    overrides: &[String],
    model: ProgrammerModel,
) -> Result<()> {
    use minipro_core::device::{
        lookup_voltage, vcc_voltage_table, voltage_table_names, vpp_voltage_table,
    };

    for raw in overrides {
        let (key, value) = raw
            .split_once('=')
            .with_context(|| format!("invalid override '{raw}': expected KEY=VALUE"))?;
        let key = key.to_ascii_lowercase();
        match key.as_str() {
            // Voltage overrides use the per-model firmware encoding tables
            // from upstream database.c.  Logic ICs only support vcc, from the
            // 4-entry logic table (1.8/2.5/3.3/5 V).
            key @ ("vpp" | "vdd" | "vcc") => {
                let is_logic =
                    device.chip_type == minipro_core::device::ChipType::Logic as u32;
                let table = match key {
                    "vpp" => {
                        vpp_voltage_table(model, device.chip_type, device.flags.custom_protocol)
                    }
                    // vdd shares the VCC table, but logic ICs only support vcc
                    // (upstream has no vdd table for logic devices).
                    "vdd" if is_logic => None,
                    _ => vcc_voltage_table(model, device.chip_type, device.flags.custom_protocol),
                }
                .with_context(|| {
                    if is_logic {
                        format!(
                            "'{key}' is not applicable to logic ICs; only 'vcc' is supported \
                             (valid values: {})",
                            voltage_table_names(minipro_core::device::LOGIC_VCC_VOLTAGES)
                        )
                    } else {
                        format!("'{key}' override is not supported for this device on {model}")
                    }
                })?;
                let code = lookup_voltage(table, value).with_context(|| {
                    format!(
                        "invalid {key} voltage '{value}'; valid values: {}",
                        voltage_table_names(table)
                    )
                })?;
                match key {
                    "vpp" => device.voltages.vpp = code,
                    "vdd" => device.voltages.vdd = code,
                    _ => device.voltages.vcc = code,
                }
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
For the TL866II+ and T48, use the "UpdateII.dat" file.
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

#[cfg(test)]
mod tests {
    use super::*;
    use minipro_core::device::{ChipType, Device};

    fn logic_device() -> Device {
        Device {
            chip_type: ChipType::Logic as u32,
            ..Default::default()
        }
    }

    fn memory_device() -> Device {
        Device {
            chip_type: ChipType::Memory as u32,
            ..Default::default()
        }
    }

    #[test]
    fn test_logic_vcc_override_accepted() {
        // The 4-entry logic table: 1.8/2.5/3.3/5 V (firmware encodings 3/2/1/0).
        for (value, code) in [("1.8", 0x03), ("2.5", 0x02), ("3.3", 0x01), ("5", 0x00)] {
            let mut dev = logic_device();
            apply_overrides(
                &mut dev,
                &[format!("vcc={value}")],
                ProgrammerModel::Tl866iiPlus,
            )
            .unwrap();
            assert_eq!(dev.voltages.vcc, code, "vcc={value}");
        }
        // 'V' suffix and .0 forms are tolerated.
        let mut dev = logic_device();
        apply_overrides(&mut dev, &["vcc=5V".into()], ProgrammerModel::Tl866iiPlus).unwrap();
        assert_eq!(dev.voltages.vcc, 0x00);
    }

    #[test]
    fn test_logic_vcc_override_rejects_invalid() {
        // Values from the old (wrong) 16-entry table must now be rejected.
        for value in ["1.9", "2.7", "4.8", "5.3", "7.0"] {
            let mut dev = logic_device();
            let err = apply_overrides(
                &mut dev,
                &[format!("vcc={value}")],
                ProgrammerModel::Tl866iiPlus,
            )
            .unwrap_err();
            assert!(
                err.to_string().contains("1.8, 2.5, 3.3, 5"),
                "error should list valid logic voltages: {err:#}"
            );
        }
    }

    #[test]
    fn test_logic_vpp_vdd_rejected() {
        for key in ["vpp", "vdd"] {
            let mut dev = logic_device();
            let err = apply_overrides(
                &mut dev,
                &[format!("{key}=12")],
                ProgrammerModel::Tl866iiPlus,
            )
            .unwrap_err();
            assert!(
                err.to_string().contains("not applicable to logic ICs"),
                "{key}: {err:#}"
            );
        }
    }

    #[test]
    fn test_tl866iiplus_memory_voltage_encoding() {
        // Firmware-encoded values, not sequential indices.
        let mut dev = memory_device();
        apply_overrides(
            &mut dev,
            &["vcc=6.5".into(), "vdd=3.3".into(), "vpp=12.5".into()],
            ProgrammerModel::Tl866iiPlus,
        )
        .unwrap();
        assert_eq!(dev.voltages.vcc, 0x05);
        assert_eq!(dev.voltages.vdd, 0x01);
        assert_eq!(dev.voltages.vpp, 0x60);

        // 7.0 V does not exist on the TL866II+.
        let mut dev = memory_device();
        assert!(
            apply_overrides(&mut dev, &["vcc=7.0".into()], ProgrammerModel::Tl866iiPlus).is_err()
        );
    }

    #[test]
    fn test_t48_memory_voltage_encoding() {
        let mut dev = memory_device();
        apply_overrides(&mut dev, &["vcc=1.2".into()], ProgrammerModel::T48).unwrap();
        assert_eq!(dev.voltages.vcc, 0x09);
    }

    #[test]
    fn test_non_voltage_overrides_unchanged() {
        let mut dev = memory_device();
        apply_overrides(
            &mut dev,
            &[
                "pulse=500".into(),
                "spi_clock=2".into(),
                "address=0xA0".into(),
            ],
            ProgrammerModel::Tl866iiPlus,
        )
        .unwrap();
        assert_eq!(dev.pulse_delay, 500);
        assert_eq!(dev.spi_clock, 2);
        assert_eq!(dev.i2c_address, 0xA0);
    }
}
