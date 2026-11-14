//! Core RAR4 reader: header loop, decompressor driver, filter scheduling.
//!
//! Port of `archive_read_format_rar_read_header`, `read_header`,
//! `read_data_stored`, `read_data_compressed`, `parse_codes`, `expand`,
//! `run_filters`, and `read_symlink_stored` from
//! `archive_read_support_format_rar.c`.

use std::ops::Deref;

use crate::entry::{Entry, Timestamp};
use crate::error::Error;
use crate::filters::{RarFilters, VM_MEMORY_SIZE, VirtualMachine, execute_filter, read_filter};
use crate::header::{self, OS_BEOS, OS_MAC_OS, OS_MSDOS, OS_OS2, OS_UNIX, OS_WIN32};
use crate::huffman::{HuffmanCode, SymError, create_code};
use crate::lzss::Lzss;
use crate::ppmd::{Ppmd7, RangeDec};
use crate::stream::{
    DataBlockOffsets, ENDARC_HEAD, FHD_EXTTIME, FHD_PASSWORD, FHD_SALT, FHD_SOLID, FHD_SPLIT_AFTER,
    FILE_HEAD, HD_ADD_SIZE_PRESENT, MAIN_HEAD, MARK_HEAD, MHD_ENCRYPTVER, MHD_PASSWORD, MHD_VOLUME,
    NEWSUB_HEAD, RAR_SIGNATURE, Stream,
};

const COMPRESS_METHOD_STORE: u8 = 0x30;

const MAINCODE_SIZE: usize = 299;
const OFFSETCODE_SIZE: usize = 60;
const LOWOFFSETCODE_SIZE: usize = 17;
const LENGTHCODE_SIZE: usize = 28;
const HUFFMAN_TABLE_SIZE: usize =
    MAINCODE_SIZE + OFFSETCODE_SIZE + LOWOFFSETCODE_SIZE + LENGTHCODE_SIZE;

const MAX_SYMBOL_LENGTH: u8 = 0xF;
const MAX_SYMBOLS: usize = 20;

const UNP_BUFFER_SIZE: usize = 128 * 1024;
const DICTIONARY_MAX_SIZE: u32 = 0x400000;
const MAX_COMPRESS_DEPTH: usize = 1024;
const RAR_MAX_SYMLINK_SIZE: i64 = 1024 * 1024;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;

// archive_entry mode bits (subset we need)
const AE_IFMT: u32 = 0o170000;
const AE_IFREG: u32 = 0o100000;
const AE_IFDIR: u32 = 0o040000;
const AE_IFLNK: u32 = 0o120000;

/// `rar_fls`: highest power of two <= word (0 for 0).
fn rar_fls(mut word: u32) -> u32 {
    word |= word >> 1;
    word |= word >> 2;
    word |= word >> 4;
    word |= word >> 8;
    word |= word >> 16;
    word - (word >> 1)
}

/// The decoder. `B` is the input backing (`&[u8]`, `Vec<u8>`, ...).
pub(crate) struct Rar<B> {
    pub stream: Stream<B>,

    // main-header fields
    encryptver: u8,
    has_encrypted_entries: bool,
    found_first_header: bool,

    // current file-header fields
    compression_method: u8,
    packed_size: i64,
    unp_size: i64,
    file_crc: u32,
    mtime: (i64, u32),
    ctime: (i64, u32),
    atime: (i64, u32),
    arctime: (i64, u32),
    mode: u32,
    salt: [u8; 8],
    current_name_decoded: Vec<u8>,
    current_name_raw: Vec<u8>,
    current_name: String,
    is_symlink_entry: bool,
    link_target: Option<Vec<u8>>,

    // decompression tracking
    bytes_uncopied: i64,
    offset: i64,
    offset_seek: i64,
    offset_outgoing: i64,
    valid: bool,
    unp_offset: usize,
    unp_buffer: Vec<u8>,
    unp_buffer_size: usize,
    dictionary_size: u32,
    entry_eof: bool,
    crc_calculated: u32,

    // LZSS/huffman
    maincode: HuffmanCode,
    offsetcode: HuffmanCode,
    lowoffsetcode: HuffmanCode,
    lengthcode: HuffmanCode,
    lengthtable: [u8; HUFFMAN_TABLE_SIZE],
    lzss: Lzss,
    lastlength: u32,
    lastoffset: u32,
    oldoffset: [u32; 4],
    lastlowoffset: u32,
    numlowoffsetrepeats: u32,
    start_new_table: bool,

    filters: RarFilters,

    // PPMd
    ppmd_valid: bool,
    ppmd_eod: bool,
    is_ppmd_block: bool,
    ppmd_escape: i32,
    ppmd: Ppmd7,
    range_dec: RangeDec,
}

impl<B: Deref<Target = [u8]>> Rar<B> {
    pub fn new(input: B) -> Self {
        Self {
            stream: Stream::new(input),
            encryptver: 0,
            has_encrypted_entries: false,
            found_first_header: false,
            compression_method: 0,
            packed_size: 0,
            unp_size: 0,
            file_crc: 0,
            mtime: (0, 0),
            ctime: (0, 0),
            atime: (0, 0),
            arctime: (0, 0),
            mode: 0,
            salt: [0; 8],
            current_name_decoded: Vec::new(),
            current_name_raw: Vec::new(),
            current_name: String::new(),
            is_symlink_entry: false,
            link_target: None,
            bytes_uncopied: 0,
            offset: 0,
            offset_seek: 0,
            offset_outgoing: 0,
            valid: true,
            unp_offset: 0,
            unp_buffer: Vec::new(),
            unp_buffer_size: 0,
            dictionary_size: 0,
            entry_eof: false,
            crc_calculated: 0,
            maincode: HuffmanCode::default(),
            offsetcode: HuffmanCode::default(),
            lowoffsetcode: HuffmanCode::default(),
            lengthcode: HuffmanCode::default(),
            lengthtable: [0; HUFFMAN_TABLE_SIZE],
            lzss: Lzss::default(),
            lastlength: 0,
            lastoffset: 0,
            oldoffset: [0; 4],
            lastlowoffset: 0,
            numlowoffsetrepeats: 0,
            start_new_table: true,
            filters: RarFilters::new(),
            ppmd_valid: false,
            ppmd_eod: false,
            is_ppmd_block: false,
            ppmd_escape: 2,
            ppmd: Ppmd7::default(),
            range_dec: RangeDec::default(),
        }
    }

    /// `archive_read_format_rar_read_header`: parse headers until the next
    /// file entry or EOF. Returns `Ok(None)` at end of archive.
    pub fn next_entry(&mut self) -> Result<Option<Entry>, Error> {
        // SFX detection on first entry.
        {
            let Some(h) = self.stream.read_ahead(7) else {
                return Ok(None);
            };
            if !self.found_first_header
                && ((h[0] == b'M' && h[1] == b'Z') || h[..4] == [0x7F, b'E', b'L', b'F'])
            {
                self.stream.skip_sfx()?;
            }
        }
        self.found_first_header = true;

        loop {
            let Some(h) = self.stream.read_ahead(7) else {
                return Err(Error::InvalidHeader("failed to read next header"));
            };
            let head_type = h[2];
            match head_type {
                MARK_HEAD => {
                    if h[..7] != RAR_SIGNATURE[..] {
                        return Err(Error::InvalidHeader("invalid marker header"));
                    }
                    self.stream.consume(7);
                }
                MAIN_HEAD => {
                    let flags = u16::from_le_bytes([h[3], h[4]]);
                    let skip = u16::from_le_bytes([h[5], h[6]]) as usize;
                    if skip < 7 + 2 + 4 {
                        return Err(Error::InvalidHeader("invalid header size"));
                    }
                    if flags & MHD_ENCRYPTVER != 0 && skip < 7 + 2 + 4 + 1 {
                        return Err(Error::InvalidHeader("invalid header size"));
                    }
                    let (encryptver, crc_ok) = {
                        let Some(h) = self.stream.read_ahead(skip) else {
                            return Err(Error::InvalidHeader("truncated main header"));
                        };
                        // reserved1[2] @7, reserved2[4] @9, encryptver @13
                        let ev = if flags & MHD_ENCRYPTVER != 0 {
                            h[13]
                        } else {
                            0
                        };
                        let expected = u16::from_le_bytes([h[0], h[1]]);
                        let crc = crate::crc32::crc32(0, &h[2..skip]);
                        (ev, (crc & 0xffff) as u16 == expected)
                    };
                    self.stream.vol.main_flags = flags;
                    self.encryptver = encryptver;
                    if flags & MHD_PASSWORD != 0 {
                        self.has_encrypted_entries = true;
                        return Err(Error::Unsupported("RAR encryption support unavailable"));
                    }
                    if !crc_ok {
                        return Err(Error::InvalidHeader("header CRC error"));
                    }
                    self.stream.consume(skip);
                }
                FILE_HEAD => {
                    return self.read_file_header(FILE_HEAD);
                }
                NEWSUB_HEAD => {
                    self.read_newsub_header()?;
                }
                ENDARC_HEAD => {
                    let flags = u16::from_le_bytes([h[3], h[4]]);
                    let skip = u16::from_le_bytes([h[5], h[6]]) as usize;
                    self.skip_header_block(head_type, flags, skip)?;
                    return Ok(None);
                }
                _ => {
                    // COMM_HEAD/AV_HEAD/SUB_HEAD/PROTECT_HEAD/SIGN_HEAD.
                    let flags = u16::from_le_bytes([h[3], h[4]]);
                    let mut skip = u16::from_le_bytes([h[5], h[6]]) as usize;
                    if skip < 7 {
                        return Err(Error::InvalidHeader("invalid header size too small"));
                    }
                    if flags & HD_ADD_SIZE_PRESENT != 0 {
                        if skip < 7 + 4 {
                            return Err(Error::InvalidHeader("invalid header size too small"));
                        }
                        let Some(h) = self.stream.read_ahead(skip) else {
                            return Err(Error::InvalidHeader("truncated header"));
                        };
                        skip += u32::from_le_bytes([h[7], h[8], h[9], h[10]]) as usize;
                    }
                    self.skip_block_crc(skip)?;
                }
            }
        }
    }

    /// Generic skim-and-CRC-check for non-file headers (the header loop's
    /// default arm).
    fn skip_block_crc(&mut self, skip: usize) -> Result<(), Error> {
        if self.stream.read_ahead(2).is_none() {
            return Err(Error::InvalidHeader("bad RAR file"));
        }
        self.stream
            .skip_block_crc(skip, 0)
            .ok_or(Error::InvalidHeader("header CRC error"))
    }

    /// ENDARC handling: verify via the same skim path; treated as EOF by the
    /// caller regardless of outcome.
    fn skip_header_block(
        &mut self,
        _head_type: u8,
        flags: u16,
        mut skip: usize,
    ) -> Result<(), Error> {
        if skip < 7 {
            return Err(Error::InvalidHeader("invalid header size too small"));
        }
        if flags & HD_ADD_SIZE_PRESENT != 0 {
            if skip < 7 + 4 {
                return Err(Error::InvalidHeader("invalid header size too small"));
            }
            let Some(h) = self.stream.read_ahead(skip) else {
                return Err(Error::InvalidHeader("truncated header"));
            };
            skip += u32::from_le_bytes([h[7], h[8], h[9], h[10]]) as usize;
        }
        self.skip_block_crc(skip)
    }

    /// NEWSUB: consume the whole block (header + packed data).
    fn read_newsub_header(&mut self) -> Result<(), Error> {
        let Some(h) = self.stream.read_ahead(7) else {
            return Err(Error::InvalidHeader("failed to read next header"));
        };
        let flags = u16::from_le_bytes([h[3], h[4]]);
        let header_size = u16::from_le_bytes([h[5], h[6]]) as usize;
        let crc_read = u16::from_le_bytes([h[0], h[1]]);
        if header_size < 7 + 25 {
            return Err(Error::InvalidHeader("invalid header size"));
        }
        let mut crc = crate::crc32::crc32(0, &h[2..7]);
        self.stream.consume(7);
        let Some(h) = self.stream.read_ahead(header_size - 7) else {
            return Err(Error::InvalidHeader("failed to read full header content"));
        };
        crc = crate::crc32::crc32(crc, &h[..header_size - 7]);
        if (crc & 0xffff) as u16 != crc_read {
            return Err(Error::InvalidHeader("header CRC error"));
        }
        let fh = header::parse_file_header_body(&h[..header_size - 7], flags)?;
        let skip = fh.pack_size + header_size as i64;
        if skip < 7 {
            return Err(Error::InvalidHeader("overlarge extended header"));
        }
        self.stream.consume(header_size - 7);
        self.stream.consume(fh.pack_size as usize);
        Ok(())
    }

    /// `read_header` for FILE_HEAD: parse the file header, set up entry state.
    /// On a same-name continuation header, emits the previous entry again
    /// (the multi-volume continuation path of `read_header`).
    pub fn read_file_header(&mut self, head_type: u8) -> Result<Option<Entry>, Error> {
        let h = self.stream.read_ahead(7).unwrap();
        let file_flags = u16::from_le_bytes([h[3], h[4]]);
        let header_size = u16::from_le_bytes([h[5], h[6]]) as usize;
        if header_size < 25 + 7 {
            return Err(Error::InvalidHeader("invalid header size"));
        }
        let mut crc_computed = crate::crc32::crc32(0, &h[2..7]);
        let crc_read = u16::from_le_bytes([h[0], h[1]]);
        self.stream.consume(7);

        if file_flags & FHD_SOLID == 0 {
            self.compression_method = 0;
            self.packed_size = 0;
            self.unp_size = 0;
            self.mtime = (0, 0);
            self.ctime = (0, 0);
            self.atime = (0, 0);
            self.arctime = (0, 0);
            self.mode = 0;
            self.salt = [0; 8];
        } else {
            return Err(Error::Unsupported("RAR solid archive support unavailable"));
        }

        let Some(h) = self.stream.read_ahead(header_size - 7) else {
            return Err(Error::InvalidHeader("failed to read full header content"));
        };
        crc_computed = crate::crc32::crc32(crc_computed, &h[..header_size - 7]);
        if (crc_computed & 0xffff) as u16 != crc_read {
            return Err(Error::InvalidHeader("header CRC error"));
        }
        let fh = header::parse_file_header_body(&h[..header_size - 7], file_flags)?;
        self.stream.vol.file_flags = file_flags;
        self.compression_method = fh.method;
        self.mtime = (header::dos_time_to_unix(fh.file_time), 0);
        self.file_crc = fh.file_crc;
        self.packed_size = fh.pack_size;
        self.unp_size = fh.unp_size;
        self.stream.bytes_remaining = self.packed_size;

        if file_flags & FHD_PASSWORD != 0 {
            self.has_encrypted_entries = true;
            // Data itself is encrypted; metadata still readable.
            return Err(Error::Unsupported("RAR encryption support unavailable"));
        }

        // Same-name continuation part of a split entry?
        if !self.stream.vol.filename_save.is_empty()
            && fh.name_decoded == self.stream.vol.filename_save
        {
            self.stream.consume(header_size - 7);
            self.stream.avail_in = 0;
            self.stream.next_in = 0;
            self.stream.vol.cursor += 1;
            if self.stream.vol.cursor >= self.stream.vol.dbo.len() {
                self.stream.vol.dbo.push(DataBlockOffsets {
                    start_offset: -1,
                    end_offset: -1,
                });
            }
            let cur = self.stream.vol.cursor;
            if self.stream.vol.dbo[cur].start_offset < 0 {
                self.stream.vol.dbo[cur].start_offset = self.stream.pos as i64;
                self.stream.vol.dbo[cur].end_offset = self.stream.pos as i64 + self.packed_size;
            }
            return Ok(Some(self.make_entry()));
        } else if self.stream.vol.filename_must_match {
            return Err(Error::InvalidHeader(
                "mismatch of file parts split across multi-volume archive",
            ));
        }

        // New entry.
        self.stream.vol.filename_save = fh.name_decoded.clone();
        self.current_name_decoded = fh.name_decoded.clone();
        self.current_name_raw = fh.name_raw.clone();
        self.current_name = header::name_to_string(&fh.name_decoded, fh.unicode_decoded);
        self.link_target = None;

        self.stream.vol.dbo = vec![DataBlockOffsets {
            start_offset: -1,
            end_offset: -1,
        }];
        self.stream.vol.cursor = 0;

        let mut tail = fh.tail;
        if file_flags & FHD_SALT != 0 {
            if tail.0 + 8 > tail.1 {
                return Err(Error::InvalidHeader("invalid header size"));
            }
            self.salt.copy_from_slice(
                &self.stream.input()[self.stream.pos + tail.0..self.stream.pos + tail.0 + 8],
            );
            tail.0 += 8;
        }
        if file_flags & FHD_EXTTIME != 0 {
            let body = &self.stream.input()[self.stream.pos..self.stream.pos + header_size - 7];
            header::read_exttime(
                body,
                tail,
                &mut self.mtime,
                &mut self.ctime,
                &mut self.atime,
                &mut self.arctime,
            )?;
        }

        self.stream.consume(header_size - 7);
        self.stream.vol.dbo[0].start_offset = self.stream.pos as i64;
        self.stream.vol.dbo[0].end_offset = self.stream.pos as i64 + self.packed_size;

        match fh.host_os {
            OS_MSDOS | OS_OS2 | OS_WIN32 => {
                let attr = fh.attr;
                if attr & FILE_ATTRIBUTE_DIRECTORY != 0 {
                    self.mode = AE_IFDIR | 0o111;
                } else {
                    self.mode = AE_IFREG;
                }
                self.mode |= 0o644;
            }
            OS_UNIX | OS_MAC_OS | OS_BEOS => {
                self.mode = fh.attr;
            }
            _ => {
                return Err(Error::InvalidHeader(
                    "unknown file attributes from RAR file's host OS",
                ));
            }
        }

        self.bytes_uncopied = 0;
        self.stream.bytes_unconsumed = 0;
        self.lzss.position = 0;
        self.offset = 0;
        self.offset_seek = 0;
        self.dictionary_size = 0;
        self.offset_outgoing = 0;
        self.stream.cache_avail = 0;
        self.stream.avail_in = 0;
        self.stream.next_in = 0;
        self.crc_calculated = 0;
        self.entry_eof = false;
        self.valid = true;
        self.is_ppmd_block = false;
        self.start_new_table = true;
        self.unp_buffer = Vec::new();
        self.unp_offset = 0;
        self.unp_buffer_size = UNP_BUFFER_SIZE;
        self.lengthtable = [0; HUFFMAN_TABLE_SIZE];
        self.ppmd.free();
        self.ppmd_valid = false;
        self.ppmd_eod = false;
        self.filters.filterstart = i64::MAX;

        self.is_symlink_entry = (self.mode & AE_IFMT) == AE_IFLNK;
        if self.is_symlink_entry {
            // A symbolic-link file has no decompressed body; the target is
            // stored inline as its packed data.
            self.stream.bytes_remaining = 0;
            self.link_target = Some(self.read_symlink_target()?);
        }
        if self.stream.bytes_remaining == 0 {
            self.entry_eof = true;
        }
        let _ = head_type;
        Ok(Some(self.make_entry()))
    }

    fn make_entry(&self) -> Entry {
        Entry {
            name: self.current_name.clone(),
            raw_name: self.current_name_raw.clone(),
            size: if self.is_symlink_entry {
                0
            } else {
                self.unp_size as u64
            },
            packed_size: self.packed_size as u64,
            crc32: self.file_crc,
            is_directory: (self.mode & AE_IFMT) == AE_IFDIR,
            is_symlink: self.is_symlink_entry,
            link_target: self.link_target.clone(),
            mode: self.mode,
            compression_method: self.compression_method,
            mtime: Timestamp::new(self.mtime),
            ctime: Timestamp::new(self.ctime),
            atime: Timestamp::new(self.atime),
            arctime: Timestamp::new(self.arctime),
        }
    }

    /// `read_symlink_stored`: read the symlink target (stored inline).
    pub fn read_symlink_target(&mut self) -> Result<Vec<u8>, Error> {
        if self.packed_size > RAR_MAX_SYMLINK_SIZE {
            return Err(Error::InvalidData("unable to read link"));
        }
        let Some((h, _)) = self.stream.rar_read_ahead(self.packed_size.max(1) as usize) else {
            return Err(Error::InvalidData("failed to read link"));
        };
        let target = h[..self.packed_size as usize].to_vec();
        self.stream.consume(self.packed_size as usize);
        Ok(target)
    }

    /// `archive_read_format_rar_read_data_skip`.
    pub fn skip_data(&mut self) -> Result<(), Error> {
        if self.stream.bytes_unconsumed > 0 {
            self.stream.consume(self.stream.bytes_unconsumed as usize);
            self.stream.bytes_unconsumed = 0;
        }
        if self.stream.bytes_remaining > 0 {
            let n = self
                .stream
                .bytes_remaining
                .min(self.stream.input().len() as i64 - self.stream.pos as i64);
            self.stream.consume(n as usize);
            self.stream.bytes_remaining -= n;
        }
        if self.stream.vol.main_flags & MHD_VOLUME != 0
            && self.stream.vol.file_flags & FHD_SPLIT_AFTER != 0
        {
            self.stream
                .read_continuation_header()
                .ok_or(Error::InvalidData("bad RAR file"))?;
            return self.skip_data();
        }
        Ok(())
    }

    /// Produce the next decompressed chunk for the current entry, or None
    /// when the entry is finished (CRC verified).
    pub fn pump(&mut self) -> Result<Option<Vec<u8>>, Error> {
        if self.stream.bytes_unconsumed > 0 {
            self.stream.consume(self.stream.bytes_unconsumed as usize);
            self.stream.bytes_unconsumed = 0;
        }
        if self.entry_eof || self.offset_seek >= self.unp_size {
            return Ok(None);
        }
        match self.compression_method {
            COMPRESS_METHOD_STORE => self.pump_stored(),
            0x31..=0x35 => self.pump_compressed(0),
            _ => Err(Error::Unsupported(
                "unsupported compression method for RAR file",
            )),
        }
    }

    fn pump_stored(&mut self) -> Result<Option<Vec<u8>>, Error> {
        if self.stream.bytes_remaining == 0
            && !(self.stream.vol.main_flags & MHD_VOLUME != 0
                && self.stream.vol.file_flags & FHD_SPLIT_AFTER != 0)
        {
            if self.file_crc != self.crc_calculated {
                return Err(Error::FileCrcMismatch {
                    expected: self.file_crc,
                    computed: self.crc_calculated,
                });
            }
            self.entry_eof = true;
            return Ok(None);
        }

        let Some((h, avail)) = self.stream.rar_read_ahead(1) else {
            return Err(Error::InvalidData("truncated RAR file data"));
        };
        if avail <= 0 {
            return Err(Error::InvalidData("truncated RAR file data"));
        }
        let size = avail as usize;
        let chunk = h[..size].to_vec();
        self.offset += size as i64;
        self.offset_seek += size as i64;
        self.stream.bytes_remaining -= size as i64;
        self.stream.bytes_unconsumed = size as i64;
        self.crc_calculated = crate::crc32::crc32(self.crc_calculated, &chunk);
        Ok(Some(chunk))
    }

    /// `copy_from_lzss_window_to_unp`: append window bytes to unp_buffer;
    /// returns true when the buffer reached `unp_buffer_size`.
    fn copy_from_lzss_window_to_unp(
        &mut self,
        startpos: i64,
        length: usize,
    ) -> Result<bool, Error> {
        if length > self.unp_buffer_size {
            return Err(Error::InvalidData("bad RAR file data"));
        }
        if self.unp_buffer.is_empty() {
            self.unp_buffer = vec![0; self.unp_buffer_size];
        }
        let windowoffs = self.lzss.offset_for_position(startpos);
        let size = self.lzss.size();
        if windowoffs + length <= size {
            self.unp_buffer[self.unp_offset..self.unp_offset + length]
                .copy_from_slice(&self.lzss.window[windowoffs..windowoffs + length]);
        } else if length <= size {
            let firstpart = size - windowoffs;
            if firstpart < length {
                self.unp_buffer[self.unp_offset..self.unp_offset + firstpart]
                    .copy_from_slice(&self.lzss.window[windowoffs..windowoffs + firstpart]);
                self.unp_buffer[self.unp_offset + firstpart..self.unp_offset + length]
                    .copy_from_slice(&self.lzss.window[..length - firstpart]);
            } else {
                self.unp_buffer[self.unp_offset..self.unp_offset + length]
                    .copy_from_slice(&self.lzss.window[windowoffs..windowoffs + length]);
            }
        } else {
            return Err(Error::InvalidData("bad RAR file data"));
        }
        self.unp_offset += length;
        Ok(self.unp_offset >= self.unp_buffer_size)
    }

    /// `read_data_compressed`: drive one output chunk per call.
    fn pump_compressed(&mut self, mut looper: usize) -> Result<Option<Vec<u8>>, Error> {
        if looper > MAX_COMPRESS_DEPTH {
            return Err(Error::InvalidData("max compress depth exceeded"));
        }
        looper += 1;

        let mut chunk: Option<Vec<u8>> = None;
        while chunk.is_none() {
            if !self.valid {
                return Err(Error::InvalidData("bad RAR file data"));
            }

            if self.filters.bytes_ready > 0 {
                // Flush unp_buffer first if it has pending data.
                if self.unp_offset > 0 {
                    let out = self.unp_buffer[..self.unp_offset].to_vec();
                    self.unp_offset = 0;
                    self.offset_outgoing += out.len() as i64;
                    chunk = Some(out);
                } else {
                    let start = self.filters.out_pos;
                    let out = self.filters.out[start..start + self.filters.bytes_ready].to_vec();
                    self.offset += out.len() as i64;
                    self.offset_outgoing += out.len() as i64;
                    self.filters.out_pos += out.len();
                    self.filters.bytes_ready -= out.len();
                    chunk = Some(out);
                }
                break;
            }

            if self.ppmd_eod || (self.dictionary_size != 0 && self.offset >= self.unp_size) {
                if self.unp_offset > 0 {
                    let out = self.unp_buffer[..self.unp_offset].to_vec();
                    self.offset_outgoing += out.len() as i64;
                    self.crc_calculated = crate::crc32::crc32(self.crc_calculated, &out);
                    self.unp_offset = 0;
                    return Ok(Some(out));
                }
                if self.file_crc != self.crc_calculated {
                    return Err(Error::FileCrcMismatch {
                        expected: self.file_crc,
                        computed: self.crc_calculated,
                    });
                }
                self.entry_eof = true;
                return Ok(None);
            }

            if !self.is_ppmd_block && self.dictionary_size != 0 && self.bytes_uncopied > 0 {
                let bs = if self.bytes_uncopied > (self.unp_buffer_size - self.unp_offset) as i64 {
                    self.unp_buffer_size - self.unp_offset
                } else {
                    self.bytes_uncopied as usize
                };
                let full = self.copy_from_lzss_window_to_unp(self.offset, bs)?;
                self.offset += bs as i64;
                self.bytes_uncopied -= bs as i64;
                if full {
                    let out = std::mem::take(&mut self.unp_buffer);
                    self.unp_offset = 0;
                    self.offset_outgoing += out.len() as i64;
                    self.crc_calculated = crate::crc32::crc32(self.crc_calculated, &out);
                    return Ok(Some(out));
                }
                continue;
            }

            if self.filters.lastend == self.filters.filterstart {
                if !self.run_filters() {
                    return Err(Error::InvalidData("bad RAR file data"));
                }
                continue;
            }

            if self.stream.next_in == 0 {
                self.stream.br_preparation()?;
            }
            if self.start_new_table {
                self.parse_codes()?;
            }

            if self.is_ppmd_block {
                let sym = {
                    let ppmd = &mut self.ppmd;
                    let rc = &mut self.range_dec;
                    let stream = &mut self.stream;
                    let valid = &mut self.valid;
                    let mut read_fn = move || -> u8 {
                        match stream.br_pull_byte() {
                            Some(b) => b,
                            None => {
                                *valid = false;
                                0
                            }
                        }
                    };
                    ppmd.decode_symbol(rc, &mut read_fn)
                };
                if sym < 0 {
                    return Err(Error::InvalidData("invalid symbol"));
                }
                if sym != self.ppmd_escape {
                    self.lzss.emit_literal(sym as u8);
                    self.bytes_uncopied += 1;
                } else {
                    let code = {
                        let ppmd = &mut self.ppmd;
                        let rc = &mut self.range_dec;
                        let stream = &mut self.stream;
                        let valid = &mut self.valid;
                        let mut read_fn = move || -> u8 {
                            match stream.br_pull_byte() {
                                Some(b) => b,
                                None => {
                                    *valid = false;
                                    0
                                }
                            }
                        };
                        ppmd.decode_symbol(rc, &mut read_fn)
                    };
                    if code < 0 {
                        return Err(Error::InvalidData("invalid symbol"));
                    }
                    match code {
                        0 => {
                            self.start_new_table = true;
                            return self.pump_compressed(looper);
                        }
                        2 => {
                            self.ppmd_eod = true;
                            continue;
                        }
                        3 => {
                            return Err(Error::Unsupported("parsing filters is unsupported"));
                        }
                        4 => {
                            let mut lzss_offset = 0i32;
                            for i in (0..3).rev() {
                                let c = {
                                    let ppmd = &mut self.ppmd;
                                    let rc = &mut self.range_dec;
                                    let stream = &mut self.stream;
                                    let valid = &mut self.valid;
                                    let mut read_fn = move || -> u8 {
                                        match stream.br_pull_byte() {
                                            Some(b) => b,
                                            None => {
                                                *valid = false;
                                                0
                                            }
                                        }
                                    };
                                    ppmd.decode_symbol(rc, &mut read_fn)
                                };
                                if c < 0 {
                                    return Err(Error::InvalidData("invalid symbol"));
                                }
                                lzss_offset |= c << (i * 8);
                            }
                            let length = {
                                let ppmd = &mut self.ppmd;
                                let rc = &mut self.range_dec;
                                let stream = &mut self.stream;
                                let valid = &mut self.valid;
                                let mut read_fn = move || -> u8 {
                                    match stream.br_pull_byte() {
                                        Some(b) => b,
                                        None => {
                                            *valid = false;
                                            0
                                        }
                                    }
                                };
                                ppmd.decode_symbol(rc, &mut read_fn)
                            };
                            if length < 0 {
                                return Err(Error::InvalidData("invalid symbol"));
                            }
                            self.lzss
                                .emit_match((lzss_offset + 2) as usize, (length + 32) as usize);
                            self.bytes_uncopied += (length + 32) as i64;
                        }
                        5 => {
                            let length = {
                                let ppmd = &mut self.ppmd;
                                let rc = &mut self.range_dec;
                                let stream = &mut self.stream;
                                let valid = &mut self.valid;
                                let mut read_fn = move || -> u8 {
                                    match stream.br_pull_byte() {
                                        Some(b) => b,
                                        None => {
                                            *valid = false;
                                            0
                                        }
                                    }
                                };
                                ppmd.decode_symbol(rc, &mut read_fn)
                            };
                            if length < 0 {
                                return Err(Error::InvalidData("invalid symbol"));
                            }
                            self.lzss.emit_match(1, (length + 4) as usize);
                            self.bytes_uncopied += (length + 4) as i64;
                        }
                        _ => {
                            self.lzss.emit_literal(sym as u8);
                            self.bytes_uncopied += 1;
                        }
                    }
                }
            } else {
                let start = self.offset;
                let mut end = start + self.dictionary_size as i64;
                if self.dictionary_size > 260 {
                    end -= 260;
                }
                if self.filters.filterstart < end {
                    end = self.filters.filterstart;
                }

                self.expand(&mut end)?;

                self.bytes_uncopied = end - start;
                self.filters.lastend = end;
                if self.filters.lastend != self.filters.filterstart && self.bytes_uncopied == 0 {
                    return Err(Error::InvalidData("internal error extracting RAR file"));
                }
            }

            let bs = if self.bytes_uncopied > (self.unp_buffer_size - self.unp_offset) as i64 {
                self.unp_buffer_size - self.unp_offset
            } else {
                self.bytes_uncopied as usize
            };
            let full = self.copy_from_lzss_window_to_unp(self.offset, bs)?;
            self.offset += bs as i64;
            self.bytes_uncopied -= bs as i64;
            if full {
                let out = std::mem::take(&mut self.unp_buffer);
                self.unp_offset = 0;
                self.offset_outgoing += out.len() as i64;
                self.crc_calculated = crate::crc32::crc32(self.crc_calculated, &out);
                return Ok(Some(out));
            }
            // If the buffer isn't full, keep extracting.
            chunk = None;
        }

        // ending_block: CRC over emitted chunk.
        if let Some(out) = &chunk {
            self.crc_calculated = crate::crc32::crc32(self.crc_calculated, out);
        }
        Ok(chunk)
    }

    /// `run_filters`: decompress a filter block and run queued VM filters.
    fn run_filters(&mut self) -> bool {
        if self.filters.stack.is_empty() {
            return false;
        }
        let generation = self.filters.generation;
        let start = self.filters.filterstart as usize;
        let end0 = start + self.filters.stack[0].blocklength as usize;

        self.filters.filterstart = i64::MAX;
        let mut tend = end0 as i64;
        if self.expand(&mut tend).is_err() {
            return false;
        }

        // Check if the filter stack was reinitialized in expand().
        if generation != self.filters.generation || self.filters.stack.is_empty() {
            return false;
        }
        if tend < 0 {
            return false;
        }
        let end = tend as usize;
        if end != end0 {
            // Bad data: filter block was not fully decompressed.
            return false;
        }

        if self.filters.vm.is_none() {
            self.filters.vm = Some(Box::new(VirtualMachine::new()));
        }
        let mut filter = self.filters.stack.pop_front().unwrap();
        if filter.blocklength as usize > VM_MEMORY_SIZE {
            return false;
        }
        {
            let vm = self.filters.vm.as_mut().unwrap();
            if self
                .lzss
                .copy_from_window(&mut vm.memory, start as i64, filter.blocklength as usize)
                .is_err()
            {
                return false;
            }
        }
        let prog_fp = self.filters.progs[filter.prog].fingerprint;
        {
            let vm = self.filters.vm.as_mut().unwrap();
            if execute_filter(&mut filter, prog_fp, vm, self.offset as usize).is_err() {
                return false;
            }
        }
        let mut lastfilteraddress = filter.filteredblockaddress;
        let mut lastfilterlength = filter.filteredblocklength;

        // Run chained filters at the same block position.
        while let Some(f) = self.filters.stack.front() {
            if f.blockstartpos as i64 != self.filters.filterstart
                || f.blocklength != lastfilterlength
            {
                break;
            }
            let mut f = self.filters.stack.pop_front().unwrap();
            {
                let vm = self.filters.vm.as_mut().unwrap();
                vm.memory.copy_within(
                    lastfilteraddress as usize
                        ..lastfilteraddress as usize + lastfilterlength as usize,
                    0,
                );
            }
            let fp = self.filters.progs[f.prog].fingerprint;
            {
                let vm = self.filters.vm.as_mut().unwrap();
                if execute_filter(&mut f, fp, vm, self.offset as usize).is_err() {
                    return false;
                }
            }
            lastfilteraddress = f.filteredblockaddress;
            lastfilterlength = f.filteredblocklength;
        }

        if let Some(f) = self.filters.stack.front() {
            if (f.blockstartpos as i64) < end as i64 {
                return false;
            }
            self.filters.filterstart = f.blockstartpos as i64;
        }

        self.filters.lastend = end as i64;
        {
            let vm = self.filters.vm.as_ref().unwrap();
            self.filters.out = vm.memory[lastfilteraddress as usize
                ..lastfilteraddress as usize + lastfilterlength as usize]
                .to_vec();
        }
        self.filters.out_pos = 0;
        self.filters.bytes_ready = lastfilterlength as usize;
        true
    }

    /// `expand`: decode LZSS symbols until the window reaches `*end`.
    fn expand(&mut self, end: &mut i64) -> Result<(), Error> {
        const LENGTHBASES: [u32; 28] = [
            0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 14, 16, 20, 24, 28, 32, 40, 48, 56, 64, 80, 96, 112,
            128, 160, 192, 224,
        ];
        const LENGTHBITS: [u32; 28] = [
            0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5,
        ];
        const OFFSETBASES: [u32; 60] = [
            0, 1, 2, 3, 4, 6, 8, 12, 16, 24, 32, 48, 64, 96, 128, 192, 256, 384, 512, 768, 1024,
            1536, 2048, 3072, 4096, 6144, 8192, 12288, 16384, 24576, 32768, 49152, 65536, 98304,
            131072, 196608, 262144, 327680, 393216, 458752, 524288, 589824, 655360, 720896, 786432,
            851968, 917504, 983040, 1048576, 1310720, 1572864, 1835008, 2097152, 2359296, 2621440,
            2883584, 3145728, 3407872, 3670016, 3932160,
        ];
        const OFFSETBITS: [u32; 60] = [
            0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12,
            12, 13, 13, 14, 14, 15, 15, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 18,
            18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18,
        ];
        const SHORTBASES: [u32; 8] = [0, 4, 8, 16, 32, 64, 128, 192];
        const SHORTBITS: [u32; 8] = [2, 2, 3, 4, 5, 6, 6, 6];

        if self.filters.filterstart < *end {
            *end = self.filters.filterstart;
        }

        loop {
            if self.lzss.position() >= *end {
                return Ok(());
            }
            if self.is_ppmd_block {
                *end = self.lzss.position();
                return Ok(());
            }

            let symbol = self
                .maincode
                .read_next_symbol(&mut self.stream)
                .map_err(|e| match e {
                    SymError::Truncated => {
                        self.valid = false;
                        Error::InvalidData("truncated RAR file data")
                    }
                    SymError::BadCode => Error::InvalidData("bad RAR file data"),
                })?;

            let (offs, len);
            if symbol < 256 {
                self.lzss.emit_literal(symbol as u8);
                continue;
            } else if symbol == 256 {
                if !self.stream.br_read_ahead(1) {
                    self.valid = false;
                    return Err(Error::InvalidData("truncated RAR file data"));
                }
                let newfile = self.stream.br_bits(1) == 0;
                self.stream.br_consume(1);
                if newfile {
                    if !self.stream.br_read_ahead(1) {
                        self.valid = false;
                        return Err(Error::InvalidData("truncated RAR file data"));
                    }
                    self.start_new_table = self.stream.br_bits(1) != 0;
                    self.stream.br_consume(1);
                    *end = self.lzss.position();
                    return Ok(());
                } else {
                    self.parse_codes()
                        .map_err(|_| Error::InvalidData("bad RAR file data"))?;
                    continue;
                }
            } else if symbol == 257 {
                let ok = {
                    let stream = &mut self.stream;
                    let filters = &mut self.filters;
                    let pos = self.lzss.position();
                    let dict = self.dictionary_size;
                    let mask = self.lzss.mask;
                    read_filter(stream, filters, pos, dict, mask, end)
                };
                if !ok {
                    return Err(Error::InvalidData("bad RAR file data"));
                }
                continue;
            } else if symbol == 258 {
                if self.lastlength == 0 {
                    continue;
                }
                offs = self.lastoffset;
                len = self.lastlength;
            } else if symbol <= 262 {
                let offsindex = (symbol - 259) as usize;
                let o = self.oldoffset[offsindex];
                let lensymbol =
                    self.lengthcode
                        .read_next_symbol(&mut self.stream)
                        .map_err(|e| match e {
                            SymError::Truncated => {
                                self.valid = false;
                                Error::InvalidData("truncated RAR file data")
                            }
                            SymError::BadCode => Error::InvalidData("bad RAR file data"),
                        })?;
                if lensymbol < 0 || lensymbol as usize >= LENGTHBASES.len() {
                    return Err(Error::InvalidData("bad RAR file data"));
                }
                let mut l = LENGTHBASES[lensymbol as usize] + 2;
                if LENGTHBITS[lensymbol as usize] > 0 {
                    let nb = LENGTHBITS[lensymbol as usize] as i32;
                    if !self.stream.br_read_ahead(nb) {
                        self.valid = false;
                        return Err(Error::InvalidData("truncated RAR file data"));
                    }
                    l += self.stream.br_bits(nb);
                    self.stream.br_consume(nb);
                }
                for i in (1..=offsindex).rev() {
                    self.oldoffset[i] = self.oldoffset[i - 1];
                }
                self.oldoffset[0] = o;
                offs = o;
                len = l;
            } else if symbol <= 270 {
                let idx = (symbol - 263) as usize;
                let mut o = SHORTBASES[idx] + 1;
                if SHORTBITS[idx] > 0 {
                    let nb = SHORTBITS[idx] as i32;
                    if !self.stream.br_read_ahead(nb) {
                        self.valid = false;
                        return Err(Error::InvalidData("truncated RAR file data"));
                    }
                    o += self.stream.br_bits(nb);
                    self.stream.br_consume(nb);
                }
                let l = 2;
                for i in (1..4).rev() {
                    self.oldoffset[i] = self.oldoffset[i - 1];
                }
                self.oldoffset[0] = o;
                offs = o;
                len = l;
            } else {
                let idx = (symbol - 271) as usize;
                if idx >= LENGTHBASES.len() {
                    return Err(Error::InvalidData("bad RAR file data"));
                }
                let mut l = LENGTHBASES[idx] + 3;
                if LENGTHBITS[idx] > 0 {
                    let nb = LENGTHBITS[idx] as i32;
                    if !self.stream.br_read_ahead(nb) {
                        self.valid = false;
                        return Err(Error::InvalidData("truncated RAR file data"));
                    }
                    l += self.stream.br_bits(nb);
                    self.stream.br_consume(nb);
                }

                let offssymbol =
                    self.offsetcode
                        .read_next_symbol(&mut self.stream)
                        .map_err(|e| match e {
                            SymError::Truncated => {
                                self.valid = false;
                                Error::InvalidData("truncated RAR file data")
                            }
                            SymError::BadCode => Error::InvalidData("bad RAR file data"),
                        })?;
                if offssymbol < 0 || offssymbol as usize >= OFFSETBASES.len() {
                    return Err(Error::InvalidData("bad RAR file data"));
                }
                let mut o = OFFSETBASES[offssymbol as usize] + 1;
                if OFFSETBITS[offssymbol as usize] > 0 {
                    if offssymbol > 9 {
                        if OFFSETBITS[offssymbol as usize] > 4 {
                            let nb = (OFFSETBITS[offssymbol as usize] - 4) as i32;
                            if !self.stream.br_read_ahead(nb) {
                                self.valid = false;
                                return Err(Error::InvalidData("truncated RAR file data"));
                            }
                            o += self.stream.br_bits(nb) << 4;
                            self.stream.br_consume(nb);
                        }
                        if self.numlowoffsetrepeats > 0 {
                            self.numlowoffsetrepeats -= 1;
                            o += self.lastlowoffset;
                        } else {
                            let lowoffsetsymbol = self
                                .lowoffsetcode
                                .read_next_symbol(&mut self.stream)
                                .map_err(|e| match e {
                                    SymError::Truncated => {
                                        self.valid = false;
                                        Error::InvalidData("truncated RAR file data")
                                    }
                                    SymError::BadCode => Error::InvalidData("bad RAR file data"),
                                })?;
                            if lowoffsetsymbol < 0 {
                                return Err(Error::InvalidData("bad RAR file data"));
                            }
                            if lowoffsetsymbol == 16 {
                                self.numlowoffsetrepeats = 15;
                                o += self.lastlowoffset;
                            } else {
                                o += lowoffsetsymbol as u32;
                                self.lastlowoffset = lowoffsetsymbol as u32;
                            }
                        }
                    } else {
                        let nb = OFFSETBITS[offssymbol as usize] as i32;
                        if !self.stream.br_read_ahead(nb) {
                            self.valid = false;
                            return Err(Error::InvalidData("truncated RAR file data"));
                        }
                        o += self.stream.br_bits(nb);
                        self.stream.br_consume(nb);
                    }
                }
                if o >= 0x40000 {
                    l += 1;
                }
                if o >= 0x2000 {
                    l += 1;
                }
                for i in (1..4).rev() {
                    self.oldoffset[i] = self.oldoffset[i - 1];
                }
                self.oldoffset[0] = o;
                offs = o;
                len = l;
            }

            self.lastoffset = offs;
            self.lastlength = len;
            self.lzss.emit_match(offs as usize, len as usize);
        }
    }

    /// `parse_codes`: read a new Huffman table block or PPMd block header.
    fn parse_codes(&mut self) -> Result<(), Error> {
        self.free_codes();
        self.stream.br_consume_unaligned_bits();

        if !self.stream.br_read_ahead(1) {
            self.valid = false;
            return Err(Error::InvalidData("truncated RAR file data"));
        }
        self.is_ppmd_block = self.stream.br_bits(1) != 0;

        if self.is_ppmd_block {
            self.stream.br_consume(1);
            if !self.stream.br_read_ahead(7) {
                self.valid = false;
                return Err(Error::InvalidData("truncated RAR file data"));
            }
            let ppmd_flags = self.stream.br_bits(7) as u8;
            self.stream.br_consume(7);

            if ppmd_flags & 0x20 != 0 {
                if !self.stream.br_read_ahead(8) {
                    self.valid = false;
                    return Err(Error::InvalidData("truncated RAR file data"));
                }
                self.dictionary_size = (self.stream.br_bits(8) + 1) << 20;
                self.stream.br_consume(8);
            }

            if ppmd_flags & 0x40 != 0 {
                if !self.stream.br_read_ahead(8) {
                    self.valid = false;
                    return Err(Error::InvalidData("truncated RAR file data"));
                }
                self.ppmd_escape = self.stream.br_bits(8) as i32;
                self.ppmd.set_init_esc(self.ppmd_escape as u32);
                self.stream.br_consume(8);
            } else {
                self.ppmd_escape = 2;
            }

            if ppmd_flags & 0x20 != 0 {
                let mut maxorder = (ppmd_flags & 0x1F) as u32 + 1;
                if maxorder > 16 {
                    maxorder = 16 + (maxorder - 16) * 3;
                }
                if maxorder == 1 {
                    return Err(Error::InvalidData("truncated RAR file data"));
                }
                self.ppmd.free();
                self.ppmd.construct();
                if self.dictionary_size == 0 {
                    return Err(Error::InvalidData("invalid zero dictionary size"));
                }
                if !self.ppmd.alloc(self.dictionary_size) {
                    return Err(Error::InvalidData("out of memory"));
                }
                let ok = {
                    let stream = &mut self.stream;
                    let valid = &mut self.valid;
                    let mut read_fn = move || -> u8 {
                        match stream.br_pull_byte() {
                            Some(b) => b,
                            None => {
                                *valid = false;
                                0
                            }
                        }
                    };
                    self.range_dec.init_rar(&mut read_fn)
                };
                if !ok {
                    return Err(Error::InvalidData(
                        "unable to initialize PPMd range decoder",
                    ));
                }
                self.ppmd.init(maxorder);
                self.ppmd_valid = true;
            } else {
                if !self.ppmd_valid {
                    return Err(Error::InvalidData("invalid PPMd sequence"));
                }
                let ok = {
                    let stream = &mut self.stream;
                    let valid = &mut self.valid;
                    let mut read_fn = move || -> u8 {
                        match stream.br_pull_byte() {
                            Some(b) => b,
                            None => {
                                *valid = false;
                                0
                            }
                        }
                    };
                    self.range_dec.init_rar(&mut read_fn)
                };
                if !ok {
                    return Err(Error::InvalidData(
                        "unable to initialize PPMd range decoder",
                    ));
                }
            }
        } else {
            self.stream.br_consume(1);
            self.lastlowoffset = 0;
            self.numlowoffsetrepeats = 0;

            if !self.stream.br_read_ahead(1) {
                self.valid = false;
                return Err(Error::InvalidData("truncated RAR file data"));
            }
            if self.stream.br_bits(1) == 0 {
                self.lengthtable = [0; HUFFMAN_TABLE_SIZE];
            }
            self.stream.br_consume(1);

            let mut bitlengths = [0u8; MAX_SYMBOLS];
            let mut i = 0usize;
            while i < MAX_SYMBOLS {
                if !self.stream.br_read_ahead(4) {
                    self.valid = false;
                    return Err(Error::InvalidData("truncated RAR file data"));
                }
                bitlengths[i] = self.stream.br_bits(4) as u8;
                i += 1;
                self.stream.br_consume(4);
                if bitlengths[i - 1] == 0xF {
                    if !self.stream.br_read_ahead(4) {
                        self.valid = false;
                        return Err(Error::InvalidData("truncated RAR file data"));
                    }
                    let zerocount = self.stream.br_bits(4) as u8;
                    self.stream.br_consume(4);
                    if zerocount != 0 {
                        i -= 1;
                        for _ in 0..zerocount + 2 {
                            if i >= MAX_SYMBOLS {
                                break;
                            }
                            bitlengths[i] = 0;
                            i += 1;
                        }
                    }
                }
            }

            let mut precode = HuffmanCode::default();
            create_code(&mut precode, &bitlengths, MAX_SYMBOL_LENGTH)?;

            i = 0;
            while i < HUFFMAN_TABLE_SIZE {
                let val = precode
                    .read_next_symbol(&mut self.stream)
                    .map_err(|e| match e {
                        SymError::Truncated => {
                            self.valid = false;
                            Error::InvalidData("truncated RAR file data")
                        }
                        SymError::BadCode => Error::InvalidData("bad RAR file data"),
                    })?;
                if val < 16 {
                    self.lengthtable[i] = (self.lengthtable[i] + val as u8) & 0xF;
                    i += 1;
                } else if val < 18 {
                    if i == 0 {
                        return Err(Error::InvalidData("internal error extracting RAR file"));
                    }
                    let n = if val == 16 {
                        if !self.stream.br_read_ahead(3) {
                            self.valid = false;
                            return Err(Error::InvalidData("truncated RAR file data"));
                        }
                        let n = self.stream.br_bits(3) + 3;
                        self.stream.br_consume(3);
                        n
                    } else {
                        if !self.stream.br_read_ahead(7) {
                            self.valid = false;
                            return Err(Error::InvalidData("truncated RAR file data"));
                        }
                        let n = self.stream.br_bits(7) + 11;
                        self.stream.br_consume(7);
                        n
                    };
                    for _ in 0..n {
                        if i >= HUFFMAN_TABLE_SIZE {
                            break;
                        }
                        self.lengthtable[i] = self.lengthtable[i - 1];
                        i += 1;
                    }
                } else {
                    let n = if val == 18 {
                        if !self.stream.br_read_ahead(3) {
                            self.valid = false;
                            return Err(Error::InvalidData("truncated RAR file data"));
                        }
                        let n = self.stream.br_bits(3) + 3;
                        self.stream.br_consume(3);
                        n
                    } else {
                        if !self.stream.br_read_ahead(7) {
                            self.valid = false;
                            return Err(Error::InvalidData("truncated RAR file data"));
                        }
                        let n = self.stream.br_bits(7) + 11;
                        self.stream.br_consume(7);
                        n
                    };
                    for _ in 0..n {
                        if i >= HUFFMAN_TABLE_SIZE {
                            break;
                        }
                        self.lengthtable[i] = 0;
                        i += 1;
                    }
                }
            }

            create_code(
                &mut self.maincode,
                &self.lengthtable[0..MAINCODE_SIZE],
                MAX_SYMBOL_LENGTH,
            )?;
            create_code(
                &mut self.offsetcode,
                &self.lengthtable[MAINCODE_SIZE..MAINCODE_SIZE + OFFSETCODE_SIZE],
                MAX_SYMBOL_LENGTH,
            )?;
            create_code(
                &mut self.lowoffsetcode,
                &self.lengthtable[MAINCODE_SIZE + OFFSETCODE_SIZE
                    ..MAINCODE_SIZE + OFFSETCODE_SIZE + LOWOFFSETCODE_SIZE],
                MAX_SYMBOL_LENGTH,
            )?;
            create_code(
                &mut self.lengthcode,
                &self.lengthtable[MAINCODE_SIZE + OFFSETCODE_SIZE + LOWOFFSETCODE_SIZE..],
                MAX_SYMBOL_LENGTH,
            )?;
        }

        if self.dictionary_size == 0
            || self.lzss.window.is_empty()
            || (self.lzss.mask + 1) < self.dictionary_size as usize
        {
            let new_size = if self.unp_size as u32 >= DICTIONARY_MAX_SIZE {
                DICTIONARY_MAX_SIZE
            } else {
                rar_fls(self.unp_size as u32) << 1
            };
            if new_size == 0 {
                return Err(Error::InvalidData("zero window size is invalid"));
            }
            self.lzss.window = vec![0; new_size as usize];
            self.dictionary_size = new_size;
            self.lzss.mask = new_size as usize - 1;
        }

        self.start_new_table = false;
        Ok(())
    }

    fn free_codes(&mut self) {
        self.maincode = HuffmanCode::default();
        self.offsetcode = HuffmanCode::default();
        self.lowoffsetcode = HuffmanCode::default();
        self.lengthcode = HuffmanCode::default();
    }
}
