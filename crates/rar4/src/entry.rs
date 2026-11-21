//! Public entry metadata returned by `Archive::next_entry`.

/// A timestamp as stored in the archive.
///
/// RAR4 stores DOS-format local times plus optional 100 ns extended
/// fractions. This crate reports the DOS time interpreted as UTC, matching
/// libarchive's field layout (see INTERNALS.md for the local-time caveat).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timestamp {
    /// Seconds since the UNIX epoch (UTC interpretation of the DOS field).
    pub secs: i64,
    /// Sub-second part in nanoseconds as computed by the RAR4 ext-time
    /// record (libarchive semantics).
    pub nsecs: u32,
}

impl Timestamp {
    pub(crate) fn new(t: (i64, u32)) -> Option<Self> {
        if t.0 == 0 {
            None
        } else {
            Some(Self {
                secs: t.0,
                nsecs: t.1,
            })
        }
    }
}

/// Metadata for one archive entry (file, directory, or symlink).
///
/// A split entry in a multi-volume archive may be reported more than once —
/// once per volume part — like libarchive's `archive_read_next_header`.
#[derive(Clone, Debug)]
pub struct Entry {
    /// Path within the archive, `/`-separated (lossily converted to UTF-8).
    pub name: String,
    /// Name bytes exactly as stored in the header (pre-decode form).
    pub raw_name: Vec<u8>,
    /// Uncompressed size in bytes (0 for symlinks).
    pub size: u64,
    /// Packed size of this part in the archive.
    pub packed_size: u64,
    /// Expected CRC-32 of the decompressed data.
    pub crc32: u32,
    /// Entry is a directory (no data follows).
    pub is_directory: bool,
    /// Entry is a symbolic link; `link_target` holds the target bytes.
    pub is_symlink: bool,
    /// Symlink target bytes, when `is_symlink` is set.
    pub link_target: Option<Vec<u8>>,
    /// Host-OS file mode (Unix mode bits for Unix-hosted archives).
    pub mode: u32,
    /// RAR compression method byte: `0x30` stored, `0x31`–`0x35` compressed.
    pub compression_method: u8,
    /// Modification time, when stored in the header.
    pub mtime: Option<Timestamp>,
    /// Creation time, when stored in the header (RAR4 ext-time record).
    pub ctime: Option<Timestamp>,
    /// Access time, when stored in the header (RAR4 ext-time record).
    pub atime: Option<Timestamp>,
    /// Archive creation time, when stored in the header.
    pub arctime: Option<Timestamp>,
}

impl Entry {
    /// Symlink target interpreted as a `/`-separated path string.
    pub fn link_target_str(&self) -> Option<String> {
        self.link_target
            .as_ref()
            .map(|t| String::from_utf8_lossy(t).replace('\\', "/"))
    }
}
