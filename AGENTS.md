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
  assets. Also run `cd gui && npm run check` (svelte-check) — the
  frontend type gate; must report 0 errors.
- **Code changed? Verify locally before committing:**
  `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test --all --locked`. The fmt gate covers the **root workspace
  only** — `gui/src-tauri` is `exclude`d (own MSRV/lockfile), so when you
  touch it also run `cd gui/src-tauri && cargo fmt --check` (and
  `cargo clippy` there for good measure).
- **No local paths in published files** — never commit paths from the
  maintainer's machine (e.g. `D:/...`, `/home/<user>/...`) into docs,
  examples, or configs. Use generic placeholders (`C:/path/to/...`).
- **Keep docs & changelog in sync with the change:**
  - User-facing change (fix, feature, behavior)? Add a `CHANGELOG.md`
    `[Unreleased]` entry under Added/Changed/Fixed.
  - Internals or subtle fix? Update the relevant `docs/*-INTERNALS.md`;
    record non-obvious fixes in `docs/KNOWN-BUGS.md` so the behavior isn't
    regressed later.

## Tech Stack

| Layer | Version | Constraint |
|-------|---------|------------|
| Svelte | 5.x | **Use Svelte 5 runes exclusively** (`$state`, `$derived`, `$effect`). Do NOT mix with legacy `$:` syntax. |
| Tauri | 2.x | WebView2 on Windows. Native dialogs freeze the JS event loop. |
| Vite | 6.x | Frontend bundler. `npm run build` produces `dist/`. |
| Rust | 1.88+ (GUI) / 1.85+ (CLI) | Backend commands in `src-tauri/src/commands.rs`. |

## Build Commands

| Goal | Command | Notes |
|------|---------|-------|
| Compile check only | `cd gui/src-tauri && cargo build --release` | Produces a **dev-mode exe** that loads `devUrl` (localhost:5173), not embedded assets — plain `cargo build` lacks the `tauri/custom-protocol` feature that `cargo tauri build` injects. Do NOT run this binary; it shows ERR_CONNECTION_REFUSED. |
| Runnable release exe | `cd gui && cargo tauri build --no-bundle` | Embeds `dist/`; skips MSI/NSIS packaging. Use for pre-commit verification. |
| Full build + installers | `cd gui && cargo tauri build` | MSI + NSIS bundles under `gui/src-tauri/target/release/bundle/`. Required for installer/resource testing and release prep (see `docs/RELEASE.md`). `.exe` at `gui/src-tauri/target/release/minipro-gui.exe`. On Windows, `build.rs` also stages `target/release/minipro{,-tui}.exe` (root workspace) into the installer — build the release CLI/TUI first to bundle them. |

**Critical rule:** If you change any `.svelte`, `.ts`, `.css`, or `.html`
file, you **must** run `cargo tauri build` (`--no-bundle` is fine) —
embedded assets go stale otherwise. This also applies after version
bumps — the GUI version badge reads `gui/package.json` at Vite build time.

### GUI development & conventions

`cd gui && cargo tauri dev` — vite dev server + debug backend with hot
reload (plain `npm run dev` serves only the frontend — `invoke()` fails).
Simulate hardware without USB: `MINIPRO_SIMULATE_PROGRAMMER=T76` env var,
`minipro-tui --simulate T76`, or Settings → Simulation.

Authoring conventions live in `docs/GUI-INTERNALS.md`. Always-on summary:

- **Svelte 5 runes only** (`$state`/`$derived`/`$effect`) — never `$:`.
- **State lives in writable stores** — module-level vars are invisible
  to reactivity; read `$store.field` directly in templates; keep
  `loading` in a store, never infer from null.
- **Native dialogs freeze the WebView2 event loop** — no `tick()` or
  heavy store updates right after a dialog closes.
- Rust command params are camelCase (Tauri auto-converts invoke keys).

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

GitLab CI workflow rules permit pipelines on `main` pushes, tags, and
manual web triggers — but GitLab skips pipeline creation when the pushed
commit message contains `[skip ci]`, and every commit/merge here carries
it. Effect: `main` pushes are free (pipelines show as "skipped"), and
pipelines actually run only on release tags (tag messages lack the
marker) or manual triggers. Dropping `[skip ci]` from a commit is what
costs a pipeline run — keep it.

**Exception — the release tag's commit must NOT carry `[skip ci]`.**
Lightweight tags have no message of their own: GitLab evaluates the
marker on the *tagged commit*, and GitHub Actions honors it on tag
pushes too, so a `v*` tag on a marked commit silently skips BOTH release
pipelines. The release merge commit (or the marker-free release commit
the tag lands on) is the only commit without it — the v0.13.0 tag was
skipped this way once when its merge carried the marker.
GitHub Actions runs on tags only. Branch pushes are free on both.

Local CI-equivalent checks (run from repo root):

| Check | Command |
|-------|---------|
| fmt (root workspace) | `cargo fmt --all -- --check` |
| fmt (gui crate — `gui/src-tauri` is a separate workspace) | `cd gui/src-tauri && cargo fmt --check` |
| clippy | `cargo clippy --all-targets -- -D warnings` |
| tests | `cargo test --all --locked` |
| frontend types | `cd gui && npm run check` |

### MSRV & cross-distro verification

**MSRV policy: the CLI/core floor tracks the rustc shipped in current
Debian stable** (1.85 as of Debian 13/Trixie) — the floor rolls forward
with each new Debian stable, no per-release debate. Declared floors:
CLI/core/`libarchive-rs` **1.85** (workspace `rust-version`),
`minipro-tui` and `gui/src-tauri` **1.88** (the TUI is `--exclude`d from
every 1.85 check).

CI green on `rust:1.93` does NOT prove MSRV compliance — a Debian Stable
user hit `is_multiple_of()` being unstable on 1.85 (use `x % y == 0`).
Two traps: the GitLab `msrv`/`msrv-tui` jobs are compile-only while the
`docs/RELEASE.md` container recipe also runs tests + clippy on 1.85 —
and `rust-toolchain.toml` (channel 1.93) shadows MSRV checks *inside*
`rust:1.85` containers unless `RUSTUP_TOOLCHAIN=1.85` is set (env beats
the file; this produced a real false-pass once).

**When bumping MSRV, update ALL of these in one commit:** root
`Cargo.toml`, `gui/src-tauri/Cargo.toml`, `README.md`, `gui/README.md`,
this file, and `.gitlab-ci.yml` (msrv job image tag). When changing Linux
package names/install commands, verify against real distro package
databases (names drift — e.g. `libappindicator3-dev` →
`libayatana-appindicator3-dev` in Trixie/Ubuntu 24.04) and update
`README.md`, `gui/README.md`, and `.github/workflows/release.yml`.

Local check: `scripts/msrv-check.sh` runs the full 1.85 gate in one
command (wslc container on Windows, `cargo +1.85` on Linux); full
container recipes in `docs/RELEASE.md`.

## Feature internals — in docs/

Read the relevant doc before working in that area:

- `docs/GUI-INTERNALS.md` — authoring conventions (store patterns,
  Tauri/WebView2 gotchas, data handling, component rules), dev workflows
  (tauri dev, simulation, pre-commit exe), project structure map, hex
  viewer (layout, hotkeys, selection model, pending edits/dirty flag,
  diff), pin-contact test (incl. the T76 `0x3E` hardware caveat), logic
  IC test/identify, config/fuse panel, favorites store, terminal
  rendering rules.
- `docs/TUI-INTERNALS.md` — `minipro-tui` crate map, two-thread event
  model, worker op orchestration (pin/ID gates, erase/verify ordering,
  `can_erase` gating), rendering invariants (resize hysteresis,
  BufWriter frame atomicity), and the simulated-programmer dev mode.
- `docs/CLI-INTERNALS.md` — CLI flag semantics, `--logic-identify`,
  `-z`/`--pin-check` gate behavior.
- `docs/KNOWN-BUGS.md` — historical bugs & fixes. Check before touching
  related code; several fixes encode upstream C minipro behavior that must
  be preserved (voltage tables, chip-ID parsing, `-x`/`-y` semantics,
  `can_erase`).
- `docs/RELEASE.md` — full release-prep procedure (version sync, cargo-deny
  audit, tag push order, MSRV bump file list).
