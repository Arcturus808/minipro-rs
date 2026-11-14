# Known bugs and quirks

Non-obvious behaviors worth preserving, ported from libarchive's RAR reader.
Record regressions found and fixed here so they are not reintroduced.

- Solid archives (`FHD_SOLID`) are rejected, matching libarchive.
- Encrypted entries (header or data) are reported, not decoded.
- **Stored entries are not CRC-verified**: the `offset_seek >= unp_size`
  EOF check in `read_data` fires before `read_data_stored`'s CRC check ever
  runs (stored `unp_size == packed_size`, so offset_seek reaches unp_size
  exactly when the last byte is delivered). Compressed entries *are*
  verified at the `ppmd_eod`/`offset >= unp_size` boundary. This matches
  libarchive; do not "fix" the ordering or tests that corrupt stored
  payloads will wrongly expect errors.
- **Chained-filter loop is effectively dead code**: `run_filters` sets
  `filters->filterstart = INT64_MAX` at entry, and `parse_filter` only
  assigns `filterstart` when pushing onto an empty stack — so the in-loop
  check `blockstartpos == filterstart` can never match (it would have had
  to fire *before* the end-of-function assignment). Chained filters still
  work via sequential `run_filters` calls: each filter waits until the
  output position reaches its `blockstartpos`. Ported verbatim.
- **PPMd `InitEsc`**: only assigned from the block header when flag `0x40`
  is present; otherwise it keeps whatever `DecodeSymbol` last set. The
  default `ppmd_escape` compare value is `2` when the flag is absent.
- **DOS timestamps are interpreted as UTC**, not local time. libarchive
  feeds the DOS fields through `mktime`/`localtime` (timezone-dependent —
  arguably a bug upstream); this crate reports deterministic UTC seconds.
- **Extended-time "nsec"**: `nsec = tm_sec + rem / NS_UNIT` — the DOS
  *seconds field* plus the whole-seconds part of the 100 ns fraction, not
  nanoseconds in the usual sense. Preserved for compatibility.
- **RAR VM programs**: only the four known built-ins execute (delta, E8/E9,
  E8E9-filesize, RGB, audio — dispatched by bytecode CRC fingerprint).
  Unknown programs return `Unsupported`, as libarchive does.
- **`symbol == 256` semantics**: bit `0` = new file block (optionally new
  Huffman table), bit `1` = re-read the code tables and continue.
- Audio filter `state.delta` is `int8_t` — the `lastdelta - delta[0]`
  difference must wrap mod 256, not saturate.
- `read_continuation_header` walks the full header grammar (MARK/MAIN/
  ENDARC/NEWSUB/skip-others), not just `FILE_HEAD`: a multi-volume part
  may be preceded by its own main header and metadata blocks.
