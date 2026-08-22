<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { Store } from "@tauri-apps/plugin-store";
  import { selectedDevice, programmer } from "../stores/device";

  interface SearchResult {
    name: string;
    manufacturer: string;
  }

  interface SpiAutodetectResult {
    jedec_id: number;
    matches: SearchResult[];
  }

  let searchQuery = $state("");
  let results = $state<SearchResult[]>([]);
  let page = $state(0);
  let selectedName = $state<string | null>(null);
  let selectedInfo = $state<any>(null);
  let viewMode = $state<"paginated" | "scroll">("paginated");
  const PAGE_SIZE = 12;
  let store: Store | null = null;

  // SPI flash autodetect state
  let autodetecting = $state(false);
  let autodetectResults = $state<SearchResult[] | null>(null);
  let autodetectJedecId = $state<string | null>(null);
  let autodetectError = $state<string | null>(null);

  // Autodetect is supported on TL866A, TL866CS, TL866II+, and T48.
  // T56 and T76 need protocol implementations (gaps 2/3).
  let autodetectSupported = $derived(
    $programmer !== null && !["T56", "T76"].includes($programmer.model)
  );

  // Live search: debounce + race-condition guard.
  // A monotonic counter tags each request; only the latest response is kept.
  let searchSeq = 0;
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;

  // Device favorites — array of {name, manufacturer} persisted to localStorage.
  interface FavoriteEntry {
    name: string;
    manufacturer: string;
  }

  const FAVORITES_KEY = "minipro_device_favorites";

  function loadFavorites(): FavoriteEntry[] {
    try {
      const raw = localStorage.getItem(FAVORITES_KEY);
      if (!raw) return [];
      const parsed = JSON.parse(raw);
      // Migrate old format (array of strings) to new format
      if (parsed.length > 0 && typeof parsed[0] === "string") {
        return parsed.map((name: string) => ({ name, manufacturer: "" }));
      }
      return parsed as FavoriteEntry[];
    } catch {
      return [];
    }
  }

  let favorites = $state<FavoriteEntry[]>(loadFavorites());

  $effect(() => {
    localStorage.setItem(FAVORITES_KEY, JSON.stringify(favorites));
  });

  function isFavorite(name: string): boolean {
    return favorites.some((f) => f.name === name);
  }

  function toggleFavorite(name: string, manufacturer?: string) {
    if (isFavorite(name)) {
      favorites = favorites.filter((f) => f.name !== name);
    } else {
      favorites = [...favorites, { name, manufacturer: manufacturer ?? "" }];
    }
  }

  // Favorites section collapse state — persisted to localStorage.
  const FAV_COLLAPSED_KEY = "minipro_device_favorites_collapsed";

  function loadFavCollapsed(): boolean {
    return localStorage.getItem(FAV_COLLAPSED_KEY) === "true";
  }

  let favoritesCollapsed = $state<boolean>(loadFavCollapsed());

  $effect(() => {
    localStorage.setItem(FAV_COLLAPSED_KEY, String(favoritesCollapsed));
  });

  // Favorite devices (sorted by name) for the pinned favorites section.
  let favoriteItems = $derived(
    [...favorites].sort((a, b) => a.name.localeCompare(b.name))
  );

  async function doSearch(query: string) {
    const trimmed = query.trim();
    if (trimmed.length < 2) {
      results = [];
      page = 0;
      return;
    }
    // Clear autodetect results when user starts searching
    if (autodetectResults !== null) clearAutoDetect();
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

  async function doAutoDetect(idType: number) {
    autodetecting = true;
    autodetectResults = null;
    autodetectJedecId = null;
    autodetectError = null;
    try {
      const result = await invoke<SpiAutodetectResult>("do_spi_autodetect", { idType });
      autodetectJedecId = "0x" + result.jedec_id.toString(16).toUpperCase().padStart(4, "0");
      autodetectResults = result.matches;
    } catch (e: any) {
      autodetectError = String(e?.message ?? e);
    } finally {
      autodetecting = false;
    }
  }

  function clearAutoDetect() {
    autodetectResults = null;
    autodetectJedecId = null;
    autodetectError = null;
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
    <input
      type="text"
      bind:value={searchQuery}
      placeholder="Search devices..."
      class="w-full rounded border border-gray-300 dark:border-slate-600 bg-white dark:bg-slate-800 text-surface-950-50 px-3 py-2 focus:outline-none focus:ring-2 focus:ring-blue-500"
      onkeydown={(e) => { if (e.key === 'Enter') onSearch(); }}
    />
    {#if autodetectSupported}
      <div class="flex items-center gap-2 mt-2">
        <span class="text-xs opacity-60">SPI Auto Detect:</span>
        <button
          class="btn preset-tonal text-xs px-2 py-1"
          onclick={() => doAutoDetect(0)}
          disabled={autodetecting}
          title="Detect 8-pin SPI flash (25xx)"
        >
          {autodetecting ? "..." : "8-pin"}
        </button>
        <button
          class="btn preset-tonal text-xs px-2 py-1"
          onclick={() => doAutoDetect(1)}
          disabled={autodetecting}
          title="Detect 16-pin SPI flash (25xx)"
        >
          {autodetecting ? "..." : "16-pin"}
        </button>
      </div>
    {/if}
  </header>

  <div class="flex-1 overflow-auto p-2">
    {#if autodetectResults !== null || autodetectError !== null}
      <div class="mb-3">
        <div class="flex items-center justify-between mb-1">
          <span class="text-xs font-semibold opacity-70 uppercase tracking-wide">
            {#if autodetectError}
              Error
            {:else if autodetectResults && autodetectResults.length > 0}
              JEDEC ID: {autodetectJedecId} · {autodetectResults.length} match(es)
            {:else if autodetectJedecId === "0x0000"}
              No SPI chip detected
            {:else}
              No device found (JEDEC ID: {autodetectJedecId})
            {/if}
          </span>
          <button class="text-xs opacity-60 hover:opacity-100" onclick={clearAutoDetect}>Clear</button>
        </div>
        {#if autodetectError}
          <p class="text-xs text-red-500 px-1">{autodetectError}</p>
        {:else if autodetectResults && autodetectResults.length > 0}
          <ul class="divide-y divide-surface-200-800">
            {#each autodetectResults as item}
              <li>{@render starRow(item.name, item.manufacturer)}</li>
            {/each}
          </ul>
        {:else if autodetectJedecId === "0x0000"}
          <p class="text-xs opacity-50 px-1">Make sure a 25xx SPI flash is inserted in the ZIF socket.</p>
        {:else}
          <p class="text-xs opacity-50 px-1">Try the other pin count option.</p>
        {/if}
      </div>
    {/if}

    {#snippet starRow(name: string, manufacturer?: string)}
      <div
        class={`w-full text-left py-2 px-3 transition-colors flex items-center gap-2 ${selectedName === name ? 'bg-primary-500/10 border-l-4 border-primary-500' : 'hover:bg-surface-200-800 border-l-4 border-transparent'}`}
        role="button"
        tabindex="0"
        onclick={() => onSelect(name)}
        onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onSelect(name); } }}
      >
        <button
          class="shrink-0 rounded p-0.5 hover:bg-surface-200-800"
          onclick={(e) => { e.stopPropagation(); toggleFavorite(name, manufacturer); }}
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
        {#if manufacturer}
          <span class="text-xs opacity-60 truncate max-w-[120px]">{manufacturer}</span>
        {/if}
      </div>
    {/snippet}

    {#if favoriteItems.length > 0}
      <div class="mb-2">
        <button
          class="w-full flex items-center gap-1 text-xs font-semibold opacity-70 uppercase tracking-wide py-1 px-1 hover:opacity-100 transition-opacity"
          onclick={() => favoritesCollapsed = !favoritesCollapsed}
          aria-expanded={!favoritesCollapsed}
        >
          <svg class="h-3 w-3 transition-transform" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2" style={favoritesCollapsed ? '' : 'transform: rotate(90deg)'}>
            <path stroke-linecap="round" stroke-linejoin="round" d="M9 5l7 7-7 7" />
          </svg>
          Favorites ({favoriteItems.length})
        </button>
        {#if !favoritesCollapsed}
          <ul class="divide-y divide-surface-200-800">
            {#each favoriteItems as fav}
              <li>{@render starRow(fav.name, fav.manufacturer || undefined)}</li>
            {/each}
          </ul>
        {/if}
      </div>
    {/if}

    {#if results.length === 0}
      {#if searchQuery.trim().length > 0 && searchQuery.trim().length < 2}
        <p class="text-sm opacity-50 text-center py-8">Keep typing...</p>
      {:else if searchQuery.trim().length >= 2}
        <p class="text-sm opacity-50 text-center py-8">No results found.</p>
      {:else if favoriteItems.length === 0}
        <p class="text-sm opacity-50 text-center py-8">Start typing to search devices...</p>
      {/if}
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
          <li>{@render starRow(item.name, item.manufacturer)}</li>
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
        VPP: {selectedInfo.voltages.vpp === "—" || selectedInfo.voltages.vpp === "?" ? selectedInfo.voltages.vpp : `${selectedInfo.voltages.vpp}V`} · VDD: {selectedInfo.voltages.vdd === "—" || selectedInfo.voltages.vdd === "?" ? selectedInfo.voltages.vdd : `${selectedInfo.voltages.vdd}V`} · VCC: {selectedInfo.voltages.vcc === "—" || selectedInfo.voltages.vcc === "?" ? selectedInfo.voltages.vcc : `${selectedInfo.voltages.vcc}V`}
      </div>
      <div class="text-xs">
        Code: {codeKb} · Data: {dataKb}
        {#if selectedInfo.can_erase}<span class="ml-1 opacity-60">· Erasable</span>{/if}
        {#if selectedInfo.has_chip_id}<span class="ml-1 opacity-60">· Chip ID</span>{/if}
      </div>
    </footer>
  {/if}
</div>
