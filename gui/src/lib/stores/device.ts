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
  chip_type: string;
  pin_count: number;
  package_type: string;
  voltages: {
    vpp: number;
    vdd: number;
    vcc: number;
  };
  code_memory_size: number;
  data_memory_size: number;
  can_erase: boolean;
  has_chip_id: boolean;
  config: ChipConfig | null;
}

export const programmer = writable<ProgrammerInfo | null>(null);
export const selectedDevice = writable<DeviceInfo | null>(null);
export const deviceList = writable<string[]>([]);
export const isConnected = derived(programmer, ($p) => $p !== null);
export const dbAvailable = writable<boolean | null>(null);

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
}
