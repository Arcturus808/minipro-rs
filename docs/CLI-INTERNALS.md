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
`docs/GUI-INTERNALS.md` under "Pin-contact test" (TL866II+/T48 only;
T56/T76 must not run the 0x3E command standalone — see the xgecu-pro
findings documented there). The `-z` handler prints from the returned
`PinTestResult` struct, preserving the upstream "Bad contact on pin: N"
output format — annotated with the corresponding ZIF socket pin
(`Bad contact on pin: 4 (ZIF pin 24)`), resolved through
`minipro_core::zif::device_to_zif` so the position honors the connected
model's insertion rule.

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
