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

// ── PIC 14-bit mid-range configs (PIC12F6xx, PIC16F6xx, PIC16F8x) ───────────
//
// Data sources: Microchip datasheets DS41232D (PIC12F635), DS41211B (PIC12F683),
// DS41191D (PIC12F629/675), DS41284E (PIC12F609/615/16F610/616),
// DS40044G (PIC16F627A/628A), DS30487D (PIC16F87/88),
// DS41391D (PIC12F1822/16F182x).

// pic_9: PIC12F635 — 14-bit config word, mask 0x1fff
const PIC_9: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[FuseByteDef {
        name: "word1",
        width: 14,
        fields: &[
            FuseBitField { name: "WURE",   description: "Wake-up Reset enable (1=standard wake-up, 0=wake-up and reset)", bit: 12 },
            FuseBitField { name: "FCMEN",  description: "Fail-Safe Clock Monitor enable (1=enabled, 0=disabled)", bit: 11 },
            FuseBitField { name: "IESO",   description: "Internal/External Switchover mode (1=enabled, 0=disabled)", bit: 10 },
            FuseBitField { name: "BOREN1", description: "Brown-out Reset enable bit 1", bit: 9 },
            FuseBitField { name: "BOREN0", description: "Brown-out Reset enable bit 0 (11=on, 10=on in run only, 01=SW control, 00=off)", bit: 8 },
            FuseBitField { name: "CPD",    description: "Data memory code protection (1=off, 0=on)", bit: 7 },
            FuseBitField { name: "CP",     description: "Program memory code protection (1=off, 0=on)", bit: 6 },
            FuseBitField { name: "MCLRE",  description: "MCLR pin function select (1=MCLR, 0=digital I/O)", bit: 5 },
            FuseBitField { name: "PWRTE",  description: "Power-up Timer enable (1=disabled, 0=enabled)", bit: 4 },
            FuseBitField { name: "WDTE",   description: "Watchdog Timer enable (1=on, 0=off)", bit: 3 },
            FuseBitField { name: "FOSC2",  description: "Oscillator selection bit 2", bit: 2 },
            FuseBitField { name: "FOSC1",  description: "Oscillator selection bit 1", bit: 1 },
            FuseBitField { name: "FOSC0",  description: "Oscillator selection bit 0", bit: 0 },
        ],
    }],
    lock_bytes: &[],
};

// pic_10: PIC12F683 — 14-bit config word, mask 0x0fff
const PIC_10: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[FuseByteDef {
        name: "word1",
        width: 14,
        fields: &[
            FuseBitField { name: "FCMEN",  description: "Fail-Safe Clock Monitor enable (1=enabled, 0=disabled)", bit: 11 },
            FuseBitField { name: "IESO",   description: "Internal/External Switchover mode (1=enabled, 0=disabled)", bit: 10 },
            FuseBitField { name: "BODEN1", description: "Brown-out Detect selection bit 1", bit: 9 },
            FuseBitField { name: "BODEN0", description: "Brown-out Detect selection bit 0 (11=on, 10=on in run, 01=SW control, 00=off)", bit: 8 },
            FuseBitField { name: "CPD",    description: "Data memory code protection (1=off, 0=on)", bit: 7 },
            FuseBitField { name: "CP",     description: "Program memory code protection (1=off, 0=on)", bit: 6 },
            FuseBitField { name: "MCLRE",  description: "GP3/MCLR pin function select (1=MCLR, 0=GP3)", bit: 5 },
            FuseBitField { name: "PWRTE",  description: "Power-up Timer enable (1=disabled, 0=enabled)", bit: 4 },
            FuseBitField { name: "WDTE",   description: "Watchdog Timer enable (1=on, 0=off via SWDTEN)", bit: 3 },
            FuseBitField { name: "FOSC2",  description: "Oscillator selection bit 2", bit: 2 },
            FuseBitField { name: "FOSC1",  description: "Oscillator selection bit 1", bit: 1 },
            FuseBitField { name: "FOSC0",  description: "Oscillator selection bit 0", bit: 0 },
        ],
    }],
    lock_bytes: &[],
};

// pic_11: PIC12F629/675 — 14-bit config word, mask 0x01ff
const PIC_11: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[FuseByteDef {
        name: "word1",
        width: 14,
        fields: &[
            FuseBitField { name: "CPD",   description: "Data memory code protection (1=off, 0=on)", bit: 8 },
            FuseBitField { name: "CP",    description: "Program memory code protection (1=off, 0=on)", bit: 7 },
            FuseBitField { name: "BODEN", description: "Brown-out Detect enable (1=on, 0=off)", bit: 6 },
            FuseBitField { name: "MCLRE", description: "MCLR pin function select (1=MCLR, 0=digital I/O)", bit: 5 },
            FuseBitField { name: "PWRTE", description: "Power-up Timer enable (1=disabled, 0=enabled)", bit: 4 },
            FuseBitField { name: "WDTE",  description: "Watchdog Timer enable (1=on, 0=off)", bit: 3 },
            FuseBitField { name: "FOSC2", description: "Oscillator selection bit 2", bit: 2 },
            FuseBitField { name: "FOSC1", description: "Oscillator selection bit 1", bit: 1 },
            FuseBitField { name: "FOSC0", description: "Oscillator selection bit 0", bit: 0 },
        ],
    }],
    lock_bytes: &[],
};

// pic_12: PIC12F609/615/HV609/HV615, PIC16F610/616 — 14-bit config word, mask 0x03ff
// All variants share the same layout (verified via DS41284E).
const PIC_12: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[FuseByteDef {
        name: "word1",
        width: 14,
        fields: &[
            FuseBitField { name: "BOREN1", description: "Brown-out Reset enable bit 1", bit: 9 },
            FuseBitField { name: "BOREN0", description: "Brown-out Reset enable bit 0 (11=on, 10=on in run, 0x=off)", bit: 8 },
            FuseBitField { name: "IOSCFS", description: "Internal oscillator frequency select (1=8MHz, 0=4MHz)", bit: 7 },
            FuseBitField { name: "CP",     description: "Code protection (1=off, 0=on)", bit: 6 },
            FuseBitField { name: "MCLRE",  description: "MCLR pin function select (1=MCLR, 0=digital I/O)", bit: 5 },
            FuseBitField { name: "PWRTE",  description: "Power-up Timer enable (1=disabled, 0=enabled)", bit: 4 },
            FuseBitField { name: "WDTE",   description: "Watchdog Timer enable (1=on, 0=off)", bit: 3 },
            FuseBitField { name: "FOSC2",  description: "Oscillator selection bit 2", bit: 2 },
            FuseBitField { name: "FOSC1",  description: "Oscillator selection bit 1", bit: 1 },
            FuseBitField { name: "FOSC0",  description: "Oscillator selection bit 0", bit: 0 },
        ],
    }],
    lock_bytes: &[],
};

// pic_13: PIC12F1822/PIC16F182x family — 14-bit, 2 config words
// CONFIG1 (word1): mask 0x3fff, CONFIG2 (word2): mask 0x3713
const PIC_13: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[
        FuseByteDef {
            name: "word1",
            width: 14,
            fields: &[
                FuseBitField { name: "FCMEN",    description: "Fail-Safe Clock Monitor enable (1=enabled, 0=disabled)", bit: 13 },
                FuseBitField { name: "IESO",     description: "Internal/External Switchover (1=enabled, 0=disabled)", bit: 12 },
                FuseBitField { name: "CLKOUTEN", description: "Clock Out enable (1=disabled, 0=enabled)", bit: 11 },
                FuseBitField { name: "BOREN1",   description: "Brown-out Reset enable bit 1", bit: 10 },
                FuseBitField { name: "BOREN0",   description: "Brown-out Reset enable bit 0 (11=on, 10=run only, 01=SW, 00=off)", bit: 9 },
                FuseBitField { name: "CPD",      description: "Data memory code protection (1=off, 0=on)", bit: 8 },
                FuseBitField { name: "CP",       description: "Program memory code protection (1=off, 0=on)", bit: 7 },
                FuseBitField { name: "MCLRE",    description: "MCLR pin function select (1=MCLR, 0=digital I/O)", bit: 6 },
                FuseBitField { name: "PWRTE",    description: "Power-up Timer enable (1=disabled, 0=enabled)", bit: 5 },
                FuseBitField { name: "WDTE1",    description: "Watchdog Timer enable bit 1", bit: 4 },
                FuseBitField { name: "WDTE0",    description: "Watchdog Timer enable bit 0 (11=on, 10=run only, 01=SW, 00=off)", bit: 3 },
                FuseBitField { name: "FOSC2",    description: "Oscillator selection bit 2", bit: 2 },
                FuseBitField { name: "FOSC1",    description: "Oscillator selection bit 1", bit: 1 },
                FuseBitField { name: "FOSC0",    description: "Oscillator selection bit 0", bit: 0 },
            ],
        },
        FuseByteDef {
            name: "word2",
            width: 14,
            fields: &[
                FuseBitField { name: "DEBUG",  description: "Debugger enable (1=disabled, 0=enabled)", bit: 12 },
                FuseBitField { name: "LVP",    description: "Low-voltage Programming enable (1=enabled, 0=disabled)", bit: 11 },
                FuseBitField { name: "STVREN", description: "Stack Overflow/Underflow Reset enable (1=on, 0=off)", bit: 10 },
                FuseBitField { name: "PLLEN",  description: "PLL enable (1=enabled, 0=disabled)", bit: 9 },
                FuseBitField { name: "BORV",   description: "Brown-out Reset voltage select (1=high, 0=low)", bit: 8 },
                FuseBitField { name: "WRT1",   description: "Flash self-write protection bit 1", bit: 1 },
                FuseBitField { name: "WRT0",   description: "Flash self-write protection bit 0 (11=off, 10=half, 01=boot, 00=all)", bit: 0 },
            ],
        },
    ],
    lock_bytes: &[],
};

// pic_21: PIC16F627A — 14-bit config word, mask 0x21ff
const PIC_21: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[FuseByteDef {
        name: "word1",
        width: 14,
        fields: &[
            FuseBitField { name: "CP",    description: "Flash program memory code protection (1=off, 0=on)", bit: 13 },
            FuseBitField { name: "CPD",   description: "Data memory code protection (1=off, 0=on)", bit: 8 },
            FuseBitField { name: "LVP",   description: "Low-voltage Programming enable (1=RB4/PGM, 0=HV on MCLR)", bit: 7 },
            FuseBitField { name: "BOREN", description: "Brown-out Reset enable (1=on, 0=off)", bit: 6 },
            FuseBitField { name: "MCLRE", description: "RA5/MCLR pin function select (1=MCLR, 0=digital I/O)", bit: 5 },
            FuseBitField { name: "FOSC2", description: "Oscillator selection bit 2", bit: 4 },
            FuseBitField { name: "PWRTE", description: "Power-up Timer enable (1=disabled, 0=enabled)", bit: 3 },
            FuseBitField { name: "WDTE",  description: "Watchdog Timer enable (1=on, 0=off)", bit: 2 },
            FuseBitField { name: "FOSC1", description: "Oscillator selection bit 1", bit: 1 },
            FuseBitField { name: "FOSC0", description: "Oscillator selection bit 0", bit: 0 },
        ],
    }],
    lock_bytes: &[],
};

// pic_23: PIC16F628A — 14-bit config word, mask 0x3fff (same layout as pic_21)
const PIC_23: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[FuseByteDef {
        name: "word1",
        width: 14,
        fields: &[
            FuseBitField { name: "CP",    description: "Flash program memory code protection (1=off, 0=on)", bit: 13 },
            FuseBitField { name: "CPD",   description: "Data memory code protection (1=off, 0=on)", bit: 8 },
            FuseBitField { name: "LVP",   description: "Low-voltage Programming enable (1=RB4/PGM, 0=HV on MCLR)", bit: 7 },
            FuseBitField { name: "BOREN", description: "Brown-out Reset enable (1=on, 0=off)", bit: 6 },
            FuseBitField { name: "MCLRE", description: "RA5/MCLR pin function select (1=MCLR, 0=digital I/O)", bit: 5 },
            FuseBitField { name: "FOSC2", description: "Oscillator selection bit 2", bit: 4 },
            FuseBitField { name: "PWRTE", description: "Power-up Timer enable (1=disabled, 0=enabled)", bit: 3 },
            FuseBitField { name: "WDTE",  description: "Watchdog Timer enable (1=on, 0=off)", bit: 2 },
            FuseBitField { name: "FOSC1", description: "Oscillator selection bit 1", bit: 1 },
            FuseBitField { name: "FOSC0", description: "Oscillator selection bit 0", bit: 0 },
        ],
    }],
    lock_bytes: &[],
};

// pic_24: PIC16F88 — 14-bit config word, mask 0x2fcf
const PIC_24: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[FuseByteDef {
        name: "word1",
        width: 14,
        fields: &[
            FuseBitField { name: "CCPMX",  description: "CCP1 pin mux (1=RB0, 0=RB3)", bit: 12 },
            FuseBitField { name: "WRT1",   description: "Flash write protection bit 1", bit: 10 },
            FuseBitField { name: "WRT0",   description: "Flash write protection bit 0 (11=off, 10=256B, 01=2KB, 00=all)", bit: 9 },
            FuseBitField { name: "CPD",    description: "Data EEPROM code protection (1=off, 0=on)", bit: 8 },
            FuseBitField { name: "LVP",    description: "Low-voltage Programming enable (1=RB3/PGM, 0=HV on MCLR)", bit: 7 },
            FuseBitField { name: "BOREN",  description: "Brown-out Reset enable (1=on, 0=off)", bit: 6 },
            FuseBitField { name: "MCLRE",  description: "RA5/MCLR pin function select (1=MCLR, 0=digital I/O)", bit: 5 },
            FuseBitField { name: "PWRTE",  description: "Power-up Timer enable (1=disabled, 0=enabled)", bit: 3 },
            FuseBitField { name: "WDTEN",  description: "Watchdog Timer enable (1=on, 0=off)", bit: 2 },
            FuseBitField { name: "FOSC1",  description: "Oscillator selection bit 1", bit: 1 },
            FuseBitField { name: "FOSC0",  description: "Oscillator selection bit 0", bit: 0 },
        ],
    }],
    lock_bytes: &[],
};

// pic_25: PIC16F88A — 14-bit config word, mask 0x3bff
const PIC_25: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[FuseByteDef {
        name: "word1",
        width: 14,
        fields: &[
            FuseBitField { name: "CCPMX",  description: "CCP1 pin mux (1=RB0, 0=RB3)", bit: 12 },
            FuseBitField { name: "DEBUG",  description: "In-Circuit Debugger enable (1=disabled, 0=enabled)", bit: 11 },
            FuseBitField { name: "WRT1",   description: "Flash write protection bit 1", bit: 10 },
            FuseBitField { name: "CPD",    description: "Data EEPROM code protection (1=off, 0=on)", bit: 8 },
            FuseBitField { name: "LVP",    description: "Low-voltage Programming enable (1=RB3/PGM, 0=HV on MCLR)", bit: 7 },
            FuseBitField { name: "BOREN",  description: "Brown-out Reset enable (1=on, 0=off)", bit: 6 },
            FuseBitField { name: "MCLRE",  description: "RA5/MCLR pin function select (1=MCLR, 0=digital I/O)", bit: 5 },
            FuseBitField { name: "FOSC2",  description: "Oscillator selection bit 2", bit: 4 },
            FuseBitField { name: "PWRTE",  description: "Power-up Timer enable (1=disabled, 0=enabled)", bit: 3 },
            FuseBitField { name: "WDTEN",  description: "Watchdog Timer enable (1=on, 0=off)", bit: 2 },
            FuseBitField { name: "FOSC1",  description: "Oscillator selection bit 1", bit: 1 },
            FuseBitField { name: "FOSC0",  description: "Oscillator selection bit 0", bit: 0 },
        ],
    }],
    lock_bytes: &[],
};

// ── PIC 16-bit PIC18F configs (7 config words, packed as CONFIG_H:CONFIG_L) ──
//
// PIC18F config registers are 8-bit, stored at byte addresses 0x300000-0x30000D.
// XGPro packs them into 7 × 16-bit words: wordN = (CONFIG_H << 8) | CONFIG_L.
//
// Data sources: Microchip DS39632 (PIC18F4550/2550), DS39631 (PIC18F2520/4520),
// DS39609 (PIC18F2420/2520), gputils configuration documentation.
//
// Note: pic_44-48 are defined in the database but no chips reference them.
// pic_28-31 (older PIC18F242/252) have a different 3-bit FOSC layout and are
// skipped pending further datasheet research.

// ── Reusable PIC18F field sets ───────────────────────────────────────────────

/// CONFIG1H: standard non-USB (FOSC3:0, IESO, FCMEN) — mask 0xCF
const PIC18F_CONFIG1H_STD: &[FuseBitField] = &[
    FuseBitField { name: "IESO",   description: "Internal/External Oscillator Switchover (1=enabled, 0=disabled)", bit: 15 },
    FuseBitField { name: "FCMEN",  description: "Fail-Safe Clock Monitor enable (1=enabled, 0=disabled)", bit: 14 },
    FuseBitField { name: "FOSC3",  description: "Oscillator selection bit 3", bit: 11 },
    FuseBitField { name: "FOSC2",  description: "Oscillator selection bit 2", bit: 10 },
    FuseBitField { name: "FOSC1",  description: "Oscillator selection bit 1", bit: 9 },
    FuseBitField { name: "FOSC0",  description: "Oscillator selection bit 0 (0000=LP, 0001=XT, 0010=HS, 0011=RC, 0100=EC, 0101=ECIO, 0110=HSPLL, 0111=RCIO, 1000=INTIO67, 1001=INTIO7)", bit: 8 },
];

/// CONFIG3H: standard non-USB (MCLRE, LPT1OSC, CCP2MX) — mask 0x87
const PIC18F_CONFIG3H_STD: &[FuseBitField] = &[
    FuseBitField { name: "MCLRE",   description: "MCLR pin enable (1=MCLR enabled, 0=RE3 input)", bit: 15 },
    FuseBitField { name: "LPT1OSC", description: "Low-power Timer1 oscillator (1=low power, 0=high power)", bit: 10 },
    FuseBitField { name: "CCP2MX",  description: "CCP2 mux (1=RC1, 0=RB3)", bit: 8 },
];

/// CONFIG3H: USB (MCLRE, LPT1OSC, PBADEN, CCP2MX) — mask 0x87
const PIC18F_CONFIG3H_USB: &[FuseBitField] = &[
    FuseBitField { name: "MCLRE",   description: "MCLR pin enable (1=MCLR enabled, 0=RE3 input)", bit: 15 },
    FuseBitField { name: "LPT1OSC", description: "Low-power Timer1 oscillator (1=low power, 0=high power)", bit: 10 },
    FuseBitField { name: "PBADEN",  description: "PORTB A/D enable (1=analog on reset, 0=digital on reset)", bit: 9 },
    FuseBitField { name: "CCP2MX",  description: "CCP2 mux (1=RC1, 0=RB3)", bit: 8 },
];

/// CONFIG4L: without XINST (DEBUG, LVP, STVREN) — mask 0x85
const PIC18F_CONFIG4L_NO_XINST: &[FuseBitField] = &[
    FuseBitField { name: "DEBUG",  description: "Background debugger enable (1=disabled, 0=enabled)", bit: 7 },
    FuseBitField { name: "LVP",    description: "Low-voltage ICSP enable (1=enabled, 0=disabled)", bit: 2 },
    FuseBitField { name: "STVREN", description: "Stack overflow/underflow reset (1=on, 0=off)", bit: 0 },
];

/// CONFIG4L: with XINST (DEBUG, XINST, LVP, STVREN) — mask 0xC5
const PIC18F_CONFIG4L_XINST: &[FuseBitField] = &[
    FuseBitField { name: "DEBUG",  description: "Background debugger enable (1=disabled, 0=enabled)", bit: 7 },
    FuseBitField { name: "XINST",  description: "Extended instruction set (1=enabled, 0=legacy)", bit: 6 },
    FuseBitField { name: "LVP",    description: "Low-voltage ICSP enable (1=enabled, 0=disabled)", bit: 2 },
    FuseBitField { name: "STVREN", description: "Stack overflow/underflow reset (1=on, 0=off)", bit: 0 },
];

/// CONFIG2H+CONFIG2L combined for standard non-USB — mask 0x1F1F
const PIC18F_WORD2_STD: &[FuseBitField] = &[
    FuseBitField { name: "WDTPS3", description: "Watchdog Timer postscale bit 3", bit: 12 },
    FuseBitField { name: "WDTPS2", description: "Watchdog Timer postscale bit 2", bit: 11 },
    FuseBitField { name: "WDTPS1", description: "Watchdog Timer postscale bit 1", bit: 10 },
    FuseBitField { name: "WDTPS0", description: "Watchdog Timer postscale bit 0 (00000=1:1 to 11111=1:32768)", bit: 9 },
    FuseBitField { name: "WDTEN",  description: "Watchdog Timer enable (1=on, 0=off/SWDTEN control)", bit: 8 },
    FuseBitField { name: "BORV1",  description: "Brown-out Reset voltage bit 1", bit: 4 },
    FuseBitField { name: "BORV0",  description: "Brown-out Reset voltage bit 0 (00=max, 11=min)", bit: 3 },
    FuseBitField { name: "BOREN1", description: "Brown-out Reset enable bit 1", bit: 2 },
    FuseBitField { name: "BOREN0", description: "Brown-out Reset enable bit 0 (00=off, 01=SW control, 10=HW only in run, 11=HW only)", bit: 1 },
    FuseBitField { name: "PWRTEN", description: "Power-up Timer enable (1=disabled, 0=enabled)", bit: 0 },
];

/// CONFIG5H+CONFIG5L: 2 code protection blocks — mask 0xC003
const PIC18F_WORD5_2BLK: &[FuseBitField] = &[
    FuseBitField { name: "CPB", description: "Boot block code protection (1=off, 0=on)", bit: 15 },
    FuseBitField { name: "CPD", description: "Data EEPROM code protection (1=off, 0=on)", bit: 14 },
    FuseBitField { name: "CP0", description: "Code protection block 0 (1=off, 0=on)", bit: 0 },
    FuseBitField { name: "CP1", description: "Code protection block 1 (1=off, 0=on)", bit: 1 },
];

/// CONFIG5H+CONFIG5L: 4 code protection blocks — mask 0xC00F
const PIC18F_WORD5_4BLK: &[FuseBitField] = &[
    FuseBitField { name: "CPB", description: "Boot block code protection (1=off, 0=on)", bit: 15 },
    FuseBitField { name: "CPD", description: "Data EEPROM code protection (1=off, 0=on)", bit: 14 },
    FuseBitField { name: "CP0", description: "Code protection block 0 (1=off, 0=on)", bit: 0 },
    FuseBitField { name: "CP1", description: "Code protection block 1 (1=off, 0=on)", bit: 1 },
    FuseBitField { name: "CP2", description: "Code protection block 2 (1=off, 0=on)", bit: 2 },
    FuseBitField { name: "CP3", description: "Code protection block 3 (1=off, 0=on)", bit: 3 },
];

/// CONFIG6H+CONFIG6L: 2 write protection blocks — mask 0xE003
const PIC18F_WORD6_2BLK: &[FuseBitField] = &[
    FuseBitField { name: "WRTC", description: "Config register write protection (1=off, 0=on)", bit: 15 },
    FuseBitField { name: "WRTB", description: "Boot block write protection (1=off, 0=on)", bit: 14 },
    FuseBitField { name: "WRTD", description: "Data EEPROM write protection (1=off, 0=on)", bit: 13 },
    FuseBitField { name: "WRT0", description: "Write protection block 0 (1=off, 0=on)", bit: 0 },
    FuseBitField { name: "WRT1", description: "Write protection block 1 (1=off, 0=on)", bit: 1 },
];

/// CONFIG6H+CONFIG6L: 4 write protection blocks — mask 0xE00F
const PIC18F_WORD6_4BLK: &[FuseBitField] = &[
    FuseBitField { name: "WRTC", description: "Config register write protection (1=off, 0=on)", bit: 15 },
    FuseBitField { name: "WRTB", description: "Boot block write protection (1=off, 0=on)", bit: 14 },
    FuseBitField { name: "WRTD", description: "Data EEPROM write protection (1=off, 0=on)", bit: 13 },
    FuseBitField { name: "WRT0", description: "Write protection block 0 (1=off, 0=on)", bit: 0 },
    FuseBitField { name: "WRT1", description: "Write protection block 1 (1=off, 0=on)", bit: 1 },
    FuseBitField { name: "WRT2", description: "Write protection block 2 (1=off, 0=on)", bit: 2 },
    FuseBitField { name: "WRT3", description: "Write protection block 3 (1=off, 0=on)", bit: 3 },
];

/// CONFIG7H+CONFIG7L: 2 table read protection blocks — mask 0x4003
const PIC18F_WORD7_2BLK: &[FuseBitField] = &[
    FuseBitField { name: "EBTRB", description: "Boot block table read protection (1=off, 0=on)", bit: 14 },
    FuseBitField { name: "EBTR0", description: "Table read protection block 0 (1=off, 0=on)", bit: 0 },
    FuseBitField { name: "EBTR1", description: "Table read protection block 1 (1=off, 0=on)", bit: 1 },
];

/// CONFIG7H+CONFIG7L: 4 table read protection blocks — mask 0x400F
const PIC18F_WORD7_4BLK: &[FuseBitField] = &[
    FuseBitField { name: "EBTRB", description: "Boot block table read protection (1=off, 0=on)", bit: 14 },
    FuseBitField { name: "EBTR0", description: "Table read protection block 0 (1=off, 0=on)", bit: 0 },
    FuseBitField { name: "EBTR1", description: "Table read protection block 1 (1=off, 0=on)", bit: 1 },
    FuseBitField { name: "EBTR2", description: "Table read protection block 2 (1=off, 0=on)", bit: 2 },
    FuseBitField { name: "EBTR3", description: "Table read protection block 3 (1=off, 0=on)", bit: 3 },
];

// pic_34/pic_38: PIC18F2410/2510 — standard non-USB, no XINST, 2 protection blocks
const PIC_34: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[
        FuseByteDef { name: "word1", width: 16, fields: PIC18F_CONFIG1H_STD },
        FuseByteDef { name: "word2", width: 16, fields: PIC18F_WORD2_STD },
        FuseByteDef { name: "word3", width: 16, fields: PIC18F_CONFIG3H_STD },
        FuseByteDef { name: "word4", width: 16, fields: PIC18F_CONFIG4L_NO_XINST },
        FuseByteDef { name: "word5", width: 16, fields: PIC18F_WORD5_2BLK },
        FuseByteDef { name: "word6", width: 16, fields: PIC18F_WORD6_2BLK },
        FuseByteDef { name: "word7", width: 16, fields: PIC18F_WORD7_2BLK },
    ],
    lock_bytes: &[],
};

// pic_35/pic_39: PIC18F2420/2520 — standard non-USB, with XINST, 2 protection blocks
const PIC_35: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[
        FuseByteDef { name: "word1", width: 16, fields: PIC18F_CONFIG1H_STD },
        FuseByteDef { name: "word2", width: 16, fields: PIC18F_WORD2_STD },
        FuseByteDef { name: "word3", width: 16, fields: PIC18F_CONFIG3H_STD },
        FuseByteDef { name: "word4", width: 16, fields: PIC18F_CONFIG4L_XINST },
        FuseByteDef { name: "word5", width: 16, fields: PIC18F_WORD5_2BLK },
        FuseByteDef { name: "word6", width: 16, fields: PIC18F_WORD6_2BLK },
        FuseByteDef { name: "word7", width: 16, fields: PIC18F_WORD7_2BLK },
    ],
    lock_bytes: &[],
};

// pic_36/pic_40: PIC18F2455/2550 — USB, with XINST, 4 protection blocks
const PIC_36: &FuseConfigDef = &FuseConfigDef {
    fuse_bytes: &[
        FuseByteDef { name: "word1", width: 16, fields: &[
            FuseBitField { name: "IESO",   description: "Internal/External Oscillator Switchover (1=enabled, 0=disabled)", bit: 15 },
            FuseBitField { name: "FCMEN",  description: "Fail-Safe Clock Monitor enable (1=enabled, 0=disabled)", bit: 14 },
            FuseBitField { name: "FOSC3",  description: "Oscillator selection bit 3", bit: 11 },
            FuseBitField { name: "FOSC2",  description: "Oscillator selection bit 2", bit: 10 },
            FuseBitField { name: "FOSC1",  description: "Oscillator selection bit 1", bit: 9 },
            FuseBitField { name: "FOSC0",  description: "Oscillator selection bit 0", bit: 8 },
            FuseBitField { name: "USBDIV",  description: "USB clock selection (1=PLL/2, 0=primary)", bit: 5 },
            FuseBitField { name: "CPUDIV1", description: "System clock postscaler bit 1", bit: 4 },
            FuseBitField { name: "CPUDIV0", description: "System clock postscaler bit 0 (00=OSC1/PLL2, 01=OSC2/PLL3, 10=OSC3/PLL4, 11=OSC4/PLL6)", bit: 3 },
            FuseBitField { name: "PLLDIV2", description: "PLL prescaler bit 2", bit: 2 },
            FuseBitField { name: "PLLDIV1", description: "PLL prescaler bit 1", bit: 1 },
            FuseBitField { name: "PLLDIV0", description: "PLL prescaler bit 0 (000=no prescale, 001=/2, 010=/3, 011=/4, 100=/5, 101=/6, 110=/10, 111=/12)", bit: 0 },
        ]},
        FuseByteDef { name: "word2", width: 16, fields: &[
            FuseBitField { name: "WDTPS3", description: "Watchdog Timer postscale bit 3", bit: 12 },
            FuseBitField { name: "WDTPS2", description: "Watchdog Timer postscale bit 2", bit: 11 },
            FuseBitField { name: "WDTPS1", description: "Watchdog Timer postscale bit 1", bit: 10 },
            FuseBitField { name: "WDTPS0", description: "Watchdog Timer postscale bit 0", bit: 9 },
            FuseBitField { name: "WDTEN",  description: "Watchdog Timer enable (1=on, 0=off/SWDTEN)", bit: 8 },
            FuseBitField { name: "VREGEN", description: "USB voltage regulator enable (1=enabled, 0=disabled)", bit: 5 },
            FuseBitField { name: "BORV1",  description: "Brown-out Reset voltage bit 1", bit: 4 },
            FuseBitField { name: "BORV0",  description: "Brown-out Reset voltage bit 0", bit: 3 },
            FuseBitField { name: "BOREN1", description: "Brown-out Reset enable bit 1", bit: 2 },
            FuseBitField { name: "BOREN0", description: "Brown-out Reset enable bit 0", bit: 1 },
            FuseBitField { name: "PWRTEN", description: "Power-up Timer enable (1=disabled, 0=enabled)", bit: 0 },
        ]},
        FuseByteDef { name: "word3", width: 16, fields: PIC18F_CONFIG3H_USB },
        FuseByteDef { name: "word4", width: 16, fields: PIC18F_CONFIG4L_XINST },
        FuseByteDef { name: "word5", width: 16, fields: PIC18F_WORD5_4BLK },
        FuseByteDef { name: "word6", width: 16, fields: PIC18F_WORD6_4BLK },
        FuseByteDef { name: "word7", width: 16, fields: PIC18F_WORD7_4BLK },
    ],
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
    // PIC 14-bit mid-range configs (PIC12F6xx, PIC16F6xx, PIC16F8x)
    ("pic_9",  PIC_9),
    ("pic_10", PIC_10),
    ("pic_11", PIC_11),
    ("pic_12", PIC_12),
    ("pic_13", PIC_13),
    ("pic_21", PIC_21),
    ("pic_23", PIC_23),
    ("pic_24", PIC_24),
    ("pic_25", PIC_25),
    // PIC 16-bit PIC18F configs (7 config words, packed as CONFIG_H:CONFIG_L)
    ("pic_34", PIC_34),
    ("pic_35", PIC_35),
    ("pic_36", PIC_36),
    ("pic_38", PIC_34),
    ("pic_39", PIC_35),
    ("pic_40", PIC_36),
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
    fn test_pic_configs_have_correct_width() {
        let baseline: &[&str] = &["pic_1", "pic_2", "pic_3", "pic_4", "pic_5", "pic_6", "pic_7", "pic_8"];
        let midrange: &[&str] = &["pic_9", "pic_10", "pic_11", "pic_12", "pic_13", "pic_21", "pic_23", "pic_24", "pic_25"];
        for (name, def) in CONFIG_TABLE {
            if !name.starts_with("pic_") { continue; }
            let expected = if baseline.contains(name) { 12 } else if midrange.contains(name) { 14 } else { continue; };
            for fb in def.fuse_bytes {
                assert_eq!(fb.width, expected, "PIC config {} word {} should be {}-bit", name, fb.name, expected);
            }
        }
    }

    #[test]
    fn test_pic_9_pic12f635_has_wure_and_fcmen() {
        let def = lookup("pic_9", "PIC12F635").unwrap();
        assert_eq!(def.fuse_bytes[0].width, 14);
        let wure = def.fuse_bytes[0].fields.iter().find(|f| f.name == "WURE").unwrap();
        assert_eq!(wure.bit, 12);
        let fcmen = def.fuse_bytes[0].fields.iter().find(|f| f.name == "FCMEN").unwrap();
        assert_eq!(fcmen.bit, 11);
    }

    #[test]
    fn test_pic_11_pic12f629_has_boden_not_boren1() {
        let def = lookup("pic_11", "PIC12F629").unwrap();
        assert_eq!(def.fuse_bytes[0].width, 14);
        // pic_11 uses BODEN (single bit) not BOREN1/BOREN0 (two bits)
        let boden = def.fuse_bytes[0].fields.iter().find(|f| f.name == "BODEN").unwrap();
        assert_eq!(boden.bit, 6);
        assert!(def.fuse_bytes[0].fields.iter().all(|f| f.name != "BOREN1"));
    }

    #[test]
    fn test_pic_12_pic16f610_has_ioscfs() {
        let def = lookup("pic_12", "PIC16F610").unwrap();
        assert_eq!(def.fuse_bytes[0].width, 14);
        let ioscfs = def.fuse_bytes[0].fields.iter().find(|f| f.name == "IOSCFS").unwrap();
        assert_eq!(ioscfs.bit, 7);
    }

    #[test]
    fn test_pic_13_has_two_config_words() {
        let def = lookup("pic_13", "PIC12F1822").unwrap();
        assert_eq!(def.fuse_bytes.len(), 2);
        assert_eq!(def.fuse_bytes[0].name, "word1");
        assert_eq!(def.fuse_bytes[1].name, "word2");
        // word1 should have FCMEN at bit 13
        let fcmen = def.fuse_bytes[0].fields.iter().find(|f| f.name == "FCMEN").unwrap();
        assert_eq!(fcmen.bit, 13);
        // word2 should have LVP at bit 11
        let lvp = def.fuse_bytes[1].fields.iter().find(|f| f.name == "LVP").unwrap();
        assert_eq!(lvp.bit, 11);
    }

    #[test]
    fn test_pic_21_pic16f627a_has_lvp() {
        let def = lookup("pic_21", "PIC16F627A").unwrap();
        assert_eq!(def.fuse_bytes[0].width, 14);
        let lvp = def.fuse_bytes[0].fields.iter().find(|f| f.name == "LVP").unwrap();
        assert_eq!(lvp.bit, 7);
    }

    #[test]
    fn test_pic_24_pic16f88_has_ccpmx() {
        let def = lookup("pic_24", "PIC16F88").unwrap();
        assert_eq!(def.fuse_bytes[0].width, 14);
        let ccpmx = def.fuse_bytes[0].fields.iter().find(|f| f.name == "CCPMX").unwrap();
        assert_eq!(ccpmx.bit, 12);
    }

    #[test]
    fn test_pic_25_pic16f88a_has_debug_bit() {
        let def = lookup("pic_25", "PIC16F88A").unwrap();
        assert_eq!(def.fuse_bytes[0].width, 14);
        // pic_25 has DEBUG at bit 11 (not in pic_24)
        let debug = def.fuse_bytes[0].fields.iter().find(|f| f.name == "DEBUG").unwrap();
        assert_eq!(debug.bit, 11);
    }

    // ── PIC18F tests ─────────────────────────────────────────────────────────

    #[test]
    fn test_pic_39_pic18f2520_has_7_words_16_bit() {
        let def = lookup("pic_39", "PIC18F2520").unwrap();
        assert_eq!(def.fuse_bytes.len(), 7);
        for fb in def.fuse_bytes.iter() {
            assert_eq!(fb.width, 16);
        }
        assert!(def.lock_bytes.is_empty());
    }

    #[test]
    fn test_pic_39_word1_has_fosc_and_ieso() {
        let def = lookup("pic_39", "PIC18F2520").unwrap();
        let word1 = &def.fuse_bytes[0];
        assert_eq!(word1.name, "word1");
        // IESO at bit 15 (CONFIG1H bit 7), FOSC0 at bit 8 (CONFIG1H bit 0)
        let ieso = word1.fields.iter().find(|f| f.name == "IESO").unwrap();
        assert_eq!(ieso.bit, 15);
        let fosc0 = word1.fields.iter().find(|f| f.name == "FOSC0").unwrap();
        assert_eq!(fosc0.bit, 8);
    }

    #[test]
    fn test_pic_39_word4_has_xinst() {
        let def = lookup("pic_39", "PIC18F2520").unwrap();
        let word4 = &def.fuse_bytes[3];
        // XINST at bit 6 (CONFIG4L bit 6)
        let xinst = word4.fields.iter().find(|f| f.name == "XINST").unwrap();
        assert_eq!(xinst.bit, 6);
    }

    #[test]
    fn test_pic_34_word4_no_xinst() {
        let def = lookup("pic_34", "PIC18F2410").unwrap();
        let word4 = &def.fuse_bytes[3];
        // pic_34 does NOT have XINST (mask 0x0085 vs 0x00c5)
        assert!(word4.fields.iter().all(|f| f.name != "XINST"));
        // But it does have DEBUG and LVP
        assert!(word4.fields.iter().any(|f| f.name == "DEBUG"));
        assert!(word4.fields.iter().any(|f| f.name == "LVP"));
    }

    #[test]
    fn test_pic_40_usb_has_plldiv_and_usbdiv() {
        let def = lookup("pic_40", "PIC18F2550").unwrap();
        let word1 = &def.fuse_bytes[0];
        // USB-specific bits in CONFIG1L
        let usbdiv = word1.fields.iter().find(|f| f.name == "USBDIV").unwrap();
        assert_eq!(usbdiv.bit, 5);
        let plldiv0 = word1.fields.iter().find(|f| f.name == "PLLDIV0").unwrap();
        assert_eq!(plldiv0.bit, 0);
    }

    #[test]
    fn test_pic_40_word2_has_vregen() {
        let def = lookup("pic_40", "PIC18F2550").unwrap();
        let word2 = &def.fuse_bytes[1];
        // VREGEN at bit 5 (CONFIG2L bit 5) — USB-specific
        let vregen = word2.fields.iter().find(|f| f.name == "VREGEN").unwrap();
        assert_eq!(vregen.bit, 5);
    }

    #[test]
    fn test_pic_40_word3_has_pbaden() {
        let def = lookup("pic_40", "PIC18F2550").unwrap();
        let word3 = &def.fuse_bytes[2];
        // PBADEN at bit 9 (CONFIG3H bit 1) — USB-specific
        let pbaden = word3.fields.iter().find(|f| f.name == "PBADEN").unwrap();
        assert_eq!(pbaden.bit, 9);
    }

    #[test]
    fn test_pic_39_word5_has_2_protection_blocks() {
        let def = lookup("pic_39", "PIC18F2520").unwrap();
        let word5 = &def.fuse_bytes[4];
        // pic_39 has CP0 and CP1 (2 blocks), not CP2/CP3
        assert!(word5.fields.iter().any(|f| f.name == "CP0"));
        assert!(word5.fields.iter().any(|f| f.name == "CP1"));
        assert!(word5.fields.iter().all(|f| f.name != "CP2"));
        assert!(word5.fields.iter().all(|f| f.name != "CP3"));
    }

    #[test]
    fn test_pic_40_word5_has_4_protection_blocks() {
        let def = lookup("pic_40", "PIC18F2550").unwrap();
        let word5 = &def.fuse_bytes[4];
        // pic_40 has CP0-CP3 (4 blocks)
        assert!(word5.fields.iter().any(|f| f.name == "CP0"));
        assert!(word5.fields.iter().any(|f| f.name == "CP3"));
    }

    #[test]
    fn test_pic_38_shares_pic_34_definition() {
        // pic_38 (PIC18F2510) should have the same layout as pic_34 (PIC18F2410)
        let def34 = lookup("pic_34", "PIC18F2410").unwrap();
        let def38 = lookup("pic_38", "PIC18F2510").unwrap();
        assert_eq!(def34.fuse_bytes.len(), def38.fuse_bytes.len());
        // Both should lack XINST
        let w4_34 = &def34.fuse_bytes[3];
        let w4_38 = &def38.fuse_bytes[3];
        assert!(w4_34.fields.iter().all(|f| f.name != "XINST"));
        assert!(w4_38.fields.iter().all(|f| f.name != "XINST"));
    }
}
