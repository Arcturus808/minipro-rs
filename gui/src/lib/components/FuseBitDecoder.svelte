<script lang="ts">
  import type { FuseByteDef } from "../stores/device";

  // ── Props ────────────────────────────────────────────────────────────────
  interface Props {
    /** Fuse byte definition (bit fields) from the backend. */
    byteDef: FuseByteDef;
    /** Current raw byte value (0–0xFF). */
    value: number;
    /** Display name for this fuse byte (e.g., "Low Fuse"). */
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

  const DANGEROUS_BITS = new Set([
    "RSTDISBL",
    "DWEN",
    "SPIEN",
    "OCDEN",
    "JTAGEN",
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
      // For AVR: programmed (bit=0) = active, show as checked
      const programmed = !isSet;
      if (programmed && danger) return "bit-danger-programmed";
      if (programmed) return "bit-programmed";
      return "bit-unprogrammed";
    } else {
      if (isSet && danger) return "bit-danger-programmed";
      if (isSet) return "bit-programmed";
      return "bit-unprogrammed";
    }
  }

  // ── Raw hex input ────────────────────────────────────────────────────────

  let hexInput = $derived(value.toString(16).padStart(2, "0").toUpperCase());

  function onHexChange(e: Event) {
    const target = e.currentTarget as HTMLInputElement;
    const v = parseInt(target.value, 16);
    if (!isNaN(v) && v >= 0 && v <= 0xFF) {
      onchange(v);
    }
  }
</script>

<div class="bg-surface-100-900 rounded-lg p-3 space-y-2">
  <div class="flex items-center justify-between">
    <span class="text-xs font-semibold opacity-70 uppercase tracking-wider">{displayName}</span>
    <div class="flex items-center gap-2">
      <span class="text-xs font-mono opacity-50">0x</span>
      <input
        type="text"
        class="input text-xs font-mono w-12 px-1 py-0.5"
        value={hexInput}
        onchange={onHexChange}
        maxlength="2"
        title="Raw hex value — editing this updates all bits below"
      />
    </div>
  </div>

  <!-- Bit-level grid: bit 7 (MSB) on left, bit 0 (LSB) on right -->
  <div class="grid grid-cols-8 gap-1">
    {#each Array.from({ length: 8 }, (_, i) => 7 - i) as bitNum}
      {@const field = byteDef.fields.find((f) => f.bit === bitNum)}
      <div class="flex flex-col items-center gap-0.5">
        <span class="text-[9px] font-mono opacity-40">{bitNum}</span>
        {#if field}
          <button
            class="bit-cell {bitClass(bitNum, field.name)}"
            onclick={() => toggleBit(bitNum)}
            title="{field.name}: {field.description} ({bitStateLabel(bitNum)})"
            aria-label="{field.name} bit {bitNum}"
          >
            {isBitSet(bitNum) ? "1" : "0"}
          </button>
          <span class="text-[8px] font-mono opacity-60 text-center leading-tight truncate w-full" title={field.name}>
            {field.name}
          </span>
        {:else}
          <!-- Reserved/unused bit -->
          <div class="bit-cell bit-reserved" title="Reserved / unused">
            {isBitSet(bitNum) ? "1" : "0"}
          </div>
          <span class="text-[8px] font-mono opacity-25 text-center">—</span>
        {/if}
      </div>
    {/each}
  </div>

  <!-- Field descriptions list -->
  <div class="space-y-0.5 mt-1">
    {#each byteDef.fields as field}
      <div class="flex items-start gap-2 text-[10px]">
        <span class="font-mono font-semibold opacity-70 w-20 shrink-0">{field.name}</span>
        <span class="opacity-60 flex-1">
          {field.description}
          {#if isDangerousField(field.name)}
            <span class="text-red-500 font-semibold" title="Dangerous — may disable programming access">⚠</span>
          {/if}
        </span>
      </div>
    {/each}
  </div>
</div>

<style>
  .bit-cell {
    width: 28px;
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 4px;
    font-family: monospace;
    font-size: 11px;
    font-weight: 600;
    cursor: pointer;
    border: 1px solid;
    transition: all 0.15s;
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
