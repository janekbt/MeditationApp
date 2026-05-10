//! Pure vibration-pattern math: quantisation, run-length encoding,
//! envelope sampling, chunking, and structural equality. All the
//! D-Bus / feedbackd transport stays in the shell that drives a
//! real motor (the GTK shell's `vibration::PatternPlayback`); same
//! goes for the Android shell's eventual JNI Vibrator wrapper.
//!
//! The math here works on `crate::db::VibrationPattern` directly so
//! every shell shares one implementation.

use crate::db::VibrationPattern;

/// feedbackd's `Vibrate` accepts up to 10 (amplitude, duration_ms)
/// tuples per call (silent truncation past `MAX_ITEMS = 10` in
/// `fbd-haptic-manager.c`). Patterns longer than that are split
/// into chained calls; this is the per-call ceiling.
const MAX_SEGMENTS_PER_CHUNK: u32 = 10;

/// Adjacent chunks overlap by this many segments. Each chunk's
/// last `CHUNK_OVERLAP_SEGMENTS` slots replay what the next chunk
/// will start on, so the supersede-instant lands on matching
/// amplitudes — no audible jump even with ~50 ms scheduling
/// jitter.
const CHUNK_OVERLAP_SEGMENTS: u32 = 2;

/// Sampling tick for the Line-mode reference envelope (the
/// envelope is sampled at this resolution and the resulting
/// quantised amplitudes are run-length encoded). Matches the
/// LRA's perception floor: amplitude transitions inside a 100 ms
/// window blur into a single perceived step.
const LINE_TICK_MS: u32 = 100;

/// Quantise an amplitude in `[0, 1]` to the nearest 0.10 (the LRA
/// can render maybe 5–10 distinct intensity levels; finer
/// authoring is wasted, and quantising lets RLE collapse held-
/// amplitude stretches into single segments — editor's
/// snap-to-decile is the same idea).
///
/// The `+ 1e-6` bias snaps values that are *just barely* below a
/// boundary up to the nearest decile. f32 inputs like 0.9499999
/// (the f32 closest to a 0.95 literal) would otherwise round to 9
/// rather than the intended 10. The 1e-6 bias resolves the half-
/// case in favour of rounding away from zero, matching the
/// editor's snap behaviour and giving deterministic RLE output.
fn quantise_amplitude(v: f32) -> f64 {
    let v = v.clamp(0.0, 1.0) as f64;
    let scaled = v * 10.0 + 1e-6;
    let bin = (scaled.round() as i32).clamp(0, 10);
    // Compute the quantum via f64 division (not f32 multiplication)
    // so the output values are clean (0.5, 1.0, …) instead of
    // f32-precision residuals like 0.5000000074.
    bin as f64 / 10.0
}

/// Run-length-encode a sequence of (amplitude, duration_ms) ticks
/// into one segment per consecutive same-amplitude run. Durations
/// sum, amplitudes are kept (they're already equal across a run).
fn rle_consecutive(ticks: impl IntoIterator<Item = (f64, u32)>) -> Vec<(f64, u32)> {
    let mut out: Vec<(f64, u32)> = Vec::new();
    for (amp, dur) in ticks {
        match out.last_mut() {
            Some(last) if (last.0 - amp).abs() < 1e-9 => {
                last.1 += dur;
            }
            _ => out.push((amp, dur)),
        }
    }
    out
}

/// Build the (amplitude, duration_ms) sequence we'll ship through
/// feedbackd. Both shape modes:
///
/// 1. Sample a fine reference envelope (Bar: bar-by-bar; Line:
///    100 ms-tick centre sampling of the linearly-interpolated
///    envelope).
/// 2. Quantise each sample to 10% amplitude steps.
/// 3. Run-length encode consecutive same-quantum runs into one
///    segment with the summed duration.
///
/// The win: a long held intensity collapses to a single segment
/// regardless of duration, so a "ramp-then-hold" pattern (e.g.
/// 0% → 100% over 1 s, then 50% held for 2 s) needs ~11 segments
/// total and fits in 2 chunks, not 30 segments / 4 chunks.
///
/// Segment durations sum to `p.duration_ms` exactly (the
/// remainder of the integer division lands on the last tick).
/// Returns an empty vec for empty / zero-duration inputs.
pub fn build_master_envelope(p: &VibrationPattern) -> Vec<(f64, u32)> {
    let n_in = p.intensities.len();
    if n_in == 0 || p.duration_ms == 0 {
        return Vec::new();
    }

    match p.chart_kind {
        crate::db::ChartKind::Bar => {
            let n = n_in as u32;
            let base = p.duration_ms / n;
            let remainder = p.duration_ms - base * n;
            let ticks = (0..n as usize).map(|i| {
                let amp = quantise_amplitude(p.intensities[i]);
                let dur = if i == n as usize - 1 { base + remainder } else { base };
                (amp, dur)
            });
            rle_consecutive(ticks)
        }
        crate::db::ChartKind::Line => {
            let n_ticks = p.duration_ms.div_ceil(LINE_TICK_MS).max(1);
            let base = p.duration_ms / n_ticks;
            let last_dur = p.duration_ms - base * (n_ticks - 1);
            let ticks = (0..n_ticks).map(|i| {
                let t_ms = ((2 * i + 1) * p.duration_ms) / (2 * n_ticks);
                let amp = quantise_amplitude(sample_line_at(p, t_ms));
                let dur = if i == n_ticks - 1 { last_dur } else { base };
                (amp, dur)
            });
            rle_consecutive(ticks)
        }
    }
}

/// Slice `master` into chunks of at most `MAX_SEGMENTS_PER_CHUNK`
/// segments each, with `CHUNK_OVERLAP_SEGMENTS` segments shared
/// between adjacent chunks. Returns one chunk for masters that
/// already fit in a single Vibrate call.
pub fn split_into_chunks(master: &[(f64, u32)]) -> Vec<Vec<(f64, u32)>> {
    let s = master.len();
    if s == 0 {
        return Vec::new();
    }
    if s <= MAX_SEGMENTS_PER_CHUNK as usize {
        return vec![master.to_vec()];
    }
    let chunk_len = MAX_SEGMENTS_PER_CHUNK as usize;
    let stride = chunk_len - CHUNK_OVERLAP_SEGMENTS as usize;
    let mut out = Vec::new();
    let mut start = 0usize;
    while start < s {
        let end = (start + chunk_len).min(s);
        out.push(master[start..end].to_vec());
        if end == s {
            break;
        }
        start += stride;
    }
    out
}

/// Master-time at which chunk `k` should fire — the cumulative
/// duration of the segments preceding the chunk's first segment.
pub fn chunk_start_offset_ms(master: &[(f64, u32)], k: usize) -> u32 {
    if k == 0 {
        return 0;
    }
    let stride = (MAX_SEGMENTS_PER_CHUNK - CHUNK_OVERLAP_SEGMENTS) as usize;
    let first_seg = (k * stride).min(master.len());
    master[..first_seg].iter().map(|(_, d)| *d).sum()
}

/// Linearly interpolate the Line-mode envelope at time `t_ms`.
/// Maps `t_ms / duration_ms` onto the `[0, n-1]` index range,
/// returns the lerp between the two adjacent control points.
fn sample_line_at(p: &VibrationPattern, t_ms: u32) -> f32 {
    let n = p.intensities.len();
    let denom = (n - 1).max(1) as f32;
    let xf = (t_ms as f32 / p.duration_ms as f32) * denom;
    let lo = xf.floor() as usize;
    let hi = (lo + 1).min(n - 1);
    let frac = xf - lo as f32;
    p.intensities[lo] * (1.0 - frac) + p.intensities[hi] * frac
}

/// Structural equality on the user-meaningful fields of
/// `VibrationPattern`. Used to gate the editor's Undo toast: only
/// flash "Restored" if the pattern actually changed (same name,
/// duration, chart-kind, and intensities-with-tolerance counts as
/// equivalent). UUID and timestamps are intentionally skipped —
/// they always change on update.
pub fn patterns_equivalent(a: &VibrationPattern, b: &VibrationPattern) -> bool {
    a.name == b.name
        && a.duration_ms == b.duration_ms
        && a.chart_kind == b.chart_kind
        && a.intensities.len() == b.intensities.len()
        && a.intensities
            .iter()
            .zip(b.intensities.iter())
            .all(|(x, y)| (x - y).abs() < 1e-6)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::ChartKind;

    fn pattern(
        duration_ms: u32,
        intensities: Vec<f32>,
        chart_kind: ChartKind,
    ) -> VibrationPattern {
        VibrationPattern {
            id: 0,
            uuid: String::new(),
            name: String::new(),
            duration_ms,
            intensities,
            chart_kind,
            is_bundled: false,
            created_iso: String::new(),
            updated_iso: String::new(),
        }
    }

    // ── build_master_envelope: empty / zero-duration ────────────────────

    #[test]
    fn master_envelope_empty_for_empty_intensities() {
        let p = pattern(1000, vec![], ChartKind::Line);
        assert!(build_master_envelope(&p).is_empty());
    }

    #[test]
    fn master_envelope_empty_for_zero_duration() {
        let p = pattern(0, vec![0.5, 1.0], ChartKind::Line);
        assert!(build_master_envelope(&p).is_empty());
    }

    // ── build_master_envelope: Bar mode + RLE ────────────────────────────

    #[test]
    fn master_envelope_bar_emits_one_segment_per_distinct_amplitude_run() {
        // 5 bars × 200 ms each. Adjacent same-amp runs collapse via
        // RLE: [0.5, 0.5, 0.5, 1.0, 1.0] → [(0.5, 600), (1.0, 400)].
        let p = pattern(1000, vec![0.5, 0.5, 0.5, 1.0, 1.0], ChartKind::Bar);
        let m = build_master_envelope(&p);
        assert_eq!(m, vec![(0.5, 600), (1.0, 400)]);
    }

    #[test]
    fn master_envelope_bar_distinct_amplitudes_keep_their_segments() {
        // 1003 ms / 5 distinct bars = 200 base + 3 ms on the last.
        let p = pattern(1003, vec![0.2, 0.5, 1.0, 0.5, 0.2], ChartKind::Bar);
        let m = build_master_envelope(&p);
        assert_eq!(m.len(), 5);
        for s in &m[..4] { assert_eq!(s.1, 200); }
        assert_eq!(m[4].1, 203);
        let total: u32 = m.iter().map(|s| s.1).sum();
        assert_eq!(total, 1003);
    }

    #[test]
    fn master_envelope_bar_quantises_amplitudes_to_10_percent() {
        // 0.05 rounds to 0.1, 0.04 to 0.0, 0.95 to 1.0. Editor will
        // snap to 10% in practice but the runtime guarantees it too.
        let p = pattern(400, vec![0.04, 0.05, 0.95, 0.96], ChartKind::Bar);
        let m = build_master_envelope(&p);
        let amps: Vec<f64> = m.iter().map(|s| (s.0 * 10.0).round() / 10.0).collect();
        // 0.04 → 0.0, 0.05 → 0.1, 0.95 → 1.0, 0.96 → 1.0 → 1.0+1.0 RLE-merge.
        assert_eq!(amps, vec![0.0, 0.1, 1.0]);
    }

    // ── build_master_envelope: Line mode + RLE ───────────────────────────

    #[test]
    fn master_envelope_line_constant_intensity_collapses_to_one_segment() {
        // Constant 0.5 for 5 s → after RLE this is a single segment
        // (0.5, 5000). Major win for "buzz steady" patterns.
        let p = pattern(5000, vec![0.5; 7], ChartKind::Line);
        let m = build_master_envelope(&p);
        assert_eq!(m.len(), 1);
        assert!((m[0].0 - 0.5).abs() < 1e-9);
        assert_eq!(m[0].1, 5000);
    }

    #[test]
    fn master_envelope_line_ramp_quantises_to_ten_percent_steps() {
        // 0 → 1 ramp over 1 s. Center sampling at 100 ms ticks lands
        // values at 0.05, 0.15, 0.25, …, 0.95. Each rounds away-from-
        // zero to 0.1, 0.2, 0.3, …, 1.0 — 10 distinct segments.
        let p = pattern(1000, vec![0.0, 1.0], ChartKind::Line);
        let m = build_master_envelope(&p);
        assert_eq!(m.len(), 10);
        let amps: Vec<f64> = m.iter().map(|s| s.0).collect();
        for (i, &a) in amps.iter().enumerate() {
            let expected = (i + 1) as f64 / 10.0;
            assert!((a - expected).abs() < 1e-3,
                "segment {i}: got {a}, expected {expected}");
        }
        for s in m.iter() { assert_eq!(s.1, 100); }
    }

    #[test]
    fn master_envelope_line_user_example_ramp_then_hold() {
        // The user's mental model: 0% → 100% over 1 s, then 50% held
        // for 2 s. Total 3 s. Three control points: [0.0, 1.0, 0.5].
        // Wait — that wouldn't give a clean ramp-then-hold. Use the
        // exact shape via a denser control-point set instead.
        //
        // Actually the cleanest way to author it is to set the second
        // control point at t = 1/3 of duration with intensity 1.0 and
        // a third at intensity 0.5, but linear interp between (1.0)
        // and (0.5) at t = 1/3..1.0 doesn't hold flat. So this test
        // covers the encoder's contract, not the literal authoring.
        //
        // For the encoder: a master envelope where the value is 0.5
        // for ⅔ of the duration should RLE-collapse that ⅔ into a
        // single long segment.
        let p = pattern(3000, vec![0.5; 7], ChartKind::Line);
        let m = build_master_envelope(&p);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].1, 3000);
    }

    #[test]
    fn master_envelope_line_total_duration_preserved_after_rle() {
        // 950 ms with a 0→1 ramp. Whatever segment count and durations
        // RLE produces, the sum must equal 950 ms exactly.
        let p = pattern(950, vec![0.0, 1.0], ChartKind::Line);
        let m = build_master_envelope(&p);
        let total: u32 = m.iter().map(|s| s.1).sum();
        assert_eq!(total, 950);
    }

    #[test]
    fn master_envelope_line_long_sparse_pattern_collapses_aggressively() {
        // 10 s pattern with 3 control points [0, 1, 0]. The slow
        // up-and-down ramp visits each 10% level once on the way up
        // and once on the way down → ~20 distinct segments via RLE,
        // not 100 (the old 100 ms-tick count).
        let p = pattern(10_000, vec![0.0, 1.0, 0.0], ChartKind::Line);
        let m = build_master_envelope(&p);
        assert!(m.len() <= 22, "RLE should collapse held quantisation runs: got {}", m.len());
        let total: u32 = m.iter().map(|s| s.1).sum();
        assert_eq!(total, 10_000);
    }

    // ── split_into_chunks + chunk_start_offset_ms ────────────────────────

    fn flat_master(n: usize, seg_dur_ms: u32) -> Vec<(f64, u32)> {
        (0..n).map(|i| (i as f64 / 100.0, seg_dur_ms)).collect()
    }

    #[test]
    fn split_returns_empty_for_empty_master() {
        assert!(split_into_chunks(&[]).is_empty());
    }

    #[test]
    fn split_returns_single_chunk_when_master_fits() {
        let m = flat_master(10, 100);
        let chunks = split_into_chunks(&m);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].len(), 10);
    }

    #[test]
    fn split_emits_two_chunks_with_two_segment_overlap_for_s_eq_18() {
        // S=18: chunk 0 [0..10), chunk 1 [8..18). Overlap = master[8..10].
        let m = flat_master(18, 100);
        let chunks = split_into_chunks(&m);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 10);
        assert_eq!(chunks[1].len(), 10);
        assert_eq!(chunks[0][8], chunks[1][0], "overlap segment 0");
        assert_eq!(chunks[0][9], chunks[1][1], "overlap segment 1");
    }

    #[test]
    fn split_handles_partial_last_chunk() {
        // S=12: chunk 0 [0..10), chunk 1 [8..12) → 4 segments.
        let m = flat_master(12, 100);
        let chunks = split_into_chunks(&m);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 10);
        assert_eq!(chunks[1].len(), 4);
        assert_eq!(chunks[0][8], chunks[1][0]);
        assert_eq!(chunks[0][9], chunks[1][1]);
    }

    #[test]
    fn split_thirteen_chunks_for_full_10s_envelope() {
        // S=100 → 1 + ceil((100 - 10) / 8) = 13 chunks.
        let m = flat_master(100, 100);
        let chunks = split_into_chunks(&m);
        assert_eq!(chunks.len(), 13);
        // Every adjacent chunk pair shares 2 segments.
        for w in chunks.windows(2) {
            let prev = &w[0];
            let next = &w[1];
            assert_eq!(prev[prev.len() - 2], next[0]);
            assert_eq!(prev[prev.len() - 1], next[1]);
        }
    }

    #[test]
    fn chunk_start_offset_aligns_with_supersede_intent() {
        // Uniform 100 ms segments. Stride = 8 → chunk K fires at
        // 8 * K * 100 ms in master time.
        let m = flat_master(100, 100);
        for k in 0..13 {
            assert_eq!(chunk_start_offset_ms(&m, k), (k * 8 * 100) as u32);
        }
    }

    #[test]
    fn chunk_start_offset_handles_variable_segment_durations() {
        // Bar-style master: first three at 200 ms, rest at 50 ms.
        // Stride = 8. Chunk 1 starts at master[8].
        let mut m = vec![(0.5, 200u32); 3];
        m.extend(std::iter::repeat((0.5, 50u32)).take(20));
        // master[0..8] = 3 × 200 + 5 × 50 = 850 ms.
        assert_eq!(chunk_start_offset_ms(&m, 1), 850);
    }

    // ── patterns_equivalent ──────────────────────────────────────────────

    #[test]
    fn patterns_equivalent_matches_when_user_meaningful_fields_match() {
        let a = pattern(1000, vec![0.5, 1.0, 0.5], ChartKind::Line);
        let b = pattern(1000, vec![0.5, 1.0, 0.5], ChartKind::Line);
        assert!(patterns_equivalent(&a, &b));
    }

    #[test]
    fn patterns_equivalent_ignores_uuid_and_timestamps() {
        let a = pattern(1000, vec![0.5], ChartKind::Bar);
        let mut b = pattern(1000, vec![0.5], ChartKind::Bar);
        b.uuid = "different-uuid".to_string();
        b.created_iso = "2026-01-01T00:00:00".to_string();
        b.updated_iso = "2026-05-10T00:00:00".to_string();
        assert!(patterns_equivalent(&a, &b));
    }

    #[test]
    fn patterns_equivalent_distinguishes_different_durations() {
        let a = pattern(1000, vec![0.5], ChartKind::Bar);
        let b = pattern(2000, vec![0.5], ChartKind::Bar);
        assert!(!patterns_equivalent(&a, &b));
    }

    #[test]
    fn patterns_equivalent_distinguishes_different_chart_kinds() {
        let a = pattern(1000, vec![0.5], ChartKind::Bar);
        let b = pattern(1000, vec![0.5], ChartKind::Line);
        assert!(!patterns_equivalent(&a, &b));
    }

    #[test]
    fn patterns_equivalent_distinguishes_different_intensities() {
        let a = pattern(1000, vec![0.5, 0.6], ChartKind::Line);
        let b = pattern(1000, vec![0.5, 0.7], ChartKind::Line);
        assert!(!patterns_equivalent(&a, &b));
    }

    #[test]
    fn patterns_equivalent_distinguishes_different_intensity_lengths() {
        let a = pattern(1000, vec![0.5, 0.6], ChartKind::Line);
        let b = pattern(1000, vec![0.5, 0.6, 0.7], ChartKind::Line);
        assert!(!patterns_equivalent(&a, &b));
    }

    #[test]
    fn patterns_equivalent_tolerates_tiny_intensity_drift() {
        // f32 round-tripping through f64 / DB / JSON can introduce
        // sub-microvolt drift; the tolerance keeps that under the
        // "is this user-meaningfully equal" bar.
        let a = pattern(1000, vec![0.5], ChartKind::Bar);
        let b = pattern(1000, vec![0.5 + 1e-7], ChartKind::Bar);
        assert!(patterns_equivalent(&a, &b));
    }

    #[test]
    fn patterns_equivalent_distinguishes_different_names() {
        let mut a = pattern(1000, vec![0.5], ChartKind::Bar);
        let mut b = pattern(1000, vec![0.5], ChartKind::Bar);
        a.name = "old".to_string();
        b.name = "new".to_string();
        assert!(!patterns_equivalent(&a, &b));
    }
}
