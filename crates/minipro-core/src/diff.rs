//! Smart firmware diff — byte-aligned comparison with three-way tail classification.
//!
//! Compares two binary buffers (e.g., a chip dump and a reference file) and
//! classifies differences into:
//! - **Differing bytes** — both buffers have data at this offset, but values differ
//! - **Padding-tail** — offsets beyond the shorter buffer where the longer buffer
//!   is all erase-value (benign, not a real diff)
//! - **Anomalous-tail** — offsets beyond the shorter buffer where the longer buffer
//!   has non-erase-value data (real problem: truncated read, wrong chip, or
//!   leftover data from previous programming)
//!
//! The erase value is configurable per device (NOR flash erases to `0xFF`,
//! some EEPROM/NAND erase to `0x00`).

use serde::Serialize;

/// A single byte that differs between the two buffers.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct DiffEntry {
    /// Offset where the difference was found.
    pub offset: usize,
    /// Value in buffer A (the "left" / chip-dump side).
    pub value_a: u8,
    /// Value in buffer B (the "right" / reference-file side).
    pub value_b: u8,
}

/// Classification of a tail region — bytes that exist in one buffer but not the other.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum TailKind {
    /// The longer buffer is all erase-value in this range — benign padding.
    Padding,
    /// The longer buffer has non-erase-value data in this range — real problem.
    Anomalous,
}

/// A contiguous tail region beyond the shorter buffer's length.
#[derive(Debug, Clone, Serialize)]
pub struct TailRegion {
    /// Start offset (inclusive) — equals the length of the shorter buffer.
    pub start: usize,
    /// End offset (exclusive) — equals the length of the longer buffer.
    pub end: usize,
    /// Whether this region is benign padding or anomalous data.
    pub kind: TailKind,
    /// Which buffer is longer ("a" or "b").
    pub longer_side: char,
}

/// Summary counts for a diff result.
#[derive(Debug, Clone, Serialize)]
pub struct DiffSummary {
    /// Total number of differing bytes.
    pub diff_count: usize,
    /// Number of contiguous regions where bytes differ.
    pub diff_regions: usize,
    /// Size of buffer A.
    pub size_a: usize,
    /// Size of buffer B.
    pub size_b: usize,
    /// Tail regions classified as padding (benign).
    pub padding_tail: usize,
    /// Tail regions classified as anomalous (real problem).
    pub anomalous_tail: usize,
    /// True if no real differences (ignoring benign padding).
    pub is_equal: bool,
}

/// Complete result of a smart diff comparison.
#[derive(Debug, Clone, Serialize)]
pub struct DiffResult {
    /// List of differing bytes (only in the compared region where both have data).
    pub diffs: Vec<DiffEntry>,
    /// Classified tail regions beyond the shorter buffer.
    pub tails: Vec<TailRegion>,
    /// Summary counts.
    pub summary: DiffSummary,
}

/// Compare two buffers with intelligent trailing-padding handling.
///
/// - `a` is typically the chip dump (left side).
/// - `b` is typically the reference file (right side).
/// - `erase_value` is the blank/erase value for the memory type
///   (`0xFF` for NOR flash, `0x00` for some EEPROM/NAND).
///
/// The comparison is byte-aligned at matching offsets. Trailing bytes in the
/// longer buffer are classified as either padding (all erase-value) or anomalous
/// (contains real data). See the [module docs](self) for details.
pub fn smart_diff(a: &[u8], b: &[u8], erase_value: u8) -> DiffResult {
    let min_len = a.len().min(b.len());
    let max_len = a.len().max(b.len());

    // ── Compared region: byte-by-byte diff ──────────────────────────────────
    let mut diffs = Vec::new();
    for i in 0..min_len {
        if a[i] != b[i] {
            diffs.push(DiffEntry {
                offset: i,
                value_a: a[i],
                value_b: b[i],
            });
        }
    }

    // ── Tail region classification ──────────────────────────────────────────
    let mut tails = Vec::new();
    let longer_side = if a.len() >= b.len() { 'a' } else { 'b' };
    let longer = if a.len() >= b.len() { a } else { b };

    if max_len > min_len {
        let tail = &longer[min_len..max_len];
        let is_all_padding = tail.iter().all(|&v| v == erase_value);

        let kind = if is_all_padding {
            TailKind::Padding
        } else {
            TailKind::Anomalous
        };

        tails.push(TailRegion {
            start: min_len,
            end: max_len,
            kind,
            longer_side,
        });
    }

    // ── Summary ─────────────────────────────────────────────────────────────
    let diff_count = diffs.len();
    let diff_regions = count_contiguous_regions(&diffs);
    let padding_tail = tails.iter().filter(|t| t.kind == TailKind::Padding).count();
    let anomalous_tail = tails
        .iter()
        .filter(|t| t.kind == TailKind::Anomalous)
        .count();
    let is_equal = diffs.is_empty() && anomalous_tail == 0;

    DiffResult {
        diffs,
        tails,
        summary: DiffSummary {
            diff_count,
            diff_regions,
            size_a: a.len(),
            size_b: b.len(),
            padding_tail,
            anomalous_tail,
            is_equal,
        },
    }
}

/// Count the number of contiguous regions in a sorted list of diff entries.
fn count_contiguous_regions(diffs: &[DiffEntry]) -> usize {
    if diffs.is_empty() {
        return 0;
    }
    let mut regions = 1;
    for w in diffs.windows(2) {
        if w[1].offset != w[0].offset + 1 {
            regions += 1;
        }
    }
    regions
}

/// Format a [`DiffResult`] as a human-readable table for CLI output.
pub fn format_diff_report(result: &DiffResult, erase_value: u8) -> String {
    let s = &result.summary;
    let mut out = String::new();

    // ── Size summary ────────────────────────────────────────────────────────
    out.push_str(&format!(
        "Buffer A: {} bytes  |  Buffer B: {} bytes  |  Erase value: 0x{:02X}\n",
        s.size_a, s.size_b, erase_value
    ));

    if s.size_a != s.size_b {
        let delta = s.size_b as i64 - s.size_a as i64;
        out.push_str(&format!("Size delta: {delta:+} bytes (B vs A)\n"));
    }

    // ── Tail classification ─────────────────────────────────────────────────
    for tail in &result.tails {
        match tail.kind {
            TailKind::Padding => {
                out.push_str(&format!(
                    "Tail [0x{:X}..0x{:X}] in buffer {}: all erase-value (0x{:02X}) — benign padding, ignored\n",
                    tail.start, tail.end, tail.longer_side, erase_value
                ));
            }
            TailKind::Anomalous => {
                out.push_str(&format!(
                    "WARNING: Tail [0x{:X}..0x{:X}] in buffer {} contains non-erase-value data — possible truncated read, wrong chip, or leftover data from previous programming\n",
                    tail.start, tail.end, tail.longer_side
                ));
            }
        }
    }

    // ── Diff summary ────────────────────────────────────────────────────────
    if s.is_equal {
        out.push_str("\nFiles match (ignoring trailing padding).\n");
    } else if s.diff_count == 0 {
        out.push_str(&format!(
            "\nNo byte diffs in compared region, but {} anomalous tail region(s) detected.\n",
            s.anomalous_tail
        ));
    } else {
        out.push_str(&format!(
            "\n{} byte diff(s) across {} region(s)\n",
            s.diff_count, s.diff_regions
        ));
        out.push('\n');
        out.push_str("  Offset    Expected (B)   Actual (A)\n");
        out.push_str("  --------   -----------   ----------\n");
        for d in &result.diffs {
            out.push_str(&format!(
                "  0x{:08X}   0x{:02X}           0x{:02X}\n",
                d.offset, d.value_b, d.value_a
            ));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identical_buffers() {
        let a = [0x01, 0x02, 0x03, 0xFF, 0xFF];
        let b = [0x01, 0x02, 0x03, 0xFF, 0xFF];
        let result = smart_diff(&a, &b, 0xFF);
        assert!(result.summary.is_equal);
        assert!(result.diffs.is_empty());
        assert!(result.tails.is_empty());
    }

    #[test]
    fn test_diff_with_trailing_padding_in_b() {
        // A is 3 bytes of code, B is 5 bytes (3 code + 2 padding)
        let a = [0x01, 0x02, 0x03];
        let b = [0x01, 0x02, 0x03, 0xFF, 0xFF];
        let result = smart_diff(&a, &b, 0xFF);
        assert!(result.summary.is_equal);
        assert!(result.diffs.is_empty());
        assert_eq!(result.tails.len(), 1);
        assert_eq!(result.tails[0].kind, TailKind::Padding);
        assert_eq!(result.tails[0].longer_side, 'b');
    }

    #[test]
    fn test_diff_with_anomalous_tail_in_b() {
        // A is 3 bytes, B has real data beyond A's length
        let a = [0x01, 0x02, 0x03];
        let b = [0x01, 0x02, 0x03, 0xAA, 0xBB];
        let result = smart_diff(&a, &b, 0xFF);
        assert!(!result.summary.is_equal);
        assert!(result.diffs.is_empty());
        assert_eq!(result.tails.len(), 1);
        assert_eq!(result.tails[0].kind, TailKind::Anomalous);
        assert_eq!(result.summary.anomalous_tail, 1);
    }

    #[test]
    fn test_byte_differences() {
        let a = [0x01, 0x02, 0x03, 0x04, 0xFF];
        let b = [0x01, 0x09, 0x03, 0x08, 0xFF];
        let result = smart_diff(&a, &b, 0xFF);
        assert!(!result.summary.is_equal);
        assert_eq!(result.diffs.len(), 2);
        assert_eq!(result.diffs[0].offset, 1);
        assert_eq!(result.diffs[0].value_a, 0x02);
        assert_eq!(result.diffs[0].value_b, 0x09);
        assert_eq!(result.diffs[1].offset, 3);
        assert_eq!(result.diffs[1].value_a, 0x04);
        assert_eq!(result.diffs[1].value_b, 0x08);
        assert_eq!(result.summary.diff_regions, 2);
    }

    #[test]
    fn test_contiguous_diff_region() {
        let a = [0x01, 0x02, 0x03, 0x04, 0x05];
        let b = [0x01, 0x09, 0x08, 0x04, 0x05];
        let result = smart_diff(&a, &b, 0xFF);
        assert_eq!(result.diffs.len(), 2);
        assert_eq!(result.summary.diff_regions, 1); // contiguous
    }

    #[test]
    fn test_trailing_padding_with_diff() {
        // Real diff at offset 1, plus trailing padding in B
        let a = [0x01, 0x02, 0x03];
        let b = [0x01, 0x09, 0x03, 0xFF, 0xFF];
        let result = smart_diff(&a, &b, 0xFF);
        assert!(!result.summary.is_equal);
        assert_eq!(result.diffs.len(), 1);
        assert_eq!(result.diffs[0].offset, 1);
        assert_eq!(result.tails.len(), 1);
        assert_eq!(result.tails[0].kind, TailKind::Padding);
    }

    #[test]
    fn test_a_longer_than_b_with_padding() {
        let a = [0x01, 0x02, 0x03, 0xFF, 0xFF];
        let b = [0x01, 0x02, 0x03];
        let result = smart_diff(&a, &b, 0xFF);
        assert!(result.summary.is_equal);
        assert_eq!(result.tails.len(), 1);
        assert_eq!(result.tails[0].kind, TailKind::Padding);
        assert_eq!(result.tails[0].longer_side, 'a');
    }

    #[test]
    fn test_a_longer_than_b_with_anomalous() {
        let a = [0x01, 0x02, 0x03, 0xAA, 0xBB];
        let b = [0x01, 0x02, 0x03];
        let result = smart_diff(&a, &b, 0xFF);
        assert!(!result.summary.is_equal);
        assert_eq!(result.tails[0].kind, TailKind::Anomalous);
        assert_eq!(result.tails[0].longer_side, 'a');
    }

    #[test]
    fn test_erase_value_zero() {
        // EEPROM that erases to 0x00
        let a = [0x01, 0x02, 0x03];
        let b = [0x01, 0x02, 0x03, 0x00, 0x00];
        let result = smart_diff(&a, &b, 0x00);
        assert!(result.summary.is_equal);
        assert_eq!(result.tails[0].kind, TailKind::Padding);
    }

    #[test]
    fn test_erase_value_zero_anomalous() {
        let a = [0x01, 0x02, 0x03];
        let b = [0x01, 0x02, 0x03, 0x00, 0xFF];
        let result = smart_diff(&a, &b, 0x00);
        assert!(!result.summary.is_equal);
        assert_eq!(result.tails[0].kind, TailKind::Anomalous);
    }

    #[test]
    fn test_empty_buffers() {
        let a: [u8; 0] = [];
        let b: [u8; 0] = [];
        let result = smart_diff(&a, &b, 0xFF);
        assert!(result.summary.is_equal);
        assert!(result.diffs.is_empty());
        assert!(result.tails.is_empty());
    }

    #[test]
    fn test_one_empty_one_nonempty_padding() {
        let a: [u8; 0] = [];
        let b = [0xFF, 0xFF, 0xFF];
        let result = smart_diff(&a, &b, 0xFF);
        assert!(result.summary.is_equal);
        assert_eq!(result.tails[0].kind, TailKind::Padding);
    }

    #[test]
    fn test_one_empty_one_nonempty_anomalous() {
        let a: [u8; 0] = [];
        let b = [0x01, 0x02, 0x03];
        let result = smart_diff(&a, &b, 0xFF);
        assert!(!result.summary.is_equal);
        assert_eq!(result.tails[0].kind, TailKind::Anomalous);
    }
}
