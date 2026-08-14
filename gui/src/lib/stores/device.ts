import { writable, derived } from "svelte/store";
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

export const programmer = writable<ProgrammerInfo | null>(null);
export const selectedDevice = writable<DeviceInfo | null>(null);
export const deviceList = writable<string[]>([]);
export const isConnected = derived(programmer, ($p) => $p !== null);
export const dbAvailable = writable<boolean | null>(null);
export const voltageOptions = writable<VoltageOptions | null>(null);

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
}

export async function loadVoltageOptions(): Promise<void> {
  try {
    const opts = await invoke<VoltageOptions>("get_voltage_options");
    voltageOptions.set(opts);
  } catch {
    voltageOptions.set(null);
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
