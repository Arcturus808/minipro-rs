<script lang="ts">
  import { selectedDevice, deselectDevice, favorites, toggleFavorite } from "../stores/device";
  import { clearIdentifyResultsContents, type IdentifyResult } from "../stores/operations";

  let { results }: { results: IdentifyResult[] } = $props();

  let passCount = $derived(results.filter((r) => r.pass).length);
  let failCount = $derived(results.length - passCount);
  let selectedName = $derived($selectedDevice?.name ?? null);
  let hasResults = $derived(results.length > 0);
  let favNames = $derived(new Set($favorites.map((f) => f.name)));
  let matches = $derived(results.filter((r) => r.pass));

  async function onSelect(name: string) {
    const { selectDevice } = await import("../stores/device");
    await selectDevice(name);
  }
</script>

<div class="h-full flex flex-col">
  {#if hasResults}
    <!-- Summary bar -->
    <div class="shrink-0 px-4 py-2.5 border-b border-surface-200-800 flex items-center gap-4 flex-wrap">
      <span class="text-sm font-semibold">
        {passCount} {passCount === 1 ? "match" : "matches"} found
      </span>
      {#if selectedName}
        <span class="text-xs opacity-50">Selected: <span class="font-mono font-medium opacity-80">{selectedName}</span></span>
      {/if}
      <div class="flex-1"></div>
      <button
        class="text-xs px-2 py-1 border border-surface-200-800 hover:bg-surface-200-800 opacity-70 hover:opacity-100 transition-opacity"
        onclick={() => clearIdentifyResultsContents()}
      >Clear</button>
    </div>

    <!-- Header (outside scroll container — no overlap) -->
    <div class="shrink-0 border-b border-surface-200-800 bg-surface-100-900">
      <table class="w-full text-sm table-fixed">
        <colgroup>
          <col class="w-10" />
          <col class="w-40" />
          <col class="w-32" />
          <col class="w-20" />
        </colgroup>
        <thead>
          <tr>
            <th class="px-2 py-2"></th>
            <th class="text-left px-4 py-2 text-xs font-semibold uppercase tracking-wider opacity-60">Device</th>
            <th class="text-left px-4 py-2 text-xs font-semibold uppercase tracking-wider opacity-60">Manufacturer</th>
            <th class="px-4 py-2"></th>
          </tr>
        </thead>
      </table>
    </div>

    <!-- Results list (scrollable body only) -->
    <div class="flex-1 overflow-auto min-h-0">
      <table class="w-full text-sm table-fixed">
        <colgroup>
          <col class="w-10" />
          <col class="w-40" />
          <col class="w-32" />
          <col class="w-20" />
        </colgroup>
        <tbody>
          {#each matches as r (r.name)}
            <tr
              class="border-b border-surface-200-800/50 hover:bg-surface-200-800/30 transition-colors {selectedName === r.name ? 'bg-primary-500/10' : ''}"
            >
              <td class="px-2 py-2 text-center">
                <button
                  class="shrink-0 rounded p-0.5 hover:bg-surface-200-800"
                  onclick={(e) => { e.stopPropagation(); toggleFavorite(r.name, r.manufacturer); }}
                  aria-label={favNames.has(r.name) ? 'Unfavorite' : 'Favorite'}
                  title={favNames.has(r.name) ? 'Remove from favorites' : 'Add to favorites'}
                >
                  <svg
                    class="h-4 w-4 transition-colors"
                    class:fill-yellow-400={favNames.has(r.name)}
                    class:text-yellow-400={favNames.has(r.name)}
                    class:fill-transparent={!favNames.has(r.name)}
                    class:text-gray-400={!favNames.has(r.name)}
                    viewBox="0 0 24 24"
                    stroke="currentColor"
                    stroke-width="2"
                  >
                    <path
                      stroke-linecap="round"
                      stroke-linejoin="round"
                      d="M11.049 2.927c.3-.921 1.603-.921 1.902 0l1.519 4.674a1 1 0 00.95.69h4.915c.969 0 1.371 1.24.588 1.81l-3.976 2.888a1 1 0 00-.363 1.118l1.518 4.674c.3.922-.755 1.688-1.538 1.118l-3.976-2.888a1 1 0 00-1.176 0l-3.976 2.888c-.783.57-1.838-.197-1.538-1.118l1.518-4.674a1 1 0 00-.363-1.118l-3.976-2.888c-.784-.57-.38-1.81.588-1.81h4.914a1 1 0 00.951-.69l1.519-4.674z"
                    />
                  </svg>
                </button>
              </td>
              <td class="px-4 py-2 font-mono font-medium text-success-700-200 truncate">{r.name}</td>
              <td class="px-4 py-2 opacity-70 truncate">{r.manufacturer}</td>
              <td class="px-4 py-2 text-right">
                {#if selectedName === r.name}
                  <button
                    class="text-xs px-2.5 py-1 text-primary-500 font-medium hover:underline transition-opacity opacity-80 hover:opacity-100"
                    onclick={() => deselectDevice()}
                    title="Deselect this device"
                  >✓ Selected</button>
                {:else}
                  <button
                    class="text-xs px-2.5 py-1 border border-primary-500/40 hover:bg-primary-500/20 rounded font-medium transition-colors"
                    onclick={() => onSelect(r.name)}
                  >Select</button>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else}
    <!-- Empty state: table is visible but no results -->
    <div class="h-full flex items-center justify-center">
      <div class="text-center">
        <svg xmlns="http://www.w3.org/2000/svg" class="h-8 w-8 mx-auto mb-2 opacity-40" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
          <path stroke-linecap="round" stroke-linejoin="round" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
        </svg>
        <p class="text-sm font-medium opacity-70 mb-1">No results yet</p>
        <p class="text-xs opacity-50">Click Identify above to search for matching logic ICs.</p>
      </div>
    </div>
  {/if}
</div>
