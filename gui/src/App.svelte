<script lang="ts">
  import { onMount } from "svelte";
  import { theme } from "./lib/stores/theme";
  import { programmer, refreshProgrammer, forceReconnect, selectedDevice, checkDatabase } from "./lib/stores/device";
  import { logs } from "./lib/stores/logs";
  import { hexLoading, loadFile } from "./lib/stores/hex";
  import { settings, initSettings, setSetting, type AppSettings } from "./lib/stores/settings";
  import { get } from "svelte/store";
  import { invoke } from "@tauri-apps/api/core";
  import { pickOpenFile, pickSaveFile } from "./lib/file-dialog";
  import {
    initProgressListener,
    isRunning,
    activeOperation,
    doRead,
    doReadToBuffer,
    doWrite,
    doVerify,
    doErase,
    doBlankCheck,
    doChipId,
    doLogicTest,
  } from "./lib/stores/operations";
  import TerminalLog from "./lib/components/TerminalLog.svelte";
  import DeviceSelector from "./lib/components/DeviceSelector.svelte";
  import DiagnosticsPanel from "./lib/components/DiagnosticsPanel.svelte";
  import ProgressPanel from "./lib/components/ProgressPanel.svelte";
  import HexViewer from "./lib/components/HexViewer.svelte";
  import SettingsPanel from "./lib/components/SettingsPanel.svelte";

  let themeValue: "system" | "dark" | "light" = $state("system");

  // Operation options
  let skipErase = $state(false);
  let skipVerify = $state(false);
  let icsp = $state(false);
  let page = $state("code");
  let format = $state("auto");
  let sizeMismatch = $state("error");

  // Active operation label for the options panel
  let opLabel = $derived($activeOperation ? $activeOperation.replace("_", " ") : "");
  let opNeedsFileIn = $derived($activeOperation === "write" || $activeOperation === "verify");

  // Panel widths as fractions of window width (persisted as percentages)
  let leftPercent = $state(0.20);
  let rightPercent = $state(0.25);

  // Computed pixel widths based on current window width
  let leftWidth = $derived(Math.round(window.innerWidth * leftPercent));
  let rightWidth = $derived(Math.round(window.innerWidth * rightPercent));

  // Drag state
  let dragMode: "left" | "right" | null = $state(null);
  let dragStartX = $state(0);
  let dragStartLeftPct = $state(0);
  let dragStartRightPct = $state(0);

  function startDrag(mode: "left" | "right", e: MouseEvent) {
    dragMode = mode;
    dragStartX = e.clientX;
    dragStartLeftPct = leftPercent;
    dragStartRightPct = rightPercent;
    e.preventDefault();
  }

  function onMouseMove(e: MouseEvent) {
    if (!dragMode) return;
    const deltaPx = e.clientX - dragStartX;
    const winW = window.innerWidth;
    if (dragMode === "left") {
      const newPct = dragStartLeftPct + (deltaPx / winW);
      leftPercent = Math.max(0.15, Math.min(0.35, newPct));
    } else if (dragMode === "right") {
      // Dragging right splitter right → right panel gets narrower
      const newPct = dragStartRightPct - (deltaPx / winW);
      rightPercent = Math.max(0.20, Math.min(0.45, newPct));
    }
  }

  function stopDrag() {
    if (dragMode) {
      setSetting("leftPanelPercent", leftPercent);
      setSetting("rightPanelPercent", rightPercent);
    }
    dragMode = null;
  }

  onMount(() => {
    theme.init();
    theme.subscribe((t) => {
      themeValue = t;
    });
    initProgressListener();
    initSettings().then(() => {
      const s = $settings;
      skipErase = s.skipErase;
      skipVerify = s.skipVerify;
      icsp = s.icsp;
      leftPercent = s.leftPanelPercent;
      rightPercent = s.rightPanelPercent;
      page = s.defaultPage;
      format = s.defaultFormat;
      sizeMismatch = s.defaultSizeMismatch;
      if (s.theme !== themeValue) {
        theme.set(s.theme);
      }
    });

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

    // Save window size on resize (debounced)
    let resizeTimeout: ReturnType<typeof setTimeout>;
    const onResize = () => {
      clearTimeout(resizeTimeout);
      resizeTimeout = setTimeout(() => {
        setSetting("windowWidth", window.innerWidth);
        setSetting("windowHeight", window.innerHeight);
      }, 500);
    };
    window.addEventListener("resize", onResize);

    return () => {
      window.removeEventListener("resize", onResize);
      clearTimeout(resizeTimeout);
    };
  });

  $effect(() => {
    const s = $settings;
    if (s.theme && s.theme !== themeValue) {
      theme.set(s.theme);
    }
    // React to panel width resets from SettingsPanel
    leftPercent = s.leftPanelPercent;
    rightPercent = s.rightPanelPercent;
  });

  function setTheme(t: "system" | "dark" | "light") {
    theme.set(t);
    setSetting("theme", t);
  }

  function getOptions() {
    return {
      skip_erase: skipErase,
      skip_verify: skipVerify,
      icsp,
      page,
      format,
      size_mismatch: sizeMismatch,
    };
  }

  function selectOp(op: "read" | "write" | "verify" | "erase" | "blank_check" | "chip_id" | "logic_test") {
    activeOperation.set(op);
    switch (op) {
      case "read":
        page = "code";
        format = "auto";
        break;
      case "write":
        page = "code";
        format = "auto";
        skipErase = false;
        skipVerify = false;
        sizeMismatch = "error";
        break;
      case "verify":
        page = "code";
        format = "auto";
        sizeMismatch = "error";
        break;
      case "erase":
      case "blank_check":
      case "chip_id":
      case "logic_test":
        break;
    }
  }

  async function onStart() {
    const op = $activeOperation;
    if (!op) return;

    switch (op) {
      case "read":
        await doReadToBuffer(getOptions());
        break;
      case "write": {
        const path = await pickOpenFile("Select file to write to chip", get(settings).defaultDirectory);
        if (path) {
          await setSetting("defaultDirectory", path.substring(0, path.lastIndexOf("\\") || path.lastIndexOf("/")));
          await doWrite(path, getOptions());
        }
        break;
      }
      case "verify": {
        const path = await pickOpenFile("Select file to verify against", get(settings).defaultDirectory);
        if (path) {
          await setSetting("defaultDirectory", path.substring(0, path.lastIndexOf("\\") || path.lastIndexOf("/")));
          await doVerify(path, getOptions());
        }
        break;
      }
      case "erase":
        await doErase(icsp);
        break;
      case "blank_check":
        await doBlankCheck(icsp);
        break;
      case "chip_id":
        await doChipId(icsp);
        break;
      case "logic_test":
        await doLogicTest(icsp);
        break;
    }
  }

  async function onLoadFile() {
    const path = await pickOpenFile("Open file to inspect");
    if (!path) return;
    hexLoading.set(true);
    try {
      await loadFile(path);
      logs.info(`File loaded: ${path}`);
    } finally {
      hexLoading.set(false);
    }
  }
</script>

<svelte:window onmousemove={onMouseMove} onmouseup={stopDrag} />

<div class="h-screen flex flex-col bg-surface-50-950 text-surface-950-50" class:cursor-col-resize={dragMode !== null} class:select-none={dragMode !== null}>
  <!-- Top bar -->
  <header
    class="flex items-center justify-between px-4 py-2 border-b border-surface-200-800 bg-surface-100-900"
  >
    <div class="flex items-center gap-3">
      <h1 class="text-lg font-bold">MINIPRO-RS</h1>
      <button
        class="flex items-center gap-1.5 cursor-pointer hover:opacity-80 transition-opacity"
        onclick={async () => {
          logs.info("Reconnecting programmer...");
          try {
            await forceReconnect();
            if ($programmer) {
              logs.info(`Programmer reconnected: ${$programmer.model} (FW ${$programmer.firmware})`);
            }
          } catch (e: any) {
            const msg = typeof e === "string" ? e : e?.message || "Unknown error";
            logs.warn(msg);
          }
        }}
        title="Click to detect programmer"
      >
        {#if $programmer}
          <span class="badge bg-emerald-600 text-white text-xs flex items-center gap-1">
            <svg xmlns="http://www.w3.org/2000/svg" class="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5">
              <path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7" />
            </svg>
            {$programmer.model} connected
          </span>
        {:else}
          <span class="badge bg-red-600 text-white text-xs flex items-center gap-1">
            <svg xmlns="http://www.w3.org/2000/svg" class="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5">
              <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
            </svg>
            No programmer
          </span>
        {/if}
        <svg xmlns="http://www.w3.org/2000/svg" class="h-3 w-3 text-surface-600-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
        </svg>
      </button>
    </div>

    <div class="flex items-center gap-2">
      <SettingsPanel />
      <span class="text-xs opacity-60">Theme:</span>
      <div class="segment bg-surface-200-800 rounded p-0.5 flex gap-0.5">
        <button
          class="p-1.5 rounded transition-colors"
          class:preset-filled-primary={themeValue === "system"}
          class:hover:preset-tonal={themeValue !== "system"}
          onclick={() => setTheme("system")}
          title="System"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z" />
          </svg>
        </button>
        <button
          class="p-1.5 rounded transition-colors"
          class:preset-filled-primary={themeValue === "dark"}
          class:hover:preset-tonal={themeValue !== "dark"}
          onclick={() => setTheme("dark")}
          title="Dark"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M20.354 15.354A9 9 0 018.646 3.646 9.003 9.003 0 0012 21a9.003 9.003 0 008.354-5.646z" />
          </svg>
        </button>
        <button
          class="p-1.5 rounded transition-colors"
          class:preset-filled-primary={themeValue === "light"}
          class:hover:preset-tonal={themeValue !== "light"}
          onclick={() => setTheme("light")}
          title="Light"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M12 3v1m0 16v1m9-9h-1M4 12H3m15.364 6.364l-.707-.707M6.343 6.343l-.707-.707m12.728 0l-.707.707M6.343 17.657l-.707.707M16 12a4 4 0 11-8 0 4 4 0 018 0z" />
          </svg>
        </button>
      </div>
    </div>
  </header>

  <!-- Main content -->
  <main class="flex-1 flex overflow-hidden">
    <!-- Left sidebar: Device selector + Diagnostics -->
    <aside class="flex flex-col border-r border-surface-200-800 gap-2 p-2 shrink-0" style="width: {leftWidth}px;">
      <div class="flex-1 min-h-0">
        <DeviceSelector />
      </div>
      <div class="shrink-0 h-64">
        <DiagnosticsPanel />
      </div>
    </aside>

    <!-- Left splitter -->
    <div
      class="w-1 shrink-0 cursor-col-resize hover:bg-primary-500/30 transition-colors self-stretch flex items-center justify-center"
      onmousedown={(e) => startDrag("left", e)}
      title="Drag to resize"
    >
      <div class="w-0.5 h-8 rounded-full bg-surface-300-700"></div>
    </div>

    <!-- Center: Operations + Hex viewer -->
    <section class="flex-1 flex flex-col p-4 gap-4 overflow-hidden min-w-0">
      <!-- Operations panel -->
      <div class="card preset-filled-surface-100-900 border border-surface-200-800 p-4 shrink-0">
        <h2 class="text-sm font-semibold mb-3">Operations</h2>

        <div class="flex flex-wrap gap-1.5 mb-3">
          <button
            class="btn preset-tonal px-2 py-1 text-sm hover:bg-primary-500/20 hover:border-primary-500/40 transition-colors"
            onclick={() => selectOp("read")}
            disabled={$isRunning || !$selectedDevice}
            class:preset-filled-primary={$activeOperation === "read"}
            class:ring-2={$activeOperation === "read"}
            class:ring-primary-400={$activeOperation === "read"}
            class:font-bold={$activeOperation === "read"}
          >
            Read
          </button>
          <button
            class="btn preset-tonal px-2 py-1 text-sm hover:bg-primary-500/20 hover:border-primary-500/40 transition-colors"
            onclick={() => selectOp("write")}
            disabled={$isRunning || !$selectedDevice}
            class:preset-filled-primary={$activeOperation === "write"}
            class:ring-2={$activeOperation === "write"}
            class:ring-primary-400={$activeOperation === "write"}
            class:font-bold={$activeOperation === "write"}
          >
            Write
          </button>
          <button
            class="btn preset-tonal px-2 py-1 text-sm hover:bg-primary-500/20 hover:border-primary-500/40 transition-colors"
            onclick={() => selectOp("verify")}
            disabled={$isRunning || !$selectedDevice}
            class:preset-filled-primary={$activeOperation === "verify"}
            class:ring-2={$activeOperation === "verify"}
            class:ring-primary-400={$activeOperation === "verify"}
            class:font-bold={$activeOperation === "verify"}
          >
            Verify
          </button>
          <button
            class="btn preset-tonal px-2 py-1 text-sm hover:bg-primary-500/20 hover:border-primary-500/40 transition-colors"
            onclick={() => selectOp("erase")}
            disabled={$isRunning || !$selectedDevice}
            class:preset-filled-primary={$activeOperation === "erase"}
            class:ring-2={$activeOperation === "erase"}
            class:ring-primary-400={$activeOperation === "erase"}
            class:font-bold={$activeOperation === "erase"}
          >
            Erase
          </button>
          <button
            class="btn preset-tonal px-2 py-1 text-sm hover:bg-primary-500/20 hover:border-primary-500/40 transition-colors"
            onclick={() => selectOp("blank_check")}
            disabled={$isRunning || !$selectedDevice}
            class:preset-filled-primary={$activeOperation === "blank_check"}
            class:ring-2={$activeOperation === "blank_check"}
            class:ring-primary-400={$activeOperation === "blank_check"}
            class:font-bold={$activeOperation === "blank_check"}
          >
            Blank Check
          </button>
          <button
            class="btn preset-tonal px-2 py-1 text-sm hover:bg-primary-500/20 hover:border-primary-500/40 transition-colors"
            onclick={() => selectOp("chip_id")}
            disabled={$isRunning || !$selectedDevice}
            class:preset-filled-primary={$activeOperation === "chip_id"}
            class:ring-2={$activeOperation === "chip_id"}
            class:ring-primary-400={$activeOperation === "chip_id"}
            class:font-bold={$activeOperation === "chip_id"}
          >
            Chip ID
          </button>
          <button
            class="btn preset-tonal px-2 py-1 text-sm hover:bg-primary-500/20 hover:border-primary-500/40 transition-colors"
            onclick={() => selectOp("logic_test")}
            disabled={$isRunning || !$selectedDevice}
            class:preset-filled-primary={$activeOperation === "logic_test"}
            class:ring-2={$activeOperation === "logic_test"}
            class:ring-primary-400={$activeOperation === "logic_test"}
            class:font-bold={$activeOperation === "logic_test"}
          >
            Logic Test
          </button>
          <button
            class="btn preset-tonal px-2 py-1 text-sm hover:bg-primary-500/20 hover:border-primary-500/40 transition-colors"
            onclick={onLoadFile}
            disabled={$isRunning || $hexLoading}
          >
            Load File
          </button>
        </div>

        <!-- Options -->
        {#if $activeOperation}
          <div class="border-t border-surface-200-800 pt-3">
            <div class="flex items-center justify-between mb-2">
              <span class="text-xs font-semibold uppercase tracking-wider opacity-60">Options for {opLabel}</span>
              <button
                class="text-xs opacity-50 hover:opacity-100 underline transition-opacity"
                onclick={() => selectOp($activeOperation!)}
                disabled={$isRunning}
              >
                Reset defaults
              </button>
            </div>
            <div class="grid grid-cols-2 gap-3 mb-3">
              {#if $activeOperation === "read" || $activeOperation === "write" || $activeOperation === "verify"}
                <div class="flex items-center gap-2 text-sm">
                  <span class="w-16 opacity-60">Page:</span>
                  <select class="select text-sm flex-1" bind:value={page}>
                    <option value="code">Code</option>
                    <option value="data">Data</option>
                    <option value="user">User</option>
                  </select>
                </div>
                <div class="flex items-center gap-2 text-sm">
                  <span class="w-16 opacity-60">Format:</span>
                  <select class="select text-sm flex-1" bind:value={format}>
                    <option value="auto">Auto</option>
                    <option value="bin">Binary</option>
                    <option value="ihex">Intel HEX</option>
                    <option value="srec">SREC</option>
                    <option value="jedec">JEDEC</option>
                  </select>
                </div>
              {/if}
              {#if $activeOperation === "write"}
                <div class="flex items-center gap-2 text-sm">
                  <span class="w-16 opacity-60">Size diff:</span>
                  <select class="select text-sm flex-1" bind:value={sizeMismatch}>
                    <option value="error">Error</option>
                    <option value="warn">Warn</option>
                    <option value="ignore">Ignore</option>
                  </select>
                </div>
                <div class="flex items-center gap-4 ml-6 flex-wrap">
                  <label class="flex items-center gap-2 text-sm">
                    <input type="checkbox" class="checkbox" bind:checked={skipErase} />
                    Skip erase
                  </label>
                  <label class="flex items-center gap-2 text-sm">
                    <input type="checkbox" class="checkbox" bind:checked={skipVerify} />
                    Skip verify
                  </label>
                  <label class="flex items-center gap-2 text-sm" title="In-circuit serial programming">
                    <input type="checkbox" class="checkbox" bind:checked={icsp} />
                    ICSP
                  </label>
                </div>
              {:else if $activeOperation === "verify"}
                <div class="flex items-center gap-2 text-sm">
                  <span class="w-16 opacity-60">Size diff:</span>
                  <select class="select text-sm flex-1" bind:value={sizeMismatch}>
                    <option value="error">Error</option>
                    <option value="warn">Warn</option>
                    <option value="ignore">Ignore</option>
                  </select>
                </div>
              {/if}
              {#if $activeOperation === "erase" || $activeOperation === "blank_check" || $activeOperation === "chip_id"}
                <p class="text-sm opacity-50 col-span-2">No options for this operation.</p>
              {/if}
            </div>

            <!-- Start button -->
            <div class="flex flex-col gap-2">
              {#if opNeedsFileIn}
                <span class="text-xs opacity-60 text-center">You will be prompted to select an input file</span>
              {/if}
              <button
                class="px-8 py-2.5 rounded-lg bg-primary-600 text-white text-base font-semibold flex items-center justify-center gap-2 shadow-md hover:shadow-lg hover:-translate-y-0.5 active:translate-y-0 transition-all disabled:opacity-40"
                onclick={onStart}
                disabled={$isRunning || !$selectedDevice}
              >
                <span>Start {opLabel}</span>
                <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" viewBox="0 0 20 20" fill="currentColor">
                  <path fill-rule="evenodd" d="M10.293 3.293a1 1 0 011.414 0l6 6a1 1 0 010 1.414l-6 6a1 1 0 01-1.414-1.414L14.586 11H3a1 1 0 110-2h11.586l-4.293-4.293a1 1 0 010-1.414z" clip-rule="evenodd" />
                </svg>
              </button>
            </div>
          </div>
        {:else}
          <div class="border-t border-surface-200-800 pt-3">
            <p class="text-sm opacity-50">Select an operation above to configure options.</p>
          </div>
        {/if}

        <!-- Progress panel -->
        <ProgressPanel />
      </div>

      <!-- Hex viewer -->
      <div class="flex-1 min-h-0">
        <HexViewer />
      </div>
    </section>

    <!-- Right splitter -->
    <div
      class="w-1 shrink-0 cursor-col-resize hover:bg-primary-500/30 transition-colors self-stretch flex items-center justify-center"
      onmousedown={(e) => startDrag("right", e)}
      title="Drag to resize"
    >
      <div class="w-0.5 h-8 rounded-full bg-surface-300-700"></div>
    </div>

    <!-- Right sidebar: Terminal log -->
    <aside class="flex flex-col border-l border-surface-200-800 gap-2 p-2 shrink-0" style="width: {rightWidth}px;">
      <div class="flex-1 min-h-0">
        <TerminalLog />
      </div>
    </aside>
  </main>
</div>
