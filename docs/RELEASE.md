# Release Procedure

Full release-prep procedure for minipro-rs. The invariants that apply to
everyday work (`[skip ci]` on all commits, never commit to `main`, never
push tags casually, version fields must match) are in AGENTS.md — this doc
is the how-to for the release task itself.

## Keep GUI and CLI versions in sync

All version numbers in the repo must match for any given release:

| File | Field | Example |
|------|-------|---------|
| `Cargo.toml` (workspace root) | `version` | `0.2.4` |
| `gui/src-tauri/Cargo.toml` | `version` | `0.2.4` |
| `gui/src-tauri/tauri.conf.json` | `version` | `0.2.4` |
| `gui/package.json` | `version` | `0.2.4` |

**Why:** The project is a monorepo with a single tag (`v0.2.4`) that
triggers builds for both the CLI and GUI. If versions drift:

- GUI installer filenames will show the wrong version (e.g.,
  `MINIPRO-RS_0.2.0_x64.msi` inside a `v0.2.3` release)
- Users get confused about which version they have
- Changelogs become unreliable

**CI version verification:** The GitHub Actions release workflow has a
`verify-versions` job that runs before any builds. It checks that all four
version fields match the tag name. If any file is out of sync, the build
fails immediately with a clear error message.

## Local Linux verification (run before the checklist)

**MSRV policy:** the CLI/core floor tracks the rustc shipped in current
Debian stable (1.85 as of Trixie — see AGENTS.md). `minipro-tui` floors at
1.88 via ratatui 0.30 and is excluded from every 1.85 command below;
`gui/src-tauri` declares 1.88 in its own workspace. These recipes run
check + test + clippy on 1.85 — deliberately stricter than the GitLab
`msrv` job (check only), so lint-level divergence gets fixed at release
prep rather than rediscovered on the next MSRV bump.

Before tagging, verify the CLI workspace builds and passes tests on Linux.
This catches MSRV regressions, missing system deps, and `cfg(unix)` issues
before they burn release-pipeline minutes. This is a maintainer
optimization — external contributors never trigger our CI (GitLab runs on
`main` pushes, GitHub Actions on tags), so it is not a contribution gate.

**Scope:** this validates the CLI/core workspace only. The Tauri GUI's
Linux build has heavier system dependencies and is covered separately
below as an optional step. On the 1.85 toolchain `minipro-tui` is
excluded from every command (`--workspace --exclude minipro-tui`) —
ratatui 0.30 floors it at Rust 1.88, mirroring the CI `msrv` job.

Pick whichever recipe matches your platform:

### Native Linux

```bash
cargo +1.85 check --workspace --exclude minipro-tui --locked && \
  cargo test --all --locked && \
  cargo clippy --all-targets -- -D warnings
```

### WSL (Windows — default distro)

Build inside the WSL filesystem, not `/mnt/c`: `/mnt` paths are slow (9P
bridge) and inherit Windows case-insensitivity, which can mask path-case
bugs that would fail on real Linux. For release verification, clone the
repo inside WSL — this also validates the committed tree (what CI sees)
rather than a possibly-dirty working copy.

```bash
# one-time setup:
wsl -d Ubuntu -u root -- apt install -y build-essential pkg-config libssl-dev
wsl -d Ubuntu -- curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
wsl -d Ubuntu -- bash -c "source ~/.cargo/env && rustup install 1.85"

# the check itself:
wsl -d Ubuntu -- bash -c "source ~/.cargo/env && \
  git clone /mnt/<your-repo-path> ~/minipro-rs-check && \
  cd ~/minipro-rs-check && \
  cargo +1.85 check --workspace --exclude minipro-tui --locked && \
  cargo test --all --locked && \
  cargo clippy --all-targets -- -D warnings"
```

### WSL containers (wslc) — Windows, no Docker needed

WSL ≥ 2.9.3 ships `wslc.exe` — Microsoft's built-in Linux-container CLI
(GA 2026-09-29). Same `rust:1.85` image, no Docker Desktop install:

```bash
MSYS_NO_PATHCONV=1 wslc run --rm \
  -v "C:/path/to/minipro-rs:/src" \
  -v wslc-cargo-registry:/usr/local/cargo/registry \
  -e CARGO_TARGET_DIR=/cargo-target -e RUSTUP_TOOLCHAIN=1.85 \
  -w /src rust:1.85 \
  sh -c "cargo check --workspace --exclude minipro-tui --locked && \
         cargo test --workspace --exclude minipro-tui --locked && \
         cargo clippy --workspace --exclude minipro-tui --all-targets -- -D warnings"
```

Notes:

- The host side of `-v` takes a **Windows path** (`D:/...` or `C:\...`).
  `MSYS_NO_PATHCONV=1` is required from Git Bash — without it Git Bash
  mangles the `host:container` spec into a Windows path list. From
  PowerShell/cmd, drop the prefix (it doesn't parse there anyway).
- **`RUSTUP_TOOLCHAIN=1.85` is required** — modern `rust:` images are
  rustup-based, and the repo's `rust-toolchain.toml` (pinned to the dev
  toolchain, not the MSRV) overrides the image tag. Without the env var
  the check silently runs on the dev toolchain and proves nothing about
  MSRV. `CARGO_TARGET_DIR` keeps Linux build artifacts out of the
  mounted `target/` dir so they can't mix with Windows ones.
- The named `wslc-cargo-registry` volume persists the crate cache across
  runs; otherwise every run re-downloads all dependencies (~1–2 min
  extra on a fast connection).
- The mount reports as `drvfs` (same bridge as WSL `/mnt`), so file I/O
  is Windows-mount speed — fine for this check's scale.

### Docker (any platform with Docker Desktop / Engine)

```bash
docker run --rm -v "$PWD:/src" -w /src \
  -e CARGO_TARGET_DIR=/cargo-target -e RUSTUP_TOOLCHAIN=1.85 \
  rust:1.85 \
  sh -c "cargo check --workspace --exclude minipro-tui --locked && \
         cargo test --workspace --exclude minipro-tui --locked && \
         cargo clippy --workspace --exclude minipro-tui --all-targets -- -D warnings"
```

Uses the same `rust:1.85` image as the GitLab `msrv` job, so results match
CI closely. `RUSTUP_TOOLCHAIN=1.85` is required for the same reason as in
the wslc recipe — `rust-toolchain.toml` would otherwise override the image
tag. (On Windows/macOS the bind mount is slower than a native filesystem —
for a pure release check, cloning inside the container is equally valid.)

### GUI workspace check (conditional — evaluate mechanically)

The `gui/src-tauri` workspace needs Tauri's Linux system dependencies
(webkit2gtk, appindicator — see `gui/README.md` for the current package
list, which drifts between distros).

**Decide whether to run this by checking what changed since the last
release tag** — do not treat it as discretionary:

```bash
git diff --name-only v<last-tag>..HEAD -- \
  gui/src-tauri/Cargo.toml gui/src-tauri/Cargo.lock \
  gui/package.json gui/src-tauri/tauri.conf.json \
  .github/workflows/ .gitlab-ci.yml
```

- **Diff empty** → skip; the release pipeline covers GUI bundling anyway.
- **Diff non-empty AND Tauri system deps already installed** → run:
  `cargo check --locked --manifest-path gui/src-tauri/Cargo.toml`
- **Diff non-empty AND deps not installed** → do NOT install the Tauri dep
  stack just for this check (large detour). Note it in the release commit
  and let CI validate the GUI build.

This check validates code compilation, not .deb/.AppImage bundling — the
release pipeline remains the authority on packaging.

## Release checklist

1. Update all four version fields above.
2. Add a `## [X.Y.Z]` section to `CHANGELOG.md` (CI extracts this for
   release notes).
3. Run `cargo generate-lockfile` and stage `Cargo.lock` +
   `gui/src-tauri/Cargo.lock`.
4. Run `cargo deny check` (supply-chain audit — see below).
5. Commit with message like `chore(release): bump version to X.Y.Z`.
   Release commits are the only commits that drop `[skip ci]` — the point
   is to run a full pipeline verifying fmt, clippy, tests, and builds
   before tagging.
6. Create the tag `vX.Y.Z`.
7. Push the commit and tag to `origin` (GitLab). The GitLab push mirror
   replicates branches and tags to GitHub automatically, but with a delay
   (seconds to minutes).
8. If GitHub Actions doesn't trigger within a few minutes, push the tag
   directly: `git push github vX.Y.Z`.
9. Let CI build and release everything consistently.

**Release artifact set** (both pipelines): `minipro` CLI + `minipro-tui`
terminal UI — bundled together in the Unix `.tar.gz` and shipped as two
standalone `.exe`s on Windows (see `.github/workflows/release.yml` and
`.gitlab-ci.yml`). The TUI follows the workspace version (`version.workspace`)
so there is no fifth version field to bump; it requires Rust 1.88 (ratatui
0.30) and is excluded from the 1.85 MSRV job.

## Supply-chain audit (cargo-deny)

Run `cargo deny check` during release prep to scan for known
vulnerabilities, license issues, and non-crates.io sources. This is a
**local-only** check — no CI job, to conserve compute minutes. The config
is `deny.toml` at the repo root.

```bash
# One-time install:
cargo install cargo-deny

# Check CLI workspace (run from repo root):
cargo deny check

# Check GUI workspace (Tauri has its own Cargo.lock with 530+ crates):
cd gui/src-tauri && cargo deny --config ../../deny.toml check
```

This checks:
- **Advisories** — known vulnerabilities from the
  [RustSec advisory database](https://rustsec.org/advisories/)
- **Licenses** — all dependencies have GPL-3.0-or-later compatible licenses
- **Sources** — all crates come from crates.io (no git dependencies)

If `cargo deny check` reports advisories, evaluate each one:

- **Critical/high severity** — update the affected dependency before
  releasing
- **Low severity / unmaintained** — document in the release notes if
  accepted
- **False positive** — add the advisory ID to `ignore = []` in `deny.toml`
  with a comment explaining why

## Git remotes — GitLab is primary, GitHub is a push mirror

The repo has multiple remotes. `origin` is GitLab (primary). GitLab has a
push mirror to GitHub (`github` remote). Both branches and tags mirror
automatically, but the mirror has latency — it is not instant. If you need
GitHub Actions to trigger immediately (e.g., for a release), push the tag
directly to `github` instead of waiting for the mirror:

```bash
git push origin main          # GitLab (mirrors to GitHub with delay)
git push origin vX.Y.Z        # GitLab (mirrors to GitHub with delay)
git push github vX.Y.Z        # Immediate — triggers GitHub Actions right away
```

## Rebuild the GUI after bumping

The version badge in the GUI reads from `gui/package.json` at Vite build
time. If you bump `package.json` but only run `cargo build --release`
(without `cargo tauri build`), the stale embedded frontend persists. Always
run `cargo tauri build` after any frontend or version change.

## Testing installers on a dev machine — clean up afterward

Running the NSIS `.exe` installer with `/D=<path>` to inspect what it lays
down is a **real install**, not a dry run — even pointed at a scratch dir
it writes persistent machine state:

- An Uninstall entry (`HKCU\Software\Microsoft\Windows\CurrentVersion\
  Uninstall\MINIPRO-RS-GUI`) pointing at that path
- The install-state key `HKCU\Software\minipro\MINIPRO-RS-GUI`, whose
  `(default)` value is the INSTDIR — **later installers (MSI included)
  read this as the remembered install location and offer it as the
  default destination**
- Start Menu / Desktop shortcuts targeting that path

If that directory is later deleted without uninstalling (e.g., pointed at
a temp dir that Windows cleans), all three become orphans: Settings →
Apps → Uninstall fails with "cannot find uninstall.exe", the next
installer defaults to the stale path, and the stale same-named shortcut
collides with the new one (MSI **Warning 1946** on `System.AppUserModel.ID`).

**Either** run the produced `uninstall.exe` when finished testing, **or**
test in Windows Sandbox / a VM. If it's already orphaned, remove
`HKCU\Software\minipro\MINIPRO-RS-GUI`, the matching `Uninstall\` entry,
and any `.lnk` targeting the dead path.

## Bumping the MSRV

MSRV: CLI/core 1.85, GUI 1.88. Under the Debian-stable policy the natural
trigger for bumping the CLI/core floor is a new Debian stable release
shipping a newer rustc — not "a dep wants a newer feature" (raising the
floor for a dependency is a deliberate policy decision, not a routine
update). When bumping either, update ALL of these files in the same commit:

1. `Cargo.toml` — root workspace `rust-version` (CLI)
2. `gui/src-tauri/Cargo.toml` — `rust-version` (GUI)
3. `README.md` — Linux support section + badge
4. `gui/README.md` — prerequisites section
5. `AGENTS.md` — the MSRV references in the cross-distro verification notes
   and the tech stack table
6. `.gitlab-ci.yml` — the `msrv:` job's `image: rust:X.Y` tag (if the CLI
   MSRV changes)

Missing any of these causes user-reported discrepancies (see GitLab work
item #5).
