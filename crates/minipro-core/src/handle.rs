//! `MiniproHandle` — top-level programmer session.
//!
//! Opens the USB device, reads firmware info, selects the right protocol
//! implementation, and exposes high-level methods used by `operations.rs`.

use std::sync::Arc;

use crate::{
    database::DatabasePaths,
    device::{Device, ProgrammerInfo, ProgrammerModel, ProgrammerStatus},
    error::{MiniproError, Result},
    protocol::{
        Protocol,
        t48::T48Protocol,
        t56::T56Protocol,
        t76::T76Protocol,
        tl866a::Tl866aProtocol,
        tl866iiplus::{Tl866iiPlusProtocol, get_system_info},
    },
    usb::{UsbDevice, open_programmer},
};

pub struct MiniproHandle {
    pub usb:      UsbDevice,
    pub info:     ProgrammerInfo,
    pub device:   Option<Arc<Device>>,
    pub protocol: Box<dyn Protocol>,
    pub db_paths: Option<DatabasePaths>,
    /// Whether ICSP mode is active.
    pub icsp:     bool,
}

impl MiniproHandle {
    /// Open the first connected programmer and read firmware info.
    pub fn open() -> Result<Self> {
        let (usb, _initial_model) = open_programmer()?;

        // Query the firmware for the authoritative model/version
        let sys_info = get_system_info(&usb)?;

        if sys_info.status == ProgrammerStatus::Bootloader {
            return Err(MiniproError::BootloaderMode);
        }

        let info = ProgrammerInfo {
            model:            sys_info.model,
            status:           sys_info.status,
            firmware:         sys_info.firmware,
            firmware_str:     sys_info.firmware_str,
            device_code:      sys_info.device_code,
            serial_number:    sys_info.serial_number,
            hardware_version: sys_info.hardware_version,
        };

        let protocol: Box<dyn Protocol> = match info.model {
            ProgrammerModel::Tl866a | ProgrammerModel::Tl866cs =>
                Box::new(Tl866aProtocol::new()),
            ProgrammerModel::T48 =>
                Box::new(T48Protocol::new()),
            ProgrammerModel::T56 =>
                Box::new(T56Protocol::new()),
            ProgrammerModel::T76 =>
                Box::new(T76Protocol::new()),
            _ =>
                Box::new(Tl866iiPlusProtocol::new()),
        };

        Ok(Self {
            usb,
            info,
            device: None,
            protocol,
            db_paths: None,
            icsp: false,
        })
    }

    /// Set the active chip device and send `begin_transaction` to the hardware.
    pub fn begin_transaction(&mut self, device: Arc<Device>) -> Result<()> {
        self.protocol.begin_transaction(&self.usb, &device)?;
        self.device = Some(device);
        Ok(())
    }

    /// Send `end_transaction` and clear the active device.
    pub fn end_transaction(&mut self) -> Result<()> {
        self.protocol.end_transaction(&self.usb)?;
        self.device = None;
        Ok(())
    }

    /// Return a reference to the active device, or error if none is set.
    pub fn device(&self) -> Result<&Device> {
        self.device
            .as_deref()
            .ok_or_else(|| MiniproError::Protocol("no device selected".into()))
    }

    /// Display programmer info in the format expected by `minipro -I`.
    pub fn print_info(&self) {
        let m = &self.info;
        println!("Model: {:?}", m.model);
        println!("Device code: {}", m.device_code);
        println!("Serial number: {}", m.serial_number);
        println!("Firmware: {}", m.firmware_str);
        println!("Hardware version: {:02x}", m.hardware_version);
    }
}
