# Minipro GUI — Developer Guide

## Tech Stack

| Layer | Version | Constraint |
|-------|---------|------------|
| Svelte | 5.x | **Use Svelte 5 runes exclusively** (`$state`, `$derived`, `$effect`). Do NOT mix with legacy `$:` syntax. |
| Tauri | 2.x | WebView2 on Windows. Native dialogs freeze the JS event loop. |
| Vite | 6.x | Frontend bundler. `npm run build` produces `dist/`. |
| Rust | 1.77+ | Backend commands in `src-tauri/src/commands.rs`. |

## Build Commands

```bash
# Fast dev build (reuses cached Rust artifacts)
cd gui && npm run build && cargo build --release

# Full production build (embeds fresh frontend into binary)
cd gui && cargo tauri build

# The `.exe` is at:
# gui/src-tauri/target/release/minipro-gui.exe
```

## Store Patterns (CRITICAL)

### Rule 1: All state lives in writable stores
NEVER use module-level variables for state that components read.

```ts
// BAD — _hexData is invisible to Svelte reactivity
let _hexData: Uint8Array | null = null;
export const hexMeta = writable({ size: 0, path: null });

// GOOD — everything the component needs is in the store
interface HexState {
  data: Uint8Array | null;
  path: string | null;
  size: number;
}
export const hexState = writable<HexState>({ data: null, path: null, size: 0 });
```

### Rule 2: Read store data directly in templates
Do NOT extract store values into local variables. Read `$store.field` directly in `{#each}` and `{#if}` blocks.

```svelte
<!-- GOOD -->
{#each $hexState.data.slice(0, 16) as b}
  <span>{b.toString(16).padStart(2, '0')}</span>
{/each}

<!-- BAD — data may not update reactively -->
<script>
  let bytes = $derived($hexState.data);  // avoid this pattern
</script>
{#each bytes as b}...{/each}
```

### Rule 3: Loading state is part of the store
Keep `loading` in the same store as the data, or in a dedicated companion store. Never infer loading from "data is null."

```ts
export const hexLoading = writable(false);
```

## Tauri / WebView2 Gotchas

### Dialogs freeze the JS event loop
When a Tauri native file dialog is open, the WebView2 thread is paused. **Any reactive update that triggers DOM work during or immediately after the dialog can deadlock.**

**Rules:**
1. Do NOT call `tick()` after a dialog closes.
2. Do NOT update stores that trigger heavy DOM updates (e.g., 16,000-row `{#each}`) inside the same microtask as the dialog close.
3. If you must update stores after a dialog, wrap in `requestAnimationFrame(() => { ... })` or `setTimeout(..., 0)`.
4. Prefer moving `logs.info()` calls **outside** `loadFile()` and into the caller, after the dialog scope has exited.

### DevTools
DevTools must be enabled in **both** places:

```toml
# gui/src-tauri/Cargo.toml
tauri = { version = "2", features = ["devtools"] }
```

```json
// gui/src-tauri/tauri.conf.json
"app": {
  "windows": [ ... ],
  "security": { "csp": null },
  "devtools": true
}
```

Right-click → Inspect and F12 will not work otherwise.

## Data Handling

### Large binary files
- The Rust backend returns file bytes as **base64** strings via `invoke("read_file_bytes")`.
- Convert to `Uint8Array` with `atob()` in the frontend.
- **Do NOT fear Svelte reactivity with 256KB Uint8Arrays.** The browser handles 16,384 `<div>` rows natively. Only optimize with virtual scrolling if profiling shows a real problem.
- When rendering hex rows, use direct array indexing (`data[offset + j]`) instead of `.slice()` inside reactive blocks.

## Component Conventions

1. **Use Svelte 5 runes exclusively.** Prefer `$state` for local variables, `$derived` for computed values, and `$effect` for side effects. Do NOT use legacy `$:` syntax.
2. **No virtual scrolling until needed.** Start with native `overflow: auto` and `{#each}`. Browser scrolling is highly optimized.
3. **Loading indicators** should be conditional on a dedicated loading store, not inferred from data absence:
   ```svelte
   {#if $hexLoading}
     <Spinner />
   {:else if $hexState.data}
     <HexRows data={$hexState.data} />
   {:else}
     <EmptyState />
   {/if}
   ```

## Project Structure

```
gui/
  src/
    App.svelte                 — main layout, operations panel
    lib/
      stores/
        hex.ts                 — file data, loading state
        operations.ts          — chip read/write/verify/erase
        logs.ts                — terminal log entries
        device.ts              — connected programmer + IC database
        settings.ts            — persisted app preferences
      components/
        HexViewer.svelte         — hex dump with offset/hex/ascii
        TerminalLog.svelte       — scrollable log panel
        DeviceSelector.svelte    — search + paginated IC list
        DiagnosticsPanel.svelte  — overcurrent, calibration, pin test
        SettingsPanel.svelte     — theme, auto-detect, etc.
        ProgressPanel.svelte     — operation progress + cancel
      file-dialog.ts             — Tauri dialog wrappers
  src-tauri/
    src/
      commands.rs                — all Rust command handlers
      lib.rs                     — Tauri app builder + plugin init
      state.rs                   — AppState (USB handle, selected device)
    Cargo.toml
    tauri.conf.json
```
