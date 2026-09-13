<script lang="ts">
  import { programmer, selectedDevice, getIcspWiring } from "../stores/device";
  import type { IcspWiring } from "../stores/device";

  // ── Connector layout definitions ──────────────────────────────────────────
  // Each layout describes the physical ICSP header for a programmer model.

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

  // Wiring diagram geometry
  const W_ROW_H = 26;        // height per chip-pin row
  const W_PIN_SQ = 15;       // header pin-number square
  const W_PIN_GAP = 3;
  const W_CHIP_X = 205;      // chip body left edge
  const W_CHIP_W = 120;
  const W_STUB = 8;          // chip pin stub length
  const W_GRP_END = 185;     // right edge of header-pin groups
  const W_TOP = 10;

  let layout = $derived(
    $programmer ? LAYOUTS[$programmer.model] ?? null : null
  );

  let isTL866CS = $derived($programmer?.model === "TL866CS");

  // ── Wiring lookup ─────────────────────────────────────────────────────────
  let wiring = $state<IcspWiring | null>(null);
  let wiringLoading = $state(false);
  let wiringKey = "";

  $effect(() => {
    const model = $programmer?.model;
    const cls = $selectedDevice?.icsp ?? 0;
    if (!model || !cls) {
      wiring = null;
      wiringKey = "";
      return;
    }
    const key = `${model}:${cls}`;
    if (key === wiringKey) return;
    wiringKey = key;
    wiringLoading = true;
    getIcspWiring(model, cls)
      .then((w) => {
        if (wiringKey === key) wiring = w;
      })
      .catch(() => {
        if (wiringKey === key) wiring = null;
      })
      .finally(() => {
        wiringLoading = false;
      });
  });

  // One row per chip pin; header pins grouped per target pin.
  interface WireRow {
    chipPin: number;
    label: string;
    headerPins: number[];
    signal: string;
  }

  let wireRows = $derived<WireRow[]>(
    wiring
      ? wiring.chip_labels.map((label, i) => {
          const pin = i + 1;
          const ws = wiring.wires.filter((w) => w.chip_pin === pin);
          return {
            chipPin: pin,
            label,
            headerPins: ws.map((w) => w.header_pin),
            signal: ws[0]?.signal ?? "",
          };
        })
      : []
  );

  let wireSvgH = $derived(wireRows.length * W_ROW_H + W_TOP * 2);
  const WIRE_SVG_W = 345;

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
  const WIRE_STROKE = "rgb(99, 102, 241)";
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

  <!-- ── Per-class wiring diagram ────────────────────────────────────────── -->
  {#if $programmer && layout && $selectedDevice}
    {#if $selectedDevice.icsp === 0}
      <p class="text-xs opacity-50 mt-2 text-center leading-tight">
        {$selectedDevice.name} has no ICSP wiring class — use the ZIF socket.
      </p>
    {:else if wiringLoading}
      <p class="text-xs opacity-50 mt-2">Loading wiring diagram…</p>
    {:else if wiring}
      <p class="text-xs font-semibold mt-2 self-start">
        {wiring.title} — wiring for {$selectedDevice.name}
      </p>
      <svg
        viewBox="0 0 {WIRE_SVG_W} {wireSvgH}"
        class="w-full max-w-[360px]"
        style="height: {wireSvgH}px;"
      >
        <!-- Chip body -->
        <rect
          x={W_CHIP_X}
          y={W_TOP - 4}
          width={W_CHIP_W}
          height={wireRows.length * W_ROW_H + 8}
          rx="3"
          fill={BODY_FILL}
          stroke="currentColor"
          stroke-width="1.5"
          opacity="0.7"
        />
        <circle cx={W_CHIP_X + 8} cy={W_TOP + 4} r="2" fill={LABEL_FILL} />
        {#each wireRows as row, i}
          {@const cy = W_TOP + i * W_ROW_H + W_ROW_H / 2}
          <!-- Chip pin stub -->
          <rect
            x={W_CHIP_X - W_STUB}
            y={cy - 5}
            width={W_STUB}
            height={10}
            fill="black"
            fill-opacity="0.12"
            stroke="currentColor"
            stroke-width="1"
            opacity={row.headerPins.length ? 0.7 : 0.3}
          />
          <!-- Chip pin label: "1 /CS" -->
          <text
            x={W_CHIP_X + 10}
            y={cy + 4}
            font-size="11"
            fill="currentColor"
            opacity={row.headerPins.length ? 0.9 : 0.45}
          ><tspan fill={LABEL_FILL} font-weight="bold">{row.chipPin}</tspan>  {row.label}</text>
          {#if row.headerPins.length}
            <!-- Wire -->
            <line
              x1={W_GRP_END}
              y1={cy}
              x2={W_CHIP_X - W_STUB}
              y2={cy}
              stroke={WIRE_STROKE}
              stroke-width="1.5"
            />
            <!-- Header pin group, right-aligned at W_GRP_END -->
            {@const sigW = row.signal.length * 7 + 6}
            {@const grpW = row.headerPins.length * (W_PIN_SQ + W_PIN_GAP) - W_PIN_GAP + 6 + sigW}
            {@const gx = W_GRP_END - grpW}
            {#each row.headerPins as hp, j}
              {@const px = gx + j * (W_PIN_SQ + W_PIN_GAP)}
              <rect
                x={px}
                y={cy - W_PIN_SQ / 2}
                width={W_PIN_SQ}
                height={W_PIN_SQ}
                rx="1"
                fill="black"
                fill-opacity="0.12"
                stroke="currentColor"
                stroke-width="1"
                opacity="0.7"
              />
              <text
                x={px + W_PIN_SQ / 2}
                y={cy + 3.5}
                font-size="9"
                fill={LABEL_FILL}
                font-weight="bold"
                text-anchor="middle"
              >{hp}</text>
            {/each}
            <text
              x={gx + row.headerPins.length * (W_PIN_SQ + W_PIN_GAP) - W_PIN_GAP + 6}
              y={cy + 4}
              font-size="11"
              fill="currentColor"
              opacity="0.9"
            >{row.signal}</text>
          {/if}
        {/each}
      </svg>
      {#each wiring.notes as note}
        <p class="text-xs opacity-60 mt-1 self-start leading-tight">• {note}</p>
      {/each}
    {:else}
      <p class="text-xs opacity-50 mt-2 text-center leading-tight">
        ICSP class 0x{$selectedDevice.icsp.toString(16).padStart(2, "0")} —
        no verified wiring diagram for {$programmer.model} yet.<br>
        Connector pin numbering shown above for reference.
      </p>
    {/if}
  {:else if $programmer && layout}
    <p class="text-xs opacity-50 mt-1 text-center leading-tight">
      Select a device to see its ICSP wiring.
    </p>
  {/if}
</div>
