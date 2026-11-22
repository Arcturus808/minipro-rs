//! Host-side bit-bang read path for bipolar and mask-programmed PROMs.
//!
//! Port of upstream `prom.c` + the `bitbang.c` pin helpers.  These devices
//! carry `custom_protocol` (high bit of `protocol_id`) with
//! `protocol_id == CP_PROM`; instead of a firmware algorithm the host
//! toggles ZIF pins directly through the `set_zif_*`/`set_pin_drivers`
//! primitives.  Only TL866II+ and T48 implement those primitives (upstream
//! is identical) — `MiniproHandle::begin_transaction` rejects custom-protocol
//! devices on other models before reaching this module.
//!
//! PROMs are read-only: fuse blowing needs current pulses the hardware
//! cannot generate (that's why `write_buffer_size` is 0 in the database and
//! upstream marks them `MP_READ_ONLY`).

use crate::{
    device::Device,
    error::{MiniproError, Result},
    protocol::{DataSet, PinDriver, Protocol},
    usb::UsbDevice,
};

/// `protocol_id` for bipolar/mask PROM entries (`CP_PROM` upstream).
pub const CP_PROM: u8 = 0x01;

/// `variant` high bit selects the mask-PROM layout table (programmable
/// CE/CS pins — Hitachi HN43xxx, 2316-class mask ROMs).
const HITACHI_MASK_PROM_MASK: u32 = 0x80;

/// Socket pins the host can address (40-pin ZIF; T48's wider driver domain
/// is handled inside its protocol impl).
const ZIF_PINS: usize = 40;

// Direction byte encodings (upstream minipro.h).
const PIN_DIRECTION_IN: u8 = 0x01;
const PIN_DIRECTION_OUT: u8 = 0x00;
const PIN_PULLUP: u8 = 0x80;

/// Layout for an ordinary bipolar PROM (`prom_t` upstream).
struct PromLayout {
    /// Ground pins (device pin numbers).
    gnd: &'static [u8],
    /// VCC pins.
    vcc: &'static [u8],
    /// Data bus pins, LSB first.
    data: &'static [u8],
    /// Address bus pins, LSB first.
    addr: &'static [u8],
    /// Active-low chip-enables.
    ce_lo: &'static [u8],
    /// Active-high chip-enables.
    ce_hi: &'static [u8],
    /// AND-mask of significant output bits (4-bit PROMs: 0x0f).
    compare_mask: u16,
}

/// Layout for a mask-programmed ROM (`mask_prom_t` upstream) — CE and CS
/// polarities are mask-programmed into the silicon, so reads probe every
/// combination until data comes back non-0xFF.
struct MaskPromLayout {
    gnd: &'static [u8],
    vcc: &'static [u8],
    data: &'static [u8],
    addr: &'static [u8],
    /// Programmable chip-enable pins.
    ce: &'static [u8],
    /// Programmable chip-select pins.
    cs: &'static [u8],
    compare_mask: u16,
}

/// Which layout table a device indexes into.
enum Layout<'a> {
    Normal(&'a PromLayout),
    Mask(&'a MaskPromLayout),
}

/// Bipolar PROM layouts, indexed by `device.variant` (upstream `prom_table`,
/// zero-terminated lists replaced by slices).
static PROM_TABLE: &[PromLayout] = &[
    // Type 0: 32x8 DIP16 (74S188, 74S288, 82S23, 82S123)
    PromLayout {
        gnd: &[8],
        vcc: &[16],
        data: &[1, 2, 3, 4, 5, 6, 7, 9],
        addr: &[10, 11, 12, 13, 14],
        ce_lo: &[15],
        ce_hi: &[],
        compare_mask: 0xff,
    },
    // Type 1: 256x4 DIP16 (74S287, 74S387, 82S126, 82S129)
    PromLayout {
        gnd: &[8],
        vcc: &[16],
        data: &[12, 11, 10, 9],
        addr: &[5, 6, 7, 4, 3, 2, 1, 15],
        ce_lo: &[13, 14],
        ce_hi: &[],
        compare_mask: 0x0f,
    },
    // Type 2: 512x4 DIP16 (74S570, 74S571, 82S130, 82S131)
    PromLayout {
        gnd: &[8],
        vcc: &[16],
        data: &[12, 11, 10, 9],
        addr: &[5, 6, 7, 4, 3, 2, 1, 15, 14],
        ce_lo: &[13],
        ce_hi: &[],
        compare_mask: 0x0f,
    },
    // Type 3: 1024x4 DIP18 (74S572, 74S573, 82S136, 82S137)
    PromLayout {
        gnd: &[9],
        vcc: &[18],
        data: &[14, 13, 12, 11],
        addr: &[5, 6, 7, 4, 3, 2, 1, 17, 16, 15],
        ce_lo: &[8, 10],
        ce_hi: &[],
        compare_mask: 0x0f,
    },
    // Type 4: 2048x4 DIP18 (82S184, 82S185)
    PromLayout {
        gnd: &[9],
        vcc: &[18],
        data: &[14, 13, 12, 11],
        addr: &[5, 6, 7, 4, 3, 2, 1, 17, 16, 15, 8],
        ce_lo: &[10],
        ce_hi: &[],
        compare_mask: 0x0f,
    },
    // Type 5: 256x8 DIP20 (82S135)
    PromLayout {
        gnd: &[10],
        vcc: &[20],
        data: &[6, 7, 8, 9, 11, 12, 13, 14],
        addr: &[1, 2, 3, 4, 5, 17, 18, 19],
        ce_lo: &[15, 16],
        ce_hi: &[],
        compare_mask: 0xff,
    },
    // Type 6: 512x8 DIP20 (74S472, 74S473, 82S146, 82S147)
    PromLayout {
        gnd: &[10],
        vcc: &[20],
        data: &[6, 7, 8, 9, 11, 12, 13, 14],
        addr: &[1, 2, 3, 4, 5, 16, 17, 18, 19],
        ce_lo: &[15],
        ce_hi: &[],
        compare_mask: 0xff,
    },
    // Type 7: 512x8 DIP24 (74S474, 74S475, 82S140, 82S141)
    PromLayout {
        gnd: &[12],
        vcc: &[24],
        data: &[9, 10, 11, 13, 14, 15, 16, 17],
        addr: &[8, 7, 6, 5, 4, 3, 2, 1, 23],
        ce_lo: &[21, 20],
        ce_hi: &[19, 18],
        compare_mask: 0xff,
    },
    // Type 8: 1024x8 DIP24 (74S478, 74S479, 82S180, 82S181)
    PromLayout {
        gnd: &[12],
        vcc: &[24],
        data: &[9, 10, 11, 13, 14, 15, 16, 17],
        addr: &[8, 7, 6, 5, 4, 3, 2, 1, 23, 22],
        ce_lo: &[21, 20],
        ce_hi: &[19, 18],
        compare_mask: 0xff,
    },
    // Type 9: 2048x8 DIP24 (82S190, 82S191)
    PromLayout {
        gnd: &[12],
        vcc: &[24],
        data: &[9, 10, 11, 13, 14, 15, 16, 17],
        addr: &[8, 7, 6, 5, 4, 3, 2, 1, 23, 22, 21],
        ce_lo: &[20],
        ce_hi: &[19, 18],
        compare_mask: 0xff,
    },
    // Type 10: 4096x8 DIP24 (82S321)
    PromLayout {
        gnd: &[12],
        vcc: &[24],
        data: &[9, 10, 11, 13, 14, 15, 16, 17],
        addr: &[8, 7, 6, 5, 4, 3, 2, 1, 23, 22, 21, 19],
        ce_lo: &[20],
        ce_hi: &[18],
        compare_mask: 0xff,
    },
    // Type 11: 8192x8 DIP24 (82HS641/A/B)
    PromLayout {
        gnd: &[12],
        vcc: &[24],
        data: &[9, 10, 11, 13, 14, 15, 16, 17],
        addr: &[8, 7, 6, 5, 4, 3, 2, 1, 23, 22, 21, 19, 18],
        ce_lo: &[20],
        ce_hi: &[],
        compare_mask: 0xff,
    },
    // Type 12: 8192x8 DIP24 (D2364C)
    PromLayout {
        gnd: &[12],
        vcc: &[24],
        data: &[9, 10, 11, 13, 14, 15, 16, 17],
        addr: &[8, 7, 6, 5, 4, 3, 2, 1, 23, 22, 19, 18, 21],
        ce_lo: &[20],
        ce_hi: &[],
        compare_mask: 0xff,
    },
];

/// Mask-ROM layouts, indexed by `device.variant & !0x80`
/// (upstream `mask_prom_table`).
static MASK_PROM_TABLE: &[MaskPromLayout] = &[
    // Type 0x80: 16384x8 DIP28 w/ programmable CE (HN43128P)
    MaskPromLayout {
        gnd: &[14],
        vcc: &[28],
        data: &[11, 12, 13, 15, 16, 17, 18, 19],
        addr: &[10, 9, 8, 7, 6, 5, 4, 3, 25, 24, 21, 20, 23, 2],
        ce: &[22],
        cs: &[27, 26],
        compare_mask: 0xff,
    },
    // Type 0x81: 2048x8 DIP24 w/ programmable CE (2316)
    MaskPromLayout {
        gnd: &[12],
        vcc: &[24],
        data: &[9, 10, 11, 13, 14, 15, 16, 17],
        addr: &[8, 7, 6, 5, 4, 3, 2, 1, 23, 22, 19],
        ce: &[],
        cs: &[20, 18, 21],
        compare_mask: 0xff,
    },
];

/// True when the device is a bit-bang PROM (`custom_protocol` +
/// `CP_PROM`).  Other custom-protocol values exist in the wild but have no
/// upstream implementation either — they fail at [`layout_for`].
pub fn is_prom(device: &Device) -> bool {
    device.flags.custom_protocol && device.protocol_id == CP_PROM
}

/// Look up the pin layout for a custom-protocol device.
fn layout_for(device: &Device) -> Result<Layout<'static>> {
    if !device.flags.custom_protocol {
        return Err(MiniproError::Protocol(format!(
            "{} is not a custom-protocol device",
            device.name
        )));
    }
    if device.protocol_id != CP_PROM {
        return Err(MiniproError::Protocol(format!(
            "unsupported custom protocol {:#04x} ({})",
            device.protocol_id, device.name
        )));
    }
    if device.variant & HITACHI_MASK_PROM_MASK != 0 {
        let idx = (device.variant & !HITACHI_MASK_PROM_MASK) as usize;
        return MASK_PROM_TABLE.get(idx).map(Layout::Mask).ok_or_else(|| {
            MiniproError::Protocol(format!(
                "unknown mask PROM variant {:#04x} ({})",
                device.variant, device.name
            ))
        });
    }
    PROM_TABLE
        .get(device.variant as usize)
        .map(Layout::Normal)
        .ok_or_else(|| {
            MiniproError::Protocol(format!(
                "unknown PROM variant {:#04x} ({})",
                device.variant, device.name
            ))
        })
}

/// Compare mask the PROM tables assign for this device — applied to the
/// `Device` at transaction start (upstream sets `device->compare_mask`
/// inside `prom_init`).
pub fn compare_mask_for(device: &Device) -> Result<u16> {
    Ok(match layout_for(device)? {
        Layout::Normal(l) => l.compare_mask,
        Layout::Mask(l) => l.compare_mask,
    })
}

/// True when the buffer reads as all-0xFF (upstream `is_empty`) — the
/// signature of an inactive chip select on pulled-up data pins.
fn is_empty(buf: &[u8]) -> bool {
    buf.iter().all(|b| *b == 0xff)
}

/// Map a device pin number (1-based, on the DIP package) to its ZIF socket
/// pin — packages are bottom-aligned in the socket (upstream `PIN()`).
/// Returns a 0-based `zif`/`drivers` index; `None` when the computed pin
/// falls outside the socket (defensive: malformed `pin_count` in a custom
/// `infoic.xml` must not panic).
fn zif_pin(dev_pin: u8, pin_count: u8) -> Option<usize> {
    let zif = if dev_pin <= pin_count / 2 {
        dev_pin
    } else {
        (ZIF_PINS as u8)
            .checked_sub(pin_count)?
            .checked_add(dev_pin)?
    };
    let idx = (zif as usize).checked_sub(1)?;
    (idx < ZIF_PINS).then_some(idx)
}

/// Write `value` into every listed pin's direction/state slot
/// (upstream `set_io_pins`).
fn set_io_pins(zif: &mut [u8; ZIF_PINS], pins: &[u8], value: u8, pin_count: u8) {
    for &p in pins {
        if let Some(i) = zif_pin(p, pin_count) {
            zif[i] = value;
        }
    }
}

/// Route power pins in the driver array (upstream `set_pwr_pins` for
/// GND/VCC — no PROM uses VPP routing).
fn set_pwr_pins(drivers: &mut [PinDriver; ZIF_PINS], pins: &[u8], pin_count: u8, vcc: bool) {
    for &p in pins {
        if let Some(i) = zif_pin(p, pin_count) {
            if vcc {
                drivers[i].vcc = true;
            } else {
                drivers[i].gnd = true;
            }
        }
    }
}

/// Drive an LSB-first value onto a pin list (upstream `set_bits`).
fn set_bits(zif: &mut [u8; ZIF_PINS], pins: &[u8], mut value: u32, pin_count: u8) {
    for &p in pins {
        if let Some(i) = zif_pin(p, pin_count) {
            zif[i] = (value & 1) as u8;
        }
        value >>= 1;
    }
}

/// Reassemble an LSB-first value from pin states (upstream `get_bits`).
fn get_bits(zif: &[u8], pins: &[u8], pin_count: u8) -> u32 {
    let mut value = 0u32;
    for (bit, &p) in pins.iter().enumerate() {
        let set = zif_pin(p, pin_count)
            .and_then(|i| zif.get(i))
            .copied()
            .unwrap_or(0);
        if set != 0 {
            value |= 1 << bit;
        }
    }
    value
}

/// `prom_init` — configure pin directions, drive enables inactive, route
/// power, then set voltages (voltages last: pin-driver changes reset them).
pub fn begin(usb: &UsbDevice, p: &dyn Protocol, device: &Device) -> Result<()> {
    let pin_count = device.package_details.pin_count;
    let mut zif_dir = [PIN_DIRECTION_IN; ZIF_PINS];
    let mut zif_state = [0u8; ZIF_PINS];
    let mut drivers = [PinDriver::default(); ZIF_PINS];

    let (gnd, vcc, addr): (&[u8], &[u8], &[u8]) = match layout_for(device)? {
        Layout::Normal(l) => {
            set_io_pins(&mut zif_dir, l.ce_lo, PIN_DIRECTION_OUT, pin_count);
            set_io_pins(&mut zif_dir, l.ce_hi, PIN_DIRECTION_OUT, pin_count);
            // Enables inactive until data pins are configured as inputs.
            set_io_pins(&mut zif_state, l.ce_lo, 1, pin_count);
            set_io_pins(&mut zif_state, l.ce_hi, 0, pin_count);
            (l.gnd, l.vcc, l.addr)
        }
        Layout::Mask(l) => {
            set_io_pins(&mut zif_dir, l.ce, PIN_DIRECTION_OUT, pin_count);
            set_io_pins(&mut zif_dir, l.cs, PIN_DIRECTION_OUT, pin_count);
            set_io_pins(&mut zif_state, l.ce, 1, pin_count);
            set_io_pins(&mut zif_state, l.cs, 1, pin_count);
            (l.gnd, l.vcc, l.addr)
        }
    };
    set_io_pins(&mut zif_dir, addr, PIN_DIRECTION_OUT, pin_count);

    p.set_zif_direction(usb, &zif_dir)?;
    p.set_zif_state(usb, &zif_state)?;

    set_pwr_pins(&mut drivers, gnd, pin_count, false);
    set_pwr_pins(&mut drivers, vcc, pin_count, true);
    p.set_pin_drivers(usb, &drivers)?;
    p.set_voltages(usb, device.voltages.vcc, device.voltages.vpp)
}

/// `prom_terminate` — reset all pin drivers.
pub fn end(usb: &UsbDevice, p: &dyn Protocol) -> Result<()> {
    p.reset_state(usb)
}

/// `prom_read` entry point (`bb_read_block` → `prom_read`/`prom_read_mask_prom`).
pub fn read_block(
    usb: &UsbDevice,
    p: &dyn Protocol,
    device: &Device,
    ds: &mut DataSet,
) -> Result<()> {
    match layout_for(device)? {
        Layout::Normal(l) => read_normal(usb, p, device, l, ds),
        Layout::Mask(l) => read_mask(usb, p, device, l, ds),
    }
}

/// Ordinary PROM read: data bus as inputs w/ pull-ups, enables active,
/// then per-address drive address bus → sample data bus.
fn read_normal(
    usb: &UsbDevice,
    p: &dyn Protocol,
    device: &Device,
    l: &PromLayout,
    ds: &mut DataSet,
) -> Result<()> {
    let pin_count = device.package_details.pin_count;
    let mut zif_dir = [PIN_DIRECTION_IN; ZIF_PINS];
    let mut zif_state = [0u8; ZIF_PINS];

    set_io_pins(&mut zif_dir, l.addr, PIN_DIRECTION_OUT, pin_count);
    set_io_pins(&mut zif_dir, l.ce_lo, PIN_DIRECTION_OUT, pin_count);
    set_io_pins(&mut zif_dir, l.ce_hi, PIN_DIRECTION_OUT, pin_count);
    set_io_pins(
        &mut zif_dir,
        l.data,
        PIN_DIRECTION_IN | PIN_PULLUP,
        pin_count,
    );

    // Enables active.
    set_io_pins(&mut zif_state, l.ce_lo, 0, pin_count);
    set_io_pins(&mut zif_state, l.ce_hi, 1, pin_count);

    p.set_zif_direction(usb, &zif_dir)?;
    p.set_zif_state(usb, &zif_state)?;

    for (i, byte) in ds.data.iter_mut().enumerate() {
        set_bits(&mut zif_state, l.addr, ds.address + i as u32, pin_count);
        p.set_zif_state(usb, &zif_state)?;
        let zif = p.get_zif_state(usb)?;
        *byte = get_bits(&zif, l.data, pin_count) as u8;
    }

    // Enables inactive on the way out (upstream tail of prom_read).
    set_io_pins(&mut zif_state, l.ce_lo, 1, pin_count);
    set_io_pins(&mut zif_state, l.ce_hi, 0, pin_count);
    p.set_zif_state(usb, &zif_state)
}

/// Mask-ROM read: CE/CS polarity is programmed into the silicon, so probe
/// every combination; the first that yields non-0xFF data wins.
fn read_mask(
    usb: &UsbDevice,
    p: &dyn Protocol,
    device: &Device,
    l: &MaskPromLayout,
    ds: &mut DataSet,
) -> Result<()> {
    let pin_count = device.package_details.pin_count;
    let mut zif_dir = [PIN_DIRECTION_IN; ZIF_PINS];
    let mut zif_state = [0u8; ZIF_PINS];

    set_io_pins(&mut zif_dir, l.addr, PIN_DIRECTION_OUT, pin_count);
    set_io_pins(&mut zif_dir, l.ce, PIN_DIRECTION_OUT, pin_count);
    set_io_pins(&mut zif_dir, l.cs, PIN_DIRECTION_OUT, pin_count);
    set_io_pins(
        &mut zif_dir,
        l.data,
        PIN_DIRECTION_IN | PIN_PULLUP,
        pin_count,
    );

    p.set_zif_direction(usb, &zif_dir)?;
    p.set_zif_state(usb, &zif_state)?;

    for ce_pattern in 0u32..(1 << l.ce.len()) {
        for cs_pattern in 0u32..(1 << l.cs.len()) {
            for (i, byte) in ds.data.iter_mut().enumerate() {
                set_bits(&mut zif_state, l.addr, ds.address + i as u32, pin_count);
                set_bits(&mut zif_state, l.cs, cs_pattern, pin_count);
                p.set_zif_state(usb, &zif_state)?;

                set_bits(&mut zif_state, l.ce, ce_pattern, pin_count);
                p.set_zif_state(usb, &zif_state)?;

                let zif = p.get_zif_state(usb)?;
                *byte = get_bits(&zif, l.data, pin_count) as u8;

                set_bits(&mut zif_state, l.ce, !ce_pattern, pin_count);
                p.set_zif_state(usb, &zif_state)?;
            }
            if !is_empty(&ds.data) {
                return Ok(());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::{DeviceFlags, PackageDetails};

    fn prom_device(variant: u32, pin_count: u8) -> Device {
        Device {
            name: "TESTPROM".into(),
            protocol_id: CP_PROM,
            variant,
            package_details: PackageDetails {
                pin_count,
                ..Default::default()
            },
            flags: DeviceFlags {
                custom_protocol: true,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn zif_pin_bottom_aligns_package() {
        // DIP16: device pin 1 → ZIF 1, device pin 16 → ZIF 40, pin 9 → ZIF 33.
        assert_eq!(zif_pin(1, 16), Some(0));
        assert_eq!(zif_pin(8, 16), Some(7));
        assert_eq!(zif_pin(9, 16), Some(32));
        assert_eq!(zif_pin(16, 16), Some(39));
        // DIP28 bottom-aligns too: pin 15 → ZIF 27, pin 28 → ZIF 40.
        assert_eq!(zif_pin(15, 28), Some(26));
        assert_eq!(zif_pin(28, 28), Some(39));
        // Malformed entries must not panic.
        assert_eq!(zif_pin(1, 0), None);
        assert_eq!(zif_pin(45, 40), None);
    }

    #[test]
    fn layout_lookup_rejects_bad_variants() {
        // Type 12 (D2364C) is the last valid normal entry.
        assert!(matches!(
            layout_for(&prom_device(12, 24)).unwrap(),
            Layout::Normal(_)
        ));
        assert!(layout_for(&prom_device(13, 24)).is_err());
        // 0x80/0x81 are the only mask entries.
        assert!(matches!(
            layout_for(&prom_device(0x80, 28)).unwrap(),
            Layout::Mask(_)
        ));
        assert!(matches!(
            layout_for(&prom_device(0x81, 24)).unwrap(),
            Layout::Mask(_)
        ));
        assert!(layout_for(&prom_device(0x82, 24)).is_err());
    }

    #[test]
    fn compare_mask_comes_from_table() {
        // 4-bit PROM (82S126 variant 1) → 0x0f; 8-bit → 0xff.
        assert_eq!(compare_mask_for(&prom_device(1, 16)).unwrap(), 0x0f);
        assert_eq!(compare_mask_for(&prom_device(0, 16)).unwrap(), 0xff);
        assert_eq!(compare_mask_for(&prom_device(0x80, 28)).unwrap(), 0xff);
    }

    #[test]
    fn set_get_bits_roundtrip() {
        // Type 0 (82S123): addr pins 10-14 are on the bottom half of a
        // DIP16 → ZIF 34-38 → indices 33-37.
        let l = &PROM_TABLE[0];
        let mut zif = [0u8; ZIF_PINS];
        set_bits(&mut zif, l.addr, 0x1f, 16);
        assert_eq!(&zif[33..38], &[1, 1, 1, 1, 1]);
        // Data byte 0xa5 = 1010_0101 → pins 1,3,6,9 high.
        let mut pins = [0u8; ZIF_PINS];
        for (i, &p) in l.data.iter().enumerate() {
            pins[zif_pin(p, 16).unwrap()] = ((0xa5u32 >> i) & 1) as u8;
        }
        assert_eq!(get_bits(&pins, l.data, 16), 0xa5);
    }

    #[test]
    fn is_prom_requires_both_flags() {
        let mut d = prom_device(0, 16);
        assert!(is_prom(&d));
        d.flags.custom_protocol = false;
        assert!(!is_prom(&d));
        d.flags.custom_protocol = true;
        d.protocol_id = 0x03;
        assert!(!is_prom(&d));
    }

    #[test]
    fn mask_probe_covers_all_combinations() {
        // 2316 (variant 0x81): ce empty + cs 3 pins → 1<<0 * 1<<3 = 8 combos.
        let l = &MASK_PROM_TABLE[1];
        let combos = (1u32 << l.ce.len()) * (1u32 << l.cs.len());
        assert_eq!(combos, 8);
        // HN43128P: 1 ce + 2 cs → 2*4 = 8 combos.
        let l = &MASK_PROM_TABLE[0];
        let combos = (1u32 << l.ce.len()) * (1u32 << l.cs.len());
        assert_eq!(combos, 8);
    }
}
