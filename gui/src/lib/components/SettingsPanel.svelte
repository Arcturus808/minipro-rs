<script lang="ts">
  import { settings, setSetting } from "../stores/settings";
  import type { AppSettings } from "../stores/settings";
  import { getCurrentWindow } from "@tauri-apps/api/window";

  let show = $state(false);

  let isDark = $derived(
    $settings.theme === "dark" ||
    ($settings.theme === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches)
  );

  function toggle() {
    show = !show;
  }

  function close() {
    show = false;
  }

  async function update<K extends keyof AppSettings>(key: K, value: AppSettings[K]) {
    await setSetting(key, value);
  }

  function handleBackdrop(event: MouseEvent) {
    if (event.target === event.currentTarget) close();
  }
</script>

<button class="btn preset-tonal p-2 flex items-center justify-center" onclick={toggle} title="Settings">
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
          class="btn preset-tonal text-sm w-full"
          onclick={async () => {
            update("leftPanelWidth", 288);
            update("rightPanelWidth", 448);
            update("hexViewerFontSize", 13);
            const win = getCurrentWindow();
            await win.setSize({ type: "Logical", width: 1400, height: 900 });
            await win.center();
          }}
        >
          Reset layout & font size
        </button>
        <p class={`text-xs ${isDark ? 'text-gray-400' : 'text-gray-500'}`}>Resets panel widths, font size, and window position to defaults. Takes effect immediately.</p>
      </div>
    </div>
  </div>
{/if}
