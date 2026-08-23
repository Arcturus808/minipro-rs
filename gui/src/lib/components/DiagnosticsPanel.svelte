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

  // Pin test is supported on TL866II+ and T48 only.
  // T48 inherits from TL866II+ protocol (alias). TL866A/CS and T56 lack
  // the bit-banging hardware required for contact detection. T76 is FPGA-
  // based with no dedicated contact-test bitstream; its 0x3E command is
  // an adapter-init pin-driver configuration step, not a standalone test.
  // XGPro itself removed pin detect from the T76 UI.
  const PIN_TEST_SUPPORTED = new Set([
    "TL866II+",
    "T48",
  ]);

  $: hardwareCheckSupported = $programmer
    ? HARDWARE_CHECK_SUPPORTED.has($programmer.model)
    : false;

  $: firmwareUpdateSupported = $programmer
    ? FIRMWARE_UPDATE_SUPPORTED.has($programmer.model)
    : false;

  $: pinTestSupported = $programmer
    ? PIN_TEST_SUPPORTED.has($programmer.model)
    : false;

  // Pin test requires a device with pin_map data and ZIF mode
  $: pinTestEnabled = pinTestSupported
    && $programmer !== null
    && $selectedDevice !== null
    && $selectedDevice.pin_map !== 0
    && $settings.icspMode === "zif"
    && !$pinTestRunning;

  async function updateFirmware() {
    const path = await pickOpenFile("Select firmware file (update.dat, UpdateII.dat, updateT76.dat)");
    if (!path) return;
    const fileName = path.split(/[\\/]/).pop() ?? path;
    const confirmed = await confirmDialog(
      "Firmware Update — EXPERIMENTAL",
      `WARNING: This feature is experimental and has not been fully validated.\n\nThis will erase and reflash your programmer's firmware.\n\nDo NOT disconnect the device during the update, or it may become bricked.\nIf the update fails, leave the device plugged in and try again — the bootloader is preserved and recovery is usually possible.\n\nSelected file: ${fileName}\n\nProceed at your own risk?`,
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
