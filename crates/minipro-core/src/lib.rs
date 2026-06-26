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
//! read_chip(&mut handle, std::path::Path::new("dump.bin"), 0, "auto", true, None)?;
//! handle.end_transaction()?;
//! # Ok::<(), minipro_core::error::MiniproError>(())
//! ```
//!
//! # Tauri integration
//!
//! Wrap USB calls in `tokio::task::spawn_blocking` to avoid blocking Tauri's
//! async executor.  See the project README for a worked example.

pub mod batch;
pub mod database;
pub mod device;
pub mod diff;
pub mod error;
pub mod format;
pub mod handle;
pub mod operations;
pub mod protocol;
pub mod usb;

// Re-export the most commonly used types so callers can write
// `use minipro_core::MiniproHandle` etc.
pub use batch::{batch_write, BatchCallbacks, BatchChipResult, BatchConfig, BatchSummary};
pub use database::{
    find_device, find_device_any, list_devices, list_devices_for_model, DatabasePaths,
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
pub use operations::{
    firmware_update, hardware_check, logic_ic_test, pin_contact_check, read_fuses, spi_autodetect,
    write_fuses, FuseValue, OpStats, SizeMismatch,
};
