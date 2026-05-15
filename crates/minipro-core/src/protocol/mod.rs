//! Protocol dispatch trait.
//!
//! Each programmer model implements this trait.  `MiniproHandle` holds a
//! `Box<dyn Protocol>` selected at open time based on the firmware response.

use crate::{
    device::{Device, FuseType},
    error::Result,
    usb::UsbDevice,
};

/// Data buffer + addressing info for a single block read/write.
#[derive(Debug)]
pub struct DataSet {
    pub data:         Vec<u8>,
    pub address:      u32,
    /// Current block count (size / 64, used per-block).
    pub block_count:  u32,
    /// Memory page type (MP_CODE / MP_DATA / MP_USER).
    pub page_type:    u8,
    /// True on the very first block in the sequence (T76 uses this to send
    /// the DMA-initialisation header before streaming via the payload EP).
    pub init:         bool,
    /// Total number of blocks in the whole read/write operation (T76 only).
    pub total_blocks: u32,
}

/// JEDEC fuse-map row transfer.
#[derive(Debug)]
pub struct JedecSet {
    pub data:      Vec<u8>,
    pub row:       u8,
    pub flags:     u8,
    pub set_type:  u8,
}

/// Over-current status returned by the programmer.
#[derive(Debug, Clone)]
pub struct OvcStatus {
    pub error:   u8,
    pub address: u32,
    pub c1:      u32,
    pub c2:      u32,
}

/// All operations a programmer model must implement.
///
/// Methods return `Ok(())` on success or an appropriate `MiniproError`.
/// Unimplemented optional features should return `Err(MiniproError::UnsupportedOperation)`.
pub trait Protocol: Send + Sync {
    /// Send the begin-transaction command (sets up chip parameters).
    fn begin_transaction(&self, usb: &UsbDevice, device: &Device) -> Result<()>;

    /// Send the end-transaction command.
    fn end_transaction(&self, usb: &UsbDevice) -> Result<()>;

    /// Read one block of memory into `ds.data`.
    fn read_block(&self, usb: &UsbDevice, ds: &mut DataSet) -> Result<()>;

    /// Write `ds.data` to the chip starting at `ds.address`.
    fn write_block(&self, usb: &UsbDevice, ds: &DataSet) -> Result<()>;

    /// Read the chip identification bytes.
    /// Returns `(id_type, chip_id)`.
    fn get_chip_id(&self, usb: &UsbDevice) -> Result<(u8, u32)>;

    /// Auto-detect an SPI chip by its JEDEC ID.
    fn spi_autodetect(&self, usb: &UsbDevice, id_type: u8) -> Result<u32> {
        let _ = (usb, id_type);
        Err(crate::error::MiniproError::UnsupportedOperation)
    }

    /// Read fuse / lock bytes.
    fn read_fuses(
        &self, usb: &UsbDevice, device: &Device, fuse_type: u8, length: usize,
        items_count: u8,
    ) -> Result<Vec<u8>>;

    /// Write fuse / lock bytes.
    fn write_fuses(
        &self, usb: &UsbDevice, device: &Device, fuse_type: u8, length: usize,
        items_count: u8, data: &[u8],
    ) -> Result<()>;

    /// Read RC calibration byte.
    fn read_calibration(&self, usb: &UsbDevice, size: usize) -> Result<Vec<u8>> {
        let _ = (usb, size);
        Err(crate::error::MiniproError::UnsupportedOperation)
    }

    /// Erase the chip.
    fn erase(&self, usb: &UsbDevice, num_fuses: u8, is_pld: bool) -> Result<()>;

    /// Read a JEDEC fuse-map row.
    fn read_jedec_row(&self, usb: &UsbDevice, js: &mut JedecSet) -> Result<()> {
        let _ = (usb, js);
        Err(crate::error::MiniproError::UnsupportedOperation)
    }

    /// Write a JEDEC fuse-map row.
    fn write_jedec_row(&self, usb: &UsbDevice, js: &JedecSet) -> Result<()> {
        let _ = (usb, js);
        Err(crate::error::MiniproError::UnsupportedOperation)
    }

    /// Disable write protection.
    fn protect_off(&self, usb: &UsbDevice) -> Result<()>;

    /// Enable write protection.
    fn protect_on(&self, usb: &UsbDevice) -> Result<()>;

    /// Query over-current status. Returns `(ovc_status, ovc_flag)`.
    fn get_ovc_status(&self, usb: &UsbDevice) -> Result<(OvcStatus, u8)>;

    /// Unlock TSOP48 adapter.
    fn unlock_tsop48(&self, usb: &UsbDevice) -> Result<u8> {
        let _ = usb;
        Err(crate::error::MiniproError::UnsupportedOperation)
    }

    /// Run the programmer's built-in hardware self-test.
    fn hardware_check(&self, usb: &UsbDevice) -> Result<()>;

    /// Update programmer firmware from a binary image.
    fn firmware_update(&self, usb: &UsbDevice, firmware: &[u8]) -> Result<()>;

    /// Test a logic IC against its test vectors.
    fn logic_ic_test(&self, usb: &UsbDevice) -> Result<()> {
        let _ = usb;
        Err(crate::error::MiniproError::UnsupportedOperation)
    }

    /// Reset ZIF socket pin state.
    fn reset_state(&self, usb: &UsbDevice) -> Result<()>;

    /// Set ZIF pin directions.
    fn set_zif_direction(&self, usb: &UsbDevice, directions: &[u8]) -> Result<()> {
        let _ = (usb, directions);
        Err(crate::error::MiniproError::UnsupportedOperation)
    }

    /// Set ZIF pin output states.
    fn set_zif_state(&self, usb: &UsbDevice, states: &[u8]) -> Result<()> {
        let _ = (usb, states);
        Err(crate::error::MiniproError::UnsupportedOperation)
    }

    /// Read ZIF pin input states.
    fn get_zif_state(&self, usb: &UsbDevice) -> Result<Vec<u8>> {
        let _ = usb;
        Err(crate::error::MiniproError::UnsupportedOperation)
    }

    /// Set VCC / VPP voltages.
    fn set_voltages(&self, usb: &UsbDevice, vcc: u8, vpp: u8) -> Result<()> {
        let _ = (usb, vcc, vpp);
        Err(crate::error::MiniproError::UnsupportedOperation)
    }
}

pub mod tl866a;
pub mod tl866iiplus;
pub mod t48;
pub mod t56;
pub mod t76;
