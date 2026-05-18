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
    #[arg(short = 'e', long = "erase", action = ArgAction::SetTrue)]
    erase: bool,

    /// Blank-check device
    #[arg(short = 'b', long = "blank-check", action = ArgAction::SetTrue)]
    blank_check: bool,

    /// Read chip ID
    #[arg(short = 'D', long = "device-id", action = ArgAction::SetTrue)]
    device_id: bool,

    /// List supported devices (optional filter substring)
    #[arg(short = 'l', long = "list", value_name = "FILTER")]
    list: Option<Option<String>>,

    /// Show programmer info and exit
    #[arg(short = 'I', long = "info", action = ArgAction::SetTrue)]
    info: bool,

    /// Check if a programmer is connected and print model + firmware version
    #[arg(short = 'k', long = "presence-check", action = ArgAction::SetTrue)]
    presence_check: bool,

    /// Skip over-current check
    #[arg(long = "no-ovc-check", action = ArgAction::SetTrue)]
    no_ovc_check: bool,

    /// Enable ICSP (in-circuit serial programming) mode
    #[arg(long = "icsp", action = ArgAction::SetTrue)]
    icsp: bool,

    /// Override path to infoic.xml chip database
    #[arg(long = "infoic-path", value_name = "PATH")]
    infoic_path: Option<PathBuf>,

    /// Override path to logicic.xml logic IC database
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

    /// File format override: auto | bin | ihex | srec | jedec  [default: auto]
    #[arg(long = "format", default_value = "auto", value_name = "FORMAT")]
    format: String,

    /// Skip chip ID verification
    #[arg(long = "skip-id", action = ArgAction::SetTrue)]
    skip_id: bool,

    /// Warn but continue on chip ID mismatch
    #[arg(long = "continue-id", action = ArgAction::SetTrue)]
    continue_id: bool,

    /// Test a logic IC against its built-in test vectors
    #[arg(long = "logic-test", action = ArgAction::SetTrue)]
    logic_test: bool,

    /// Update programmer firmware from binary file (UpdateII.dat / updateT76.dat)
    #[arg(long = "firmware-update", value_name = "FILE")]
    firmware_update: Option<PathBuf>,

    /// Read fuse/configuration bits and print them to stdout (or write to FILE)
    #[arg(long = "read-fuses", value_name = "FILE")]
    read_fuses: Option<Option<PathBuf>>,

    /// Write fuse/configuration bits from a key=value text file
    #[arg(long = "write-fuses", value_name = "FILE")]
    write_fuses: Option<PathBuf>,

    /// Auto-detect SPI flash chip JEDEC ID (0 = 8-pin, 1 = 16-pin) [default: 0]
    #[arg(long = "spi-autodetect", value_name = "TYPE")]
    spi_autodetect: Option<Option<u8>>,

    /// Generate shell completions and print to stdout (bash|zsh|fish|powershell)
    #[arg(long = "generate-completions", value_name = "SHELL", hide = true)]
    generate_completions: Option<String>,

    /// Print the man page in groff format to stdout
    #[arg(long = "generate-man", hide = true)]
    generate_man: bool,

    /// List supported programmer models and exit
    #[arg(short = 'Q', long = "query-supported", action = ArgAction::SetTrue)]
    query_supported: bool,
}
