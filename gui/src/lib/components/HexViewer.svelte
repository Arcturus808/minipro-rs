<script lang="ts">
  import { hexMeta } from "../stores/hex";

  const ROW_SIZE = 16;
  const MAX_ROWS = 16384;

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
</script>

<div style="border: 1px solid #ccc; display: flex; flex-direction: column; height: 100%;">
  <div style="padding: 8px 12px; border-bottom: 1px solid #ccc;">
    <span style="font-size: 14px; font-weight: 600;">Hex Viewer</span>
    {#if $hexMeta}
      <span style="font-size: 12px; opacity: 0.6; margin-left: 8px;">
        {$hexMeta.size.toLocaleString()} bytes
      </span>
    {/if}
  </div>
  <div
    style="flex: 1; overflow: auto; font-family: 'Consolas', 'Courier New', monospace; font-size: 13px; line-height: 22px; padding: 8px;"
  >
    {#if $hexMeta?.data && $hexMeta.data.length > 0}
      {#each Array.from({length: Math.min(MAX_ROWS, Math.ceil($hexMeta.data.length / ROW_SIZE))}, (_, i) => i) as rowIdx}
        {@const offset = rowIdx * ROW_SIZE}
        {@const end = Math.min(offset + ROW_SIZE, $hexMeta.data.length)}
        {@const len = end - offset}
        <div style="display: flex; white-space: nowrap; height: 22px;">
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
    {:else}
      <p style="opacity: 0.4;">No data loaded.</p>
    {/if}
  </div>
</div>
