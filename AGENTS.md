# Minipro — Developer Guide

Rules every agent needs before touching anything. Feature internals and bug
history live in `docs/` — see "Feature internals" at the bottom.

## Critical rules — read first

- **All work on its own branch** — never commit to `main`. Prefix: `fix/`,
  `feat/`, `refactor/`, `docs/`, `chore/`, `release/`. Branch from `main`,
  merge back `--no-ff`, delete the branch after merging. Merge message:
  `Merge <branch>: <description> [skip ci]`.
- **`[skip ci]` on every commit and merge** — CI minutes are limited;
  pipelines run only during release prep.
- **No AI attribution in commits** — no "Generated with …", no
  Co-Authored-By, no watermarks. Messages read as human-authored.
- **Frontend changed (`.svelte`/`.ts`/`.css`/`.html`)? Build with
  `cargo tauri build`** — `cargo build --release` keeps stale embedded
  assets.
- **Code changed? Verify locally before committing:**
  `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test --all --locked`.

## Tech Stack

| Layer | Version | Constraint |
|-------|---------|------------|
| Svelte | 5.x | **Use Svelte 5 runes exclusively** (`$state`, `$derived`, `$effect`). Do NOT mix with legacy `$:` syntax. |
| Tauri | 2.x | WebView2 on Windows. Native dialogs freeze the JS event loop. |
| Vite | 6.x | Frontend bundler. `npm run build` produces `dist/`. |
| Rust | 1.88+ (GUI) / 1.85+ (CLI) | Backend commands in `src-tauri/src/commands.rs`. |

## Build Commands

```bash
# Rust-only changes (no .svelte/.ts/.css/.html touched):
cd gui && npm run build && cargo build --release

# ANY frontend change — embeds fresh assets into the binary:
cd gui && cargo tauri build
# .exe at gui/src-tauri/target/release/minipro-gui.exe
```

**Critical rule:** If you change any `.svelte`, `.ts`, `.css`, or `.html`
file, you **must** run `cargo tauri build`. This also applies after version
bumps — the GUI version badge reads `gui/package.json` at Vite build time.

### GUI development workflows

**Fast iteration (active UI development):** `cd gui && npm run dev` — Vite
dev server with HMR; Svelte/CSS/TS changes appear instantly in the browser
preview. Tauri commands won't work (no Rust backend).

**Full verification (final testing with the real backend):**

```powershell
# The running .exe locks the output binary on Windows — kill it first.
Get-Process minipro-gui -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 1
cd gui && cargo tauri build
Start-Process "gui\src-tauri\target\release\minipro-gui.exe"
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
Do NOT extract store values into local variables. Read `$store.field`
directly in `{#each}` and `{#if}` blocks.

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
Keep `loading` in the same store as the data, or in a dedicated companion
store. Never infer loading from "data is null."

```ts
export const hexLoading = writable(false);
```

## Tauri / WebView2 Gotchas

### Dialogs freeze the JS event loop
When a Tauri native file dialog is open, the WebView2 thread is paused.
**Any reactive update that triggers DOM work during or immediately after
the dialog can deadlock.**

**Rules:**
1. Do NOT call `tick()` after a dialog closes.
2. Do NOT update stores that trigger heavy DOM updates (e.g., 16,000-row
   `{#each}`) inside the same microtask as the dialog close.
3. If you must update stores after a dialog, wrap in
   `requestAnimationFrame(() => { ... })` or `setTimeout(..., 0)`.
4. Prefer moving `logs.info()` calls **outside** `loadFile()` and into the
   caller, after the dialog scope has exited.

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
Adding `window:default` to capabilities does **not** work. Use specific
granular permissions:

```json
"permissions": [
  "core:window:allow-set-size",
  "core:window:allow-center"
]
```

### Tauri v2 command parameter naming
Tauri v2 automatically camelCases top-level invoke keys before matching
them to Rust function parameter names. The Rust parameter names must use
camelCase to match.

**Rule:** When JS sends `{ snake_case: value }`, Tauri converts it to
`camelCase`. Rust params must match that camelCase.

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

**Note:** This only applies to top-level invoke keys. Nested objects (like
`options`) are serialized directly by serde and are not affected by
Tauri's key mapping.

**Commands using this convention:**
| Command | JS sends | Rust expects |
|---------|----------|--------------|
| `write_fuses` | `cfg_fuses`, `lock_bits`, `icsp_mode` | `cfgFuses`, `lockBits`, `icspMode` |
| `save_bytes_to_file` | `base64Data` | `base64Data` |
| `do_erase`, `do_blank_check`, `do_chip_id`, `do_logic_test`, `read_fuses`, `check_lock_protection` | `icspMode` | `icspMode` (already camelCase in JS) |
| `do_logic_identify` | `pinCount`, `vcc` | `pinCount`, `vcc` |

## Data Handling

### Large binary files
- The Rust backend returns file bytes as **base64** strings via
  `invoke("read_file_bytes")`.
- Convert to `Uint8Array` with `atob()` in the frontend.
- **Do NOT fear Svelte reactivity with 256KB Uint8Arrays.** The browser
  handles 16,384 `<div>` rows natively. Only optimize with virtual
  scrolling if profiling shows a real problem.
- When rendering hex rows, use direct array indexing (`data[offset + j]`)
  instead of `.slice()` inside reactive blocks.

### Base64 encoding large arrays
`String.fromCharCode(...data)` crashes with "Maximum call stack size
exceeded" for arrays >65K elements. Chunk the conversion:

```ts
const CHUNK = 0x8000; // 32KB
let result = "";
for (let i = 0; i < data.length; i += CHUNK) {
  result += String.fromCharCode(...data.subarray(i, i + CHUNK));
}
return btoa(result);
```

## Component Conventions

1. **Use Svelte 5 runes exclusively.** Prefer `$state` for local variables,
   `$derived` for computed values, and `$effect` for side effects. Do NOT
   use legacy `$:` syntax.
2. **No virtual scrolling until needed.** Start with native
   `overflow: auto` and `{#each}`. Browser scrolling is highly optimized.
3. **Loading indicators** should be conditional on a dedicated loading
   store, not inferred from data absence:
   ```svelte
   {#if $hexLoading}
     <Spinner />
   {:else if $hexState.data}
     <HexRows data={$hexState.data} />
   {:else}
     <EmptyState />
   {/if}
   ```

## Git, CI & Release

### Commits

- Always `[skip ci]` — the only exception is deliberate release pipelines.
- No AI attribution footers, "Generated with …", or Co-Authored-By lines.
- `rustfmt` enforces exactly **one space** before `//` comments — a common
  CI `fmt` failure.
- After dependency or version changes, run `cargo generate-lockfile` and
  commit `Cargo.lock` + `gui/src-tauri/Cargo.lock` — CI runs
  `cargo test --all --locked`.

### Versioning & releases

- All four version fields must stay in sync: root `Cargo.toml`,
  `gui/src-tauri/Cargo.toml`, `gui/src-tauri/tauri.conf.json`,
  `gui/package.json`. The `verify-versions` CI job fails on mismatch.
- **Never push tags casually** — each tag triggers both GitLab CI and
  GitHub Actions release builds.
- **Full release procedure: `docs/RELEASE.md`** — read it before preparing
  any release (version bump checklist, `cargo deny` audit, tag push order,
  MSRV-bump file list).

### CI compute conservation

GitLab CI runs on every `main` push, every tag, and manual triggers.
GitHub Actions runs on tags only. Branch pushes are free on both.
Batch commits — one push = one pipeline run.

Local CI-equivalent checks (run from repo root):

| Check | Command |
|-------|---------|
| fmt | `cargo fmt --all -- --check` |
| clippy | `cargo clippy --all-targets -- -D warnings` |
| tests | `cargo test --all --locked` |

### MSRV & cross-distro verification

MSRV: CLI/core **1.85**, GUI **1.88** (`rust-version` in each Cargo.toml).
CI green on `rust:1.93` does NOT prove MSRV compliance — a Debian Stable
user hit `is_multiple_of()` being unstable on 1.85. When in doubt, use the
older equivalent (`x % y == 0`). The GitLab `msrv` job runs
`cargo check --all --locked` on `rust:1.85` (root workspace only — GUI MSRV
is not CI-enforced).

Before changing Linux package names/install commands, verify against real
distro package databases — names drift (e.g. `libappindicator3-dev` became
`libayatana-appindicator3-dev` in Debian Trixie/Ubuntu 24.04). Cover
Debian/Ubuntu, Fedora, Arch, openSUSE at minimum. Update BOTH `README.md`
and `gui/README.md`, plus `.github/workflows/release.yml` if it installs
system packages.

**When bumping MSRV, update ALL of these in one commit:** root
`Cargo.toml`, `gui/src-tauri/Cargo.toml`, `README.md`, `gui/README.md`,
this file, and `.gitlab-ci.yml` (msrv job image tag).

Local Linux verification (WSL/Docker/native recipes, incl. MSRV check):
see `docs/RELEASE.md` — run it before releases and before merging
build-affecting changes.

## Feature internals — in docs/

Read the relevant doc before working in that area:

- `docs/GUI-INTERNALS.md` — project structure map, hex viewer (layout,
  hotkeys, selection model, pending edits/dirty flag, diff), pin-contact
  test (incl. the T76 `0x3E` hardware caveat), logic IC test/identify,
  config/fuse panel, favorites store, terminal rendering rules.
- `docs/CLI-INTERNALS.md` — CLI flag semantics, `--logic-identify`,
  `-z`/`--pin-check` gate behavior.
- `docs/KNOWN-BUGS.md` — historical bugs & fixes. Check before touching
  related code; several fixes encode upstream C minipro behavior that must
  be preserved (voltage tables, chip-ID parsing, `-x`/`-y` semantics,
  `can_erase`).
- `docs/RELEASE.md` — full release-prep procedure (version sync, cargo-deny
  audit, tag push order, MSRV bump file list).
