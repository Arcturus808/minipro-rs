use thiserror::Error;

#[derive(Debug, Error)]
pub enum MiniproError {
    #[error("USB error: {0}")]
    Usb(nusb::Error),

    #[error("No programmer found; is one connected and the driver installed?")]
    NoProgrammerFound,

    #[error("Multiple programmers connected; please connect only one")]
    MultipleProgrammersFound,

    #[error("Device '{0}' not found in the chip database")]
    DeviceNotFound(String),

    #[error("Chip ID mismatch: expected {expected:#010x}, got {actual:#010x}")]
    ChipIdMismatch { expected: u32, actual: u32 },

    #[error("Overcurrent detected at address {address:#010x}")]
    Overcurrent { address: u32 },

    #[error("Verify failed at {address:#010x}: expected {expected:#04x}, got {actual:#04x}")]
    VerifyFailed { address: u32, expected: u8, actual: u8 },

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
