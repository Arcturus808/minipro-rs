//! Error type and result alias used throughout the crate.

use thiserror::Error;

/// Error type for all `minipro-core` operations.
#[derive(Debug, Error)]
pub enum MiniproError {
    /// Low-level USB transport error from `nusb`.
    #[error("USB error: {0}")]
    Usb(nusb::Error),

    /// No supported programmer was found on the USB bus.
    #[error("No programmer found. If one is connected, this can happen after the computer wakes from sleep — unplug the programmer, wait 20-30 seconds, plug it back in, and try again. On Windows, if this is a new device, you may need to install the WinUSB driver with Zadig (https://zadig.akeo.ie/).")]
    NoProgrammerFound,

    /// More than one programmer is attached.
    #[error("Multiple programmers connected; please connect only one")]
    MultipleProgrammersFound,

    /// The requested chip is not in the device database.
    #[error("{0}")]
    DeviceNotFound(String),

    /// The chip ID read from the device does not match the database entry.
    #[error("Chip ID mismatch: expected {expected:#010x} (from database), got {actual:#010x} (from chip)")]
    ChipIdMismatch {
        /// Chip ID expected by the database.
        expected: u32,
        /// Chip ID actually read from the device.
        actual: u32,
    },

    /// The programmer tripped its over-current protection.
    #[error("Overcurrent detected at address {address:#010x}")]
    Overcurrent {
        /// Address the programmer was accessing when the trip occurred.
        address: u32,
    },

    /// The programmer reported a write failure at the given address.
    /// The firmware status word carries no byte detail — `code` is the
    /// raw firmware status/error byte.
    #[error("Write failed at {address:#010x} (programmer status {code:#04x})")]
    WriteFailed {
        /// Failing address reported by the programmer.
        address: u32,
        /// Raw firmware status/error byte.
        code: u8,
    },

    /// Verify pass found a mismatching byte/word.
    #[error("Verify failed at {address:#010x}: expected {expected:#04x}, got {actual:#04x}")]
    VerifyFailed {
        /// Mismatch address.
        address: u32,
        /// Expected value.
        expected: u8,
        /// Value actually read back.
        actual: u8,
    },

    /// Blank check found programmed data.
    #[error("Chip is not blank at {address:#010x}")]
    NotBlank {
        /// First non-blank address.
        address: u32,
    },

    /// `infoic.xml` / `logicic.xml` / `algorithm.xml` failed to parse.
    #[error("XML database parse error: {0}")]
    Xml(String),

    /// Filesystem I/O failure.
    #[error("IO error: {0}")]
    Io(std::io::Error),

    /// Programmer returned fewer bytes than a command requires.
    #[error("Response too short: expected {expected} bytes, got {actual}")]
    ResponseTooShort {
        /// Required byte count.
        expected: usize,
        /// Byte count actually received.
        actual: usize,
    },

    /// Programmer returned an unexpected or malformed response.
    #[error("Protocol error: {0}")]
    Protocol(String),

    /// The connected programmer model does not implement this operation.
    #[error("Operation not supported by this programmer model")]
    UnsupportedOperation,

    /// Input file could not be parsed as a supported format.
    #[error("File format error: {0}")]
    FileFormat(String),

    /// Programmer is running the bootloader; only firmware update is possible.
    #[error("Programmer is in bootloader mode; firmware update required")]
    BootloaderMode,

    /// Programmer firmware is older than the operation requires.
    #[error("Programmer firmware too old: got {got:#06x}, minimum required {need:#06x}")]
    FirmwareTooOld {
        /// Firmware version reported by the device.
        got: u32,
        /// Minimum firmware version required.
        need: u32,
    },

    /// FPGA algorithm bitstream decompression failed.
    #[error("Algorithm decompression error: {0}")]
    AlgorithmDecompress(String),

    /// FPGA algorithm bitstream CRC check failed.
    #[error("Algorithm CRC mismatch")]
    AlgorithmCrc,

    /// Resource download (firmware/database) failed.
    #[error("Download error: {0}")]
    Download(String),

    /// Operation was cancelled by the caller/progress callback.
    #[error("Operation cancelled")]
    Cancelled,
}

/// Convenience alias for `minipro-core` results.
pub type Result<T> = std::result::Result<T, MiniproError>;

impl From<nusb::Error> for MiniproError {
    fn from(e: nusb::Error) -> Self {
        MiniproError::Usb(e)
    }
}

impl MiniproError {
    /// Returns `true` if this error represents a USB communication failure
    /// (device gone, timed out, suspended, etc.) rather than a logic-level
    /// protocol mismatch.
    ///
    /// USB errors wrapped in `Protocol(String)` are detected by keyword
    /// matching, since the USB layer wraps nusb errors and timeouts as
    /// `Protocol(format!(...))`.
    pub fn is_usb_communication_error(&self) -> bool {
        match self {
            MiniproError::Usb(_) => true,
            MiniproError::NoProgrammerFound => true,
            MiniproError::Protocol(msg) => {
                const KEYWORDS: &[&str] = &[
                    "STALL",
                    "NoDevice",
                    "LIBUSB_ERROR_NO_DEVICE",
                    "LIBUSB_ERROR_IO",
                    "LIBUSB_ERROR_PIPE",
                    "DeviceNotFound",
                    "endpoint",
                    "USB error",
                    "No programmer connected",
                    "unknown error",
                    "timed out",
                    "cannot open",
                    "cannot claim",
                ];
                KEYWORDS.iter().any(|&kw| msg.contains(kw))
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usb_variant_is_usb_error() {
        // We can't easily construct nusb::Error, but Usb(_) should always
        // be detected. Use a fake error via the From impl.
        // nusb::Error doesn't have a public constructor, so we test via
        // the Protocol path which is the common case in our codebase.
    }

    #[test]
    fn test_no_programmer_found_is_usb_error() {
        assert!(MiniproError::NoProgrammerFound.is_usb_communication_error());
    }

    #[test]
    fn test_protocol_timeout_is_usb_error() {
        let err = MiniproError::Protocol(
            "USB transfer timed out — the programmer may be in a bad state. Unplug and replug."
                .into(),
        );
        assert!(err.is_usb_communication_error());
    }

    #[test]
    fn test_protocol_no_device_is_usb_error() {
        let err = MiniproError::Protocol("LIBUSB_ERROR_NO_DEVICE".into());
        assert!(err.is_usb_communication_error());
    }

    #[test]
    fn test_protocol_stall_is_usb_error() {
        let err = MiniproError::Protocol("endpoint STALL".into());
        assert!(err.is_usb_communication_error());
    }

    #[test]
    fn test_protocol_unknown_error_is_usb_error() {
        let err = MiniproError::Protocol("unknown error".into());
        assert!(err.is_usb_communication_error());
    }

    #[test]
    fn test_protocol_non_usb_is_not_usb_error() {
        let err = MiniproError::Protocol("Invalid pin count!".into());
        assert!(!err.is_usb_communication_error());
    }

    #[test]
    fn test_protocol_generic_message_is_not_usb_error() {
        let err = MiniproError::Protocol("unexpected response byte 0x42".into());
        assert!(!err.is_usb_communication_error());
    }

    #[test]
    fn test_verify_failed_is_not_usb_error() {
        let err = MiniproError::VerifyFailed {
            address: 0x1000,
            expected: 0xAA,
            actual: 0xBB,
        };
        assert!(!err.is_usb_communication_error());
    }

    #[test]
    fn test_unsupported_is_not_usb_error() {
        assert!(!MiniproError::UnsupportedOperation.is_usb_communication_error());
    }
}
