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

/// feedbackd caps `Vibrate` at 10 (amplitude, duration_ms) tuples
/// per call (silent truncation in `fbd-haptic-manager.c`'s
/// `MAX_ITEMS = 10`). Anything past the tenth segment is dropped
/// without an error reply, so a 40-segment 2-second envelope plays
/// only the first ~500 ms. Cap our output here so the duration
/// always matches what the user authored.
const MAX_SEGMENTS: u32 = 10;
/// Preferred Line-mode tick when the pattern is short enough to
/// fit in MAX_SEGMENTS. For longer patterns the per-segment
/// duration is stretched (the segment count is capped, the total
/// duration is preserved).
const LINE_TICK_MS: u32 = 50;

/// Translate a `VibrationPattern` into the `Vec<(f64, u32)>` tuple
/// sequence `Haptic.Vibrate(s, a(du))` consumes.
///
/// Both shape modes coalesce to at most `MAX_SEGMENTS` (= 10)
/// tuples — feedbackd's hard limit — and the segment durations
/// always sum to `pattern.duration_ms` exactly (remainder lands
/// on the last segment).
///
/// * `Bar` mode emits min(N, 10) segments. When N > 10, adjacent
///   control points are averaged into bins.
/// * `Line` mode samples the linearly-interpolated envelope at
///   the centre of each output segment.
///
/// Returns an empty vec for empty / zero-duration inputs.
pub fn sample_to_segments(p: &crate::db::VibrationPattern) -> Vec<(f64, u32)> {
    let n_in = p.intensities.len();
    if n_in == 0 || p.duration_ms == 0 {
        return Vec::new();
    }

    let n_out: u32 = match p.chart_kind {
        crate::db::ChartKind::Bar => (n_in as u32).min(MAX_SEGMENTS).max(1),
        crate::db::ChartKind::Line => {
            let raw_ticks = (p.duration_ms + LINE_TICK_MS - 1) / LINE_TICK_MS;
            raw_ticks.max(1).min(MAX_SEGMENTS)
        }
    };

    let base = p.duration_ms / n_out;
    let remainder = p.duration_ms - base * n_out;

    (0..n_out)
        .map(|i| {
            let dur = if i == n_out - 1 { base + remainder } else { base };
            let mag: f32 = match p.chart_kind {
                crate::db::ChartKind::Bar => {
                    let lo = (i as usize * n_in) / n_out as usize;
                    let hi_raw = ((i as usize + 1) * n_in) / n_out as usize;
                    let hi = hi_raw.max(lo + 1).min(n_in);
                    let slice = &p.intensities[lo..hi];
                    let sum: f32 = slice.iter().sum();
                    sum / slice.len() as f32
                }
                crate::db::ChartKind::Line => {
                    // Sample at the centre of segment i → t = (i + 0.5) * D / n_out.
                    // Integer-clean: (2i + 1) * D / (2 * n_out). Both factors
                    // fit in u32 for D ≤ 10 000 ms and n_out ≤ 10.
                    let t_ms = ((2 * i + 1) * p.duration_ms) / (2 * n_out);
                    sample_line_at(p, t_ms)
                }
            };
            (mag as f64, dur)
        })
        .collect()
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
        &glib::VariantTy::new("(du)").expect("(du) is a valid variant type"),
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
        let segments = sample_to_segments(pattern);
        if segments.is_empty() {
            return Self { cancel, cancel_on_drop: true };
        }
        let cancel_clone = cancel.clone();
        glib::MainContext::default().spawn_local(async move {
            if cancel_clone.load(Ordering::Relaxed) { return; }
            let Ok(conn) = gio::bus_get_future(gio::BusType::Session).await else {
                return;
            };
            if cancel_clone.load(Ordering::Relaxed) { return; }
            let args = build_vibrate_args(&segments);
            // success out-arg is parsed from the returned tuple but we
            // ignore it — feedbackd may refuse if a higher-priority
            // event is mid-flight, which is fine. No-op-on-failure
            // matches the existing trigger_if_enabled shape.
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

    #[test]
    fn empty_intensities_yields_empty_segments() {
        let p = pattern(1000, vec![], ChartKind::Line);
        assert!(sample_to_segments(&p).is_empty());
    }

    #[test]
    fn zero_duration_yields_empty_segments() {
        let p = pattern(0, vec![0.5, 1.0], ChartKind::Line);
        assert!(sample_to_segments(&p).is_empty());
    }

    #[test]
    fn bar_mode_emits_n_equal_segments_with_remainder_on_last() {
        // 1003 ms / 5 segments = 200 base + 3 ms remainder on last.
        let p = pattern(
            1003,
            vec![0.2, 0.5, 1.0, 0.5, 0.2],
            ChartKind::Bar,
        );
        let segments = sample_to_segments(&p);
        assert_eq!(segments.len(), 5);
        for (i, s) in segments.iter().take(4).enumerate() {
            assert_eq!(s.1, 200, "segment {i} should be 200ms");
        }
        assert_eq!(segments[4].1, 203, "last segment absorbs the 3ms remainder");
        let total: u32 = segments.iter().map(|s| s.1).sum();
        assert_eq!(total, 1003, "segment durations sum to pattern duration");
    }

    #[test]
    fn bar_mode_amplitudes_match_intensities() {
        let p = pattern(
            500,
            vec![0.0, 0.3, 0.7, 1.0],
            ChartKind::Bar,
        );
        let segments = sample_to_segments(&p);
        assert_eq!(
            segments.iter().map(|s| s.0).collect::<Vec<_>>(),
            vec![0.0, 0.3, 0.7, 1.0]
                .into_iter()
                .map(|v: f32| v as f64)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn line_mode_caps_at_max_segments_and_preserves_total_duration() {
        // 1 000 ms / 50 ms = 20 ticks raw, capped at 10. Each
        // segment then spans 100 ms.
        let p = pattern(1000, vec![0.0, 1.0], ChartKind::Line);
        let segments = sample_to_segments(&p);
        assert_eq!(segments.len(), 10, "feedbackd caps at 10 segments");
        for s in &segments[..segments.len() - 1] {
            assert_eq!(s.1, 100);
        }
        let total: u32 = segments.iter().map(|s| s.1).sum();
        assert_eq!(total, 1000, "durations sum to pattern duration");
    }

    #[test]
    fn line_mode_short_pattern_uses_50ms_tick() {
        // 300 ms < 10 × 50 ms → no capping, ticks stay at 50 ms.
        let p = pattern(300, vec![0.0, 1.0], ChartKind::Line);
        let segments = sample_to_segments(&p);
        assert_eq!(segments.len(), 6);
        for s in segments.iter() {
            assert_eq!(s.1, 50);
        }
    }

    #[test]
    fn line_mode_samples_envelope_at_segment_centres() {
        // Symmetric ramp from 0.0 to 1.0 with 10 output segments
        // (each 100 ms). Sample centres land at 50, 150, 250, …,
        // 950 ms → t/D = 0.05, 0.15, …, 0.95. With two control
        // points (linear interp from 0 to 1), each amp = t/D.
        let p = pattern(1000, vec![0.0, 1.0], ChartKind::Line);
        let segments = sample_to_segments(&p);
        for (i, s) in segments.iter().enumerate() {
            let expected = (i as f64 * 100.0 + 50.0) / 1000.0;
            assert!((s.0 - expected).abs() < 1e-3,
                "segment {i}: got {}, expected {}", s.0, expected);
        }
    }

    #[test]
    fn line_mode_caps_long_pattern_at_max_segments() {
        // 5 s pattern: 100 ticks raw → capped at 10 × 500 ms.
        let p = pattern(5_000, vec![0.0, 1.0, 0.0], ChartKind::Line);
        let segments = sample_to_segments(&p);
        assert_eq!(segments.len(), 10, "cap at MAX_SEGMENTS");
        let total: u32 = segments.iter().map(|s| s.1).sum();
        assert_eq!(total, 5_000, "total duration preserved despite capping");
    }

    #[test]
    fn bar_mode_coalesces_when_n_exceeds_max_segments() {
        // 12 control points → 10 output bins. First 8 bins hold
        // one input each, last 2 bins hold 2 inputs averaged.
        let intensities: Vec<f32> = (0..12).map(|i| i as f32 / 11.0).collect();
        let p = pattern(1_000, intensities.clone(), ChartKind::Bar);
        let segments = sample_to_segments(&p);
        assert_eq!(segments.len(), 10, "cap at MAX_SEGMENTS");
        let total: u32 = segments.iter().map(|s| s.1).sum();
        assert_eq!(total, 1_000, "total duration preserved");
        // Bins are non-decreasing for a non-decreasing input.
        let amps: Vec<f64> = segments.iter().map(|s| s.0).collect();
        for w in amps.windows(2) {
            assert!(w[0] <= w[1] + 1e-9,
                "amplitudes should stay monotone: {:?}", amps);
        }
    }
}
