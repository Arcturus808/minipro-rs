import { writable, derived } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { logs } from "./logs";
import { selectedDevice } from "./device";
import { loadFile } from "./hex";

export interface ProgressEvent {
  done: number;
  total: number;
  operation: string;
}

export interface OperationOptions {
  skip_erase: boolean;
  skip_verify: boolean;
  page: string;
  format: string;
  size_mismatch: string;
}

export const isRunning = writable(false);
export const currentOperation = writable<string | null>(null);
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
    page: "code",
    format: "auto",
    size_mismatch: "error",
  };
}

async function runOp(
  name: string,
  fn: () => Promise<unknown>,
) {
  if (!selectedDevice) {
    logs.error("No device selected");
    return;
  }
  isRunning.set(true);
  currentOperation.set(name);
  progress.set(null);
  logs.info(`${name} started...`);

  try {
    const result = await fn();
    logs.info(`${name} completed successfully`);
    if (result && typeof result === "object" && "bytes" in result) {
      const stats = result as { bytes: number; crc32: number };
      logs.info(`  ${stats.bytes} bytes, CRC-32: ${stats.crc32.toString(16).padStart(8, "0")}`);
    }
  } catch (e) {
    logs.error(`${name} failed: ${e}`);
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

export async function doWrite(path: string, options: OperationOptions = defaultOptions()) {
  await runOp("Write", () =>
    invoke("do_write", { path, options }),
  );
}

export async function doVerify(path: string, options: OperationOptions = defaultOptions()) {
  await runOp("Verify", () =>
    invoke("do_verify", { path, options }),
  );
}

export async function doErase() {
  await runOp("Erase", () => invoke("do_erase"));
}

export async function doBlankCheck() {
  await runOp("Blank check", () => invoke("do_blank_check"));
}

export async function doChipId() {
  await runOp("Chip ID", async () => {
    const id = await invoke<string>("do_chip_id");
    logs.info(`Chip ID: ${id}`);
  });
}
