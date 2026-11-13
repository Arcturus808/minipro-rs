use std::fmt;

/// Errors returned by the RAR reader.
#[derive(Debug)]
pub enum Error {
    /// The archive signature could not be located.
    SignatureNotFound,
    /// A header failed its CRC check or had an invalid layout.
    InvalidHeader(&'static str),
    /// Compressed data was truncated or malformed.
    InvalidData(&'static str),
    /// The archive uses a feature this reader does not support.
    Unsupported(&'static str),
    /// Decompressed data failed its file CRC check.
    FileCrcMismatch { expected: u32, computed: u32 },
    /// I/O error from the underlying reader.
    Io(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SignatureNotFound => write!(f, "couldn't find RAR header"),
            Self::InvalidHeader(m) => write!(f, "invalid RAR header: {m}"),
            Self::InvalidData(m) => write!(f, "bad RAR file data: {m}"),
            Self::Unsupported(m) => write!(f, "unsupported RAR feature: {m}"),
            Self::FileCrcMismatch { expected, computed } => write!(
                f,
                "file CRC error: expected {expected:#010x}, computed {computed:#010x}"
            ),
            Self::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
