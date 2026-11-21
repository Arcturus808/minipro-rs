//! RAR4 header field parsing: file headers, filename decoding, timestamps.
//!
//! Port of `read_header` field handling, `read_exttime`, `get_time`, and the
//! FHD_UNICODE filename decoding from `archive_read_support_format_rar.c`.

use crate::error::Error;
use crate::stream::{FHD_LARGE, FHD_UNICODE};

pub(crate) const OS_MSDOS: u8 = 0;
pub(crate) const OS_OS2: u8 = 1;
pub(crate) const OS_WIN32: u8 = 2;
pub(crate) const OS_UNIX: u8 = 3;
pub(crate) const OS_MAC_OS: u8 = 4;
pub(crate) const OS_BEOS: u8 = 5;

const NS_UNIT: i64 = 10_000_000;

/// Decoded fields of a `FILE_HEAD`/`NEWSUB_HEAD` block body (the 21-byte
/// `rar_file_header` plus its variable tail).
#[derive(Debug)]
pub(crate) struct FileHeader {
    /// Raw name bytes exactly as stored in the header (pre-decode).
    pub name_raw: Vec<u8>,
    /// Decoded filename bytes: UTF-16BE when `unicode_decoded`, else the
    /// stored bytes with '\\' unchanged.
    pub name_decoded: Vec<u8>,
    /// True when `name_decoded` holds UTF-16BE units.
    pub unicode_decoded: bool,
    pub pack_size: i64,
    pub unp_size: i64,
    pub host_os: u8,
    pub file_crc: u32,
    pub file_time: u32,
    pub method: u8,
    pub attr: u32,
    /// Offset range within the header body still unread after the name
    /// (salt / extended times live there), as offsets into `body`.
    pub tail: (usize, usize),
}

fn le16(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}
fn le32(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

/// Parse a file-header body (`header_size - 7` bytes following the common
/// 7-byte header). `flags`/`head_type` come from the common header.
/// Port of the field walk in `read_header`.
pub(crate) fn parse_file_header_body(body: &[u8], flags: u16) -> Result<FileHeader, Error> {
    // rar_file_header is 25 bytes.
    if body.len() < 25 {
        return Err(Error::InvalidHeader("file header too small"));
    }
    let endp = body.len();

    let pack_size = le32(body, 0);
    let unp_size = le32(body, 4);
    let host_os = body[8];
    let file_crc = le32(body, 9);
    let file_time = le32(body, 13);
    let _unp_ver = body[17];
    let method = body[18];
    // rar_file_header: pack(4) unp(4) host(1) crc(4) time(4) ver(1) method(1)
    //   name_size(2) attr(4) = 25 bytes
    let name_size = le16(body, 19) as usize;
    let attr = le32(body, 21);
    let mut p = 25usize;

    let (pack_size, unp_size) = if flags & FHD_LARGE != 0 {
        if p + 8 > endp {
            return Err(Error::InvalidHeader("invalid header size"));
        }
        let hi_pack = le32(body, p) as u64;
        let hi_unp = le32(body, p + 4) as u64;
        p += 8;
        (
            ((hi_pack << 32) | pack_size as u64) as i64,
            ((hi_unp << 32) | unp_size as u64) as i64,
        )
    } else {
        (pack_size as i64, unp_size as i64)
    };
    if pack_size < 0 || unp_size < 0 {
        return Err(Error::InvalidHeader("invalid sizes specified"));
    }

    if p + name_size > endp {
        return Err(Error::InvalidHeader("invalid filename size"));
    }
    let name_raw = body[p..p + name_size].to_vec();
    let (name_decoded, unicode_decoded, consumed) = decode_name(&body[p..p + name_size], flags)?;
    p += consumed;

    Ok(FileHeader {
        name_raw,
        name_decoded,
        unicode_decoded,
        pack_size,
        unp_size,
        host_os,
        file_crc,
        file_time,
        method,
        attr,
        tail: (p, endp),
    })
}

/// Port of the FHD_UNICODE filename decode from `read_header`.
///
/// Returns `(decoded_bytes, is_utf16be, bytes_consumed_of_raw)`.
fn decode_name(raw: &[u8], flags: u16) -> Result<(Vec<u8>, bool, usize), Error> {
    if flags & FHD_UNICODE == 0 {
        return Ok((raw.to_vec(), false, raw.len()));
    }
    // The plain part ends at the first NUL; if the whole name has no NUL the
    // name is UTF-8 and there is no extra unicode payload.
    let nul = raw.iter().position(|&b| b == 0);
    if nul.is_none() {
        return Ok((raw.to_vec(), false, raw.len()));
    }
    let nul = nul.unwrap();

    let mut decoded: Vec<u8> = Vec::with_capacity(raw.len() * 2 + 2);
    let end = raw.len();
    let fn_end = raw.len() * 2;
    let mut offset = nul + 1;
    let highbyte = if offset >= end { 0 } else { raw[offset] };
    if offset < end {
        offset += 1;
    }
    let mut flagbits = 0u32;
    let mut flagbyte = 0u8;
    while offset < end && decoded.len() < fn_end {
        if flagbits == 0 {
            flagbyte = raw[offset];
            offset += 1;
            flagbits = 8;
        }
        flagbits -= 2;
        match (flagbyte >> flagbits) & 3 {
            0 => {
                if offset >= end {
                    continue;
                }
                decoded.push(0);
                decoded.push(raw[offset]);
                offset += 1;
            }
            1 => {
                if offset >= end {
                    continue;
                }
                decoded.push(highbyte);
                decoded.push(raw[offset]);
                offset += 1;
            }
            2 => {
                if offset + 1 >= end {
                    offset = end;
                    continue;
                }
                decoded.push(raw[offset + 1]);
                decoded.push(raw[offset]);
                offset += 2;
            }
            _ => {
                if offset >= end {
                    continue;
                }
                let mut length = raw[offset];
                offset += 1;
                let (extra, high);
                if length & 0x80 != 0 {
                    if offset >= end {
                        continue;
                    }
                    extra = raw[offset];
                    offset += 1;
                    high = highbyte;
                } else {
                    extra = 0;
                    high = 0;
                }
                length = (length & 0x7f) + 2;
                while length > 0 && decoded.len() < fn_end {
                    let cp = decoded.len() >> 1;
                    decoded.push(high);
                    decoded.push(raw[cp].wrapping_add(extra));
                    length -= 1;
                }
            }
        }
    }
    if decoded.len() > fn_end {
        return Err(Error::InvalidHeader("invalid filename"));
    }
    Ok((decoded, true, offset))
}

/// Convert decoded name bytes to the display path: UTF-16BE→String or
/// lossy UTF-8, then '\\' → '/'.
pub(crate) fn name_to_string(decoded: &[u8], utf16: bool) -> String {
    let s = if utf16 {
        let units: Vec<u16> = decoded
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(decoded).into_owned()
    };
    s.replace('\\', "/")
}

/// DOS date-time → seconds since UNIX epoch, computed as if the fields were
/// UTC. libarchive used `mktime` (local time); we keep a deterministic UTC
/// interpretation — see INTERNALS.md.
pub(crate) fn dos_time_to_unix(ttime: u32) -> i64 {
    let sec = (ttime & 0x1f) * 2;
    let min = (ttime >> 5) & 0x3f;
    let hour = (ttime >> 11) & 0x1f;
    let day = (ttime >> 16) & 0x1f;
    let mon = ((ttime >> 21) & 0x0f) as i64;
    let year = ((ttime >> 25) & 0x7f) as i64 + 1980;
    days_from_civil(year, mon, day as i64) * 86_400
        + hour as i64 * 3600
        + min as i64 * 60
        + sec as i64
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Decoded extended times (`read_exttime`). Slots: mtime, ctime, atime,
/// arctime — each `(secs, nsec)` updated only when its flag says present.
pub(crate) fn read_exttime(
    body: &[u8],
    tail: (usize, usize),
    mtime: &mut (i64, u32),
    ctime: &mut (i64, u32),
    atime: &mut (i64, u32),
    arctime: &mut (i64, u32),
) -> Result<(), Error> {
    let (mut p, endp) = tail;
    if p + 2 > endp {
        return Err(Error::InvalidHeader("invalid header size"));
    }
    let flags = le16(body, p);
    p += 2;

    for i in (0..4).rev() {
        let mut t: i64 = 0;
        if i == 3 {
            t = mtime.0;
        }
        let rmode = flags >> (i * 4);
        if rmode & 8 == 0 {
            continue;
        }
        if t == 0 {
            if p + 4 > endp {
                return Err(Error::InvalidHeader("invalid header size"));
            }
            t = dos_time_to_unix(le32(body, p));
            p += 4;
        }
        let mut rem: u32 = 0;
        let count = rmode & 3;
        if p + count as usize > endp {
            return Err(Error::InvalidHeader("invalid header size"));
        }
        for _ in 0..count {
            rem = ((body[p] as u32) << 16) | (rem >> 8);
            p += 1;
        }
        // Whole seconds embedded in t; rem is in 100ns units. nsec = seconds
        // part + rem/NS_UNIT as in libarchive (tm_sec + rem/NS_UNIT).
        let sec_part = t.rem_euclid(60);
        let nsec = (sec_part + rem as i64 / NS_UNIT) as u32;
        let t = if rmode & 4 != 0 { t + 1 } else { t };
        let slot = match i {
            3 => &mut *mtime,
            2 => &mut *ctime,
            1 => &mut *atime,
            _ => &mut *arctime,
        };
        *slot = (t, nsec);
    }
    Ok(())
}
