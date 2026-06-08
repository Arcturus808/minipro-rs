<script lang="ts">
  import { hexBuffer, hexSize } from "../stores/hex";

  const BYTES_PER_ROW = 16;

  function toHex(n: number): string {
    return n.toString(16).padStart(2, "0").toUpperCase();
  }

  function toAscii(b: number): string {
    return b >= 32 && b < 127 ? String.fromCharCode(b) : ".";
  }

  function getRows(buffer: Uint8Array): { address: number; bytes: number[]; ascii: string }[] {
    const rows: { address: number; bytes: number[]; ascii: string }[] = [];
    for (let i = 0; i < buffer.length; i += BYTES_PER_ROW) {
      const slice = buffer.slice(i, i + BYTES_PER_ROW);
      const bytes = Array.from(slice);
      const ascii = bytes.map(toAscii).join("");
      rows.push({ address: i, bytes, ascii });
    }
    return rows;
  }

  let rows = $derived($hexBuffer ? getRows($hexBuffer) : []);
</script>

<div class="card preset-filled-surface-100-900 border border-surface-200-800 flex flex-col h-full">
  <header class="flex items-center justify-between px-3 py-2 border-b border-surface-200-800">
    <h2 class="text-sm font-semibold">Hex Viewer</h2>
    {#if $hexBuffer}
      <span class="text-xs opacity-60">{$hexSize.toLocaleString()} bytes</span>
    {/if}
  </header>

  <div class="flex-1 overflow-auto p-2 hex-grid">
    {#if rows.length === 0}
      <div class="flex items-center justify-center h-full opacity-40 text-sm">
        No data loaded. Read a chip or load a file to view.
      </div>
    {:else}
      <div class="min-w-full">
        <!-- Header -->
        <div class="flex text-xs opacity-50 mb-1 select-none sticky top-0 bg-surface-100-900">
          <span class="w-20 shrink-0">Address</span>
          {#each Array(BYTES_PER_ROW) as _, i}
            <span class="w-5 text-center">{toHex(i)}</span>
          {/each}
          <span class="w-32 ml-2">ASCII</span>
        </div>

        <!-- Rows -->
        {#each rows as row}
          <div class="flex text-xs py-px hover:bg-surface-200-800/30">
            <span class="w-20 shrink-0 opacity-50 select-none">{toHex(row.address).padStart(8, "0")}</span>
            {#each row.bytes as b}
              <span class="w-5 text-center font-mono">{toHex(b)}</span>
            {/each}
            {#if row.bytes.length < BYTES_PER_ROW}
              {#each Array(BYTES_PER_ROW - row.bytes.length) as _}
                <span class="w-5 text-center opacity-20">--</span>
              {/each}
            {/if}
            <span class="w-32 ml-2 opacity-70 select-none">{row.ascii}</span>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>
