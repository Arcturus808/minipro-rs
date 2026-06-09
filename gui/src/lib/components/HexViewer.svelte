<script lang="ts">
  import { hexMeta, hexLoading, clearHexBuffer } from "../stores/hex";
  import { settings, setSetting } from "../stores/settings";
  import { saveBufferToFile, openFolder } from "../stores/operations";
  import { pickSaveFile } from "../file-dialog";
  import { get } from "svelte/store";

  const ROW_SIZE = 16;
  const BUFFER_ROWS = 5;

  let fontSize = $state($settings.hexViewerFontSize);
  let rowHeight = $derived(fontSize + 9);
  let savedPath = $state<string | null>(null);

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
          class="text-xs opacity-50 hover:opacity-100 transition-opacity px-2 py-0.5 rounded border border-transparent hover:border-surface-200-800 flex items-center gap-1"
          onclick={async () => {
            const dir = get(settings).defaultDirectory ?? "";
            const defaultPath = dir ? `${dir}\\dump.bin` : "dump.bin";
            let path = await pickSaveFile(
              "Save chip dump as",
              defaultPath,
              [{ name: "Binary", extensions: ["bin"] }]
            );
            if (path) {
              // Auto-append .bin if user didn't type an extension
              if (!path.includes(".")) {
                path += ".bin";
              }
              await setSetting("defaultDirectory", path.substring(0, path.lastIndexOf("\\") || path.lastIndexOf("/")));
              await saveBufferToFile(path);
              savedPath = path;
            }
          }}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M8 7H5a2 2 0 00-2 2v9a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-3m-1 4l-3 3m0 0l-3-3m3 3V4" />
          </svg>
          Save
        </button>
        {#if savedPath}
          <button
            class="text-xs opacity-50 hover:opacity-100 transition-opacity px-2 py-0.5 rounded border border-transparent hover:border-surface-200-800 flex items-center gap-1"
            onclick={() => savedPath && openFolder(savedPath)}
            title="Open folder in Explorer"
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M5 19a2 2 0 01-2-2V7a2 2 0 012-2h4l2 2h4a2 2 0 012 2v1M5 19h14a2 2 0 002-2v-5a2 2 0 00-2-2H9a2 2 0 00-2 2v5a2 2 0 01-2 2z" />
            </svg>
            Open
          </button>
        {/if}
        <button
          class="text-xs opacity-50 hover:opacity-100 transition-opacity px-2 py-0.5 rounded border border-transparent hover:border-surface-200-800"
          onclick={() => { savedPath = null; clearHexBuffer(); }}
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
