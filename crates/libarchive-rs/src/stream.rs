//! Input stream and bit-level reader for the compressed data region.
//!
//! Port of the `rar_br` bit reader and the stream plumbing from
//! `archive_read_support_format_rar.c`: `rar_read_ahead`, `rar_br_fillup`,
//! `skip_sfx`, and the multi-volume data-block bookkeeping (`dbo`).

use std::ops::Deref;

use crate::error::Error;

pub(crate) const RAR_SIGNATURE: [u8; 7] = *b"Rar!\x1a\x07\x00";
pub(crate) const SFX_MAX_READAHEAD: usize = 1024 * 512;

// Header types
pub(crate) const MARK_HEAD: u8 = 0x72;
pub(crate) const MAIN_HEAD: u8 = 0x73;
pub(crate) const FILE_HEAD: u8 = 0x74;
pub(crate) const NEWSUB_HEAD: u8 = 0x7a;
pub(crate) const ENDARC_HEAD: u8 = 0x7b;

// Main header flags
pub(crate) const MHD_VOLUME: u16 = 0x0001;
pub(crate) const MHD_PASSWORD: u16 = 0x0080;
pub(crate) const MHD_ENCRYPTVER: u16 = 0x0200;

// Flags common to all headers
pub(crate) const HD_ADD_SIZE_PRESENT: u16 = 0x8000;

// File header flags
pub(crate) const FHD_SPLIT_AFTER: u16 = 0x0002;
pub(crate) const FHD_PASSWORD: u16 = 0x0004;
pub(crate) const FHD_SOLID: u16 = 0x0010;
pub(crate) const FHD_LARGE: u16 = 0x0100;
pub(crate) const FHD_UNICODE: u16 = 0x0200;
pub(crate) const FHD_SALT: u16 = 0x0400;
pub(crate) const FHD_EXTTIME: u16 = 0x1000;

const CACHE_BITS: i32 = 64;

const CACHE_MASKS: [u32; 36] = [
    0x00000000, 0x00000001, 0x00000003, 0x00000007, 0x0000000F, 0x0000001F, 0x0000003F, 0x0000007F,
    0x000000FF, 0x000001FF, 0x000003FF, 0x000007FF, 0x00000FFF, 0x00001FFF, 0x00003FFF, 0x00007FFF,
    0x0000FFFF, 0x0001FFFF, 0x0003FFFF, 0x0007FFFF, 0x000FFFFF, 0x001FFFFF, 0x003FFFFF, 0x007FFFFF,
    0x00FFFFFF, 0x01FFFFFF, 0x03FFFFFF, 0x07FFFFFF, 0x0FFFFFFF, 0x1FFFFFFF, 0x3FFFFFFF, 0x7FFFFFFF,
    0xFFFFFFFF, 0xFFFFFFFF, 0xFFFFFFFF, 0xFFFFFFFF,
];

/// Offsets of one contiguous packed-data block of the current entry.
/// In multi-volume archives a file's data is split over several blocks.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct DataBlockOffsets {
    pub start_offset: i64,
    pub end_offset: i64,
}

/// Per-archive state that the multi-volume continuation path needs to touch
/// while parsing a continuation `FILE_HEAD` mid-stream. Bundled so the byte
/// puller in [`Stream`] can drive it without borrowing the whole decoder.
#[derive(Debug, Default)]
pub(crate) struct VolumeState {
    pub main_flags: u16,
    pub file_flags: u16,
    /// Raw (undecoded) filename bytes of the current entry, including the
    /// trailing NUL the C code compares. Used to verify continuation headers
    /// name the same file part.
    pub filename_save: Vec<u8>,
    pub filename_must_match: bool,
    pub has_endarc_header: bool,
    pub dbo: Vec<DataBlockOffsets>,
    pub cursor: usize,
}

/// Buffered input plus the 64-bit cache bit reader used by the decompressor.
///
/// `pos` is the consumed stream position; `next_in` is where the bit reader
/// pulls from. `next_in - pos == bytes_unconsumed` at all times: bytes are
/// pulled into the cache speculatively and only committed to `pos` when the
/// higher layers consume them.
pub(crate) struct Stream<B> {
    input: B,
    pub pos: usize,
    /// Compressed bytes the bit reader pulled but the stream hasn't consumed.
    pub bytes_unconsumed: i64,
    /// Compressed bytes still permitted for the current data block
    /// (`packed_size` of the current part, counting *pulled* bytes down).
    pub bytes_remaining: i64,
    /// Bit cache.
    pub cache_buffer: u64,
    pub cache_avail: i32,
    /// Bytes the bit reader may still pull within the current window.
    pub avail_in: i64,
    /// Absolute position of the next byte the bit reader will pull.
    pub next_in: usize,

    pub vol: VolumeState,
}

impl<B: Deref<Target = [u8]>> Stream<B> {
    pub fn new(input: B) -> Self {
        Self {
            input,
            pos: 0,
            bytes_unconsumed: 0,
            bytes_remaining: 0,
            cache_buffer: 0,
            cache_avail: 0,
            avail_in: 0,
            next_in: 0,
            vol: VolumeState::default(),
        }
    }

    #[inline]
    pub fn input(&self) -> &[u8] {
        &self.input
    }

    #[inline]
    pub fn input_ref(&self) -> &B {
        &self.input
    }

    #[inline]
    pub fn into_input(self) -> B {
        self.input
    }

    /// Remaining bytes in the whole input starting at `pos`.
    #[inline]
    fn stream_remaining(&self) -> usize {
        self.input.len().saturating_sub(self.pos)
    }

    /// Equivalent of `__archive_read_ahead`: return the unread slice at `pos`
    /// when at least `min` bytes remain, without consuming.
    pub fn read_ahead(&self, min: usize) -> Option<&[u8]> {
        if self.stream_remaining() >= min {
            Some(&self.input[self.pos..])
        } else {
            None
        }
    }

    /// Equivalent of `__archive_read_consume`.
    pub fn consume(&mut self, n: usize) {
        let n = n.min(self.stream_remaining());
        self.pos += n;
    }

    /// Read a 7-byte common header at `pos`. Returns `(crc, type, flags, size)`
    /// or `None` when fewer than 7 bytes remain.
    fn common_header(input: &[u8], pos: usize) -> Option<(u16, u8, u16, u16)> {
        let h = input.get(pos..pos + 7)?;
        Some((
            u16::from_le_bytes([h[0], h[1]]),
            h[2],
            u16::from_le_bytes([h[3], h[4]]),
            u16::from_le_bytes([h[5], h[6]]),
        ))
    }

    /// `rar_read_ahead`: like [`Self::read_ahead`], but reports how many bytes
    /// may be consumed, capped by `bytes_remaining`. When the cap is hit and
    /// the entry is split across volumes, parses the continuation header and
    /// retries — exactly like libarchive.
    ///
    /// Returns `(slice_at_pos, avail)`; `None` on truncation.
    pub fn rar_read_ahead(&mut self, min: usize) -> Option<(&[u8], i64)> {
        loop {
            let mut avail = self.stream_remaining() as i64;
            if avail > self.bytes_remaining {
                avail = self.bytes_remaining;
            }
            if avail < 0 {
                return None;
            }
            if avail == 0
                && self.vol.main_flags & MHD_VOLUME != 0
                && self.vol.file_flags & FHD_SPLIT_AFTER != 0
            {
                self.vol.filename_must_match = true;
                let ret = self.read_continuation_header();
                self.vol.filename_must_match = false;
                ret?;
                continue;
            }
            return self.read_ahead(min).map(|s| (s, avail));
        }
    }

    /// Parse the continuation `FILE_HEAD` of a split entry (the `filename_save`
    /// match path of `read_header`), skipping any intervening volume headers.
    /// `bytes_remaining` is set to the new part's `packed_size`; decompressor
    /// state is left untouched.
    pub(crate) fn read_continuation_header(&mut self) -> Option<()> {
        loop {
            let (_, head_type, flags, size) = Self::common_header(self.input(), self.pos)?;
            match head_type {
                MARK_HEAD => {
                    let h = self.read_ahead(7)?;
                    if h[..7] != RAR_SIGNATURE[..] {
                        return None;
                    }
                    self.consume(7);
                }
                MAIN_HEAD => {
                    let skip = size as usize;
                    if skip < 13 {
                        return None;
                    }
                    let (main_flags, crc_ok) = {
                        let h = self.read_ahead(skip)?;
                        (
                            u16::from_le_bytes([h[3], h[4]]),
                            (crate::crc32::crc32(0, &h[2..skip]) & 0xffff) as u16
                                == u16::from_le_bytes([h[0], h[1]]),
                        )
                    };
                    self.vol.main_flags = main_flags;
                    if !crc_ok {
                        return None;
                    }
                    self.consume(skip);
                }
                FILE_HEAD => {
                    return self.read_continuation_file_head(flags, size as usize);
                }
                NEWSUB_HEAD => {
                    // Metadata-only in this reader: verify CRC, then skip the
                    // header body plus its packed data payload.
                    let header_size = size as usize;
                    if header_size < 7 + 25 {
                        return None;
                    }
                    let h = self.read_ahead(header_size)?;
                    let expected = u16::from_le_bytes([h[0], h[1]]);
                    let crc = crate::crc32::crc32(0, &h[2..header_size]);
                    if (crc & 0xffff) as u16 != expected {
                        return None;
                    }
                    let body = &h[7..header_size];
                    let fh = crate::header::parse_file_header_body(body, flags).ok()?;
                    self.consume(header_size + fh.pack_size as usize);
                }
                ENDARC_HEAD => {
                    let skip = size as usize;
                    if skip < 7 {
                        return None;
                    }
                    self.skip_block_crc(skip, flags)?;
                    self.vol.has_endarc_header = true;
                    // Fall through to read the next volume's headers.
                }
                _ => {
                    // COMM/AV/SUB/PROTECT/SIGN: skim and CRC-check.
                    let mut skip = size as usize;
                    if skip < 7 {
                        return None;
                    }
                    if flags & HD_ADD_SIZE_PRESENT != 0 {
                        if skip < 11 {
                            return None;
                        }
                        let h = self.read_ahead(skip)?;
                        let add = u32::from_le_bytes([h[7], h[8], h[9], h[10]]);
                        skip += add as usize;
                    }
                    self.skip_block_crc(skip, flags)?;
                }
            }
        }
    }

    /// The `FILE_HEAD` arm of continuation: verify the decoded name matches the
    /// current entry, then adopt the new part's packed size.
    fn read_continuation_file_head(&mut self, flags: u16, header_size: usize) -> Option<()> {
        self.vol.file_flags = flags;
        if header_size < 7 + 25 {
            return None;
        }
        let h = self.read_ahead(header_size)?;
        let expected = u16::from_le_bytes([h[0], h[1]]);
        let crc = crate::crc32::crc32(0, &h[2..header_size]);
        if (crc & 0xffff) as u16 != expected {
            return None;
        }
        let fh = crate::header::parse_file_header_body(&h[7..header_size], flags).ok()?;
        if (crc & 0xffff) as u16 != expected {
            return None;
        }
        if fh.name_decoded != self.vol.filename_save {
            return None;
        }
        let packed_size = fh.pack_size;
        self.consume(header_size);
        self.avail_in = 0;

        self.vol.cursor += 1;
        if self.vol.cursor >= self.vol.dbo.len() {
            self.vol.dbo.push(DataBlockOffsets {
                start_offset: -1,
                end_offset: -1,
            });
        }
        let cur = self.vol.cursor;
        if self.vol.dbo[cur].start_offset < 0 {
            self.vol.dbo[cur].start_offset = self.pos as i64;
            self.vol.dbo[cur].end_offset = self.pos as i64 + packed_size;
        }
        self.bytes_remaining = packed_size;
        Some(())
    }

    /// Skim a whole header (or header+add-size) verifying the low 16 bits of
    /// its CRC — the generic skip path of the header loop.
    pub(crate) fn skip_block_crc(&mut self, mut skip: usize, _flags: u16) -> Option<()> {
        let expected = {
            let h = self.read_ahead(2)?;
            u16::from_le_bytes([h[0], h[1]])
        };
        self.consume(2);
        skip -= 2;
        let mut crc = 0u32;
        while skip > 0 {
            let n = skip.min(32 * 1024);
            let h = self.read_ahead(n)?;
            crc = crate::crc32::crc32(crc, &h[..n]);
            self.consume(n);
            skip -= n;
        }
        if (crc & 0xffff) as u16 != expected {
            return None;
        }
        Some(())
    }

    /// Skip over an executable stub to find the RAR signature (`skip_sfx`).
    /// Scans 16-byte-aligned offsets within `SFX_MAX_READAHEAD`.
    pub fn skip_sfx(&mut self) -> Result<(), Error> {
        let mut total = 0usize;
        let mut window = 4096usize;
        while total + window <= SFX_MAX_READAHEAD {
            match self.read_ahead(window) {
                None => {
                    window >>= 1;
                    if window < 0x40 {
                        return Err(Error::SignatureNotFound);
                    }
                    continue;
                }
                Some(h) => {
                    let bytes = h.len();
                    if bytes < 0x40 {
                        return Err(Error::SignatureNotFound);
                    }
                    let mut p = 0usize;
                    while p + 7 < bytes {
                        if h[p..p + 7] == RAR_SIGNATURE {
                            self.consume(p);
                            return Ok(());
                        }
                        p += 0x10;
                    }
                    self.consume(p);
                    total += p;
                }
            }
        }
        Err(Error::SignatureNotFound)
    }

    // ---- bit reader (rar_br) ----

    #[inline]
    pub fn br_has(&self, n: i32) -> bool {
        self.cache_avail >= n
    }

    #[inline]
    pub fn br_bits(&self, n: i32) -> u32 {
        ((self.cache_buffer >> (self.cache_avail - n)) as u32) & CACHE_MASKS[n as usize]
    }

    #[inline]
    pub fn br_consume(&mut self, n: i32) {
        self.cache_avail -= n;
    }

    #[inline]
    pub fn br_consume_unaligned_bits(&mut self) {
        self.cache_avail &= !7;
    }

    /// `rar_br_fillup`: pull bytes into the cache until full or input ends.
    /// Returns true when the cache holds 64 bits.
    fn br_fillup(&mut self) -> bool {
        let mut n = CACHE_BITS - self.cache_avail;
        loop {
            match n >> 3 {
                8 => {
                    if self.avail_in >= 8 {
                        self.pull_bytes(8);
                        return true;
                    }
                }
                7 => {
                    if self.avail_in >= 7 {
                        self.pull_bytes(7);
                        return true;
                    }
                }
                6 => {
                    if self.avail_in >= 6 {
                        self.pull_bytes(6);
                        return true;
                    }
                }
                0 => return true,
                _ => {}
            }
            if self.avail_in <= 0 {
                if self.bytes_unconsumed > 0 {
                    self.consume(self.bytes_unconsumed as usize);
                    self.bytes_unconsumed = 0;
                }
                match self.rar_read_ahead(1) {
                    None => return false,
                    Some((_, avail)) => {
                        if avail == 0 {
                            return false;
                        }
                        // next_in already tracks the pull position; refresh the
                        // window size only (data is contiguous).
                        self.avail_in = avail;
                        self.next_in = self.pos;
                    }
                }
            }
            let b = self.input[self.next_in];
            self.cache_buffer = (self.cache_buffer << 8) | b as u64;
            self.next_in += 1;
            self.avail_in -= 1;
            self.cache_avail += 8;
            self.bytes_unconsumed += 1;
            self.bytes_remaining -= 1;
            n -= 8;
        }
    }

    /// Fast-path cache fill of `count` whole bytes from `next_in`.
    fn pull_bytes(&mut self, count: usize) {
        let base = self.next_in;
        match count {
            8 => {
                let s = &self.input[base..base + 8];
                self.cache_buffer =
                    u64::from_be_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]);
            }
            7 => {
                let s = &self.input[base..base + 7];
                self.cache_buffer = (self.cache_buffer << 56)
                    | (s[0] as u64) << 48
                    | (s[1] as u64) << 40
                    | (s[2] as u64) << 32
                    | (s[3] as u64) << 24
                    | (s[4] as u64) << 16
                    | (s[5] as u64) << 8
                    | s[6] as u64;
            }
            6 => {
                let s = &self.input[base..base + 6];
                self.cache_buffer = (self.cache_buffer << 48)
                    | (s[0] as u64) << 40
                    | (s[1] as u64) << 32
                    | (s[2] as u64) << 24
                    | (s[3] as u64) << 16
                    | (s[4] as u64) << 8
                    | s[5] as u64;
            }
            _ => unreachable!(),
        }
        self.next_in += count;
        self.avail_in -= count as i64;
        self.cache_avail += 8 * count as i32;
        self.bytes_unconsumed += count as i64;
        self.bytes_remaining -= count as i64;
    }

    /// `rar_br_read_ahead`: ensure at least `n` bits are in the cache.
    pub fn br_read_ahead(&mut self, n: i32) -> bool {
        if self.br_has(n) {
            return true;
        }
        self.br_fillup();
        self.br_has(n)
    }

    /// `rar_br_preparation`: set up the pull window at the current position.
    pub fn br_preparation(&mut self) -> Result<(), Error> {
        if self.bytes_remaining > 0 {
            if self.read_ahead(1).is_none() {
                return Err(Error::InvalidData("truncated RAR file data"));
            }
            let avail = self.stream_remaining().min(self.bytes_remaining as usize) as i64;
            self.avail_in = avail;
            self.next_in = self.pos;
            if self.cache_avail == 0 {
                let _ = self.br_fillup();
            }
        }
        Ok(())
    }

    /// Decode one byte from the bit stream (`rar_decode_byte` / `ppmd_read`).
    pub fn br_pull_byte(&mut self) -> Option<u8> {
        if !self.br_read_ahead(8) {
            return None;
        }
        let b = self.br_bits(8) as u8;
        self.br_consume(8);
        Some(b)
    }
}
