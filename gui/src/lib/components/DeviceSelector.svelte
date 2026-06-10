<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { Store } from "@tauri-apps/plugin-store";
  import { selectedDevice } from "../stores/device";
  import { readFuses, writeFuses } from "../stores/operations";
  import { logs } from "../stores/logs";
  import ComboSearch from "./ComboSearch.svelte";

  let searchQuery = $state("");
  let results = $state<string[]>([]);
  let page = $state(0);
  let selectedName = $state<string | null>(null);
  let selectedInfo = $state<any>(null);
  let viewMode = $state<"paginated" | "scroll">("paginated");
  let fuseBytes = $state<Record<number, number[]>>({});
  let showConfig = $state(false);
  const PAGE_SIZE = 12;
  let store: Store | null = null;

  async function onSearch() {
    const trimmed = searchQuery.trim();
    if (!trimmed) return;
    page = 0;
    selectedName = null;
    selectedInfo = null;
    results = await invoke<string[]>("search_devices", { query: trimmed });
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
    fuseBytes = {};
    showConfig = false;
    selectedDevice.set(null);
  }

  async function readAllFuses() {
    if (!selectedInfo?.config || selectedInfo.config.type !== "Mcu") return;
    try {
      const user = await readFuses(0);
      const cfg = await readFuses(1);
      const lock = await readFuses(2);
      fuseBytes = { 0: user, 1: cfg, 2: lock };
      logs.info("Config read successfully");
    } catch (e) {
      logs.error(`Config read failed: ${e}`);
    }
  }

  function decodeFuseValue(bytes: number[], index: number, mask: number): boolean {
    if (!bytes || index >= bytes.length) return false;
    return (bytes[index] & mask) !== 0;
  }

  function isDangerousFuse(name: string): boolean {
    const lower = name.toLowerCase();
    return [
      "rstdisbl", "disable reset", "rstdis",
      "spien", "enable spi", "disable spi",
      "jtagen", "jtag",
      "dwen", "debugwire",
    ].some((k) => lower.includes(k));
  }

  function toggleFuseBit(fuseType: number, byteIndex: number, mask: number) {
    const copy = { ...fuseBytes };
    const arr = [...(copy[fuseType] || [])];
    if (byteIndex >= arr.length) {
      for (let i = arr.length; i <= byteIndex; i++) arr.push(0);
    }
    arr[byteIndex] ^= mask;
    copy[fuseType] = arr;
    fuseBytes = copy;
  }

  async function writeAllFuses() {
    try {
      if (fuseBytes[1]?.length) await writeFuses(1, fuseBytes[1]);
      if (fuseBytes[2]?.length) await writeFuses(2, fuseBytes[2]);
      logs.info("Config written to chip");
    } catch (e) {
      logs.error(`Config write failed: ${e}`);
    }
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
      <p class="text-sm opacity-50 text-center py-8">No devices found. Enter a search term.</p>
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
        {#each displayItems as name}
          <li>
            <button
              class={`w-full text-left text-sm py-2 px-3 transition-colors ${selectedName === name ? 'bg-primary-500/10 border-l-4 border-primary-500 font-semibold' : 'hover:bg-surface-200-800 border-l-4 border-transparent'}`}
              onclick={() => onSelect(name)}
            >
              {name}
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
      <div class="text-xs">{selectedInfo.chip_type} · {selectedInfo.package_type} · {selectedInfo.pin_count} pins</div>
      <div class="text-xs">
        VPP: {selectedInfo.voltages.vpp}V · VDD: {selectedInfo.voltages.vdd}V · VCC: {selectedInfo.voltages.vcc}V
      </div>
      <div class="text-xs">
        Code: {codeKb} · Data: {dataKb}
        {#if selectedInfo.can_erase}<span class="ml-1 opacity-60">· Erasable</span>{/if}
        {#if selectedInfo.has_chip_id}<span class="ml-1 opacity-60">· Chip ID</span>{/if}
      </div>
      {#if selectedInfo.config && selectedInfo.config.type === "Mcu"}
        <div class="pt-2 border-t border-surface-200-800 space-y-2">
          <div class="flex items-center justify-between">
            <span class="text-xs font-semibold opacity-70 uppercase tracking-wide">Configuration</span>
            <button
              class="btn preset-tonal text-xs px-2 py-0.5"
              onclick={() => showConfig = !showConfig}
            >
              {showConfig ? "Hide" : "Show"}
            </button>
          </div>
          {#if showConfig}
            <div class="space-y-2">
              <button
                class="btn preset-tonal text-xs px-2 py-1 w-full"
                onclick={readAllFuses}
              >
                Read Config from Chip
              </button>
              {#if selectedInfo.config.fuses.length > 0}
                <div class="space-y-1">
                  <span class="text-xs font-semibold opacity-70">Fuses</span>
                  {#each selectedInfo.config.fuses as field, i}
                    <label class="flex items-center gap-2 text-xs cursor-pointer">
                      <input
                        type="checkbox"
                        class="checkbox"
                        checked={decodeFuseValue(fuseBytes[1] || [], i, field.mask)}
                        disabled={!fuseBytes[1]}
                        onchange={() => toggleFuseBit(1, i, field.mask)}
                      />
                      <span class={isDangerousFuse(field.name) ? "text-red-500 font-semibold" : ""}>{field.name}</span>
                      {#if isDangerousFuse(field.name)}<span class="text-red-500 text-[10px]" title="Dangerous — may disable programming access">!</span>{/if}
                      {#if !fuseBytes[1]}<span class="opacity-40">(not read)</span>{/if}
                    </label>
                  {/each}
                </div>
              {/if}
              {#if selectedInfo.config.locks.length > 0}
                <div class="space-y-1">
                  <span class="text-xs font-semibold opacity-70">Lock Bits</span>
                  {#each selectedInfo.config.locks as field, i}
                    <label class="flex items-center gap-2 text-xs cursor-pointer">
                      <input
                        type="checkbox"
                        class="checkbox"
                        checked={decodeFuseValue(fuseBytes[2] || [], i, field.mask)}
                        disabled={!fuseBytes[2]}
                        onchange={() => toggleFuseBit(2, i, field.mask)}
                      />
                      <span class={isDangerousFuse(field.name) ? "text-red-500 font-semibold" : ""}>{field.name}</span>
                      {#if isDangerousFuse(field.name)}<span class="text-red-500 text-[10px]" title="Dangerous — may disable programming access">!</span>{/if}
                      {#if !fuseBytes[2]}<span class="opacity-40">(not read)</span>{/if}
                    </label>
                  {/each}
                </div>
              {/if}
              {#if fuseBytes[1]?.length || fuseBytes[2]?.length}
                <button
                  class="btn preset-filled-primary text-xs px-2 py-1 w-full"
                  onclick={writeAllFuses}
                >
                  Write Config to Chip
                </button>
              {/if}
            </div>
          {/if}
        </div>
      {/if}
    </footer>
  {/if}
</div>
