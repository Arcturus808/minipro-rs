# minipro-gui

A desktop GUI for [minipro-rs](https://gitlab.com/arcturus8081/minipro-rs) built with [Tauri v2](https://tauri.app/) and [Svelte 5](https://svelte.dev/).

## Features

| Feature | Status | Description |
|---------|--------|-------------|
| **Device Search** | Done | Search the 13,000+ device database with instant results |
| **Device Selection** | Done | Select a device to configure operations; shows chip type, pin count, voltages |
| **Read** | Done | Read chip contents to a file (bin/hex/srec/jedec) |
| **Write** | Done | Write a file to the chip with optional erase/verify skipping |
| **Verify** | Done | Compare chip contents against a file |
| **Erase** | Done | Erase the selected chip |
| **Blank Check** | Done | Verify the chip is blank |
| **Chip ID** | Done | Read and display the chip ID |
| **Hex Viewer** | Done | Inspect loaded files or chip dumps with virtualized scrolling |
| **File Dialogs** | Done | Native OS open/save dialogs with last-used-directory persistence |
| **Progress** | Done | Live progress bar with bytes-read/written and CRC32 |
| **Terminal Log** | Done | Timestamped info/warn/error log panel |
| **Settings** | Done | Persisted preferences (defaults, theme, device view mode) via `tauri-plugin-store` |
| **Diagnostics** | Done | Programmer details, overcurrent check, calibration read |
| **Firmware Update** | Planned | TL866A/CS `update.dat` decryption + flashing (algorithm known, pending implementation) |

## Screenshots

*(Screenshots to be added once UI stabilizes)*

## Tech Stack

| Layer | Technology |
|-------|------------|
| **Backend** | Rust — Tauri v2, `tokio`, `minipro-core` (path dependency) |
| **Frontend** | Svelte 5 (runes), TypeScript, Vite |
| **Styling** | Tailwind CSS v4, Skeleton v3 |
| **Persistence** | `tauri-plugin-store` |
| **Dialogs** | `tauri-plugin-dialog` |
| **Build** | `cargo tauri build` (produces `.msi` + `.exe` installer on Windows) |

## Project Structure

```
gui/
├── src/
│   ├── App.svelte                    # Root layout: header + 3-column main
│   ├── app.css                       # Tailwind/Skeleton imports + custom scrollbar
│   ├── lib/
│   │   ├── file-dialog.ts            # Native open/save wrappers with defaultPath
│   │   ├── components/
│   │   │   ├── DeviceSelector.svelte # Search + paginated/scrollable device list
│   │   │   ├── HexViewer.svelte      # Virtualized hex grid
│   │   │   ├── ProgressPanel.svelte  # Progress bar + stats
│   │   │   ├── TerminalLog.svelte    # Scrollable log output
│   │   │   ├── SettingsPanel.svelte  # Modal preferences panel
│   │   │   └── DiagnosticsPanel.svelte # Programmer info + diagnostic buttons
│   │   └── stores/
│   │       ├── device.ts             # programmer, selectedDevice, deviceList stores
│   │       ├── hex.ts                # hexBuffer store + loadFile helper
│   │       ├── logs.ts               # Terminal log store with timestamped levels
│   │       ├── operations.ts         # doRead/doWrite/... + progress event listener
│   │       ├── settings.ts           # Persisted settings via tauri-plugin-store
│   │       └── theme.ts              # System/dark/light theme toggle
│   └── main.ts                       # Svelte mount entry point
├── src-tauri/
│   ├── src/
│   │   ├── commands.rs               # All Tauri invoke handlers
│   │   ├── lib.rs                    # App setup + command registration
│   │   └── state.rs                  # Shared AppState (handle, device, db paths)
│   ├── capabilities/
│   │   └── default.json              # Tauri v2 capability manifest
│   └── Cargo.toml                    # Tauri deps + minipro-core path
├── index.html
├── package.json
├── vite.config.ts
└── tailwind.config.ts
```

## Build

### Prerequisites

- [Rust](https://rustup.rs/)
- [Node.js](https://nodejs.org/) (LTS)
- Windows: [Zadig](https://zadig.akeo.ie/) to install WinUSB driver (one-time)

### Development

```bash
cd gui
npm install              # once
cargo tauri dev          # hot-reload frontend + Rust backend
```

### Production Build

```bash
cd gui
cargo tauri build
```

Output:
- `src-tauri/target/release/bundle/msi/minipro-gui_0.1.0_x64_en-US.msi`
- `src-tauri/target/release/bundle/nsis/minipro-gui_0.1.0_x64-setup.exe`

### Chip Database

The GUI searches for `infoic.xml` and `logicic.xml` in the same locations as the CLI. For development, copy them next to the built `.exe`:

```powershell
Copy-Item ..\data\infoic.xml src-tauri\target\release\
Copy-Item ..\data\logicic.xml src-tauri\target\release\
```

## Architecture

### Frontend State Management

Svelte 5 runes (`$state`, `$derived`) are used for local component state. Cross-component state lives in module-level stores under `src/lib/stores/`:

- **`device.ts`** — `programmer` (connection info), `selectedDevice`, `deviceList`, `dbAvailable`
- **`operations.ts`** — `isRunning` flag, `doRead`/`doWrite`/... async action wrappers, `initProgressListener()` that listens for `"progress"` events from the backend
- **`settings.ts`** — `settings` object persisted via `tauri-plugin-store`, `initSettings()` loads on app startup
- **`logs.ts`** — `logs` array with `info()`/`warn()`/`error()` helpers; `TerminalLog.svelte` subscribes and auto-scrolls
- **`hex.ts`** — `hexBuffer` for the hex viewer; `loadFile()` reads a file via `invoke("read_file_bytes")`

### Backend Commands

All USB/programmer operations are exposed as async Tauri commands in `src-tauri/src/commands.rs`:

| Command | What It Does |
|---------|-------------|
| `get_programmer_info` | Detect connected programmer, return model/firmware/serial |
| `search_devices` | Query `infoic.xml` by name substring |
| `select_device` | Load full `Device` struct and begin USB transaction |
| `do_read` / `do_write` / `do_verify` | High-level chip operations with progress events |
| `do_erase` / `do_blank_check` / `do_chip_id` | Chip control operations |
| `check_overcurrent` | Read programmer OVC status registers |
| `read_calibration` | Read internal RC oscillator calibration bytes |
| `read_file_bytes` | Read a local file into bytes for the hex viewer |
| `check_database` | Verify `infoic.xml` / `logicic.xml` are locatable |

Commands that access USB use `tokio::task::spawn_blocking` to avoid blocking Tauri's async executor.

### Progress Events

Long-running operations emit `"progress"` events via `window.emit()`:

```rust
window.emit("progress", ProgressPayload { done, total, operation: "read".into() })?;
```

The frontend `operations.ts` listens and updates `progress` / `isRunning` stores, which `ProgressPanel.svelte` and operation buttons react to.

## Development Notes

### WebView2 Thread Sensitivity (Windows)

On Windows, Tauri uses the system's WebView2 (Edge) renderer. Complex reactive updates — especially rendering large lists with Svelte's `{#each}` — can freeze the UI thread. We encountered this during device search with 200+ results.

**Mitigations applied:**
- Paginated list rendering (12 items per page) with a "Paginate / Scroll" toggle
- Avoid `flex`/`card` wrapper classes in small panels (e.g., DiagnosticsPanel) — use minimal Tailwind utilities
- Keep reactive computations simple; offload heavy work to the backend

### Theme System

The app supports System / Dark / Light themes. Skeleton's `preset-filled-surface-100-900` classes adapt automatically. For custom modals that need explicit backgrounds, we use `$derived` from `$settings.theme` rather than Tailwind's `dark:` prefix, because `@custom-variant dark (&:is(.dark *))` is required in `app.css` for the `dark:` variant to work with Skeleton's theme class toggling.

### Settings Persistence

`tauri-plugin-store` writes to the app's data directory (e.g., `%APPDATA%/minipro-gui/settings.json` on Windows). Settings include:

- Default operation options (skip erase/verify, page, format, size mismatch)
- Theme preference
- Device list view mode (paginated vs scroll)
- Last-used directory for file dialogs

## Roadmap

- [x] Phase 1 — Core operations + progress + basic layout
- [x] Phase 2 — Hex viewer + file I/O + device search
- [x] Phase 3 — Diagnostics (overcurrent, calibration, programmer details)
- [ ] Phase 4 — Firmware update (TL866A/CS `update.dat` decryption)
- [ ] Phase 5 — Settings & config (completed; includes theme, defaults, last directory)

*(Phases 3 and 5 were completed out of order.)*

## License

Same as the parent project: see [../LICENSE](../LICENSE).
