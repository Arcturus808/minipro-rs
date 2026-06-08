<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  let searchQuery = $state("");
  let results = $state<string[]>([]);
  let page = $state(0);
  let selectedName = $state<string | null>(null);
  let selectedInfo = $state<any>(null);
  const PAGE_SIZE = 20;

  async function onSearch() {
    page = 0;
    selectedName = null;
    selectedInfo = null;
    results = await invoke<string[]>("search_devices", { query: searchQuery });
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === "Enter") onSearch();
  }

  function goPrev() { if (page > 0) page--; }
  function goNext() {
    const maxPage = Math.ceil(results.length / PAGE_SIZE) - 1;
    if (page < maxPage) page++;
  }

  async function onSelect(name: string) {
    selectedName = name;
    selectedInfo = await invoke("select_device", { name });
  }

  function onDeselect() {
    selectedName = null;
    selectedInfo = null;
  }

  let start = $derived(page * PAGE_SIZE);
  let pageItems = $derived(results.slice(start, start + PAGE_SIZE));
  let totalPages = $derived(Math.max(1, Math.ceil(results.length / PAGE_SIZE)));
</script>

<div class="card preset-filled-surface-100-900 border border-surface-200-800 flex flex-col h-full">
  <header class="p-3 border-b border-surface-200-800">
    <h3 class="text-sm font-semibold mb-2">Device Selector</h3>
    <div class="flex gap-2">
      <input
        type="text"
        class="input flex-1 text-sm"
        placeholder="Search devices..."
        bind:value={searchQuery}
        onkeydown={handleKeydown}
      />
      <button class="btn preset-filled-primary text-sm px-3" onclick={onSearch}>Search</button>
    </div>
  </header>

  <div class="flex-1 overflow-auto p-2">
    {#if results.length === 0}
      <p class="text-sm opacity-50 text-center py-8">No devices found. Enter a search term.</p>
    {:else}
      <div class="text-xs opacity-60 mb-1 flex justify-between">
        <span>{results.length} total</span>
        <span>Page {page + 1} / {totalPages}</span>
      </div>
      <ul class="space-y-1">
        {#each pageItems as name}
          <li>
            <button
              class="w-full text-left text-sm px-2 py-1 rounded hover:preset-tonal-primary transition-colors"
              class:preset-filled-primary-100-900={selectedName === name}
              onclick={() => onSelect(name)}
            >
              {name}
            </button>
          </li>
        {/each}
      </ul>
      {#if results.length > PAGE_SIZE}
        <div class="flex justify-between mt-2">
          <button class="btn preset-tonal text-xs px-2" onclick={goPrev} disabled={page === 0}>Prev</button>
          <button class="btn preset-tonal text-xs px-2" onclick={goNext} disabled={page + 1 >= totalPages}>Next</button>
        </div>
      {/if}
    {/if}
  </div>

  {#if selectedInfo}
    <footer class="p-3 border-t border-surface-200-800 space-y-1">
      <div class="flex items-center justify-between">
        <span class="font-semibold text-sm">{selectedInfo.name}</span>
        <button class="text-xs opacity-60 hover:opacity-100" onclick={onDeselect}>Clear</button>
      </div>
      <div class="text-xs">Type: {selectedInfo.chip_type} | Pins: {selectedInfo.pin_count}</div>
      <div class="text-xs">
        VPP: {selectedInfo.voltages.vpp}V | VDD: {selectedInfo.voltages.vdd}V | VCC: {selectedInfo.voltages.vcc}V
      </div>
    </footer>
  {/if}
</div>
