<script lang="ts">
  import { onMount } from "svelte";
  import { theme } from "./lib/stores/theme";
  import { programmer, refreshProgrammer, selectedDevice } from "./lib/stores/device";
  import { logs } from "./lib/stores/logs";
  import {
    initProgressListener,
    isRunning,
    doRead,
    doWrite,
    doVerify,
    doErase,
    doBlankCheck,
    doChipId,
  } from "./lib/stores/operations";
  import TerminalLog from "./lib/components/TerminalLog.svelte";
  import DeviceSelector from "./lib/components/DeviceSelector.svelte";
  import ProgressPanel from "./lib/components/ProgressPanel.svelte";

  let themeValue: "system" | "dark" | "light" = $state("system");

  // Operation options
  let skipErase = $state(false);
  let skipVerify = $state(false);
  let page = $state("code");
  let format = $state("auto");
  let sizeMismatch = $state("error");

  onMount(() => {
    theme.init();
    theme.subscribe((t) => {
      themeValue = t;
    });
    initProgressListener();

    refreshProgrammer().catch(() => {
      logs.warn("No programmer detected on startup");
    });
  });

  function setTheme(t: "system" | "dark" | "light") {
    theme.set(t);
  }

  function getOptions() {
    return {
      skip_erase: skipErase,
      skip_verify: skipVerify,
      page,
      format,
      size_mismatch: sizeMismatch,
    };
  }

  async function onRead() {
    const path = await pickFile("read");
    if (path) await doRead(path, getOptions());
  }

  async function onWrite() {
    const path = await pickFile("write");
    if (path) await doWrite(path, getOptions());
  }

  async function onVerify() {
    const path = await pickFile("verify");
    if (path) await doVerify(path, getOptions());
  }

  // Simple file path input for now — will integrate tauri-plugin-dialog later
  async function pickFile(_context: string): Promise<string | null> {
    const path = prompt("Enter file path:");
    return path;
  }
</script>

<div class="h-screen flex flex-col bg-surface-50-950 text-surface-950-50">
  <!-- Top bar -->
  <header
    class="flex items-center justify-between px-4 py-2 border-b border-surface-200-800 bg-surface-100-900"
  >
    <div class="flex items-center gap-3">
      <h1 class="text-lg font-bold">MINIPRO</h1>
      {#if $programmer}
        <span class="badge preset-filled-success-500 text-xs">
          {$programmer.model} (FW {$programmer.firmware})
        </span>
      {:else}
        <span class="badge preset-filled-error-500 text-xs">No programmer</span>
      {/if}
    </div>

    <div class="flex items-center gap-2">
      <span class="text-xs opacity-60">Theme:</span>
      <div class="segment bg-surface-200-800 rounded p-0.5 flex gap-0.5">
        {#each (["system", "dark", "light"] as const) as t}
          <button
            class="text-xs px-2 py-1 rounded transition-colors"
            class:preset-filled-primary={themeValue === t}
            class:hover:preset-tonal={themeValue !== t}
            onclick={() => setTheme(t)}
          >
            {t[0].toUpperCase() + t.slice(1)}
          </button>
        {/each}
      </div>
    </div>
  </header>

  <!-- Main content -->
  <main class="flex-1 flex overflow-hidden">
    <!-- Left sidebar: Device selector -->
    <aside class="w-72 flex flex-col border-r border-surface-200-800">
      <DeviceSelector />
    </aside>

    <!-- Center: Operations + Hex viewer placeholder -->
    <section class="flex-1 flex flex-col p-4 gap-4 overflow-auto">
      <!-- Operations panel -->
      <div class="card preset-filled-surface-100-900 border border-surface-200-800 p-4">
        <h2 class="text-sm font-semibold mb-3">Operations</h2>

        <div class="flex flex-wrap gap-2 mb-4">
          <button
            class="btn preset-filled-primary"
            onclick={onRead}
            disabled={$isRunning || !$selectedDevice}
          >
            Read
          </button>
          <button
            class="btn preset-filled-primary"
            onclick={onWrite}
            disabled={$isRunning || !$selectedDevice}
          >
            Write
          </button>
          <button
            class="btn preset-tonal"
            onclick={onVerify}
            disabled={$isRunning || !$selectedDevice}
          >
            Verify
          </button>
          <button
            class="btn preset-tonal"
            onclick={doErase}
            disabled={$isRunning || !$selectedDevice}
          >
            Erase
          </button>
          <button
            class="btn preset-tonal"
            onclick={doBlankCheck}
            disabled={$isRunning || !$selectedDevice}
          >
            Blank Check
          </button>
          <button
            class="btn preset-tonal"
            onclick={doChipId}
            disabled={$isRunning || !$selectedDevice}
          >
            Chip ID
          </button>
        </div>

        <!-- Progress panel -->
        <ProgressPanel />

        <!-- Options -->
        <div class="mt-4 grid grid-cols-2 gap-4 border-t border-surface-200-800 pt-4">
          <label class="flex items-center gap-2 text-sm">
            <input type="checkbox" class="checkbox" bind:checked={skipErase} />
            Skip erase
          </label>
          <label class="flex items-center gap-2 text-sm">
            <input type="checkbox" class="checkbox" bind:checked={skipVerify} />
            Skip verify
          </label>
          <div class="flex items-center gap-2 text-sm">
            <span class="opacity-60">Page:</span>
            <select class="select text-sm flex-1" bind:value={page}>
              <option value="code">Code</option>
              <option value="data">Data</option>
              <option value="user">User</option>
            </select>
          </div>
          <div class="flex items-center gap-2 text-sm">
            <span class="opacity-60">Format:</span>
            <select class="select text-sm flex-1" bind:value={format}>
              <option value="auto">Auto</option>
              <option value="bin">Binary</option>
              <option value="ihex">Intel HEX</option>
              <option value="srec">SREC</option>
              <option value="jedec">JEDEC</option>
            </select>
          </div>
          <div class="flex items-center gap-2 text-sm">
            <span class="opacity-60">Size mismatch:</span>
            <select class="select text-sm flex-1" bind:value={sizeMismatch}>
              <option value="error">Error</option>
              <option value="warn">Warn</option>
              <option value="ignore">Ignore</option>
            </select>
          </div>
        </div>
      </div>

      <!-- Hex viewer placeholder -->
      <div
        class="card preset-filled-surface-100-900 border border-surface-200-800 flex-1 p-4 flex flex-col"
      >
        <h2 class="text-sm font-semibold mb-2">Hex Viewer</h2>
        <div class="flex-1 flex items-center justify-center opacity-40 text-sm">
          Select a device and read a chip to view data here.
        </div>
      </div>
    </section>

    <!-- Right: Terminal log -->
    <aside class="w-96 flex flex-col border-l border-surface-200-800">
      <TerminalLog />
    </aside>
  </main>
</div>
