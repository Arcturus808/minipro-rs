//! `minipro-core` — core library for the minipro-rs chip programmer.

pub mod database;
pub mod device;
pub mod error;
pub mod format;
pub mod handle;
pub mod operations;
pub mod protocol;
pub mod usb;

// Re-export the most commonly used types so callers can write
// `use minipro_core::MiniproHandle` etc.
pub use database::{DatabasePaths, find_device, list_devices};
pub use device::{
    Algorithm, ChipConfig, ChipType, DataOrg, Device, DeviceFlags, Endianness,
    FuseConfig, FuseField, FuseType, GalConfig, PackageDetails, ProgrammerInfo,
    ProgrammerModel, ProgrammerStatus, Voltages,
};
pub use error::{MiniproError, Result};
pub use handle::MiniproHandle;
pub use operations::{FuseValue, read_fuses, write_fuses};
