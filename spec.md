# MINIPRO-RS GUI — Specification

This document describes the architecture, conventions, and data flow of the MINIPRO-RS GUI application. It is intended as a reference for contributors and maintainers.

---

## 1. Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                     Frontend (Svelte 5)                     │
│  ┌──────────────┐  ┌──────────────┐  ┌────────────────┐  │
│  │ App.svelte   │  │ Stores       │  │ Components     │  │
│  │ (main layout)│  │ (operations, │  │ (HexViewer,    │  │
│  │              │  │  device, hex,│  │  TerminalLog,  │  │
│  │              │  │  logs,       │  │  DeviceSelector│  │
│  │              │  │  settings)   │  │  )             │  │
│  └──────┬───────┘  └──────┬───────┘  └───────┬────────┘  │
│         │                   │                    │           │
│         └───────────────────┴────────────────────┘       │
│                             │                              │
│                     invoke() calls                         │
│                     (Tauri v2 IPC)                         │
└─────────────────────────────┬─────────────────────────────┘
                              │
┌─────────────────────────────┼─────────────────────────────┐
│                     Backend (Rust)                          │
│  ┌──────────────────────────┴──────────────────────────┐  │
│  │              Tauri Command Handlers                  │  │
│  │  (gui/src-tauri/src/commands.rs)                       │  │
│  └──────────────────────────┬──────────────────────────┘  │
│                             │                              │
│  ┌──────────────────────────┴──────────────────────────┐  │
│  │              minipro-core Library                    │  │
│  │  (crates/minipro-core/src/)                          │  │
│  │  • operations.rs — read_chip, write_chip, verify_chip │  │
│  │  • protocol/ — TL866A, TL866II+, T56, T76 backends   │  │
│  │  • device.rs — Device, ChipConfig, fuse definitions   │  │
│  │  • usb.rs — USB bulk transfers                         │  │
│  └──────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

---

## 2. Tech Stack & Versions

| Layer | Technology | Version | Notes |
|-------|-----------|---------|-------|
| Frontend framework | Svelte | 5.x | Runes (`$state`, `$derived`, `$effect`) only |
| UI toolkit | Tailwind CSS | 3.x | Skeleton UI components |
| Build tool | Vite | 6.x | Bundles to `gui/dist/` |
| Desktop shell | Tauri | 2.x | WebView2 on Windows |
| Backend language | Rust | 1.77+ | Async via `tokio` |
| USB library | `nusb` | — | Pure-Rust, no `libusb` DLL |

---

## 3. Command Mapping

### 3.1 Frontend → Backend Commands

All commands are invoked via `invoke("command_name", args)` from `@tauri-apps/api/core`.

**Important:** Tauri v2 auto-camelCases top-level invoke keys. JS sends `snake_case`, Rust receives `camelCase`. See §4.3 for details.

| Command | JS Invoke | Rust Handler | Purpose |
|---------|-----------|--------------|---------|
| `get_programmer_info` | `{ }` | `get_programmer_info(state)` | Detect connected programmer |
| `force_reconnect` | `{ }` | `force_reconnect(state)` | Reclaim stale USB handle |
| `search_devices` | `{ query }` | `search_devices(query, state)` | Search IC database |
| `get_device_info` | `{ name }` | `get_device_info(name, state)` | Get device details (no programmer) |
| `select_device` | `{ name }` | `select_device(name, state)` | Select and resolve device |
| `deselect_device` | `{ }` | `deselect_device(state)` | Clear selected device |
| `do_read` | `{ path, options }` | `do_read(path, options, ...)` | Read chip to file |
| `read_chip_to_bytes` | `{ options }` | `read_chip_to_bytes(options, ...)` | Read chip to memory (hex viewer) |
| `do_write` | `{ path, options }` | `do_write(path, options, ...)` | Write file to chip |
| `do_verify` | `{ path, options }` | `do_verify(path, options, ...)` | Verify chip against file |
| `do_erase` | `{ icspMode }` | `do_erase(icspMode, state)` | Erase chip |
| `do_blank_check` | `{ icspMode }` | `do_blank_check(icspMode, ...)` | Check if chip is blank |
| `do_chip_id` | `{ icspMode }` | `do_chip_id(icspMode, state)` | Read and compare chip ID |
| `do_logic_test` | `{ icspMode }` | `do_logic_test(icspMode, ...)` | Test logic IC |
| `read_fuses` | `{ icspMode }` | `read_fuses(icspMode, state)` | Read fuses/locks/config |
| `write_fuses` | `{ cfgFuses, lockBits, icspMode }` | `write_fuses(cfgFuses, lockBits, icspMode, ...)` | Write fuses/locks |
| `check_lock_protection` | `{ icspMode }` | `check_lock_protection(icspMode, ...)` | Check if chip is locked |
| `check_overcurrent` | `{ }` | `check_overcurrent(state)` | Check OVC status |
| `run_hardware_check` | `{ }` | `run_hardware_check(state)` | Programmer self-test |
| `read_calibration` | `{ }` | `read_calibration(state)` | Read calibration bytes |
| `save_bytes_to_file` | `{ path, base64Data }` | `save_bytes_to_file(path, base64Data)` | Save hex data to disk |
| `open_folder` | `{ path }` | `open_folder(path)` | Open folder in Explorer |

### 3.2 OperationOptions (nested object)

Passed as `options` in `do_read`, `do_write`, `do_verify`, `read_chip_to_bytes`.

```ts
interface OperationOptions {
  skip_erase: boolean;      // Skip erase before write
  skip_verify: boolean;      // Skip verify after write
  icsp_mode: "zif" | "icsp" | "icsp_no_vcc";
  page: string;             // "code", "data", "config", etc.
  format: string;           // "auto", "bin", "ihex", "srec", "jedec"
  size_mismatch: string;    // "error", "warn", "ignore"
}
```

**Note:** `size_mismatch` is distinct from `icsp_mode` in the Rust struct but both arrive in the same `OperationOptions` object. The Rust `size_mismatch` field is snake_case because serde deserializes nested objects directly without Tauri's key mapping.

---

## 4. Key Conventions

### 4.1 Svelte 5 Runes (Mandatory)

Use `$state`, `$derived`, `$effect` exclusively. Do not mix with legacy `$:` reactive syntax.

```svelte
<script>
  // GOOD
  let count = $state(0);
  let doubled = $derived(count * 2);
  $effect(() => {
    console.log(`count is now ${count}`);
  });

  // BAD — legacy syntax, do not use
  $: doubled_legacy = count * 2;
</script>
```

### 4.2 Store Patterns

All reactive state that components read must live in a writable store. Module-level variables are invisible to Svelte reactivity.

```ts
// BAD
let _hexData: Uint8Array | null = null;  // Components can't see changes

// GOOD
export const hexMeta = writable<HexMeta | null>(null);  // Reactive
```

Read store data directly in templates:

```svelte
<!-- GOOD — $hexMeta.data is reactive -->
{#each $hexMeta.data.slice(0, 16) as b}
  <span>{b.toString(16).padStart(2, '0')}</span>
{/each}
```

### 4.3 Tauri v2 Command Parameter Naming

Tauri v2 automatically converts top-level invoke keys from `snake_case` to `camelCase` before matching them to Rust function parameter names.

**Rule:** Rust handler parameter names must match the camelCase version of the JS keys.

```ts
// JavaScript — sends snake_case keys
await invoke("write_fuses", {
  cfg_fuses: cfg,     // Tauri converts to cfgFuses
  lock_bits: lock,    // Tauri converts to lockBits
  icsp_mode: mode,    // Tauri converts to icspMode
});
```

```rust
// Rust — parameter names must match camelCase
#[tauri::command]
pub async fn write_fuses(
    cfgFuses: Vec<FuseValueDto>,    // matches "cfgFuses" from Tauri
    lockBits: Vec<FuseValueDto>,     // matches "lockBits" from Tauri
    icspMode: String,               // matches "icspMode" from Tauri
) { ... }
```

**Exception:** Nested objects (like `options: OperationOptions`) are serialized directly by serde and are **not** affected by Tauri's camelCase mapping. The JS key `size_mismatch` inside the `options` object is preserved as `size_mismatch` in Rust.

---

## 5. Data Flow

### 5.1 Chip Read → Hex Viewer

```
User clicks "Read" in App.svelte
  ↓
doReadToBuffer(getOptions()) in operations.ts
  ↓
invoke("read_chip_to_bytes", { options }) → Rust
  ↓
read_chip_to_bytes() in commands.rs
  ↓
read_chip() in minipro-core operations.rs (writes to temp file)
  ↓
Base64-encode temp file → return { base64, stats }
  ↓
operations.ts: base64ToUint8Array(result.base64)
  ↓
setHexData(bytes, null) in hex.ts
  ↓
hexMeta store updates → HexViewer.svelte re-renders
```

### 5.2 File Write → Chip

```
User clicks "Write" → pick file → App.svelte
  ↓
doWrite(path, getOptions()) in operations.ts
  ↓
invoke("do_write", { path, options }) → Rust
  ↓
do_write() in commands.rs
  ↓
If !skip_erase: erase_chip() → begin_transaction(device) → write_chip()
  ↓
If !skip_verify: verify_chip()
  ↓
Return result → operations.ts → runOp() logs "Verify passed"
```

### 5.3 Config Read → Fuse Editor

```
User clicks "Config" → readFuses(icspMode) in operations.ts
  ↓
invoke("read_fuses", { icspMode }) → Rust
  ↓
read_fuses() in commands.rs
  ↓
minipro_core::operations::read_fuses(handle)
  ↓
Protocol::read_fuses() for each fuse type (CFG, LOCK, etc.)
  ↓
Return ConfigDataDto { cfg_fuses, lock_bits, user_fuses, calibration }
  ↓
App.svelte: configData = result
  ↓
Svelte re-renders fuse checkboxes with isFuseProgrammed() logic
```

### 5.4 Config Write

```
User changes hex input or toggles checkbox → setCfgValue(index, value)
  ↓
App.svelte local state updates (configData is $state)
  ↓
User clicks "Write Config to Chip"
  ↓
writeFuses(configData.cfg_fuses, configData.lock_bits, icspMode)
  ↓
invoke("write_fuses", { cfgFuses: cfg, lockBits: lock, icspMode }) → Rust
  ↓
write_fuses() in commands.rs
  ↓
minipro_core::operations::write_fuses(handle, all_fuses)
  ↓
Protocol::write_fuses() for each fuse type
```

---

## 6. AVR Fuse Bit Convention

For AVR-family devices (ATtiny, ATmega, etc.), the `invert_fuse_bits` flag is set to `true`. This means:

- **Bit = 0** → fuse is **programmed** (active)
- **Bit = 1** → fuse is **unprogrammed** (inactive)
- **Checkbox checked** → fuse is programmed → bit is 0
- **Checkbox unchecked** → fuse is unprogrammed → bit is 1

This matches the convention used by AVR tools like avrdude and XGPro.

The toggle logic in `toggleFuseValue()` handles this inversion:
- For invert=true: toggling clears the bit (programmed → unprogrammed = bit goes 0 → 1)
- For invert=false: toggling sets the bit (programmed = bit goes 0 → 1)

---

## 7. Package Variant Handling

Device names in the XGPro database often include package variants: `ATTINY85V@DIP8`, `ATMEGA328P@TQFP`, etc.

**Problem:** These variants frequently have:
- Incorrect `protocol_id` values
- Wrong pin mappings
- Copied `chip_id` from the base device that doesn't match the variant's protocol

**Solution:**
1. **Frontend warning:** When a variant is selected for Read/Write/Verify, a `[WARN]` message suggests selecting the base device name
2. **Chip ID comparison:** For variants, comparison is skipped and a contextual message explains the mismatch
3. **Fuse operations:** Config reads/writes work correctly regardless of variant name

The `base_name` is extracted by splitting the device name at `@`:
```rust
let base_name = device.name.split('@').next().unwrap().to_string();
```

---

## 8. Build & Release

### Development

```bash
cd gui
npm install
cargo tauri dev    # hot-reload for both frontend and backend
```

### Production Build

```bash
cd gui
cargo tauri build   # always use this when any .svelte, .ts, .css, or .html changed
```

**Critical:** `cargo build --release` without `cargo tauri build` will embed stale frontend assets from the previous full build. The embedded `dist/` is only refreshed by `cargo tauri build`.

### Fast Backend-Only Build

Use only when you have changed **only Rust code** and no frontend files:

```bash
cd gui && npm run build && cargo build --release
```

### Output Location

```
gui/src-tauri/target/release/minipro-gui.exe
```

---

## 9. File Organization

```
minipro-rs/
├── AGENTS.md              ← Developer guide (this project's conventions)
├── spec.md                ← This document
├── ROADMAP.md             ← Feature planning
├── CHANGELOG.md           ← Release notes
├── README.md              ← User-facing documentation
│
├── crates/
│   └── minipro-core/      ← Core library (no GUI)
│       ├── src/
│       │   ├── operations.rs    ← read_chip, write_chip, verify_chip, etc.
│       │   ├── protocol/        ← TL866A, TL866II+, T56, T76
│       │   ├── device.rs        ← Device, ChipConfig, FuseField
│       │   ├── usb.rs           ← USB bulk transfers
│       │   └── format/          ← bin, ihex, srec, jedec parsers
│       └── Cargo.toml
│
└── gui/                   ← Tauri desktop application
    ├── src/
    │   ├── App.svelte     ← Main layout, operations, splitters
    │   └── lib/
    │       ├── stores/
    │       │   ├── operations.ts    ← invoke wrappers, OperationOptions
    │       │   ├── device.ts          ← programmer, selectedDevice, search
    │       │   ├── hex.ts             ← hex data, loading state
    │       │   ├── logs.ts            ← terminal log entries
    │       │   └── settings.ts        ← persisted preferences
    │       ├── components/
    │       │   ├── HexViewer.svelte   ← hex dump rendering
    │       │   ├── TerminalLog.svelte ← scrollable log panel
    │       │   ├── DeviceSelector.svelte
    │       │   ├── DiagnosticsPanel.svelte
    │       │   ├── SettingsPanel.svelte
    │       │   └── ProgressPanel.svelte
    │       └── file-dialog.ts       ← Tauri dialog wrappers
    │
    └── src-tauri/
        ├── src/
        │   ├── commands.rs  ← All #[tauri::command] handlers
        │   ├── lib.rs       ← Tauri builder, plugin init
        │   └── state.rs     ← AppState (USB handle, selected device)
        ├── Cargo.toml
        └── tauri.conf.json
```

---

## 10. Progress Events

Rust commands emit progress events via Tauri's event system:

```rust
window.emit("progress", ProgressPayload {
    done: bytes_done,
    total: total_bytes,
    operation: "read".to_string(),
})?;
```

Frontend listens in `operations.ts`:

```ts
const unlisten = await listen("progress", (event) => {
  progress.set(event.payload);
});
```

The `ProgressPanel.svelte` component subscribes to the `progress` store and renders a progress bar.

---

## 11. Error Handling

### Frontend

All `invoke()` calls are wrapped in `try/catch` with terminal logging:

```ts
export async function doReadToBuffer(options: OperationOptions) {
  return await runOp("Read", async () => {
    const result = await invoke("read_chip_to_bytes", { options });
    // ...
  });
}
```

`runOp()` in `operations.ts`:
- Sets `isRunning` and `currentOperation` stores
- Calls the operation function
- On success: logs completion with stats
- On error: logs `[ERROR] Operation failed: {message}`
- Finally: clears running state

### Backend

Rust commands return `Result<T, String>` where the error is a user-facing message string. Internal errors are converted:

```rust
read_chip(...).map_err(|e| e.to_string())?
```

This produces messages like:
- `"file size 1936 does not match device size 8192. Set Size Diff to 'Warn' or 'Ignore' to proceed."`
- `"Protocol error: no device selected"`
- `"Chip is not blank at 0x00000420"`

---

## 12. State Management

### AppState (Rust)

```rust
pub struct AppState {
    pub programmer_info: Mutex<Option<ProgrammerInfo>>,
    pub selected_device: Mutex<Option<Device>>,
    pub usb_handle: Mutex<Option<MiniproHandle>>,
    pub running: AtomicBool,
}
```

- `programmer_info`: Cached programmer model/firmware/serial
- `selected_device`: Currently selected IC from database
- `usb_handle`: Active USB connection (must be `take()`/`store()` pattern for cross-thread transfer)
- `running`: Atomic flag to prevent concurrent operations

### take_handle() / store_handle() Pattern

Because `MiniproHandle` is not `Clone`, it must be moved between threads:

```rust
let mut handle = state.take_handle()?;  // Removes handle from state
// ... use handle ...
let _ = state.store_handle(handle);      // Returns handle to state
```

This prevents two operations from using the same USB handle simultaneously.

---

*Last updated: 2026-06-10*
