<script lang="ts">
  import { selectedDevice, programmer } from "../stores/device";

  // ── Props ────────────────────────────────────────────────────────────────
  // badPins: device pin numbers (1-based) that failed contact test.
  // pinTestActive: true only when a pin test has been run and returned
  //   results. When false, slots render in their default color regardless
  //   of badPins content.
  let { badPins = [], pinTestActive = false, previewPinCount = null, identifyMode = false }: { badPins?: number[]; pinTestActive?: boolean; previewPinCount?: number | null; identifyMode?: boolean } = $props();

  // ── Socket geometry ──────────────────────────────────────────────────────
  // SVG coordinate system: width=200, height scales with pin count.
  // Pin 1 is always at the TOP of the diagram (ZIF pin 1 = top).
  // Lever is drawn at top (TL866A/CS/II+) or bottom (T48/T56/T76).

  const PIN_PITCH = 12;        // vertical distance between pin slots
  const SOCKET_W = 130;        // socket body width
  const SVG_PADDING = 30;      // extra space outside socket for labels
  const SVG_W = SOCKET_W + 2 * SVG_PADDING;
  const SIDE_MARGIN = 8;       // space between socket edge and pin slot
  const SLOT_W = 42;           // pin slot width (wide socket holes)
  const SLOT_H = 6;            // pin slot height
  const MARGIN_TOP = 48;       // space above first pin (for lever + labels)
  const MARGIN_BOTTOM = 48;    // space below last pin (for lever + labels)
  const KNOB_SCALE = 0.09;     // scale for lever knob path
  const KNOB_W = 129.8 * KNOB_SCALE;
  const KNOB_H = 220.3 * KNOB_SCALE;

  // Models with lever at top (pin 1 end). All others have lever at bottom.
  const LEVER_TOP_MODELS = new Set(["TL866A", "TL866CS", "TL866II+"]);

  // ── Derived state ────────────────────────────────────────────────────────
  let socketSize = $derived(
    $programmer && LEVER_TOP_MODELS.has($programmer.model) ? 40 : 48
  );
  let leverAtTop = $derived(
    $programmer ? LEVER_TOP_MODELS.has($programmer.model) : true
  );

  // Compute occupied ZIF pin numbers from pin_count.
  // DIP chips are placed at the top of the ZIF socket with pin 1 at the
  // top-left. Left side: ZIF pins 1 to N/2. Right side: ZIF pins
  // (socketSize - N/2 + 1) to socketSize.
  // On a 48-pin socket, pins 1-24 are left, 25-48 are right.
  // A DIP-8 chip uses ZIF pins 1-4 (left) and 45-48 (right).
  let occupiedPins = $derived.by(() => {
    const dev = $selectedDevice;
    // Use preview pin count when no device is selected (identify mode)
    const pc = dev?.pin_count ?? previewPinCount;
    if (!pc) return [];
    const half = Math.floor(pc / 2);
    const pins: number[] = [];
    for (let i = 1; i <= half; i++) pins.push(i);
    for (let i = 0; i < half; i++) pins.push(socketSize - half + 1 + i);
    return pins.sort((a, b) => a - b);
  });

  // ── Geometry helpers ─────────────────────────────────────────────────────
  let svgHeight = $derived(MARGIN_TOP + MARGIN_BOTTOM + socketSize / 2 * PIN_PITCH);

  // Map ZIF pin number to (x, y) coordinate.
  // Pins 1..N/2 are on the LEFT side (top to bottom).
  // Pins N/2+1..N are on the RIGHT side (bottom to top).
  function pinToCoord(pin: number, totalPins: number): { x: number; y: number } {
    const half = totalPins / 2;
    const slotLeftX = SIDE_MARGIN;
    const slotRightX = SOCKET_W - SIDE_MARGIN - SLOT_W;
    if (pin <= half) {
      // Left side: pin 1 at top, pin N/2 at bottom
      const y = MARGIN_TOP + (pin - 1) * PIN_PITCH;
      return { x: slotLeftX, y };
    } else {
      // Right side: pin N/2+1 at bottom, pin N at top
      const y = MARGIN_TOP + (totalPins - pin) * PIN_PITCH;
      return { x: slotRightX, y };
    }
  }

  // Compute chip overlay rectangle from occupied pins
  let chipRect = $derived.by(() => {
    if (occupiedPins.length === 0) return null;
    const half = socketSize / 2;
    const leftPins = occupiedPins.filter((p) => p <= half);
    const rightPins = occupiedPins.filter((p) => p > half);
    if (leftPins.length === 0 && rightPins.length === 0) return null;

    const allCoords = occupiedPins.map((p) => pinToCoord(p, socketSize));
    const minY = Math.min(...allCoords.map((c) => c.y));
    const maxY = Math.max(...allCoords.map((c) => c.y)) + SLOT_H;
    // Chip body: fixed width, centered in socket (independent of socket width)
    const chipW = 36;
    const chipX = SOCKET_W / 2 - chipW / 2;
    return { x: chipX, y: minY - 3, w: chipW, h: maxY - minY + 6 };
  });

  // ── Render data ──────────────────────────────────────────────────────────
  let allPins = $derived(
    Array.from({ length: socketSize }, (_, i) => i + 1)
  );

  // Detect package type from device name suffix (e.g., "@DIP28", "@SOP8", "@TSOP48")
  // The package_type field from backend is unreliable (always says DIP for non-PLCC).
  let packageName = $derived(
    $selectedDevice ? ($selectedDevice.name.split("@")[1] || $selectedDevice.package_type) : ""
  );
  let isDip = $derived(packageName.toUpperCase().startsWith("DIP"));

  // ── Pin test state ──────────────────────────────────────────────────────
  // Map device pin numbers to ZIF socket pin numbers for highlighting.
  // Device pins 1..HALF map to ZIF pins 1..HALF (left side).
  // Device pins HALF+1..N map to ZIF pins (socketSize - HALF + 1)..socketSize (right side).
  let badZifPins = $derived.by(() => {
    if (!badPins || badPins.length === 0 || !$selectedDevice) return new Set<number>();
    const pc = $selectedDevice.pin_count;
    const half = Math.floor(pc / 2);
    const set = new Set<number>();
    for (const dPin of badPins) {
      if (dPin <= half) {
        set.add(dPin);
      } else {
        set.add(socketSize - half + (dPin - half));
      }
    }
    return set;
  });

  // Whether pin test results are active (passed in as a prop from App.svelte)
</script>

<div class="border border-surface-200-800 p-2 flex flex-col items-center">
  <h3 class="text-sm font-semibold mb-1 self-start">ZIF Socket Placement</h3>
  {#if !$selectedDevice && !previewPinCount && !identifyMode}
    <p class="text-sm opacity-50 py-4">Select a device to see placement.</p>
  {:else if $selectedDevice && !isDip}
    <p class="text-xs opacity-60 py-3 text-center">
      {packageName} — adapter required.<br>
      Diagram available for DIP packages only.
    </p>
  {:else}
    <svg
      viewBox="0 0 {SVG_W} {svgHeight}"
      class="w-full max-w-[280px]"
      style="height: {svgHeight}px;"
    >
      <g transform="translate({SVG_PADDING}, 0)">
      <!-- Socket body -->
      <rect
        x="4"
        y="{MARGIN_TOP - 10}"
        width="{SOCKET_W - 8}"
        height="{svgHeight - MARGIN_TOP - MARGIN_BOTTOM + 20}"
        rx="5"
        fill="var(--bg-color, #f5f5f5)"
        stroke="currentColor"
        stroke-width="2"
        opacity="0.7"
      />

      <!-- Vertical channel down the middle of the socket -->
      <rect
        x="{SOCKET_W / 2 - 6}"
        y="{MARGIN_TOP - 10}"
        width="12"
        height="{svgHeight - MARGIN_TOP - MARGIN_BOTTOM + 20}"
        fill="currentColor"
        opacity="0.08"
      />

      <!-- Lever handle: vertical bar with shaped knob -->
      {#if leverAtTop}
        <!-- Lever at top: knob up, near left edge. Bar ends at socket top edge. -->
        {@const knobX = 12 - KNOB_W / 2}
        <rect x="10" y="{KNOB_H - 2}" width="4" height="{MARGIN_TOP - 10 - (KNOB_H - 2)}" rx="1"
          fill="currentColor" opacity="1" />
        <path
          d="M60.809,113.373C61.187,76.518 90.148,46.848 125.738,46.858C161.315,46.869 190.252,76.535 190.629,113.373L190.633,113.373L190.629,113.397C190.629,113.408 190.629,113.418 190.629,113.429L190.624,113.429L168.536,267.136L82.902,267.136L60.809,113.391L60.806,113.373L60.809,113.373Z"
          fill="currentColor" opacity="1"
          transform="translate({knobX}, 0) scale({KNOB_SCALE}) translate(-60.806, -46.848)"
        />
      {:else}
        <!-- Lever at bottom: knob down, near right edge. Bar starts at socket bottom edge. -->
        {@const knobX = SOCKET_W - 12 - KNOB_W / 2}
        <rect x="{SOCKET_W - 14}" y="{svgHeight - MARGIN_BOTTOM + 10}" width="4" height="{svgHeight - (KNOB_H - 2) - (svgHeight - MARGIN_BOTTOM + 10)}" rx="1"
          fill="currentColor" opacity="1" />
        <path
          d="M60.809,113.373C61.187,76.518 90.148,46.848 125.738,46.858C161.315,46.869 190.252,76.535 190.629,113.373L190.633,113.373L190.629,113.397C190.629,113.408 190.629,113.418 190.629,113.429L190.624,113.429L168.536,267.136L82.902,267.136L60.809,113.391L60.806,113.373L60.809,113.373Z"
          fill="currentColor" opacity="1"
          transform="translate({knobX}, {svgHeight - KNOB_H}) scale({KNOB_SCALE}) translate(-60.806, -46.848) rotate(180, 125.7, 156.9)"
        />
      {/if}

      <!-- Pin slots (socket holes) -->
      {#each allPins as pin}
        {@const coord = pinToCoord(pin, socketSize)}
        {@const isBad = badZifPins.has(pin)}
        <rect
          x={coord.x}
          y={coord.y}
          width={SLOT_W}
          height={SLOT_H}
          rx="1"
          fill={isBad ? "#f38ba8" : "black"}
          fill-opacity={isBad ? "0.9" : "0.15"}
          stroke={isBad ? "#f38ba8" : "currentColor"}
          stroke-width="1.5"
          opacity={isBad ? "1" : "0.5"}
        />
        {#if isBad}
          {@const isLeft = pin <= socketSize / 2}
          {@const labelX = isLeft ? coord.x - 4 : coord.x + SLOT_W + 4}
          {@const labelAnchor = isLeft ? "end" : "start"}
          {@const half = Math.floor(($selectedDevice?.pin_count ?? 0) / 2)}
          {@const dPin = pin <= half ? pin : pin - (socketSize - half) + half}
          <text
            x={labelX}
            y={coord.y + SLOT_H + 1}
            font-size="9"
            fill="#f38ba8"
            font-weight="bold"
            text-anchor={labelAnchor}
          >PIN {dPin}</text>
        {/if}
      {/each}

      <!-- Chip pins (fixed-width stubs extending from chip body) -->
      {#if chipRect}
        {@const CHIP_PIN_W = 8}
        {#each occupiedPins as pin}
          {@const coord = pinToCoord(pin, socketSize)}
          {@const isLeft = pin <= socketSize / 2}
          {@const stubX = isLeft ? chipRect.x - CHIP_PIN_W : chipRect.x + chipRect.w}
          <rect
            x={stubX}
            y={coord.y}
            width={CHIP_PIN_W}
            height={SLOT_H}
            rx="0.5"
            fill="rgb(99, 102, 241)"
            opacity="0.8"
          />
        {/each}
      {/if}

      <!-- Chip overlay -->
      {#if chipRect}
        {@const centerX = chipRect.x + chipRect.w / 2}
        {@const centerY = chipRect.y + chipRect.h / 2}
        <rect
          x={chipRect.x}
          y={chipRect.y}
          width={chipRect.w}
          height={chipRect.h}
          rx="2"
          fill="rgb(99, 102, 241)"
          fill-opacity="1"
          stroke="rgb(99, 102, 241)"
          stroke-width="2.5"
        />

        <!-- Pin 1 notch: semicircle cut into top edge at horizontal center -->
        <path
          d="M {centerX - 6} {chipRect.y} A 6 6 0 0 0 {centerX + 6} {chipRect.y}"
          fill="var(--bg-color, #f5f5f5)"
          stroke="rgb(99, 102, 241)"
          stroke-width="1.2"
        />

        <!-- Pin 1 dot: at top-left of chip, near pin 1 (inverted for visibility on opaque body) -->
        <circle cx={chipRect.x + 7} cy={chipRect.y + 10} r="4"
          fill="var(--bg-color, #fff)"
          stroke="rgb(99, 102, 241)" stroke-width="1.5" />

        <!-- "ZIF PIN 1" label: hidden if pin 1 is bad (red PIN 1 label takes its place) -->
        {#if !badZifPins.has(1)}
          {@const pin1Coord = pinToCoord(1, socketSize)}
          <text
            x={pin1Coord.x - 8}
            y={pin1Coord.y + SLOT_H + 1}
            font-size="10"
            fill="rgb(99, 102, 241)"
            font-weight="bold"
            text-anchor="end"
          >ZIF PIN 1</text>
        {/if}
      {/if}
      </g>
    </svg>
  {/if}
</div>
