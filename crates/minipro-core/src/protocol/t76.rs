//! T76 protocol.
//!
//! The T76 shares the TL866II+ command set but uses different USB endpoints
//! for payload transfers (EP 0x05 OUT instead of EP 0x02/0x03).
//! Endpoint selection is handled at the `UsbDevice` level based on PID;
//! the command protocol is identical.

pub use super::tl866iiplus::Tl866iiPlusProtocol as T76Protocol;
