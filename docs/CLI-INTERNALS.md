# CLI Internals — Feature Documentation

CLI-specific behavior specs for `crates/minipro-cli`. Extracted from
AGENTS.md. For flag-semantics history (`-x`/`-y`, `--vcc`/`--vdd`/`--vpp`
override tables, VCC override warning, `can_erase` handling) see
`docs/KNOWN-BUGS.md` — those fixes encode behavior that must be preserved.

## Logic IC identify

`--logic-identify --pin-count N` tests an unknown logic IC against all
`logicic.xml` entries with the matching pin count. Calls `logic_auto_find()`
in `operations.rs` with a progress callback that prints `Testing candidates... N/M`
to stderr. Passing matches are printed as `name  manufacturer` to stdout
(same format as SPI autodetect). Optional `--vcc` overrides the test voltage.
Supported pin counts: 8, 14, 16, 20, 24, 28, 40. No `-p` device required —
the flag iterates candidates itself. Standalone action, exits after completion.

## Pin-contact test (`-z` / `--pin-check`)

The `-z` / `--pin-check` flag doubles as both a standalone test and a
pre-operation gate, matching upstream minipro. When combined with
`-w`/`-r`/`-E`/`-m`/`-b`/`-D`/`-a`, the pin test runs first and the
operation only proceeds if all pins are good. If `-z` is the only flag,
the test runs standalone and exits.

Model support and hardware caveats are documented in
`docs/GUI-INTERNALS.md` under "Pin-contact test" (TL866II+, T48, and
T76 — the T48 and T76 each have their own single-command `0x3E`
pin-detect protocol; the T76 form returns a 48-pin ZIF bitmap and
requires a bitstream re-upload afterward to restore chip driving —
see `docs/T76-SUPPORT-STATUS.md` and `KNOWN-BUGS.md`). The `-z`
handler prints from the returned
`PinTestResult` struct, preserving the upstream "Bad contact on pin: N"
output format — annotated with the corresponding ZIF socket pin
(`Bad contact on pin: 4 (ZIF pin 24)`), resolved through
`minipro_core::zif::device_to_zif` so the position honors the connected
model's insertion rule.

**Verification:** T48 path verified on real hardware (seated DIP-8
passes; lifted pins 2 and 2+5 report "Bad contact on pin: 2 (ZIF pin
2)" and "5 (ZIF pin 37)" — the annotation lands on the physical lifted
slots). The TL866II+ annotation path shares `device_to_zif` but its
bit-banging measurement remains to be spot-checked when an II+ is
available.

## Health check (`--health-check [N]`)

`--health-check [N]` runs a read-only stability diagnostic on the selected
device (`-p` required): ~20 `get_chip_id` reads plus N full-memory reads
(default 5, clamped 2–64), then analyzes how the results disagree. It is
a standalone action — it exits after printing the report and never
reaches the read/write/erase dispatch or the chip-ID pre-gate (a marginal
chip's drifting ID must not block the check that would flag it).

Analysis lives in `minipro_core::health` (pure buffer math, unit-tested);
the orchestrator `operations::health_check` reuses `read_chip_buffer`
(the read loop extracted from `read_chip`). Report contents: per-pass
CRC-32s, unique chip IDs seen, per-byte disagreement → unstable-byte
total, worst 4 KiB regions, `0→1`/`1→0` flip counts, dominant periodic
spacing, plus a consensus (majority-vote) buffer when instability was
found — save it with `--consensus-out FILE`. Chip-ID characterization
distinguishes drift (multiple plausible IDs) from no-response: when
every observed ID is a stuck-bus value (`0x0` or any-width all-ones —
MISO held low / floating high), the report says the chip isn't
answering the ID command rather than calling the ID unstable.

Verdicts: `Stable` / `Marginal-ID` / `Marginal-Data` /
`Reference-Mismatch` / `Incomplete` (some passes erred) /
`Transport-Error` (no pass completed — programmer fault, not a chip
verdict). Exit codes: 0 / 2 / 3 / 5 / 4 / 1 respectively — deliberately
non-1 for chip-side findings so scripts can distinguish "bad chip" from
"broken setup". Blank/uniform reads are *not* treated as failure — a
blank chip and a dead chip are indistinguishable from reads alone, so
the report only notes uniformity.

`--reference FILE` (with `--health-check`) compares the effective read
image — consensus when unstable, else the single consistent image —
against a known-good file (bin/ihex/srec auto-detected). This catches
what repeated reads can't: deterministic corruption where every pass
agrees on the *wrong* data, e.g. a decode fault or a chip that wasn't
actually programmed with what you think. Mismatch count is reported
(`reference_mismatches`); nonzero → `Reference-Mismatch` (exit 5),
zero → confirming note. Size mismatches compare the common prefix with
a note.

Transport limitations (by design): the XGecu protocols don't expose
opcode-level SPI control, SPI mode switching, clock sweeps, direct power
cycling, or raw status-register reads — those belong to a dedicated
bus-level tester and are out of scope.

## ICSP wiring diagram (`-d` / `--get-info` + `-q` / `--programmer`)

`-d <DEVICE>` prints the database record including the upstream
`ICP<NNN>.JPG` wiring-class reference. Adding `-q <MODEL>` also prints a
human-readable wiring table resolved from the verified static tables in
`minipro_core::icsp` — header pin → signal → chip pin (or signal name for
non-numbered targets like eMMC/PIC/AVR). Without `-q` only the class
reference is shown, with a hint to pass `--programmer`. Unknown classes or
models without a verified table print a fallback note rather than guessing.

## ZIF placement hint (`-d` / `--get-info` + `-q` / `--programmer`)

`-d` also prints a one-line insertion hint for direct-DIP devices,
resolved from the per-model socket rules in `minipro_core::zif` — e.g.
`ZIF placement (T76): bottom of socket — occupies ZIF 21-24 + 25-28
(chip pin 1 at ZIF pin 21)`. Socket size, lever position, and top- vs
bottom-justified insertion are per-model facts (the same table the GUI's
`ZifSocketDiagram` consumes via `get_zif_layout`). ICSP-only devices skip
the line entirely; adapter packages print a "requires an adapter" note
instead — the DIP/non-DIP decision uses the same name-suffix rule as the
GUI (`@DIP*` or bare-name non-PLCC), since `package_details.adapter` is
0 for some adapter packages. Without `-q` a `--programmer` hint is
printed instead, since placement is model-specific.

## XGPro algorithm/firmware downloader (`--download-algorithms`)

`--download-algorithms` is an early-exit action handled before
`MiniproHandle::open()` — no programmer required. It replaces upstream's
`dump-alg-minipro.bash` entirely in Rust (`minipro_core::xgpro_fetch`):
download (`ureq`, env-proxy honored, platform TLS roots) → pinned
SHA-256 verify → in-memory nested RAR5 unwrap via `rar-codec` →
`algorithm.xml` generation → full re-decode + CRC verify of every
emitted bitstream → atomic writes.

- `--output DIR` — destination for `algorithm.xml` + `firmware-*.dat`;
  default is the per-user data dir (the one `DatabasePaths::resolve`
  searches), so a default run needs no elevation. Errors out with a
  hint when no per-user dir can be determined and no `--output` given.
- `--from-file FILE` — offline/mirror input; repeatable (one per
  archive). Auto-detects outer `.rar`, SFX `.exe`, or bare inner RAR,
  and assigns the T48/T56 vs T76 role by inner-archive layout
  (`algorithm/` vs `algoT76/`). SHA-256 is computed and a mismatch is a
  *warning* (local files may be a different XGPro version); for
  downloads the pinned checksum is a hard error.
- `--xgpro-url BASE` — mirror URL base (`<base>/<filename>`); pinned
  checksums still enforced.
- Downloaded archives are cached under `<per-user-dir>/cache/` keyed by
  filename; a cached file that fails the checksum is re-downloaded.
- Progress (indicatif) and all status go to stderr; the result summary
  goes to stdout. After completion, if a programmer happens to be
  connected, the bundled `firmware-<MODEL>-<VER>.dat` matching its model
  is printed **informationally only** — no version comparison, no update
  suggestion (cross-revision flashing is a brick risk).
- Expected counts (354 T56 / 370 T76 for the pinned versions) are
  warn-only, so a future XGPro repack doesn't hard-fail.
- `.gitignore` covers `algorithm.xml`, `firmware-*.dat`, `*.rar` — the
  outputs are XGecu-proprietary and must never be committed.

## SPI status register (`--spi-status` / `--spi-status-write`)

25-series SPI NOR (`protocol_id 0x03`) only — bails with a protocol-id
message on anything else. Both flags run inside the normal transaction
(they're ordinary action blocks in `do_operations`).

- `--spi-status` reads the SR via `spi_status_read` — the fuse `0x01`
  (CFG) path, which on T76 is the real on-chip register (verified on
  W25X20CL; see `docs/T76-SUPPORT-STATUS.md`). Prints the raw byte plus a
  best-effort decode of the common SR1 layout (`SPI_SR1_BIT_NAMES`,
  WIP/WEL marked read-only via `SPI_SR1_READ_ONLY`), a warning when any
  BP bits are set, and any extra bytes the firmware returned.
- `--spi-status-write <BYTE>` writes one byte via `spi_status_write`
  (fuse `0x01` write then a read-back) and prints the post-write state.
  Accepts `0xNN` or decimal. The BP/SRP bits are non-volatile on most
  parts — the flag's help text warns, and `-u`/protect-off clears BP
  bits if the chip ends up protected (verified T76/TL866A/T48).

## Overrides (`-o KEY=VALUE`)

`-o` merges into the `Device` before `begin_transaction` (see
`apply_overrides` in `main.rs`). Keys: `vpp`/`vdd`/`vcc` (validated
against the per-model firmware voltage tables; logic ICs take `vcc`
only), `pulse` (µs), `spi_clock` (raw index), `address` (I²C slave,
`0xNN` ok), `ovc_level`.

- `ovc_level=N` (0–9) sets the VCC over-current trip level sent as LE16
  at offset `0x34` of BEGIN_TRANS. `0` = firmware default; 1–9 ≈
  120–400 mA nominal. Encoding differs per model: T48/T56/TL866II+ get
  the raw calibrated trip code (`OVC_IMAX` in `protocol/mod.rs` — T48
  and TL866II+ share `tl866iiplus.rs`, upstream wires t48.c only but the
  vendor TL866II build sends the same field); the T76 sends the level
  enum itself and additionally honors the `T76_OVC_LEVEL` env var when
  no `-o` override was given. Purpose: ICSP/in-circuit work, where the
  target board loads the VCC rail and trips the default limit — the
  hardware OVC protection stays active, just at the higher trip point.
