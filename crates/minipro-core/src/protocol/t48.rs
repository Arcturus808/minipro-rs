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
    crate::outln!("Testing {} {} pins", pins.len(), name);
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
            crate::outln!(
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
                crate::outln!(
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
    crate::outln!("Testing 53 Logic level {} pins", level);
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
        crate::outln!(
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
                crate::outln!(
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
    crate::outln!(
        "Overcurrent protection detected while testing {} pin driver {}!\u{07}",
        name,
        pin
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
    crate::outln!();
    check_pins(proto, usb, "VCC", CMD_SET_VCC_PIN, VCC_PINS, &mut errors)?;
    crate::outln!();
    check_pins(proto, usb, "GND", CMD_SET_GND_PIN, GND_PINS, &mut errors)?;
    crate::outln!();
    check_logic(proto, usb, 0, &mut errors)?;
    crate::outln!();
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
    crate::outln!();
    if resp[1] != 0 {
        crate::outln!("VPP overcurrent protection is OK.");
    } else {
        crate::outln!("VPP overcurrent protection failed!\u{07}");
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
        crate::outln!("VCC overcurrent protection is OK.");
    } else {
        crate::outln!("VCC overcurrent protection failed!\u{07}");
        errors += 1;
    }

    let mut pkt = [0u8; 10];
    pkt[0] = CMD_RESET_PIN_DRIVERS;
    usb.msg_send(&pkt)?;

    crate::outln!();
    if errors > 0 {
        crate::outln!("Hardware test completed with {} error(s).\u{07}", errors);
    } else {
        crate::outln!("Hardware test completed successfully!");
    }
    Ok(())
}

// ── T48 Firmware update ───────────────────────────────────────────────────────

/// XGECU reset command (used to switch between normal and bootloader modes).
const XGECU_RESET: u8 = 0x3F;

// Bootloader-mode commands (upstream t48.c).
const T48_SWITCH: u8 = 0x3D; // Switch to bootloader (with magic)
const T48_BTLDR_ERASE: u8 = 0x3C; // Erase flash (bootloader mode)
const T48_BTLDR_WRITE: u8 = 0x3B; // Write flash block (bootloader mode)
const T48_BTLDR_MAGIC: u64 = 0xCDEF_89AB_4567_0123; // Bootloader entry magic
const T48_TRAILER_MAGIC: u32 = 0xCDEF_8668; // Last-block trailer magic

// Firmware update file format.
const T48_UPDATE_FILE_VERSION: u32 = 0xf048_0000; // (version & 0xFFFF0000) must equal this
const T48_UPDATE_VERS_MASK: u32 = 0xFFFF_0000;
const T48_BLOCK_SIZE: usize = 0x114; // 276 bytes per firmware block
const T48_BLOCK_MSG_SIZE: usize = 8 + T48_BLOCK_SIZE; // 0x11c = 284
const T48_TRAILER_MSG_SIZE: usize = 264;

/// Validate an `updateT48.dat` image and return the firmware block count.
///
/// Checks the family magic, the exact file size and the block-region CRC —
/// everything that can be verified without hardware.
///
/// File layout (from upstream `t48.c::t48_firmware_update`):
/// ```text
/// [0..4]    version (LE32)   must have (version & 0xffff0000) == 0xf0480000
/// [4..8]    CRC32 of data blocks (LE32)
/// [8..12]   unknown
/// [12..16]  block count N (LE32)
/// [16 .. 16+N*0x114]  N blocks of 276 bytes each
/// Total size = N*0x114 + 16
/// ```
fn validate_update_dat(dat: &[u8]) -> Result<usize> {
    use crate::util::crc_32;

    if dat.len() < 16 {
        return Err(MiniproError::FileFormat("updateT48.dat too short".into()));
    }
    let version = u32::from_le_bytes([dat[0], dat[1], dat[2], dat[3]]);
    let file_crc = u32::from_le_bytes([dat[4], dat[5], dat[6], dat[7]]);
    let n_blocks = u32::from_le_bytes([dat[12], dat[13], dat[14], dat[15]]) as usize;

    if version & T48_UPDATE_VERS_MASK != T48_UPDATE_FILE_VERSION {
        return Err(MiniproError::FileFormat(format!(
            "Unsupported T48 firmware version {:#010x}",
            version
        )));
    }
    let expected = n_blocks * T48_BLOCK_SIZE + 16;
    if dat.len() != expected {
        return Err(MiniproError::FileFormat(format!(
            "updateT48.dat wrong size: got {}, expected {}",
            dat.len(),
            expected
        )));
    }

    // CRC is over the data blocks region (dat[16..end]) with init 0xFFFFFFFF.
    // The result (NOT inverted) must equal file_crc.
    let computed = crc_32(&dat[16..], 0xFFFF_FFFF);
    if computed != file_crc {
        return Err(MiniproError::AlgorithmCrc);
    }
    Ok(n_blocks)
}

/// Parse and flash an `updateT48.dat` firmware image.
///
/// Protocol (upstream `t48.c::t48_firmware_update`, 16-byte messages unless
/// noted; magic at msg[8..16]):
/// 1. If in normal mode: send switch-to-bootloader (`0x3D` + magic), then
///    reset (`0x3F`) and reconnect — the device re-enumerates.
/// 2. Erase flash (`0x3C` + magic).
/// 3. Begin write (`0x3B`/`1` + magic), write each block
///    (`0x3B`/`0`, `0x11c`-byte messages), write the trailer message
///    (`0x3B`/`3`, 264 bytes, `0xCDEF8668` @260), end write (`0x3B`/`2` +
///    magic).
/// 4. Reset back to normal mode (`0x3F`) and reconnect.
pub fn firmware_update_t48(
    handle: &mut crate::handle::MiniproHandle,
    dat: &[u8],
    out: &mut dyn std::io::Write,
    mut progress: Option<&mut dyn FnMut(usize, usize)>,
) -> Result<()> {
    use crate::device::ProgrammerStatus;

    let n_blocks = validate_update_dat(dat)?;

    let version = u32::from_le_bytes([dat[0], dat[1], dat[2], dat[3]]) & 0xFFFF;
    writeln!(
        out,
        "Firmware image OK ({} blocks, version 00.{}.{:02}).",
        n_blocks,
        (version >> 8) & 0xFF,
        version & 0xFF
    )
    .ok();

    // ── Switch to bootloader if necessary ────────────────────────────────────
    if handle.info.status == ProgrammerStatus::Normal {
        write!(out, "Switching to bootloader... ").ok();
        {
            let mut msg = [0u8; 16];
            msg[0] = T48_SWITCH;
            msg[8..16].copy_from_slice(&T48_BTLDR_MAGIC.to_le_bytes());
            handle.usb.msg_send(&msg)?;
            let resp = handle.usb.msg_recv(32)?;
            if resp.first().copied().unwrap_or(1) != 0 {
                return Err(MiniproError::Protocol(
                    "T48 bootloader switch failed".into(),
                ));
            }
        }
        // Reset and reconnect in bootloader mode.
        {
            let mut msg = [0u8; 8];
            msg[0] = XGECU_RESET;
            handle.usb.msg_send(&msg)?;
        }
        handle.reconnect(true)?;
        if handle.info.status != ProgrammerStatus::Bootloader {
            return Err(MiniproError::Protocol(
                "T48 did not enter bootloader mode".into(),
            ));
        }
        writeln!(out, "OK").ok();
    }

    // ── Erase ────────────────────────────────────────────────────────────────
    write!(out, "Erasing... ").ok();
    {
        let mut msg = [0u8; 16];
        msg[0] = T48_BTLDR_ERASE;
        msg[8..16].copy_from_slice(&T48_BTLDR_MAGIC.to_le_bytes());
        handle.usb.msg_send(&msg)?;
        let resp = handle.usb.msg_recv(32)?;
        if resp.get(1).copied().unwrap_or(1) != 0 {
            return Err(MiniproError::Protocol("T48 bootloader erase failed".into()));
        }
    }
    writeln!(out, "OK").ok();

    // ── Begin write sequence ─────────────────────────────────────────────────
    {
        let mut msg = [0u8; 16];
        msg[0] = T48_BTLDR_WRITE;
        msg[1] = 1; // begin
        msg[8..16].copy_from_slice(&T48_BTLDR_MAGIC.to_le_bytes());
        handle.usb.msg_send(&msg)?;
    }

    // ── Flash blocks ─────────────────────────────────────────────────────────
    write!(out, "Reflashing... ").ok();
    for b in 0..n_blocks {
        let blk_off = 16 + b * T48_BLOCK_SIZE;
        let blk = &dat[blk_off..blk_off + T48_BLOCK_SIZE];

        // Block write: 0x11c (= 8 header + 0x114 data) bytes total
        let mut msg = vec![0u8; T48_BLOCK_MSG_SIZE];
        msg[0] = T48_BTLDR_WRITE;
        msg[1] = 0; // data block
        msg[2] = 0x14; // Data Length LSB (0x0114)
        msg[3] = 0x01; // Data Length MSB
        msg[8..8 + T48_BLOCK_SIZE].copy_from_slice(blk);
        handle.usb.msg_send_large(&msg)?;

        let resp = handle.usb.msg_recv(32)?;
        if resp.get(1).copied().unwrap_or(1) != 0 {
            return Err(MiniproError::Protocol(format!(
                "T48 block {} write failed",
                b
            )));
        }

        if let Some(ref mut cb) = progress {
            cb(b + 1, n_blocks);
        }
    }

    // ── Trailer message (upstream "last block") ──────────────────────────────
    {
        let mut msg = [0u8; T48_TRAILER_MSG_SIZE];
        msg[0] = T48_BTLDR_WRITE;
        msg[1] = 3; // last block
        msg[3] = 1;
        msg[5] = 0xFF;
        msg[6] = 3;
        msg[7] = 8;
        msg[260..264].copy_from_slice(&T48_TRAILER_MAGIC.to_le_bytes());
        handle.usb.msg_send_large(&msg)?;

        let resp = handle.usb.msg_recv(32)?;
        if resp.get(1).copied().unwrap_or(1) != 0 {
            return Err(MiniproError::Protocol("T48 trailer write failed".into()));
        }
    }

    // ── End write sequence ───────────────────────────────────────────────────
    {
        let mut msg = [0u8; 16];
        msg[0] = T48_BTLDR_WRITE;
        msg[1] = 2; // end
        msg[8..16].copy_from_slice(&T48_BTLDR_MAGIC.to_le_bytes());
        handle.usb.msg_send(&msg)?;
    }
    writeln!(out, "100%").ok();

    // ── Reset back to normal mode ────────────────────────────────────────────
    write!(out, "Resetting device... ").ok();
    {
        let mut msg = [0u8; 8];
        msg[0] = XGECU_RESET;
        handle.usb.msg_send(&msg)?;
    }
    handle.reconnect(false)?;
    if handle.info.status != ProgrammerStatus::Normal {
        return Err(MiniproError::Protocol(
            "T48 did not return to normal mode after firmware update".into(),
        ));
    }
    writeln!(out, "OK").ok();
    writeln!(out, "Reflash... OK").ok();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::crc_32;

    /// Build a synthetic `updateT48.dat` with `n` blocks and a correct CRC.
    fn make_dat(n_blocks: u32, version: u32, corrupt: bool) -> Vec<u8> {
        let n = n_blocks as usize;
        let mut dat = vec![0u8; 16 + n * T48_BLOCK_SIZE];
        let blocks_region = &mut dat[16..];
        for (i, b) in blocks_region.iter_mut().enumerate() {
            *b = (i & 0xFF) as u8;
        }
        let crc = crc_32(&dat[16..], 0xFFFF_FFFF);
        if corrupt {
            dat[16] ^= 0xFF;
        }
        dat[0..4].copy_from_slice(&version.to_le_bytes());
        dat[4..8].copy_from_slice(&crc.to_le_bytes());
        dat[12..16].copy_from_slice(&n_blocks.to_le_bytes());
        dat
    }

    #[test]
    fn valid_image_passes() {
        let dat = make_dat(2, 0xf048_0123, false);
        assert!(matches!(validate_update_dat(&dat), Ok(2)));
    }

    #[test]
    fn wrong_family_rejected() {
        // TL866II+ family magic must not parse as T48.
        let dat = make_dat(1, 0xf8cc_0123, false);
        assert!(matches!(
            validate_update_dat(&dat),
            Err(MiniproError::FileFormat(_))
        ));
    }

    #[test]
    fn wrong_size_rejected() {
        let mut dat = make_dat(1, 0xf048_0123, false);
        dat.truncate(dat.len() - 1);
        assert!(matches!(
            validate_update_dat(&dat),
            Err(MiniproError::FileFormat(_))
        ));
    }

    #[test]
    fn corrupt_blocks_fail_crc() {
        let dat = make_dat(1, 0xf048_0123, true);
        assert!(matches!(
            validate_update_dat(&dat),
            Err(MiniproError::AlgorithmCrc)
        ));
    }

    #[test]
    fn real_image_layout_matches() {
        // The bundled firmware-T48-01.1.35.dat: 942 blocks, 260008 bytes,
        // version 0xf0480123. Verify the layout math holds for its header.
        assert_eq!(942usize * T48_BLOCK_SIZE + 16, 260008);
    }
}
