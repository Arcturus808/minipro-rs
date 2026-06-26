<script lang="ts">
  import { onMount } from "svelte";
  import pkg from "../package.json";
  import { theme } from "./lib/stores/theme";
  import { programmer, refreshProgrammer, forceReconnect, selectedDevice, checkDatabase } from "./lib/stores/device";
  import { logs } from "./lib/stores/logs";
  import { hexLoading, loadFile, hexMeta, getHexData, hexEdits, applyHexEdits } from "./lib/stores/hex";
  import { settings, initSettings, setSetting, type AppSettings } from "./lib/stores/settings";
  import { get } from "svelte/store";
  import { invoke } from "@tauri-apps/api/core";
  import { pickOpenFile, pickSaveFile } from "./lib/file-dialog";
  import {
    initProgressListener,
    initLogListener,
    isRunning,
    activeOperation,
    doRead,
    doReadToBuffer,
    doWrite,
    doWriteBytes,
    doVerify,
    doErase,
    doBlankCheck,
    doChipId,
    doLogicTest,
    readFuses,
    writeFuses,
    checkLockProtection,
    type FuseValue,
    type ConfigData,
  } from "./lib/stores/operations";
  import TerminalLog from "./lib/components/TerminalLog.svelte";
  import DeviceSelector from "./lib/components/DeviceSelector.svelte";
  import DiagnosticsPanel from "./lib/components/DiagnosticsPanel.svelte";
  import ProgressPanel from "./lib/components/ProgressPanel.svelte";
  import HexViewer from "./lib/components/HexViewer.svelte";
  import SettingsPanel from "./lib/components/SettingsPanel.svelte";
  import {
    batchState,
    batchModeEnabled,
    batchActive,
    batchWaiting,
    startBatch,
    nextChip,
    retryChip,
    stopBatch,
  } from "./lib/stores/batch";

  let themeValue: "system" | "dark" | "light" = $state("system");

  // Operation options
  let skipErase = $state(false);
  let skipVerify = $state(false);
  let skipBlank = $state(false);
  let checkDeviceId = $state(true);
  let showAdvanced = $state(false);
  let overrideVpp = $state("");
  let overrideVcc = $state("");
  let overrideVdd = $state("");
  let icspMode = $state("zif");
  let page = $state("code");
  let format = $state("auto");
  let sizeMismatch = $state("error");
  let batchCount = $state(""); // empty = unlimited

  const VPP_OPTIONS = ["9.0", "9.5", "10.0", "11.0", "11.5", "12.0", "12.5", "13.0", "13.5", "14.0", "14.5", "15.0", "16.0", "17.0", "18.0", "21.0"];
  const VCC_OPTIONS = ["3.3", "4.0", "4.5", "5.0", "6.0", "6.3", "6.5", "7.0"];

  // Config data state (fuses, locks, user bytes, calibration)
  let configData = $state<ConfigData | null>(null);

  // Auto-initialize config panel with database defaults when a device is selected
  $effect(() => {
    const dev = $selectedDevice;
    if (dev?.config && dev.config.type === "Mcu") {
      configData = {
        cfg_fuses: dev.config.fuses.map((f) => ({ name: f.name, value: f.default_value })),
        lock_bits: dev.config.locks.map((l) => ({ name: l.name, value: l.default_value })),
        user_fuses: [],
        calibration: [],
      };
    }
  });

  // Active operation label for the options panel
  let opLabel = $derived($activeOperation ? $activeOperation.replace("_", " ") : "");
  // Custom start button label per operation
  let startButtonLabel = $derived(
    $activeOperation === "config" ? "Read Config from Chip" :
    $activeOperation ? `Start ${opLabel}` : ""
  );
  let opFileHint = $derived(
    $activeOperation === "write" ? "You will be prompted to select a file to write to the chip" :
    $activeOperation === "verify" ? "You will be prompted to select a file to verify against" :
    null
  );
  let hasHexData = $derived(!!$hexMeta?.data && $hexMeta.data.length > 0);
  let hasPendingHexEdits = $derived($hexEdits.size > 0);

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
    initLogListener();
    initSettings().then(() => {
      const s = $settings;
      skipErase = s.skipErase;
      skipVerify = s.skipVerify;
      icspMode = s.icspMode;
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

  $effect(() => {
    setSetting("icspMode", icspMode);
  });

  function setTheme(t: "system" | "dark" | "light") {
    theme.set(t);
    setSetting("theme", t);
  }

  function getCfgValue(index: number): number {
    return configData?.cfg_fuses[index]?.value ?? 0xff;
  }

  function setCfgValue(index: number, value: number) {
    if (!configData) return;
    configData = {
      ...configData,
      cfg_fuses: configData.cfg_fuses.map((fv, i) => i === index ? { ...fv, value } : fv),
    };
  }

  function getLockValue(index: number): number {
    return configData?.lock_bits[index]?.value ?? 0xff;
  }

  function setLockValue(index: number, value: number) {
    if (!configData) return;
    configData = {
      ...configData,
      lock_bits: configData.lock_bits.map((fv, i) => i === index ? { ...fv, value } : fv),
    };
  }

  // isProgrammed: true when the fuse is active.
  // invert=true (AVR): bit=0 means programmed.
  // invert=false (PIC, etc.): bit=1 means programmed.
  function isFuseProgrammed(value: number, mask: number, invert: boolean): boolean {
    const bitSet = (value & mask) !== 0;
    return invert ? !bitSet : bitSet;
  }

  function toggleFuseValue(current: number, mask: number, invert: boolean): number {
    const programmed = isFuseProgrammed(current, mask, invert);
    if (programmed) {
      return invert ? current | mask : current & ~mask;  // unprogram
    } else {
      return invert ? current & ~mask : current | mask;  // program
    }
  }

  async function writeAllFuses() {
    if (!configData) return;
    try {
      await writeFuses(configData.cfg_fuses, configData.lock_bits, icspMode);
      logs.info("Config written to chip");
    } catch (e) {
      logs.error(`Config write failed: ${e}`);
    }
  }

  function isDangerousFuse(name: string): boolean {
    const lower = name.toLowerCase();
    return [
      "rstdisbl", "disable reset", "rstdis",
      "spien", "enable spi", "disable spi",
      "jtagen", "jtag",
      "dwen", "debugwire",
    ].some((k) => lower.includes(k));
  }

  function getOptions() {
    return {
      skip_erase: skipErase,
      skip_verify: skipVerify,
      skip_blank: skipBlank,
      check_device_id: checkDeviceId,
      vpp: overrideVpp || null,
      vcc: overrideVcc || null,
      vdd: overrideVdd || null,
      icsp_mode: icspMode,
      page,
      format,
      size_mismatch: sizeMismatch,
    };
  }

  function selectOp(op: "read" | "write" | "verify" | "erase" | "blank_check" | "chip_id" | "logic_test" | "config") {
    activeOperation.set(op);
    showAdvanced = false;
    checkDeviceId = true;
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
        skipBlank = false;
        sizeMismatch = "error";
        break;
      case "verify":
        page = "code";
        format = "auto";
        sizeMismatch = "error";
        break;
      case "config":
        break;
      case "erase":
      case "blank_check":
      case "chip_id":
      case "logic_test":
        break;
    }
  }

  async function warnIfLocked() {
    try {
      const status = await checkLockProtection(icspMode);
      if (status.is_protected) {
        logs.warn(`Lock bits are active (0x${status.lock_byte.toString(16).toUpperCase().padStart(2, '0')}). This chip may be read/write protected.`);
      }
    } catch {
      // Ignore — programmer might not be connected
    }
  }

  function isAllBlank(data: Uint8Array | null): boolean {
    if (!data || data.length === 0) return false;
    return data.every((b) => b === 0xff);
  }

  function warnIfVariant() {
    if ($selectedDevice?.name.includes("@")) {
      const base = $selectedDevice.name.split("@")[0];
      logs.warn(`Package variant selected (${$selectedDevice.name}). The data read may be incorrect or garbage. For reliable flash operations, select "${base}" instead.`);
    }
  }

  function logChipIdCheck() {
    if (checkDeviceId && $selectedDevice?.has_chip_id) {
      logs.info("Chip ID check passed");
    }
  }

  async function onWriteFromFile() {
    await warnIfLocked();
    warnIfVariant();
    const path = await pickOpenFile("Select file to write to chip", get(settings).defaultDirectory);
    if (path) {
      await setSetting("defaultDirectory", path.substring(0, path.lastIndexOf("\\") || path.lastIndexOf("/")));
      if ($batchModeEnabled) {
        const count = batchCount.trim() ? parseInt(batchCount.trim(), 10) || null : null;
        await startBatch(path, getOptions(), count);
      } else {
        await doWrite(path, getOptions());
        logChipIdCheck();
      }
    }
  }

  async function onWriteFromHex() {
    await warnIfLocked();
    warnIfVariant();
    if (hasPendingHexEdits) {
      applyHexEdits();
      logs.info("Applied pending hex edits before write");
    }
    await doWriteBytes(getOptions());
    logChipIdCheck();
  }

  async function onStart() {
    const op = $activeOperation;
    if (!op) return;

    switch (op) {
      case "read":
        await warnIfLocked();
        warnIfVariant();
        await doReadToBuffer(getOptions());
        logChipIdCheck();
        if (isAllBlank(getHexData())) {
          logs.warn("Read returned all 0xFF bytes. The chip may be read-protected (lock bits active) or blank.");
        }
        break;
      case "write":
        // When hex data exists, onStart shouldn't be called directly;
        // the UI shows separate buttons instead.
        await onWriteFromFile();
        break;
      case "verify": {
        await warnIfLocked();
        warnIfVariant();
        const path = await pickOpenFile("Select file to verify against", get(settings).defaultDirectory);
        if (path) {
          await setSetting("defaultDirectory", path.substring(0, path.lastIndexOf("\\") || path.lastIndexOf("/")));
          await doVerify(path, getOptions());
          logChipIdCheck();
        }
        break;
      }
      case "erase":
        await doErase(getOptions());
        logChipIdCheck();
        logs.info("Chip erased successfully");
        break;
      case "blank_check": {
        const bcResult = await doBlankCheck(icspMode);
        if (bcResult) {
          if (bcResult.is_blank) {
            logs.info("Chip is blank");
          } else {
            logs.info(`Chip is not blank at 0x${bcResult.address.toString(16).padStart(8, '0').toUpperCase()}`);
          }
        }
        break;
      }
      case "chip_id": {
        const chipResult = await doChipId(icspMode);
        if (chipResult) {
          if (chipResult.is_variant) {
            logs.info(`Chip ID mismatch for ${$selectedDevice.name}: read ${chipResult.id}, expected ${chipResult.expected}. For chip ID verification, select "${chipResult.base_name}" instead.`);
          } else {
            const expectedVal = parseInt(chipResult.expected, 16);
            if (expectedVal === 0) {
              logs.info(`Chip ID: ${chipResult.id} (no expected value in database)`);
            } else if (chipResult.is_match) {
              logs.info(`Chip ID: ${chipResult.id} (matches expected)`);
            } else {
              logs.warn(`Chip ID mismatch: read ${chipResult.id}, expected ${chipResult.expected}`);
            }
          }
        }
        break;
      }
      case "logic_test":
        await doLogicTest(icspMode);
        break;
      case "config":
        try {
          const read = await readFuses(icspMode);
          if (configData) {
            // Merge read values into existing configData (preserving field order)
            const mergedCfg = configData.cfg_fuses.map((fv) => {
              const rv = read.cfg_fuses.find((r) => r.name === fv.name);
              return rv ? { ...fv, value: rv.value } : fv;
            });
            const mergedLock = configData.lock_bits.map((fv) => {
              const rv = read.lock_bits.find((r) => r.name === fv.name);
              return rv ? { ...fv, value: rv.value } : fv;
            });
            configData = {
              cfg_fuses: mergedCfg,
              lock_bits: mergedLock,
              user_fuses: read.user_fuses,
              calibration: read.calibration,
            };
          } else {
            configData = read;
          }
          const total = read.cfg_fuses.length + read.lock_bits.length;
          logs.info(`Config read — ${total} fuse/lock values, ${read.user_fuses.length} user bytes, ${read.calibration.length} calibration bytes`);
        } catch (e) {
          logs.error(`Config read failed: ${e}`);
        }
        break;
    }
  }

  async function onLoadFile() {
    const path = await pickOpenFile("Open file to inspect");
    if (!path) return;
    hexLoading.set(true);
    try {
      const dev = get(selectedDevice);
      const size = dev?.code_memory_size ?? 0;
      await loadFile(path, size > 0 ? size : undefined);
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
      <span class="text-xs opacity-60 font-mono">v{pkg.version}</span>
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
        <div class="flex items-center gap-2 mb-3">
          <h2 class="text-sm font-semibold">Operations</h2>
          <select
            class="select text-xs"
            style="width: auto;"
            title="Programming interface"
            bind:value={icspMode}
          >
            <option value="zif">ZIF socket</option>
            <option value="icsp">ICSP</option>
            <option value="icsp_no_vcc">ICSP (no VCC)</option>
          </select>
        </div>

        <div class="flex flex-wrap gap-1.5 mb-3">
          <button
            class="btn preset-tonal px-2 py-1 text-sm hover:bg-primary-500/20 hover:border-primary-500/40 transition-colors"
            onclick={() => selectOp("read")}
            disabled={$isRunning}
            class:opacity-60={!$selectedDevice}
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
            disabled={$isRunning}
            class:opacity-60={!$selectedDevice}
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
            disabled={$isRunning}
            class:opacity-60={!$selectedDevice}
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
            disabled={$isRunning}
            class:opacity-60={!$selectedDevice}
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
            disabled={$isRunning}
            class:opacity-60={!$selectedDevice}
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
            disabled={$isRunning}
            class:opacity-60={!$selectedDevice}
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
            disabled={$isRunning}
            class:opacity-60={!$selectedDevice}
            class:preset-filled-primary={$activeOperation === "logic_test"}
            class:ring-2={$activeOperation === "logic_test"}
            class:ring-primary-400={$activeOperation === "logic_test"}
            class:font-bold={$activeOperation === "logic_test"}
          >
            Logic Test
          </button>
          <button
            class="btn preset-tonal px-2 py-1 text-sm hover:bg-primary-500/20 hover:border-primary-500/40 transition-colors"
            onclick={() => selectOp("config")}
            disabled={$isRunning || ($selectedDevice !== null && !$selectedDevice.config)}
            class:opacity-60={!$selectedDevice}
            class:preset-filled-primary={$activeOperation === "config"}
            class:ring-2={$activeOperation === "config"}
            class:ring-primary-400={$activeOperation === "config"}
            class:font-bold={$activeOperation === "config"}
          >
            Config
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
            {#if $selectedDevice}
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
            <div class="space-y-2 mb-3">
              {#if $selectedDevice && !$selectedDevice.has_chip_id && ($activeOperation === "read" || $activeOperation === "write" || $activeOperation === "verify")}
                <div class="bg-amber-500/10 border border-amber-500/20 rounded-md px-3 py-1.5">
                  <p class="text-xs text-amber-700-200 font-medium">Warning: This device does not support chip ID verification. Ensure the correct chip is inserted.</p>
                </div>
              {/if}
              <!-- Row 1: Format, Page, Size diff -->
              {#if $activeOperation === "read" || $activeOperation === "write" || $activeOperation === "verify"}
                <div class="flex flex-wrap items-center gap-6 text-sm">
                  <div class="flex items-center gap-2">
                    <span class="opacity-60">Format:</span>
                    <select class="select text-sm" bind:value={format}>
                      <option value="auto">Auto</option>
                      <option value="bin">Binary</option>
                      <option value="ihex">Intel HEX</option>
                      <option value="srec">SREC</option>
                      <option value="jedec">JEDEC</option>
                    </select>
                  </div>
                  <div class="flex items-center gap-2">
                    <span class="opacity-60">Page:</span>
                    <select class="select text-sm" bind:value={page}>
                      <option value="code">Code</option>
                      <option value="data">Data</option>
                      <option value="user">User</option>
                    </select>
                  </div>
                  {#if $activeOperation === "write" || $activeOperation === "verify"}
                    <div class="flex items-center gap-2">
                      <span class="opacity-60 whitespace-nowrap">Size diff:</span>
                      <select class="select text-sm" bind:value={sizeMismatch}>
                        <option value="error">Error</option>
                        <option value="warn">Warn</option>
                        <option value="ignore">Ignore</option>
                      </select>
                    </div>
                  {/if}
                  <label class="flex items-center gap-2" title="Verify chip ID before operation">
                    <input type="checkbox" class="checkbox" bind:checked={checkDeviceId} />
                    Chip ID check
                  </label>
                </div>
              {/if}
              <!-- Row 2: Checkboxes + Advanced toggle -->
              {#if $activeOperation === "write"}
                <div class="flex flex-wrap items-center gap-6 text-sm">
                  <label class="flex items-center gap-2">
                    <input type="checkbox" class="checkbox" bind:checked={skipErase} />
                    Skip erase
                  </label>
                  <label class="flex items-center gap-2">
                    <input type="checkbox" class="checkbox" bind:checked={skipVerify} />
                    Skip verify
                  </label>
                  <label class="flex items-center gap-2" title="Skip writing pages that are all blank (0xFF)">
                    <input type="checkbox" class="checkbox" bind:checked={skipBlank} />
                    Skip blank
                  </label>
                  <span class="w-px h-4 bg-surface-300-700 mx-2"></span>
                  <button
                    class="text-xs opacity-70 hover:opacity-100 underline transition-opacity"
                    onclick={() => showAdvanced = !showAdvanced}
                    type="button"
                  >
                    {showAdvanced ? "▲ Hide Advanced" : "▼ Advanced"}
                  </button>
                </div>
                <!-- Expanded: Voltage overrides -->
                {#if showAdvanced}
                  <div class="flex flex-wrap items-center gap-6 text-sm bg-surface-100-900 rounded-md p-2">
                    <div class="flex items-center gap-2">
                      <span class="opacity-60">VPP:</span>
                      <select class="select text-xs" bind:value={overrideVpp}>
                        <option value="">Default ({$selectedDevice?.voltages?.vpp ?? "—"}V)</option>
                        {#each VPP_OPTIONS as v}
                          <option value={v}>{v}V</option>
                        {/each}
                      </select>
                    </div>
                    <div class="flex items-center gap-2">
                      <span class="opacity-60">VCC:</span>
                      <select class="select text-xs" bind:value={overrideVcc}>
                        <option value="">Default ({$selectedDevice?.voltages?.vcc ?? "—"}V)</option>
                        {#each VCC_OPTIONS as v}
                          <option value={v}>{v}V</option>
                        {/each}
                      </select>
                    </div>
                    <div class="flex items-center gap-2">
                      <span class="opacity-60">VDD:</span>
                      <select class="select text-xs" bind:value={overrideVdd}>
                        <option value="">Default ({$selectedDevice?.voltages?.vdd ?? "—"}V)</option>
                        {#each VCC_OPTIONS as v}
                          <option value={v}>{v}V</option>
                        {/each}
                      </select>
                    </div>
                    <button
                      class="text-xs opacity-60 hover:opacity-100 underline"
                      onclick={() => { overrideVpp = ""; overrideVcc = ""; overrideVdd = ""; }}
                      type="button"
                    >
                      Reset voltages
                    </button>
                  </div>
                {/if}
              {/if}
              {#if $activeOperation === "erase"}
                <div class="flex flex-wrap items-center gap-6 text-sm">
                  <label class="flex items-center gap-2" title="Verify chip ID before operation">
                    <input type="checkbox" class="checkbox" bind:checked={checkDeviceId} />
                    Chip ID check
                  </label>
                </div>
              {/if}
              {#if $activeOperation === "blank_check" || $activeOperation === "chip_id"}
                <p class="text-sm opacity-50 col-span-2">No options for this operation.</p>
              {/if}
              {#if $activeOperation === "config"}
                {#if $selectedDevice?.config && $selectedDevice.config.type === "Mcu" && configData}
                  <div class="col-span-2 space-y-3">
                    {#if $selectedDevice.invert_fuse_bits}
                      <div class="bg-primary-500/10 border border-primary-500/20 rounded-md px-3 py-2">
                        <p class="text-xs text-primary-700-200 font-medium">AVR fuse convention: checked = programmed (active), unchecked = unprogrammed</p>
                      </div>
                    {/if}
                    <div class="flex flex-wrap gap-3">
                      {#if configData.cfg_fuses.length > 0}
                        <div class="bg-surface-100-900 rounded-lg p-3 space-y-2 flex-1 min-w-[240px]">
                          <span class="text-xs font-semibold opacity-70 uppercase tracking-wider">Fuses</span>
                          {#each configData.cfg_fuses as field, i}
                            <div class="flex items-center gap-3">
                              <span class="text-xs font-mono font-semibold opacity-70 w-12">{field.name}</span>
                              <input
                                type="text"
                                class="input text-xs font-mono w-12 px-1 py-0.5"
                                value={field.value.toString(16).padStart(2, '0').toUpperCase()}
                                onchange={(e) => {
                                  const v = parseInt(e.currentTarget.value, 16);
                                  if (!isNaN(v) && v >= 0 && v <= 0xFF) setCfgValue(i, v);
                                }}
                              />
                              <label class="flex items-center gap-2 text-xs cursor-pointer flex-1">
                                <input
                                  type="checkbox"
                                  class="checkbox"
                                  checked={isFuseProgrammed(field.value, $selectedDevice.config.fuses[i].mask, $selectedDevice.invert_fuse_bits)}
                                  onchange={() => setCfgValue(i, toggleFuseValue(field.value, $selectedDevice.config.fuses[i].mask, $selectedDevice.invert_fuse_bits))}
                                />
                                <span class={isDangerousFuse(field.name) ? "text-red-500 font-semibold" : ""}>{$selectedDevice.config.fuses[i].display_name}</span>
                                {#if isDangerousFuse(field.name)}<span class="text-red-500 text-[10px]" title="Dangerous — may disable programming access">!</span>{/if}
                              </label>
                            </div>
                          {/each}
                        </div>
                      {/if}
                      {#if configData.lock_bits.length > 0}
                        <div class="bg-surface-100-900 rounded-lg p-3 space-y-2 flex-1 min-w-[240px]">
                          <span class="text-xs font-semibold opacity-70 uppercase tracking-wider">Lock Bits</span>
                          {#each configData.lock_bits as field, i}
                            <div class="flex items-center gap-3">
                              <span class="text-xs font-mono font-semibold opacity-70 w-12">{field.name}</span>
                              <input
                                type="text"
                                class="input text-xs font-mono w-12 px-1 py-0.5"
                                value={field.value.toString(16).padStart(2, '0').toUpperCase()}
                                onchange={(e) => {
                                  const v = parseInt(e.currentTarget.value, 16);
                                  if (!isNaN(v) && v >= 0 && v <= 0xFF) setLockValue(i, v);
                                }}
                              />
                              <label class="flex items-center gap-2 text-xs cursor-pointer flex-1">
                                <input
                                  type="checkbox"
                                  class="checkbox"
                                  checked={isFuseProgrammed(field.value, $selectedDevice.config.locks[i].mask, $selectedDevice.invert_fuse_bits)}
                                  onchange={() => setLockValue(i, toggleFuseValue(field.value, $selectedDevice.config.locks[i].mask, $selectedDevice.invert_fuse_bits))}
                                />
                                <span>{$selectedDevice.config.locks[i].display_name}</span>
                              </label>
                            </div>
                          {/each}
                        </div>
                      {/if}
                    </div>
                    {#if configData.user_fuses.length > 0}
                      <div class="bg-surface-100-900 rounded-lg p-3 space-y-2">
                        <span class="text-xs font-semibold opacity-70 uppercase tracking-wider">User/ID Fuses</span>
                        <div class="text-xs font-mono opacity-70">{configData.user_fuses.map(b => b.toString(16).padStart(2, '0').toUpperCase()).join(' ')}</div>
                      </div>
                    {/if}
                    {#if configData.calibration.length > 0}
                      <div class="bg-surface-100-900 rounded-lg p-3 space-y-2">
                        <span class="text-xs font-semibold opacity-70 uppercase tracking-wider">Calibration Bytes</span>
                        <div class="text-xs font-mono opacity-70">{configData.calibration.map(b => b.toString(16).padStart(2, '0').toUpperCase()).join(' ')}</div>
                      </div>
                    {/if}
                    <button
                      class="btn preset-filled-primary text-sm px-3 py-2 w-full font-semibold"
                      onclick={writeAllFuses}
                    >
                      Write Config to Chip
                    </button>
                  </div>
                {:else}
                  <p class="text-sm opacity-50 col-span-2">No configuration data for this device.</p>
                {/if}
              {/if}
            </div>

            <!-- Batch mode toggle (write only) -->
            {#if $activeOperation === "write" && !$batchActive}
              <div class="flex items-center gap-3 py-2 px-3 rounded-lg bg-surface-100-900 border border-surface-200-800">
                <label class="flex items-center gap-2 cursor-pointer text-sm">
                  <input type="checkbox" bind:checked={$batchModeEnabled} class="accent-primary-600" />
                  <span>Batch Mode</span>
                </label>
                {#if $batchModeEnabled}
                  <input
                    type="text"
                    bind:value={batchCount}
                    placeholder="unlimited"
                    class="w-24 px-2 py-1 text-sm rounded border border-surface-200-800 bg-transparent"
                    title="Number of chips to program (empty = unlimited)"
                  />
                  <span class="text-xs opacity-50">chips</span>
                {/if}
              </div>
            {/if}

            <!-- Batch progress panel -->
            {#if $batchActive}
              <div class="flex flex-col gap-3 p-4 rounded-lg bg-surface-100-900 border-2 border-primary-500">
                <div class="flex items-center justify-between">
                  <span class="text-sm font-semibold">
                    Batch: Chip {$batchState.chipNumber}{$batchState.total ? ` / ${$batchState.total}` : ""}
                  </span>
                  <span class="text-xs opacity-60">
                    {$batchState.passed} passed · {$batchState.failed} failed
                  </span>
                </div>
                {#if $batchState.lastError}
                  <div class="text-xs text-red-500 bg-red-100 dark:bg-red-900/20 rounded px-2 py-1">
                    Last error: {$batchState.lastError}
                  </div>
                {/if}
                {#if $batchWaiting}
                  <div class="flex gap-2">
                    {#if $batchState.lastError}
                      <button
                        class="flex-1 px-4 py-2 rounded-lg bg-amber-500 text-white text-sm font-semibold hover:bg-amber-600 transition-all"
                        onclick={retryChip}
                      >
                        Retry Chip
                      </button>
                    {/if}
                    <button
                      class="flex-1 px-4 py-2 rounded-lg bg-primary-600 text-white text-sm font-semibold hover:bg-primary-700 transition-all"
                      onclick={nextChip}
                    >
                      {#if $batchState.lastError}Skip & Next{:else}Next Chip{/if}
                    </button>
                    <button
                      class="px-4 py-2 rounded-lg bg-red-500 text-white text-sm font-semibold hover:bg-red-600 transition-all"
                      onclick={stopBatch}
                    >
                      Stop Batch
                    </button>
                  </div>
                {:else}
                  <div class="text-sm opacity-60 text-center py-2">
                    Programming chip {$batchState.chipNumber}...
                  </div>
                {/if}
              </div>
            {/if}

            <!-- Start button -->
            <div class="flex flex-col gap-2">
              {#if opFileHint && !($activeOperation === "write" && hasHexData)}
                <span class="text-xs opacity-60 text-center">{opFileHint}</span>
              {/if}
              {#if $activeOperation === "write" && hasHexData}
                <div class="flex flex-col gap-2">
                  {#if hasPendingHexEdits}
                    <span class="text-xs text-amber-500 text-center">{ $hexEdits.size } pending edit{ $hexEdits.size === 1 ? "" : "s" } will be applied</span>
                  {/if}
                  <button
                    class="px-6 py-2 rounded-lg bg-primary-600 text-white text-sm font-semibold flex items-center justify-center gap-2 shadow-md hover:shadow-lg hover:-translate-y-0.5 active:translate-y-0 transition-all disabled:opacity-40"
                    onclick={onWriteFromHex}
                    disabled={$isRunning || !$selectedDevice || $batchActive}
                  >
                    <span>Write from Hex Buffer</span>
                    <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" viewBox="0 0 20 20" fill="currentColor">
                      <path fill-rule="evenodd" d="M10.293 3.293a1 1 0 011.414 0l6 6a1 1 0 010 1.414l-6 6a1 1 0 01-1.414-1.414L14.586 11H3a1 1 0 110-2h11.586l-4.293-4.293a1 1 0 010-1.414z" clip-rule="evenodd" />
                    </svg>
                  </button>
                  <button
                    class="px-6 py-2 rounded-lg bg-surface-200-800 text-sm font-semibold flex items-center justify-center gap-2 hover:bg-surface-300-700 transition-all disabled:opacity-40"
                    onclick={onWriteFromFile}
                    disabled={$isRunning || !$selectedDevice || $batchActive}
                  >
                    <span>Write from File</span>
                    <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" viewBox="0 0 20 20" fill="currentColor">
                      <path fill-rule="evenodd" d="M10.293 3.293a1 1 0 011.414 0l6 6a1 1 0 010 1.414l-6 6a1 1 0 01-1.414-1.414L14.586 11H3a1 1 0 110-2h11.586l-4.293-4.293a1 1 0 010-1.414z" clip-rule="evenodd" />
                    </svg>
                  </button>
                </div>
              {:else}
                <button
                  class="px-8 py-2.5 rounded-lg bg-primary-600 text-white text-base font-semibold flex items-center justify-center gap-2 shadow-md hover:shadow-lg hover:-translate-y-0.5 active:translate-y-0 transition-all disabled:opacity-40"
                  onclick={onStart}
                  disabled={$isRunning || !$selectedDevice}
                >
                  <span>{startButtonLabel}</span>
                  <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" viewBox="0 0 20 20" fill="currentColor">
                    <path fill-rule="evenodd" d="M10.293 3.293a1 1 0 011.414 0l6 6a1 1 0 010 1.414l-6 6a1 1 0 01-1.414-1.414L14.586 11H3a1 1 0 110-2h11.586l-4.293-4.293a1 1 0 010-1.414z" clip-rule="evenodd" />
                  </svg>
                </button>
              {/if}
            </div>
            {:else}
              <div class="border border-dashed border-surface-300-600 rounded-lg p-6 text-center">
                <svg xmlns="http://www.w3.org/2000/svg" class="h-8 w-8 mx-auto mb-2 opacity-40" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
                </svg>
                <p class="text-sm font-medium opacity-70 mb-1">No device selected</p>
                <p class="text-xs opacity-50">Search for your chip in the Device Selector (left panel) to enable this operation.</p>
              </div>
            {/if}
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
