//! Haptic feedback on session end, routed through feedbackd's D-Bus API
//! (org.sigxcpu.Feedback). No-op on systems without feedbackd, so desktop
//! users with the toggle accidentally enabled just get silence.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

/// Probe whether the device exposes feedbackd's `Haptic` interface.
/// Synchronous DBus call to `Vibrate(app_id, [])` on the session bus —
/// the empty `a(du)` is the documented no-op cancel, so the probe
/// doesn't actually buzz. The `Haptic` interface is exported only when
/// a vibration motor is present, so a successful call confirms both
/// feedbackd and motor presence.
///
/// Returns `false` on any failure: bus unreachable, service file
/// missing (laptop), service auto-start failed, interface missing (no
/// motor), or method timeout. Auto-start is allowed (`DBusCallFlags::
/// NONE`) so a freshly-booted phone with lazily-started feedbackd
/// doesn't falsely report `false` on first launch.
///
/// Intended to run once at app startup; result cached on
/// `MeditateApplication`. Worst-case wait is the 500 ms timeout
/// ceiling, but typical perceived freeze is <50 ms (feedbackd answers
/// in tens of ms on the phone, DBus returns `ServiceUnknown` near-
/// instantly when the service file isn't installed).
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_haptic_returns_false_when_no_feedbackd_present() {
        // Smoke test: on the dev laptop there's no feedbackd service
        // exposing org.sigxcpu.Feedback.Haptic, so the probe must
        // return false gracefully — without panicking, without
        // blocking past the 500 ms timeout, and without an unhandled
        // DBus error escaping. This is the path every laptop user
        // hits at startup; the on-device "returns true" half of the
        // contract is verified in the on-device test pass (step 10).
        assert!(!probe_haptic());
    }
}

pub fn probe_haptic() -> bool {
    let Ok(conn) = gio::bus_get_sync(
        gio::BusType::Session,
        gio::Cancellable::NONE,
    ) else {
        return false;
    };
    // a(du) — array of (amplitude:f64, duration_ms:u32). Empty form
    // matches the upstream-documented "stop any in-flight pattern"
    // primitive, harmless to fire as a probe.
    let empty_pattern: Vec<(f64, u32)> = Vec::new();
    let args = glib::Variant::tuple_from_iter([
        crate::config::APP_ID.to_variant(),
        empty_pattern.to_variant(),
    ]);
    conn.call_sync(
        Some("org.sigxcpu.Feedback"),
        "/org/sigxcpu/Feedback",
        "org.sigxcpu.Feedback.Haptic",
        "Vibrate",
        Some(&args),
        None,
        gio::DBusCallFlags::NONE,
        500,
        gio::Cancellable::NONE,
    )
    .is_ok()
}

// trigger_if_enabled used to be the entire vibration system: a single
// fire-and-forget haptic at session end, gated by a vibrate_on_end
// boolean. Replaced in step 9 by per-bell + per-phase + per-mode
// pattern-driven playback through PatternPlayback below. The old
// vibrate_on_end setting + the Preferences toggle that drove it are
// also gone.

// ── Pattern sampler ──────────────────────────────────────────────────────
// Pure-Rust translation from VibrationPattern → the (amplitude,
// duration_ms) tuple sequence feedbackd's Haptic.Vibrate consumes.
// Lives here so the playback driver further down can reuse it; tests
// are laptop-runnable without any DBus or motor.

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
/// INTENSITY_STEP matches this).
///
/// The editor's authored values land on exact 0.05 multiples
/// (which don't have exact f32 representations — 0.95 stores as
/// 0.9499999…), so direct `(v * 10).round()` would map 0.95 to 9
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
pub fn build_master_envelope(p: &crate::db::VibrationPattern) -> Vec<(f64, u32)> {
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
fn split_into_chunks(master: &[(f64, u32)]) -> Vec<Vec<(f64, u32)>> {
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
fn chunk_start_offset_ms(master: &[(f64, u32)], k: usize) -> u32 {
    if k == 0 {
        return 0;
    }
    let stride = (MAX_SEGMENTS_PER_CHUNK - CHUNK_OVERLAP_SEGMENTS) as usize;
    let first_seg = (k * stride).min(master.len());
    master[..first_seg].iter().map(|(_, d)| *d).sum()
}

// ── Playback driver ──────────────────────────────────────────────────────

/// Build the `(s, a(du))` argument tuple for `Haptic.Vibrate`. The
/// pattern variant is constructed from the segment vec — empty vec
/// is the documented no-op cancel form.
fn build_vibrate_args(segments: &[(f64, u32)]) -> glib::Variant {
    // a(du) — array of (amplitude, duration_ms). Build by collecting
    // a Vec<glib::Variant> of inner tuples, then wrapping into the
    // typed array.
    let inner: Vec<glib::Variant> = segments
        .iter()
        .map(|(amp, dur)| {
            glib::Variant::tuple_from_iter([
                amp.to_variant(),
                dur.to_variant(),
            ])
        })
        .collect();
    let pattern_variant = glib::Variant::array_from_iter_with_type(
        glib::VariantTy::new("(du)").expect("(du) is a valid variant type"),
        inner.iter().cloned(),
    );
    glib::Variant::tuple_from_iter([
        crate::config::APP_ID.to_variant(),
        pattern_variant,
    ])
}

/// Handle for an in-flight feedbackd vibration. Drop / `stop()`
/// fires `Vibrate(app_id, [])` to cancel — feedbackd's documented
/// no-op pattern. Spawned async on the GLib main context so the
/// caller never blocks waiting for DBus.
///
/// `cancel_on_drop` defaults true. Setting it false via `disarm`
/// skips the cancel — used when this handle is being replaced by
/// a new `Vibrate(app_id, ...)` call from the same app, since
/// feedbackd already replaces in-flight patterns per-app on each
/// new Vibrate. Without disarm the cancel would race behind the
/// new pattern's call_future and silently kill it.
#[derive(Debug)]
pub struct PatternPlayback {
    cancel: Arc<AtomicBool>,
    cancel_on_drop: bool,
}

impl PatternPlayback {
    /// Fire `pattern` through feedbackd's `Haptic.Vibrate`. Returns a
    /// handle whose Drop / stop() cancels mid-playback. No-op when
    /// `app.has_haptic()` is false — the laptop authoring path stays
    /// silent without going near the bus.
    pub fn play(
        app: &crate::application::MeditateApplication,
        pattern: &crate::db::VibrationPattern,
    ) -> Self {
        let cancel = Arc::new(AtomicBool::new(false));
        if !app.has_haptic() {
            return Self { cancel, cancel_on_drop: true };
        }
        let master = build_master_envelope(pattern);
        if master.is_empty() {
            return Self { cancel, cancel_on_drop: true };
        }
        let chunks = split_into_chunks(&master);

        // Fire chunk 0 immediately. Each subsequent chunk is
        // scheduled to fire shortly *before* the previous one
        // ends — feedbackd's per-app supersede swaps it in mid-
        // playback, and the 2-segment overlap means both chunks
        // describe the same amplitude at the supersede instant
        // so the swap is inaudible.
        for (k, chunk) in chunks.iter().enumerate() {
            let segments = chunk.clone();
            let cancel_clone = cancel.clone();
            let fire = move || {
                if cancel_clone.load(Ordering::Relaxed) { return; }
                let cancel_inner = cancel_clone.clone();
                glib::MainContext::default().spawn_local(async move {
                    if cancel_inner.load(Ordering::Relaxed) { return; }
                    let Ok(conn) = gio::bus_get_future(gio::BusType::Session).await else {
                        return;
                    };
                    if cancel_inner.load(Ordering::Relaxed) { return; }
                    let args = build_vibrate_args(&segments);
                    let _ = conn
                        .call_future(
                            Some("org.sigxcpu.Feedback"),
                            "/org/sigxcpu/Feedback",
                            "org.sigxcpu.Feedback.Haptic",
                            "Vibrate",
                            Some(&args),
                            None,
                            gio::DBusCallFlags::NONE,
                            -1,
                        )
                        .await;
                });
            };

            if k == 0 {
                fire();
            } else {
                let delay_ms = chunk_start_offset_ms(&master, k);
                glib::timeout_add_local_once(
                    std::time::Duration::from_millis(delay_ms as u64),
                    fire,
                );
            }
        }

        Self { cancel, cancel_on_drop: true }
    }

    /// Skip the Drop cancel. Use when this handle is being replaced
    /// by another `Vibrate(...)` from the same app — feedbackd
    /// already supersedes per-app, so an explicit cancel here would
    /// race behind the replacement and silently kill it.
    pub fn disarm(&mut self) {
        self.cancel_on_drop = false;
    }

    /// Cancel the in-flight pattern. Sets the cancel flag (so the
    /// async task short-circuits if it hasn't fired the call yet) AND
    /// fires `Vibrate(app_id, [])` to stop any pattern feedbackd is
    /// already playing. Latency = one DBus round-trip (~10–30 ms).
    pub fn stop(&self) {
        self.cancel.store(true, Ordering::Relaxed);
        glib::MainContext::default().spawn_local(async {
            let Ok(conn) = gio::bus_get_future(gio::BusType::Session).await else {
                return;
            };
            let args = build_vibrate_args(&[]);
            let _ = conn
                .call_future(
                    Some("org.sigxcpu.Feedback"),
                    "/org/sigxcpu/Feedback",
                    "org.sigxcpu.Feedback.Haptic",
                    "Vibrate",
                    Some(&args),
                    None,
                    gio::DBusCallFlags::NONE,
                    -1,
                )
                .await;
        });
    }
}

impl Drop for PatternPlayback {
    fn drop(&mut self) {
        if self.cancel_on_drop {
            self.stop();
        }
    }
}

// ── Channel-resolution helpers ───────────────────────────────────────────
// Whether sound / vibration should fire for a given bell or phase
// is the AND of two gates: the per-bell (or per-phase) signal_mode
// (intent for this row), and the per-mode "what plays" toggle
// (override for this session-mode). Both have three values —
// Sound / Vibration / Both — so the resolution is the intersection
// of the two.

/// Read a per-mode `signal_mode` setting key, defaulting to Both
/// when the value is missing or unparseable.
fn read_mode_signal_mode(
    app: &crate::application::MeditateApplication,
    setting_key: &'static str,
) -> crate::db::SignalMode {
    let raw = app
        .with_db(|db| db.get_setting(setting_key, "both"))
        .and_then(|r| r.ok())
        .unwrap_or_else(|| "both".to_string());
    crate::db::SignalMode::from_db_str(&raw).unwrap_or(crate::db::SignalMode::Both)
}

/// True if the sound channel should fire given per-bell intent +
/// the per-mode override.
pub fn should_fire_sound(
    app: &crate::application::MeditateApplication,
    per_bell: crate::db::SignalMode,
    mode_setting_key: &'static str,
) -> bool {
    use crate::db::SignalMode::*;
    let mode = read_mode_signal_mode(app, mode_setting_key);
    matches!(per_bell, Sound | Both) && matches!(mode, Sound | Both)
}

/// Fire the configured vibration pattern for a bell or phase if
/// every gate allows: device has haptic, per-bell signal_mode
/// includes vibration, per-mode override includes vibration. Looks
/// up the pattern by uuid and returns a `PatternPlayback` handle —
/// the caller MUST stash the handle to keep the pattern alive
/// through its natural duration (drop fires cancel).
pub fn fire_pattern_if_allowed(
    app: &crate::application::MeditateApplication,
    per_bell: crate::db::SignalMode,
    mode_setting_key: &'static str,
    pattern_uuid: &str,
) -> Option<PatternPlayback> {
    use crate::db::SignalMode::*;
    if !app.has_haptic() { return None; }
    if !matches!(per_bell, Vibration | Both) { return None; }
    let mode = read_mode_signal_mode(app, mode_setting_key);
    if !matches!(mode, Vibration | Both) { return None; }
    let pattern = app
        .with_db(|db| db.find_vibration_pattern_by_uuid(pattern_uuid))
        .and_then(|r| r.ok())
        .flatten()?;
    Some(PatternPlayback::play(app, &pattern))
}

/// Linearly interpolate the Line-mode envelope at time `t_ms`.
/// Maps `t_ms / duration_ms` onto the `[0, n-1]` index range,
/// returns the lerp between the two adjacent control points.
fn sample_line_at(p: &crate::db::VibrationPattern, t_ms: u32) -> f32 {
    let n = p.intensities.len();
    let denom = (n - 1).max(1) as f32;
    let xf = (t_ms as f32 / p.duration_ms as f32) * denom;
    let lo = xf.floor() as usize;
    let hi = (lo + 1).min(n - 1);
    let frac = xf - lo as f32;
    p.intensities[lo] * (1.0 - frac) + p.intensities[hi] * frac
}

#[cfg(test)]
mod sampler_tests {
    use super::*;
    use crate::db::{ChartKind, VibrationPattern};

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
}
