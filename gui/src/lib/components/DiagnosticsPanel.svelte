<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { programmer } from "../stores/device";
  import { logs } from "../stores/logs";

  async function checkOvc() {
    try {
      const r = await invoke<any>("check_overcurrent");
      logs.info(r.safe ? "Overcurrent check: OK" : `Overcurrent! flag=${r.ovc_flag}`);
    } catch (e) {
      logs.error(`OVC failed: ${e}`);
    }
  }

  async function readCalib() {
    try {
      const r = await invoke<any>("read_calibration");
      const hex = r.bytes.map((b: number) => b.toString(16).padStart(2, "0")).join(" ");
      logs.info(`Calibration: ${hex}`);
    } catch (e) {
      logs.error(`Calib failed: ${e}`);
    }
  }
</script>

<div class="border border-surface-200-800 p-2">
  <h3 class="text-sm font-semibold mb-2">Diagnostics</h3>
  {#if $programmer}
    <div class="text-xs space-y-0.5 mb-2">
      <div class="flex justify-between"><span class="opacity-60">Model</span><span>{$programmer.model}</span></div>
      <div class="flex justify-between"><span class="opacity-60">FW</span><span>{$programmer.firmware}</span></div>
      <div class="flex justify-between"><span class="opacity-60">SN</span><span>{$programmer.serial_number}</span></div>
    </div>
  {:else}
    <p class="text-sm opacity-50 mb-2">No programmer detected.</p>
  {/if}
  <div class="space-y-1">
    <button class="w-full text-left text-sm px-2 py-1.5 border border-surface-200-800 hover:bg-surface-200-800 disabled:opacity-40" onclick={checkOvc} disabled={!$programmer}>Check Overcurrent</button>
    <button class="w-full text-left text-sm px-2 py-1.5 border border-surface-200-800 hover:bg-surface-200-800 disabled:opacity-40" onclick={readCalib} disabled={!$programmer}>Read Calibration</button>
    <button class="w-full text-left text-sm px-2 py-1.5 border border-surface-200-800 opacity-40 cursor-not-allowed" disabled>Pin Test (unsupported)</button>
  </div>
</div>
