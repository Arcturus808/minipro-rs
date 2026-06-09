import { writable, derived, get } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { logs } from "./logs";

interface HexMeta {
  size: number;
  path: string | null;
  data: Uint8Array | null;
}

export const hexMeta = writable<HexMeta | null>(null);
export const hexLoading = writable(false);

export const hexSize = derived(hexMeta, ($m) => $m?.size ?? 0);
export const hexFilePath = derived(hexMeta, ($m) => $m?.path ?? null);

/** Get the raw hex data (not reactive — call this imperatively when needed). */
export function getHexData(): Uint8Array | null {
  return get(hexMeta)?.data ?? null;
}

/** Check if any hex data is loaded. */
export function hasHexData(): boolean {
  return get(hexMeta)?.data !== null;
}

/** Directly set hex data (for testing or chip reads). */
export function setHexData(data: Uint8Array | null, path: string | null = null) {
  hexMeta.set(data ? { size: data.length, path, data } : null);
}

export function base64ToUint8Array(base64: string): Uint8Array {
  const binary = atob(base64);
  const len = binary.length;
  const bytes = new Uint8Array(len);
  for (let i = 0; i < len; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

export async function loadFile(path: string) {
  try {
    const base64 = await invoke<string>("read_file_bytes", { path });
    const bytes = base64ToUint8Array(base64);
    setHexData(bytes, path);
  } catch (e) {
    logs.error(`Failed to load file: ${e}`);
    setHexData(null);
  }
}

export function clearHexBuffer() {
  setHexData(null);
}
