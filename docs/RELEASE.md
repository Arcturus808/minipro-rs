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

Before tagging, verify the CLI workspace builds and passes tests on Linux.
This catches MSRV regressions, missing system deps, and `cfg(unix)` issues
before they burn release-pipeline minutes. This is a maintainer
optimization — external contributors never trigger our CI (GitLab runs on
`main` pushes, GitHub Actions on tags), so it is not a contribution gate.

**Scope:** this validates the CLI/core workspace only. The Tauri GUI's
Linux build has heavier system dependencies and is covered separately
below as an optional step.

Pick whichever recipe matches your platform:

### Native Linux

```bash
cargo +1.85 check --all --locked && cargo test --all --locked && \
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
  cargo +1.85 check --all --locked && \
  cargo test --all --locked && \
  cargo clippy --all-targets -- -D warnings"
```

### Docker (any platform with Docker Desktop / Engine)

```bash
docker run --rm -v "$PWD:/src" -w /src rust:1.85 \
  sh -c "cargo check --all --locked && cargo test --all --locked && \
         cargo clippy --all-targets -- -D warnings"
```

Uses the same `rust:1.85` image as the GitLab `msrv` job, so results match
CI closely. (On Windows/macOS the bind mount is slower than a native
filesystem — for a pure release check, cloning inside the container is
equally valid.)

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

## Bumping the MSRV

MSRV: CLI/core 1.85, GUI 1.88. When bumping either, update ALL of these
files in the same commit:

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
