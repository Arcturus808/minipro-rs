//! T48 protocol.
//!
//! The T48 shares the TL866II+ command set for chip operations, so the
//! protocol implementation is re-used via [`T48Protocol`] (a flag on
//! `Tl866iiPlusProtocol` branches where behavior diverges).  Its
//! pin-driver domain is genuinely different, though: 56 pin drivers
//! behind the 40-pin ZIF + ICSP connector, bit-packed pin-state reads,
//! and its own self-test pin tables — ported here from upstream `t48.c`.

pub use super::tl866iiplus::Tl866iiPlusProtocol as T48Protocol;

use super::{tl866iiplus::Tl866iiPlusProtocol, Protocol};
use crate::{
    error::{MiniproError, Result},
    usb::UsbDevice,
};

/// Minimum firmware version for the T48.
///
/// The T48 runs its own firmware line (e.g. `0x127` = 01.1.39) that is
/// versioned independently of the TL866II+ — it must not share
/// [`super::tl866iiplus::MIN_FIRMWARE`] (`0x255`), which rejects every
/// released T48 firmware at connect time.  Upstream C minipro expects
/// `0x127` and only warns on mismatch; it never refuses to connect, and
/// every released T48 firmware supports the shared command set.  So the
/// minimum is `0` (no hard floor), matching the TL866A/CS handling.
pub const MIN_FIRMWARE_T48: u32 = 0;

// ── Pin-driver commands (upstream t48.c) ─────────────────────────────────────
// These share opcodes with the TL866II+ pin-driver commands *except* the
// pull-up/pull-down pair, which is swapped on the T48.

const CMD_RESET_PIN_DRIVERS: u8 = 0x2D;
const CMD_SET_VCC_PIN: u8 = 0x2E;
const CMD_SET_VPP_PIN: u8 = 0x2F;
const CMD_SET_GND_PIN: u8 = 0x30;
const CMD_SET_PULLUPS: u8 = 0x31;
const CMD_SET_PULLDOWNS: u8 = 0x32;
const CMD_READ_PINS: u8 = 0x35;
const CMD_SET_OUT: u8 = 0x36;

/// Number of pin drivers (upstream `T48_NPINS`).
const T48_NPINS: usize = 56;

/// A ZIF pin descriptor: driver pin number (1-based), byte offset within
/// the set-pin message, and bitmask within that byte.
#[derive(Copy, Clone)]
struct ZifPin {
    pin: u8,
    byte: usize,
    mask: u8,
}

/// 17 VPP-capable pins (upstream `vpp_pins`).
const VPP_PINS: &[ZifPin] = &[
    ZifPin {
        pin: 31,
        byte: 8,
        mask: 0x01,
    },
    ZifPin {
        pin: 30,
        byte: 8,
        mask: 0x02,
    },
    ZifPin {
        pin: 10,
        byte: 8,
        mask: 0x04,
    },
    ZifPin {
        pin: 9,
        byte: 8,
        mask: 0x08,
    },
    ZifPin {
        pin: 4,
        byte: 8,
        mask: 0x10,
    },
    ZifPin {
        pin: 3,
        byte: 8,
        mask: 0x20,
    },
    ZifPin {
        pin: 2,
        byte: 8,
        mask: 0x40,
    },
    ZifPin {
        pin: 1,
        byte: 8,
        mask: 0x80,
    },
    ZifPin {
        pin: 32,
        byte: 9,
        mask: 0x01,
    },
    ZifPin {
        pin: 33,
        byte: 9,
        mask: 0x02,
    },
    ZifPin {
        pin: 34,
        byte: 9,
        mask: 0x04,
    },
    ZifPin {
        pin: 36,
        byte: 9,
        mask: 0x08,
    },
    ZifPin {
        pin: 37,
        byte: 9,
        mask: 0x10,
    },
    ZifPin {
        pin: 38,
        byte: 9,
        mask: 0x20,
    },
    ZifPin {
        pin: 39,
        byte: 9,
        mask: 0x40,
    },
    ZifPin {
        pin: 40,
        byte: 9,
        mask: 0x80,
    },
    // J1
    ZifPin {
        pin: 41,
        byte: 12,
        mask: 0x01,
    },
];

/// 34 VCC-capable pins (upstream `vcc_pins`).
const VCC_PINS: &[ZifPin] = &[
    ZifPin {
        pin: 1,
        byte: 8,
        mask: 0x01,
    },
    ZifPin {
        pin: 2,
        byte: 8,
        mask: 0x02,
    },
    ZifPin {
        pin: 3,
        byte: 8,
        mask: 0x04,
    },
    ZifPin {
        pin: 4,
        byte: 8,
        mask: 0x08,
    },
    ZifPin {
        pin: 5,
        byte: 8,
        mask: 0x10,
    },
    ZifPin {
        pin: 6,
        byte: 8,
        mask: 0x20,
    },
    ZifPin {
        pin: 7,
        byte: 8,
        mask: 0x40,
    },
    ZifPin {
        pin: 8,
        byte: 8,
        mask: 0x80,
    },
    ZifPin {
        pin: 9,
        byte: 9,
        mask: 0x80,
    },
    ZifPin {
        pin: 10,
        byte: 9,
        mask: 0x40,
    },
    ZifPin {
        pin: 11,
        byte: 9,
        mask: 0x20,
    },
    ZifPin {
        pin: 12,
        byte: 9,
        mask: 0x10,
    },
    ZifPin {
        pin: 13,
        byte: 9,
        mask: 0x08,
    },
    ZifPin {
        pin: 14,
        byte: 9,
        mask: 0x04,
    },
    ZifPin {
        pin: 15,
        byte: 9,
        mask: 0x02,
    },
    ZifPin {
        pin: 16,
        byte: 9,
        mask: 0x01,
    },
    ZifPin {
        pin: 25,
        byte: 10,
        mask: 0x01,
    },
    ZifPin {
        pin: 26,
        byte: 10,
        mask: 0x02,
    },
    ZifPin {
        pin: 27,
        byte: 10,
        mask: 0x04,
    },
    ZifPin {
        pin: 28,
        byte: 10,
        mask: 0x08,
    },
    ZifPin {
        pin: 29,
        byte: 10,
        mask: 0x10,
    },
    ZifPin {
        pin: 30,
        byte: 10,
        mask: 0x20,
    },
    ZifPin {
        pin: 31,
        byte: 10,
        mask: 0x40,
    },
    ZifPin {
        pin: 32,
        byte: 10,
        mask: 0x80,
    },
    ZifPin {
        pin: 33,
        byte: 11,
        mask: 0x01,
    },
    ZifPin {
        pin: 34,
        byte: 11,
        mask: 0x02,
    },
    ZifPin {
        pin: 35,
        byte: 11,
        mask: 0x04,
    },
    ZifPin {
        pin: 36,
        byte: 11,
        mask: 0x08,
    },
    ZifPin {
        pin: 37,
        byte: 11,
        mask: 0x10,
    },
    ZifPin {
        pin: 38,
        byte: 11,
        mask: 0x20,
    },
    ZifPin {
        pin: 39,
        byte: 11,
        mask: 0x40,
    },
    ZifPin {
        pin: 40,
        byte: 11,
        mask: 0x80,
    },
    // J13
    ZifPin {
        pin: 53,
        byte: 16,
        mask: 0x01,
    },
    // J14
    ZifPin {
        pin: 54,
        byte: 16,
        mask: 0x02,
    },
];

/// 35 GND-capable pins (upstream `gnd_pins`).
const GND_PINS: &[ZifPin] = &[
    ZifPin {
        pin: 1,
        byte: 8,
        mask: 0x80,
    },
    ZifPin {
        pin: 2,
        byte: 8,
        mask: 0x40,
    },
    ZifPin {
        pin: 3,
        byte: 8,
        mask: 0x20,
    },
    ZifPin {
        pin: 4,
        byte: 8,
        mask: 0x10,
    },
    ZifPin {
        pin: 5,
        byte: 8,
        mask: 0x08,
    },
    ZifPin {
        pin: 6,
        byte: 8,
        mask: 0x04,
    },
    ZifPin {
        pin: 7,
        byte: 8,
        mask: 0x02,
    },
    ZifPin {
        pin: 8,
        byte: 8,
        mask: 0x01,
    },
    ZifPin {
        pin: 9,
        byte: 9,
        mask: 0x80,
    },
    ZifPin {
        pin: 10,
        byte: 9,
        mask: 0x40,
    },
    ZifPin {
        pin: 11,
        byte: 9,
        mask: 0x20,
    },
    ZifPin {
        pin: 12,
        byte: 9,
        mask: 0x10,
    },
    ZifPin {
        pin: 13,
        byte: 9,
        mask: 0x08,
    },
    ZifPin {
        pin: 14,
        byte: 9,
        mask: 0x04,
    },
    ZifPin {
        pin: 15,
        byte: 9,
        mask: 0x02,
    },
    ZifPin {
        pin: 16,
        byte: 9,
        mask: 0x01,
    },
    ZifPin {
        pin: 18,
        byte: 10,
        mask: 0x80,
    },
    ZifPin {
        pin: 20,
        byte: 10,
        mask: 0x40,
    },
    ZifPin {
        pin: 25,
        byte: 10,
        mask: 0x20,
    },
    ZifPin {
        pin: 27,
        byte: 10,
        mask: 0x10,
    },
    ZifPin {
        pin: 29,
        byte: 10,
        mask: 0x08,
    },
    ZifPin {
        pin: 30,
        byte: 10,
        mask: 0x04,
    },
    ZifPin {
        pin: 31,
        byte: 10,
        mask: 0x02,
    },
    ZifPin {
        pin: 32,
        byte: 10,
        mask: 0x01,
    },
    ZifPin {
        pin: 33,
        byte: 11,
        mask: 0x80,
    },
    ZifPin {
        pin: 34,
        byte: 11,
        mask: 0x40,
    },
    ZifPin {
        pin: 35,
        byte: 11,
        mask: 0x20,
    },
    ZifPin {
        pin: 36,
        byte: 11,
        mask: 0x10,
    },
    ZifPin {
        pin: 37,
        byte: 11,
        mask: 0x08,
    },
    ZifPin {
        pin: 38,
        byte: 11,
        mask: 0x04,
    },
    ZifPin {
        pin: 39,
        byte: 11,
        mask: 0x02,
    },
    ZifPin {
        pin: 40,
        byte: 11,
        mask: 0x01,
    },
    // J6
    ZifPin {
        pin: 46,
        byte: 16,
        mask: 0x01,
    },
    // J8
    ZifPin {
        pin: 48,
        byte: 16,
        mask: 0x02,
    },
    // J16
    ZifPin {
        pin: 56,
        byte: 16,
        mask: 0x04,
    },
];

// Indices into the pin tables used by the OVC tail (upstream VPP1/VCC1/GND1).
const VPP1: usize = 7;
const VCC1: usize = 0;
const GND1: usize = 0;

/// True for 0-based driver indices with no IO — unconnected ICSP pins
/// (pins 52, 54, 56; upstream `zif_no_io`).
fn zif_no_io(pin: usize) -> bool {
    matches!(pin, 51 | 53 | 55)
}

/// Logic state of 0-based driver `pin` in a READ_PINS response.
/// Pin states are bit-packed starting at `msg[8]` (upstream `zif_pin_state`).
fn zif_pin_state(msg: &[u8], pin: usize) -> u8 {
    (msg[8 + (pin >> 3)] >> (pin & 7)) & 1
}

/// `t48_reset_state` — reset all pin drivers (48-byte message).
fn reset_state(usb: &UsbDevice) -> Result<()> {
    let mut msg = [0u8; 48];
    msg[0] = CMD_RESET_PIN_DRIVERS;
    usb.msg_send(&msg)
}

/// `t48_set_input_and_pullup` — all drivers to input with pull-ups.
fn set_input_and_pullup(usb: &UsbDevice) -> Result<()> {
    let mut msg = [0u8; 48];
    msg[0] = CMD_SET_PULLUPS;
    usb.msg_send(&msg)
}

/// `t48_set_input_and_pulldown` — all drivers to input with pull-downs.
fn set_input_and_pulldown(usb: &UsbDevice) -> Result<()> {
    let mut msg = [0u8; 48];
    msg[0] = CMD_SET_PULLDOWNS;
    usb.msg_send(&msg)
}

/// READ_PINS request/response cycle shared by the pin and logic checks.
/// Returns the full response; T48 replies are ~16 bytes (header + 7
/// bit-packed pin-state bytes at offset 8).
fn read_pins(usb: &UsbDevice) -> Result<Vec<u8>> {
    let mut pkt = [0u8; 8];
    pkt[0] = CMD_READ_PINS;
    usb.msg_send(&pkt)?;
    let resp = usb.msg_recv(64)?;
    // Pin-state bytes occupy indices 8..15 — anything shorter can't be parsed.
    if resp.len() < 15 {
        return Err(MiniproError::ResponseTooShort {
            expected: 15,
            actual: resp.len(),
        });
    }
    Ok(resp)
}

/// `t48_check_pins` — drive each pin in `pins` via `cmd` and check it
/// reads back at `check_level`; also checks no *other* driver follows
/// (short detection).
fn check_pins(
    proto: &Tl866iiPlusProtocol,
    usb: &UsbDevice,
    name: &str,
    cmd: u8,
    pins: &[ZifPin],
    errors: &mut u32,
) -> Result<()> {
    reset_state(usb)?;
    // GND drivers are tested against pull-ups (expect level 0); VPP/VCC
    // against pull-downs (expect level 1).
    let check_level = if cmd == CMD_SET_GND_PIN {
        set_input_and_pullup(usb)?;
        0
    } else {
        set_input_and_pulldown(usb)?;
        1
    };
    eprintln!("Testing {} {} pins", pins.len(), name);
    for p in pins {
        let mut msg = [0u8; 64];
        msg[0] = cmd;
        msg[p.byte] = p.mask;
        usb.msg_send(&msg)?;

        let resp = read_pins(usb)?;
        if resp[1] != 0 {
            overcurrent_abort(proto, usb, name, p.pin)?;
        }
        let idx = (p.pin - 1) as usize;
        if !zif_no_io(idx) {
            let ok = zif_pin_state(&resp, idx) == check_level;
            eprintln!(
                "{} pin {} state is {}",
                name,
                p.pin,
                if ok { "Good" } else { "Bad" }
            );
            if !ok {
                *errors += 1;
            }
        }
        // Check all other pins do *not* match the expected level.
        for j in 0..T48_NPINS {
            if j == idx || zif_no_io(j) {
                continue;
            }
            if zif_pin_state(&resp, j) == check_level {
                eprintln!(
                    "Unexpected state of pin {} when testing {} pin {}: shorted?",
                    j + 1,
                    name,
                    p.pin
                );
                *errors += 1;
            }
        }
    }
    Ok(())
}

/// `t48_check_logic` — drive every connected driver to `level` one at a
/// time and verify readback plus shorts on the other pins.
fn check_logic(
    proto: &Tl866iiPlusProtocol,
    usb: &UsbDevice,
    level: u8,
    errors: &mut u32,
) -> Result<()> {
    eprintln!("Testing 53 Logic level {} pins", level);
    for i in 0..T48_NPINS {
        // Unconnected ICSP pins can't be driven — skip them.
        if zif_no_io(i) {
            continue;
        }
        if level != 0 {
            set_input_and_pulldown(usb)?;
        } else {
            set_input_and_pullup(usb)?;
        }
        let mut msg = [0u8; 64];
        msg[0] = CMD_SET_OUT;
        msg[1] = level;
        msg[4] = i as u8;
        usb.msg_send(&msg[..8])?;

        let resp = read_pins(usb)?;
        if resp[1] != 0 {
            overcurrent_abort(proto, usb, &format!("logic {}", level), i as u8 + 1)?;
        }
        let ok = zif_pin_state(&resp, i) == level;
        eprintln!(
            "Logic {} pin {} state is {}",
            level,
            i + 1,
            if ok { "Good" } else { "Bad" }
        );
        if !ok {
            *errors += 1;
        }
        for j in 0..T48_NPINS {
            if j == i || zif_no_io(j) {
                continue;
            }
            if zif_pin_state(&resp, j) == level {
                eprintln!(
                    "Unexpected state of pin {} when testing logic {} pin {}: shorted?",
                    j + 1,
                    level,
                    i + 1
                );
                *errors += 1;
            }
        }
    }
    Ok(())
}

/// On an overcurrent flag, reset the pin drivers and end the transaction
/// (upstream behaviour), then report a hard error.
fn overcurrent_abort(
    proto: &Tl866iiPlusProtocol,
    usb: &UsbDevice,
    name: &str,
    pin: u8,
) -> Result<()> {
    let mut rst = [0u8; 10];
    rst[0] = CMD_RESET_PIN_DRIVERS;
    usb.msg_send(&rst)?;
    proto.end_transaction(usb)?;
    eprintln!(
        "Overcurrent protection detected while testing {} pin driver {}!\u{07}",
        name, pin
    );
    Err(MiniproError::Protocol(format!(
        "{} overcurrent on pin {}",
        name, pin
    )))
}

/// `t48_hardware_check` — pin-driver, logic-level and overcurrent
/// self-test.  Returns `Err` only on transport failures or overcurrent
/// trips; per-pin faults are counted and reported at the end, matching
/// upstream.
pub(super) fn hardware_check(proto: &Tl866iiPlusProtocol, usb: &UsbDevice) -> Result<()> {
    let mut errors: u32 = 0;

    check_pins(proto, usb, "VPP", CMD_SET_VPP_PIN, VPP_PINS, &mut errors)?;
    eprintln!();
    check_pins(proto, usb, "VCC", CMD_SET_VCC_PIN, VCC_PINS, &mut errors)?;
    eprintln!();
    check_pins(proto, usb, "GND", CMD_SET_GND_PIN, GND_PINS, &mut errors)?;
    eprintln!();
    check_logic(proto, usb, 0, &mut errors)?;
    eprintln!();
    check_logic(proto, usb, 1, &mut errors)?;

    // Overcurrent verification: cross-drive VPP1 against GND1 and expect
    // the OVC flag to trip, then the same for VCC1.
    let mut msg = [0u8; 64];
    msg[0] = CMD_RESET_PIN_DRIVERS;
    usb.msg_send(&msg[..10])?;

    let mut pkt = [0u8; 10];
    pkt[0] = CMD_SET_GND_PIN;
    pkt[GND_PINS[GND1].byte] = GND_PINS[GND1].mask;
    usb.msg_send(&pkt)?;

    let mut pkt = [0u8; 10];
    pkt[0] = CMD_SET_VPP_PIN;
    pkt[VPP_PINS[VPP1].byte] = VPP_PINS[VPP1].mask;
    usb.msg_send(&pkt)?;

    let resp = read_pins(usb)?;
    eprintln!();
    if resp[1] != 0 {
        eprintln!("VPP overcurrent protection is OK.");
    } else {
        eprintln!("VPP overcurrent protection failed!\u{07}");
        errors += 1;
    }

    let mut pkt = [0u8; 10];
    pkt[0] = CMD_RESET_PIN_DRIVERS;
    usb.msg_send(&pkt)?;

    let mut pkt = [0u8; 10];
    pkt[0] = CMD_SET_GND_PIN;
    pkt[GND_PINS[GND1].byte] = GND_PINS[GND1].mask;
    usb.msg_send(&pkt)?;

    let mut pkt = [0u8; 10];
    pkt[0] = CMD_SET_VCC_PIN;
    pkt[VCC_PINS[VCC1].byte] = VCC_PINS[VCC1].mask;
    usb.msg_send(&pkt)?;

    let resp = read_pins(usb)?;
    if resp[1] != 0 {
        eprintln!("VCC overcurrent protection is OK.");
    } else {
        eprintln!("VCC overcurrent protection failed!\u{07}");
        errors += 1;
    }

    let mut pkt = [0u8; 10];
    pkt[0] = CMD_RESET_PIN_DRIVERS;
    usb.msg_send(&pkt)?;

    eprintln!();
    if errors > 0 {
        eprintln!("Hardware test completed with {} error(s).\u{07}", errors);
    } else {
        eprintln!("Hardware test completed successfully!");
    }
    Ok(())
}
