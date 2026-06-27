import { writable, derived } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { logs } from "./logs";
import { refreshProgrammer } from "./device";
import type { OperationOptions } from "./operations";

export interface SerialConfig {
  start: number;
  address: number;
  width: number;
  format: "bin" | "ascii" | "bcd";
  endian: "little" | "big";
  step: number;
  checksum: "none" | "xor" | "crc8";
}

export interface BatchState {
  active: boolean;       // true when a batch run is in progress
  chipNumber: number;    // current chip (1-based)
  total: number | null;  // total chips to program (null = unlimited)
  passed: number;
  failed: number;
  lastError: string | null;
  waitingForNext: boolean; // true after a chip completes, waiting for user to click "Next Chip"
  filePath: string | null; // firmware file path for the batch
  options: OperationOptions | null;
  serialConfig: SerialConfig | null;
}

const initialState: BatchState = {
  active: false,
  chipNumber: 0,
  total: null,
  passed: 0,
  failed: 0,
  lastError: null,
  waitingForNext: false,
  filePath: null,
  options: null,
  serialConfig: null,
};

export const batchState = writable<BatchState>(initialState);

export const batchActive = derived(batchState, ($s) => $s.active);
export const batchWaiting = derived(batchState, ($s) => $s.waitingForNext);

/** Toggle batch mode on/off (before starting a run). */
export const batchModeEnabled = writable(false);

/** Start a batch run: program the first chip, then wait for user to continue. */
export async function startBatch(
  filePath: string,
  options: OperationOptions,
  count: number | null,
  serialConfig: SerialConfig | null,
) {
  batchState.set({
    active: true,
    chipNumber: 1,
    total: count,
    passed: 0,
    failed: 0,
    lastError: null,
    waitingForNext: false,
    filePath,
    options,
    serialConfig,
  });

  if (serialConfig) {
    logs.info(
      `Batch started: programming ${count ?? "unlimited"} chip(s) with ${filePath}, serial start=${serialConfig.start} addr=0x${serialConfig.address.toString(16).toUpperCase()}`,
    );
  } else {
    logs.info(
      `Batch started: programming ${count ?? "unlimited"} chip(s) with ${filePath}`,
    );
  }

  await programCurrentChip();
}

/** Program the current chip in the batch. */
async function programCurrentChip() {
  const state = getBatchState();
  if (!state.filePath || !state.options) return;

  batchState.update((s) => ({ ...s, waitingForNext: false }));

  try {
    const result = await invoke("do_batch_write_chip", {
      path: state.filePath,
      chipNumber: state.chipNumber,
      options: state.options,
      serialConfig: state.serialConfig,
    });

    batchState.update((s) => ({
      ...s,
      passed: s.passed + 1,
      waitingForNext: true,
    }));

    // Check if we've reached the count limit
    const updated = getBatchState();
    if (updated.total !== null && updated.chipNumber >= updated.total) {
      finishBatch();
    }
  } catch (e) {
    batchState.update((s) => ({
      ...s,
      failed: s.failed + 1,
      lastError: String(e),
      waitingForNext: true,
    }));
    logs.error(`Chip ${state.chipNumber}: FAIL — ${e}`);
    await refreshProgrammer();
  }
}

/** Advance to the next chip (user clicked "Next Chip"). */
export async function nextChip() {
  const state = getBatchState();
  if (!state.active) return;

  batchState.update((s) => ({
    ...s,
    chipNumber: s.chipNumber + 1,
    lastError: null,
    waitingForNext: false,
  }));

  await programCurrentChip();
}

/** Retry the current chip (user clicked "Retry"). */
export async function retryChip() {
  const state = getBatchState();
  if (!state.active) return;

  // Reset the failed chip's count (don't double-count)
  batchState.update((s) => ({
    ...s,
    failed: Math.max(0, s.failed - 1),
    lastError: null,
    waitingForNext: false,
  }));

  await programCurrentChip();
}

/** Stop the batch run and print summary. */
export function stopBatch() {
  const state = getBatchState();
  if (!state.active) return;

  logs.info(
    `Batch stopped: ${state.chipNumber} chip(s) attempted, ${state.passed} passed, ${state.failed} failed`,
  );

  batchState.set(initialState);
}

/** Internal: batch completed naturally (reached count limit). */
function finishBatch() {
  const state = getBatchState();
  logs.info(
    `Batch complete: ${state.passed} chip(s) programmed successfully, ${state.failed} failed`,
  );
  batchState.set(initialState);
}

/** Get the current batch state (non-reactive). */
function getBatchState(): BatchState {
  let state: BatchState = initialState;
  const unsub = batchState.subscribe((s) => (state = s));
  unsub();
  return state;
}
