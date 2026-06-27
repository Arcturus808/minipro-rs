<script lang="ts">
  import { hexMeta, hexLoading, clearHexBuffer, hexEdits, setHexEdit, clearHexEdits, applyHexEdits, getHexData, trimTrailing, padToSize } from "../stores/hex";
  import { settings, setSetting } from "../stores/settings";
  import { selectedDevice } from "../stores/device";
  import { saveBufferToFile, openFolder } from "../stores/operations";
  import { pickSaveFile, pickOpenFile, getIterativeSavePath } from "../file-dialog";
  import { get } from "svelte/store";
  import { invoke } from "@tauri-apps/api/core";
  import { logs } from "../stores/logs";

  const ROW_SIZE = 16;
  const BUFFER_ROWS = 5;

  let fontSize = $state($settings.hexViewerFontSize);
  let rowHeight = $derived(fontSize + 9);
  let savedPath = $state<string | null>(null);

  // Trim/Pad panel state
  let showTrimPad = $state(false);
  let padTargetSize = $state("");
  let eraseValue = $state(0xFF);

  // Hex editing state
  let editingOffset = $state<number | null>(null);
  let editValue = $state("");
  let editInputRef = $state<HTMLInputElement | null>(null);
  let editCursorPos = $state(0);
  let editingMode = $state<"hex" | "ascii">("hex");

  // Go-to-offset dialog state
  let showGotoDialog = $state(false);
  let gotoValue = $state("");
  let gotoInputRef = $state<HTMLInputElement | null>(null);

  // ── Diff mode state ────────────────────────────────────────────────────────
  interface DiffEntry { offset: number; value_a: number; value_b: number; }
  interface TailRegion { start: number; end: number; kind: "Padding" | "Anomalous"; longer_side: string; }
  interface DiffSummary {
    diff_count: number; diff_regions: number; size_a: number; size_b: number;
    padding_tail: number; anomalous_tail: number; is_equal: boolean;
  }
  interface DiffResult { diffs: DiffEntry[]; tails: TailRegion[]; summary: DiffSummary; }

  let diffResult = $state<DiffResult | null>(null);
  let diffRefPath = $state<string | null>(null);
  let diffNavIndex = $state(0); // current position in diff list for navigation
  let diffComparing = $state(false);

  // Set of differing offsets for quick lookup during rendering
  let diffOffsets = $derived(new Set(diffResult?.diffs.map(d => d.offset) ?? []));
  // Tail region info for rendering: map offset → kind
  let tailMap = $derived(() => {
    const m = new Map<number, "Padding" | "Anomalous">();
    if (!diffResult) return m;
    for (const t of diffResult.tails) {
      for (let i = t.start; i < t.end; i++) {
        m.set(i, t.kind);
      }
    }
    return m;
  });

  // Determine the effective size for rendering (may be longer than hexMeta.data if reference is longer)
  let diffRenderSize = $derived(diffResult ? Math.max($hexMeta?.data?.length ?? 0, diffResult.summary.size_b) : ($hexMeta?.data?.length ?? 0));

  $effect(() => {
    fontSize = $settings.hexViewerFontSize;
  });

  function parseOffset(input: string): number | null {
    const trimmed = input.trim();
    if (trimmed.startsWith("0x") || trimmed.startsWith("0X")) {
      const v = parseInt(trimmed.slice(2), 16);
      return isNaN(v) ? null : v;
    }
    const v = parseInt(trimmed, 10);
    return isNaN(v) ? null : v;
  }

  function openGotoDialog() {
    if (!($hexMeta?.data) || $hexMeta.data.length === 0) return;
    // Commit any pending hex edit before opening the dialog.
    // Navigation (Ctrl+G) should not discard the user's current edit.
    commitEdit();
    showGotoDialog = true;
    gotoValue = "";
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        gotoInputRef?.focus();
        gotoInputRef?.select();
      });
    });
  }

  function closeGotoDialog() {
    showGotoDialog = false;
    gotoValue = "";
  }

  function confirmGoto() {
    const offset = parseOffset(gotoValue);
    if (offset === null) return;
    const dataLen = $hexMeta?.data?.length ?? 0;
    if (dataLen === 0) return;
    const clamped = Math.max(0, Math.min(offset, dataLen - 1));
    closeGotoDialog();
    // Defer startEdit so the modal fully unmounts and focus settles
    // before we try to focus the hex edit input.
    requestAnimationFrame(() => {
      startEdit(clamped, "hex");
    });
  }

  // Global keydown for go-to-offset (Ctrl+G)
  function handleGlobalKeydown(e: KeyboardEvent) {
    if (e.key === "g" && e.ctrlKey && !e.altKey && !e.metaKey && !e.shiftKey) {
      e.preventDefault();
      if (!showGotoDialog) {
        openGotoDialog();
      }
    }
  }

  $effect(() => {
    document.addEventListener("keydown", handleGlobalKeydown);
    return () => document.removeEventListener("keydown", handleGlobalKeydown);
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

  let totalRows = $derived(Math.ceil(diffRenderSize / ROW_SIZE));
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

  function scrollToOffset(offset: number) {
    if (!scrollContainer || !($hexMeta?.data)) return;
    const row = Math.floor(offset / ROW_SIZE);
    const rowTop = row * rowHeight;
    const rowBottom = rowTop + rowHeight;
    const viewTop = scrollContainer.scrollTop;
    const viewBottom = viewTop + containerHeight;
    // If the row is already fully visible, don't scroll
    if (rowTop >= viewTop && rowBottom <= viewBottom) return;
    // Scroll to center the row, or to the edge if near the top/bottom
    const target = Math.max(0, rowTop - containerHeight / 2 + rowHeight / 2);
    scrollContainer.scrollTo({ top: target, behavior: "auto" });
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
    scrollToOffset(offset);
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
      case "ArrowLeft": {
        e.preventDefault();
        const current = editingOffset;
        if (current !== null && current > 0) {
          commitEdit();
          startEdit(current - 1);
        }
        break;
      }
      case "ArrowRight": {
        e.preventDefault();
        const current = editingOffset;
        if (current !== null && current < dataLen - 1) {
          commitEdit();
          startEdit(current + 1);
        }
        break;
      }
      case "ArrowUp": {
        e.preventDefault();
        const current = editingOffset;
        if (current !== null && current >= ROW_SIZE) {
          commitEdit();
          startEdit(current - ROW_SIZE);
        }
        break;
      }
      case "ArrowDown": {
        e.preventDefault();
        const current = editingOffset;
        if (current !== null && current < dataLen - ROW_SIZE) {
          commitEdit();
          startEdit(current + ROW_SIZE);
        }
        break;
      }
    }
  }

  // Attach/detach global keydown listener when editing state changes
  $effect(() => {
    if (editingOffset !== null) {
      document.addEventListener("keydown", handleEditKeydown);
      return () => document.removeEventListener("keydown", handleEditKeydown);
    }
  });

  // ── Diff mode helpers ──────────────────────────────────────────────────────

  function uint8ArrayToBase64(data: Uint8Array): string {
    const CHUNK_SIZE = 0x8000;
    let result = "";
    for (let i = 0; i < data.length; i += CHUNK_SIZE) {
      const chunk = data.subarray(i, i + CHUNK_SIZE);
      result += String.fromCharCode(...chunk);
    }
    return btoa(result);
  }

  async function startCompare() {
    const data = getHexData();
    if (!data || data.length === 0) {
      logs.error("No data in hex viewer to compare");
      return;
    }

    const refPath = await pickOpenFile(
      "Select reference file to compare",
      get(settings).defaultDirectory ?? undefined
    );
    if (!refPath) return;

    diffComparing = true;
    try {
      // Commit any pending edits before comparing
      if (get(hexEdits).size > 0) {
        applyHexEdits();
      }
      const freshData = getHexData();
      if (!freshData) return;

      const base64Data = uint8ArrayToBase64(freshData);
      const result = await invoke<DiffResult>("do_smart_diff", {
        base64Data,
        referencePath: refPath,
        eraseValue: 0xFF,
      });
      diffResult = result;
      diffRefPath = refPath;
      diffNavIndex = 0;

      if (result.summary.is_equal) {
        logs.info(`Compare: Files match (ignoring trailing padding) — ${refPath}`);
      } else {
        const s = result.summary;
        let msg = `Compare: ${s.diff_count} byte diff${s.diff_count === 1 ? '' : 's'} across ${s.diff_regions} region${s.diff_regions === 1 ? '' : 's'}`;
        if (s.anomalous_tail > 0) {
          msg += ` · WARNING: ${s.anomalous_tail} anomalous tail region(s) detected`;
        }
        logs.info(msg);
      }
    } catch (e) {
      logs.error(`Compare failed: ${e}`);
      diffResult = null;
      diffRefPath = null;
    } finally {
      diffComparing = false;
    }
  }

  function clearCompare() {
    diffResult = null;
    diffRefPath = null;
    diffNavIndex = 0;
  }

  function navigateDiff(direction: "next" | "prev") {
    if (!diffResult || diffResult.diffs.length === 0) return;
    const n = diffResult.diffs.length;
    if (direction === "next") {
      diffNavIndex = (diffNavIndex + 1) % n;
    } else {
      diffNavIndex = (diffNavIndex - 1 + n) % n;
    }
    const targetOffset = diffResult.diffs[diffNavIndex].offset;
    scrollToOffset(targetOffset);
  }

  // Global keydown for diff navigation (F3 / Shift+F3)
  function handleDiffKeydown(e: KeyboardEvent) {
    if (!diffResult) return;
    if (e.key === "F3" && !e.ctrlKey && !e.altKey && !e.metaKey) {
      e.preventDefault();
      navigateDiff(e.shiftKey ? "prev" : "next");
    }
  }

  $effect(() => {
    if (diffResult) {
      document.addEventListener("keydown", handleDiffKeydown);
      return () => document.removeEventListener("keydown", handleDiffKeydown);
    }
  });

  // Get diff cell style for a given offset
  function getDiffCellStyle(offset: number): string {
    if (!diffResult) return "";
    if (diffOffsets.has(offset)) {
      // Highlight the currently-navigated diff byte more prominently
      const isCurrent = diffResult.diffs.length > 0 && diffResult.diffs[diffNavIndex]?.offset === offset;
      if (isCurrent) {
        return "background: #dc2626; color: #ffffff; font-weight: 700; border-radius: 2px; box-shadow: 0 0 0 2px #fbbf24;";
      }
      return "background: #fee2e2; color: #991b1b; font-weight: 600; border-radius: 2px;";
    }
    const tm = tailMap();
    const tailKind = tm.get(offset);
    if (tailKind === "Anomalous") {
      return "background: #fef3c7; color: #92400e; font-weight: 600; border-radius: 2px;";
    }
    if (tailKind === "Padding") {
      return "background: #f3f4f6; color: #9ca3af; border-radius: 2px;";
    }
    return "";
  }

  // Get byte value for rendering — in diff mode, bytes beyond the chip buffer
  // (from the reference file) show as the reference value
  function getRenderByte(offset: number): number {
    const dataLen = $hexMeta?.data?.length ?? 0;
    if (offset < dataLen) {
      return getByte(offset);
    }
    // Beyond chip data — this is from the reference file's tail
    // We don't have the reference bytes in the frontend, so show 0xFF or 0x00
    // The tail coloring already indicates this is padding or anomalous
    return 0xFF;
  }
</script>

<div class="hex-viewer-container" style="border: 1px solid #ccc; display: flex; flex-direction: column; height: 100%;">
  <!-- Row 1: Title + metadata (left), font size + reset (right) -->
  <div style="padding: 8px 12px; border-bottom: 1px solid #ccc; display: flex; align-items: center; justify-content: space-between;">
    <div style="min-width: 0; flex-shrink: 1;">
      <div style="font-size: 14px; font-weight: 600;">Hex Viewer</div>
      {#if $hexMeta}
        <div style="font-size: 12px; opacity: 0.6; margin-top: 2px; white-space: nowrap;">
          {$hexMeta.size.toLocaleString()} bytes
          {#if $hexMeta.crc32 !== null}
            · CRC-32: {$hexMeta.crc32.toString(16).padStart(8, '0').toUpperCase()}
          {/if}
        </div>
        {#if editCount > 0}
          <div style="font-size: 12px; color: #f59e0b; margin-top: 2px; font-weight: 500; white-space: nowrap;">
            {editCount} edit{editCount === 1 ? '' : 's'} pending
          </div>
        {/if}
        {#if diffResult}
          <div style="font-size: 12px; margin-top: 2px; font-weight: 500; color: {diffResult.summary.is_equal ? '#16a34a' : '#dc2626'}; white-space: nowrap;">
            {#if diffResult.summary.is_equal}
              Compare: Files match (ignoring trailing padding)
            {:else if diffResult.summary.diff_count > 0}
              Compare: {diffResult.summary.diff_count} diff{diffResult.summary.diff_count === 1 ? '' : 's'} across {diffResult.summary.diff_regions} region{diffResult.summary.diff_regions === 1 ? '' : 's'}
            {:else}
              Compare: No byte diffs, but {diffResult.summary.anomalous_tail} anomalous tail region(s)
            {/if}
          </div>
        {/if}
      {/if}
    </div>
    <div class="flex items-center gap-2 flex-shrink-0">
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
      {/if}
    </div>
  </div>
  <!-- Row 2: Action buttons (wraps naturally if too wide) -->
  {#if $hexMeta}
    <div class="flex items-center gap-2 flex-wrap" style="padding: 6px 12px; border-bottom: 1px solid #ccc;">
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
            [
              { name: "Binary", extensions: ["bin"] },
              { name: "Intel HEX", extensions: ["hex"] },
              { name: "Motorola SREC", extensions: ["srec", "mot"] },
              { name: "JEDEC", extensions: ["jed"] },
            ]
          );
          if (path) {
            if (!path.includes(".")) {
              path += ".bin";
            }
            const ext = path.split(".").pop()?.toLowerCase() ?? "bin";
            const format = ext === "hex" ? "ihex" : ext === "srec" || ext === "mot" ? "srec" : ext === "jed" ? "jedec" : "bin";
            await setSetting("defaultDirectory", path.substring(0, path.lastIndexOf("\\") || path.lastIndexOf("/")));
            await saveBufferToFile(path, format, dev?.name);
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
        onclick={() => { savedPath = null; clearHexEdits(); clearHexBuffer(); clearCompare(); }}
      >
        Clear
      </button>
      {#if $hexMeta?.data}
        <button
          class="opacity-70 hover:opacity-100 transition-opacity px-3 py-1.5 rounded border border-transparent hover:border-surface-200-800"
          style="font-size: 13px;"
          onclick={() => showTrimPad = !showTrimPad}
          title="Trim trailing padding or pad to a specific size"
        >
          Trim/Pad
        </button>
      {/if}
      {#if showTrimPad && $hexMeta?.data}
        <div class="flex items-center gap-3 px-3 py-1.5 rounded border border-surface-200-800 bg-surface-100-900" style="font-size: 13px;">
          <label class="flex items-center gap-1" title="Byte value used for trimming and padding">
            <span class="opacity-60">Fill byte:</span>
            <select bind:value={eraseValue} class="px-1 py-1 rounded border border-surface-200-800 bg-transparent" style="font-size: 13px;">
              <option value={0xFF}>0xFF (NOR flash)</option>
              <option value={0x00}>0x00 (EEPROM/NAND)</option>
            </select>
          </label>
          <button
            class="px-2 py-1 rounded bg-primary-600 text-white hover:bg-primary-700 transition-colors"
            onclick={() => {
              const result = trimTrailing(eraseValue);
              if (result !== null) {
                logs.info(`Trimmed trailing 0x${eraseValue.toString(16).toUpperCase()} bytes → ${result} bytes`);
                savedPath = null;
              } else {
                logs.info("No trailing fill bytes to trim");
              }
            }}
            title="Remove trailing bytes equal to the fill byte"
          >
            Trim trailing
          </button>
          <div class="flex items-center gap-1">
            <span class="opacity-60">Pad to:</span>
            <input
              type="text"
              bind:value={padTargetSize}
              placeholder="size"
              class="w-20 px-2 py-1 rounded border border-surface-200-800 bg-transparent font-mono"
              style="font-size: 13px;"
              onkeydown={(e) => { if (e.key === "Enter") (document.getElementById("pad-btn") as HTMLButtonElement)?.click(); }}
            />
            <button
              id="pad-btn"
              class="px-2 py-1 rounded bg-primary-600 text-white hover:bg-primary-700 transition-colors"
              onclick={() => {
                const ts = parseInt(padTargetSize, padTargetSize.startsWith("0x") ? 16 : 10);
                if (isNaN(ts) || ts <= 0) {
                  logs.error("Invalid pad target size");
                  return;
                }
                const result = padToSize(ts, eraseValue);
                if (result !== null) {
                  logs.info(`Padded to ${result} bytes with 0x${eraseValue.toString(16).toUpperCase()}`);
                  savedPath = null;
                } else {
                  logs.info(`Buffer is already ${$hexMeta!.size} bytes — no padding needed`);
                }
              }}
              title="Pad buffer to the specified size with fill byte"
            >
              Pad
            </button>
          </div>
          <button
            class="opacity-50 hover:opacity-100 transition-opacity"
            onclick={() => showTrimPad = false}
            title="Close trim/pad panel"
          >
            ✕
          </button>
        </div>
      {/if}
      {#if !diffResult}
        <button
          class="opacity-70 hover:opacity-100 transition-opacity px-3 py-1.5 rounded border border-transparent hover:border-surface-200-800 flex items-center gap-1.5"
          style="font-size: 13px;"
          onclick={startCompare}
          disabled={diffComparing}
          title="Compare hex buffer against a reference file (F3 to navigate diffs)"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M8 7h8m-8 5h8m-8 5h8M3 5l4 4-4 4M21 5l-4 4 4 4" />
          </svg>
          {diffComparing ? "Comparing..." : "Compare"}
        </button>
      {:else}
        {#if diffResult.diffs.length > 0}
          <button
            class="opacity-70 hover:opacity-100 transition-opacity px-3 py-1.5 rounded border border-transparent hover:border-surface-200-800"
            style="font-size: 13px; white-space: nowrap;"
            onclick={() => navigateDiff("prev")}
            title="Previous diff (Shift+F3)"
          >↑ Prev</button>
          <span style="font-size: 12px; opacity: 0.6; min-width: 60px; text-align: center; white-space: nowrap;">
            {diffNavIndex + 1}/{diffResult.diffs.length}
          </span>
          <button
            class="opacity-70 hover:opacity-100 transition-opacity px-3 py-1.5 rounded border border-transparent hover:border-surface-200-800"
            style="font-size: 13px; white-space: nowrap;"
            onclick={() => navigateDiff("next")}
            title="Next diff (F3)"
          >Next ↓</button>
        {/if}
        <button
          class="opacity-70 hover:opacity-100 transition-opacity px-3 py-1.5 rounded border border-transparent hover:border-surface-200-800"
          style="font-size: 13px; color: #dc2626; white-space: nowrap;"
          onclick={clearCompare}
          title="Clear comparison"
        >✕ Clear Compare</button>
      {/if}
    </div>
  {/if}
  {#if diffResult && diffResult.summary.anomalous_tail > 0}
    <div style="padding: 6px 12px; background: #fef3c7; border-bottom: 1px solid #f59e0b; font-size: 12px; color: #92400e; display: flex; align-items: center; gap: 8px;">
      <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4 flex-shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
        <path stroke-linecap="round" stroke-linejoin="round" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
      </svg>
      <span>
        <strong>WARNING:</strong> Reference file has non-padding data beyond the buffer length —
        possible truncated read, wrong chip selected, or leftover data from previous programming.
        {#each diffResult.tails.filter(t => t.kind === "Anomalous") as tail}
          <br>Tail [0x{tail.start.toString(16).toUpperCase()}..0x{tail.end.toString(16).toUpperCase()}] in buffer {tail.longer_side}
        {/each}
      </span>
    </div>
  {/if}
  {#if showGotoDialog}
    <div style="position: absolute; top: 0; left: 0; right: 0; bottom: 0; display: flex; align-items: center; justify-content: center; z-index: 10; background: rgba(0,0,0,0.3);"
      onclick={(e) => { if (e.target === e.currentTarget) closeGotoDialog(); }}
    >
      <div style="background: var(--bg-color, #fff); border: 1px solid #ccc; border-radius: 6px; padding: 16px; box-shadow: 0 4px 16px rgba(0,0,0,0.2); min-width: 280px;">
        <div style="font-size: 14px; font-weight: 600; margin-bottom: 8px;">Go to Offset</div>
        <div style="font-size: 12px; opacity: 0.6; margin-bottom: 12px;">Enter offset in hex (0x1234) or decimal (1234)</div>
        <input
          bind:this={gotoInputRef}
          bind:value={gotoValue}
          type="text"
          placeholder="0x0"
          style="width: 100%; padding: 6px 8px; border: 1px solid #ccc; border-radius: 4px; font-family: 'Hack', 'Consolas', 'Courier New', monospace; font-size: 14px; outline: none;"
          onkeydown={(e: KeyboardEvent) => {
            if (e.key === "Enter") { e.preventDefault(); confirmGoto(); }
            if (e.key === "Escape") { e.preventDefault(); closeGotoDialog(); }
          }}
        />
        <div style="display: flex; justify-content: flex-end; gap: 8px; margin-top: 12px;">
          <button
            style="padding: 6px 12px; border: 1px solid #ccc; border-radius: 4px; background: transparent; cursor: pointer; font-size: 13px;"
            onclick={closeGotoDialog}
          >Cancel</button>
          <button
            style="padding: 6px 12px; border: none; border-radius: 4px; background: #d97706; color: white; cursor: pointer; font-size: 13px;"
            onclick={confirmGoto}
          >Go</button>
        </div>
      </div>
    </div>
  {/if}

  <div
    bind:this={scrollContainer}
    onscroll={onScroll}
    style="flex: 1; overflow: auto; font-family: 'Hack', 'Consolas', 'Courier New', monospace; font-size: {fontSize}px; line-height: {rowHeight}px; padding: 0 8px 8px 8px;"
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
          {@const end = Math.min(offset + ROW_SIZE, diffRenderSize)}
          {@const len = end - offset}
          <div style="display: flex; white-space: nowrap; height: {rowHeight}px;">
            <span style="width: 9ch; margin-right: 1.5ch; opacity: 0.55; flex-shrink: 0;">{formatOffset(offset)}</span>
            <span style="width: 48ch; margin-right: 1.5ch; flex-shrink: 0; opacity: 0.85; user-select: none;">
              {#each Array.from({length: len}, (_, j) => offset + j) as byteOffset, j}
                {@const isEditingHex = editingOffset === byteOffset && editingMode === "hex"}
                {@const isEditingAscii = editingOffset === byteOffset && editingMode === "ascii"}
                {@const edited = isEdited(byteOffset)}
                {@const byteVal = getRenderByte(byteOffset)}
                {@const diffStyle = getDiffCellStyle(byteOffset)}
                {#if isEditingHex}
                  <input
                    type="text"
                    class="hex-edit-input"
                    style="width: 2ch; padding: 0; margin: 0; border: none; border-radius: 2px; background: #fbbf24; color: #78350f; outline: none; text-align: center; font-family: inherit; font-size: inherit; line-height: inherit; box-sizing: border-box; display: inline-block; vertical-align: middle;"
                    maxlength="2"
                    bind:value={editValue}
                    bind:this={editInputRef}
                  />
                {:else}
                  <span
                    class="hex-cell"
                    style="cursor: pointer; display: inline-block; vertical-align: middle; {edited ? 'background: #fef3c7; color: #92400e; font-weight: 600;' : diffStyle}{isEditingAscii ? 'background: #fbbf24; color: #78350f; font-weight: 600; border-radius: 2px;' : ''}"
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
                {@const byteVal = getRenderByte(byteOffset)}
                {@const diffStyle = getDiffCellStyle(byteOffset)}
                {#if isEditingAscii}
                  <input
                    type="text"
                    class="hex-edit-input"
                    style="width: 1ch; padding: 0; margin: 0; border: none; border-radius: 2px; background: #fbbf24; color: #78350f; outline: none; text-align: center; font-family: inherit; font-size: inherit; line-height: inherit; box-sizing: border-box; display: inline-block; vertical-align: middle;"
                    maxlength="1"
                    bind:value={editValue}
                    bind:this={editInputRef}
                  />
                {:else}
                  <span
                    style="cursor: pointer; display: inline-block; vertical-align: middle; {edited ? 'background: #fef3c7; color: #92400e; font-weight: 600;' : diffStyle}{isEditingHex ? 'background: #fbbf24; color: #78350f; font-weight: 600; border-radius: 2px;' : ''}"
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
