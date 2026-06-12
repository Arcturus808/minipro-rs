<script lang="ts">
  import { hexMeta, hexLoading, clearHexBuffer, hexEdits, setHexEdit, clearHexEdits, applyHexEdits } from "../stores/hex";
  import { settings, setSetting } from "../stores/settings";
  import { selectedDevice } from "../stores/device";
  import { saveBufferToFile, openFolder } from "../stores/operations";
  import { pickSaveFile, getIterativeSavePath } from "../file-dialog";
  import { get } from "svelte/store";

  const ROW_SIZE = 16;
  const BUFFER_ROWS = 5;

  let fontSize = $state($settings.hexViewerFontSize);
  let rowHeight = $derived(fontSize + 9);
  let savedPath = $state<string | null>(null);

  // Hex editing state
  let editingOffset = $state<number | null>(null);
  let editValue = $state("");
  let editInputRef = $state<HTMLInputElement | null>(null);
  let editCursorPos = $state(0);
  let editingMode = $state<"hex" | "ascii">("hex");

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

  // Edit count for toolbar
  let editCount = $derived($hexEdits.size);

  $effect(() => {
    if (scrollContainer) {
      containerHeight = scrollContainer.clientHeight;
    }
  });

  // ── Hex editing helpers ────────────────────────────────────────────────────

  function isEdited(offset: number): boolean {
    return $hexEdits.has(offset);
  }

  function getByte(offset: number): number {
    if (!($hexMeta?.data)) return 0;
    if ($hexEdits.has(offset)) return $hexEdits.get(offset)!;
    return $hexMeta.data[offset];
  }

  function focusInput(pos: number) {
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        const input = document.querySelector('.hex-edit-input') as HTMLInputElement | null;
        if (input) {
          input.focus();
          input.setSelectionRange(pos, pos);
        }
      });
    });
  }

  function startEdit(offset: number, mode: "hex" | "ascii" = "hex") {
    if (!($hexMeta?.data) || offset < 0 || offset >= $hexMeta.data.length) return;
    // Commit any pending edit on the current byte before switching
    if (editingOffset !== null && editingOffset !== offset) {
      commitEdit();
    }
    editingOffset = offset;
    editingMode = mode;
    const b = getByte(offset);
    if (mode === "hex") {
      editValue = b.toString(16).padStart(2, "0").toUpperCase();
    } else {
      editValue = toAscii(b);
    }
    editCursorPos = 0;
    focusInput(0);
  }

  function commitEdit() {
    if (editingOffset === null) return;
    let v: number;
    if (editingMode === "ascii") {
      v = editValue.length > 0 ? editValue.charCodeAt(0) : 0x20;
    } else {
      v = parseInt(editValue, 16);
    }
    if (!isNaN(v) && v >= 0 && v <= 0xFF) {
      const meta = $hexMeta;
      if (meta?.data) {
        const original = meta.data[editingOffset];
        if (v !== original) {
          setHexEdit(editingOffset, v);
        } else {
          setHexEdit(editingOffset, null); // reset to original
        }
      }
    }
    editingOffset = null;
  }

  function cancelEdit() {
    editingOffset = null;
  }

  // Global keydown handler for hex editing — attached to document so it survives
  // DOM changes when the input element is destroyed/recreated on overflow.
  function handleEditKeydown(e: KeyboardEvent) {
    if (editingOffset === null || !($hexMeta?.data)) return;
    const dataLen = $hexMeta.data.length;

    if (editingMode === "ascii") {
      // ASCII mode: any printable character sets the byte and overflows
      if (e.key.length === 1 && e.key.charCodeAt(0) >= 0x20 && e.key.charCodeAt(0) <= 0x7E) {
        e.preventDefault();
        const byteVal = e.key.charCodeAt(0);
        const currentOffset = editingOffset;
        // Commit current
        if (currentOffset !== null && currentOffset >= 0 && currentOffset < dataLen) {
          const meta = $hexMeta;
          if (meta?.data) {
            const original = meta.data[currentOffset];
            if (byteVal !== original) {
              setHexEdit(currentOffset, byteVal);
            } else {
              setHexEdit(currentOffset, null);
            }
          }
        }
        // Overflow to next byte
        const nextOffset = currentOffset + 1;
        if (nextOffset < dataLen) {
          const nextByte = getByte(nextOffset);
          editingOffset = nextOffset;
          editValue = toAscii(nextByte);
          editCursorPos = 1;
          focusInput(1);
        } else {
          editingOffset = null;
        }
        return;
      }

      // Backspace in ASCII mode: reset to space (0x20)
      if (e.key === "Backspace") {
        e.preventDefault();
        editValue = " ";
        editCursorPos = 0;
        if (editInputRef) {
          editInputRef.value = " ";
          editInputRef.setSelectionRange(0, 0);
        }
        return;
      }
    } else {
      // Hex char: overwrite nibble at cursor, overflow to next byte
      if (/^[0-9A-Fa-f]$/.test(e.key)) {
        e.preventDefault();
        const pos = editCursorPos;

        if (pos >= 2) {
          // Overflow to next byte — manually commit current to avoid blur re-entry
          const currentOffset = editingOffset;
          const currentValue = parseInt(editValue, 16);
          if (!isNaN(currentValue) && currentValue >= 0 && currentValue <= 0xFF) {
            const meta = $hexMeta;
            if (meta?.data) {
              const original = meta.data[currentOffset];
              if (currentValue !== original) {
                setHexEdit(currentOffset, currentValue);
              } else {
                setHexEdit(currentOffset, null);
              }
            }
          }
          const nextOffset = currentOffset + 1;
          if (nextOffset < dataLen) {
            const nextByte = getByte(nextOffset);
            const nextHex = nextByte.toString(16).padStart(2, "0").toUpperCase();
            editingOffset = nextOffset;
            editValue = e.key.toUpperCase() + nextHex.charAt(1);
            editCursorPos = 1;
            focusInput(1);
          } else {
            editingOffset = null;
          }
          return;
        }

        const chars = editValue.split("");
        chars[pos] = e.key.toUpperCase();
        const newValue = chars.join("").slice(0, 2);
        editValue = newValue;
        editCursorPos = Math.min(pos + 1, 2);
        // Sync the actual input element's value and cursor
        if (editInputRef) {
          editInputRef.value = newValue;
          editInputRef.setSelectionRange(editCursorPos, editCursorPos);
        }
        return;
      }

      // Backspace in hex mode
      if (e.key === "Backspace") {
        e.preventDefault();
        const pos = editCursorPos;
        if (pos > 0) {
          const chars = editValue.split("");
          chars[pos - 1] = "0";
          const newValue = chars.join("");
          editValue = newValue;
          editCursorPos = pos - 1;
          if (editInputRef) {
            editInputRef.value = newValue;
            editInputRef.setSelectionRange(editCursorPos, editCursorPos);
          }
        }
        return;
      }
    }

    switch (e.key) {
      case "Enter":
        e.preventDefault();
        commitEdit();
        break;
      case "Escape":
        e.preventDefault();
        cancelEdit();
        break;
      case "ArrowLeft":
        e.preventDefault();
        if (editingOffset > 0) {
          commitEdit();
          startEdit(editingOffset - 1);
        }
        break;
      case "ArrowRight":
        e.preventDefault();
        if (editingOffset < dataLen - 1) {
          commitEdit();
          startEdit(editingOffset + 1);
        }
        break;
      case "ArrowUp":
        e.preventDefault();
        if (editingOffset >= ROW_SIZE) {
          commitEdit();
          startEdit(editingOffset - ROW_SIZE);
        }
        break;
      case "ArrowDown":
        e.preventDefault();
        if (editingOffset < dataLen - ROW_SIZE) {
          commitEdit();
          startEdit(editingOffset + ROW_SIZE);
        }
        break;
    }
  }

  // Attach/detach global keydown listener when editing state changes
  $effect(() => {
    if (editingOffset !== null) {
      document.addEventListener("keydown", handleEditKeydown);
      return () => document.removeEventListener("keydown", handleEditKeydown);
    }
  });
</script>

<div class="hex-viewer-container" style="border: 1px solid #ccc; display: flex; flex-direction: column; height: 100%;">
  <div style="padding: 8px 12px; border-bottom: 1px solid #ccc; display: flex; align-items: center; justify-content: space-between;">
    <div>
      <span style="font-size: 14px; font-weight: 600;">Hex Viewer</span>
      {#if $hexMeta}
        <span style="font-size: 12px; opacity: 0.6; margin-left: 8px;">
          {$hexMeta.size.toLocaleString()} bytes
          {#if $hexMeta.crc32 !== null}
            · CRC-32: {$hexMeta.crc32.toString(16).padStart(8, '0').toUpperCase()}
          {/if}
        </span>
        {#if editCount > 0}
          <span style="font-size: 12px; color: #f59e0b; margin-left: 8px; font-weight: 500;">
            {editCount} edit{editCount === 1 ? '' : 's'} pending
          </span>
        {/if}
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
        {#if editCount > 0}
          <button
            class="opacity-70 hover:opacity-100 transition-opacity px-3 py-1.5 rounded border border-transparent hover:border-surface-200-800 flex items-center gap-1.5 text-amber-600 font-medium"
            style="font-size: 13px;"
            onclick={() => { applyHexEdits(); savedPath = null; }}
            title="Apply pending edits to the buffer"
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7" />
            </svg>
            Apply
          </button>
          <button
            class="opacity-70 hover:opacity-100 transition-opacity px-3 py-1.5 rounded border border-transparent hover:border-surface-200-800"
            style="font-size: 13px;"
            onclick={() => clearHexEdits()}
            title="Discard all pending edits"
          >
            Reset
          </button>
        {/if}
        <button
          class="opacity-70 hover:opacity-100 transition-opacity px-3 py-1.5 rounded border border-transparent hover:border-surface-200-800 flex items-center gap-1.5" style="font-size: 13px;"
          onclick={async () => {
            const dir = get(settings).defaultDirectory ?? "";
            const dev = get(selectedDevice);
            const devName = dev?.name?.replace(/[\\/:*?"<>|@]/g, "_") ?? "dump";
            const defaultName = `${devName}.bin`;
            const defaultPath = dir ? `${dir}\\${defaultName}` : defaultName;
            const iterativePath = await getIterativeSavePath(defaultPath);
            let path = await pickSaveFile(
              "Save chip dump as",
              iterativePath,
              [{ name: "Binary", extensions: ["bin"] }]
            );
            if (path) {
              if (!path.includes(".")) {
                path += ".bin";
              }
              await setSetting("defaultDirectory", path.substring(0, path.lastIndexOf("\\") || path.lastIndexOf("/")));
              await saveBufferToFile(path);
              savedPath = path;
            }
          }}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M8 7H5a2 2 0 00-2 2v9a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-3m-1 4l-3 3m0 0l-3-3m3 3V4" />
          </svg>
          Save
        </button>
        {#if savedPath}
          <button
            class="opacity-70 hover:opacity-100 transition-opacity px-3 py-1.5 rounded border border-transparent hover:border-surface-200-800 flex items-center gap-1.5" style="font-size: 13px;"
            onclick={() => savedPath && openFolder(savedPath)}
            title="Open folder in Explorer"
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M5 19a2 2 0 01-2-2V7a2 2 0 012-2h4l2 2h4a2 2 0 012 2v1M5 19h14a2 2 0 002-2v-5a2 2 0 00-2-2H9a2 2 0 00-2 2v5a2 2 0 01-2 2z" />
            </svg>
            Open
          </button>
        {/if}
        <button
          class="opacity-70 hover:opacity-100 transition-opacity px-3 py-1.5 rounded border border-transparent hover:border-surface-200-800"
          style="font-size: 13px;"
          onclick={() => { savedPath = null; clearHexEdits(); clearHexBuffer(); }}
        >
          Clear
        </button>
      {/if}
    </div>
  </div>
  <div
    bind:this={scrollContainer}
    onscroll={onScroll}
    style="flex: 1; overflow: auto; font-family: 'Hack', 'Consolas', 'Courier New', monospace; font-size: {fontSize}px; line-height: {rowHeight}px; padding: 8px;"
  >
    {#if $hexLoading}
      <div style="display: flex; align-items: center; justify-content: center; height: 100%; gap: 8px;">
        <div class="spinner"></div>
        <span style="opacity: 0.6;">Loading...</span>
      </div>
    {:else if $hexMeta?.data && $hexMeta.data.length > 0}
      <div style="height: {totalHeight + rowHeight}px; position: relative;">
        <!-- Column header -->
        <div class="bg-surface-100-900 border-b-2 border-surface-300-700" style="display: flex; white-space: nowrap; height: {rowHeight}px; position: sticky; top: 0; z-index: 1; margin-bottom: 4px;">
          <span style="width: 9ch; margin-right: 1.5ch; opacity: 0.75; flex-shrink: 0; font-weight: 600;">Offset</span>
          <span style="width: 48ch; margin-right: 1.5ch; flex-shrink: 0; opacity: 0.75; user-select: none; font-weight: 600;">
            {#each Array.from({length: ROW_SIZE}, (_, i) => i) as colIdx}
              <span style="display: inline-block; width: 2ch; text-align: center; margin-right: {colIdx < ROW_SIZE - 1 ? '1ch' : '0'}">{colIdx.toString(16).toUpperCase().padStart(2, '0')}</span>
            {/each}
          </span>
          <span style="opacity: 0.75; font-weight: 600;">ASCII</span>
        </div>
        <div style="height: {topPadding}px;"></div>
        {#each visibleRows as rowIdx (rowIdx)}
          {@const offset = rowIdx * ROW_SIZE}
          {@const end = Math.min(offset + ROW_SIZE, $hexMeta.data.length)}
          {@const len = end - offset}
          <div style="display: flex; white-space: nowrap; height: {rowHeight}px;">
            <span style="width: 9ch; margin-right: 1.5ch; opacity: 0.55; flex-shrink: 0;">{formatOffset(offset)}</span>
            <span style="width: 48ch; margin-right: 1.5ch; flex-shrink: 0; opacity: 0.85; user-select: none;">
              {#each Array.from({length: len}, (_, j) => offset + j) as byteOffset, j}
                {@const isEditingHex = editingOffset === byteOffset && editingMode === "hex"}
                {@const isEditingAscii = editingOffset === byteOffset && editingMode === "ascii"}
                {@const edited = isEdited(byteOffset)}
                {@const byteVal = getByte(byteOffset)}
                {#if isEditingHex}
                  <input
                    type="text"
                    class="hex-edit-input"
                    style="width: 2ch; padding: 0; margin: 0; border: 1px solid #d97706; border-radius: 2px; background: #fbbf24; color: #78350f; outline: none; text-align: center; font-family: inherit; font-size: inherit; line-height: inherit; box-sizing: border-box;"
                    maxlength="2"
                    bind:value={editValue}
                    bind:this={editInputRef}
                  />
                {:else}
                  <span
                    class="hex-cell"
                    style="cursor: pointer; {edited ? 'background: #fef3c7; color: #92400e; font-weight: 600;' : ''}{isEditingAscii ? 'background: #fbbf24; color: #78350f; font-weight: 600; border-radius: 2px;' : ''}"
                    onclick={() => startEdit(byteOffset)}
                    title="Click to edit (offset 0x{byteOffset.toString(16).toUpperCase()})"
                  >{formatHex(byteVal)}</span>
                {/if}
                {#if j < len - 1}
                  <span> </span>
                {/if}
              {/each}
            </span>
            <span style="opacity: 0.7;">
              {#each Array.from({length: len}, (_, j) => offset + j) as byteOffset}
                {@const edited = isEdited(byteOffset)}
                {@const isEditingAscii = editingOffset === byteOffset && editingMode === "ascii"}
                {@const isEditingHex = editingOffset === byteOffset && editingMode === "hex"}
                {@const byteVal = getByte(byteOffset)}
                {#if isEditingAscii}
                  <input
                    type="text"
                    class="hex-edit-input"
                    style="width: 1ch; padding: 0; margin: 0; border: 1px solid #d97706; border-radius: 2px; background: #fbbf24; color: #78350f; outline: none; text-align: center; font-family: inherit; font-size: inherit; line-height: inherit; box-sizing: border-box;"
                    maxlength="1"
                    bind:value={editValue}
                    bind:this={editInputRef}
                  />
                {:else}
                  <span
                    style="cursor: pointer; {edited ? 'background: #fef3c7; color: #92400e; font-weight: 600;' : ''}{isEditingHex ? 'background: #fbbf24; color: #78350f; font-weight: 600; border-radius: 2px;' : ''}"
                    onclick={() => startEdit(byteOffset, "ascii")}
                    title="Click to edit (offset 0x{byteOffset.toString(16).toUpperCase()})"
                  >{toAscii(byteVal)}</span>
                {/if}
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
  .hex-cell:hover {
    background: #e5e7eb;
    color: #111827;
    border-radius: 2px;
  }
</style>
