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
        IcspWire {
            header_pin: 14,
            signal: "/CS",
            chip_pin: 1,
        },
        IcspWire {
            header_pin: 16,
            signal: "SCK",
            chip_pin: 6,
        },
        IcspWire {
            header_pin: 8,
            signal: "MOSI/IO0",
            chip_pin: 5,
        },
        IcspWire {
            header_pin: 6,
            signal: "MISO/IO1",
            chip_pin: 2,
        },
        IcspWire {
            header_pin: 7,
            signal: "/WP/IO2",
            chip_pin: 3,
        },
        IcspWire {
            header_pin: 9,
            signal: "/HOLD/IO3",
            chip_pin: 7,
        },
        IcspWire {
            header_pin: 22,
            signal: "VCC",
            chip_pin: 8,
        },
        IcspWire {
            header_pin: 26,
            signal: "GND",
            chip_pin: 4,
        },
        IcspWire {
            header_pin: 28,
            signal: "GND",
            chip_pin: 4,
        },
    ],
    notes: &[
        "Header pins 1/11/15/21/27 are SGND (signal ground/shield) — do not use as the main GND.",
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

// ════════════════════════════════════════════════════════════════════════════
// Newer-programmer tables — verified against the Xgpro V13.17 `img/` set
// (unprefixed ICPnnn = TL866II+ 1×6; T48ICPnnn = T48 2×8 zigzag;
//  T56ICPnnn = T56 1×8; T76ICPnnn = T76 2×14 zigzag).
// ════════════════════════════════════════════════════════════════════════════

// ── Shared AVR/PIC tables for T48 (2×8), T56 (1×8), T76 (2×14) ───────────────
//
// The MCU classes reuse the TL866A chip-side signal labels; only the header
// pin assignment differs per model (each header is muxed per class).

/// T48 AVR ISP: 1=RST, 3=SCK, 5=MOSI, 7=MISO, 13=VCC, 16=GND (T48ICP001/006/007/008).
static AVR_SPI_WIRES_T48: &[IcspWire] = &[
    IcspWire {
        header_pin: 1,
        signal: "/RST",
        chip_pin: 2,
    },
    IcspWire {
        header_pin: 3,
        signal: "SCK",
        chip_pin: 5,
    },
    IcspWire {
        header_pin: 5,
        signal: "MOSI",
        chip_pin: 3,
    },
    IcspWire {
        header_pin: 7,
        signal: "MISO",
        chip_pin: 4,
    },
    IcspWire {
        header_pin: 13,
        signal: "VCC",
        chip_pin: 1,
    },
    IcspWire {
        header_pin: 16,
        signal: "GND",
        chip_pin: 6,
    },
];

/// T56 AVR ISP: 1=RST, 2=GND, 3=SCK, 4=GND, 5=MOSI, 6=MISO, 8=VCC (T56ICP001/006/007/008).
static AVR_SPI_WIRES_T56: &[IcspWire] = &[
    IcspWire {
        header_pin: 1,
        signal: "/RST",
        chip_pin: 2,
    },
    IcspWire {
        header_pin: 2,
        signal: "GND",
        chip_pin: 6,
    },
    IcspWire {
        header_pin: 3,
        signal: "SCK",
        chip_pin: 5,
    },
    IcspWire {
        header_pin: 4,
        signal: "GND",
        chip_pin: 6,
    },
    IcspWire {
        header_pin: 5,
        signal: "MOSI",
        chip_pin: 3,
    },
    IcspWire {
        header_pin: 6,
        signal: "MISO",
        chip_pin: 4,
    },
    IcspWire {
        header_pin: 8,
        signal: "VCC",
        chip_pin: 1,
    },
];

/// T76 AVR ISP: 2=RST, 3=SCK, 7=MOSI, 11=MISO, 22=VCC, 28=GND (T76ICP001/006/007/008).
static AVR_SPI_WIRES_T76: &[IcspWire] = &[
    IcspWire {
        header_pin: 2,
        signal: "/RST",
        chip_pin: 2,
    },
    IcspWire {
        header_pin: 3,
        signal: "SCK",
        chip_pin: 5,
    },
    IcspWire {
        header_pin: 7,
        signal: "MOSI",
        chip_pin: 3,
    },
    IcspWire {
        header_pin: 11,
        signal: "MISO",
        chip_pin: 4,
    },
    IcspWire {
        header_pin: 22,
        signal: "VCC",
        chip_pin: 1,
    },
    IcspWire {
        header_pin: 28,
        signal: "GND",
        chip_pin: 6,
    },
];

macro_rules! avr_tables_for {
    ($wires:expr, $notes:expr, $notes64:expr, [$n1:ident, $n2:ident, $n3:ident, $n4:ident]) => {
        static $n1: IcspWiring = IcspWiring {
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
            wires: $wires,
            notes: $notes,
        };
        static $n2: IcspWiring = IcspWiring {
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
            wires: $wires,
            notes: $notes64,
        };
        static $n3: IcspWiring = IcspWiring {
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
            wires: $wires,
            notes: $notes,
        };
        static $n4: IcspWiring = IcspWiring {
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
            wires: $wires,
            notes: $notes,
        };
    };
}

avr_tables_for!(
    AVR_SPI_WIRES_T48,
    &[
        GENERIC_TARGET_NOTE,
        "No pull resistor on RST or SCK, no clamping diode on MOSI, no decoupling capacitor on MISO.",
    ],
    &[
        GENERIC_TARGET_NOTE,
        "ATmega64/128 route ISP data through PE0/PE1 — SCK stays on PB1.",
        "AVCC must be powered alongside VCC.",
        "No pull resistor on RST or SCK, no clamping diode on MOSI, no decoupling capacitor on MISO.",
    ],
    [
        ATMEL_SPI_T48,
        ATMEGA64_SPI_T48,
        AVR_SPI_PB345_T48,
        AVR_SPI_PB123_T48
    ]
);
avr_tables_for!(
    AVR_SPI_WIRES_T56,
    &[GENERIC_TARGET_NOTE, "Keep the ribbon under ~25 cm."],
    &[
        GENERIC_TARGET_NOTE,
        "ATmega64/128 route ISP data through PE0/PE1 — SCK stays on PB1.",
        "AVCC must be powered alongside VCC.",
        "Keep the ribbon under ~25 cm.",
    ],
    [
        ATMEL_SPI_T56,
        ATMEGA64_SPI_T56,
        AVR_SPI_PB345_T56,
        AVR_SPI_PB123_T56
    ]
);
avr_tables_for!(
    AVR_SPI_WIRES_T76,
    &[GENERIC_TARGET_NOTE],
    &[
        GENERIC_TARGET_NOTE,
        "ATmega64/128 route ISP data through PE0/PE1 — SCK stays on PB1.",
        "AVCC must be powered alongside VCC.",
    ],
    [
        ATMEL_SPI_T76,
        ATMEGA64_SPI_T76,
        AVR_SPI_PB345_T76,
        AVR_SPI_PB123_T76
    ]
);

// ── Class 0x02 — PIC ICSP on T48 / T56 / T76 ─────────────────────────────────
//
// Same chip-side signals as the 6-pin table; header pinout differs per model.
// The T56ICP002 image marks wire prohibitions: no decoupling cap on VPP/MCLR,
// no clamp diode on PGD, no pulldown on PGC.

const PIC_WIRE_NOTES: &[&str] = &[
    GENERIC_TARGET_NOTE,
    "Do not connect a decoupling capacitor on VPP/MCLR, a clamping diode on PGD, or a pulldown resistor on PGC.",
    "Low-voltage-programming parts may also need the LVP/PGM pin handled per the device datasheet.",
];

static PIC_T48: IcspWiring = IcspWiring {
    title: "PIC ICSP (ICD2-compatible)",
    numbered: false,
    chip_labels: PIC_ICD2_6PIN.chip_labels,
    wires: &[
        IcspWire {
            header_pin: 1,
            signal: "VPP/MCLR",
            chip_pin: 2,
        },
        IcspWire {
            header_pin: 3,
            signal: "PGC",
            chip_pin: 4,
        },
        IcspWire {
            header_pin: 5,
            signal: "PGD",
            chip_pin: 3,
        },
        IcspWire {
            header_pin: 13,
            signal: "VCC",
            chip_pin: 1,
        },
        IcspWire {
            header_pin: 16,
            signal: "GND",
            chip_pin: 5,
        },
    ],
    notes: PIC_WIRE_NOTES,
};

static PIC_T56: IcspWiring = IcspWiring {
    title: "PIC ICSP (ICD2-compatible)",
    numbered: false,
    chip_labels: PIC_ICD2_6PIN.chip_labels,
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
    notes: PIC_WIRE_NOTES,
};

static PIC_T76: IcspWiring = IcspWiring {
    title: "PIC ICSP (ICD2-compatible)",
    numbered: false,
    chip_labels: PIC_ICD2_6PIN.chip_labels,
    wires: &[
        IcspWire {
            header_pin: 3,
            signal: "PGC",
            chip_pin: 4,
        },
        IcspWire {
            header_pin: 7,
            signal: "PGD",
            chip_pin: 3,
        },
        IcspWire {
            header_pin: 22,
            signal: "VCC",
            chip_pin: 1,
        },
        IcspWire {
            header_pin: 26,
            signal: "VPP/MCLR",
            chip_pin: 2,
        },
        IcspWire {
            header_pin: 28,
            signal: "GND",
            chip_pin: 5,
        },
    ],
    notes: PIC_WIRE_NOTES,
};

// ── Classes 0x05/0x09 — SPI EEPROM / 25-series NOR on T56 (1×8) ──────────────
//
// T56ICP005/009: 1=GND, 2=CLK, 4=/CS, 5=MOSI, 6=MISO, 7=GND, 8=VCC.

static SPI_NOR_T56: IcspWiring = IcspWiring {
    title: "25-series SPI NOR / SPI EEPROM",
    numbered: true,
    chip_labels: SPI_NOR_LABELS,
    wires: &[
        IcspWire {
            header_pin: 1,
            signal: "GND",
            chip_pin: 4,
        },
        IcspWire {
            header_pin: 2,
            signal: "SCK",
            chip_pin: 6,
        },
        IcspWire {
            header_pin: 4,
            signal: "/CS",
            chip_pin: 1,
        },
        IcspWire {
            header_pin: 5,
            signal: "MOSI",
            chip_pin: 5,
        },
        IcspWire {
            header_pin: 6,
            signal: "MISO",
            chip_pin: 2,
        },
        IcspWire {
            header_pin: 7,
            signal: "GND",
            chip_pin: 4,
        },
        IcspWire {
            header_pin: 8,
            signal: "VCC",
            chip_pin: 8,
        },
    ],
    notes: &[
        "Keep the ribbon under ~25 cm; do not separate the three signal wires.",
        "/WP (pin 3) and /HOLD (pin 7) are not driven — tie them to VCC on the target.",
    ],
};

// ── Classes 0x05/0x09 — SPI EEPROM / 25-series NOR on T48 (2×8 zigzag) ────────
//
// T48ICP005/009: 1=/CS, 5=MISO, 6=GND, 7=SCK, 8=GND, 13=VCC, 15=MOSI, 16=GND.

static SPI_NOR_T48: IcspWiring = IcspWiring {
    title: "25-series SPI NOR / SPI EEPROM",
    numbered: true,
    chip_labels: SPI_NOR_LABELS,
    wires: &[
        IcspWire {
            header_pin: 1,
            signal: "/CS",
            chip_pin: 1,
        },
        IcspWire {
            header_pin: 5,
            signal: "MISO",
            chip_pin: 2,
        },
        IcspWire {
            header_pin: 6,
            signal: "GND",
            chip_pin: 4,
        },
        IcspWire {
            header_pin: 7,
            signal: "SCK",
            chip_pin: 6,
        },
        IcspWire {
            header_pin: 8,
            signal: "GND",
            chip_pin: 4,
        },
        IcspWire {
            header_pin: 13,
            signal: "VCC",
            chip_pin: 8,
        },
        IcspWire {
            header_pin: 15,
            signal: "MOSI",
            chip_pin: 5,
        },
        IcspWire {
            header_pin: 16,
            signal: "GND",
            chip_pin: 4,
        },
    ],
    notes: &[
        "Keep the ribbon under ~25 cm; do not separate the pin 6/7/8 wire group.",
        "The VCC lead also ties the target's /RESET, /WP and /HOLD pins high.",
    ],
};

// ── Class 0x0a — 93xx Microwire (DIP-8 target) ───────────────────────────────
//
// Standard 93xx DIP-8 pinout: 1=CS, 2=SK, 3=DI, 4=DO, 5=GND, 6=ORG, 7=NC,
// 8=VCC.  Verified against ICP010 / T48ICP010 / T76ICP010.

const MW93_LABELS: &[&str] = &["CS", "SK/CLK", "DI", "DO", "GND", "ORG", "NC", "VCC"];

const MW93_ORG_NOTE: &str =
    "ORG (pin 6) selects memory organisation — tie to VCC for ×16 or GND for ×8 per the device datasheet.";

static MW93_6PIN: IcspWiring = IcspWiring {
    title: "93xx Microwire EEPROM",
    numbered: true,
    chip_labels: MW93_LABELS,
    wires: &[
        IcspWire {
            header_pin: 1,
            signal: "CS",
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
            chip_pin: 5,
        },
        IcspWire {
            header_pin: 4,
            signal: "DI",
            chip_pin: 3,
        },
        IcspWire {
            header_pin: 5,
            signal: "DO",
            chip_pin: 4,
        },
        IcspWire {
            header_pin: 6,
            signal: "CLK",
            chip_pin: 2,
        },
    ],
    notes: &[MW93_ORG_NOTE],
};

static MW93_T48: IcspWiring = IcspWiring {
    title: "93xx Microwire EEPROM",
    numbered: true,
    chip_labels: MW93_LABELS,
    wires: &[
        IcspWire {
            header_pin: 1,
            signal: "CS",
            chip_pin: 1,
        },
        IcspWire {
            header_pin: 3,
            signal: "DI",
            chip_pin: 3,
        },
        IcspWire {
            header_pin: 5,
            signal: "DO",
            chip_pin: 4,
        },
        IcspWire {
            header_pin: 7,
            signal: "CLK",
            chip_pin: 2,
        },
        IcspWire {
            header_pin: 9,
            signal: "ORG",
            chip_pin: 6,
        },
        IcspWire {
            header_pin: 13,
            signal: "VCC",
            chip_pin: 8,
        },
        IcspWire {
            header_pin: 16,
            signal: "GND",
            chip_pin: 5,
        },
    ],
    notes: &["The T48 drives ORG from header pin 9 — no manual strap needed."],
};

/// T56ICP010: 1=CS, 2=VCC, 3=GND, 4=DI, 5=DO, 6=CLK, 7=ORG, 8=NC.
static MW93_T56: IcspWiring = IcspWiring {
    title: "93xx Microwire EEPROM",
    numbered: true,
    chip_labels: MW93_LABELS,
    wires: &[
        IcspWire {
            header_pin: 1,
            signal: "CS",
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
            chip_pin: 5,
        },
        IcspWire {
            header_pin: 4,
            signal: "DI",
            chip_pin: 3,
        },
        IcspWire {
            header_pin: 5,
            signal: "DO",
            chip_pin: 4,
        },
        IcspWire {
            header_pin: 6,
            signal: "CLK",
            chip_pin: 2,
        },
        IcspWire {
            header_pin: 7,
            signal: "ORG",
            chip_pin: 6,
        },
    ],
    notes: &[
        "Header pin 8 is not connected.",
        "The T56 drives ORG from header pin 7 — no manual strap needed.",
    ],
};

static MW93_T76: IcspWiring = IcspWiring {
    title: "93xx Microwire EEPROM",
    numbered: true,
    chip_labels: MW93_LABELS,
    wires: &[
        IcspWire {
            header_pin: 3,
            signal: "DO",
            chip_pin: 4,
        },
        IcspWire {
            header_pin: 5,
            signal: "CLK",
            chip_pin: 2,
        },
        IcspWire {
            header_pin: 7,
            signal: "DI",
            chip_pin: 3,
        },
        IcspWire {
            header_pin: 9,
            signal: "CS",
            chip_pin: 1,
        },
        IcspWire {
            header_pin: 11,
            signal: "ORG",
            chip_pin: 6,
        },
        IcspWire {
            header_pin: 22,
            signal: "VCC",
            chip_pin: 8,
        },
        IcspWire {
            header_pin: 28,
            signal: "GND",
            chip_pin: 5,
        },
    ],
    notes: &["The T76 drives ORG from header pin 11 — no manual strap needed."],
};

// ── Class 0x0b — 24xx I²C EEPROM (DIP-8 target) ──────────────────────────────
//
// Standard 24xx DIP-8 pinout: 1=A0, 2=A1, 3=A2, 4=VSS, 5=SDA, 6=SCL, 7=WP,
// 8=VCC.  Verified against ICP011 / T48ICP011 / T56ICP011 / T76ICP011.

const I2C24_LABELS: &[&str] = &["A0", "A1", "A2", "VSS", "SDA", "SCL", "WP", "VCC"];

const I2C24_NOTE: &str =
    "A0–A2 (pins 1–3) are not driven — set the slave address on the target board.";

static I2C24_6PIN: IcspWiring = IcspWiring {
    title: "24xx I2C EEPROM",
    numbered: true,
    chip_labels: I2C24_LABELS,
    wires: &[
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
            signal: "WP",
            chip_pin: 7,
        },
        IcspWire {
            header_pin: 5,
            signal: "SDA",
            chip_pin: 5,
        },
        IcspWire {
            header_pin: 6,
            signal: "SCL",
            chip_pin: 6,
        },
    ],
    notes: &["Header pin 1 is not connected.", I2C24_NOTE],
};

static I2C24_T48: IcspWiring = IcspWiring {
    title: "24xx I2C EEPROM",
    numbered: true,
    chip_labels: I2C24_LABELS,
    wires: &[
        IcspWire {
            header_pin: 1,
            signal: "SDA",
            chip_pin: 5,
        },
        IcspWire {
            header_pin: 3,
            signal: "SCL",
            chip_pin: 6,
        },
        IcspWire {
            header_pin: 5,
            signal: "WP",
            chip_pin: 7,
        },
        IcspWire {
            header_pin: 13,
            signal: "VCC",
            chip_pin: 8,
        },
        IcspWire {
            header_pin: 16,
            signal: "GND",
            chip_pin: 4,
        },
    ],
    notes: &[I2C24_NOTE],
};

static I2C24_T56: IcspWiring = IcspWiring {
    title: "24xx I2C EEPROM",
    numbered: true,
    chip_labels: I2C24_LABELS,
    wires: &[
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
            signal: "WP",
            chip_pin: 7,
        },
        IcspWire {
            header_pin: 5,
            signal: "SDA",
            chip_pin: 5,
        },
        IcspWire {
            header_pin: 6,
            signal: "SCL",
            chip_pin: 6,
        },
    ],
    notes: &["Header pins 1, 7 and 8 are not connected.", I2C24_NOTE],
};

static I2C24_T76: IcspWiring = IcspWiring {
    title: "24xx I2C EEPROM",
    numbered: true,
    chip_labels: I2C24_LABELS,
    wires: &[
        IcspWire {
            header_pin: 3,
            signal: "SDA",
            chip_pin: 5,
        },
        IcspWire {
            header_pin: 5,
            signal: "SCL",
            chip_pin: 6,
        },
        IcspWire {
            header_pin: 7,
            signal: "WP",
            chip_pin: 7,
        },
        IcspWire {
            header_pin: 22,
            signal: "VCC",
            chip_pin: 8,
        },
        IcspWire {
            header_pin: 28,
            signal: "GND",
            chip_pin: 4,
        },
    ],
    notes: &[I2C24_NOTE],
};

// ── Class 0x0c — KB901x/KB9022 embedded controller (LQFP-128) ────────────────
//
// SPI target on LQFP-128 keyboard-controller pins: 59=/CS (KS14), 60=CLK
// (KS15), 61=MOSI (KS16), 62=MISO (KS17), 42=GND (KS03).  Chip labels carry
// the package pin numbers in the text (numbered=false — the label index is
// not the pin number).  Verified against ICP012 / T48ICP012 / T56ICP012 /
// T76ICP012.

const KB90_LABELS: &[&str] = &[
    "pin 59 — /CS (KS14)",
    "pin 60 — CLK (KS15)",
    "pin 61 — MOSI (KS16)",
    "pin 62 — MISO (KS17)",
    "pin 42 — GND (KS03)",
    "VCC — external power",
];

const KB90_EXT_PWR_NOTE: &str = "Target VCC is not supplied from the header — use external power.";

static KB90_6PIN: IcspWiring = IcspWiring {
    title: "KB901x/KB9022 EC (SPI)",
    numbered: false,
    chip_labels: KB90_LABELS,
    wires: &[
        IcspWire {
            header_pin: 1,
            signal: "/CS",
            chip_pin: 1,
        },
        IcspWire {
            header_pin: 3,
            signal: "GND",
            chip_pin: 5,
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
            signal: "CLK",
            chip_pin: 2,
        },
    ],
    notes: &[
        KB90_EXT_PWR_NOTE,
        "Xgpro routes this class through an external SPI driver/level-shifter board.",
    ],
};

static KB90_T48: IcspWiring = IcspWiring {
    title: "KB901x/KB9022 EC (SPI)",
    numbered: false,
    chip_labels: KB90_LABELS,
    wires: &[
        IcspWire {
            header_pin: 1,
            signal: "/CS",
            chip_pin: 1,
        },
        IcspWire {
            header_pin: 5,
            signal: "MISO",
            chip_pin: 4,
        },
        IcspWire {
            header_pin: 6,
            signal: "GND",
            chip_pin: 5,
        },
        IcspWire {
            header_pin: 7,
            signal: "CLK",
            chip_pin: 2,
        },
        IcspWire {
            header_pin: 8,
            signal: "GND",
            chip_pin: 5,
        },
        IcspWire {
            header_pin: 15,
            signal: "MOSI",
            chip_pin: 3,
        },
        IcspWire {
            header_pin: 16,
            signal: "GND",
            chip_pin: 5,
        },
    ],
    notes: &[
        KB90_EXT_PWR_NOTE,
        "Header pin 13 is the VCC pin but is left unconnected for this class.",
        "Keep the pin 6/7/8 wires together — do not separate the ribbon group.",
    ],
};

static KB90_T56: IcspWiring = IcspWiring {
    title: "KB901x/KB9022 EC (SPI)",
    numbered: false,
    chip_labels: KB90_LABELS,
    wires: &[
        IcspWire {
            header_pin: 1,
            signal: "GND",
            chip_pin: 5,
        },
        IcspWire {
            header_pin: 2,
            signal: "CLK",
            chip_pin: 2,
        },
        IcspWire {
            header_pin: 4,
            signal: "/CS",
            chip_pin: 1,
        },
        IcspWire {
            header_pin: 5,
            signal: "MOSI",
            chip_pin: 3,
        },
        IcspWire {
            header_pin: 6,
            signal: "MISO",
            chip_pin: 4,
        },
        IcspWire {
            header_pin: 7,
            signal: "GND",
            chip_pin: 5,
        },
        IcspWire {
            header_pin: 8,
            signal: "VCC",
            chip_pin: 6,
        },
    ],
    notes: &[KB90_EXT_PWR_NOTE],
};

static KB90_T76: IcspWiring = IcspWiring {
    title: "KB901x/KB9022 EC (SPI)",
    numbered: false,
    chip_labels: KB90_LABELS,
    wires: &[
        IcspWire {
            header_pin: 6,
            signal: "SO/MISO",
            chip_pin: 4,
        },
        IcspWire {
            header_pin: 8,
            signal: "SI/MOSI",
            chip_pin: 3,
        },
        IcspWire {
            header_pin: 14,
            signal: "/CS",
            chip_pin: 1,
        },
        IcspWire {
            header_pin: 16,
            signal: "CLK",
            chip_pin: 2,
        },
        IcspWire {
            header_pin: 21,
            signal: "GND",
            chip_pin: 5,
        },
        IcspWire {
            header_pin: 22,
            signal: "VCC",
            chip_pin: 6,
        },
        IcspWire {
            header_pin: 28,
            signal: "GND",
            chip_pin: 5,
        },
    ],
    notes: &[KB90_EXT_PWR_NOTE],
};

// ── Class 0x0e — AT17LV FPGA config EEPROM (T56/T76 only) ────────────────────
//
// Atmel AT17LVxxx serial config EEPROM — an SPI-like serial interface with
// vendor-specific control pins.  The vendor images draw a signal-name box
// (no pin numbers).  Verified against T56ICP014 / T76ICP014.

const AT17_LABELS: &[&str] = &["VCC", "/CS", "RESET/OE", "/SER_EN", "DATA", "CLK", "GND"];

static AT17_T56: IcspWiring = IcspWiring {
    title: "AT17LV FPGA config EEPROM",
    numbered: false,
    chip_labels: AT17_LABELS,
    wires: &[
        IcspWire {
            header_pin: 1,
            signal: "GND",
            chip_pin: 7,
        },
        IcspWire {
            header_pin: 2,
            signal: "CLK",
            chip_pin: 6,
        },
        IcspWire {
            header_pin: 4,
            signal: "DATA",
            chip_pin: 5,
        },
        IcspWire {
            header_pin: 5,
            signal: "/SER_EN",
            chip_pin: 4,
        },
        IcspWire {
            header_pin: 6,
            signal: "RESET/OE",
            chip_pin: 3,
        },
        IcspWire {
            header_pin: 7,
            signal: "/CS",
            chip_pin: 2,
        },
        IcspWire {
            header_pin: 8,
            signal: "VCC",
            chip_pin: 1,
        },
    ],
    notes: &["DATA is bidirectional; /CS and RESET/OE polarities are per the AT17LV datasheet."],
};

static AT17_T76: IcspWiring = IcspWiring {
    title: "AT17LV FPGA config EEPROM",
    numbered: false,
    chip_labels: AT17_LABELS,
    wires: &[
        IcspWire {
            header_pin: 3,
            signal: "DATA",
            chip_pin: 5,
        },
        IcspWire {
            header_pin: 5,
            signal: "CLK",
            chip_pin: 6,
        },
        IcspWire {
            header_pin: 7,
            signal: "/SER_EN",
            chip_pin: 4,
        },
        IcspWire {
            header_pin: 9,
            signal: "/CE",
            chip_pin: 2,
        },
        IcspWire {
            header_pin: 11,
            signal: "RESET/OE",
            chip_pin: 3,
        },
        IcspWire {
            header_pin: 22,
            signal: "VCC",
            chip_pin: 1,
        },
        IcspWire {
            header_pin: 28,
            signal: "GND",
            chip_pin: 7,
        },
    ],
    notes: &["DATA is bidirectional; /CE and RESET/OE polarities are per the AT17LV datasheet."],
};

// ── Class 0x40 — eMMC ISP ────────────────────────────────────────────────────
//
// T56ICP064 wires the 1×8 header directly: 1=GND, 2=CLK, 3=GND, 4=CMD,
// 5=D0, 6=D1, 7=D2, 8=D3.  The target supplies its own power — the VCCQ
// rail (1.8 V or 3.3 V) comes from the BGA adapter or an external supply.
// On T48 the class goes through a dedicated EMMC_ISP driver board instead
// of a direct header pinout, so no per-pin table exists there.

const EMMC_LABELS: &[&str] = &["VCCQ", "VCC", "CLK", "CMD", "D0", "D1", "D2", "D3", "GND"];

static EMMC_T56: IcspWiring = IcspWiring {
    title: "eMMC ISP (4-bit)",
    numbered: false,
    chip_labels: EMMC_LABELS,
    wires: &[
        IcspWire { header_pin: 1, signal: "GND", chip_pin: 9 },
        IcspWire { header_pin: 2, signal: "CLK", chip_pin: 3 },
        IcspWire { header_pin: 3, signal: "GND", chip_pin: 9 },
        IcspWire { header_pin: 4, signal: "CMD", chip_pin: 4 },
        IcspWire { header_pin: 5, signal: "D0",  chip_pin: 5 },
        IcspWire { header_pin: 6, signal: "D1",  chip_pin: 6 },
        IcspWire { header_pin: 7, signal: "D2",  chip_pin: 7 },
        IcspWire { header_pin: 8, signal: "D3",  chip_pin: 8 },
    ],
    notes: &[
        "No VCC on the header — power the target separately. VCCQ is only valid at 3.3 V; at 1.8 V it is high-impedance, so supply VCCQ externally.",
        "Keep the ribbon under ~25 cm; do not separate the three ground/signal wire groups.",
        "eMMC is a BGA device — use a BGA ISP adapter or soldered flying leads.",
    ],
};

/// T76ICP064 — full 8-bit eMMC ISP on the 2×14 header: 5=D0, 8=D1, 7=D2,
/// 3=D3, 6=D4, 2=D5, 10=D6, 9=D7, 12=DS, 14=CMD, 16=CLK, VCC=20/22/24,
/// GND=11/21/26/28, SGND=1/15/27.
const EMMC_T76_LABELS: &[&str] = &[
    "VCCQ (1.8/3.0 V via regulator)",
    "VCC",
    "CLK",
    "CMD",
    "D0",
    "D1",
    "D2",
    "D3",
    "D4",
    "D5",
    "D6",
    "D7",
    "DS (strobe)",
    "VDDI (2.2 µF to GND)",
    "GND",
];

static EMMC_T76: IcspWiring = IcspWiring {
    title: "eMMC ISP (8-bit)",
    numbered: false,
    chip_labels: EMMC_T76_LABELS,
    wires: &[
        IcspWire { header_pin: 2,  signal: "D5",  chip_pin: 10 },
        IcspWire { header_pin: 3,  signal: "D3",  chip_pin: 8 },
        IcspWire { header_pin: 5,  signal: "D0",  chip_pin: 5 },
        IcspWire { header_pin: 6,  signal: "D4",  chip_pin: 9 },
        IcspWire { header_pin: 7,  signal: "D2",  chip_pin: 7 },
        IcspWire { header_pin: 8,  signal: "D1",  chip_pin: 6 },
        IcspWire { header_pin: 9,  signal: "D7",  chip_pin: 12 },
        IcspWire { header_pin: 10, signal: "D6",  chip_pin: 11 },
        IcspWire { header_pin: 11, signal: "GND", chip_pin: 15 },
        IcspWire { header_pin: 12, signal: "DS",  chip_pin: 13 },
        IcspWire { header_pin: 14, signal: "CMD", chip_pin: 4 },
        IcspWire { header_pin: 16, signal: "CLK", chip_pin: 3 },
        IcspWire { header_pin: 20, signal: "VCC", chip_pin: 2 },
        IcspWire { header_pin: 21, signal: "GND", chip_pin: 15 },
        IcspWire { header_pin: 22, signal: "VCC", chip_pin: 2 },
        IcspWire { header_pin: 24, signal: "VCC", chip_pin: 2 },
        IcspWire { header_pin: 26, signal: "GND", chip_pin: 15 },
        IcspWire { header_pin: 28, signal: "GND", chip_pin: 15 },
    ],
    notes: &[
        "Header VCC pins feed the eMMC VCC and the on-header regulator that generates VCCQ (1.8 V/3.0 V) — VCCQ is not a header pin.",
        "D0 is used in 1-bit mode; D1/D2/D3 join it in 4-bit mode.",
        "VDDI needs a 2.2 µF decoupling capacitor to GND. Keep the ribbon under ~25 cm.",
        "Header pins 1/15/27 are SGND (shield); use pins 11/21/26/28 for GND.",
        "eMMC is a BGA device — use a BGA ISP adapter or soldered flying leads.",
    ],
};

/// Look up the verified wiring for a `(model, class)` pair.
///
/// Returns `None` when no verified table exists — the caller should show a
/// pin-numbering-only diagram with an "unverified" notice.
pub fn icsp_wiring(model: ProgrammerModel, class: u8) -> Option<&'static IcspWiring> {
    match (model, class) {
        (ProgrammerModel::Tl866a | ProgrammerModel::Tl866iiPlus, 0x09) => Some(&SPI_NOR_6PIN),
        (ProgrammerModel::T56, 0x09) => Some(&SPI_NOR_T56),
        (ProgrammerModel::T76, 0x09) => Some(&SPI_NOR_T76),
        // Class 0x05 means AT45DB DataFlash in the legacy TL866 DB, but
        // SPI NAND (25-series pinout) in the newer databases.
        (ProgrammerModel::Tl866a, 0x05) => Some(&AT45DB_6PIN),
        (ProgrammerModel::Tl866iiPlus, 0x05) => Some(&SPI_NAND_6PIN),
        (ProgrammerModel::T56, 0x05) => Some(&SPI_NOR_T56),
        (ProgrammerModel::T76, 0x05) => Some(&SPI_NOR_T76),
        // MCU classes — verified identical on the TL866A and TL866II+ 1×6
        // headers (MiniPro ICP001/002/006/007/008 vs Xgpro ICP001/002/006/007/008).
        (ProgrammerModel::Tl866a | ProgrammerModel::Tl866iiPlus, 0x01) => Some(&ATMEL_SPI_6PIN),
        (ProgrammerModel::Tl866a | ProgrammerModel::Tl866iiPlus, 0x02) => Some(&PIC_ICD2_6PIN),
        (ProgrammerModel::Tl866a | ProgrammerModel::Tl866iiPlus, 0x06) => Some(&ATMEGA64_SPI_6PIN),
        (ProgrammerModel::Tl866a | ProgrammerModel::Tl866iiPlus, 0x07) => Some(&AVR_SPI_PB345_6PIN),
        (ProgrammerModel::Tl866a | ProgrammerModel::Tl866iiPlus, 0x08) => Some(&AVR_SPI_PB123_6PIN),
        // TL866II+-only 6-pin classes (INFOIC has no 0x0a/0x0b/0x0c devices).
        (ProgrammerModel::Tl866iiPlus, 0x0a) => Some(&MW93_6PIN),
        (ProgrammerModel::Tl866iiPlus, 0x0b) => Some(&I2C24_6PIN),
        (ProgrammerModel::Tl866iiPlus, 0x0c) => Some(&KB90_6PIN),
        // T48 2×8 zigzag header (T48ICPnnn images).
        (ProgrammerModel::T48, 0x01) => Some(&ATMEL_SPI_T48),
        (ProgrammerModel::T48, 0x02) => Some(&PIC_T48),
        (ProgrammerModel::T48, 0x05) => Some(&SPI_NOR_T48),
        (ProgrammerModel::T48, 0x06) => Some(&ATMEGA64_SPI_T48),
        (ProgrammerModel::T48, 0x07) => Some(&AVR_SPI_PB345_T48),
        (ProgrammerModel::T48, 0x08) => Some(&AVR_SPI_PB123_T48),
        (ProgrammerModel::T48, 0x09) => Some(&SPI_NOR_T48),
        (ProgrammerModel::T48, 0x0a) => Some(&MW93_T48),
        (ProgrammerModel::T48, 0x0b) => Some(&I2C24_T48),
        (ProgrammerModel::T48, 0x0c) => Some(&KB90_T48),
        // T56 1×8 header (T56ICPnnn images).
        (ProgrammerModel::T56, 0x01) => Some(&ATMEL_SPI_T56),
        (ProgrammerModel::T56, 0x02) => Some(&PIC_T56),
        (ProgrammerModel::T56, 0x06) => Some(&ATMEGA64_SPI_T56),
        (ProgrammerModel::T56, 0x07) => Some(&AVR_SPI_PB345_T56),
        (ProgrammerModel::T56, 0x08) => Some(&AVR_SPI_PB123_T56),
        (ProgrammerModel::T56, 0x0a) => Some(&MW93_T56),
        (ProgrammerModel::T56, 0x0b) => Some(&I2C24_T56),
        (ProgrammerModel::T56, 0x0c) => Some(&KB90_T56),
        (ProgrammerModel::T56, 0x0e) => Some(&AT17_T56),
        (ProgrammerModel::T56, 0x40) => Some(&EMMC_T56),
        // T76 2×14 zigzag header (T76ICPnnn images).
        (ProgrammerModel::T76, 0x01) => Some(&ATMEL_SPI_T76),
        (ProgrammerModel::T76, 0x02) => Some(&PIC_T76),
        (ProgrammerModel::T76, 0x06) => Some(&ATMEGA64_SPI_T76),
        (ProgrammerModel::T76, 0x07) => Some(&AVR_SPI_PB345_T76),
        (ProgrammerModel::T76, 0x08) => Some(&AVR_SPI_PB123_T76),
        (ProgrammerModel::T76, 0x0a) => Some(&MW93_T76),
        (ProgrammerModel::T76, 0x0b) => Some(&I2C24_T76),
        (ProgrammerModel::T76, 0x0c) => Some(&KB90_T76),
        (ProgrammerModel::T76, 0x0e) => Some(&AT17_T76),
        (ProgrammerModel::T76, 0x40) => Some(&EMMC_T76),
        _ => None,
    }
}
