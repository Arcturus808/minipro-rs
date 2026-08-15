import { writable, derived, get } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";

export interface ProgrammerInfo {
  model: string;
  firmware: string;
  serial_number: string;
  hardware_version: string;
}

export interface FuseField {
  name: string;
  display_name: string;
  mask: number;
  default_value: number;
}

export interface ChipConfig {
  type: "Mcu" | "Pld";
  fuses: FuseField[];
  locks: FuseField[];
}

export interface DeviceInfo {
  name: string;
  manufacturer: string;
  chip_type: string;
  pin_count: number;
  package_type: string;
  voltages: {
    vpp: string;
    vdd: string;
    vcc: string;
  };
  code_memory_size: number;
  data_memory_size: number;
  can_erase: boolean;
  has_chip_id: boolean;
  config: ChipConfig | null;
  /** True for AVR-family devices where fuse bit=0 means programmed. */
  invert_fuse_bits: boolean;
  /** Config name from the XML `<config name="...">` attribute (e.g., "avr_11").
   *  Used to look up fuse bit definitions. */
  config_name: string | null;
  /** Raw pin_map value from the database (lower byte = index into <maps>).
   *  0 means no contact-test data (use pin_count fallback for placement). */
  pin_map: number;
}

export interface PinMap {
  /** ZIF pin numbers to drive as GND during contact test. */
  gnd_table: number[];
  /** ZIF pin numbers that must make electrical contact (chip footprint). */
  mask: number[];
}

export interface VoltageOptions {
  vcc: string[] | null;
  vpp: string[] | null;
  is_logic: boolean;
}

// ── Fuse bit definitions (from backend get_fuse_bit_defs) ───────────────────

export interface FuseBitField {
  name: string;
  description: string;
  bit: number;
}

export interface FuseByteDef {
  name: string;
  /** Bit width of the config word (8 for AVR, 12/14/16 for PIC). */
  width: number;
  fields: FuseBitField[];
}

export interface FuseConfigDef {
  fuse_bytes: FuseByteDef[];
  lock_bytes: FuseByteDef[];
}

export const programmer = writable<ProgrammerInfo | null>(null);
export const selectedDevice = writable<DeviceInfo | null>(null);
export const deviceList = writable<string[]>([]);
export const isConnected = derived(programmer, ($p) => $p !== null);
export const dbAvailable = writable<boolean | null>(null);
export const voltageOptions = writable<VoltageOptions | null>(null);
export const fuseBitDefs = writable<FuseConfigDef | null>(null);

export async function refreshProgrammer() {
  try {
    const info = await invoke<ProgrammerInfo>("get_programmer_info");
    programmer.set(info);
  } catch (e) {
    programmer.set(null);
    throw e;
  }
}

export async function forceReconnect() {
  try {
    const info = await invoke<ProgrammerInfo>("force_reconnect");
    programmer.set(info);
  } catch (e) {
    programmer.set(null);
    throw e;
  }
}

export async function checkDatabase() {
  try {
    const ok = await invoke<boolean>("check_database");
    dbAvailable.set(ok);
    return ok;
  } catch (e) {
    dbAvailable.set(false);
    return false;
  }
}

export async function searchDevices(query: string) {
  const results = await invoke<string[]>("search_devices", { query });
  deviceList.set(results);
  return results;
}

export async function selectDevice(name: string) {
  const info = await invoke<DeviceInfo>("select_device", { name });
  selectedDevice.set(info);
}

export async function deselectDevice() {
  await invoke("deselect_device");
  selectedDevice.set(null);
  voltageOptions.set(null);
  fuseBitDefs.set(null);
}

export async function loadVoltageOptions(): Promise<void> {
  try {
    const opts = await invoke<VoltageOptions>("get_voltage_options");
    voltageOptions.set(opts);
  } catch {
    voltageOptions.set(null);
  }
}

/**
 * Load fuse bit definitions for the currently selected device.
 * Sets the store to null if no definitions are available (frontend falls
 * back to hex-only input in that case).
 */
export async function loadFuseBitDefs(): Promise<void> {
  const dev = get(selectedDevice);
  if (!dev?.config_name) {
    fuseBitDefs.set(null);
    return;
  }
  try {
    const defs = await invoke<FuseConfigDef | null>("get_fuse_bit_defs", {
      configName: dev.config_name,
      chipName: dev.name,
    });
    fuseBitDefs.set(defs);
  } catch {
    fuseBitDefs.set(null);
  }
}

export interface DbDirStatus {
  customDir: string | null;
  active: boolean;
}

export async function getDbStatus(): Promise<DbDirStatus> {
  return invoke<DbDirStatus>("get_db_status");
}

export async function setCustomDbDir(dir: string | null): Promise<void> {
  await invoke("set_custom_db_dir", { dir });
}

/**
 * Clear the selected device after a database directory change.
 * The backend has already reloaded device names; the next searchDevices()
 * call will return results from the new database.
 */
export async function reloadDatabase(): Promise<void> {
  await deselectDevice();
  deviceList.set([]);
}
