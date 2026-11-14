//! Pure-Rust decoder for RAR4 archives.
//!
//! Derived from libarchive's BSD-2-Clause RAR reader
//! (`archive_read_support_format_rar.c`) and its bundled PPMd variant-H
//! decoder (`archive_ppmd7.c`, public domain). No unrar code was consulted.
//! Licensed under GPL-3.0-or-later.
//!
//! The reader accepts archives embedded in self-extracting stubs: the RAR
//! signature is located by scanning (as libarchive's `skip_sfx` does), so a
//! plain `.rar` or an installer `.exe` both work.
//!
//! ```no_run
//! let data = std::fs::read("setup.exe")?;
//! let mut ar = libarchive_rs::Archive::new(&data[..]);
//! while let Some(entry) = ar.next_entry()? {
//!     if entry.is_directory {
//!         continue;
//!     }
//!     let mut buf = Vec::new();
//!     let n = ar.read_data(&mut buf)?;
//!     assert_eq!(n, entry.size);
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod crc32;
mod entry;
mod error;
mod filters;
mod header;
mod huffman;
mod lzss;
mod ppmd;
mod rar;
mod stream;

use std::io::Write;
use std::ops::Deref;

pub use entry::{Entry, Timestamp};
pub use error::Error;

/// A RAR4 archive opened over an in-memory byte buffer.
///
/// `B` may be `&[u8]`, `Vec<u8>`, `Box<[u8]>`, `&Vec<u8>`, etc.
pub struct Archive<B> {
    inner: rar::Rar<B>,
}

impl<B: Deref<Target = [u8]>> Archive<B> {
    /// Wrap a byte buffer containing a RAR4 archive (possibly behind an SFX
    /// stub). Locating the archive happens lazily in `next_entry`, mirroring
    /// libarchive.
    pub fn new(data: B) -> Self {
        Self {
            inner: rar::Rar::new(data),
        }
    }

    /// Parse headers until the next file entry.
    ///
    /// Returns `Ok(None)` at end of archive (`ENDARC_HEAD` or exhausted
    /// input). A split entry spanning volumes may be returned once per part.
    ///
    /// Each entry's data must be consumed — via `read_data`, `read_all`,
    /// or `skip_data` — before calling this again. Pending data is not
    /// auto-skipped; otherwise parsing resumes inside the pending bytes
    /// and fails with `Error::InvalidHeader`.
    pub fn next_entry(&mut self) -> Result<Option<Entry>, Error> {
        self.inner.next_entry()
    }

    /// Decompress the current entry into `w`. Returns bytes written.
    ///
    /// Must be called only after `next_entry` returned `Some`. Handles
    /// stored (`0x30`) and normal-compression (`0x31`..`0x35`) entries.
    /// Compressed entries are CRC-verified at end of data
    /// (`Error::FileCrcMismatch` on failure); stored entries are delivered
    /// without a CRC check, matching libarchive's ordering (see
    /// docs/KNOWN-BUGS.md).
    pub fn read_data<W: Write>(&mut self, w: &mut W) -> Result<u64, Error> {
        let mut total = 0u64;
        while let Some(chunk) = self.inner.pump()? {
            w.write_all(&chunk).map_err(Error::Io)?;
            total += chunk.len() as u64;
        }
        Ok(total)
    }

    /// Decompress the current entry and return its bytes.
    pub fn read_all(&mut self) -> Result<Vec<u8>, Error> {
        let mut out = Vec::new();
        self.read_data(&mut out)?;
        Ok(out)
    }

    /// Skip the current entry's packed data (`read_data_skip`), crossing
    /// volume continuations for split entries.
    pub fn skip_data(&mut self) -> Result<(), Error> {
        self.inner.skip_data()
    }

    /// The input buffer.
    pub fn get_ref(&self) -> &B {
        self.inner.stream.input_ref()
    }

    /// Consume the archive and return the input buffer.
    pub fn into_inner(self) -> B {
        self.inner.stream.into_input()
    }
}
