<script lang="ts">
  import { onMount } from "svelte";
  import { theme } from "./lib/stores/theme";
  import { programmer, refreshProgrammer, selectedDevice, checkDatabase } from "./lib/stores/device";
  import { logs } from "./lib/stores/logs";
  import { hexBuffer, clearHexBuffer, loadFile } from "./lib/stores/hex";
  import { pickOpenFile, pickSaveFile } from "./lib/file-dialog";
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
  import HexViewer from "./lib/components/HexViewer.svelte";

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

    // Delay startup checks to let the UI render first
    setTimeout(() => {
      checkDatabase().then((ok) => {
        if (!ok) {
          logs.error("Chip database (infoic.xml / logicic.xml) not found. Searches will not work.");
        }
      });

      refreshProgrammer().catch(() => {
        logs.warn("No programmer detected on startup");
      });
    }, 100);
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
    const path = await pickSaveFile("Save chip dump as");
    if (path) await doRead(path, getOptions());
  }

  async function onWrite() {
    const path = await pickOpenFile("Select file to write to chip");
    if (path) await doWrite(path, getOptions());
  }

  async function onVerify() {
    const path = await pickOpenFile("Select file to verify against");
    if (path) await doVerify(path, getOptions());
  }

  async function onLoadFile() {
    const path = await pickOpenFile("Open file to inspect");
    if (path) await loadFile(path);
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

    <!-- Center: Operations + Hex viewer -->
    <section class="flex-1 flex flex-col p-4 gap-4 overflow-hidden">
      <!-- Operations panel -->
      <div class="card preset-filled-surface-100-900 border border-surface-200-800 p-4 shrink-0">
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
          <button
            class="btn preset-tonal"
            onclick={onLoadFile}
            disabled={$isRunning}
          >
            Load File
          </button>
          {#if $hexBuffer}
            <button
              class="btn preset-tonal"
              onclick={clearHexBuffer}
            >
              Clear
            </button>
          {/if}
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

      <!-- Hex viewer -->
      <div class="flex-1 min-h-0">
        <HexViewer />
      </div>
    </section>

    <!-- Right: Terminal log -->
    <aside class="w-96 flex flex-col border-l border-surface-200-800">
      <TerminalLog />
    </aside>
  </main>
</div>
