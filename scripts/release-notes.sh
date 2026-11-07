#!/bin/sh
# Assembles release notes shared by the GitHub Actions and GitLab CI release
# jobs. Single source of truth for the footer (Installers + CLI/TUI guidance)
# so the two forges can't drift. POSIX sh — the GitLab release job runs on
# Alpine (busybox, no bash).
#
# Usage: release-notes.sh <tag> [github|gitlab]   (default forge: github)
set -eu

TAG="${1:?usage: release-notes.sh <tag> [github|gitlab]}"
FORGE="${2:-github}"
VERSION="${TAG#v}"

CHANGELOG_SECTION=$(sed -n "/^## \[$VERSION\]/,/^## \[/p" CHANGELOG.md | sed '$d')
[ -z "$CHANGELOG_SECTION" ] && CHANGELOG_SECTION="Release ${TAG}"

printf '%s\n' "$CHANGELOG_SECTION"
echo ''
echo '---'
echo ''
echo '## Installers'
echo ''
echo '- **Linux**: `.deb` (Debian/Ubuntu)'
echo '- **Windows users**: Download `.msi` / `.exe` from GitHub Releases — native MSVC builds scan clean. The GitLab Windows binaries are cross-compiled and may trigger AV heuristics.'
if [ "$FORGE" = "gitlab" ]; then
  # GitLab release pages link to the GitHub-hosted macOS artifacts directly,
  # so point users at them — but keep the Gatekeeper note too, since clicking
  # an asset link skips the GitHub notes page where it would otherwise appear.
  echo '- **macOS**: Download `.dmg` (GUI) and CLI binary from GitHub Releases (linked below under "Other")'
fi
echo '- **macOS users**: If you see "MINIPRO-RS-GUI is damaged and can'"'"'t be opened," this is Gatekeeper, not real corruption — the app is unsigned. Drag the app to Applications, then run `xattr -cr /Applications/MINIPRO-RS-GUI.app` in Terminal to strip the quarantine attribute.'
echo ''
echo '## CLI + TUI Binaries'
echo 'Standalone `minipro` CLI binaries for Linux, macOS, and Windows are attached below. `minipro-tui` (terminal UI, MVP — feedback welcome) ships alongside: bundled inside the Unix `.tar.gz` archives (with man page + shell completions), and as a separate `minipro-tui-windows-x86_64.exe` on Windows.'
echo ''
echo 'See [CHANGELOG.md](https://gitlab.com/arcturus8081/minipro-rs/-/blob/main/CHANGELOG.md) for full history.'
