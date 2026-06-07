import { writable, derived } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";

export interface ProgrammerInfo {
  model: string;
  firmware: string;
  serial_number: string;
  hardware_version: string;
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
}

export const programmer = writable<ProgrammerInfo | null>(null);
export const selectedDevice = writable<DeviceInfo | null>(null);
export const deviceList = writable<string[]>([]);
export const isConnected = derived(programmer, ($p) => $p !== null);

export async function refreshProgrammer() {
  try {
    const info = await invoke<ProgrammerInfo>("get_programmer_info");
    programmer.set(info);
  } catch (e) {
    programmer.set(null);
    throw e;
  }
}

export async function searchDevices(query: string) {
  const results = await invoke<string[]>("search_devices", { query });
  deviceList.set(results);
  return results;
}

export async function selectDevice(name: string) {
  const info = await invoke<DeviceInfo>("get_device_info", { name });
  selectedDevice.set(info);
}
