<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { Store } from "@tauri-apps/plugin-store";
  import { selectedDevice } from "../stores/device";
  import ComboSearch from "./ComboSearch.svelte";

  interface SearchResult {
    name: string;
    manufacturer: string;
  }

  let searchQuery = $state("");
  let results = $state<SearchResult[]>([]);
  let page = $state(0);
  let selectedName = $state<string | null>(null);
  let selectedInfo = $state<any>(null);
  let viewMode = $state<"paginated" | "scroll">("paginated");
  const PAGE_SIZE = 12;
  let store: Store | null = null;

  // Live search: debounce + race-condition guard.
  // A monotonic counter tags each request; only the latest response is kept.
  let searchSeq = 0;
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;

  async function doSearch(query: string) {
    const trimmed = query.trim();
    if (trimmed.length < 2) {
      results = [];
      page = 0;
      return;
    }
    const seq = ++searchSeq;
    const r = await invoke<SearchResult[]>("search_devices", { query: trimmed });
    // Discard stale responses (user typed more characters since this request).
    if (seq !== searchSeq) return;
    results = r;
    page = 0;
    selectedName = null;
    selectedInfo = null;
  }

  // Debounced live search: fires 200ms after the user stops typing.
  $effect(() => {
    const query = searchQuery;
    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => doSearch(query), 200);
    return () => { if (debounceTimer) clearTimeout(debounceTimer); };
  });

  async function onSearch() {
    // Immediate search (Search button or Enter from ComboSearch).
    if (debounceTimer) clearTimeout(debounceTimer);
    await doSearch(searchQuery);
  }

  function goPrev() { if (page > 0) page--; }
  function goNext() {
    const maxPage = Math.ceil(results.length / PAGE_SIZE) - 1;
    if (page < maxPage) page++;
  }

  onMount(async () => {
    store = await Store.load("settings.json");
    const saved = await store.get<string>("deviceViewMode");
    if (saved === "scroll" || saved === "paginated") {
      viewMode = saved;
    }
  });

  async function toggleView() {
    viewMode = viewMode === "paginated" ? "scroll" : "paginated";
    page = 0;
    if (store) {
      await store.set("deviceViewMode", viewMode);
      await store.save();
    }
  }

  async function onSelect(name: string) {
    selectedName = name;
    selectedInfo = await invoke("select_device", { name });
    selectedDevice.set(selectedInfo);
  }

  function onDeselect() {
    selectedName = null;
    selectedInfo = null;
    selectedDevice.set(null);
  }

  let start = $derived(page * PAGE_SIZE);
  let pageItems = $derived(results.slice(start, start + PAGE_SIZE));
  let totalPages = $derived(Math.max(1, Math.ceil(results.length / PAGE_SIZE)));
  let displayItems = $derived(viewMode === "paginated" ? pageItems : results);
</script>

<div class="card preset-filled-surface-100-900 border border-surface-200-800 flex flex-col h-full">
  <header class="p-3 border-b border-surface-200-800">
    <h3 class="text-sm font-semibold mb-2">Device Selector</h3>
    <div class="flex gap-2">
      <div class="flex-1">
        <ComboSearch
          bind:value={searchQuery}
          placeholder="Search devices..."
          storageKey="minipro_device_search_history"
          onselect={() => onSearch()}
          onsubmit={() => onSearch()}
        />
      </div>
      <button class="btn preset-filled-primary text-sm px-3" onclick={onSearch}>Search</button>
    </div>
  </header>

  <div class="flex-1 overflow-auto p-2">
    {#if results.length === 0}
      <p class="text-sm opacity-50 text-center py-8">
        {searchQuery.trim().length > 0 && searchQuery.trim().length < 2
          ? "Keep typing..."
          : "Start typing to search devices..."}
      </p>
    {:else}
      <div class="text-xs opacity-60 mb-1 flex justify-between items-center">
        <span>{results.length} total</span>
        <div class="flex items-center gap-2">
          {#if viewMode === "paginated"}
            <span>Page {page + 1} / {totalPages}</span>
          {/if}
          <button
            class="btn preset-tonal text-xs px-2 py-0.5"
            onclick={toggleView}
            title={viewMode === "paginated" ? "Switch to scroll view" : "Switch to paginated view"}
          >
            {viewMode === "paginated" ? "Scroll" : "Paginate"}
          </button>
        </div>
      </div>
      <ul class="divide-y divide-surface-200-800">
        {#each displayItems as item}
          <li>
            <button
              class={`w-full text-left py-2 px-3 transition-colors ${selectedName === item.name ? 'bg-primary-500/10 border-l-4 border-primary-500' : 'hover:bg-surface-200-800 border-l-4 border-transparent'}`}
              onclick={() => onSelect(item.name)}
            >
              <div class="flex items-center justify-between gap-2">
                <span class={`text-sm ${selectedName === item.name ? 'font-semibold' : ''}`}>{item.name}</span>
                <span class="text-xs opacity-60 truncate max-w-[120px]">{item.manufacturer}</span>
              </div>
            </button>
          </li>
        {/each}
      </ul>
      {#if viewMode === "paginated" && results.length > PAGE_SIZE}
        <div class="flex justify-between mt-2">
          <button class="btn preset-tonal text-xs px-2" onclick={goPrev} disabled={page === 0}>Prev</button>
          <button class="btn preset-tonal text-xs px-2" onclick={goNext} disabled={page + 1 >= totalPages}>Next</button>
        </div>
      {/if}
    {/if}
  </div>

  {#if selectedInfo}
    {@const codeKb = selectedInfo.code_memory_size > 0 ? (selectedInfo.code_memory_size / 1024).toFixed(1) + " KB" : "—"}
    {@const dataKb = selectedInfo.data_memory_size > 0 ? (selectedInfo.data_memory_size / 1024).toFixed(1) + " KB" : "—"}
    <footer class="p-3 border-t border-surface-200-800 space-y-1">
      <div class="flex items-center justify-between">
        <span class="text-xs font-semibold opacity-70 uppercase tracking-wide">Selected Device</span>
        <button class="text-xs opacity-60 hover:opacity-100" onclick={onDeselect}>Clear</button>
      </div>
      <div class="flex items-center justify-between">
        <span class="font-semibold text-sm">{selectedInfo.name}</span>
      </div>
      <div class="text-xs">{selectedInfo.manufacturer} · {selectedInfo.chip_type} · {selectedInfo.package_type} · {selectedInfo.pin_count} pins</div>
      <div class="text-xs">
        VPP: {selectedInfo.voltages.vpp}V · VDD: {selectedInfo.voltages.vdd}V · VCC: {selectedInfo.voltages.vcc}V
      </div>
      <div class="text-xs">
        Code: {codeKb} · Data: {dataKb}
        {#if selectedInfo.can_erase}<span class="ml-1 opacity-60">· Erasable</span>{/if}
        {#if selectedInfo.has_chip_id}<span class="ml-1 opacity-60">· Chip ID</span>{/if}
      </div>
    </footer>
  {/if}
</div>
