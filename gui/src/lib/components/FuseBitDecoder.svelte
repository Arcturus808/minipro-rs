<script lang="ts">
  import type { FuseByteDef } from "../stores/device";

  // ── Props ────────────────────────────────────────────────────────────────
  interface Props {
    /** Fuse byte/word definition (bit fields) from the backend. */
    byteDef: FuseByteDef;
    /** Current raw value (0 to 2^width - 1). */
    value: number;
    /** Display name for this fuse byte (e.g., "Low Fuse", "Config Word 1"). */
    displayName: string;
    /** True for AVR devices (bit=0 means programmed). */
    invertFuseBits: boolean;
    /** Callback when the raw value changes. */
    onchange: (value: number) => void;
  }

  let {
    byteDef,
    value,
    displayName,
    invertFuseBits,
    onchange,
  }: Props = $props();

  // ── Width-derived values ──────────────────────────────────────────────────

  /** Number of bits in this config word (8, 12, 14, or 16). */
  let width = $derived(byteDef.width || 8);

  /** Maximum value for this width. */
  let maxValue = $derived((1 << width) - 1);

  /** Number of hex digits needed to display the value. */
  let hexDigits = $derived(width <= 8 ? 2 : width <= 12 ? 3 : width <= 16 ? 4 : 4);

  /** Sorted fields (MSB first) — $derived tracks byteDef reactivity. */
  let sortedFields = $derived(
    [...byteDef.fields].sort((a, b) => b.bit - a.bit)
  );

  /** Number of grid columns = number of defined fields. */
  let gridCols = $derived(byteDef.fields.length);

  /** Column width in px — smaller for wider words. */
  let colWidth = $derived(gridCols > 8 ? '34px' : '42px');

  /** Full grid-template-columns style string. */
  let gridTemplate = $derived(`repeat(${gridCols}, ${colWidth})`);

  // ── Bit helpers ──────────────────────────────────────────────────────────

  /** Check if a specific bit is set in the current value. */
  function isBitSet(bit: number): boolean {
    return (value & (1 << bit)) !== 0;
  }

  /** Toggle a single bit and emit the new value. */
  function toggleBit(bit: number) {
    const newValue = value ^ (1 << bit);
    onchange(newValue);
  }

  // ── Dangerous bit detection ──────────────────────────────────────────────
  // AVR: SPIEN/RSTDISBL/DWEN programmed (0) can disable programming access.
  // PIC: LVP/WRTC/CP/CPD/DEBUG cleared (0) can lock out programming or
  //   block code readback. All follow the same "0 is the dangerous state"
  //   convention, so the existing bit-danger-programmed CSS class works
  //   for both AVR (programmed=0) and PIC (set=1, danger when 0).

  const DANGEROUS_BITS = new Set([
    // AVR
    "RSTDISBL",
    "DWEN",
    "SPIEN",
    "OCDEN",
    "JTAGEN",
    // PIC
    "LVP",
    "WRTC",
    "CP",
    "CPD",
    "DEBUG",
  ]);

  function isDangerousField(name: string): boolean {
    return DANGEROUS_BITS.has(name.toUpperCase());
  }

  // ── Bit display ──────────────────────────────────────────────────────────

  /** For AVR: "Programmed" when bit=0, "Unprogrammed" when bit=1.
   *  For non-AVR: just "1" or "0". */
  function bitStateLabel(bit: number): string {
    const isSet = isBitSet(bit);
    if (invertFuseBits) {
      return isSet ? "Unprogrammed" : "Programmed";
    }
    return isSet ? "1" : "0";
  }

  /** CSS class for a bit checkbox based on its state. */
  function bitClass(bit: number, fieldName: string): string {
    const isSet = isBitSet(bit);
    const danger = isDangerousField(fieldName);
    if (invertFuseBits) {
      // AVR: programmed (bit=0) = active. Danger when programmed (0).
      const programmed = !isSet;
      if (programmed && danger) return "bit-danger-programmed";
      if (programmed) return "bit-programmed";
      return "bit-unprogrammed";
    } else {
      // PIC: bit=1 is set/active. Danger when cleared (0) for LVP/WRTC/CP/CPD/DEBUG.
      if (!isSet && danger) return "bit-danger-programmed";
      if (isSet) return "bit-programmed";
      return "bit-unprogrammed";
    }
  }

  /** Full tooltip text for a bit cell: name, description, state, danger warning. */
  function bitTooltip(bit: number): string {
    const field = byteDef.fields.find((f) => f.bit === bit);
    if (!field) return "Reserved / unused";
    const danger = isDangerousField(field.name) ? " ⚠ Dangerous — may disable programming access" : "";
    return `${field.name}: ${field.description} (${bitStateLabel(bit)})${danger}`;
  }

  // ── Raw hex input ────────────────────────────────────────────────────────

  let hexInput = $derived(value.toString(16).padStart(hexDigits, "0").toUpperCase());

  function onHexChange(e: Event) {
    const target = e.currentTarget as HTMLInputElement;
    const v = parseInt(target.value, 16);
    if (!isNaN(v) && v >= 0 && v <= maxValue) {
      onchange(v);
    }
  }
</script>

<div class="fuse-card bg-surface-100-900 rounded-lg p-3 space-y-2">
  <div class="flex items-center justify-between gap-2">
    <span class="text-sm font-semibold opacity-70 uppercase tracking-wider whitespace-nowrap">{displayName}</span>
    <div class="flex items-center gap-1 shrink-0">
      <span class="text-sm font-mono opacity-50">0x</span>
      <input
        type="text"
        class="input text-sm font-mono px-1 py-0.5"
        style:width="{hexDigits + 2}ch"
        value={hexInput}
        onchange={onHexChange}
        maxlength={hexDigits}
        title="Raw hex value — editing this updates all bits below"
      />
    </div>
  </div>

  <!-- Bit-level grid: MSB on left, LSB on right — only defined bits, not reserved. -->
  <div class="grid gap-1.5 {gridCols > 8 ? 'bit-grid-wide' : ''}" style:grid-template-columns={gridTemplate} style:justify-content="start">
    {#each sortedFields as field (field.bit)}
      <div class="flex flex-col items-center gap-1">
        <span class="text-xs font-mono opacity-40">{field.bit}</span>
        <button
          class="bit-cell {bitClass(field.bit, field.name)}"
          onclick={() => toggleBit(field.bit)}
          title={bitTooltip(field.bit)}
          aria-label="{field.name} bit {field.bit}"
        >
          {isBitSet(field.bit) ? "1" : "0"}
        </button>
        <span class="bit-name-label" title={bitTooltip(field.bit)}>
          {field.name}{#if isDangerousField(field.name)}<span class="text-red-500"> ⚠</span>{/if}
        </span>
      </div>
    {/each}
  </div>
</div>

<style>
  /* Card border — more visible than border-surface-200-800.
     width: fit-content prevents flexbox from stretching the card beyond its bit grid. */
  .fuse-card {
    border: 1px solid rgb(148 163 184 / 0.4);
    box-shadow: 0 1px 3px rgb(0 0 0 / 0.08);
    width: fit-content;
    max-width: 100%;
  }

  .bit-cell {
    width: 36px;
    height: 36px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 5px;
    font-family: monospace;
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
    border: 1px solid;
    transition: all 0.15s;
  }

  /* For wider words (12-16 bits), use smaller cells so they fit in wrapped columns */
  .bit-grid-wide .bit-cell {
    width: 28px;
    height: 28px;
    font-size: 12px;
  }

  .bit-name-label {
    font-family: monospace;
    font-size: 11px;
    text-align: center;
    line-height: 1.1;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    opacity: 0.7;
  }

  .bit-grid-wide .bit-name-label {
    font-size: 9px;
  }

  .bit-programmed {
    background: rgb(59 130 246 / 0.2);
    border-color: rgb(59 130 246 / 0.4);
    color: rgb(59 130 246);
  }

  .bit-unprogrammed {
    background: rgb(100 116 139 / 0.1);
    border-color: rgb(100 116 139 / 0.2);
    color: rgb(100 116 139);
  }

  .bit-danger-programmed {
    background: rgb(239 68 68 / 0.2);
    border-color: rgb(239 68 68 / 0.5);
    color: rgb(239 68 68);
  }

  .bit-reserved {
    background: rgb(100 116 139 / 0.05);
    border-color: rgb(100 116 139 / 0.1);
    color: rgb(100 116 139 / 0.4);
    cursor: default;
  }

  .bit-cell:hover:not(.bit-reserved) {
    transform: scale(1.1);
  }
</style>
