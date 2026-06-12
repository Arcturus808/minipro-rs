import { writable, derived, get } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { logs } from "./logs";

interface HexMeta {
  size: number;
  path: string | null;
  data: Uint8Array | null;
  crc32: number | null;
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

/** Compute CRC-32 (IEEE 802.3) for a byte array. */
function computeCrc32(data: Uint8Array): number {
  const table = new Uint32Array(256);
  for (let i = 0; i < 256; i++) {
    let c = i;
    for (let k = 0; k < 8; k++) {
      c = (c & 1) ? (0xEDB88320 ^ (c >>> 1)) : (c >>> 1);
    }
    table[i] = c >>> 0;
  }
  let crc = 0xFFFFFFFF;
  for (let i = 0; i < data.length; i++) {
    crc = (crc >>> 8) ^ table[(crc ^ data[i]) & 0xFF];
  }
  return (crc ^ 0xFFFFFFFF) >>> 0;
}

/** Directly set hex data (for testing or chip reads). */
export function setHexData(data: Uint8Array | null, path: string | null = null) {
  hexMeta.set(data ? { size: data.length, path, data, crc32: computeCrc32(data) } : null);
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

export async function loadFile(path: string, deviceSize?: number) {
  try {
    const args: Record<string, any> = { path };
    if (path.toLowerCase().endsWith(".hex") && deviceSize && deviceSize > 0) {
      args.target_size = deviceSize;
      args.blank_value = 0xFF;
    }
    const base64 = await invoke<string>("read_file_bytes", args);
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

// ── Hex editing ─────────────────────────────────────────────────────────────

/** Sparse map of edited bytes: offset → new value.  Does not modify the
 *  underlying buffer until the user explicitly applies changes. */
export const hexEdits = writable<Map<number, number>>(new Map());

/** Record a single-byte edit.  Pass `null` to clear an edit. */
export function setHexEdit(offset: number, value: number | null) {
  hexEdits.update((map) => {
    const next = new Map(map);
    if (value === null || (value & 0xFF) === value) {
      if (value === null) {
        next.delete(offset);
      } else {
        next.set(offset, value & 0xFF);
      }
    }
    return next;
  });
}

/** Clear all pending edits. */
export function clearHexEdits() {
  hexEdits.set(new Map());
}

/** Apply pending edits to the underlying hex buffer and clear the edit map.
 *  Returns the modified Uint8Array (or null if no data is loaded). */
export function applyHexEdits(): Uint8Array | null {
  const meta = get(hexMeta);
  const edits = get(hexEdits);
  if (!meta || !meta.data || edits.size === 0) return null;

  const newData = new Uint8Array(meta.data);
  for (const [offset, value] of edits) {
    if (offset >= 0 && offset < newData.length) {
      newData[offset] = value;
    }
  }
  setHexData(newData, meta.path);
  clearHexEdits();
  return newData;
}

/** Get the effective value at an offset (edited or original). */
export function getHexByte(offset: number): number | null {
  const meta = get(hexMeta);
  if (!meta || !meta.data) return null;
  const edits = get(hexEdits);
  if (edits.has(offset)) return edits.get(offset)!;
  return meta.data[offset] ?? null;
}
