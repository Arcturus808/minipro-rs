// Integration tests — USB trace replay
//
// These tests do not require physical hardware.  Each test loads a JSON
// fixture from tests/fixtures/ and drives the library against a mock USB
// device.
//
// To add a new test:
//   1. Record a real trace (see framework.rs header for instructions) and save
//      it as tests/fixtures/<name>.json.
//   2. Write a #[test] fn that calls MockUsb::new(Fixture::load("<name>"))
//      and passes the mock into the relevant operation under test.
//
// Current tests are "skeleton" tests that demonstrate the fixture machinery
// without needing a real programmer or fixture files.

mod framework;
use framework::{hex_encode, Fixture, MockUsb, TraceEntry};

// ── Fixture round-trip ────────────────────────────────────────────────────────

/// Verify that fixtures can be serialized and deserialized without loss.
#[test]
fn fixture_round_trip() {
    let entries = vec![
        TraceEntry {
            dir: "out".into(),
            data: "0300000000000000".into(),
        },
        TraceEntry {
            dir: "in".into(),
            data: "0300000000000000".into(),
        },
    ];
    let fixture = Fixture {
        entries: entries.clone(),
    };

    let json = serde_json::to_string(&fixture.entries).unwrap();
    let back: Vec<TraceEntry> = serde_json::from_str(&json).unwrap();
    assert_eq!(back.len(), 2);
    assert_eq!(back[0].dir, "out");
    assert_eq!(back[0].data, "0300000000000000");
    assert_eq!(back[1].dir, "in");
}

// ── MockUsb ───────────────────────────────────────────────────────────────────

/// Verify the mock correctly matches outgoing packets.
#[test]
fn mock_usb_send_matches() {
    let fixture = Fixture {
        entries: vec![
            TraceEntry {
                dir: "out".into(),
                data: "deadbeef".into(),
            },
            TraceEntry {
                dir: "in".into(),
                data: "cafebabe".into(),
            },
        ],
    };
    let mock = MockUsb::new(fixture);
    mock.expect_send(&[0xde, 0xad, 0xbe, 0xef]);
    let resp = mock.provide_recv();
    assert_eq!(resp, &[0xca, 0xfe, 0xba, 0xbe]);
    mock.assert_complete();
}

/// Verify the mock panics when an unexpected packet is sent.
#[test]
#[should_panic(expected = "outgoing packet mismatch")]
fn mock_usb_send_mismatch_panics() {
    let fixture = Fixture {
        entries: vec![TraceEntry {
            dir: "out".into(),
            data: "01020304".into(),
        }],
    };
    let mock = MockUsb::new(fixture);
    mock.expect_send(&[0xff, 0xff, 0xff, 0xff]); // wrong bytes → panic
}

// ── hex helpers ───────────────────────────────────────────────────────────────

#[test]
fn hex_encode_decode_roundtrip() {
    let data = [0x00u8, 0x1a, 0xff, 0x80, 0x3c];
    let encoded = hex_encode(&data);
    assert_eq!(encoded, "001aff803c");
    let decoded = framework::hex_decode(&encoded);
    assert_eq!(decoded, data);
}
