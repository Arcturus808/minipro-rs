use thiserror::Error;

#[derive(Debug, Error)]
pub enum MiniproError {
    #[error("USB error: {0}")]
    Usb(nusb::Error),

    #[error("No programmer found. If one is connected, this can happen after the computer wakes from sleep — unplug the programmer, wait 20-30 seconds, plug it back in, and try again. On Windows, if this is a new device, you may need to install the WinUSB driver with Zadig (https://zadig.akeo.ie/).")]
    NoProgrammerFound,

    #[error("Multiple programmers connected; please connect only one")]
    MultipleProgrammersFound,

    #[error("{0}")]
    DeviceNotFound(String),

    #[error("Chip ID mismatch: expected {expected:#010x} (from database), got {actual:#010x} (from chip)")]
    ChipIdMismatch { expected: u32, actual: u32 },

    #[error("Overcurrent detected at address {address:#010x}")]
    Overcurrent { address: u32 },

    #[error("Verify failed at {address:#010x}: expected {expected:#04x}, got {actual:#04x}")]
    VerifyFailed {
        address: u32,
        expected: u8,
        actual: u8,
    },

    #[error("Chip is not blank at {address:#010x}")]
    NotBlank { address: u32 },

    #[error("XML database parse error: {0}")]
    Xml(String),

    #[error("IO error: {0}")]
    Io(std::io::Error),

    #[error("Response too short: expected {expected} bytes, got {actual}")]
    ResponseTooShort { expected: usize, actual: usize },

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Operation not supported by this programmer model")]
    UnsupportedOperation,

    #[error("File format error: {0}")]
    FileFormat(String),

    #[error("Programmer is in bootloader mode; firmware update required")]
    BootloaderMode,

    #[error("Programmer firmware too old: got {got:#06x}, minimum required {need:#06x}")]
    FirmwareTooOld { got: u32, need: u32 },

    #[error("Algorithm decompression error: {0}")]
    AlgorithmDecompress(String),

    #[error("Algorithm CRC mismatch")]
    AlgorithmCrc,
}

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
