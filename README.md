# minipro-rs

A Rust reimplementation of [minipro](https://gitlab.com/DavidGriffith/minipro) — an open-source program for controlling XGecu's TL866xx/T48/T56/T76 series of chip programmers.

> **Status:** Phase 4 complete — all protocols + logic-IC testing + firmware update + ZIF pin control + SPI autodetect implemented, `cargo check` clean

---

## Goals

- Full feature parity with the C `minipro` 0.7.x
- **Native Windows 11 support** — no Cygwin, no MSYS2, no WSL, no `libusb` DLL; builds and runs with only `rustup` + `cargo`
- Cross-platform (Linux, macOS, Windows) without requiring a separately installed `libusb`
- Idiomatic Rust: strong types, `Result`-based error handling, no unsafe except at the USB boundary
- Library (`minipro-core`) + binary (`minipro-cli`) split so third-party GUIs (including [Tauri](#gui-front-ends-with-tauri)) can embed the core

---

## Windows support

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
```
minipro-rs [OPTIONS] -p <DEVICE>

Options:
  -r, --read <FILE>           Read chip to file
  -w, --write <FILE>          Write file to chip
  -e, --erase                 Erase chip
  -b, --blank-check           Blank-check chip
  -v, --verify <FILE>         Verify chip against file
  -I, --id-check-only         Read and display chip ID, then exit
      --no-erase              Skip erase before write
      --no-verify             Skip verify after write
      --protect-off           Disable write-protect before operation
      --protect-on            Enable write-protect after operation
      --format <FORMAT>       File format: auto|bin|ihex|srec [default: auto]
      --page <PAGE>           Memory page: code|data|config|user|calibration
      --icsp                  Use ICSP (in-circuit) programming
      --spi-clock <HZ>        Override SPI clock for 25C devices
      --i2c-addr <ADDR>       I2C slave address for 24C devices
      --skip-id               Skip chip-ID verification
      --continue-id           Continue even if chip-ID mismatch
      --logic-test            Test logic IC
  -l, --list                  List supported devices
  -V, --version               Print version and programmer info
  -v, --verbose               Verbose output
  -h, --help                  Print help
```

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
| `indicatif` | Progress bars for read/write |
| `crc` | CRC-32 for verify operations |

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
- [ ] Integration tests (recorded USB traces for replay)
- [ ] `cargo doc` public API documentation
- [ ] Packaging: `.deb`, `.rpm`, Windows `.msi` via GitHub/GitLab CI
- [ ] Bash / Zsh / Fish shell completions via `clap_complete`

---

## Reference

- Upstream C project: <https://gitlab.com/DavidGriffith/minipro>
- USB protocol documentation: `tl866iiplus.md` in upstream repo
- [`nusb`](https://crates.io/crates/nusb) — pure-Rust cross-platform USB (replaces libusb)
- [`quick-xml`](https://crates.io/crates/quick-xml) — fast XML chip database parsing
- [Zadig](https://zadig.akeo.ie/) — one-time WinUSB driver association tool for Windows
- [Tauri](https://tauri.app/) — recommended framework for GUI front-ends

## Description
Let people know what your project can do specifically. Provide context and add a link to any reference visitors might be unfamiliar with. A list of Features or a Background subsection can also be added here. If there are alternatives to your project, this is a good place to list differentiating factors.

## Badges
On some READMEs, you may see small images that convey metadata, such as whether or not all the tests are passing for the project. You can use Shields to add some to your README. Many services also have instructions for adding a badge.

## Visuals
Depending on what you are making, it can be a good idea to include screenshots or even a video (you'll frequently see GIFs rather than actual videos). Tools like ttygif can help, but check out Asciinema for a more sophisticated method.

## Installation
Within a particular ecosystem, there may be a common way of installing things, such as using Yarn, NuGet, or Homebrew. However, consider the possibility that whoever is reading your README is a novice and would like more guidance. Listing specific steps helps remove ambiguity and gets people to using your project as quickly as possible. If it only runs in a specific context like a particular programming language version or operating system or has dependencies that have to be installed manually, also add a Requirements subsection.

## Usage
Use examples liberally, and show the expected output if you can. It's helpful to have inline the smallest example of usage that you can demonstrate, while providing links to more sophisticated examples if they are too long to reasonably include in the README.

## Support
Tell people where they can go to for help. It can be any combination of an issue tracker, a chat room, an email address, etc.

## Roadmap
If you have ideas for releases in the future, it is a good idea to list them in the README.

## Contributing
State if you are open to contributions and what your requirements are for accepting them.

For people who want to make changes to your project, it's helpful to have some documentation on how to get started. Perhaps there is a script that they should run or some environment variables that they need to set. Make these steps explicit. These instructions could also be useful to your future self.

You can also document commands to lint the code or run tests. These steps help to ensure high code quality and reduce the likelihood that the changes inadvertently break something. Having instructions for running tests is especially helpful if it requires external setup, such as starting a Selenium server for testing in a browser.

## Authors and acknowledgment
Show your appreciation to those who have contributed to the project.

## License
For open source projects, say how it is licensed.

## Project status
If you have run out of energy or time for your project, put a note at the top of the README saying that development has slowed down or stopped completely. Someone may choose to fork your project or volunteer to step in as a maintainer or owner, allowing your project to keep going. You can also make an explicit request for maintainers.
