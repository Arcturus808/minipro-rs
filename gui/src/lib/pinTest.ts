import type { DeviceInfo } from "./stores/device";

/** Programmer models with a real `0x3E` pin-detect command — mirrors
 *  `pin_check_gate_applies` / `Protocol::pin_test` in core. */
export const PIN_TEST_SUPPORTED: ReadonlySet<string> = new Set([
  "TL866II+",
  "T48",
  "T76",
]);

/** T76 contact test is documented for plain DIP parts only
 *  (@DIPn/@PDIPn, n even ≤ 40, non-ICSP) — mirrors
 *  `t76_pin_test_supported` in core. */
export function t76PinTestOk(dev: DeviceInfo): boolean {
  if (dev.icsp_only) return false;
  const pkg = dev.name.split(",")[0]?.split("@").pop() ?? "";
  const m = /^(?:P?DIP)(\d+)$/.exec(pkg);
  return !!m && +m[1] % 2 === 0 && +m[1] <= 40 && +m[1] === dev.pin_count;
}

/** Whether the device carries pin-contact test data — mirrors the core's
 *  `device.pin_map & 0xFF != 0` (`pin_check_gate_applies`,
 *  `pin_contact_check`): only the LOW BYTE is the `<maps>` index and 0
 *  marks "no data", so a raw `pin_map !== 0` wrongly enables the check
 *  for flag-only values like 0x8800 (e.g. generic 628512). */
export function hasPinMap(
  dev: Pick<DeviceInfo, "pin_map"> | null | undefined,
): boolean {
  return !!dev && (dev.pin_map & 0xff) !== 0;
}

/** Whether the model supports pin contact checks for `dev` at all —
 *  the T76 additionally requires a plain DIP package. pin_map and
 *  ICSP-mode applicability are checked separately by callers. */
export function pinCheckModelOk(
  model: string | undefined,
  dev: DeviceInfo | null,
): boolean {
  if (!model || !PIN_TEST_SUPPORTED.has(model)) return false;
  return model === "T76" ? !!dev && t76PinTestOk(dev) : true;
}
