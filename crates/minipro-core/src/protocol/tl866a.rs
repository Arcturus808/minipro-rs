//! TL866A/CS protocol stub.
//!
//! The TL866A uses a different command set from the TL866II+.
//! Full implementation is deferred to Phase 2.

use crate::{
    device::Device,
    error::{MiniproError, Result},
    usb::UsbDevice,
};
use super::{DataSet, OvcStatus, Protocol};

pub struct Tl866aProtocol;

impl Tl866aProtocol {
    pub fn new() -> Self { Self }
}

impl Default for Tl866aProtocol {
    fn default() -> Self { Self::new() }
}

impl Protocol for Tl866aProtocol {
    fn begin_transaction(&self, _usb: &UsbDevice, _device: &Device) -> Result<()> {
        Err(MiniproError::UnsupportedOperation)
    }
    fn end_transaction(&self, _usb: &UsbDevice) -> Result<()> {
        Err(MiniproError::UnsupportedOperation)
    }
    fn read_block(&self, _usb: &UsbDevice, _ds: &mut DataSet) -> Result<()> {
        Err(MiniproError::UnsupportedOperation)
    }
    fn write_block(&self, _usb: &UsbDevice, _ds: &DataSet) -> Result<()> {
        Err(MiniproError::UnsupportedOperation)
    }
    fn get_chip_id(&self, _usb: &UsbDevice) -> Result<(u8, u32)> {
        Err(MiniproError::UnsupportedOperation)
    }
    fn read_fuses(&self, _usb: &UsbDevice, _fuse_type: u8, _length: usize, _items_count: u8) -> Result<Vec<u8>> {
        Err(MiniproError::UnsupportedOperation)
    }
    fn write_fuses(&self, _usb: &UsbDevice, _fuse_type: u8, _length: usize, _items_count: u8, _data: &[u8]) -> Result<()> {
        Err(MiniproError::UnsupportedOperation)
    }
    fn erase(&self, _usb: &UsbDevice, _num_fuses: u8, _is_pld: bool) -> Result<()> {
        Err(MiniproError::UnsupportedOperation)
    }
    fn protect_off(&self, _usb: &UsbDevice) -> Result<()> {
        Err(MiniproError::UnsupportedOperation)
    }
    fn protect_on(&self, _usb: &UsbDevice) -> Result<()> {
        Err(MiniproError::UnsupportedOperation)
    }
    fn get_ovc_status(&self, _usb: &UsbDevice) -> Result<(OvcStatus, u8)> {
        Err(MiniproError::UnsupportedOperation)
    }
    fn hardware_check(&self, _usb: &UsbDevice) -> Result<()> {
        Err(MiniproError::UnsupportedOperation)
    }
    fn firmware_update(&self, _usb: &UsbDevice, _firmware: &[u8]) -> Result<()> {
        Err(MiniproError::UnsupportedOperation)
    }
    fn reset_state(&self, _usb: &UsbDevice) -> Result<()> {
        Err(MiniproError::UnsupportedOperation)
    }
}
