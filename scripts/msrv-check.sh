#!/bin/sh
# Runs the workspace MSRV gate — check + test + clippy on the declared
# CLI/core floor (Rust 1.85, tracked to Debian stable; see "MSRV &
# cross-distro verification" in AGENTS.md). Exists so the ritual can't
# drift between docs, CI, and memory. Two load-bearing details baked in:
#
#   * RUSTUP_TOOLCHAIN=1.85 — the repo's rust-toolchain.toml pins the dev
#     toolchain (1.93) and shadows the rust:1.85 image tag; without the
#     env override the check silently runs the dev toolchain and proves
#     nothing about MSRV (a false-pass we've hit on a real release run).
#   * --exclude minipro-tui — the TUI declares rust-version 1.88
#     (ratatui 0.30's floor) and cannot compile on 1.85 by design.
#
# Usage (from repo root):  scripts/msrv-check.sh
#   Git Bash on Windows -> wslc rust:1.85 container (WSL >= 2.9.3)
#   Linux               -> cargo +1.85 via rustup (auto-installs if missing)
set -eu

cd "$(dirname "$0")/.."
MSRV=1.85
PKGS="--workspace --exclude minipro-tui"

if command -v wslc >/dev/null 2>&1; then
    # -v needs the repo path in Windows form (Git Bash pwd -W);
    # MSYS_NO_PATHCONV keeps Git Bash from mangling host:container specs.
    REPO=$(pwd -W 2>/dev/null || pwd)
    MSYS_NO_PATHCONV=1 wslc run --rm \
        -v "$REPO:/src" \
        -v wslc-cargo-registry:/usr/local/cargo/registry \
        -e CARGO_TARGET_DIR=/cargo-target \
        -e RUSTUP_TOOLCHAIN=$MSRV \
        -w /src "rust:$MSRV" \
        sh -c "cargo check $PKGS --locked && \
               cargo test $PKGS --locked && \
               cargo clippy $PKGS --all-targets -- -D warnings"
elif command -v rustup >/dev/null 2>&1; then
    rustup toolchain install "$MSRV" --profile minimal --component clippy
    cargo "+$MSRV" check $PKGS --locked
    cargo "+$MSRV" test $PKGS --locked
    cargo "+$MSRV" clippy $PKGS --all-targets -- -D warnings
else
    echo "error: need wslc (Windows) or a rustup-managed toolchain (Linux)" >&2
    exit 1
fi
