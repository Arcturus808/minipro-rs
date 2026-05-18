# minipro-rs

A Rust reimplementation of [minipro](https://gitlab.com/DavidGriffith/minipro) — an open-source program for controlling XGecu's TL866xx/T48/T56/T76 series of chip programmers.

> **Status:** CI/CD pipeline fully operational — clippy, fmt, tests, Linux/Windows release builds, and all package jobs (`.deb`, `.rpm`, `.msi`, shell completions) passing on every commit. macOS builds are community-supported (see [macOS support](#macos-support)).

---

## Goals

- Full feature parity with the C `minipro` 0.7.x
- **Native Windows 11 support** — no Cygwin, no MSYS2, no WSL, no `libusb` DLL; builds and runs with only `rustup` + `cargo`
- Cross-platform (Linux, macOS, Windows) without requiring a separately installed `libusb`
- Idiomatic Rust: strong types, `Result`-based error handling, no unsafe except at the USB boundary
- Library (`minipro-core`) + binary (`minipro-cli`) split so third-party GUIs (including [Tauri](#gui-front-ends-with-tauri)) can embed the core

---

## Windows support

> **Note on antivirus false positives:** Some AV vendors (notably Bkav Pro) heuristically flag this binary because Rust's standard library links `ws2_32.dll` (Windows Sockets) on all Windows targets, even when the program never uses networking. This is a known characteristic of every Rust Windows binary, not malware. You can verify the binary integrity via the [VirusTotal scan linked in each release](https://gitlab.com/arcturus8081/minipro-rs/-/releases) or build from source yourself. A code-signing certificate is on the roadmap.

### For end users

The distributed binary is a **single self-contained `.exe`** with no installation required beyond placing it on `PATH`.  No Cygwin, no MSYS2, no WSL, no Visual C++ Redistributable, no `libusb-1.0.dll`.

**One-time USB driver step** — Windows associates USB devices with a driver that persists across reboots.  The programmer's interface must be associated with Microsoft's built-in **WinUSB** driver before first use:

1. Download and run [Zadig](https://zadig.akeo.ie/) (free, no install needed).
2. Select the XGecu programmer from the device list.
3. Choose **WinUSB** and click **Install Driver**.

This is a one-time step per machine.  It is the same requirement the original C `minipro` has on Windows — this project does not add any new hurdle; it only removes the `libusb` runtime layer that sat on top.

### For developers

| What you need | What you do NOT need |
|---|---|
| [rustup](https://rustup.rs/) (Rust toolchain) | Cygwin / MSYS2 / WSL |
| `cargo build --release` | C compiler (gcc / clang / MSVC) |
| Zadig (one-time, per machine) | `libusb`, `pkg-config`, or any C library |

The USB layer uses [`nusb`](https://crates.io/crates/nusb) — a pure-Rust library that calls the Windows WinUSB API directly through Rust's `windows-sys` bindings.  There is no C FFI, no `.dll` to bundle, and no system-level package manager step.

```powershell
# Clone and build on Windows — this is all that is required
git clone https://gitlab.com/your-fork/minipro-rs
cd minipro-rs
cargo build --release
# Binary is at target\release\minipro.exe
```

---

## macOS support

macOS binaries are **not built in CI** — GitLab's free tier does not include macOS runners, and the maintainer does not have access to Apple hardware.

**Mac users:** build from source with:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
git clone https://gitlab.com/arcturus8081/minipro-rs
cd minipro-rs
cargo build --release
# Binary is at target/release/minipro
```

Community contributors with Mac hardware are warmly welcomed — especially for testing USB device access, which requires physical hardware. If you run into a macOS-specific bug, please open an issue.

---

## GUI front-ends with Tauri

[Tauri](https://tauri.app/) is a well-suited choice for building a native GUI front-end for this project.  Because `minipro-core` is a plain Rust library crate, a Tauri app can depend on it directly:

```toml
# In your Tauri app's src-tauri/Cargo.toml
[dependencies]
minipro-core = { path = "../minipro-rs/crates/minipro-core" }
```

Then expose operations as Tauri commands:

```rust
#[tauri::command]
async fn read_chip(device: String, output: String) -> Result<(), String> {
    // Tauri commands run on a thread pool, so blocking USB I/O is safe here.
    tokio::task::spawn_blocking(move || {
        let mut handle = minipro_core::MiniproHandle::open()
            .map_err(|e| e.to_string())?;
        // ... begin_transaction, read_chip, etc.
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}
```

**Integration notes:**
- `minipro-core` uses `pollster` for blocking USB calls.  Wrap them in `tokio::task::spawn_blocking` (as above) so they don't block Tauri's async executor.
- `read_chip`, `write_chip`, and `verify_chip` accept an `Option<&mut dyn FnMut(usize, usize)>` progress callback invoked with `(bytes_done, total_bytes)` after each block.  Wire it to `window.emit("progress", …)` to drive a front-end progress bar — `minipro-core` has no terminal UI dependency.
- `read_chip` and `write_chip` return `Result<OpStats>` with `bytes` and `crc32` fields.  Use these to update a status label without parsing CLI output.
- The WinUSB driver requirement is identical on Windows — no extra setup for a Tauri deployment.
- The same `minipro-core` crate works on Linux and macOS, so the Tauri app is automatically cross-platform.
- A Tauri app ships as a native installer (`.msi` on Windows, `.dmg` on macOS, `.deb`/`.AppImage` on Linux) and still requires no `libusb` or Cygwin.

---

## Architecture Plan

### Crate layout

```
minipro-rs/
├── Cargo.toml            # workspace root
├── crates/
│   ├── minipro-core/     # library: USB, protocol, database, file formats
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── error.rs
│   │   │   ├── device.rs       # Device / chip descriptor types
│   │   │   ├── database.rs     # XML chip database (infoic.xml / logicic.xml)
│   │   │   ├── handle.rs       # MiniproHandle — open/close, dispatch
│   │   │   ├── usb.rs          # nusb abstraction layer
│   │   │   ├── protocol/
│   │   │   │   ├── mod.rs      # Protocol trait
│   │   │   │   ├── tl866a.rs
│   │   │   │   ├── tl866iiplus.rs
│   │   │   │   ├── t48.rs
│   │   │   │   ├── t56.rs      # FPGA bitstream upload
│   │   │   │   └── t76.rs
│   │   │   ├── format/
│   │   │   │   ├── mod.rs      # FileFormat enum, auto-detect
│   │   │   │   ├── ihex.rs     # Intel HEX read/write
│   │   │   │   ├── srec.rs     # Motorola S-Record read/write
│   │   │   │   └── jedec.rs    # JEDEC fuse-map read/write
│   │   │   └── operations.rs   # High-level read/write/erase/verify/blank-check
│   │   └── Cargo.toml
│   └── minipro-cli/      # binary: clap CLI front-end
│       ├── src/
│       │   └── main.rs
│       └── Cargo.toml
├── data/
│   ├── infoic.xml        # 13 000+ device definitions (vendored from upstream)
│   └── logicic.xml       # Logic IC test vectors
└── tests/
    └── integration/
```

---

### Key design decisions

#### USB layer — `nusb`
Use [`nusb`](https://crates.io/crates/nusb) (pure Rust, no libusb dependency) instead of `rusb`.  
This enables a single binary on Windows without requiring DLL distribution.

USB VID/PID targets:
| Model        | VID    | PID    |
|--------------|--------|--------|
| TL866CS/A    | 0x04D8 | 0xE11C |
| TL866II+     | 0x04D8 | 0xE11C |
| T48          | 0x04D8 | 0xE11C |
| T56          | 0xA466 | 0x0A53 |
| T76          | (TBD)  |        |

#### Protocol abstraction — `trait Protocol`
Replace the C function-pointer dispatch table in `minipro_handle_t` with a Rust trait:

```rust
pub trait Protocol: Send + Sync {
    fn begin_transaction(&self, handle: &UsbHandle, device: &Device) -> Result<()>;
    fn end_transaction(&self, handle: &UsbHandle) -> Result<()>;
    fn read_block(&self, handle: &UsbHandle, ds: &mut DataSet) -> Result<()>;
    fn write_block(&self, handle: &UsbHandle, ds: &DataSet) -> Result<()>;
    fn get_chip_id(&self, handle: &UsbHandle) -> Result<(IdType, u32)>;
    fn read_fuses(&self, handle: &UsbHandle, fuse_type: FuseType, count: usize) -> Result<Vec<u8>>;
    fn write_fuses(&self, handle: &UsbHandle, fuse_type: FuseType, data: &[u8]) -> Result<()>;
    fn erase(&self, handle: &UsbHandle, num_fuses: u8, is_pld: bool) -> Result<()>;
    fn protect_off(&self, handle: &UsbHandle) -> Result<()>;
    fn protect_on(&self, handle: &UsbHandle) -> Result<()>;
    fn get_ovc_status(&self, handle: &UsbHandle) -> Result<OvcStatus>;
    fn hardware_check(&self, handle: &UsbHandle) -> Result<()>;
    fn firmware_update(&self, handle: &UsbHandle, firmware: &[u8]) -> Result<()>;
    fn logic_ic_test(&self, handle: &UsbHandle) -> Result<()>;
    // ... etc.
}
```

`MiniproHandle` holds a `Box<dyn Protocol>` selected at open time.

#### Device database — `database.rs`
Parse `infoic.xml` and `logicic.xml` at startup with [`quick-xml`](https://crates.io/crates/quick-xml).  
Store as `HashMap<String, Arc<Device>>` for O(1) name lookup.

Strong-typed enums replace C `#define` constants:
```rust
pub enum ChipType { Memory, Mcu, Pld, Sram, Logic, Nand, Emmc, Vga }
pub enum DataOrg  { Bytes, Words, Bits }
pub enum FuseType { User, Config, Lock }
pub enum Endianness { Little, Big }
```

The full `Device` struct mirrors `device_t` but uses `Option<T>` for optional fields and avoids raw pointers.

#### Error handling — `thiserror`
```rust
#[derive(Debug, thiserror::Error)]
pub enum MiniproError {
    #[error("USB error: {0}")]          Usb(#[from] nusb::Error),
    #[error("Device not found: {0}")]   DeviceNotFound(String),
    #[error("Chip ID mismatch (expected {expected:#010x}, got {actual:#010x})")]
                                        ChipIdMismatch { expected: u32, actual: u32 },
    #[error("Overcurrent detected at address {address:#010x}")]
                                        Overcurrent { address: u32 },
    #[error("Verify failed at address {address:#010x}: expected {expected:#04x}, got {actual:#04x}")]
                                        VerifyFailed { address: u32, expected: u8, actual: u8 },
    #[error("XML parse error: {0}")]    Xml(#[from] quick_xml::Error),
    #[error("IO error: {0}")]           Io(#[from] std::io::Error),
}
```

#### CLI — `clap` derive

See `minipro --help` or the [manual page](minipro.1.html) for the current authoritative option list.
The implementation lives in `crates/minipro-cli/src/cli.rs`.

---

### Dependency plan

| Crate | Purpose |
|-------|---------|
| `nusb` | Cross-platform USB (pure Rust) |
| `clap` (derive) | CLI argument parsing |
| `quick-xml` | XML chip database parsing |
| `thiserror` | Structured error types |
| `anyhow` | CLI error propagation |
| `ihex` | Intel HEX read/write |
| `log` + `env_logger` | Logging / verbose output |
| `indicatif` | Progress bars for read/write/verify |
| `crc` | CRC-32/ISO-HDLC checksum for `OpStats` (read/write summary) |

---

### Implementation phases

#### Phase 1 — Foundation
- [x] Cargo workspace scaffold
- [x] `error.rs` — full error type hierarchy
- [x] `device.rs` — all device/chip structs and enums
- [x] `database.rs` — parse `infoic.xml` and `logicic.xml`
- [x] `usb.rs` — open device, send/receive bulk transfers
- [x] `handle.rs` — `MiniproHandle::open()`, model detection, firmware version

#### Phase 2 — TL866II+ protocol (primary target)
- [x] `protocol/tl866iiplus.rs` — all protocol commands
- [x] `operations.rs` — read, write, erase, verify, blank-check
- [x] `format/ihex.rs`, `format/srec.rs`, `format/jedec.rs`
- [x] Basic CLI wired up end-to-end

#### Phase 3 — Remaining protocols
- [x] `protocol/tl866a.rs` (TL866CS / TL866A)
- [x] `protocol/t48.rs`
- [x] `protocol/t56.rs` (FPGA bitstream upload)
- [x] `protocol/t76.rs` (chunked bitstream, DMA streaming code memory)

#### Phase 4 — Advanced features
- [x] Logic IC testing (`logicic.xml` vectors)
- [x] Fuse / configuration-bit read/write
- [x] JEDEC fuse-map support
- [x] Chip ID verify + autodetect
- [x] Overcurrent protection handling
- [x] Firmware update (TL866II+/T48 `UpdateII.dat`, T76 `updateT76.dat`)
- [x] Bitbang / ZIF pin control
- [x] SPI autodetect

#### Phase 5 — Quality
- [x] Integration tests (JSON fixture replay — no hardware required)
- [x] `cargo doc` public API documentation (module-level + type-level doc comments)
- [x] Shell completions: bash, zsh, fish, PowerShell via `clap_complete`
  - Generated at build time into `$OUT_DIR/completions/`
  - Install at runtime:
    - **bash** (Linux/macOS): `minipro --generate-completions bash | sudo tee /etc/bash_completion.d/minipro`
    - **zsh**: `minipro --generate-completions zsh > ~/.zfunc/_minipro`
    - **fish**: `minipro --generate-completions fish > ~/.config/fish/completions/minipro.fish`
    - **PowerShell** (Windows): `minipro --generate-completions powershell >> $PROFILE`
- [x] Packaging via GitLab CI (`.gitlab-ci.yml`):
  - Linux `.deb` (Debian/Ubuntu) and `.rpm` (Fedora/RHEL)
  - Windows `.msi` via `cargo-wix` (`wix/main.wxs`)
  - Cross-compile Linux→Windows with `x86_64-pc-windows-gnu`
  - Tag-triggered release stage auto-attaches artifacts to GitLab releases

---

## Reference

- Upstream C project: <https://gitlab.com/DavidGriffith/minipro>
- USB protocol documentation: `tl866iiplus.md` in upstream repo
- [`nusb`](https://crates.io/crates/nusb) — pure-Rust cross-platform USB (replaces libusb)
- [`quick-xml`](https://crates.io/crates/quick-xml) — fast XML chip database parsing
- [Zadig](https://zadig.akeo.ie/) — one-time WinUSB driver association tool for Windows
- [Tauri](https://tauri.app/) — recommended framework for GUI front-ends

## Installation

Pre-built binaries are attached to each [GitLab release](https://gitlab.com/arcturus8081/minipro-rs/-/releases).

### Windows

Download `minipro-<version>-x86_64-windows.zip` (or the `.msi` installer), extract, and place `minipro.exe` on your `PATH`.  See [Windows support](#windows-support) above for the one-time Zadig/WinUSB driver step.

### Linux

```sh
# Debian / Ubuntu
sudo dpkg -i minipro_<version>_amd64.deb

# RPM-based (Fedora / RHEL)
sudo rpm -i minipro-<version>.x86_64.rpm
```

Both packages place `infoic.xml` and `logicic.xml` in `/usr/share/minipro/` automatically.

### Build from source

```sh
cargo install --path crates/minipro-cli
```

The database files must be available alongside the binary or in one of the search paths described in the [manual page](minipro.1.html).

---

## Usage

See `minipro --help` or the [manual page](minipro.1.html) for the full option reference.

```sh
# Read an ATmega48 to a file
minipro -p ATMEGA48 -r atmega48.bin

# Write a file to an ATmega48
minipro -p ATMEGA48 -w atmega48.bin

# List all devices matching a name substring
minipro -l W25Q

# Show connected programmer info
minipro -I

# Test a logic IC
minipro -p 7404 --logic-test
```

---

## Contributing

Bug reports, hardware test results, and pull requests are welcome.  Please open an issue before writing large changes.

```sh
cargo test          # runs all tests (no hardware required)
cargo clippy -- -D warnings
cargo fmt --check
```

---

## License

GNU General Public License version 3 or later — <https://www.gnu.org/licenses/gpl-3.0.en.html>.

## Authors and acknowledgment

`minipro` was created by Valentin Dudouyt in 2014; ongoing development of the original C project is coordinated by David Griffith.  `minipro-rs` is a Rust reimplementation by the minipro-rs contributors.
