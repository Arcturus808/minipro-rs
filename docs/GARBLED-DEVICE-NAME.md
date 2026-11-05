# Garbled device name in Device Manager

## What it looks like

Under Device Manager → *Universal Serial Bus devices*, the XGecu
programmer appears with a name of random CJK / garbled characters
instead of "XGecu T48" (or TL866II+/T56).

## Why

The device's own USB `iProduct` string descriptor contains junk bytes —
a bug in XGecu's firmware USB stack (uninitialized/garbage data where
the product string should be). Windows decodes the raw bytes as
UTF-16LE and renders whatever CJK glyphs they happen to form. The
garbage varies between firmware versions because it's baked into the
firmware image.

Affected: every unit in the `0xA466` VID family — TL866II+, T48, T56
(all share VID `0xA466` / PID `0x0A53`). The T76 (`0xA466` / `0x1A86`)
may exhibit the same quirk.

**It's cosmetic and harmless.** It is not a sign of a counterfeit unit,
a broken driver, or a misconfigured Windows locale — every unit does
this on every machine. The software never reads the display string:
programmer detection is VID/PID + the sysinfo model byte.

Windows shows the raw device string because the WinUSB-style driver
binding provides no friendly name of its own. The fix below overrides
the display only — it changes nothing on the device.

## Fix (Windows — requires administrator)

Windows honors a `FriendlyName` registry value over the device string.
The key is per-device, keyed to the unit's USB serial number.

### Registry Editor

1. Open `regedit` as administrator.
2. Navigate to
   `HKLM\SYSTEM\CurrentControlSet\Enum\USB\VID_A466&PID_0A53\`
3. Expand it — there is one subkey per unit, named after its USB
   serial number (visible in the app's Diagnostics panel, e.g.
   `RTEM3GUXO797QTUVCXS9091`).
4. In that subkey, create a **String value** named `FriendlyName`
   with the display name you want (e.g. `XGecu T48`).
5. Unplug and replug the programmer (or disable/enable it in Device
   Manager).

### Or elevated PowerShell

```powershell
# List the instance keys to confirm your unit's serial
Get-ChildItem "HKLM:\SYSTEM\CurrentControlSet\Enum\USB\VID_A466&PID_0A53"

# Then set the name on your unit's serial key
New-ItemProperty "HKLM:\SYSTEM\CurrentControlSet\Enum\USB\VID_A466&PID_0A53\<YOUR-SERIAL>" `
    -Name FriendlyName -Value "XGecu T48" -PropertyType String -Force
```

### Caveats

- **Don't wildcard all instance keys** — TL866II+, T48 and T56 share
  `VID_A466&PID_0A53`, so a blanket rename would mislabel any other
  units you own. Target only the serial subkey of the unit you want
  renamed.
- The instance key follows the unit's serial number, so the name
  survives replugging into different ports and reboots.
- Reinstalling the driver (Zadig, etc.) can create a fresh device
  instance — repeat the rename afterwards if the garbled name returns.
- T76 units have their own VID/PID key
  (`...\USB\VID_A466&PID_1A86\<serial>`); same procedure if affected.

## Linux / macOS

The same junk bytes show up in `lsusb` output or System Information —
equally cosmetic. There's no per-device friendly-name mechanism worth
bothering with; just ignore it.
