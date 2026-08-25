<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import type { LogicTestResult } from "../stores/operations";
  import { settings, setSetting } from "../stores/settings";
  import { logs } from "../stores/logs";

  // ── Props ────────────────────────────────────────────────────────────────
  let { result }: { result: LogicTestResult } = $props();

  // ── Constants ─────────────────────────────────────────────────────────────
  // Logic state codes (match the C minipro / database encoding)
  const LOGIC_0 = 0; // Input Low
  const LOGIC_1 = 1; // Input High
  const LOGIC_L = 2; // Output Low
  const LOGIC_H = 3; // Output High
  const LOGIC_C = 4; // Pulse / Clock
  const LOGIC_Z = 5; // High-Z
  const LOGIC_X = 6; // Ignore
  const LOGIC_G = 7; // GND
  const LOGIC_V = 8; // VCC

  const SYMBOLS = "01LHCZXGV";
  const STATE_NAMES = [
    "Input Low", "Input High", "Output Low", "Output High",
    "Pulse/Clock", "High-Z", "Ignore", "GND", "VCC",
  ];

  // ── Zoom (Ctrl+Scroll) ────────────────────────────────────────────────────
  // Cell size in px, persisted across sessions. Range: 20–44.
  const ZOOM_MIN = 20;
  const ZOOM_MAX = 44;
  let cellSize = $state($settings.logicTestZoom);

  $effect(() => {
    cellSize = $settings.logicTestZoom;
  });

  function setZoom(size: number) {
    cellSize = size;
    setSetting("logicTestZoom", size);
  }

  // Ctrl+mousewheel to adjust cell size (non-passive listener so preventDefault works)
  let scrollContainer: HTMLDivElement | null = null;

  function handleWheel(e: WheelEvent) {
    if (!e.ctrlKey) return;
    e.preventDefault();
    const delta = e.deltaY < 0 ? 2 : -2;
    const next = Math.max(ZOOM_MIN, Math.min(ZOOM_MAX, cellSize + delta));
    if (next !== cellSize) setZoom(next);
  }

  $effect(() => {
    const el = scrollContainer;
    if (!el) return;
    el.addEventListener("wheel", handleWheel, { passive: false });
    return () => el.removeEventListener("wheel", handleWheel);
  });

  // ── Derived ───────────────────────────────────────────────────────────────
  let pinCount = $derived(result.pinCount);
  let vectorCount = $derived(result.vectorCount);
  let hasTwoPass = $derived(result.step2.length > 0);

  // Cell dimensions scale with zoom
  let cellHeight = $derived(Math.round(cellSize * 0.85));
  let cellFontSize = $derived(Math.max(9, Math.round(cellSize * 0.4)));
  let headerFontSize = $derived(Math.max(8, Math.round(cellSize * 0.32)));
  let vecColWidth = $derived(Math.max(36, Math.round(cellSize * 1.4)));

  // Grid template for pin columns
  let gridTemplate = $derived(`${vecColWidth}px repeat(${pinCount}, ${cellSize}px)`);

  // ── Cell classification ───────────────────────────────────────────────────
  // Returns: { symbol, category, error, tooltip }
  // category: "input" | "output-pass" | "output-fail" | "ignore"
  function cellInfo(vecIdx: number, pinIdx: number) {
    const idx = vecIdx * pinCount + pinIdx;
    const state = result.vectors[idx];
    const s1 = result.step1[idx];
    const s2 = hasTwoPass ? result.step2[idx] : 0;
    const symbol = SYMBOLS[state] ?? "?";

    let error = false;
    if (hasTwoPass) {
      switch (state) {
        case LOGIC_L: error = s1 !== 0 || s2 !== 0; break;
        case LOGIC_H: error = s1 === 0 || s2 === 0; break;
        case LOGIC_Z: error = s1 === 0 || s2 !== 0; break;
      }
    } else {
      switch (state) {
        case LOGIC_L: error = s1 !== 0; break;
        case LOGIC_H:
        case LOGIC_Z: error = s1 === 0; break;
      }
    }

    let category: string;
    switch (state) {
      case LOGIC_0:
      case LOGIC_1:
      case LOGIC_C:
        category = "input";
        break;
      case LOGIC_L:
      case LOGIC_H:
      case LOGIC_Z:
        category = error ? "output-fail" : "output-pass";
        break;
      default:
        category = "ignore";
    }

    const measured = hasTwoPass
      ? `measured: pull-up=${s1}, pull-down=${s2}`
      : `measured: ${s1}`;
    const tooltip = `Pin ${pinIdx + 1} — ${STATE_NAMES[state]}${error ? " (MISMATCH)" : ""}\n${measured}`;

    return { symbol, category, error, tooltip };
  }

  // ── Copy results to clipboard as TSV ──────────────────────────────────────
  async function copyResults() {
    const lines: string[] = [];

    // Header row: Vec + pin numbers
    const header = ["Vec"];
    for (let p = 0; p < pinCount; p++) header.push(`Pin${p + 1}`);
    lines.push(header.join("\t"));

    // One row per vector
    for (let v = 0; v < vectorCount; v++) {
      const row = [v.toString().padStart(4, "0")];
      for (let p = 0; p < pinCount; p++) {
        const cell = cellInfo(v, p);
        // Append "-" suffix for mismatched output cells
        row.push(cell.error ? cell.symbol + "-" : cell.symbol);
      }
      lines.push(row.join("\t"));
    }

    const tsv = lines.join("\n");
    try {
      await invoke("plugin:clipboard-manager|write_text", { text: tsv });
      logs.info(`Copied ${vectorCount} vectors to clipboard (TSV format)`);
    } catch {
      logs.warn("Failed to copy logic test results to clipboard");
    }
  }
</script>

<div class="logic-test-grid flex flex-col h-full overflow-hidden">
  <!-- Summary bar -->
  <div class="flex items-center gap-3 px-4 py-2 border-b border-surface-200-800 shrink-0">
    {#if result.pass}
      <span class="badge-base badge-pass">
        <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5">
          <path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7" />
        </svg>
        PASS
      </span>
    {:else}
      <span class="badge-base badge-fail">
        <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5">
          <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
        </svg>
        FAIL
      </span>
    {/if}
    <span class="text-sm opacity-70">
      {vectorCount} vectors &times; {pinCount} pins
    </span>
    {#if result.errors > 0}
      <span class="text-sm text-red-500 font-medium">{result.errors} error{result.errors === 1 ? "" : "s"}</span>
    {/if}
    <div class="flex-1"></div>
    <button
      class="text-xs px-2 py-1 border border-surface-200-800 hover:bg-surface-200-800 opacity-70 hover:opacity-100 transition-opacity rounded flex items-center gap-1"
      onclick={copyResults}
      title="Copy results to clipboard (TSV — paste into spreadsheet or text)"
    >
      <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
        <path stroke-linecap="round" stroke-linejoin="round" d="M15.75 17.25v3.375c0 .621-.504 1.125-1.125 1.125h-9.75a1.125 1.125 0 01-1.125-1.125V7.875c0-.621.504-1.125 1.125-1.125H6.75a9 9 0 011.5.124m7.5 10.5V9.375c0-.621-.504-1.125-1.125-1.125H9.75M15.75 17.25h.008v.008h-.008v-.008z" />
      </svg>
      Copy
    </button>
    <!-- Legend -->
    <div class="flex items-center gap-3 text-xs opacity-60">
      <span class="flex items-center gap-1"><span class="legend-cell cell-input">0/1</span> Input</span>
      <span class="flex items-center gap-1"><span class="legend-cell cell-output-pass">L/H</span> Output OK</span>
      <span class="flex items-center gap-1"><span class="legend-cell cell-output-fail">X-</span> Mismatch</span>
      <span class="flex items-center gap-1"><span class="legend-cell cell-ignore">X</span> Ignore</span>
    </div>
    <!-- Zoom indicator + reset -->
    <div class="flex items-center gap-1 text-xs opacity-50 ml-2">
      <span title="Ctrl+Scroll to zoom">{cellSize}px</span>
      {#if cellSize !== 28}
        <button
          class="opacity-60 hover:opacity-100 transition-opacity underline"
          onclick={() => setZoom(28)}
          title="Reset zoom"
        >
          Reset
        </button>
      {/if}
    </div>
  </div>

  <!-- Grid body — scrollable -->
  <div class="flex-1 overflow-auto" bind:this={scrollContainer}>
    <table
      class="logic-table"
      style:grid-template-columns={gridTemplate}
      style="--cell-h: {cellHeight}px; --cell-fs: {cellFontSize}px; --hdr-fs: {headerFontSize}px;"
    >
      <thead>
        <tr>
          <th class="vec-header">Vec</th>
          {#each Array(pinCount) as _, i}
            <th class="pin-header" title={`Pin ${i + 1}`}>{i + 1}</th>
          {/each}
        </tr>
      </thead>
      <tbody>
        {#each Array(vectorCount) as _, v}
          <tr>
            <td class="vec-label">{v.toString().padStart(4, "0")}</td>
            {#each Array(pinCount) as _, p}
              {@const cell = cellInfo(v, p)}
              <td
                class="logic-cell cell-{cell.category}"
                title={cell.tooltip}
              >
                {cell.symbol}{#if cell.error}<span class="err-dash">-</span>{/if}
              </td>
            {/each}
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
</div>

<style>
  .logic-test-grid {
    font-family: var(--font-family-base);
  }

  .badge-base {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 2px 10px;
    border-radius: 9999px;
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.05em;
  }
  .badge-pass {
    background: rgb(34 197 94 / 0.15);
    color: rgb(34 197 94);
    border: 1px solid rgb(34 197 94 / 0.3);
  }
  .badge-fail {
    background: rgb(239 68 68 / 0.15);
    color: rgb(239 68 68);
    border: 1px solid rgb(239 68 68 / 0.3);
  }

  .legend-cell {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 18px;
    border-radius: 3px;
    font-family: var(--font-family-mono);
    font-size: 9px;
    font-weight: 600;
    border: 1px solid;
  }

  /* Table layout */
  .logic-table {
    display: grid;
    border-collapse: collapse;
    font-family: var(--font-family-mono);
    font-size: 12px;
  }

  /* The table is a CSS grid — each row is a grid row */
  thead, tbody, tr {
    display: contents;
  }

  th, td {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0;
  }

  /* Sticky header */
  thead th {
    position: sticky;
    top: 0;
    z-index: 2;
    background: var(--bg-color, #1e1e2e);
    border-bottom: 1px solid rgb(148 163 184 / 0.3);
  }

  .pin-header {
    font-size: var(--hdr-fs, 10px);
    font-weight: 600;
    opacity: 0.5;
    padding: 4px 0;
    height: var(--cell-h, 24px);
  }

  .vec-header {
    font-size: var(--hdr-fs, 10px);
    font-weight: 600;
    opacity: 0.5;
    padding: 4px 0;
    position: sticky;
    left: 0;
    z-index: 3;
    text-align: center;
  }

  .vec-label {
    font-size: var(--hdr-fs, 10px);
    opacity: 0.4;
    padding: 2px 0;
    position: sticky;
    left: 0;
    z-index: 1;
    background: var(--bg-color, #1e1e2e);
    border-right: 1px solid rgb(148 163 184 / 0.15);
  }

  /* Cells */
  .logic-cell {
    height: var(--cell-h, 24px);
    margin: 1px;
    border-radius: 4px;
    font-size: var(--cell-fs, 11px);
    font-weight: 600;
    border: 1px solid;
    transition: transform 0.1s;
  }
  .logic-cell:hover {
    transform: scale(1.15);
    z-index: 4;
  }

  .cell-input {
    background: rgb(59 130 246 / 0.15);
    border-color: rgb(59 130 246 / 0.3);
    color: rgb(59 130 246);
  }

  .cell-output-pass {
    background: rgb(34 197 94 / 0.15);
    border-color: rgb(34 197 94 / 0.3);
    color: rgb(34 197 94);
  }

  .cell-output-fail {
    background: rgb(239 68 68 / 0.2);
    border-color: rgb(239 68 68 / 0.5);
    color: rgb(239 68 68);
  }

  .cell-ignore {
    background: rgb(100 116 139 / 0.08);
    border-color: rgb(100 116 139 / 0.15);
    color: rgb(100 116 139 / 0.6);
  }

  .err-dash {
    font-weight: 700;
  }

  /* Legend cell colors match the grid cells */
  .legend-cell.cell-input {
    background: rgb(59 130 246 / 0.15);
    border-color: rgb(59 130 246 / 0.3);
    color: rgb(59 130 246);
  }
  .legend-cell.cell-output-pass {
    background: rgb(34 197 94 / 0.15);
    border-color: rgb(34 197 94 / 0.3);
    color: rgb(34 197 94);
  }
  .legend-cell.cell-output-fail {
    background: rgb(239 68 68 / 0.2);
    border-color: rgb(239 68 68 / 0.5);
    color: rgb(239 68 68);
  }
  .legend-cell.cell-ignore {
    background: rgb(100 116 139 / 0.08);
    border-color: rgb(100 116 139 / 0.15);
    color: rgb(100 116 139 / 0.6);
  }

  /* Light mode adjustments */
  :global(html:not(.dark)) .vec-label,
  :global(html:not(.dark)) thead th,
  :global(html:not(.dark)) .vec-header {
    background: #ffffff;
  }
</style>
