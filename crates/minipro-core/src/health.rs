//! Chip read-stability health check.
//!
//! Diagnoses marginal/degrading chips by repeating chip-ID reads and full
//! memory reads, then characterizing how the results disagree. Motivating
//! case (2026-11): a PM25LV010 read cleanly months earlier but produced
//! non-deterministic corruption on both a TL866A and a T76 — the stored
//! bits were fine; the output driver was dying. Repeated reads surface
//! that in seconds.
//!
//! What this detects: *read-path* instability — weak/floating data-output
//! drivers, marginal contacts, intermittent HOLD#/WP# faults. Signature:
//! bytes that differ between repeated reads. What it does NOT detect:
//! stored-cell charge loss (a uniformly-rotten cell reads consistently)
//! or deterministic decode bugs (also consistent) — use a known-good
//! reference dump for those.

/// Per-region instability count for the disagreement map.
#[derive(Debug, Clone)]
pub struct RegionStat {
    /// Byte offset of the region start.
    pub start: usize,
    /// Region length in bytes (last region may be short).
    pub len: usize,
    /// Bytes within the region that disagreed across passes.
    pub unstable: usize,
}

/// Overall health verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthVerdict {
    /// All read passes were byte-identical and the chip ID was stable.
    Stable,
    /// The JEDEC/chip ID returned different values across reads but the
    /// memory data was consistent.
    MarginalId,
    /// Memory contents differed between read passes.
    MarginalData,
    /// Every read pass failed — transport or chip never answered.
    TransportError,
    /// Some passes failed; results are from the completed subset only.
    Incomplete,
}

impl HealthVerdict {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Stable => "STABLE",
            Self::MarginalId => "MARGINAL-ID",
            Self::MarginalData => "MARGINAL-DATA",
            Self::TransportError => "TRANSPORT-ERROR",
            Self::Incomplete => "INCOMPLETE",
        }
    }
}

/// Full report from [`crate::operations::health_check`].
#[derive(Debug)]
pub struct HealthReport {
    /// Selected device name.
    pub device_name: String,
    /// Code-memory size in bytes.
    pub size: usize,
    /// Requested full-read passes.
    pub read_passes_requested: usize,
    /// Full-read passes that completed.
    pub read_passes_completed: usize,
    /// Raw chip IDs observed (one per ID run; empty if unsupported).
    pub chip_ids: Vec<u32>,
    /// Expected chip ID from the database, if the device has one.
    pub chip_id_expected: Option<u32>,
    /// Per-pass errors for reads that failed mid-operation.
    pub pass_errors: Vec<String>,
    /// Per-pass error strings for chip-ID reads that failed.
    pub chip_id_errors: Vec<String>,
    /// CRC-32 of each completed read pass.
    pub pass_crcs: Vec<u32>,
    /// Number of byte positions that were not identical across passes.
    pub unstable_bytes: usize,
    /// Disagreement map over 4 KiB regions.
    pub regions: Vec<RegionStat>,
    /// Total bit positions where a minority read had 1 vs majority 0
    /// (flip direction consistent with weak/floating-high MISO).
    pub flips_up: u64,
    /// Total bit positions where a minority read had 0 vs majority 1.
    pub flips_down: u64,
    /// Dominant spacing between consecutive unstable bytes, when a clear
    /// periodicity exists (e.g. the every-512-byte boundary artifact).
    pub periodicity: Option<u32>,
    /// Human-readable characterization lines.
    pub notes: Vec<String>,
    /// Per-byte majority-vote reconstruction (present only when
    /// `unstable_bytes > 0` — a best-effort recovery dump).
    pub consensus: Option<Vec<u8>>,
    /// Final verdict.
    pub verdict: HealthVerdict,
}

/// Region size for the disagreement map (4 KiB).
const REGION_SIZE: usize = 4096;

/// Minimum unstable bytes before periodicity analysis is attempted.
const MIN_PERIODIC_SAMPLE: usize = 8;

/// Analyze a set of read passes. All buffers must be the same length.
fn analyze(reads: &[Vec<u8>]) -> (Vec<u8>, Vec<usize>, u64, u64) {
    let size = reads[0].len();
    let mut consensus = vec![0u8; size];
    let mut unstable_positions = Vec::new();
    let mut flips_up = 0u64;
    let mut flips_down = 0u64;

    for (i, slot) in consensus.iter_mut().enumerate() {
        // Histogram is unnecessary at this scale — count per distinct value.
        let mut best_val = reads[0][i];
        let mut best_count = 1usize;
        let mut any_diff = false;
        for r in reads.iter().skip(1) {
            let v = r[i];
            if v == best_val {
                best_count += 1;
            } else {
                any_diff = true;
                // Count occurrences of this challenger across all passes.
                let count = reads.iter().filter(|rr| rr[i] == v).count();
                if count > best_count {
                    best_count = count;
                    best_val = v;
                }
            }
        }
        *slot = best_val;
        if any_diff {
            unstable_positions.push(i);
            // Flip direction per bit: bits where a minority pass shows the
            // opposite of the consensus bit.
            for r in reads {
                let v = r[i];
                if v == best_val {
                    continue;
                }
                let minority_set = v & !best_val;
                let minority_clear = !v & best_val;
                flips_up += minority_set.count_ones() as u64;
                flips_down += minority_clear.count_ones() as u64;
            }
        }
    }
    (consensus, unstable_positions, flips_up, flips_down)
}

/// Detect dominant spacing between consecutive unstable offsets.
fn detect_periodicity(positions: &[usize]) -> Option<u32> {
    if positions.len() < MIN_PERIODIC_SAMPLE {
        return None;
    }
    // Histogram of consecutive deltas.
    let mut deltas: Vec<u32> = Vec::with_capacity(positions.len() - 1);
    for w in positions.windows(2) {
        let d = (w[1] - w[0]) as u32;
        if d > 0 {
            deltas.push(d);
        }
    }
    if deltas.is_empty() {
        return None;
    }
    // Mode delta wins when it covers most gaps.
    let mut counts: Vec<(u32, usize)> = Vec::new();
    for &d in &deltas {
        match counts.iter_mut().find(|(k, _)| *k == d) {
            Some((_, c)) => *c += 1,
            None => counts.push((d, 1)),
        }
    }
    counts.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
    let (delta, count) = counts[0];
    if count * 2 >= deltas.len() && delta >= 4 {
        Some(delta)
    } else {
        None
    }
}

/// Uniform-content check: `Some(0xFF)`/`Some(0x00)` if every byte is that value.
fn uniform_value(buf: &[u8]) -> Option<u8> {
    let first = *buf.first()?;
    if buf.iter().all(|&b| b == first) {
        Some(first)
    } else {
        None
    }
}

/// Build the report from raw pass data.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_report(
    device_name: String,
    size: usize,
    read_passes_requested: usize,
    chip_ids: Vec<u32>,
    chip_id_expected: Option<u32>,
    chip_id_errors: Vec<String>,
    reads: Vec<Vec<u8>>,
    pass_errors: Vec<String>,
) -> HealthReport {
    let pass_crcs: Vec<u32> = reads
        .iter()
        .map(|r| crc::Crc::<u32>::new(&crc::CRC_32_ISO_HDLC).checksum(r))
        .collect();

    let mut notes = Vec::new();
    let mut consensus = None;
    let mut unstable_bytes = 0usize;
    let mut regions: Vec<RegionStat> = Vec::new();
    let mut flips_up = 0u64;
    let mut flips_down = 0u64;
    let mut periodicity = None;

    // ── Chip ID analysis ────────────────────────────────────────────────
    let mut unique_ids: Vec<u32> = chip_ids.clone();
    unique_ids.sort_unstable();
    unique_ids.dedup();
    if !chip_ids.is_empty() {
        // Stuck-bus detection: 0x0 and any-width all-ones (0xFF, 0xFFFF, …)
        // are the MISO-held-low / floating-high signatures, not real IDs.
        // `(id + 1).is_power_of_two()` matches both cases at any width.
        let all_stuck = unique_ids
            .iter()
            .all(|&id| id.wrapping_add(1).is_power_of_two());
        if all_stuck {
            let seen: Vec<String> = unique_ids
                .iter()
                .map(|id| format!("0x{:06x}", id))
                .collect();
            notes.push(format!(
                "chip-ID reads returned only stuck-bus values ({}) — chip is not \
                 answering the ID command (contact, HOLD#/WP#, or dead output stage)",
                seen.join(", ")
            ));
        } else if unique_ids.len() > 1 {
            let seen: Vec<String> = unique_ids
                .iter()
                .map(|id| format!("0x{:06x}", id))
                .collect();
            notes.push(format!(
                "chip ID unstable across {} reads — saw {}",
                chip_ids.len(),
                seen.join(", ")
            ));
        }
        if let Some(expected) = chip_id_expected {
            let exp = crate::operations::normalize_chip_id(expected);
            if unique_ids
                .iter()
                .all(|id| crate::operations::normalize_chip_id(*id) != exp)
            {
                notes.push(format!(
                    "no read matched expected chip ID 0x{:06x} — wrong chip or missing device",
                    expected
                ));
            }
        }
    }

    // ── Read-pass analysis ───────────────────────────────────────────────
    if reads.len() >= 2 {
        // Defensive: all passes should be `size` bytes from the same device,
        // but never index past the shortest buffer if a mid-read shortfall
        // ever slips through.
        let min_len = reads.iter().map(|r| r.len()).min().unwrap_or(0);
        if min_len < size {
            notes.push(format!(
                "read passes returned differing lengths — comparing first {} bytes",
                min_len
            ));
        }
        let trimmed: Vec<Vec<u8>> = reads.iter().map(|r| r[..min_len].to_vec()).collect();
        let (cons, unstable, fu, fd) = analyze(&trimmed);
        let n_unstable = unstable.len();
        flips_up = fu;
        flips_down = fd;
        unstable_bytes = n_unstable;

        if n_unstable == 0 {
            notes.push(format!(
                "{}/{} read passes byte-identical",
                reads.len(),
                read_passes_requested
            ));
            if let Some(v) = uniform_value(&reads[0]) {
                notes.push(format!(
                    "all reads uniform 0x{:02x} — blank chip or chip not responding",
                    v
                ));
            }
        } else {
            consensus = Some(cons);
            // Region map.
            let mut buckets: Vec<usize> = vec![0; min_len.div_ceil(REGION_SIZE)];
            for &pos in &unstable {
                buckets[pos / REGION_SIZE] += 1;
            }
            for (i, &u) in buckets.iter().enumerate() {
                if u > 0 {
                    let start = i * REGION_SIZE;
                    regions.push(RegionStat {
                        start,
                        len: REGION_SIZE.min(min_len - start),
                        unstable: u,
                    });
                }
            }

            periodicity = detect_periodicity(&unstable);
            let pct = n_unstable as f64 * 100.0 / min_len as f64;
            notes.push(format!(
                "{} byte position{} ({:.2}% of chip) disagreed across {} passes",
                n_unstable,
                if n_unstable == 1 { "" } else { "s" },
                pct,
                reads.len()
            ));
            if let Some(d) = periodicity {
                notes.push(format!(
                    "unstable bytes cluster at every-{}-byte boundaries — deterministic-looking artifact (data structure or decode quirk), not random noise",
                    d
                ));
            }
            let total_flips = flips_up + flips_down;
            if total_flips > 0 {
                let up_pct = flips_up * 100 / total_flips;
                if up_pct >= 75 {
                    notes.push(
                        "bit flips biased strongly 0→1 — consistent with weak/floating data-output drive (MISO)"
                            .to_string(),
                    );
                } else if up_pct <= 25 {
                    notes.push(
                        "bit flips biased strongly 1→0 — consistent with output held low / CS or addressing fault"
                            .to_string(),
                    );
                }
            }
            // Uniform passes among the set.
            for (idx, r) in reads.iter().enumerate() {
                if let Some(v) = uniform_value(r) {
                    notes.push(format!(
                        "pass {} returned all-0x{:02x} — chip stopped responding mid-test",
                        idx + 1,
                        v
                    ));
                    break;
                }
            }
        }
    }

    let verdict = if reads.is_empty() && !pass_errors.is_empty() {
        HealthVerdict::TransportError
    } else if unstable_bytes > 0 {
        HealthVerdict::MarginalData
    } else if unique_ids.len() > 1 {
        HealthVerdict::MarginalId
    } else if !pass_errors.is_empty() || reads.len() < read_passes_requested.min(2) {
        HealthVerdict::Incomplete
    } else {
        HealthVerdict::Stable
    };

    HealthReport {
        device_name,
        size,
        read_passes_requested,
        read_passes_completed: reads.len(),
        chip_ids,
        chip_id_expected,
        chip_id_errors,
        pass_errors,
        pass_crcs,
        unstable_bytes,
        regions,
        flips_up,
        flips_down,
        periodicity,
        notes,
        consensus,
        verdict,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reads_eq(a: &[Vec<u8>]) -> HealthReport {
        build_report(
            "test".into(),
            a[0].len(),
            a.len(),
            Vec::new(),
            None,
            Vec::new(),
            a.to_vec(),
            Vec::new(),
        )
    }

    #[test]
    fn identical_reads_are_stable() {
        let r = reads_eq(&[vec![0xAA; 64], vec![0xAA; 64], vec![0xAA; 64]]);
        assert_eq!(r.verdict, HealthVerdict::Stable);
        assert_eq!(r.unstable_bytes, 0);
        assert!(r.consensus.is_none());
    }

    #[test]
    fn single_unstable_byte_is_marginal() {
        let mut a = vec![0u8; 64];
        let mut b = vec![0u8; 64];
        b[10] = 1;
        a[20] = 0; // same in both — stable
        let r = reads_eq(&[a, b]);
        assert_eq!(r.verdict, HealthVerdict::MarginalData);
        assert_eq!(r.unstable_bytes, 1);
        assert_eq!(r.consensus.as_ref().unwrap()[10], 0); // majority wins
        assert_eq!(r.flips_up, 1);
        assert_eq!(r.flips_down, 0);
    }

    #[test]
    fn periodic_strays_detected() {
        // Unstable bytes at exact 512-byte spacing like the LE25FS406M case.
        let size = 8 * 512 + 1;
        let a = vec![0xFF; size];
        let mut b = vec![0xFF; size];
        for i in 0..8 {
            b[i * 512] = 0x7F; // 0→1..0 direction: majority FF, minority 7F
        }
        let r = reads_eq(&[a, b]);
        assert_eq!(r.periodicity, Some(512));
        assert_eq!(r.unstable_bytes, 8);
        assert_eq!(r.flips_down, 8); // 7F differs from FF by one cleared bit
        assert!(r
            .notes
            .iter()
            .any(|n| n.contains("every-512-byte boundaries")));
    }

    #[test]
    fn drifting_id_is_marginal_id() {
        let r = build_report(
            "pm25".into(),
            4,
            2,
            vec![0x9d7c, 0xdf7e, 0x9d7c],
            Some(0x9d7c),
            Vec::new(),
            vec![vec![0xAA; 4], vec![0xAA; 4]],
            Vec::new(),
        );
        assert_eq!(r.verdict, HealthVerdict::MarginalId);
        assert!(r.notes.iter().any(|n| n.contains("chip ID unstable")));
    }

    #[test]
    fn stuck_bus_ids_reported_as_no_response() {
        // Real case: PM25LV010 on T48 returned only 0x000000/0x00ffff across
        // 20 ID reads while data stayed stable — MISO stuck, not drifting.
        let r = build_report(
            "pm25".into(),
            4,
            2,
            vec![0, 0xffff, 0, 0xffff],
            Some(0x9d7c),
            Vec::new(),
            vec![vec![0xAA; 4], vec![0xAA; 4]],
            Vec::new(),
        );
        assert_eq!(r.verdict, HealthVerdict::MarginalId);
        assert!(r.notes.iter().any(|n| n.contains("stuck-bus")));
    }

    #[test]
    fn all_passes_failed_is_transport_error() {
        let r = build_report(
            "x".into(),
            4,
            3,
            Vec::new(),
            None,
            Vec::new(),
            Vec::new(),
            vec!["usb error".into()],
        );
        assert_eq!(r.verdict, HealthVerdict::TransportError);
    }

    #[test]
    fn consensus_majority_vote() {
        // 3 passes: position 5 reads [0x10, 0xFF, 0x10] → consensus 0x10.
        let mut a = vec![0x10; 8];
        let mut b = vec![0x10; 8];
        let c = vec![0x10; 8];
        b[5] = 0xFF;
        a[5] = 0x10;
        let r = reads_eq(&[a, b, c]);
        assert_eq!(r.unstable_bytes, 1);
        assert_eq!(r.consensus.unwrap()[5], 0x10);
    }

    #[test]
    fn uniform_blank_reads_are_stable_not_no_response() {
        // An all-0xFF chip and a dead chip are indistinguishable from reads
        // alone — verdict stays Stable with a note, never a chip-failure call.
        let r = reads_eq(&[vec![0xFF; 32], vec![0xFF; 32]]);
        assert_eq!(r.verdict, HealthVerdict::Stable);
        assert!(r.notes.iter().any(|n| n.contains("blank chip")));
    }

    #[test]
    fn mid_pass_failure_is_incomplete() {
        // 2 of 5 passes completed, rest erred — data was consistent, but the
        // failed passes keep this from being a clean Stable.
        let r = build_report(
            "x".into(),
            4,
            5,
            Vec::new(),
            None,
            Vec::new(),
            vec![vec![0xAA; 4], vec![0xAA; 4]],
            vec!["pass 3: usb timeout".into()],
        );
        assert_eq!(r.verdict, HealthVerdict::Incomplete);
        assert_eq!(r.read_passes_completed, 2);
    }

    #[test]
    fn single_pass_alone_is_incomplete() {
        // One pass succeeded but nothing to compare against.
        let r = build_report(
            "x".into(),
            4,
            3,
            Vec::new(),
            None,
            Vec::new(),
            vec![vec![0xAA; 4]],
            vec!["pass 2: usb timeout".into(), "pass 3: usb timeout".into()],
        );
        assert_eq!(r.verdict, HealthVerdict::Incomplete);
    }

    #[test]
    fn mismatched_pass_lengths_compare_shorter() {
        // A short pass must not panic the analyzer; comparison clamps to the
        // shortest buffer and notes the truncation.
        let r = reads_eq(&[vec![0x00; 64], vec![0x00; 32]]);
        assert_eq!(r.read_passes_completed, 2);
        assert!(r.notes.iter().any(|n| n.contains("differing lengths")));
    }

    #[test]
    fn flips_down_direction_detected() {
        // Majority 0x00, minority 0xFF → all 8 bits read 1 in minority → 0→1.
        let a = vec![0x00; 16];
        let mut b = vec![0x00; 16];
        b[3] = 0xFF;
        let r = reads_eq(&[a, b]);
        assert_eq!(r.flips_up, 8);
        assert_eq!(r.flips_down, 0);
    }
}
