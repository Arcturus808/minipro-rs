# Internals

Design notes for the RAR4 decoder. Bug history lives in `docs/KNOWN-BUGS.md`.

## Provenance

Ported from libarchive's `archive_read_support_format_rar.c` (BSD-2-Clause)
and `archive_ppmd7.c` (public domain). See README.md for details. Unrar was
not consulted.

## Layout

- `src/lib.rs` — public API (`Archive`, `Entry`, `Timestamp`, `Error`).
- `src/stream.rs` — buffered input, read-ahead/consume, the 64-bit `rar_br`
  bit reader, `skip_sfx` signature scan, and the multi-volume continuation
  (`dbo`) bookkeeping used by `rar_read_ahead`.
- `src/header.rs` — file-header field parsing, FHD_UNICODE filename decode,
  DOS timestamps and extended times (`read_exttime`).
- `src/huffman.rs` — canonical Huffman code construction (tree + flat
  decode table with tree fallback).
- `src/lzss.rs` — the sliding dictionary window, literal/match emission.
- `src/filters.rs` — the RAR VM filter machinery: program registry keyed by
  CRC fingerprint, filter stack, and the four built-ins (delta, x86 E8/E9,
  RGB, audio). Arbitrary VM bytecode programs are rejected as unsupported.
- `src/ppmd.rs` — PPMd variant-H decoder (`Ppmd7` over a `Vec<u8>` arena
  with u32 offset "pointers", plus the RAR range decoder).
- `src/rar.rs` — the archive state machine: header loop, `parse_codes`,
  `expand`, `run_filters`, the compressed/stored output pump, and the
  per-entry reset.
- `src/crc32.rs` — IEEE CRC-32 (header CRCs and file CRCs).
- `src/entry.rs` — `Entry`/`Timestamp` metadata types.

## Stream model

`Stream<B>` mirrors libarchive's `archive_read` plumbing: `pos` is the
consumed position; `next_in` is where the bit reader pulls from (0 = unset,
the NULL sentinel in C). Bytes the bit reader pulls but the upper layers
haven't committed yet are `bytes_unconsumed`; `bytes_remaining` caps the
current packed-data block. When the cap hits zero in a `MHD_VOLUME` archive
whose file has `FHD_SPLIT_AFTER`, `rar_read_ahead` transparently parses the
next volume's headers and verifies the continuation `FILE_HEAD` names the
same (decoded) filename — the `filename_must_match` mechanism.

## Header loop

`Rar::next_entry` is `archive_read_format_rar_read_header`: MARK/MAIN/FILE/
NEWSUB/ENDARC dispatch, generic CRC-checked skipping for other types. A
`FILE_HEAD` whose decoded name equals `filename_save` is a split-entry
continuation — it updates `bytes_remaining`/`dbo` and reports the *same*
entry again (the caller reads the next part's data).

`Rar::read_file_header` performs the per-entry decompressor reset: LZSS
position, dictionary size, bit reader, filter `filterstart`, PPMd state,
unp buffer, length table.

## Output pump

`Rar::pump` produces chunks: `pump_stored` slices packed bytes through the
reader (CRC updated, EOF check on `bytes_remaining == 0 && !split`);
`pump_compressed` is `read_data_compressed` — it drains `filters.out`,
flushes the 128 KiB `unp_buffer` when full, runs `run_filters` at filter
block boundaries, drives the PPMd symbol loop or the LZSS `expand` loop,
and copies emitted window data into `unp_buffer` until a full chunk is
ready. Entry EOF happens on `ppmd_eod` or `offset >= unp_size`, where the
file CRC is verified.

The `filters.lastend == filters.filterstart` boundary plus the `expand`
`end` parameter reproduce libarchive's filter scheduling exactly, including
the (effectively dead in libarchive, and therefore here) chained-filter
loop keyed on `blockstartpos == filterstart` — see KNOWN-BUGS.

## PPMd arena

`Ppmd7` keeps C's memory model: a `base: Vec<u8>` arena; every "pointer"
(context refs, state refs, text cursor, freelist heads) is a u32 byte
offset. Context = 12 bytes (NumStats@0, SummFreq@2, Stats@4, Suffix@8);
state = 6 bytes (Symbol, Freq, Successor lo/hi); node = 12 bytes
(Stamp/NU/Next/Prev, freelist link aliased at offset 0). `glue_free_blocks`
rebuilds the allocator like the C original, including reading the freelist
link at offset 0 *before* overwriting Stamp/NU.
