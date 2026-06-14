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
# ⚠️ Only use when ONLY Rust code changed.
cd gui && npm run build && cargo build --release

# Full production build (embeds fresh frontend into binary)
# Use this when ANY frontend code (Svelte, CSS, JS, HTML) changed.
cd gui && cargo tauri build

# The `.exe` is at:
# gui/src-tauri/target/release/minipro-gui.exe
```

**Critical rule:** If you change any `.svelte`, `.ts`, `.css`, or `.html` file, you **must** run `cargo tauri build`. `cargo build --release` will keep stale embedded frontend assets from the previous full build.

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

### Tauri window permissions (Tauri 2.x)
Adding `window:default` to capabilities does **not** work. Use specific granular permissions:

```json
"permissions": [
  "core:window:allow-set-size",
  "core:window:allow-center"
]
```

### Tauri v2 command parameter naming
Tauri v2 automatically camelCases top-level invoke keys before matching them to Rust function parameter names. The Rust parameter names must use camelCase to match.

**Rule:** When JS sends `{ snake_case: value }`, Tauri converts it to `camelCase`. Rust params must match that camelCase.

```ts
// JS invoke — Tauri auto-converts keys to camelCase
await invoke("write_fuses", { cfg_fuses: cfg, lock_bits: lock, icsp_mode });
// Tauri converts: cfg_fuses -> cfgFuses, lock_bits -> lockBits, icsp_mode -> icspMode
```

```rust
// Rust handler — parameter names must match camelCase keys
#[tauri::command]
pub async fn write_fuses(cfgFuses: Vec<FuseValueDto>, lockBits: Vec<FuseValueDto>, icspMode: String) { ... }
```

**Note:** This only applies to top-level invoke keys. Nested objects (like `options`) are serialized directly by serde and are not affected by Tauri's key mapping.

**Commands using this convention:**
| Command | JS sends | Rust expects |
|---------|----------|--------------|
| `write_fuses` | `cfg_fuses`, `lock_bits`, `icsp_mode` | `cfgFuses`, `lockBits`, `icspMode` |
| `save_bytes_to_file` | `base64Data` | `base64Data` |
| `do_erase`, `do_blank_check`, `do_chip_id`, `do_logic_test`, `read_fuses`, `check_lock_protection` | `icspMode` | `icspMode` (already camelCase in JS) |

## Data Handling

### Large binary files
- The Rust backend returns file bytes as **base64** strings via `invoke("read_file_bytes")`.
- Convert to `Uint8Array` with `atob()` in the frontend.
- **Do NOT fear Svelte reactivity with 256KB Uint8Arrays.** The browser handles 16,384 `<div>` rows natively. Only optimize with virtual scrolling if profiling shows a real problem.
- When rendering hex rows, use direct array indexing (`data[offset + j]`) instead of `.slice()` inside reactive blocks.

### Base64 encoding large arrays
`String.fromCharCode(...data)` crashes with "Maximum call stack size exceeded" for arrays >65K elements. Chunk the conversion:

```ts
const CHUNK = 0x8000; // 32KB
let result = "";
for (let i = 0; i < data.length; i += CHUNK) {
  result += String.fromCharCode(...data.subarray(i, i + CHUNK));
}
return btoa(result);
```

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

## Hex Viewer Layout

Use `ch` (character-width) units for columns so spacing scales with font size:

```svelte
<!-- Offset column: 8 hex chars + 1ch padding -->
<span style="width: 9ch;">{formatOffset(offset)}</span>

<!-- Hex bytes: 32 chars + 15 spaces = 47ch, rounded up -->
<span style="width: 48ch;">{bytes.map(b => formatHex(b)).join(' ')}</span>

<!-- ASCII: natural width -->
<span>{bytes.map(b => toAscii(b)).join('')}</span>
```

## Project Structure

```
gui/
  src/
    App.svelte                 — main layout, operations panel, draggable splitters
    lib/
      stores/
        hex.ts                 — file data, loading state
        operations.ts          — chip read/write/verify/erase/blank-check/chip-id/logic-test/config
        logs.ts                — terminal log entries
        device.ts              — connected programmer + IC database
        settings.ts            — persisted app preferences (includes panel widths)
      components/
        HexViewer.svelte         — hex dump with offset/hex/ascii, save/open/clear
        TerminalLog.svelte       — scrollable log panel with copy/clear
        DeviceSelector.svelte    — search + paginated IC list
        DiagnosticsPanel.svelte  — overcurrent, calibration, pin test
        SettingsPanel.svelte     — theme, defaults, layout reset
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

## Known Bugs & Fixes

### `selectedDevice` store held string instead of object
`DeviceSelector.svelte` was doing `selectedDevice.set(name)` (a string), but the store is typed as `DeviceInfo | null`. Fixed by storing the full `DeviceInfo` object: `selectedDevice.set(selectedInfo)`.

### `do_write` called `erase_chip` before `begin_transaction`
The handle had no active device, so the firmware returned "Protocol error: no device selected". Fixed by calling `begin_transaction(device)` before `erase_chip`.

### Global `select-none` prevented text selection
Adding `select-none` to the root app container blocked selection everywhere including terminal logs. Fixed by only applying it conditionally during active drag operations.

### `verify_chip` panic when file smaller than device
`verify_chip` read the reference file but did not pad it to device size. When auto-verify ran after a write with a smaller file, `expected[offset..]` panicked at offsets beyond the file length. Fixed by resizing the expected buffer to `size` with blank_value padding, matching `write_chip` behavior.

## Release Versioning

### Keep GUI and CLI versions in sync

All version numbers in the repo must match for any given release:

| File | Field | Example |
|------|-------|---------|
| `Cargo.toml` (workspace root) | `version` | `0.2.4` |
| `gui/src-tauri/Cargo.toml` | `version` | `0.2.4` |
| `gui/src-tauri/tauri.conf.json` | `version` | `0.2.4` |
| `gui/package.json` | `version` | `0.2.4` |

**Why:** The project is a monorepo with a single tag (`v0.2.4`) that triggers builds for both the CLI and GUI. If versions drift:
- GUI installer filenames will show the wrong version (e.g., `MINIPRO-RS_0.2.0_x64.msi` inside a `v0.2.3` release)
- Users get confused about which version they have
- Changelogs become unreliable

**When bumping for a release:**
1. Update all four version fields above
2. Commit with message like `chore(release): bump version to X.Y.Z`
3. Create/push the tag `vX.Y.Z`
4. Let CI build and release everything consistently

---

### Pre-commit checks (prevent CI failures)

The GitLab CI runs `cargo fmt --all -- --check` and `cargo clippy`. Running these locally before pushing prevents red pipelines.

```bash
# Must pass before committing Rust changes
cargo fmt --all          # auto-fixes formatting
cargo fmt --all -- --check   # verify (same as CI)
cargo clippy --all-targets -- -D warnings  # lint check
```

**Common `fmt` failure:** Inline comment alignment. `rustfmt` enforces exactly **one space** before `//`. This will fail:
```rust
msg[0x65] = 0x03;                              // comment  (too much space)
```
This passes:
```rust
msg[0x65] = 0x03; // comment
```

**When editing `.svelte` / `.ts` files:** Run `cargo tauri build` (not just `cargo build`), because the frontend is embedded into the Rust binary at build time.

**Commit `Cargo.lock` after dependency changes:** The CI runs `cargo test --all --locked`, which fails if `Cargo.lock` is out of sync with `Cargo.toml`. Always regenerate and commit the lockfile when adding/updating dependencies or bumping versions:
```bash
cargo generate-lockfile   # updates Cargo.lock
git add Cargo.lock gui/src-tauri/Cargo.lock
```

---

### Commit message rules

- **Do NOT add any AI attribution footers** to commit messages
- **Do NOT add** `"Generated with [Devin]"` or similar watermarks
- **Do NOT add** `"Co-Authored-By"` lines for AI agents or bots
- **Commit messages must be written as if authored solely by the human developer**
- Keep commit messages concise: a short subject line, optional body, nothing else
