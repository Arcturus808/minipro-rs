// This file is shared between build.rs (for shell completion generation) and
// main.rs (at runtime) via `include!`.  Keep it free of `use` statements — the
// including file is responsible for bringing the necessary items into scope:
//   use std::path::PathBuf;
//   use clap::{ArgAction, Parser};

#[derive(Debug, Parser)]
#[command(
    name    = "minipro",
    version,
    about   = "Open-source programmer for XGecu TL866xx/T48/T56/T76 chip programmers",
    long_about = "Open-source programmer for XGecu TL866xx/T48/T56/T76 chip programmers.\n\nFull documentation: https://arcturus8081.gitlab.io/minipro-rs/\nManual page:        https://arcturus8081.gitlab.io/minipro-rs/minipro.1.html",
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
    #[arg(short = 'E', long = "erase", action = ArgAction::SetTrue)]
    erase: bool,

    /// Blank-check device
    #[arg(short = 'b', long = "blank-check", action = ArgAction::SetTrue)]
    blank_check: bool,

    /// Read chip ID from the inserted device.
    /// Requires -p to select the device so the programmer knows how to
    /// configure the socket before issuing the ID read sequence.
    #[arg(short = 'D', long = "device-id", action = ArgAction::SetTrue)]
    device_id: bool,

    /// List supported devices (optional filter substring)
    #[arg(short = 'l', long = "list", value_name = "FILTER")]
    list: Option<Option<String>>,

    /// Restrict -l/--list to devices supported by this programmer model
    /// (TL866A, TL866CS, TL866II, T48, T56, T76)
    #[arg(short = 'q', long = "programmer", value_name = "MODEL")]
    programmer: Option<String>,

    /// Show device information from the database (no programmer needed)
    #[arg(short = 'd', long = "get-info", value_name = "DEVICE")]
    get_info: Option<String>,

    /// Show programmer info and exit
    #[arg(long = "info", action = ArgAction::SetTrue)]
    info: bool,

    /// Check if a programmer is connected and print model + firmware version
    #[arg(short = 'k', long = "presence-check", action = ArgAction::SetTrue)]
    presence_check: bool,

    /// Skip over-current check
    #[arg(long = "no-ovc-check", action = ArgAction::SetTrue)]
    no_ovc_check: bool,

    /// Enable ICSP with VCC (in-circuit serial programming)
    #[arg(short = 'i', long = "icsp", action = ArgAction::SetTrue)]
    icsp: bool,

    /// Enable ICSP without VCC (in-circuit serial programming)
    #[arg(short = 'I', long = "icsp-no-vcc", action = ArgAction::SetTrue)]
    icsp_no_vcc: bool,

    /// Override path to infoic.xml chip database
    #[arg(long = "infoic-path", value_name = "PATH")]
    infoic_path: Option<PathBuf>,

    /// Override path to logicic.xml logic IC database
    #[arg(long = "logicic-path", value_name = "PATH")]
    logicic_path: Option<PathBuf>,

    /// Override path to algorithms.xml FPGA bitstream database (T56/T76)
    #[arg(long = "algorithms", value_name = "PATH")]
    algorithms_path: Option<PathBuf>,

    /// Memory page: code (default), data, config, user, calibration, or 0-4
    #[arg(short = 'c', long = "page", default_value = "code", value_name = "PAGE")]
    page: String,

    /// Select the fuses/config-bits page (equivalent to -c config)
    #[arg(long = "fuses", action = ArgAction::SetTrue)]
    fuses: bool,

    /// Select the user-ID / user-row page (equivalent to -c user)
    #[arg(long = "uid", action = ArgAction::SetTrue)]
    uid: bool,

    /// Select the lock-bits page (equivalent to -c config; lock bits are included in fuse output)
    #[arg(long = "lock", action = ArgAction::SetTrue)]
    lock: bool,

    /// Verbose output
    #[arg(long = "verbose", action = ArgAction::SetTrue)]
    verbose: bool,

    /// Do NOT erase device before write (matches upstream -e / --skip_erase)
    #[arg(short = 'e', long = "skip-erase", alias = "no-erase", action = ArgAction::SetTrue)]
    no_erase: bool,

    /// Do NOT verify after write (matches upstream -v / --skip_verify)
    #[arg(short = 'v', long = "skip-verify", alias = "no-verify", action = ArgAction::SetTrue)]
    no_verify: bool,

    /// Disable write protection before operation
    #[arg(short = 'u', long = "protect-off", alias = "unprotect", action = ArgAction::SetTrue)]
    protect_off: bool,

    /// Enable write protection after operation
    #[arg(short = 'P', long = "protect-on", alias = "protect", action = ArgAction::SetTrue)]
    protect_on: bool,

    /// File format override: auto | bin | ihex | srec | jedec  [default: auto]
    #[arg(short = 'f', long = "format", default_value = "auto", value_name = "FORMAT")]
    format: String,

    /// Skip chip ID verification
    #[arg(short = 'x', long = "skip-id", alias = "skip_id", action = ArgAction::SetTrue)]
    skip_id: bool,

    /// Warn but continue on chip ID mismatch
    #[arg(short = 'y', long = "continue-id", alias = "no_id_error", action = ArgAction::SetTrue)]
    continue_id: bool,

    /// Test a logic IC against its built-in test vectors
    #[arg(short = 'T', long = "logic-test", alias = "logic_test", action = ArgAction::SetTrue)]
    logic_test: bool,

    /// Write logic IC test vector results to FILE instead of stdout
    #[arg(long = "logicic-out", alias = "logicic_out", value_name = "FILE")]
    logicic_out: Option<PathBuf>,

    /// Update programmer firmware from binary file (UpdateII.dat / updateT76.dat)
    #[arg(short = 'F', long = "firmware-update", alias = "update", value_name = "FILE")]
    firmware_update: Option<PathBuf>,

    /// Read fuse/configuration bits and print them to stdout (or write to FILE)
    #[arg(long = "read-fuses", value_name = "FILE")]
    read_fuses: Option<Option<PathBuf>>,

    /// Write fuse/configuration bits from a key=value text file
    #[arg(long = "write-fuses", value_name = "FILE")]
    write_fuses: Option<PathBuf>,

    /// Auto-detect SPI flash chip JEDEC ID (0 = 8-pin, 1 = 16-pin) [default: 0]
    #[arg(short = 'a', long = "spi-autodetect", alias = "auto_detect", value_name = "TYPE")]
    spi_autodetect: Option<Option<u8>>,

    /// Generate shell completions and print to stdout (bash|zsh|fish|powershell)
    #[arg(long = "generate-completions", value_name = "SHELL", hide = true)]
    generate_completions: Option<String>,

    /// Print the man page in groff format to stdout
    #[arg(long = "generate-man", hide = true)]
    generate_man: bool,

    /// Run the programmer's built-in hardware self-test
    #[arg(short = 't', long = "hardware-check", action = ArgAction::SetTrue)]
    hardware_check: bool,

    /// Test pin contact of the chip in the ZIF socket
    #[arg(short = 'z', long = "pin-check", action = ArgAction::SetTrue)]
    pin_check: bool,

    /// Warn (but continue) if input file size doesn't match device size (binary only)
    #[arg(short = 's', long = "size-error", action = ArgAction::SetTrue)]
    size_warn: bool,

    /// Silently ignore file size mismatch (binary only)
    #[arg(short = 'S', long = "no-size-error", action = ArgAction::SetTrue)]
    size_ignore: bool,

    /// List supported programmer models and exit
    #[arg(short = 'Q', long = "query-supported", action = ArgAction::SetTrue)]
    query_supported: bool,

    /// Override a device parameter: KEY=VALUE.
    /// Supported keys: vpp=<V>, vdd=<V>, vcc=<V>, pulse=<us>, spi_clock=<N>, address=<N>.
    /// May be repeated for multiple overrides.
    #[arg(short = 'o', long = "override", value_name = "KEY=VALUE", action = ArgAction::Append)]
    overrides: Vec<String>,

    /// Set the programming voltage (VPP). Equivalent to -o vpp=<V>.
    #[arg(long = "vpp", value_name = "V")]
    vpp: Option<String>,

    /// Set the VCC verify voltage. Equivalent to -o vcc=<V>.
    #[arg(long = "vcc", value_name = "V")]
    vcc: Option<String>,

    /// Set the VDD write voltage. Equivalent to -o vdd=<V>.
    #[arg(long = "vdd", value_name = "V")]
    vdd: Option<String>,

    /// Set the programming pulse delay in microseconds. Equivalent to -o pulse=<us>.
    #[arg(long = "pulse", value_name = "US")]
    pulse: Option<String>,

    /// Set the SPI clock frequency in MHz. Equivalent to -o spi_clock=<N>.
    #[arg(long = "spi_clock", alias = "spi-clock", value_name = "N")]
    spi_clock: Option<String>,

    /// Set the I2C slave address (T76 only), e.g. 0xA0. Equivalent to -o address=<hex>.
    #[arg(long = "address", value_name = "HEX")]
    address: Option<String>,
}
