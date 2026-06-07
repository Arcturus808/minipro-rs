<script lang="ts">
  import { theme } from "./lib/stores/theme";
  import { programmer, refreshProgrammer } from "./lib/stores/device";
  import { logs } from "./lib/stores/logs";
  import TerminalLog from "./lib/components/TerminalLog.svelte";
  import DeviceSelector from "./lib/components/DeviceSelector.svelte";
  import { onMount } from "svelte";

  let themeValue: "system" | "dark" | "light" = $state("system");

  onMount(() => {
    theme.init();
    theme.subscribe((t) => {
      themeValue = t;
    });

    // Try to detect programmer on startup
    refreshProgrammer().catch(() => {
      logs.warn("No programmer detected on startup");
    });
  });

  function setTheme(t: "system" | "dark" | "light") {
    theme.set(t);
  }
</script>

<div class="h-screen flex flex-col bg-surface-50-950 text-surface-950-50">
  <!-- Top bar -->
  <header class="flex items-center justify-between px-4 py-2 border-b border-surface-200-800 bg-surface-100-900">
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
        <div class="flex flex-wrap gap-2">
          <button class="btn preset-filled-primary" onclick={() => logs.info("Read clicked")}>
            Read
          </button>
          <button class="btn preset-filled-primary" onclick={() => logs.info("Write clicked")}>
            Write
          </button>
          <button class="btn preset-tonal" onclick={() => logs.info("Verify clicked")}>
            Verify
          </button>
          <button class="btn preset-tonal" onclick={() => logs.info("Erase clicked")}>
            Erase
          </button>
          <button class="btn preset-tonal" onclick={() => logs.info("Blank check clicked")}>
            Blank Check
          </button>
          <button class="btn preset-tonal" onclick={() => logs.info("Chip ID clicked")}>
            Chip ID
          </button>
        </div>

        <div class="mt-4 grid grid-cols-2 gap-4">
          <label class="flex items-center gap-2 text-sm">
            <input type="checkbox" class="checkbox" />
            Skip erase
          </label>
          <label class="flex items-center gap-2 text-sm">
            <input type="checkbox" class="checkbox" />
            Skip verify
          </label>
          <label class="flex items-center gap-2 text-sm">
            <input type="checkbox" class="checkbox" />
            Write protect on
          </label>
          <label class="flex items-center gap-2 text-sm">
            <input type="checkbox" class="checkbox" />
            ICSP mode
          </label>
        </div>
      </div>

      <!-- Hex viewer placeholder -->
      <div class="card preset-filled-surface-100-900 border border-surface-200-800 flex-1 p-4 flex flex-col">
        <h2 class="text-sm font-semibold mb-2">Hex Viewer</h2>
        <div class="flex-1 flex items-center justify-center opacity-40 text-sm">
          Select a device and read a chip to view data here.
        </div>
      </div>
    </section>

    <!-- Right / Bottom: Terminal log -->
    <aside class="w-96 flex flex-col border-l border-surface-200-800">
      <TerminalLog />
    </aside>
  </main>
</div>
