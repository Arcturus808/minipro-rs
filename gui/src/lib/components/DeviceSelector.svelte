<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { Store } from "@tauri-apps/plugin-store";
  import { selectedDevice } from "../stores/device";

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

  // Device favorites — a set of device names persisted to localStorage.
  const FAVORITES_KEY = "minipro_device_favorites";

  function loadFavorites(): Set<string> {
    try {
      const raw = localStorage.getItem(FAVORITES_KEY);
      return raw ? new Set(JSON.parse(raw)) : new Set();
    } catch {
      return new Set();
    }
  }

  let favorites = $state<Set<string>>(loadFavorites());

  $effect(() => {
    localStorage.setItem(FAVORITES_KEY, JSON.stringify([...favorites]));
  });

  function isFavorite(name: string): boolean {
    return favorites.has(name);
  }

  function toggleFavorite(name: string) {
    const next = new Set(favorites);
    if (next.has(name)) next.delete(name);
    else next.add(name);
    favorites = next;
  }

  // Sort results: favorites first, then preserve backend order (stable sort).
  let sortedResults = $derived(
    [...results].sort((a, b) => {
      const af = favorites.has(a.name);
      const bf = favorites.has(b.name);
      if (af === bf) return 0;
      return af ? -1 : 1;
    })
  );

  // Favorite device names for the empty-state list.
  let favoriteItems = $derived(
    Array.from(favorites).sort((a, b) => a.localeCompare(b))
  );

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
    // Immediate search (Enter key bypasses the debounce).
    if (debounceTimer) clearTimeout(debounceTimer);
    await doSearch(searchQuery);
  }

  function goPrev() { if (page > 0) page--; }
  function goNext() {
    const maxPage = Math.ceil(sortedResults.length / PAGE_SIZE) - 1;
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
  let pageItems = $derived(sortedResults.slice(start, start + PAGE_SIZE));
  let totalPages = $derived(Math.max(1, Math.ceil(sortedResults.length / PAGE_SIZE)));
  let displayItems = $derived(viewMode === "paginated" ? pageItems : sortedResults);
</script>

<div class="card preset-filled-surface-100-900 border border-surface-200-800 flex flex-col h-full">
  <header class="p-3 border-b border-surface-200-800">
    <h3 class="text-sm font-semibold mb-2">Device Selector</h3>
    <input
      type="text"
      bind:value={searchQuery}
      placeholder="Search devices..."
      class="w-full rounded border border-gray-300 dark:border-slate-600 bg-white dark:bg-slate-800 text-surface-950-50 px-3 py-2 focus:outline-none focus:ring-2 focus:ring-blue-500"
      onkeydown={(e) => { if (e.key === 'Enter') onSearch(); }}
    />
  </header>

  <div class="flex-1 overflow-auto p-2">
    {#if results.length === 0}
      {#if searchQuery.trim().length > 0 && searchQuery.trim().length < 2}
        <p class="text-sm opacity-50 text-center py-8">Keep typing...</p>
      {:else if searchQuery.trim().length >= 2}
        <p class="text-sm opacity-50 text-center py-8">No results found.</p>
      {:else if favoriteItems.length > 0}
        <div class="text-xs opacity-60 mb-1">Favorites</div>
        <ul class="divide-y divide-surface-200-800">
          {#each favoriteItems as name}
            <li>
              <div
                class={`w-full text-left py-2 px-3 transition-colors flex items-center gap-2 ${selectedName === name ? 'bg-primary-500/10 border-l-4 border-primary-500' : 'hover:bg-surface-200-800 border-l-4 border-transparent'}`}
                role="button"
                tabindex="0"
                onclick={() => onSelect(name)}
                onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onSelect(name); } }}
              >
                <button
                  class="shrink-0 rounded p-0.5 hover:bg-surface-200-800"
                  onclick={(e) => { e.stopPropagation(); toggleFavorite(name); }}
                  aria-label={isFavorite(name) ? 'Unfavorite' : 'Favorite'}
                  title={isFavorite(name) ? 'Remove from favorites' : 'Add to favorites'}
                >
                  <svg
                    class="h-4 w-4 transition-colors"
                    class:fill-yellow-400={isFavorite(name)}
                    class:text-yellow-400={isFavorite(name)}
                    class:fill-transparent={!isFavorite(name)}
                    class:text-gray-400={!isFavorite(name)}
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
                <span class={`text-sm flex-1 ${selectedName === name ? 'font-semibold' : ''}`}>{name}</span>
              </div>
            </li>
          {/each}
        </ul>
      {:else}
        <p class="text-sm opacity-50 text-center py-8">Start typing to search devices...</p>
      {/if}
    {:else}
      <div class="text-xs opacity-60 mb-1 flex justify-between items-center">
        <span>{sortedResults.length} total</span>
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
            <div
              class={`w-full text-left py-2 px-3 transition-colors flex items-center gap-2 ${selectedName === item.name ? 'bg-primary-500/10 border-l-4 border-primary-500' : 'hover:bg-surface-200-800 border-l-4 border-transparent'}`}
              role="button"
              tabindex="0"
              onclick={() => onSelect(item.name)}
              onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onSelect(item.name); } }}
            >
              <button
                class="shrink-0 rounded p-0.5 hover:bg-surface-200-800"
                onclick={(e) => { e.stopPropagation(); toggleFavorite(item.name); }}
                aria-label={isFavorite(item.name) ? 'Unfavorite' : 'Favorite'}
                title={isFavorite(item.name) ? 'Remove from favorites' : 'Add to favorites'}
              >
                <svg
                  class="h-4 w-4 transition-colors"
                  class:fill-yellow-400={isFavorite(item.name)}
                  class:text-yellow-400={isFavorite(item.name)}
                  class:fill-transparent={!isFavorite(item.name)}
                  class:text-gray-400={!isFavorite(item.name)}
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
              <span class={`text-sm flex-1 ${selectedName === item.name ? 'font-semibold' : ''}`}>{item.name}</span>
              <span class="text-xs opacity-60 truncate max-w-[120px]">{item.manufacturer}</span>
            </div>
          </li>
        {/each}
      </ul>
      {#if viewMode === "paginated" && sortedResults.length > PAGE_SIZE}
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
