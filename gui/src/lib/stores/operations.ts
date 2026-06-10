import { writable, derived } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { logs } from "./logs";
import { selectedDevice, refreshProgrammer } from "./device";
import { setHexData, base64ToUint8Array, loadFile, getHexData } from "./hex";

export interface ProgressEvent {
  done: number;
  total: number;
  operation: string;
}

export interface OperationOptions {
  skip_erase: boolean;
  skip_verify: boolean;
  icsp_mode: "zif" | "icsp" | "icsp_no_vcc";
  page: string;
  format: string;
  size_mismatch: string;
}

export const isRunning = writable(false);
export const currentOperation = writable<string | null>(null);
export const activeOperation = writable<"read" | "write" | "verify" | "erase" | "blank_check" | "chip_id" | "logic_test" | null>(null);
export const progress = writable<ProgressEvent | null>(null);
export const progressPercent = derived(progress, ($p) => {
  if (!$p || $p.total === 0) return 0;
  return Math.round(($p.done / $p.total) * 100);
});

// Listen for progress events from Rust
let unlisten: (() => void) | null = null;

export async function initProgressListener() {
  if (unlisten) return;
  unlisten = await listen<ProgressEvent>("progress", (event) => {
    progress.set(event.payload);
  });
}

export function stopProgressListener() {
  if (unlisten) {
    unlisten();
    unlisten = null;
  }
}

function defaultOptions(): OperationOptions {
  return {
    skip_erase: false,
    skip_verify: false,
    icsp_mode: "zif",
    page: "code",
    format: "auto",
    size_mismatch: "error",
  };
}

function deferLog(level: "info" | "warn" | "error", message: string) {
  requestAnimationFrame(() => logs[level](message));
}

function formatDuration(ms: number): string {
  const seconds = ms / 1000;
  if (seconds < 60) {
    return `${seconds.toFixed(1)}s`;
  }
  const m = Math.floor(seconds / 60);
  const s = seconds % 60;
  return `${m}m ${s.toFixed(1)}s`;
}

async function runOp(
  name: string,
  fn: () => Promise<unknown>,
) {
  if (!selectedDevice) {
    deferLog("error", "No device selected");
    return;
  }
  isRunning.set(true);
  currentOperation.set(name);
  progress.set(null);
  deferLog("info", `${name} started...`);

  const start = Date.now();
  try {
    const result = await fn();
    const elapsed = Date.now() - start;
    deferLog("info", `${name} completed in ${formatDuration(elapsed)}`);
    if (result && typeof result === "object" && "bytes" in result) {
      const stats = result as { bytes: number; crc32: number };
      deferLog("info", `  ${stats.bytes} bytes, CRC-32: ${stats.crc32.toString(16).padStart(8, "0")}`);
    }
  } catch (e) {
    const elapsed = Date.now() - start;
    deferLog("error", `${name} failed after ${formatDuration(elapsed)}: ${e}`);
    // If the programmer was unplugged mid-operation, sync the badge state
    await refreshProgrammer();
  } finally {
    isRunning.set(false);
    currentOperation.set(null);
    progress.set(null);
  }
}

export async function doRead(path: string, options: OperationOptions = defaultOptions()) {
  await runOp("Read", async () => {
    const result = await invoke("do_read", { path, options });
    // Load the resulting file into the hex viewer
    await loadFile(path);
    return result;
  });
}

export async function doReadToBuffer(options: OperationOptions = defaultOptions()) {
  await runOp("Read", async () => {
    const result = await invoke<{ base64: string; stats: { bytes: number; crc32: number } }>("read_chip_to_bytes", { options });
    const bytes = base64ToUint8Array(result.base64);
    setHexData(bytes, null); // no file path since we read to memory
    return result.stats;
  });
}

function uint8ArrayToBase64(data: Uint8Array): string {
  // Chunked conversion to avoid "Maximum call stack size exceeded"
  // when spreading large arrays to String.fromCharCode
  const CHUNK_SIZE = 0x8000; // 32KB chunks
  let result = "";
  for (let i = 0; i < data.length; i += CHUNK_SIZE) {
    const chunk = data.subarray(i, i + CHUNK_SIZE);
    result += String.fromCharCode(...chunk);
  }
  return btoa(result);
}

export async function saveBufferToFile(path: string) {
  const data = getHexData();
  if (!data) {
    logs.error("No data loaded to save");
    return;
  }
  const base64 = uint8ArrayToBase64(data);
  await invoke("save_bytes_to_file", { path, base64Data: base64 });
}

export async function openFolder(path: string) {
  await invoke("open_folder", { path });
}

export async function doWrite(path: string, options: OperationOptions = defaultOptions()) {
  await runOp("Write", async () => {
    const result = await invoke("do_write", { path, options });
    if (!options.skip_verify) {
      deferLog("info", "  Verify passed");
    }
    return result;
  });
}

export async function doVerify(path: string, options: OperationOptions = defaultOptions()) {
  await runOp("Verify", () =>
    invoke("do_verify", { path, options }),
  );
}

export async function doErase(icsp_mode: string = "zif") {
  await runOp("Erase", () => invoke("do_erase", { icsp_mode }));
}

export async function doBlankCheck(icsp_mode: string = "zif") {
  await runOp("Blank check", () => invoke("do_blank_check", { icsp_mode }));
}

export async function doChipId(icsp_mode: string = "zif") {
  await runOp("Chip ID", async () => {
    const id = await invoke<string>("do_chip_id", { icsp_mode });
    deferLog("info", `Chip ID: ${id}`);
  });
}

export async function doLogicTest(icsp_mode: string = "zif") {
  await runOp("Logic test", async () => {
    const result = await invoke<string>("do_logic_test", { icsp_mode });
    if (result.trim()) {
      // Print each line of the test result table to the terminal
      for (const line of result.trim().split("\n")) {
        deferLog("info", line);
      }
    }
    deferLog("info", "Logic test completed");
  });
}

export async function readFuses(fuseType: number): Promise<number[]> {
  const dto = await invoke<{ bytes: number[] }>("read_fuses", { fuseType });
  return dto.bytes;
}

export async function writeFuses(fuseType: number, data: number[]): Promise<void> {
  await invoke("write_fuses", { fuseType, data });
}
