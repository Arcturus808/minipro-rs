//! `minipro-core` — core library for the minipro-rs chip programmer.
//!
//! # Architecture
//!
//! ```text
//! MiniproHandle
//!   ├── UsbDevice          (nusb abstraction — src/usb.rs)
//!   ├── Box<dyn Protocol>  (model-specific commands — src/protocol/)
//!   └── Arc<Device>        (chip descriptor from XML database — src/database.rs)
//! ```
//!
//! # Quick start (library users)
//!
//! ```no_run
//! use minipro_core::{MiniproHandle, DatabasePaths, find_device};
//! use minipro_core::device::ProgrammerModel;
//! use minipro_core::operations::read_chip;
//!
//! let db = DatabasePaths::resolve(None, None, None)?;
//! let device = std::sync::Arc::new(find_device(&db, "AT28C256", ProgrammerModel::Tl866iiPlus)?);
//! let mut handle = MiniproHandle::open()?;
//! handle.begin_transaction(device)?;
//! read_chip(&mut handle, std::path::Path::new("dump.bin"), 0, "auto", None)?;
//! handle.end_transaction()?;
//! # Ok::<(), minipro_core::error::MiniproError>(())
//! ```
//!
//! # Tauri integration
//!
//! Wrap USB calls in `tokio::task::spawn_blocking` to avoid blocking Tauri's
//! async executor.  See the project README for a worked example.

pub mod algorithm;
pub mod batch;
pub mod database;
pub mod device;
pub mod diff;
pub mod error;
pub mod format;
pub mod handle;
pub mod health;
pub mod icsp;
pub mod operations;
pub mod prefs;
pub mod protocol;
pub mod usb;
pub(crate) mod util;
pub mod xgpro_fetch;
pub mod zif;

/// Route `outln!` diagnostics to `log::info!` instead of stderr.
///
/// Several protocol paths print progress/diagnostic lines with
/// `eprintln!` (algorithm names, pin-test progress, overcurrent
/// results). A frontend that owns the terminal — the TUI — must not let
/// library code write to stderr mid-frame, or the raw text corrupts the
/// rendered screen. Setting this flag makes every `outln!` emit a
/// `log::info!` record instead, which the frontend's logger can route
/// into its own UI. The CLI and GUI leave it unset and keep the
/// upstream-style stderr output.
pub static PRINT_TO_LOG: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Print a diagnostic line to stderr, or to `log::info!` when
/// [`PRINT_TO_LOG`] is set. Use instead of `eprintln!` anywhere a
/// frontend might own the terminal.
#[macro_export]
macro_rules! outln {
    () => {
        if $crate::PRINT_TO_LOG.load(::std::sync::atomic::Ordering::Relaxed) {
            ::log::info!("");
        } else {
            ::std::eprintln!();
        }
    };
    ($($arg:tt)*) => {
        if $crate::PRINT_TO_LOG.load(::std::sync::atomic::Ordering::Relaxed) {
            ::log::info!($($arg)*);
        } else {
            ::std::eprintln!($($arg)*);
        }
    };
}

// Re-export the most commonly used types so callers can write
// `use minipro_core::MiniproHandle` etc.
pub use batch::{
    batch_write, patch_serial, BatchCallbacks, BatchChipResult, BatchConfig, BatchSummary,
    SerialChecksum, SerialConfig, SerialEndian, SerialFormat,
};
pub use database::{
    find_device, find_device_any, find_devices_by_chip_id, list_devices, list_devices_for_model,
    list_logic_ics_by_pin_count, user_data_dir, DatabasePaths,
};
pub use device::{
    Algorithm, ChipConfig, ChipType, DataOrg, Device, DeviceFlags, Endianness, FuseConfig,
    FuseField, FuseType, GalConfig, PackageDetails, ProgrammerInfo, ProgrammerModel,
    ProgrammerStatus, Voltages,
};
pub use diff::{
    format_diff_report, smart_diff, DiffEntry, DiffResult, DiffSummary, TailKind, TailRegion,
};
pub use error::{MiniproError, Result};
pub use handle::MiniproHandle;
pub use health::{HealthReport, HealthVerdict, RegionStat};
pub use icsp::{icsp_wiring, IcspWire, IcspWiring};
pub use operations::{
    chip_id_info, firmware_update, hardware_check, health_check, logic_auto_find, logic_ic_test,
    pin_check_gate_applies, pin_contact_check, read_fuses, spi_autodetect,
    spi_autodetect_and_lookup, spi_status_read, spi_status_write, write_fuses, AutoFindEntry,
    ChipIdInfo, FuseValue, OpStats, SizeMismatch, SpiAutodetectResult, SPI_SR1_BIT_NAMES,
    SPI_SR1_READ_ONLY,
};
pub use protocol::{LogicTestResult, PinTestResult};
