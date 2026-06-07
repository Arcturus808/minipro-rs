<script lang="ts">
  import { deviceList, selectedDevice, searchDevices, selectDevice, deselectDevice } from "../stores/device";
  import { logs } from "../stores/logs";

  let searchQuery = $state("");
  let isLoading = $state(false);

  async function onSearch() {
    isLoading = true;
    try {
      await searchDevices(searchQuery);
      logs.info(`Found ${$deviceList.length} devices`);
    } catch (e) {
      logs.error(`Search failed: ${e}`);
    } finally {
      isLoading = false;
    }
  }

  async function onSelect(name: string) {
    try {
      await selectDevice(name);
      logs.info(`Selected device: ${name}`);
    } catch (e) {
      logs.error(`Failed to select device: ${e}`);
    }
  }

  async function onDeselect() {
    try {
      await deselectDevice();
      logs.info("Device deselected");
    } catch (e) {
      logs.error(`Failed to deselect device: ${e}`);
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === "Enter") {
      onSearch();
    }
  }
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
      <button class="btn preset-filled-primary text-sm px-3" onclick={onSearch} disabled={isLoading}>
        {isLoading ? "..." : "Search"}
      </button>
    </div>
  </header>

  <div class="flex-1 overflow-auto p-2">
    {#if $deviceList.length === 0}
      <p class="text-sm opacity-50 text-center py-8">No devices found. Enter a search term.</p>
    {:else}
      <ul class="space-y-1">
        {#each $deviceList as name}
          <li>
            <button
              class="w-full text-left text-sm px-2 py-1 rounded hover:preset-tonal-primary transition-colors"
              class:preset-filled-primary-100-900={$selectedDevice?.name === name}
              onclick={() => onSelect(name)}
            >
              {name}
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </div>

  {#if $selectedDevice}
    <footer class="p-3 border-t border-surface-200-800 space-y-1">
      <div class="flex items-center justify-between">
        <span class="font-semibold text-sm">{$selectedDevice.name}</span>
        <button class="text-xs opacity-60 hover:opacity-100" onclick={onDeselect}>Clear</button>
      </div>
      <div class="text-xs">Type: {$selectedDevice.chip_type} | Pins: {$selectedDevice.pin_count}</div>
      <div class="text-xs">
        VPP: {$selectedDevice.voltages.vpp}V | VDD: {$selectedDevice.voltages.vdd}V | VCC:
        {$selectedDevice.voltages.vcc}V
      </div>
    </footer>
  {/if}
</div>
