//! `MiniproHandle` — top-level programmer session.
//!
//! Opens the USB device, reads firmware info, selects the right protocol
//! implementation, and exposes high-level methods used by `operations.rs`.

use std::sync::Arc;

use log::info;

use crate::{
    database::DatabasePaths,
    device::{Device, ProgrammerInfo, ProgrammerModel, ProgrammerStatus},
    error::{MiniproError, Result},
    protocol::{
        t48::T48Protocol,
        t56::{T56Protocol, MIN_FIRMWARE_T56},
        t76::{T76Protocol, MIN_FIRMWARE_T76},
        tl866a::{self, Tl866aProtocol},
        tl866iiplus::{get_system_info, Tl866iiPlusProtocol, MIN_FIRMWARE},
        Protocol,
    },
    usb::{open_programmer, wait_for_disconnect, wait_for_reconnect, UsbDevice},
};

/// Top-level programmer session.
///
/// Obtained via [`MiniproHandle::open`].  Holds the USB connection, detected
/// programmer info, the active chip descriptor, and the model-specific
/// [`Protocol`] implementation.
pub struct MiniproHandle {
    /// Raw USB device handle.
    pub usb: UsbDevice,
    /// Detected programmer hardware info (model, firmware version, serial).
    pub info: ProgrammerInfo,
    /// Active chip descriptor, set by [`MiniproHandle::begin_transaction`].
    pub device: Option<Arc<Device>>,
    /// Model-specific protocol implementation.
    pub protocol: Box<dyn Protocol>,
    /// Database paths for XML chip database resolution.
    pub db_paths: Option<DatabasePaths>,
    /// Whether ICSP (in-circuit serial programming) mode is active.
    pub icsp: bool,
}

impl MiniproHandle {
    /// Open the first connected programmer and read firmware info.
    pub fn open() -> Result<Self> {
        let (usb, initial_model) = open_programmer()?;

        // Query the firmware for the authoritative model/version.
        // TL866A/CS uses a 40-byte response layout; all others use 41 bytes.
        let sys_info = if initial_model == ProgrammerModel::Tl866a {
            tl866a::get_system_info(&usb)?
        } else {
            get_system_info(&usb)?
        };

        if sys_info.status == ProgrammerStatus::Bootloader {
            return Err(MiniproError::BootloaderMode);
        }

        let info = ProgrammerInfo {
            model: sys_info.model,
            status: sys_info.status,
            firmware: sys_info.firmware,
            firmware_str: sys_info.firmware_str,
            device_code: sys_info.device_code,
            serial_number: sys_info.serial_number,
            hardware_version: sys_info.hardware_version,
        };

        // Firmware version check — each model has a different minimum.
        let min_fw = match info.model {
            ProgrammerModel::T56 => MIN_FIRMWARE_T56,
            ProgrammerModel::T76 => MIN_FIRMWARE_T76,
            ProgrammerModel::Tl866iiPlus | ProgrammerModel::T48 => MIN_FIRMWARE,
            _ => 0,
        };
        if min_fw > 0 && info.firmware < min_fw {
            return Err(MiniproError::FirmwareTooOld {
                got: info.firmware,
                need: min_fw,
            });
        }

        info!(
            "Programmer: {:?} firmware {}",
            info.model, info.firmware_str
        );

        let protocol: Box<dyn Protocol> = match info.model {
            ProgrammerModel::Tl866a | ProgrammerModel::Tl866cs => Box::new(Tl866aProtocol::new()),
            ProgrammerModel::T48 => Box::new(T48Protocol::new()),
            ProgrammerModel::T56 => Box::new(T56Protocol::new()),
            ProgrammerModel::T76 => Box::new(T76Protocol::new()),
            _ => Box::new(Tl866iiPlusProtocol::new()),
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
        info!("Device: {}", device.name);

        // T56/T76: look up the FPGA algorithm bitstream if not already
        // populated and algorithm.xml is available.  The protocol layer
        // checks `device.algorithm` during begin_transaction to upload the
        // bitstream.  Without this, FPGA-based operations fail silently.
        let device = if device.algorithm.is_none()
            && crate::algorithm::needs_algorithm(&device, self.info.model)
        {
            if let Some(ref paths) = self.db_paths {
                if let Some(ref algo_path) = paths.algorithms {
                    match crate::algorithm::get_algorithm(
                        &device,
                        self.info.model,
                        self.icsp,
                        crate::algorithm::V_3V3,
                        algo_path,
                    ) {
                        Ok(algo) => {
                            let mut d = (*device).clone();
                            d.algorithm = Some(algo);
                            Arc::new(d)
                        }
                        Err(e) => {
                            log::warn!("Algorithm lookup failed for {}: {}", device.name, e);
                            device
                        }
                    }
                } else {
                    device
                }
            } else {
                device
            }
        } else {
            device
        };

        self.protocol
            .begin_transaction(&self.usb, &device, self.icsp)?;

        // Overcurrent safety check: poll the status register after the
        // FPGA is initialized. All chip types now support this — T76
        // NAND/eMMC repack the chip-parameter header into the 0x39 request.
        {
            let (status, ovc) = self.protocol.get_ovc_status(&self.usb, &device)?;
            if ovc != 0 || status.error != 0 {
                return Err(MiniproError::Overcurrent {
                    address: status.address,
                });
            }
        }

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

    /// Reopen the programmer after a disconnect (e.g., bootloader switch).
    ///
    /// Waits up to 20 seconds for disconnect and 20 seconds for reconnect.
    /// Skips the bootloader-mode rejection so the handle can be used in
    /// bootloader mode for firmware updates.
    pub fn reconnect(&mut self, allow_bootloader: bool) -> Result<()> {
        let pid = self.usb.pid();
        let _old = std::mem::take(&mut self.usb);
        wait_for_disconnect(pid, 20_000)?;
        wait_for_reconnect(pid, 20_000)?;

        let (usb, initial_model) = open_programmer()?;
        let sys_info = if initial_model == ProgrammerModel::Tl866a {
            tl866a::get_system_info(&usb)?
        } else {
            get_system_info(&usb)?
        };

        if !allow_bootloader && sys_info.status == ProgrammerStatus::Bootloader {
            return Err(MiniproError::BootloaderMode);
        }

        self.info = ProgrammerInfo {
            model: sys_info.model,
            status: sys_info.status,
            firmware: sys_info.firmware,
            firmware_str: sys_info.firmware_str,
            device_code: sys_info.device_code,
            serial_number: sys_info.serial_number,
            hardware_version: sys_info.hardware_version,
        };
        self.usb = usb;
        // Protocol object doesn't need to change; it's model-specific
        Ok(())
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
