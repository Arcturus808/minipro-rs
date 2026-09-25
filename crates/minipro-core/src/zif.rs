//! ZIF socket physical placement rules per programmer model.
//!
//! These describe the real socket geometry observed from XGPro's own
//! placement diagrams — socket size, lever position, and which end of the
//! socket a DIP chip justifies against.  They live in core (not the GUI) so
//! every frontend shares one source of truth: the GUI's SVG diagram, its
//! pin-test highlighting, and the CLI's textual placement hints.

use crate::device::ProgrammerModel;

/// Which end of the ZIF socket a DIP chip justifies against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZifInsertion {
    /// Chip pin 1 sits at ZIF pin 1 (top of socket).
    Top,
    /// The chip's lower-left pin (`pin_count / 2`) sits at ZIF pin
    /// `pins / 2` — e.g. a DIP-8 occupies ZIF 21-24 + 25-28.
    Bottom,
}

/// Physical ZIF socket description for one programmer model.
#[derive(Debug, Clone, Copy)]
pub struct ZifSpec {
    /// Total socket pins (40 or 48).
    pub pins: usize,
    /// Lever at the pin-1 (top) end of the socket.  False only on the T48.
    pub lever_top: bool,
    /// Which end the chip justifies against.
    pub insertion: ZifInsertion,
}

/// Socket description for a programmer model.
///
/// Observed empirically from XGPro's placement diagrams:
/// TL866A/CS/II+ and T48 are 40-pin, top-justified; T48 alone has the
/// lever at the bottom.  T56/T76 are 48-pin, bottom-justified, lever at
/// the top.
pub fn zif_spec(model: ProgrammerModel) -> ZifSpec {
    use ProgrammerModel::*;
    match model {
        Tl866a | Tl866cs | Tl866iiPlus => ZifSpec {
            pins: 40,
            lever_top: true,
            insertion: ZifInsertion::Top,
        },
        T48 => ZifSpec {
            pins: 40,
            lever_top: false,
            insertion: ZifInsertion::Top,
        },
        T56 | T76 => ZifSpec {
            pins: 48,
            lever_top: true,
            insertion: ZifInsertion::Bottom,
        },
    }
}

/// ZIF socket pin occupied by `device_pin` (1-based) of a `pin_count`-pin
/// DIP inserted per `spec`'s rule.
///
/// Returns `None` when no mapping exists: `device_pin` out of range, or a
/// `pin_count` that is zero, odd, or larger than the socket.
pub fn device_to_zif(spec: &ZifSpec, device_pin: usize, pin_count: usize) -> Option<usize> {
    if device_pin == 0
        || device_pin > pin_count
        || pin_count == 0
        || pin_count % 2 != 0
        || pin_count > spec.pins
    {
        return None;
    }
    let half = pin_count / 2;
    Some(match spec.insertion {
        ZifInsertion::Top if device_pin <= half => device_pin,
        ZifInsertion::Top => spec.pins - half + (device_pin - half),
        ZifInsertion::Bottom => spec.pins / 2 - half + device_pin,
    })
}

/// Inverse of [`device_to_zif`]: which device pin sits at `zif_pin`.
pub fn zif_to_device(spec: &ZifSpec, zif_pin: usize, pin_count: usize) -> Option<usize> {
    (1..=pin_count).find(|d| device_to_zif(spec, *d, pin_count) == Some(zif_pin))
}

/// Sorted ZIF pins occupied by a `pin_count`-pin DIP under `spec`'s rule.
/// Empty when `pin_count` cannot map (zero, odd, or exceeds the socket).
pub fn occupied_zif_pins(spec: &ZifSpec, pin_count: usize) -> Vec<usize> {
    (1..=pin_count)
        .filter_map(|d| device_to_zif(spec, d, pin_count))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_specs() {
        for m in [
            ProgrammerModel::Tl866a,
            ProgrammerModel::Tl866cs,
            ProgrammerModel::Tl866iiPlus,
        ] {
            let s = zif_spec(m);
            assert_eq!(s.pins, 40);
            assert!(s.lever_top);
            assert_eq!(s.insertion, ZifInsertion::Top);
        }
        let s = zif_spec(ProgrammerModel::T48);
        assert_eq!(s.pins, 40);
        assert!(!s.lever_top);
        assert_eq!(s.insertion, ZifInsertion::Top);
        for m in [ProgrammerModel::T56, ProgrammerModel::T76] {
            let s = zif_spec(m);
            assert_eq!(s.pins, 48);
            assert!(s.lever_top);
            assert_eq!(s.insertion, ZifInsertion::Bottom);
        }
    }

    #[test]
    fn top_justified_dip8_on_40pin() {
        let s = zif_spec(ProgrammerModel::Tl866a);
        // Left column 1-4, right column 37-40.
        let occ = occupied_zif_pins(&s, 8);
        assert_eq!(occ, vec![1, 2, 3, 4, 37, 38, 39, 40]);
        assert_eq!(device_to_zif(&s, 1, 8), Some(1));
        assert_eq!(device_to_zif(&s, 5, 8), Some(37));
        assert_eq!(device_to_zif(&s, 8, 8), Some(40));
    }

    #[test]
    fn bottom_justified_dip8_on_48pin() {
        let s = zif_spec(ProgrammerModel::T76);
        // Chip's lower-left pin (4) at ZIF 24: ZIF 21-24 + 25-28.
        let occ = occupied_zif_pins(&s, 8);
        assert_eq!(occ, vec![21, 22, 23, 24, 25, 26, 27, 28]);
        assert_eq!(device_to_zif(&s, 1, 8), Some(21));
        assert_eq!(device_to_zif(&s, 4, 8), Some(24));
        assert_eq!(device_to_zif(&s, 5, 8), Some(25));
        assert_eq!(device_to_zif(&s, 8, 8), Some(28));
    }

    #[test]
    fn full_socket_device_fills_all_pins() {
        let s = zif_spec(ProgrammerModel::T76);
        assert_eq!(occupied_zif_pins(&s, 48).len(), 48);
        let s40 = zif_spec(ProgrammerModel::T48);
        assert_eq!(occupied_zif_pins(&s40, 40).len(), 40);
        assert_eq!(device_to_zif(&s40, 40, 40), Some(40));
    }

    #[test]
    fn zif_to_device_is_inverse() {
        for m in [
            ProgrammerModel::Tl866a,
            ProgrammerModel::T48,
            ProgrammerModel::T56,
            ProgrammerModel::T76,
        ] {
            let s = zif_spec(m);
            for d in 1..=28 {
                let z = device_to_zif(&s, d, 28).unwrap();
                assert_eq!(zif_to_device(&s, z, 28), Some(d));
            }
        }
    }

    #[test]
    fn invalid_counts_return_none() {
        let s = zif_spec(ProgrammerModel::Tl866a);
        assert_eq!(device_to_zif(&s, 1, 0), None);
        assert_eq!(device_to_zif(&s, 1, 7), None); // odd pin count
        assert_eq!(device_to_zif(&s, 9, 8), None); // out of range
        assert!(occupied_zif_pins(&s, 42).is_empty()); // larger than socket
    }
}
