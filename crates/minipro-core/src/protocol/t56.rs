//! T56 protocol.
//!
//! The T56 shares the TL866II+ command set with FPGA algorithm upload support.
//! For now this re-uses the TL866II+ implementation; FPGA bitstream upload
//! will be added in Phase 3.

pub use super::tl866iiplus::Tl866iiPlusProtocol as T56Protocol;
