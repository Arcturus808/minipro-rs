//! LZSS dictionary window (`struct lzss` + `lzss_emit_*`).

use crate::error::Error;

#[derive(Debug, Default)]
pub(crate) struct Lzss {
    pub window: Vec<u8>,
    /// window.len() - 1 when the size is a power of two; 0 when empty.
    pub mask: usize,
    /// Absolute output position (bytes ever emitted into the window).
    pub position: i64,
}

impl Lzss {
    #[inline]
    pub fn position(&self) -> i64 {
        self.position
    }

    #[inline]
    pub fn size(&self) -> usize {
        self.mask + 1
    }

    #[inline]
    pub fn offset_for_position(&self, pos: i64) -> usize {
        (pos as usize) & self.mask
    }

    /// `lzss_emit_literal`
    #[inline]
    pub fn emit_literal(&mut self, literal: u8) {
        let off = self.offset_for_position(self.position);
        self.window[off] = literal;
        self.position += 1;
    }

    /// `lzss_emit_match`: copy `length` bytes from `offset` back in the window.
    pub fn emit_match(&mut self, offset: usize, length: usize) {
        let mut dstoffs = self.offset_for_position(self.position);
        let mut srcoffs = dstoffs.wrapping_sub(offset) & self.mask;
        let mut remaining = length;
        while remaining > 0 {
            let mut l = remaining;
            if dstoffs > srcoffs {
                if l > self.size() - dstoffs {
                    l = self.size() - dstoffs;
                }
            } else if l > self.size() - srcoffs {
                l = self.size() - srcoffs;
            }
            if dstoffs + l < srcoffs || srcoffs + l < dstoffs {
                self.window.copy_within(srcoffs..srcoffs + l, dstoffs);
            } else {
                for li in 0..l {
                    self.window[dstoffs + li] = self.window[srcoffs + li];
                }
            }
            remaining -= l;
            dstoffs = (dstoffs + l) & self.mask;
            srcoffs = (srcoffs + l) & self.mask;
        }
        self.position += length as i64;
    }

    /// `copy_from_lzss_window`: copy `length` bytes starting at absolute
    /// position `startpos` into `buffer`.
    pub fn copy_from_window(
        &self,
        buffer: &mut [u8],
        startpos: i64,
        length: usize,
    ) -> Result<(), Error> {
        let windowoffs = self.offset_for_position(startpos);
        let firstpart = self.size() as i64 - windowoffs as i64;
        if length > self.size() || firstpart < 0 {
            return Err(Error::InvalidData("bad LZSS window copy"));
        }
        let firstpart = firstpart as usize;
        if firstpart < length {
            buffer[..firstpart].copy_from_slice(&self.window[windowoffs..windowoffs + firstpart]);
            buffer[firstpart..length].copy_from_slice(&self.window[..length - firstpart]);
        } else {
            buffer[..length].copy_from_slice(&self.window[windowoffs..windowoffs + length]);
        }
        Ok(())
    }
}
