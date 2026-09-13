//! ICSP wiring tables.
//!
//! The device database encodes a *wiring class* index in bits 8–15 of
//! `package_details` (parsed as [`crate::device::PackageDetails::icsp`]).
//! Upstream C minipro uses that index only to name a canned image
//! (`ICP%03d.JPG`).  This module instead maps each verified
//! `(ProgrammerModel, class)` pair to an explicit list of header-to-chip
//! connections so both the CLI and the GUI can render real wiring diagrams.
//!
//! Mappings are added only after verification against official documentation
//! (XGecu user guides) or hardware measurement.  Unknown combinations return
//! `None` — callers must fall back to a pin-numbering-only diagram rather
//! than guess.

use crate::device::ProgrammerModel;

/// One connection between the programmer's ICSP header and the target chip.
#[derive(Debug, Clone, Copy)]
pub struct IcspWire {
    /// Pin number on the programmer's ICSP header.
    pub header_pin: u8,
    /// Signal name carried on that header pin (e.g. `"SCK"`, `"/CS"`).
    pub signal: &'static str,
    /// Pin number on the target chip package (index into `chip_labels`).
    pub chip_pin: u8,
}

/// A verified wiring diagram for one (model, class) combination.
#[derive(Debug)]
pub struct IcspWiring {
    /// Human-readable title, e.g. `"25-series SPI NOR"`.
    pub title: &'static str,
    /// Label for each chip pin; index 0 is chip pin 1.  Pins with no wire are
    /// rendered as not-connected by callers.
    pub chip_labels: &'static [&'static str],
    /// Header-to-chip connections.
    pub wires: &'static [IcspWire],
    /// Safety/usage notes rendered under the diagram.
    pub notes: &'static [&'static str],
}

// ── Class 0x09 — 25-series SPI NOR flash (SOIC-8 target) ─────────────────────
//
// Verified against the XGecu T56/TL866II user guide (in-circuit programming
// section) and the T76 guide's ISP connection reference.
//
// SOIC-8 pinout (all 25-series parts):
//   1 /CS   2 SO/IO1   3 /WP/IO2   4 GND   5 SI/IO0   6 SCK   7 /HOLD/IO3   8 VCC

const SPI_NOR_LABELS: &[&str] = &[
    "/CS",
    "SO/IO1",
    "/WP/IO2",
    "GND",
    "SI/IO0",
    "SCK",
    "/HOLD/IO3",
    "VCC",
];

/// TL866A / TL866II+ 1×6 header: pin 1 at the /CS end.
static SPI_NOR_6PIN: IcspWiring = IcspWiring {
    title: "25-series SPI NOR",
    chip_labels: SPI_NOR_LABELS,
    wires: &[
        IcspWire {
            header_pin: 1,
            signal: "/CS",
            chip_pin: 1,
        },
        IcspWire {
            header_pin: 2,
            signal: "VCC",
            chip_pin: 8,
        },
        IcspWire {
            header_pin: 3,
            signal: "GND",
            chip_pin: 4,
        },
        IcspWire {
            header_pin: 4,
            signal: "MOSI",
            chip_pin: 5,
        },
        IcspWire {
            header_pin: 5,
            signal: "MISO",
            chip_pin: 2,
        },
        IcspWire {
            header_pin: 6,
            signal: "SCK",
            chip_pin: 6,
        },
    ],
    notes: &[
        "/WP (pin 3) and /HOLD (pin 7) are not driven — tie them to VCC on the target.",
        "A ~30 pF capacitor from MISO to GND can improve read stability.",
    ],
};

/// T76 2×14 header: odd pins bottom row, even pins top row, pin 1 lower-left.
static SPI_NOR_T76: IcspWiring = IcspWiring {
    title: "25-series SPI NOR",
    chip_labels: SPI_NOR_LABELS,
    wires: &[
        IcspWire { header_pin: 14, signal: "/CS",       chip_pin: 1 },
        IcspWire { header_pin: 16, signal: "SCK",       chip_pin: 6 },
        IcspWire { header_pin: 8,  signal: "MOSI/IO0",  chip_pin: 5 },
        IcspWire { header_pin: 6,  signal: "MISO/IO1",  chip_pin: 2 },
        IcspWire { header_pin: 7,  signal: "/WP/IO2",   chip_pin: 3 },
        IcspWire { header_pin: 9,  signal: "/HOLD/IO3", chip_pin: 7 },
        IcspWire { header_pin: 20, signal: "VCC",       chip_pin: 8 },
        IcspWire { header_pin: 22, signal: "VCC",       chip_pin: 8 },
        IcspWire { header_pin: 24, signal: "VCC",       chip_pin: 8 },
        IcspWire { header_pin: 27, signal: "GND",       chip_pin: 4 },
        IcspWire { header_pin: 28, signal: "GND",       chip_pin: 4 },
    ],
    notes: &[
        "Multiple VCC/GND header pins are shown for completeness — one connection of each is sufficient.",
    ],
};

// ── Class 0x40 — eMMC ISP (T56, 1×8 header, partial) ─────────────────────────
//
// From the XGecu T56 guide ISP schematic the 8-pin header carries (pin 1 at
// left): 1=GND, 2=CLK, 3=GND, 4=CMD, 5=D0, 6=D1, 7=D2, 8=D3 — no VCC on the
// header (target is powered separately).  Held back until the target-side
// connection detail is verified.

/// Look up the verified wiring for a `(model, class)` pair.
///
/// Returns `None` when no verified table exists — the caller should show a
/// pin-numbering-only diagram with an "unverified" notice.
pub fn icsp_wiring(model: ProgrammerModel, class: u8) -> Option<&'static IcspWiring> {
    match (model, class) {
        (ProgrammerModel::Tl866a | ProgrammerModel::Tl866iiPlus, 0x09) => Some(&SPI_NOR_6PIN),
        (ProgrammerModel::T76, 0x09) => Some(&SPI_NOR_T76),
        _ => None,
    }
}
