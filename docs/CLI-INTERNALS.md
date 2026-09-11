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
output format.
