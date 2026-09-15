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
    /// When true, `chip_labels[i]` is the label for physical chip pin `i + 1`
    /// (fixed-pinout serial devices).  When false, the target is a generic
    /// MCU whose pin positions vary by package — `chip_labels` are signal
    /// names and `chip_pin` on a wire only selects a label row.  Callers
    /// must not render the row index as a pin number.
    pub numbered: bool,
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
    numbered: true,
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
    numbered: true,
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

// ── Class 0x05 — AT45DB DataFlash on TL866A/CS (SOIC-8 target) ───────────────
//
// NOTE: class 0x05 is family-dependent.  In the legacy INFOIC section
// (TL866A/CS) it marks the `AT45DBxxx@ICSP` DataFlash entries; in INFOIC2PLUS
// and INFOICT76 it marks SPI NAND (e.g. W25N-series) which uses the standard
// 25-series SOIC-8 pinout.  The AT45DB pinout is entirely different:
//   1 SI   2 SCK   3 /RESET   4 /CS   5 /WP   6 VCC   7 GND   8 SO
// (AT45DB161D datasheet; the 6-pin header's signal roles are unchanged.)

static AT45DB_6PIN: IcspWiring = IcspWiring {
    title: "AT45DB DataFlash",
    numbered: true,
    chip_labels: &["SI", "SCK", "/RESET", "/CS", "/WP", "VCC", "GND", "SO"],
    wires: &[
        IcspWire {
            header_pin: 1,
            signal: "/CS",
            chip_pin: 4,
        },
        IcspWire {
            header_pin: 2,
            signal: "VCC",
            chip_pin: 6,
        },
        IcspWire {
            header_pin: 3,
            signal: "GND",
            chip_pin: 7,
        },
        IcspWire {
            header_pin: 4,
            signal: "MOSI",
            chip_pin: 1,
        },
        IcspWire {
            header_pin: 5,
            signal: "MISO",
            chip_pin: 8,
        },
        IcspWire {
            header_pin: 6,
            signal: "SCK",
            chip_pin: 2,
        },
    ],
    notes: &[
        "/RESET (pin 3) and /WP (pin 5) are not driven — tie them to VCC on the target.",
        "Chip pinout differs from 25-series SPI flash — do not mix up the wiring.",
    ],
};

// ── Class 0x05 — SPI NAND on TL866II+/newer (SOIC-8 target) ──────────────────
//
// SPI NAND (W25N, GD5F, …) uses the standard 25-series SOIC-8 pinout, so the
// wiring is identical to class 0x09 on the same header; only the title and
// chip labels context differ.

static SPI_NAND_6PIN: IcspWiring = IcspWiring {
    title: "SPI NAND flash",
    numbered: true,
    chip_labels: SPI_NOR_LABELS,
    wires: SPI_NOR_6PIN.wires,
    notes: SPI_NOR_6PIN.notes,
};

// ── Class 0x01 — Atmel SPI ISP: AT89S, AT90S, ATmega (TL866A) ────────────────
//
// Verified against MiniPro ICP001.JPG (legacy TL866A/CS app img folder).
// Header: 1=RST/nRST, 2=VCC, 3=GND, 4=MOSI, 5=MISO, 6=SCK.
// Chip side is a generic MCU — pin positions vary by package:
//   ATmega (PB5–7 variant): MOSI=PB5, MISO=PB6, SCK=PB7
//   AT89S51/52:             MOSI=P1.5, MISO=P1.6, SCK=P1.7, RST=pin 9 (DIP-40)

/// Shared 6-pin wiring for all AVR-style classes: the header always carries
/// RST, VCC, GND, MOSI, MISO, SCK — only the chip-side port pins differ.
static AVR_SPI_WIRES: &[IcspWire] = &[
    IcspWire {
        header_pin: 1,
        signal: "/RST",
        chip_pin: 2,
    },
    IcspWire {
        header_pin: 2,
        signal: "VCC",
        chip_pin: 1,
    },
    IcspWire {
        header_pin: 3,
        signal: "GND",
        chip_pin: 6,
    },
    IcspWire {
        header_pin: 4,
        signal: "MOSI",
        chip_pin: 3,
    },
    IcspWire {
        header_pin: 5,
        signal: "MISO",
        chip_pin: 4,
    },
    IcspWire {
        header_pin: 6,
        signal: "SCK",
        chip_pin: 5,
    },
];

const GENERIC_TARGET_NOTE: &str =
    "Generic MCU target — connect by signal name; pin numbers vary by device and package.";

static ATMEL_SPI_6PIN: IcspWiring = IcspWiring {
    title: "Atmel SPI ISP (AT89S/AT90S/ATmega)",
    numbered: false,
    chip_labels: &[
        "VCC",
        "RST/nRST",
        "MOSI (PB5/P1.5)",
        "MISO (PB6/P1.6)",
        "SCK (PB7/P1.7)",
        "GND",
    ],
    wires: AVR_SPI_WIRES,
    notes: &[
        GENERIC_TARGET_NOTE,
        "ATmega: MOSI=PB5, MISO=PB6, SCK=PB7 — AT89S: MOSI=P1.5, MISO=P1.6, SCK=P1.7.",
    ],
};

// ── Class 0x02 — PIC ICSP, ICD2-compatible (TL866A) ──────────────────────────
//
// Verified against MiniPro ICP002.JPG and the community-documented TL866A
// header pinout (microsin.net).  Header: 1=VPP/MCLR, 2=VCC, 3=GND, 4=PGD,
// 5=PGC, 6=NC — same order as the Microchip ICD2 connector.

static PIC_ICD2_6PIN: IcspWiring = IcspWiring {
    title: "PIC ICSP (ICD2-compatible)",
    numbered: false,
    chip_labels: &["VDD", "VPP/MCLR", "PGD", "PGC", "VSS"],
    wires: &[
        IcspWire {
            header_pin: 1,
            signal: "VPP/MCLR",
            chip_pin: 2,
        },
        IcspWire {
            header_pin: 2,
            signal: "VCC",
            chip_pin: 1,
        },
        IcspWire {
            header_pin: 3,
            signal: "GND",
            chip_pin: 5,
        },
        IcspWire {
            header_pin: 4,
            signal: "PGD",
            chip_pin: 3,
        },
        IcspWire {
            header_pin: 5,
            signal: "PGC",
            chip_pin: 4,
        },
    ],
    notes: &[
        GENERIC_TARGET_NOTE,
        "Header pin 6 is not connected.",
        "Low-voltage-programming parts may also need the LVP/PGM pin handled per the device datasheet.",
    ],
};

// ── Classes 0x06/0x07/0x08 — AVR SPI variants (TL866A) ───────────────────────
//
// Verified against MiniPro ICP006/007/008.JPG.  The header map is identical
// to class 0x01; the classes differ only in which chip port pins carry SPI:
//   0x06: ATmega64/128 — MOSI=PE0, MISO=PE1, SCK=PB1
//   0x07: "AVR SPI download 2" (ATmega8, AT90S) — MOSI=PB3, MISO=PB4, SCK=PB5
//   0x08: "AVR SPI download 3" (ATmega8U2/16U2/32U2) — MOSI=PB2, MISO=PB3, SCK=PB1

static ATMEGA64_SPI_6PIN: IcspWiring = IcspWiring {
    title: "ATmega64/128 SPI ISP",
    numbered: false,
    chip_labels: &[
        "VCC+AVCC",
        "RST/nRST",
        "MOSI (PE0)",
        "MISO (PE1)",
        "SCK (PB1)",
        "GND",
    ],
    wires: AVR_SPI_WIRES,
    notes: &[
        GENERIC_TARGET_NOTE,
        "ATmega64/128 route ISP data through PE0/PE1 — SCK stays on PB1.",
        "AVCC must be powered alongside VCC.",
    ],
};

static AVR_SPI_PB345_6PIN: IcspWiring = IcspWiring {
    title: "AVR SPI ISP (PB3/4/5)",
    numbered: false,
    chip_labels: &[
        "VCC",
        "RST/nRST",
        "MOSI (PB3)",
        "MISO (PB4)",
        "SCK (PB5)",
        "GND",
    ],
    wires: AVR_SPI_WIRES,
    notes: &[GENERIC_TARGET_NOTE],
};

static AVR_SPI_PB123_6PIN: IcspWiring = IcspWiring {
    title: "AVR SPI ISP (PB1/2/3)",
    numbered: false,
    chip_labels: &[
        "VCC",
        "RST/nRST",
        "MOSI (PB2)",
        "MISO (PB3)",
        "SCK (PB1)",
        "GND",
    ],
    wires: AVR_SPI_WIRES,
    notes: &[GENERIC_TARGET_NOTE],
};

// NOTE: legacy classes 0x03 (SyncMos SM39R/SM59R 2-wire) and 0x04 (SM59D
// 3-wire) are documented in the MiniPro ICP003/004 images, but no device in
// the current database references them — no table until a device needs it.

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
        // Class 0x05 means AT45DB DataFlash in the legacy TL866 DB, but
        // SPI NAND (25-series pinout) in the newer databases.
        (ProgrammerModel::Tl866a, 0x05) => Some(&AT45DB_6PIN),
        (ProgrammerModel::Tl866iiPlus, 0x05) => Some(&SPI_NAND_6PIN),
        // Legacy MiniPro ICP001/002/006/007/008 classes (TL866A 6-pin header).
        // TL866II+ shares the physical header but its per-class signal muxing
        // for these classes is not yet verified — keep it on the fallback path.
        (ProgrammerModel::Tl866a, 0x01) => Some(&ATMEL_SPI_6PIN),
        (ProgrammerModel::Tl866a, 0x02) => Some(&PIC_ICD2_6PIN),
        (ProgrammerModel::Tl866a, 0x06) => Some(&ATMEGA64_SPI_6PIN),
        (ProgrammerModel::Tl866a, 0x07) => Some(&AVR_SPI_PB345_6PIN),
        (ProgrammerModel::Tl866a, 0x08) => Some(&AVR_SPI_PB123_6PIN),
        _ => None,
    }
}
