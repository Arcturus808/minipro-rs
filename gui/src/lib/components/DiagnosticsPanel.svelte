<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { programmer, refreshProgrammer, selectedDevice } from "../stores/device";
  import { logs } from "../stores/logs";
  import { doFirmwareUpdate } from "../stores/operations";
  import { doPinTest, pinTestResult, pinTestRunning, clearPinTestResult } from "../stores/operations";
  import { settings } from "../stores/settings";
  import { pickOpenFile, confirmDialog } from "../file-dialog";

  const HARDWARE_CHECK_SUPPORTED = new Set([
    "TL866II+",
    "T48",
    "T56",
    "T76",
  ]);

  const FIRMWARE_UPDATE_SUPPORTED = new Set([
    "TL866A",
    "TL866CS",
    "TL866II+",
    "T48",
    "T76",
  ]);

  // Pin test is implemented for TL866II+ only — the sole model upstream
  // supports. The T48 has its own single-command pin-detect protocol
  // (0x3E → per-pin contact bitmap, documented at
  // proghq.org/wiki/XGecu_Protocol and used by XGPro's "Pin Detect") that
  // upstream never implemented, so the II+ bit-banging commands
  // (0x2D-0x36) don't apply. TL866A/CS and T56 are likewise unimplemented
  // upstream. T76 is FPGA-based with no dedicated contact-test bitstream;
  // Matt Brown's t76-improvements branch discovered that its 0x3E command
  // is an adapter-init pin-driver configuration step, not a standalone
  // test — the xgecu-pro project confirmed on real hardware that it
  // "measured nothing and corrupted every read."
  const PIN_TEST_SUPPORTED = new Set([
    "TL866II+",
  ]);

  const hardwareCheckSupported = $derived(
    $programmer ? HARDWARE_CHECK_SUPPORTED.has($programmer.model) : false,
  );

  const firmwareUpdateSupported = $derived(
    $programmer ? FIRMWARE_UPDATE_SUPPORTED.has($programmer.model) : false,
  );

  const pinTestSupported = $derived(
    $programmer ? PIN_TEST_SUPPORTED.has($programmer.model) : false,
  );

  // Pin test requires a device with pin_map data and ZIF mode
  const pinTestEnabled = $derived(
    pinTestSupported
      && $programmer !== null
      && $selectedDevice !== null
      && $selectedDevice.pin_map !== 0
      && $settings.icspMode === "zif"
      && !$pinTestRunning,
  );

  // TL866A/CS clones are ubiquitous — firmware >=3.2.85 (MiniPro 6.82+)
  // detects them and disables boot mode, bricking the unit until boot
  // mode is forced in hardware. Warn on every firmware-update path.
  const CLONE_RISK_MODELS = new Set(["TL866A", "TL866CS"]);

  function parseFwVersion(v: string): number[] | null {
    const m = String(v).match(/(\d+)\.(\d+)\.(\d+)/);
    return m ? [Number(m[1]), Number(m[2]), Number(m[3])] : null;
  }

  // Extra confirm-dialog note comparing installed vs candidate firmware.
  // The first version component is the hardware revision — a mismatch
  // gets a loud warning rather than a hard block, matching upstream's
  // warn-only posture.
  function firmwareVersionNote(installed: string, file: string): string {
    const a = parseFwVersion(installed);
    const b = parseFwVersion(file);
    if (!a || !b) return "";
    if (a[0] !== b[0]) {
      return `\n\nWARNING: hardware revision differs — installed ${installed}, file ${file}. Flashing a different revision can brick the device.`;
    }
    const cmp = b[1] - a[1] || b[2] - a[2];
    if (cmp < 0) {
      return `\n\nNote: this is a DOWNGRADE (installed ${installed}, file ${file}).`;
    }
    if (cmp === 0) {
      return `\n\nFile version matches installed (${installed}) — reflash only to recover from a fault.`;
    }
    return `\n\nFile version: ${file} (installed: ${installed}).`;
  }

  async function updateFirmware() {
    // Prefer a firmware image previously fetched by the algorithm
    // downloader — the firmware-<model>-*.dat naming makes the match
    // model-safe by construction. Fall back to manual file selection
    // when none exists (e.g. TL866A/CS, not in the pinned archives).
    let path = null;
    let fileName = "";
    let versionNote = "";
    try {
      const found = await invoke<{ path: string; model: string; version: string }[]>(
        "find_downloaded_firmware",
        {
          model: $programmer.model,
          customDir: $settings.customDbDir ?? null,
        },
      );
      if (found.length > 0) {
        path = found[0].path;
        fileName = path.split(/[\\/]/).pop() ?? path;
        versionNote = firmwareVersionNote($programmer.firmware, found[0].version);
      }
    } catch {
      // resolution failure is non-fatal — fall through to the picker
    }
    if (!path) {
      path = await pickOpenFile("Select firmware file (update.dat, UpdateII.dat, updateT76.dat)");
      if (!path) return;
      fileName = path.split(/[\\/]/).pop() ?? path;
    }
    const cloneNote = CLONE_RISK_MODELS.has($programmer.model)
      ? "\n\nCLONE WARNING: many TL866A/CS units in circulation are clones, often without the owner's knowledge. Firmware 3.2.85/3.2.86 (MiniPro 6.82+) contains clone detection that disables boot mode — flashing it on a clone bricks the device, recoverable only by forcing boot mode in hardware. Proceed only if you know your unit is genuine."
      : "";
    const confirmed = await confirmDialog(
      "Firmware Update — EXPERIMENTAL",
      `WARNING: This feature is experimental and has not been fully validated.\n\nThis will erase and reflash your programmer's firmware.\n\nDo NOT disconnect the device during the update, or it may become bricked.\nIf the update fails, leave the device plugged in and try again — the bootloader is preserved and recovery is usually possible.\n\nSelected file: ${fileName}${versionNote}${cloneNote}\n\nProceed at your own risk?`,
      "warning",
    );
    if (!confirmed) return;
    await doFirmwareUpdate(path);
    await refreshProgrammer();
  }

  async function checkOvc() {
    try {
      const r = await invoke<any>("check_overcurrent");
      logs.info(r.safe ? "Overcurrent check: OK" : `Overcurrent! flag=${r.ovc_flag}`);
    } catch (e) {
      logs.error(`OVC failed: ${e}`);
      await refreshProgrammer();
    }
  }

  async function runHardwareCheck() {
    try {
      const r = await invoke<{ supported: boolean; pass: boolean; message: string }>("run_hardware_check");
      if (r.supported && r.pass) {
        logs.info("Hardware check: PASS");
      } else {
        logs.warn(`Hardware check: ${r.message}`);
      }
    } catch (e) {
      logs.error(`Hardware check failed: ${e}`);
      await refreshProgrammer();
    }
  }

  async function runPinTest() {
    await doPinTest($settings.icspMode);
  }
</script>

<div class="border border-surface-200-800 p-2">
  <h3 class="text-sm font-semibold mb-2">Programmer Diagnostics</h3>
  {#if $programmer}
    <div class="text-xs space-y-0.5 mb-2">
      <div class="flex gap-2"><span class="opacity-60 w-10">Model</span><span>{$programmer.model}</span></div>
      <div class="flex gap-2"><span class="opacity-60 w-10">FW</span><span>{$programmer.firmware}</span></div>
      <div class="flex gap-2"><span class="opacity-60 w-10">SN</span><span>{$programmer.serial_number}</span></div>
    </div>
  {:else}
    <p class="text-sm opacity-50 mb-2">No programmer detected.</p>
  {/if}
  <details class="text-sm">
    <summary class="cursor-pointer opacity-70 hover:opacity-100 select-none py-1">Diagnostics</summary>
    <div class="space-y-1 mt-1">
      <button class="w-full text-left text-sm px-2 py-1.5 border border-surface-200-800 hover:bg-surface-200-800 disabled:opacity-40" onclick={checkOvc} disabled={!$programmer}>Check Overcurrent</button>
      {#if hardwareCheckSupported}
        <button class="w-full text-left text-sm px-2 py-1.5 border border-surface-200-800 hover:bg-surface-200-800 disabled:opacity-40" onclick={runHardwareCheck} disabled={!$programmer}>Hardware Check</button>
      {:else}
        <button class="w-full text-left text-sm px-2 py-1.5 border border-surface-200-800 opacity-40 cursor-not-allowed" disabled title="Not supported on this programmer model">Hardware Check</button>
      {/if}
      {#if firmwareUpdateSupported}
        <button class="w-full text-left text-sm px-2 py-1.5 border border-surface-200-800 hover:bg-surface-200-800 disabled:opacity-40" onclick={updateFirmware} disabled={!$programmer}>Firmware Update</button>
      {:else}
        <button class="w-full text-left text-sm px-2 py-1.5 border border-surface-200-800 opacity-40 cursor-not-allowed" disabled title="Not supported on this programmer model">Firmware Update</button>
      {/if}
      {#if pinTestSupported}
        <button
          class="w-full text-left text-sm px-2 py-1.5 border border-surface-200-800 hover:bg-surface-200-800 disabled:opacity-40"
          onclick={runPinTest}
          disabled={!pinTestEnabled}
          title={$settings.icspMode !== "zif" ? "Pin test requires ZIF mode" : !$selectedDevice ? "Select a device first" : $selectedDevice.pin_map === 0 ? "No pin-map data for this device" : "Run ZIF socket pin-contact test"}
        >
          {#if $pinTestRunning}
            Pin Contact Test (running...)
          {:else}
            Pin Contact Test
          {/if}
        </button>
      {:else}
        <button class="w-full text-left text-sm px-2 py-1.5 border border-surface-200-800 opacity-40 cursor-not-allowed" disabled title="Not supported on this programmer model">Pin Contact Test</button>
      {/if}
    </div>
  </details>
</div>
