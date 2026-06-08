import { writable, derived } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { logs } from "./logs";

export const hexBuffer = writable<Uint8Array | null>(null);
export const hexFilePath = writable<string | null>(null);

export const hexSize = derived(hexBuffer, ($b) => $b?.length ?? 0);

export async function loadFile(path: string) {
  try {
    const bytes = await invoke<number[]>("read_file_bytes", { path });
    hexBuffer.set(new Uint8Array(bytes));
    hexFilePath.set(path);
    logs.info(`Loaded ${bytes.length} bytes from ${path}`);
  } catch (e) {
    logs.error(`Failed to load file: ${e}`);
    hexBuffer.set(null);
    hexFilePath.set(null);
  }
}

export function clearHexBuffer() {
  hexBuffer.set(null);
  hexFilePath.set(null);
}
