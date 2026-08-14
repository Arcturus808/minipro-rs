<script lang="ts">
  import { programmer } from "../stores/device";

  // ── Connector layout definitions ──────────────────────────────────────────
  // Each layout describes the physical ICSP header for a programmer model.
  // Pin numbering only — no signal labels (see note below diagram).

  interface LinearLayout {
    kind: "linear";
    pins: number;
  }

  interface ZigzagLayout {
    kind: "zigzag";
    rows: number;
    cols: number;
  }

  type Layout = LinearLayout | ZigzagLayout;

  const LAYOUTS: Record<string, Layout> = {
    TL866A: { kind: "linear", pins: 6 },
    "TL866II+": { kind: "linear", pins: 6 },
    T56: { kind: "linear", pins: 8 },
    T48: { kind: "zigzag", rows: 2, cols: 8 },
    T76: { kind: "zigzag", rows: 2, cols: 14 },
  };

  // ── SVG geometry ──────────────────────────────────────────────────────────
  const PIN_SIZE = 24;       // pin pad size (square)
  const PIN_GAP = 5;         // gap between pins
  const ROW_GAP = 10;        // gap between rows (zigzag)
  const PAD = 18;            // padding around connector body
  const LABEL_FONT = 11;     // pin number label font size

  let layout = $derived(
    $programmer ? LAYOUTS[$programmer.model] ?? null : null
  );

  let isTL866CS = $derived($programmer?.model === "TL866CS");

  // ── Linear layout geometry (1×N header) ───────────────────────────────────
  let linearWidth = $derived(
    layout && layout.kind === "linear"
      ? PAD * 2 + layout.pins * (PIN_SIZE + PIN_GAP) - PIN_GAP
      : 0
  );
  let linearHeight = $derived(PAD * 2 + PIN_SIZE + LABEL_FONT + 4);

  // ── Zigzag layout geometry (2×N IDC header) ───────────────────────────────
  let zigzagWidth = $derived(
    layout && layout.kind === "zigzag"
      ? PAD * 2 + layout.cols * (PIN_SIZE + PIN_GAP) - PIN_GAP
      : 0
  );
  let zigzagHeight = $derived(
    PAD * 2 + 2 * PIN_SIZE + ROW_GAP + LABEL_FONT + 4
  );

  // SVG dimensions
  let svgW = $derived(
    layout?.kind === "linear" ? linearWidth : layout?.kind === "zigzag" ? zigzagWidth : 0
  );
  let svgH = $derived(
    layout?.kind === "linear" ? linearHeight : layout?.kind === "zigzag" ? zigzagHeight : 0
  );

  // Pin number label color
  const LABEL_FILL = "rgb(99, 102, 241)";
  const BODY_FILL = "var(--bg-color, #f5f5f5)";
</script>

<div class="border border-surface-200-800 p-2 flex flex-col items-center">
  <h3 class="text-sm font-semibold mb-1 self-start">ICSP Connector</h3>
  {#if !$programmer}
    <p class="text-sm opacity-50 py-4">Connect a programmer to see ICSP pinout.</p>
  {:else if isTL866CS}
    <p class="text-xs opacity-60 py-3 text-center">
      ICSP not supported on TL866CS.
    </p>
  {:else if !layout}
    <p class="text-xs opacity-60 py-3 text-center">
      ICSP pinout not available for {$programmer.model}.
    </p>
  {:else if layout.kind === "linear"}
    {@const pins = layout.pins}
    <svg
      viewBox="0 0 {svgW} {svgH}"
      class="w-full max-w-[280px]"
      style="height: {svgH}px;"
    >
      <!-- Connector body -->
      <rect
        x="{PAD - 4}"
        y="{PAD - 4}"
        width="{linearWidth - 2 * (PAD - 4)}"
        height="{PIN_SIZE + 8}"
        rx="3"
        fill={BODY_FILL}
        stroke="currentColor"
        stroke-width="1.5"
        opacity="0.7"
      />
      <!-- Pin 1 indicator (notch/dot above pin 1) -->
      <circle cx={PAD + PIN_SIZE / 2} cy={PAD - 8} r="2.5"
        fill={LABEL_FILL} />
      <!-- Pins -->
      {#each Array.from({ length: pins }, (_, i) => i) as i}
        {@const x = PAD + i * (PIN_SIZE + PIN_GAP)}
        {@const y = PAD}
        <rect
          x={x}
          y={y}
          width={PIN_SIZE}
          height={PIN_SIZE}
          rx="1"
          fill="black"
          fill-opacity="0.12"
          stroke="currentColor"
          stroke-width="1"
          opacity="0.6"
        />
        <text
          x={x + PIN_SIZE / 2}
          y={y + PIN_SIZE / 2 + LABEL_FONT / 2 - 1}
          font-size={LABEL_FONT}
          fill={LABEL_FILL}
          font-weight="bold"
          text-anchor="middle"
        >{i + 1}</text>
      {/each}
    </svg>
  {:else if layout.kind === "zigzag"}
    {@const cols = layout.cols}
    <svg
      viewBox="0 0 {svgW} {svgH}"
      class="w-full max-w-[280px]"
      style="height: {svgH}px;"
    >
      <!-- Connector body -->
      <rect
        x="{PAD - 4}"
        y="{PAD - 4}"
        width="{zigzagWidth - 2 * (PAD - 4)}"
        height="{2 * PIN_SIZE + ROW_GAP + 8}"
        rx="3"
        fill={BODY_FILL}
        stroke="currentColor"
        stroke-width="1.5"
        opacity="0.7"
      />
      <!-- Pin 1 indicator (dot above pin 1, bottom-left) -->
      <circle cx={PAD + PIN_SIZE / 2} cy={PAD - 8} r="2.5"
        fill={LABEL_FILL} />
      <!-- Pins: odd pins bottom row, even pins top row -->
      {#each Array.from({ length: cols * 2 }, (_, i) => i) as i}
        {@const col = Math.floor(i / 2)}
        {@const isOdd = i % 2 === 0}  // i=0 → pin 1 (odd, bottom), i=1 → pin 2 (even, top)
        {@const pinNum = i + 1}
        {@const x = PAD + col * (PIN_SIZE + PIN_GAP)}
        {@const y = isOdd ? PAD + PIN_SIZE + ROW_GAP : PAD}
        <rect
          x={x}
          y={y}
          width={PIN_SIZE}
          height={PIN_SIZE}
          rx="1"
          fill="black"
          fill-opacity="0.12"
          stroke="currentColor"
          stroke-width="1"
          opacity="0.6"
        />
        <text
          x={x + PIN_SIZE / 2}
          y={y + PIN_SIZE / 2 + LABEL_FONT / 2 - 1}
          font-size={LABEL_FONT}
          fill={LABEL_FILL}
          font-weight="bold"
          text-anchor="middle"
        >{pinNum}</text>
      {/each}
    </svg>
  {/if}
  {#if $programmer && layout}
    <p class="text-xs opacity-50 mt-1 text-center leading-tight">
      ICSP mode active. Pin numbering shown for reference.<br>
      For chip-specific signal assignment (VCC, GND, MISO, MOSI, SCK, RST),<br>
      use Xgpro's [View ICSP Connection] button.
    </p>
  {/if}
</div>
