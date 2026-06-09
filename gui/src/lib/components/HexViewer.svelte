<script lang="ts">
  import { hexMeta, hexLoading, clearHexBuffer } from "../stores/hex";
  import { settings, setSetting } from "../stores/settings";

  const ROW_SIZE = 16;
  const BUFFER_ROWS = 5;

  let fontSize = $state($settings.hexViewerFontSize);
  let rowHeight = $derived(fontSize + 9);

  $effect(() => {
    fontSize = $settings.hexViewerFontSize;
  });

  function setFontSize(size: number) {
    fontSize = size;
    setSetting("hexViewerFontSize", size);
  }

  let scrollContainer: HTMLDivElement;
  let scrollTop = $state(0);
  let containerHeight = $state(400);

  function formatHex(n: number): string {
    return n.toString(16).padStart(2, "0").toUpperCase();
  }

  function formatOffset(n: number): string {
    return n.toString(16).padStart(8, "0").toUpperCase();
  }

  function toAscii(byte: number): string {
    if (byte >= 0x20 && byte < 0x7f) return String.fromCharCode(byte);
    return ".";
  }

  function onScroll() {
    if (scrollContainer) {
      scrollTop = scrollContainer.scrollTop;
    }
  }

  let totalRows = $derived($hexMeta?.data ? Math.ceil($hexMeta.data.length / ROW_SIZE) : 0);
  let startRow = $derived(Math.max(0, Math.floor(scrollTop / rowHeight) - BUFFER_ROWS));
  let endRow = $derived(Math.min(totalRows, Math.ceil((scrollTop + containerHeight) / rowHeight) + BUFFER_ROWS));
  let visibleRows = $derived(Array.from({ length: endRow - startRow }, (_, i) => startRow + i));
  let totalHeight = $derived(totalRows * rowHeight);
  let topPadding = $derived(startRow * rowHeight);

  $effect(() => {
    if (scrollContainer) {
      containerHeight = scrollContainer.clientHeight;
    }
  });
</script>

<div style="border: 1px solid #ccc; display: flex; flex-direction: column; height: 100%;">
  <div style="padding: 8px 12px; border-bottom: 1px solid #ccc; display: flex; align-items: center; justify-content: space-between;">
    <div>
      <span style="font-size: 14px; font-weight: 600;">Hex Viewer</span>
      {#if $hexMeta}
        <span style="font-size: 12px; opacity: 0.6; margin-left: 8px;">
          {$hexMeta.size.toLocaleString()} bytes
        </span>
      {/if}
    </div>
    <div class="flex items-center gap-2">
      {#if $hexMeta}
        <select
          class="text-xs opacity-60 hover:opacity-100 bg-transparent border border-surface-200-800 rounded px-1 py-0.5"
          value={fontSize}
          onchange={(e) => setFontSize(Number(e.currentTarget.value))}
          title="Font size"
        >
          <option value={10}>10px</option>
          <option value={11}>11px</option>
          <option value={12}>12px</option>
          <option value={13}>13px</option>
          <option value={14}>14px</option>
          <option value={16}>16px</option>
        </select>
        {#if fontSize !== 13}
          <button
            class="text-xs opacity-40 hover:opacity-80 transition-opacity underline"
            onclick={() => setFontSize(13)}
            title="Reset font size"
          >
            Reset
          </button>
        {/if}
        <button
          class="text-xs opacity-50 hover:opacity-100 transition-opacity px-2 py-0.5 rounded border border-transparent hover:border-surface-200-800"
          onclick={clearHexBuffer}
        >
          Clear
        </button>
      {/if}
    </div>
  </div>
  <div
    bind:this={scrollContainer}
    onscroll={onScroll}
    style="flex: 1; overflow: auto; font-family: 'Consolas', 'Courier New', monospace; font-size: {fontSize}px; line-height: {rowHeight}px; padding: 8px;"
  >
    {#if $hexLoading}
      <div style="display: flex; align-items: center; justify-content: center; height: 100%; gap: 8px;">
        <div class="spinner"></div>
        <span style="opacity: 0.6;">Loading...</span>
      </div>
    {:else if $hexMeta?.data && $hexMeta.data.length > 0}
      <div style="height: {totalHeight}px; position: relative;">
        <div style="height: {topPadding}px;"></div>
        {#each visibleRows as rowIdx (rowIdx)}
          {@const offset = rowIdx * ROW_SIZE}
          {@const end = Math.min(offset + ROW_SIZE, $hexMeta.data.length)}
          {@const len = end - offset}
          <div style="display: flex; white-space: nowrap; height: {rowHeight}px;">
            <span style="width: 80px; color: #888; flex-shrink: 0;">{formatOffset(offset)}</span>
            <span style="width: 340px; flex-shrink: 0; display: flex; gap: 4px;">
              {#each {length: len} as _, j}
                {@const b = $hexMeta.data[offset + j]}
                <span>{formatHex(b)}</span>
              {/each}
            </span>
            <span style="opacity: 0.7;">
              {#each {length: len} as _, j}
                {@const b = $hexMeta.data[offset + j]}
                {toAscii(b)}
              {/each}
            </span>
          </div>
        {/each}
      </div>
    {:else}
      <p style="opacity: 0.4;">No data loaded.</p>
    {/if}
  </div>
</div>

<style>
  .spinner {
    width: 20px;
    height: 20px;
    border: 2px solid #ccc;
    border-top-color: #333;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to { transform: rotate(360deg); }
  }
</style>
