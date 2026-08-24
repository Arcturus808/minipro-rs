# Contributing to minipro-rs

Thanks for your interest in contributing! This is a community-driven project and all contributions are welcome — bug reports, hardware testing, code, documentation, and feature ideas.

## Reporting Issues

Open an issue on [GitLab](https://gitlab.com/arcturus8081/minipro-rs/-/issues). The [GitHub repo](https://github.com/Arcturus808/minipro-rs) is a mirror for release builds only — issues and merge requests should go to GitLab.

**For bugs, please include:**
- Programmer model (TL866A, TL866II+, T48, T56, T76)
- Chip name and package type
- What command you ran or what GUI operation you tried
- What happened (exact error message or unexpected behavior)
- Whether the same operation works with XGPro or the C `minipro`
- OS and version

**For hardware testing reports**, see the testing priorities in [ROADMAP.md](ROADMAP.md).

## Development Setup

### Prerequisites

- **Rust 1.85+** (CLI) / **1.88+** (GUI) — install via [rustup](https://rustup.rs/)
- **Node.js 18+** and **npm** — for the GUI frontend
- A USB chip programmer from the supported list (TL866A/CS, TL866II+, T48, T56, T76) for hardware testing

### Build the CLI

```sh
git clone https://gitlab.com/arcturus8081/minipro-rs
cd minipro-rs
cargo build --release
# Binary: target/release/minipro (or target\release\minipro.exe on Windows)
```

### Build the GUI

The GUI uses Tauri v2 + Svelte 5. A full build embeds the frontend into the Rust binary:

```sh
cd gui
npm install
cargo tauri build
# Binary: gui/src-tauri/target/release/minipro-gui.exe
```

For faster GUI dev iteration (frontend-only HMR, no Rust backend):

```sh
cd gui
npm install
npm run dev
```

For full dev iteration with the Rust backend (slower, but Tauri commands work):

```sh
cd gui
npm install
npm run tauri dev
```

### Running Tests

```sh
cargo test --all
```

### Pre-commit Checks

CI runs these checks — run them locally before pushing to avoid red pipelines:

```sh
# CLI + core (run from repo root)
cargo fmt --all                          # auto-fix formatting
cargo fmt --all -- --check               # verify (must pass)
cargo clippy --all-targets -- -D warnings  # lint (must pass)
cargo test --all --locked                # tests with locked lockfile
cargo deny check                         # supply-chain audit

# GUI (run from gui/src-tauri/)
cargo clippy --all-targets --all-features -- -D warnings  # GUI lint (must pass)
```

If you changed any `.svelte`, `.ts`, `.css`, or `.html` file, also run:

```sh
cd gui && cargo tauri build   # embeds fresh frontend into the Rust binary
```

`cargo build --release` alone is only safe for Rust-only GUI changes (it reuses
stale embedded frontend assets). On Windows, stop any running `minipro-gui.exe`
before rebuilding — it locks the output binary.

## Code Style

### Rust
- Follow `rustfmt` defaults — no custom configuration
- `clippy` must pass with `-D warnings`
- Use `Result<T, MiniproError>` for all fallible operations — no `unwrap()` or `expect()` in library code
- Match existing patterns in the codebase; look at neighboring code before writing new code

### Frontend (Svelte / TypeScript)
- **Svelte 5 runes only** — use `$state`, `$derived`, `$effect`. Do NOT use legacy `$:` syntax
- All reactive state lives in writable stores (see `gui/src/lib/stores/`)
- See `AGENTS.md` for detailed GUI conventions and gotchas

### Commits
- Concise subject line, optional body explaining why (not what)
- One logical change per commit
- No AI attribution footers or "Co-Authored-By" bot lines

## Architecture Overview

```
minipro-rs/
├── crates/
│   ├── minipro-core/     # Library: USB, protocol, database, file formats
│   │   ├── src/
│   │   │   ├── protocol/ # Protocol trait + per-programmer implementations
│   │   │   ├── device.rs # Device descriptor types
│   │   │   ├── database.rs # XML chip database parser
│   │   │   ├── handle.rs # MiniproHandle — high-level dispatch
│   │   │   ├── operations.rs # read/write/erase/verify/blank-check/pin-test
│   │   │   ├── algorithm.rs # T56/T76 FPGA bitstream loading
│   │   │   └── format/  # Intel HEX, SREC, JEDEC file parsers
│   │   └── Cargo.toml
│   └── minipro-cli/      # CLI binary (clap)
├── gui/                  # Tauri v2 + Svelte 5 GUI
│   ├── src/              # Svelte frontend
│   └── src-tauri/        # Rust backend (Tauri commands)
├── data/                 # infoic.xml, logicic.xml (vendored)
└── tests/
```

The core library (`minipro-core`) is protocol-agnostic and can be embedded in other frontends. The CLI and GUI are thin wrappers around it.

## Protocol Implementation Notes

If you're working on programmer protocol code:

- **TL866A/CS, TL866II+, T48** — well-established, documented in the C `minipro` source. T48 aliases the TL866II+ protocol (`T48Protocol = Tl866iiPlusProtocol`) — they share the same USB command set
- **T56, T76** — FPGA-based, require `algorithm.xml` bitstream loading. See `crates/minipro-core/src/algorithm.rs`
- **Pin contact test** — supported on TL866II+ and T48 only. T76 is intentionally unsupported: its `0x3E` command is an adapter-init pin-driver configuration step, not a standalone contact test. The upstream C `minipro`'s `t76_pin_test` is broken (never reads the response, reports every pin as bad). The [xgecu-pro](https://github.com/jfabienke/xgecu-pro) project confirmed on real hardware that it "measured nothing and corrupted every read." TL866A/CS and T56 also lack the bit-banging command set used by the pin test
- The C `minipro` source ([GitLab](https://gitlab.com/DavidGriffith/minipro)) and [Matt Brown's t76-improvements branch](https://gitlab.com/nmatt0/minipro/-/tree/t76-improvements) are the primary references for protocol behavior
- When porting from C, match the exact byte-level protocol — do not "improve" the protocol flow

## What We Need Help With

- **Hardware testing** — especially T56 and T76 programmers. Most protocol code is implemented but untested on real hardware
- **macOS testing** — USB access, build verification, packaging
- **Database updates** — extracting newer chip databases from XGPro (see ROADMAP.md)
- **Bug reports** — any programmer model, any chip, any OS

## License

By contributing, you agree that your contributions will be licensed under the [GPL-3.0-or-later](LICENSE) license.
