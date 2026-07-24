//! Integration test framework: USB trace replay.
//!
//! Real hardware is not required — tests drive a [`MockUsbDevice`] that reads
//! pre-recorded request/response pairs from JSON fixture files under
//! `tests/fixtures/`.
//!
//! # Fixture format
//!
//! ```json
//! [
//!   { "dir": "out", "data": "2800000000000000" },
//!   { "dir": "in",  "data": "28010000000000000000000000000000" }
//! ]
//! ```
//!
//! Each entry is a hex-encoded USB bulk transfer.  `"out"` entries are
//! expected requests from the library; `"in"` entries are simulated device
//! responses.  The test fails if an `"out"` packet does not match the next
//! expected entry.
//!
//! # Recording real traces
//!
//! On Linux, enable `nusb`'s debug logging (`RUST_LOG=nusb=debug`) and capture
//! `usbmon` output.  A helper script `tools/record_trace.py` converts usbmon
//! pcap files into the JSON fixture format.
//!
//! # Running
//!
//! ```
//! cargo test --test integration
//! ```

use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};

// ── Fixture types ─────────────────────────────────────────────────────────────

/// A single USB bulk transfer recorded in a fixture file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEntry {
    /// `"in"` (device→host) or `"out"` (host→device).
    pub dir: String,
    /// Hex-encoded byte payload (no spaces, no `0x` prefix).
    pub data: String,
}

/// A complete recorded session loaded from a fixture file.
#[derive(Debug, Clone)]
pub struct Fixture {
    pub entries: Vec<TraceEntry>,
}

impl Fixture {
    /// Load a fixture from a JSON file under `tests/fixtures/`.
    #[allow(dead_code)]
    pub fn load(name: &str) -> Self {
        let path = fixture_path(name);
        let json = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read fixture {}: {}", path.display(), e));
        let entries: Vec<TraceEntry> = serde_json::from_str(&json)
            .unwrap_or_else(|e| panic!("bad fixture JSON in {}: {}", path.display(), e));
        Fixture { entries }
    }

    /// Save a fixture (used by the recording helper, not by tests).
    #[allow(dead_code)]
    pub fn save(&self, name: &str) {
        let path = fixture_path(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("cannot create fixture directory");
        }
        let json = serde_json::to_string_pretty(&self.entries).expect("cannot serialize fixture");
        fs::write(&path, json).unwrap_or_else(|e| panic!("cannot write fixture: {}", e));
    }
}

#[allow(dead_code)]
fn fixture_path(name: &str) -> PathBuf {
    // CARGO_MANIFEST_DIR points at the crate being tested (minipro-core).
    // Fixtures live two levels up in the workspace tests/fixtures/ directory.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .unwrap() // crates/
        .parent()
        .unwrap() // workspace root
        .join("tests")
        .join("fixtures")
        .join(name)
        .with_extension("json")
}

// ── Mock USB device ───────────────────────────────────────────────────────────

/// A replay USB device backed by a pre-recorded [`Fixture`].
///
/// Feed this into protocol functions in tests by replacing the real
/// [`minipro_core::usb::UsbDevice`] with the mock's `send`/`recv` closures.
///
/// The mock keeps a cursor into the fixture entry list.  Each `send` call
/// asserts that the outgoing bytes match the next `"out"` entry; each `recv`
/// call returns the bytes from the next `"in"` entry.
#[derive(Debug)]
pub struct MockUsb {
    entries: Vec<TraceEntry>,
    cursor: Mutex<usize>,
}

impl MockUsb {
    pub fn new(fixture: Fixture) -> Arc<Self> {
        Arc::new(MockUsb {
            entries: fixture.entries,
            cursor: Mutex::new(0),
        })
    }

    /// Assert that `data` matches the next `"out"` entry in the fixture.
    pub fn expect_send(&self, data: &[u8]) {
        let mut cur = self.cursor.lock().unwrap();
        assert!(
            *cur < self.entries.len(),
            "MockUsb: send called but fixture has no more entries (cursor {})",
            *cur
        );
        let entry = &self.entries[*cur];
        assert_eq!(
            entry.dir, "out",
            "MockUsb: expected 'out' entry at cursor {}, got '{}'",
            *cur, entry.dir
        );
        let expected = hex_decode(&entry.data);
        assert_eq!(
            data,
            expected.as_slice(),
            "MockUsb: outgoing packet mismatch at cursor {}\n  got      {}\n  expected {}",
            *cur,
            hex_encode(data),
            entry.data,
        );
        *cur += 1;
    }

    /// Return the bytes of the next `"in"` entry in the fixture.
    pub fn provide_recv(&self) -> Vec<u8> {
        let mut cur = self.cursor.lock().unwrap();
        assert!(
            *cur < self.entries.len(),
            "MockUsb: recv called but fixture has no more entries (cursor {})",
            *cur
        );
        let entry = &self.entries[*cur];
        assert_eq!(
            entry.dir, "in",
            "MockUsb: expected 'in' entry at cursor {}, got '{}'",
            *cur, entry.dir
        );
        let data = hex_decode(&entry.data);
        *cur += 1;
        data
    }

    /// Assert that all fixture entries were consumed.
    pub fn assert_complete(&self) {
        let cur = *self.cursor.lock().unwrap();
        assert_eq!(
            cur,
            self.entries.len(),
            "MockUsb: only {} of {} fixture entries were consumed",
            cur,
            self.entries.len()
        );
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

pub fn hex_encode(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn hex_decode(s: &str) -> Vec<u8> {
    assert!(s.len() % 2 == 0, "odd-length hex string: {s}");
    (0..s.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&s[i..i + 2], 16)
                .unwrap_or_else(|_| panic!("invalid hex byte at {i}: {}", &s[i..i + 2]))
        })
        .collect()
}
