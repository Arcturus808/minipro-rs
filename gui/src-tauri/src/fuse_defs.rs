//! AVR fuse bit definitions for the GUI fuse bit decoder.
//!
//! The XGPro database (`infoic.xml`) stores fuses as monolithic bytes with a
//! mask and default value — it does not break out individual bit fields or
//! provide human-readable names.  This module provides bit-level definitions
//! keyed by the infoic.xml `<config name="...">` attribute, with an optional
//! chip-name prefix override for configs that span multiple architectures.
//!
//! Data sources: avr-libc device headers (`iom*.h`, `iotn*.h`) and Microchip
//! AVR datasheets.  AVR convention: bit = 0 means programmed (active).
//!
//! Configs not listed here fall back to hex-only input in the GUI.

use serde::Serialize;

// ── Data structures ─────────────────────────────────────────────────────────

/// A single bit field within a fuse or lock byte.
#[derive(Debug, Clone, Serialize)]
pub struct FuseBitField {
    /// Field name (e.g., "CKSEL3", "SPIEN").
    pub name: &'static str,
    /// Human-readable description.
    pub description: &'static str,
    /// Bit position (0 = LSB, up to width-1 = MSB).
    pub bit: u8,
}

/// Bit-level definition for a fuse or lock byte (or wider config word for PIC).
#[derive(Debug, Clone, Serialize)]
pub struct FuseByteDef {
    /// Fuse byte/word name (e.g., "lfuse", "word1").
    pub name: &'static str,
    /// Bit width of the config word (8 for AVR, 12/14/16 for PIC).
    pub width: u8,
    /// Bit fields, ordered from MSB to LSB.
    pub fields: &'static [FuseBitField],
}

/// Complete fuse bit definition for a config name + optional chip prefix.
#[derive(Debug, Clone, Serialize)]
pub struct FuseConfigDef {
    /// Fuse byte definitions.
    pub fuse_bytes: &'static [FuseByteDef],
    /// Lock byte definitions (may be empty).
    pub lock_bytes: &'static [FuseByteDef],
}

// ── Lookup ──────────────────────────────────────────────────────────────────

/// Look up fuse bit definitions for a given config name and chip name.
///
/// Tries `(config_name, chip_prefix)` first for configs that span multiple
/// architectures, then falls back to `config_name` alone.  Returns `None` if
/// no definitions exist (caller should fall back to hex-only input).
pub fn lookup(config_name: &str, chip_name: &str) -> Option<&'static FuseConfigDef> {
    // First try chip-specific overrides (for mixed configs).
    let chip_upper = chip_name.to_ascii_uppercase();
    for entry in CHIP_SPECIFIC {
        if entry.config == config_name && chip_upper.starts_with(entry.chip_prefix) {
            return Some(entry.def);
        }
    }
    // Then try config-name-only entries.
    CONFIG_TABLE
        .iter()
        .find(|(name, _)| *name == config_name)
        .map(|(_, def)| *def)
}

// ── Reusable field sets ─────────────────────────────────────────────────────

/// Standard modern AVR lfuse: CKDIV8/CKOUT/SUT/CKSEL
const LFUSE_CKDIV8: &[FuseBitField] = &[
    FuseBitField { name: "CKDIV8", description: "Divide clock by 8", bit: 7 },
    FuseBitField { name: "CKOUT",  description: "Clock output on PB0", bit: 6 },
    FuseBitField { name: "SUT1",   description: "Select start-up time", bit: 5 },
    FuseBitField { name: "SUT0",   description: "Select start-up time", bit: 4 },
    FuseBitField { name: "CKSEL3", description: "Select clock source", bit: 3 },
    FuseBitField { name: "CKSEL2", description: "Select clock source", bit: 2 },
    FuseBitField { name: "CKSEL1", description: "Select clock source", bit: 1 },
    FuseBitField { name: "CKSEL0", description: "Select clock source", bit: 0 },
];

/// Older AVR lfuse with BODEN/BODLEVEL (ATmega8/16/32/64/128/8515/8535)
const LFUSE_BODEN: &[FuseBitField] = &[
    FuseBitField { name: "BODLEVEL", description: "Brown-out trigger level", bit: 7 },
    FuseBitField { name: "BODEN",    description: "Brown-out detect enable", bit: 6 },
    FuseBitField { name: "SUT1",     description: "Select start-up time", bit: 5 },
    FuseBitField { name: "SUT0",     description: "Select start-up time", bit: 4 },
    FuseBitField { name: "CKSEL3",   description: "Select clock source", bit: 3 },
    FuseBitField { name: "CKSEL2",   description: "Select clock source", bit: 2 },
    FuseBitField { name: "CKSEL1",   description: "Select clock source", bit: 1 },
    FuseBitField { name: "CKSEL0",   description: "Select clock source", bit: 0 },
];

/// HFUSE with JTAG: OCDEN/JTAGEN/SPIEN/CKOPT/EESAVE/BOOTSZ/BOOTRST (ATmega16/32/64/128)
const HFUSE_JTAG_CKOPT: &[FuseBitField] = &[
    FuseBitField { name: "OCDEN",   description: "OCD enable", bit: 7 },
    FuseBitField { name: "JTAGEN",  description: "JTAG enable", bit: 6 },
    FuseBitField { name: "SPIEN",   description: "Enable SPI programming", bit: 5 },
    FuseBitField { name: "CKOPT",   description: "Clock oscillator option", bit: 4 },
    FuseBitField { name: "EESAVE",  description: "Preserve EEPROM on chip erase", bit: 3 },
    FuseBitField { name: "BOOTSZ1", description: "Boot size", bit: 2 },
    FuseBitField { name: "BOOTSZ0", description: "Boot size", bit: 1 },
    FuseBitField { name: "BOOTRST", description: "Boot reset vector", bit: 0 },
];

/// HFUSE with JTAG + WDTON (no CKOPT): OCDEN/JTAGEN/SPIEN/WDTON/EESAVE/BOOTSZ/BOOTRST
const HFUSE_JTAG_WDTON: &[FuseBitField] = &[
    FuseBitField { name: "OCDEN",   description: "OCD enable", bit: 7 },
    FuseBitField { name: "JTAGEN",  description: "JTAG enable", bit: 6 },
    FuseBitField { name: "SPIEN",   description: "Enable SPI programming", bit: 5 },
    FuseBitField { name: "WDTON",   description: "Watchdog timer always on", bit: 4 },
    FuseBitField { name: "EESAVE",  description: "Preserve EEPROM on chip erase", bit: 3 },
    FuseBitField { name: "BOOTSZ1", description: "Boot size", bit: 2 },
    FuseBitField { name: "BOOTSZ0", description: "Boot size", bit: 1 },
    FuseBitField { name: "BOOTRST", description: "Boot reset vector", bit: 0 },
];

/// HFUSE with DWEN (no JTAG, no bootloader): RSTDISBL/DWEN/SPIEN/WDTON/EESAVE/BODLEVEL
/// Used by ATmega48/88/168/328P, ATtiny24/44/84, ATtiny25/45/85
const HFUSE_DWEN_BOD: &[FuseBitField] = &[
    FuseBitField { name: "RSTDISBL",  description: "External reset disable", bit: 7 },
    FuseBitField { name: "DWEN",      description: "debugWIRE enable", bit: 6 },
    FuseBitField { name: "SPIEN",     description: "Enable SPI programming", bit: 5 },
    FuseBitField { name: "WDTON",     description: "Watchdog timer always on", bit: 4 },
    FuseBitField { name: "EESAVE",    description: "Preserve EEPROM on chip erase", bit: 3 },
    FuseBitField { name: "BODLEVEL2", description: "Brown-out trigger level", bit: 2 },
    FuseBitField { name: "BODLEVEL1", description: "Brown-out trigger level", bit: 1 },
    FuseBitField { name: "BODLEVEL0", description: "Brown-out trigger level", bit: 0 },
];

/// EFUSE with BODLEVEL only (bits 0-2)
const EFUSE_BODLEVEL: &[FuseBitField] = &[
    FuseBitField { name: "BODLEVEL2", description: "Brown-out trigger level", bit: 2 },
    FuseBitField { name: "BODLEVEL1", description: "Brown-out trigger level", bit: 1 },
    FuseBitField { name: "BODLEVEL0", description: "Brown-out trigger level", bit: 0 },
];

/// EFUSE with SELFPRGEN (bit 0 only)
const EFUSE_SELFPRGEN: &[FuseBitField] = &[
    FuseBitField { name: "SELFPRGEN", description: "Self-programming enable", bit: 0 },
];

/// Standard AVR lock bits
const LOCK_STANDARD: &[FuseBitField] = &[
    FuseBitField { name: "LB1", description: "Lock bit", bit: 1 },
    FuseBitField { name: "LB0", description: "Lock bit", bit: 0 },
];

/// Macro to create a lock byte def inline (avoids the `&[&FuseByteDef]` typing issue).
macro_rules! lock_byte {
    () => {
        FuseByteDef { name: "lock", width: 8, fields: LOCK_STANDARD }
    };
}

// ── Clean config definitions (single architecture) ──────────────────────────

// avr_1: ATtiny12 — 1 fuse byte
const AVR_1: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[FuseByteDef {
        name: "lfuse",
        width: 8,
        fields: &[
            FuseBitField { name: "BODLEVEL", description: "Brown-out trigger level", bit: 7 },
            FuseBitField { name: "BODEN",    description: "Brown-out detect enable", bit: 6 },
            FuseBitField { name: "SPIEN",    description: "Enable SPI programming", bit: 5 },
            FuseBitField { name: "RSTDISBL", description: "External reset disable", bit: 4 },
            FuseBitField { name: "CKSEL3",   description: "Select clock source", bit: 3 },
            FuseBitField { name: "CKSEL2",   description: "Select clock source", bit: 2 },
            FuseBitField { name: "CKSEL1",   description: "Select clock source", bit: 1 },
            FuseBitField { name: "CKSEL0",   description: "Select clock source", bit: 0 },
        ],
    }],
    lock_bytes: &[lock_byte!()],
};

// avr_2: ATtiny15 — 1 fuse byte
const AVR_2: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[FuseByteDef {
        name: "lfuse",
        width: 8,
        fields: &[
            FuseBitField { name: "BODLEVEL", description: "Brown-out trigger level", bit: 7 },
            FuseBitField { name: "BODEN",    description: "Brown-out detect enable", bit: 6 },
            FuseBitField { name: "SPIEN",    description: "Enable SPI programming", bit: 5 },
            FuseBitField { name: "RSTDISBL", description: "External reset disable", bit: 4 },
            FuseBitField { name: "CKSEL1",   description: "Select clock source", bit: 1 },
            FuseBitField { name: "CKSEL0",   description: "Select clock source", bit: 0 },
        ],
    }],
    lock_bytes: &[lock_byte!()],
};

// avr_3: ATtiny28 — 1 fuse byte
const AVR_3: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[FuseByteDef {
        name: "lfuse",
        width: 8,
        fields: &[
            FuseBitField { name: "INTCAP",  description: "Interrupt cap", bit: 4 },
            FuseBitField { name: "CKSEL3",  description: "Select clock source", bit: 3 },
            FuseBitField { name: "CKSEL2",  description: "Select clock source", bit: 2 },
            FuseBitField { name: "CKSEL1",  description: "Select clock source", bit: 1 },
            FuseBitField { name: "CKSEL0",  description: "Select clock source", bit: 0 },
        ],
    }],
    lock_bytes: &[lock_byte!()],
};

// avr_5: ATtiny13 — 2 fuse bytes
const AVR_5: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[
        FuseByteDef {
            name: "lfuse",
            width: 8,
            fields: &[
                FuseBitField { name: "SPIEN",   description: "Enable SPI programming", bit: 7 },
                FuseBitField { name: "EESAVE",  description: "Preserve EEPROM on chip erase", bit: 6 },
                FuseBitField { name: "WDTON",   description: "Watchdog timer always on", bit: 5 },
                FuseBitField { name: "CKDIV8",  description: "Divide clock by 8", bit: 4 },
                FuseBitField { name: "SUT1",    description: "Select start-up time", bit: 3 },
                FuseBitField { name: "SUT0",    description: "Select start-up time", bit: 2 },
                FuseBitField { name: "CKSEL1",  description: "Select clock source", bit: 1 },
                FuseBitField { name: "CKSEL0",  description: "Select clock source", bit: 0 },
            ],
        },
        FuseByteDef {
            name: "hfuse",
            width: 8,
            fields: &[
                FuseBitField { name: "SELFPRGEN", description: "Self-programming enable", bit: 4 },
                FuseBitField { name: "DWEN",      description: "debugWIRE enable", bit: 3 },
                FuseBitField { name: "BODLEVEL1", description: "Brown-out trigger level", bit: 2 },
                FuseBitField { name: "BODLEVEL0", description: "Brown-out trigger level", bit: 1 },
                FuseBitField { name: "RSTDISBL",  description: "External reset disable", bit: 0 },
            ],
        },
    ],
    lock_bytes: &[lock_byte!()],
};

// avr_7: ATtiny26 — 2 fuse bytes
const AVR_7: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[
        FuseByteDef {
            name: "lfuse",
            width: 8,
            fields: &[
                FuseBitField { name: "PLLCK",  description: "PLL clock select", bit: 7 },
                FuseBitField { name: "CKOPT",  description: "Clock oscillator option", bit: 6 },
                FuseBitField { name: "SUT1",   description: "Select start-up time", bit: 5 },
                FuseBitField { name: "SUT0",   description: "Select start-up time", bit: 4 },
                FuseBitField { name: "CKSEL3", description: "Select clock source", bit: 3 },
                FuseBitField { name: "CKSEL2", description: "Select clock source", bit: 2 },
                FuseBitField { name: "CKSEL1", description: "Select clock source", bit: 1 },
                FuseBitField { name: "CKSEL0", description: "Select clock source", bit: 0 },
            ],
        },
        FuseByteDef {
            name: "hfuse",
            width: 8,
            fields: &[
                FuseBitField { name: "RSTDISBL", description: "External reset disable", bit: 4 },
                FuseBitField { name: "SPIEN",    description: "Enable SPI programming", bit: 3 },
                FuseBitField { name: "EESAVE",   description: "Preserve EEPROM on chip erase", bit: 2 },
                FuseBitField { name: "BODLEVEL", description: "Brown-out trigger level", bit: 1 },
                FuseBitField { name: "BODEN",    description: "Brown-out detect enable", bit: 0 },
            ],
        },
    ],
    lock_bytes: &[lock_byte!()],
};

// avr_8: ATmega16 — 2 fuse bytes
const AVR_8: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[
        FuseByteDef { name: "lfuse", width: 8, fields: LFUSE_BODEN },
        FuseByteDef { name: "hfuse", width: 8, fields: HFUSE_JTAG_CKOPT },
    ],
    lock_bytes: &[lock_byte!()],
};

// avr_9: ATmega8515 — 2 fuse bytes
const AVR_9: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[
        FuseByteDef { name: "lfuse", width: 8, fields: LFUSE_BODEN },
        FuseByteDef {
            name: "hfuse",
            width: 8,
            fields: &[
                FuseBitField { name: "S8515C",  description: "ATmega8515 compatibility", bit: 7 },
                FuseBitField { name: "WDTON",   description: "Watchdog timer always on", bit: 6 },
                FuseBitField { name: "SPIEN",   description: "Enable SPI programming", bit: 5 },
                FuseBitField { name: "CKOPT",   description: "Clock oscillator option", bit: 4 },
                FuseBitField { name: "EESAVE",  description: "Preserve EEPROM on chip erase", bit: 3 },
                FuseBitField { name: "BOOTSZ1", description: "Boot size", bit: 2 },
                FuseBitField { name: "BOOTSZ0", description: "Boot size", bit: 1 },
                FuseBitField { name: "BOOTRST", description: "Boot reset vector", bit: 0 },
            ],
        },
    ],
    lock_bytes: &[lock_byte!()],
};

// avr_10: ATmega88/168 — 3 fuse bytes
const AVR_10: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[
        FuseByteDef { name: "lfuse", width: 8, fields: LFUSE_CKDIV8 },
        FuseByteDef { name: "hfuse", width: 8, fields: HFUSE_DWEN_BOD },
        FuseByteDef { name: "efuse", width: 8, fields: EFUSE_SELFPRGEN },
    ],
    lock_bytes: &[lock_byte!()],
};

// avr_11: ATmega328/328P — 3 fuse bytes (same as avr_10 but efuse has BODLEVEL)
const AVR_11: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[
        FuseByteDef { name: "lfuse", width: 8, fields: LFUSE_CKDIV8 },
        FuseByteDef { name: "hfuse", width: 8, fields: HFUSE_DWEN_BOD },
        FuseByteDef { name: "efuse", width: 8, fields: EFUSE_BODLEVEL },
    ],
    lock_bytes: &[lock_byte!()],
};

// avr_12: ATmega162 — 3 fuse bytes
const AVR_12: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[
        FuseByteDef { name: "lfuse", width: 8, fields: LFUSE_CKDIV8 },
        FuseByteDef { name: "hfuse", width: 8, fields: HFUSE_JTAG_WDTON },
        FuseByteDef {
            name: "efuse",
            width: 8,
            fields: &[
                FuseBitField { name: "M161C",     description: "ATmega161 compatibility", bit: 4 },
                FuseBitField { name: "BODLEVEL2", description: "Brown-out trigger level", bit: 3 },
                FuseBitField { name: "BODLEVEL1", description: "Brown-out trigger level", bit: 2 },
                FuseBitField { name: "BODLEVEL0", description: "Brown-out trigger level", bit: 1 },
            ],
        },
    ],
    lock_bytes: &[lock_byte!()],
};

// avr_14: ATmega165/169/325/645 — 3 fuse bytes
const AVR_14: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[
        FuseByteDef { name: "lfuse", width: 8, fields: LFUSE_CKDIV8 },
        FuseByteDef { name: "hfuse", width: 8, fields: HFUSE_JTAG_WDTON },
        FuseByteDef {
            name: "efuse",
            width: 8,
            fields: &[
                FuseBitField { name: "BODLEVEL2", description: "Brown-out trigger level", bit: 3 },
                FuseBitField { name: "BODLEVEL1", description: "Brown-out trigger level", bit: 2 },
                FuseBitField { name: "BODLEVEL0", description: "Brown-out trigger level", bit: 1 },
            ],
        },
    ],
    lock_bytes: &[lock_byte!()],
};

// avr_16: ATmega64/128 — 3 fuse bytes
const AVR_16: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[
        FuseByteDef { name: "lfuse", width: 8, fields: LFUSE_BODEN },
        FuseByteDef { name: "hfuse", width: 8, fields: HFUSE_JTAG_CKOPT },
        FuseByteDef {
            name: "efuse",
            width: 8,
            fields: &[
                FuseBitField { name: "M103C",  description: "ATmega103 compatibility", bit: 1 },
                FuseBitField { name: "WDTON",  description: "Watchdog timer always on", bit: 0 },
            ],
        },
    ],
    lock_bytes: &[lock_byte!()],
};

// avr_18: ATmega32 — 2 fuse bytes (same as ATmega16)
const AVR_18: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[
        FuseByteDef { name: "lfuse", width: 8, fields: LFUSE_BODEN },
        FuseByteDef { name: "hfuse", width: 8, fields: HFUSE_JTAG_CKOPT },
    ],
    lock_bytes: &[lock_byte!()],
};

// ── Chip-specific overrides for mixed configs ───────────────────────────────

// avr_4: ATMEGA48 — 3 fuse bytes (same as avr_10)
const AVR_4_MEGA48: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[
        FuseByteDef { name: "lfuse", width: 8, fields: LFUSE_CKDIV8 },
        FuseByteDef { name: "hfuse", width: 8, fields: HFUSE_DWEN_BOD },
        FuseByteDef { name: "efuse", width: 8, fields: EFUSE_SELFPRGEN },
    ],
    lock_bytes: &[lock_byte!()],
};

// avr_4: ATTINY24/44/84 — 3 fuse bytes
const AVR_4_TINY24: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[
        FuseByteDef { name: "lfuse", width: 8, fields: LFUSE_CKDIV8 },
        FuseByteDef { name: "hfuse", width: 8, fields: HFUSE_DWEN_BOD },
        FuseByteDef { name: "efuse", width: 8, fields: EFUSE_SELFPRGEN },
    ],
    lock_bytes: &[lock_byte!()],
};

// avr_6: ATTINY25/45/85 — 3 fuse bytes
const AVR_6_TINY85: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[
        FuseByteDef { name: "lfuse", width: 8, fields: LFUSE_CKDIV8 },
        FuseByteDef { name: "hfuse", width: 8, fields: HFUSE_DWEN_BOD },
        FuseByteDef { name: "efuse", width: 8, fields: EFUSE_SELFPRGEN },
    ],
    lock_bytes: &[lock_byte!()],
};

// avr_6: ATTINY2313/4313 — 3 fuse bytes (different hfuse bit order from ATtiny85!)
const AVR_6_TINY2313: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[
        FuseByteDef { name: "lfuse", width: 8, fields: LFUSE_CKDIV8 },
        FuseByteDef {
            name: "hfuse",
            width: 8,
            fields: &[
                FuseBitField { name: "DWEN",      description: "debugWIRE enable", bit: 7 },
                FuseBitField { name: "EESAVE",    description: "Preserve EEPROM on chip erase", bit: 6 },
                FuseBitField { name: "SPIEN",     description: "Enable SPI programming", bit: 5 },
                FuseBitField { name: "WDTON",     description: "Watchdog timer always on", bit: 4 },
                FuseBitField { name: "BODLEVEL2", description: "Brown-out trigger level", bit: 3 },
                FuseBitField { name: "BODLEVEL1", description: "Brown-out trigger level", bit: 2 },
                FuseBitField { name: "BODLEVEL0", description: "Brown-out trigger level", bit: 1 },
                FuseBitField { name: "RSTDISBL",  description: "External reset disable", bit: 0 },
            ],
        },
        FuseByteDef { name: "efuse", width: 8, fields: EFUSE_SELFPRGEN },
    ],
    lock_bytes: &[lock_byte!()],
};

// avr_13: ATMEGA128A — 3 fuse bytes (older style, same as avr_16)
const AVR_13_MEGA128: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[
        FuseByteDef { name: "lfuse", width: 8, fields: LFUSE_BODEN },
        FuseByteDef { name: "hfuse", width: 8, fields: HFUSE_JTAG_CKOPT },
        FuseByteDef {
            name: "efuse",
            width: 8,
            fields: &[
                FuseBitField { name: "M103C",  description: "ATmega103 compatibility", bit: 1 },
                FuseBitField { name: "WDTON",  description: "Watchdog timer always on", bit: 0 },
            ],
        },
    ],
    lock_bytes: &[lock_byte!()],
};

// avr_13: ATMEGA164/324/644/1284/329/649 family — 3 fuse bytes (modern style)
const AVR_13_MEGA164: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[
        FuseByteDef { name: "lfuse", width: 8, fields: LFUSE_CKDIV8 },
        FuseByteDef { name: "hfuse", width: 8, fields: HFUSE_JTAG_WDTON },
        FuseByteDef { name: "efuse", width: 8, fields: EFUSE_BODLEVEL },
    ],
    lock_bytes: &[lock_byte!()],
};

// avr_15: ATMEGA8 — 2 fuse bytes
const AVR_15_MEGA8: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[
        FuseByteDef { name: "lfuse", width: 8, fields: LFUSE_BODEN },
        FuseByteDef {
            name: "hfuse",
            width: 8,
            fields: &[
                FuseBitField { name: "RSTDISBL", description: "External reset disable", bit: 7 },
                FuseBitField { name: "WDTON",    description: "Watchdog timer always on", bit: 6 },
                FuseBitField { name: "SPIEN",    description: "Enable SPI programming", bit: 5 },
                FuseBitField { name: "CKOPT",    description: "Clock oscillator option", bit: 4 },
                FuseBitField { name: "EESAVE",   description: "Preserve EEPROM on chip erase", bit: 3 },
                FuseBitField { name: "BOOTSZ1",  description: "Boot size", bit: 2 },
                FuseBitField { name: "BOOTSZ0",  description: "Boot size", bit: 1 },
                FuseBitField { name: "BOOTRST",  description: "Boot reset vector", bit: 0 },
            ],
        },
    ],
    lock_bytes: &[lock_byte!()],
};

// avr_15: ATMEGA8535 — 2 fuse bytes (bit 7 of hfuse is S8535C, not RSTDISBL)
const AVR_15_MEGA8535: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[
        FuseByteDef { name: "lfuse", width: 8, fields: LFUSE_BODEN },
        FuseByteDef {
            name: "hfuse",
            width: 8,
            fields: &[
                FuseBitField { name: "S8535C",  description: "ATmega8535 compatibility", bit: 7 },
                FuseBitField { name: "WDTON",   description: "Watchdog timer always on", bit: 6 },
                FuseBitField { name: "SPIEN",   description: "Enable SPI programming", bit: 5 },
                FuseBitField { name: "CKOPT",   description: "Clock oscillator option", bit: 4 },
                FuseBitField { name: "EESAVE",  description: "Preserve EEPROM on chip erase", bit: 3 },
                FuseBitField { name: "BOOTSZ1", description: "Boot size", bit: 2 },
                FuseBitField { name: "BOOTSZ0", description: "Boot size", bit: 1 },
                FuseBitField { name: "BOOTRST", description: "Boot reset vector", bit: 0 },
            ],
        },
    ],
    lock_bytes: &[lock_byte!()],
};

// avr_17: ATMEGA8U2/16U2/32U2 — 3 fuse bytes
const AVR_17_U2: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[
        FuseByteDef { name: "lfuse", width: 8, fields: LFUSE_CKDIV8 },
        FuseByteDef {
            name: "hfuse",
            width: 8,
            fields: &[
                FuseBitField { name: "DWEN",     description: "debugWIRE enable", bit: 7 },
                FuseBitField { name: "RSTDISBL", description: "External reset disable", bit: 6 },
                FuseBitField { name: "SPIEN",    description: "Enable SPI programming", bit: 5 },
                FuseBitField { name: "WDTON",    description: "Watchdog timer always on", bit: 4 },
                FuseBitField { name: "EESAVE",   description: "Preserve EEPROM on chip erase", bit: 3 },
                FuseBitField { name: "BOOTSZ1",  description: "Boot size", bit: 2 },
                FuseBitField { name: "BOOTSZ0",  description: "Boot size", bit: 1 },
                FuseBitField { name: "BOOTRST",  description: "Boot reset vector", bit: 0 },
            ],
        },
        FuseByteDef {
            name: "efuse",
            width: 8,
            fields: &[
                FuseBitField { name: "HWBE",      description: "Hardware boot enable", bit: 3 },
                FuseBitField { name: "BODLEVEL2", description: "Brown-out trigger level", bit: 2 },
                FuseBitField { name: "BODLEVEL1", description: "Brown-out trigger level", bit: 1 },
                FuseBitField { name: "BODLEVEL0", description: "Brown-out trigger level", bit: 0 },
            ],
        },
    ],
    lock_bytes: &[lock_byte!()],
};

// avr_17: ATMEGA16U4/32U4 — 3 fuse bytes (JTAG instead of DWEN/RSTDISBL)
const AVR_17_U4: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[
        FuseByteDef { name: "lfuse", width: 8, fields: LFUSE_CKDIV8 },
        FuseByteDef {
            name: "hfuse",
            width: 8,
            fields: &[
                FuseBitField { name: "OCDEN",   description: "OCD enable", bit: 7 },
                FuseBitField { name: "JTAGEN",  description: "JTAG enable", bit: 6 },
                FuseBitField { name: "SPIEN",   description: "Enable SPI programming", bit: 5 },
                FuseBitField { name: "WDTON",   description: "Watchdog timer always on", bit: 4 },
                FuseBitField { name: "EESAVE",  description: "Preserve EEPROM on chip erase", bit: 3 },
                FuseBitField { name: "BOOTSZ1", description: "Boot size", bit: 2 },
                FuseBitField { name: "BOOTSZ0", description: "Boot size", bit: 1 },
                FuseBitField { name: "BOOTRST", description: "Boot reset vector", bit: 0 },
            ],
        },
        FuseByteDef {
            name: "efuse",
            width: 8,
            fields: &[
                FuseBitField { name: "HWBE",      description: "Hardware boot enable", bit: 3 },
                FuseBitField { name: "BODLEVEL2", description: "Brown-out trigger level", bit: 2 },
                FuseBitField { name: "BODLEVEL1", description: "Brown-out trigger level", bit: 1 },
                FuseBitField { name: "BODLEVEL0", description: "Brown-out trigger level", bit: 0 },
            ],
        },
    ],
    lock_bytes: &[lock_byte!()],
};

// avr_17: ATMEGA328PB — 3 fuse bytes (BODLEVEL in hfuse, BOOTRST/BOOTSZ in efuse)
const AVR_17_328PB: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[
        FuseByteDef { name: "lfuse", width: 8, fields: LFUSE_CKDIV8 },
        FuseByteDef { name: "hfuse", width: 8, fields: HFUSE_DWEN_BOD },
        FuseByteDef {
            name: "efuse",
            width: 8,
            fields: &[
                FuseBitField { name: "BOOTSZ1", description: "Boot size", bit: 2 },
                FuseBitField { name: "BOOTSZ0", description: "Boot size", bit: 1 },
                FuseBitField { name: "BOOTRST", description: "Boot reset vector", bit: 0 },
            ],
        },
    ],
    lock_bytes: &[lock_byte!()],
};

// ── PIC fuse bit definitions ────────────────────────────────────────────────
//
// PIC config words are wider than AVR fuse bytes (12, 14, or 16 bits depending
// on family).  PIC convention is active-high: bit = 1 means enabled/active.
// No PIC configs have lock bits.
//
// Data sources: Microchip PIC10F/PIC12F5 datasheets (DS40001239F, DS41227E,
// DS41257B, DS41266C, DS41316C).

// pic_1: PIC10F200/204 — 12-bit config word, mask 0x001c
const PIC_1: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[FuseByteDef {
        name: "word1",
        width: 12,
        fields: &[
            FuseBitField { name: "MCLRE", description: "GP3/MCLR pin function select (1=MCLR, 0=GP3)", bit: 4 },
            FuseBitField { name: "CP",    description: "Code protection (1=off, 0=on)", bit: 3 },
            FuseBitField { name: "WDTE",  description: "Watchdog timer enable (1=on, 0=off)", bit: 2 },
        ],
    }],
    lock_bytes: &[],
};

// pic_2: PIC10F202/206 — 12-bit config word, mask 0x001c (same layout as pic_1)
const PIC_2: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[FuseByteDef {
        name: "word1",
        width: 12,
        fields: &[
            FuseBitField { name: "MCLRE", description: "GP3/MCLR pin function select (1=MCLR, 0=GP3)", bit: 4 },
            FuseBitField { name: "CP",    description: "Code protection (1=off, 0=on)", bit: 3 },
            FuseBitField { name: "WDTE",  description: "Watchdog timer enable (1=on, 0=off)", bit: 2 },
        ],
    }],
    lock_bytes: &[],
};

// pic_3: PIC10F220 — 12-bit config word, mask 0x001f
const PIC_3: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[FuseByteDef {
        name: "word1",
        width: 12,
        fields: &[
            FuseBitField { name: "MCLRE",  description: "GP3/MCLR pin function select (1=MCLR, 0=GP3)", bit: 4 },
            FuseBitField { name: "CP",     description: "Code protection (1=off, 0=on)", bit: 3 },
            FuseBitField { name: "WDTE",   description: "Watchdog timer enable (1=on, 0=off)", bit: 2 },
            FuseBitField { name: "MCPU",   description: "Master Clear pull-up enable (1=disabled, 0=enabled)", bit: 1 },
            FuseBitField { name: "IOFSCS", description: "Internal oscillator frequency select (1=8MHz, 0=4MHz)", bit: 0 },
        ],
    }],
    lock_bytes: &[],
};

// pic_4: PIC10F222 — 12-bit config word, mask 0x001f (same layout as pic_3)
const PIC_4: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[FuseByteDef {
        name: "word1",
        width: 12,
        fields: &[
            FuseBitField { name: "MCLRE",  description: "GP3/MCLR pin function select (1=MCLR, 0=GP3)", bit: 4 },
            FuseBitField { name: "CP",     description: "Code protection (1=off, 0=on)", bit: 3 },
            FuseBitField { name: "WDTE",   description: "Watchdog timer enable (1=on, 0=off)", bit: 2 },
            FuseBitField { name: "MCPU",   description: "Master Clear pull-up enable (1=disabled, 0=enabled)", bit: 1 },
            FuseBitField { name: "IOFSCS", description: "Internal oscillator frequency select (1=8MHz, 0=4MHz)", bit: 0 },
        ],
    }],
    lock_bytes: &[],
};

// pic_5: PIC12F510 — 12-bit config word, mask 0x003f
const PIC_5: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[FuseByteDef {
        name: "word1",
        width: 12,
        fields: &[
            FuseBitField { name: "IOSCFS", description: "Internal oscillator frequency select (1=8MHz, 0=4MHz)", bit: 5 },
            FuseBitField { name: "MCLRE",  description: "MCLR/VPP/GP3 pin function select (1=MCLR, 0=GP3)", bit: 4 },
            FuseBitField { name: "CP",     description: "Code protection (1=off, 0=on)", bit: 3 },
            FuseBitField { name: "WDTE",   description: "Watchdog timer enable (1=on, 0=off)", bit: 2 },
            FuseBitField { name: "FOSC1",  description: "Oscillator selection bit 1", bit: 1 },
            FuseBitField { name: "FOSC0",  description: "Oscillator selection bit 0 (00=LP, 01=XT, 10=INTOSC, 11=EXTRC)", bit: 0 },
        ],
    }],
    lock_bytes: &[],
};

// pic_6: PIC12F508 — 12-bit config word, mask 0x001f
const PIC_6: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[FuseByteDef {
        name: "word1",
        width: 12,
        fields: &[
            FuseBitField { name: "MCLRE", description: "GP3/MCLR pin function select (1=MCLR, 0=GP3)", bit: 4 },
            FuseBitField { name: "CP",    description: "Code protection (1=off, 0=on)", bit: 3 },
            FuseBitField { name: "WDTE",  description: "Watchdog timer enable (1=on, 0=off)", bit: 2 },
            FuseBitField { name: "FOSC1", description: "Oscillator selection bit 1", bit: 1 },
            FuseBitField { name: "FOSC0", description: "Oscillator selection bit 0 (00=LP, 01=XT, 10=INTOSC, 11=EXTRC)", bit: 0 },
        ],
    }],
    lock_bytes: &[],
};

// pic_7: PIC12F509 — 12-bit config word, mask 0x001f (same layout as pic_6)
const PIC_7: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[FuseByteDef {
        name: "word1",
        width: 12,
        fields: &[
            FuseBitField { name: "MCLRE", description: "GP3/MCLR pin function select (1=MCLR, 0=GP3)", bit: 4 },
            FuseBitField { name: "CP",    description: "Code protection (1=off, 0=on)", bit: 3 },
            FuseBitField { name: "WDTE",  description: "Watchdog timer enable (1=on, 0=off)", bit: 2 },
            FuseBitField { name: "FOSC1", description: "Oscillator selection bit 1", bit: 1 },
            FuseBitField { name: "FOSC0", description: "Oscillator selection bit 0 (00=LP, 01=XT, 10=INTOSC, 11=EXTRC)", bit: 0 },
        ],
    }],
    lock_bytes: &[],
};

// pic_8: PIC12F519 — 12-bit config word, mask 0x007f
const PIC_8: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[FuseByteDef {
        name: "word1",
        width: 12,
        fields: &[
            FuseBitField { name: "CPDF",   description: "Code protection - Flash data memory (1=off, 0=on)", bit: 6 },
            FuseBitField { name: "IOSCFS", description: "Internal oscillator frequency select (1=8MHz, 0=4MHz)", bit: 5 },
            FuseBitField { name: "MCLRE",  description: "RB3/MCLR pin function select (1=MCLR, 0=RB3)", bit: 4 },
            FuseBitField { name: "CP",     description: "Code protection - User program memory (1=off, 0=on)", bit: 3 },
            FuseBitField { name: "WDTE",   description: "Watchdog timer enable (1=on, 0=off)", bit: 2 },
            FuseBitField { name: "FOSC1",  description: "Oscillator selection bit 1", bit: 1 },
            FuseBitField { name: "FOSC0",  description: "Oscillator selection bit 0 (00=LP, 01=XT, 10=INTRC, 11=EXTRC)", bit: 0 },
        ],
    }],
    lock_bytes: &[],
};

// ── Lookup tables ───────────────────────────────────────────────────────────

/// Config-name-only entries (13 AVR + 8 PIC configs with consistent bit layouts).
static CONFIG_TABLE: &[(&str, &FuseConfigDef)] = &[
    ("avr_1",  AVR_1),
    ("avr_2",  AVR_2),
    ("avr_3",  AVR_3),
    ("avr_5",  AVR_5),
    ("avr_7",  AVR_7),
    ("avr_8",  AVR_8),
    ("avr_9",  AVR_9),
    ("avr_10", AVR_10),
    ("avr_11", AVR_11),
    ("avr_12", AVR_12),
    ("avr_14", AVR_14),
    ("avr_16", AVR_16),
    ("avr_18", AVR_18),
    // PIC baseline 12-bit configs (PIC10F/PIC12F5 family)
    ("pic_1",  PIC_1),
    ("pic_2",  PIC_2),
    ("pic_3",  PIC_3),
    ("pic_4",  PIC_4),
    ("pic_5",  PIC_5),
    ("pic_6",  PIC_6),
    ("pic_7",  PIC_7),
    ("pic_8",  PIC_8),
];

/// Chip-specific overrides for configs that span multiple architectures.
struct ChipSpecific {
    config: &'static str,
    chip_prefix: &'static str,
    def: &'static FuseConfigDef,
}

static CHIP_SPECIFIC: &[ChipSpecific] = &[
    // avr_4: ATmega48 vs ATtiny24/44
    ChipSpecific { config: "avr_4",  chip_prefix: "ATMEGA48",    def: AVR_4_MEGA48 },
    ChipSpecific { config: "avr_4",  chip_prefix: "ATTINY24",    def: AVR_4_TINY24 },
    ChipSpecific { config: "avr_4",  chip_prefix: "ATTINY44",    def: AVR_4_TINY24 },
    // avr_6: ATtiny25/45/85 vs ATtiny2313/4313
    ChipSpecific { config: "avr_6",  chip_prefix: "ATTINY25",    def: AVR_6_TINY85 },
    ChipSpecific { config: "avr_6",  chip_prefix: "ATTINY45",    def: AVR_6_TINY85 },
    ChipSpecific { config: "avr_6",  chip_prefix: "ATTINY85",    def: AVR_6_TINY85 },
    ChipSpecific { config: "avr_6",  chip_prefix: "ATTINY2313",  def: AVR_6_TINY2313 },
    ChipSpecific { config: "avr_6",  chip_prefix: "ATTINY4313",  def: AVR_6_TINY2313 },
    // avr_13: ATmega128A vs ATmega164/324/644/1284/329/649 family
    // NOTE: ATMEGA1284 must come before ATMEGA128 (prefix match order matters).
    ChipSpecific { config: "avr_13", chip_prefix: "ATMEGA1284",  def: AVR_13_MEGA164 },
    ChipSpecific { config: "avr_13", chip_prefix: "ATMEGA128",   def: AVR_13_MEGA128 },
    ChipSpecific { config: "avr_13", chip_prefix: "ATMEGA164",   def: AVR_13_MEGA164 },
    ChipSpecific { config: "avr_13", chip_prefix: "ATMEGA324",   def: AVR_13_MEGA164 },
    ChipSpecific { config: "avr_13", chip_prefix: "ATMEGA329",   def: AVR_13_MEGA164 },
    ChipSpecific { config: "avr_13", chip_prefix: "ATMEGA644",   def: AVR_13_MEGA164 },
    ChipSpecific { config: "avr_13", chip_prefix: "ATMEGA649",   def: AVR_13_MEGA164 },
    // avr_15: ATmega8 vs ATmega8535
    // NOTE: ATMEGA8535 must come before ATMEGA8 (prefix match order matters).
    ChipSpecific { config: "avr_15", chip_prefix: "ATMEGA8535",  def: AVR_15_MEGA8535 },
    ChipSpecific { config: "avr_15", chip_prefix: "ATMEGA8",     def: AVR_15_MEGA8 },
    // avr_17: U2 vs U4 vs 328PB
    ChipSpecific { config: "avr_17", chip_prefix: "ATMEGA8U",    def: AVR_17_U2 },
    ChipSpecific { config: "avr_17", chip_prefix: "ATMEGA16U2",  def: AVR_17_U2 },
    ChipSpecific { config: "avr_17", chip_prefix: "ATMEGA32U2",  def: AVR_17_U2 },
    ChipSpecific { config: "avr_17", chip_prefix: "ATMEGA16U4",  def: AVR_17_U4 },
    ChipSpecific { config: "avr_17", chip_prefix: "ATMEGA32U4",  def: AVR_17_U4 },
    ChipSpecific { config: "avr_17", chip_prefix: "ATMEGA328PB", def: AVR_17_328PB },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lookup_avr_11() {
        let def = lookup("avr_11", "ATMEGA328P").unwrap();
        assert_eq!(def.fuse_bytes.len(), 3);
        assert_eq!(def.fuse_bytes[0].name, "lfuse");
        assert_eq!(def.fuse_bytes[1].name, "hfuse");
        assert_eq!(def.fuse_bytes[2].name, "efuse");
        // Check a known bit: SPIEN at bit 5 of hfuse
        let hfuse = &def.fuse_bytes[1];
        let spien = hfuse.fields.iter().find(|f| f.name == "SPIEN").unwrap();
        assert_eq!(spien.bit, 5);
    }

    #[test]
    fn test_lookup_unknown_config() {
        assert!(lookup("avr_99", "ATMEGA999").is_none());
    }

    #[test]
    fn test_lookup_avr_4_chip_specific() {
        let mega48 = lookup("avr_4", "ATMEGA48").unwrap();
        // ATmega48 hfuse has DWEN at bit 6
        let dwen = mega48.fuse_bytes[1].fields.iter().find(|f| f.name == "DWEN").unwrap();
        assert_eq!(dwen.bit, 6);

        let tiny24 = lookup("avr_4", "ATTINY24").unwrap();
        let dwen = tiny24.fuse_bytes[1].fields.iter().find(|f| f.name == "DWEN").unwrap();
        assert_eq!(dwen.bit, 6);
    }

    #[test]
    fn test_lookup_avr_6_tiny85_vs_tiny2313() {
        // ATtiny85: RSTDISBL at bit 7
        let tiny85 = lookup("avr_6", "ATTINY85").unwrap();
        let rstdisbl = tiny85.fuse_bytes[1].fields.iter().find(|f| f.name == "RSTDISBL").unwrap();
        assert_eq!(rstdisbl.bit, 7);

        // ATtiny2313: RSTDISBL at bit 0
        let tiny2313 = lookup("avr_6", "ATTINY2313").unwrap();
        let rstdisbl = tiny2313.fuse_bytes[1].fields.iter().find(|f| f.name == "RSTDISBL").unwrap();
        assert_eq!(rstdisbl.bit, 0);
    }

    #[test]
    fn test_lookup_avr_13_mega128_vs_mega164() {
        // ATmega128: lfuse has BODEN at bit 6
        let mega128 = lookup("avr_13", "ATMEGA128A").unwrap();
        let boden = mega128.fuse_bytes[0].fields.iter().find(|f| f.name == "BODEN").unwrap();
        assert_eq!(boden.bit, 6);

        // ATmega164: lfuse has CKOUT at bit 6 (no BODEN)
        let mega164 = lookup("avr_13", "ATMEGA164A").unwrap();
        assert!(mega164.fuse_bytes[0].fields.iter().all(|f| f.name != "BODEN"));
        let ckout = mega164.fuse_bytes[0].fields.iter().find(|f| f.name == "CKOUT").unwrap();
        assert_eq!(ckout.bit, 6);
    }

    #[test]
    fn test_lookup_avr_15_mega8_vs_mega8535() {
        let mega8 = lookup("avr_15", "ATMEGA8").unwrap();
        let bit7 = &mega8.fuse_bytes[1].fields[0];
        assert_eq!(bit7.name, "RSTDISBL");

        let mega8535 = lookup("avr_15", "ATMEGA8535").unwrap();
        let bit7 = &mega8535.fuse_bytes[1].fields[0];
        assert_eq!(bit7.name, "S8535C");
    }

    #[test]
    fn test_lookup_avr_17_u2_vs_u4() {
        let u2 = lookup("avr_17", "ATMEGA32U2").unwrap();
        let bit7 = &u2.fuse_bytes[1].fields[0];
        assert_eq!(bit7.name, "DWEN");

        let u4 = lookup("avr_17", "ATMEGA32U4").unwrap();
        let bit7 = &u4.fuse_bytes[1].fields[0];
        assert_eq!(bit7.name, "OCDEN");
    }

    #[test]
    fn test_avr_15_mega8_lfuse_has_boden() {
        let mega8 = lookup("avr_15", "ATMEGA8").unwrap();
        let lfuse = &mega8.fuse_bytes[0];
        assert_eq!(lfuse.fields[0].name, "BODLEVEL");
        assert_eq!(lfuse.fields[1].name, "BODEN");
    }

    #[test]
    fn test_all_avr_configs_have_lock_bytes() {
        for (name, def) in CONFIG_TABLE {
            if name.starts_with("avr_") {
                assert!(!def.lock_bytes.is_empty(), "missing lock bytes for {}", name);
            }
        }
    }

    #[test]
    fn test_all_chip_specific_have_lock_bytes() {
        for entry in CHIP_SPECIFIC {
            assert!(!entry.def.lock_bytes.is_empty(), "missing lock bytes for {}", entry.chip_prefix);
        }
    }

    #[test]
    fn test_avr_5_tiny13_lfuse_layout() {
        let tiny13 = lookup("avr_5", "ATTINY13").unwrap();
        // ATtiny13 lfuse: SPIEN at bit 7, CKSEL0 at bit 0
        let spien = tiny13.fuse_bytes[0].fields.iter().find(|f| f.name == "SPIEN").unwrap();
        assert_eq!(spien.bit, 7);
        let cksel0 = tiny13.fuse_bytes[0].fields.iter().find(|f| f.name == "CKSEL0").unwrap();
        assert_eq!(cksel0.bit, 0);
    }

    #[test]
    fn test_avr_7_tiny26_lfuse_has_pllck() {
        let tiny26 = lookup("avr_7", "ATTINY26").unwrap();
        let pllck = tiny26.fuse_bytes[0].fields.iter().find(|f| f.name == "PLLCK").unwrap();
        assert_eq!(pllck.bit, 7);
    }

    #[test]
    fn test_avr_13_mega1284_uses_mega164_def() {
        // ATMEGA1284 must use the modern mega164 layout, not the legacy mega128 layout.
        // This tests prefix ordering: ATMEGA1284 must be checked before ATMEGA128.
        let mega1284 = lookup("avr_13", "ATMEGA1284P").unwrap();
        // mega164 lfuse has CKOUT at bit 6 (modern), mega128 has BODEN at bit 6 (legacy)
        assert!(mega1284.fuse_bytes[0].fields.iter().any(|f| f.name == "CKOUT"));
        assert!(mega1284.fuse_bytes[0].fields.iter().all(|f| f.name != "BODEN"));
    }

    // ── PIC tests ───────────────────────────────────────────────────────────

    #[test]
    fn test_pic_1_pic10f200_config_word() {
        let def = lookup("pic_1", "PIC10F200").unwrap();
        assert_eq!(def.fuse_bytes.len(), 1);
        assert_eq!(def.fuse_bytes[0].name, "word1");
        assert_eq!(def.fuse_bytes[0].width, 12);
        assert!(def.lock_bytes.is_empty(), "PIC configs should not have lock bytes");
        // MCLRE at bit 4, CP at bit 3, WDTE at bit 2
        let mclre = def.fuse_bytes[0].fields.iter().find(|f| f.name == "MCLRE").unwrap();
        assert_eq!(mclre.bit, 4);
        let cp = def.fuse_bytes[0].fields.iter().find(|f| f.name == "CP").unwrap();
        assert_eq!(cp.bit, 3);
        let wdte = def.fuse_bytes[0].fields.iter().find(|f| f.name == "WDTE").unwrap();
        assert_eq!(wdte.bit, 2);
    }

    #[test]
    fn test_pic_3_pic10f220_has_mcpu_iofscs() {
        let def = lookup("pic_3", "PIC10F220").unwrap();
        assert_eq!(def.fuse_bytes[0].width, 12);
        // pic_3 has MCPU at bit 1 and IOFSCS at bit 0 (not in pic_1/pic_2)
        let mcpu = def.fuse_bytes[0].fields.iter().find(|f| f.name == "MCPU").unwrap();
        assert_eq!(mcpu.bit, 1);
        let iofscs = def.fuse_bytes[0].fields.iter().find(|f| f.name == "IOFSCS").unwrap();
        assert_eq!(iofscs.bit, 0);
    }

    #[test]
    fn test_pic_5_pic12f510_has_ioscfs_and_fosc() {
        let def = lookup("pic_5", "PIC12F510").unwrap();
        assert_eq!(def.fuse_bytes[0].width, 12);
        // IOSCFS at bit 5, FOSC1 at bit 1, FOSC0 at bit 0
        let ioscfs = def.fuse_bytes[0].fields.iter().find(|f| f.name == "IOSCFS").unwrap();
        assert_eq!(ioscfs.bit, 5);
        let fosc1 = def.fuse_bytes[0].fields.iter().find(|f| f.name == "FOSC1").unwrap();
        assert_eq!(fosc1.bit, 1);
        let fosc0 = def.fuse_bytes[0].fields.iter().find(|f| f.name == "FOSC0").unwrap();
        assert_eq!(fosc0.bit, 0);
    }

    #[test]
    fn test_pic_8_pic12f519_has_cpdf() {
        let def = lookup("pic_8", "PIC12F519").unwrap();
        assert_eq!(def.fuse_bytes[0].width, 12);
        // CPDF at bit 6 (unique to pic_8, not in pic_5/pic_6/pic_7)
        let cpdf = def.fuse_bytes[0].fields.iter().find(|f| f.name == "CPDF").unwrap();
        assert_eq!(cpdf.bit, 6);
    }

    #[test]
    fn test_pic_configs_have_no_lock_bytes() {
        for (name, def) in CONFIG_TABLE {
            if name.starts_with("pic_") {
                assert!(def.lock_bytes.is_empty(), "PIC config {} should not have lock bytes", name);
            }
        }
    }

    #[test]
    fn test_pic_configs_have_12_bit_width() {
        for (name, def) in CONFIG_TABLE {
            if name.starts_with("pic_") {
                for fb in def.fuse_bytes {
                    assert_eq!(fb.width, 12, "PIC config {} word {} should be 12-bit", name, fb.name);
                }
            }
        }
    }
}
