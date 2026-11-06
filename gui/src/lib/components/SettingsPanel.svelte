<script lang="ts">
  import { settings, setSetting } from "../stores/settings";
  import type { AppSettings } from "../stores/settings";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { LogicalSize } from "@tauri-apps/api/dpi";
  import { invoke } from "@tauri-apps/api/core";
  import { pickDirectory } from "../file-dialog";
  import { getDbStatus, setCustomDbDir, reloadDatabase, type DbDirStatus } from "../stores/device";
  import { progress } from "../stores/operations";

  interface AlgoDownloadOutcome {
    output_dir: string;
    xml_path: string;
    t56_algorithms: number;
    t76_algorithms: number;
    firmware: { model: string; version: string; path: string }[];
    warnings: string[];
  }

  let show = $state(false);
  let dbStatus = $state<DbDirStatus | null>(null);
  let dbError = $state<string | null>(null);
  let dbBusy = $state(false);
  let dlBusy = $state(false);
  let dlError = $state<string | null>(null);
  let dlResult = $state<AlgoDownloadOutcome | null>(null);

  let isDark = $derived(
    $settings.theme === "dark" ||
    ($settings.theme === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches)
  );

  function toggle() {
    show = !show;
    if (show) {
      dbError = null;
      // Fetch db status when opening
      getDbStatus().then((s) => { dbStatus = s; });
    }
  }

  function close() {
    show = false;
  }

  async function update<K extends keyof AppSettings>(key: K, value: AppSettings[K]) {
    await setSetting(key, value);
  }

  async function browseDbDir() {
    dbError = null;
    const dir = await pickDirectory("Select database directory");
    if (!dir) return;
    dbBusy = true;
    try {
      await setCustomDbDir(dir);
      await setSetting("customDbDir", dir);
      await reloadDatabase();
      dbStatus = await getDbStatus();
    } catch (e: any) {
      dbError = String(e);
    } finally {
      dbBusy = false;
    }
  }

  async function resetDbDir() {
    dbError = null;
    dbBusy = true;
    try {
      await setCustomDbDir(null);
      await setSetting("customDbDir", null);
      await reloadDatabase();
      dbStatus = await getDbStatus();
    } catch (e: any) {
      dbError = String(e);
    } finally {
      dbBusy = false;
    }
  }

  async function downloadAlgos() {
    dlBusy = true;
    dlError = null;
    dlResult = null;
    try {
      // Writes to the custom db dir when one is configured so the new
      // algorithm.xml lives alongside the user's other database files;
      // otherwise the per-user data dir (which the resolver searches).
      dlResult = await invoke<AlgoDownloadOutcome>("download_algorithms", {
        outputDir: $settings.customDbDir ?? null,
      });
    } catch (e: any) {
      dlError = String(e);
    } finally {
      dlBusy = false;
    }
  }

  async function cancelDownload() {
    try {
      await invoke("cancel_algo_download");
    } catch {
      // fire-and-forget — the flag may arrive after completion anyway
    }
  }

  async function openSaveFolder() {
    try {
      await invoke("open_download_dir", { dir: $settings.customDbDir ?? null });
    } catch (e: any) {
      dlError = String(e);
    }
  }

  function handleBackdrop(event: MouseEvent) {
    if (event.target === event.currentTarget) close();
  }
</script>

<button class="btn preset-tonal p-2 flex items-center justify-center hover:bg-primary-500/20 hover:border-primary-500/40 transition-colors" onclick={toggle} title="Settings">
  <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
    <path stroke-linecap="round" stroke-linejoin="round" d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
    <path stroke-linecap="round" stroke-linejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
  </svg>
</button>

{#if show}
  <div
    class="fixed inset-0 bg-black/50 z-50 flex items-center justify-center"
    onclick={handleBackdrop}
    onkeydown={(e) => { if (e.key === 'Escape') close(); }}
    role="dialog"
    aria-modal="true"
    tabindex="-1"
  >
    <div class={`rounded-lg shadow-xl w-full max-w-md p-6 space-y-6 border ${isDark ? 'bg-slate-800 text-white border-slate-600' : 'bg-white text-gray-900 border-gray-300'}`}>
      <div class="flex items-center justify-between">
        <h2 class="text-lg font-semibold">Settings</h2>
        <button class="text-sm opacity-60 hover:opacity-100" onclick={close}>Close</button>
      </div>

      <!-- Operation Defaults -->
      <div class="space-y-3">
        <h3 class={`text-sm font-semibold border-b pb-1 ${isDark ? 'text-gray-200 border-slate-600' : 'text-gray-700 border-gray-300'}`}>Operation Defaults</h3>

        <label class="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            class="checkbox"
            checked={$settings.skipErase}
            onchange={(e) => update("skipErase", e.currentTarget.checked)}
          />
          Skip erase by default
        </label>

        <label class="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            class="checkbox"
            checked={$settings.skipVerify}
            onchange={(e) => update("skipVerify", e.currentTarget.checked)}
          />
          Skip verify by default
        </label>

        <div class="flex items-center gap-2 text-sm">
          <span class={`w-24 ${isDark ? 'text-gray-400' : 'text-gray-500'}`}>Page:</span>
          <select
            class="select text-sm flex-1"
            value={$settings.defaultPage}
            onchange={(e) => update("defaultPage", e.currentTarget.value as AppSettings["defaultPage"])}
          >
            <option value="code">Code</option>
            <option value="data">Data</option>
            <option value="user">User</option>
          </select>
        </div>

        <div class="flex items-center gap-2 text-sm">
          <span class={`w-24 ${isDark ? 'text-gray-400' : 'text-gray-500'}`}>Format:</span>
          <select
            class="select text-sm flex-1"
            value={$settings.defaultFormat}
            onchange={(e) => update("defaultFormat", e.currentTarget.value as AppSettings["defaultFormat"])}
          >
            <option value="auto">Auto</option>
            <option value="bin">Binary</option>
            <option value="ihex">Intel HEX</option>
            <option value="srec">SREC</option>
            <option value="jedec">JEDEC</option>
          </select>
        </div>

        <div class="flex items-center gap-2 text-sm">
          <span class={`w-24 ${isDark ? 'text-gray-400' : 'text-gray-500'}`}>Size mismatch:</span>
          <select
            class="select text-sm flex-1"
            value={$settings.defaultSizeMismatch}
            onchange={(e) => update("defaultSizeMismatch", e.currentTarget.value as AppSettings["defaultSizeMismatch"])}
          >
            <option value="error">Error</option>
            <option value="warn">Warn</option>
            <option value="ignore">Ignore</option>
          </select>
        </div>
      </div>

      <!-- Appearance -->
      <div class="space-y-3">
        <h3 class={`text-sm font-semibold border-b pb-1 ${isDark ? 'text-gray-200 border-slate-600' : 'text-gray-700 border-gray-300'}`}>Appearance</h3>

        <div class="flex items-center gap-2 text-sm">
          <span class={`w-24 ${isDark ? 'text-gray-400' : 'text-gray-500'}`}>Theme:</span>
          <select
            class="select text-sm flex-1"
            value={$settings.theme}
            onchange={(e) => update("theme", e.currentTarget.value as AppSettings["theme"])}
          >
            <option value="system">System</option>
            <option value="dark">Dark</option>
            <option value="light">Light</option>
          </select>
        </div>

        <div class="flex items-center gap-2 text-sm">
          <span class={`w-24 ${isDark ? 'text-gray-400' : 'text-gray-500'}`}>Device view:</span>
          <select
            class="select text-sm flex-1"
            value={$settings.deviceViewMode}
            onchange={(e) => update("deviceViewMode", e.currentTarget.value as AppSettings["deviceViewMode"])}
          >
            <option value="paginated">Paginated</option>
            <option value="scroll">Scroll</option>
          </select>
        </div>
      </div>

      <!-- Layout -->
      <div class="space-y-3">
        <h3 class={`text-sm font-semibold border-b pb-1 ${isDark ? 'text-gray-200 border-slate-600' : 'text-gray-700 border-gray-300'}`}>Layout</h3>
        <button
          class="btn preset-tonal text-sm w-full hover:bg-primary-500/20 hover:border-primary-500/40 transition-colors"
          title="Reset panel layout, hex font size and window size to defaults"
          onclick={async () => {
            // Reset panel percentages to defaults
            update("leftPanelPercent", 0.20);
            update("rightPanelPercent", 0.25);
            update("hexViewerFontSize", 13);
            // Clear saved window size and re-apply dynamic sizing
            const [winW, winH] = await invoke<[number, number]>("get_dynamic_window_size");
            const win = getCurrentWindow();
            await win.setSize(new LogicalSize(winW, winH));
            await win.center();
            // Save the new dynamically-computed size
            await setSetting("windowWidth", winW);
            await setSetting("windowHeight", winH);
          }}
        >
          Reset layout & font size
        </button>
        <p class={`text-xs ${isDark ? 'text-gray-400' : 'text-gray-500'}`}>Resets panel widths, font size, and window position to defaults. Takes effect immediately.</p>
      </div>

      <!-- Hex Viewer -->
      <div class="space-y-3">
        <h3 class={`text-sm font-semibold border-b pb-1 ${isDark ? 'text-gray-200 border-slate-600' : 'text-gray-700 border-gray-300'}`}>Hex Viewer</h3>
        <label class={`flex items-center gap-2 text-sm ${isDark ? 'text-gray-300' : 'text-gray-700'}`}>
          <input
            type="checkbox"
            class="checkbox"
            checked={$settings.showEntropyBar}
            onchange={(e) => update("showEntropyBar", e.currentTarget.checked)}
          />
          Show entropy bar
          <span class={`text-xs ${isDark ? 'text-gray-500' : 'text-gray-400'}`}>— color-coded strip showing data randomness per row</span>
        </label>
      </div>

      <!-- Database -->
      <div class="space-y-3">
        <h3 class={`text-sm font-semibold border-b pb-1 ${isDark ? 'text-gray-200 border-slate-600' : 'text-gray-700 border-gray-300'}`}>Database</h3>
        <div class="text-sm space-y-2">
          <div class={`text-xs ${isDark ? 'text-gray-400' : 'text-gray-500'}`}>
            {$settings.customDbDir
              ? `Custom directory: ${$settings.customDbDir}`
              : "Using default search paths"}
          </div>
          {#if $settings.customDbDir && dbStatus && !dbStatus.active}
            <div class="text-xs text-amber-500 font-medium">
              This directory is no longer valid — using default search paths. Browse for a new directory or reset to default.
            </div>
          {/if}
          {#if dbError}
            <div class="text-xs text-red-500">{dbError}</div>
          {/if}
          <div class="flex gap-2">
            <button
              class="btn preset-tonal text-xs hover:bg-primary-500/20 hover:border-primary-500/40 transition-colors"
              title="Choose a directory containing infoic.xml and logicic.xml"
              onclick={browseDbDir}
              disabled={dbBusy}
            >
              Browse...
            </button>
            {#if $settings.customDbDir}
              <button
                class="btn preset-tonal text-xs hover:bg-primary-500/20 hover:border-primary-500/40 transition-colors"
                title="Use the built-in database search paths"
                onclick={resetDbDir}
                disabled={dbBusy}
              >
                Reset to default
              </button>
            {/if}
          </div>
          <p class={`text-xs ${isDark ? 'text-gray-500' : 'text-gray-400'}`}>
            Select a directory containing custom <span class="font-mono">infoic.xml</span> and <span class="font-mono">logicic.xml</span> files. The device list reloads immediately.
          </p>

          <div class={`border-t pt-3 mt-1 space-y-2 ${isDark ? 'border-slate-600' : 'border-gray-300'}`}>
            <div class="flex gap-2 items-center">
              <button
                class="btn preset-tonal text-xs hover:bg-primary-500/20 hover:border-primary-500/40 transition-colors"
                title="Download the official XGPro archives and generate algorithm.xml + firmware images"
                onclick={downloadAlgos}
                disabled={dlBusy}
              >
                {dlBusy ? "Downloading…" : "Download algorithm.xml + firmware…"}
              </button>
              <button
                class="btn preset-tonal text-xs hover:bg-primary-500/20 hover:border-primary-500/40 transition-colors"
                onclick={openSaveFolder}
                disabled={dlBusy}
                title="Open the folder the files are saved to"
              >
                Open folder
              </button>
              {#if dlBusy}
                <button
                  class="btn preset-tonal text-xs hover:bg-primary-500/20 hover:border-primary-500/40 transition-colors"
                  title="Cancel the download"
                  onclick={cancelDownload}
                >
                  Cancel
                </button>
              {/if}
            </div>
            {#if dlBusy && $progress}
              <div class={`text-xs ${isDark ? 'text-gray-400' : 'text-gray-500'}`}>
                {$progress.operation}: {$progress.done.toLocaleString()}{$progress.total ? ` / ${$progress.total.toLocaleString()}` : ""}
              </div>
            {/if}
            {#if dlError}
              <div class="text-xs text-red-500">{dlError}</div>
            {/if}
            {#if dlResult}
              <div class={`text-xs ${isDark ? 'text-green-400' : 'text-green-600'}`}>
                algorithm.xml: {dlResult.t56_algorithms} T56 + {dlResult.t76_algorithms} T76
                algorithms, {dlResult.firmware.length} firmware image(s) → {dlResult.output_dir}.
                Reconnect the programmer to use them.
              </div>
            {/if}
            <p class={`text-xs ${isDark ? 'text-gray-500' : 'text-gray-400'}`}>
              Required for T56/T76 chip operations — not bundled for licensing reasons.
              Downloads the official XGPro archives, verifies checksums, and generates
              <span class="font-mono">algorithm.xml</span> plus firmware images.
            </p>
          </div>
        </div>
      </div>
    </div>
  </div>
{/if}
