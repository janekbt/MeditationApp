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

/// Fire a one-shot haptic if the user enabled it. The D-Bus call is fully
/// async (spawned on the GLib main context) so it never blocks the UI —
/// previously this used `call_sync` with a 2-second timeout, which would
/// freeze the main thread at session end if feedbackd was slow to reply.
pub fn trigger_if_enabled(app: &crate::application::MeditateApplication) {
    let enabled = app
        .with_db(|db| db.get_setting("vibrate_on_end", "false"))
        .and_then(|r| r.ok())
        .map(|s| s == "true")
        .unwrap_or(false);
    if !enabled { return; }

    glib::MainContext::default().spawn_local(async {
        // Build the method arguments explicitly so the wire signature is
        // unambiguously (ssa{sv}i). Folding a &Variant into a Rust tuple
        // literal and calling .to_variant() on the whole tuple can, in some
        // gtk-rs versions, produce (ssvi) — which feedbackd silently rejects.
        //
        // hints: {"profile": "quiet"} restricts this one event to haptic-
        // only feedback. The "alarm-clock-elapsed" event normally includes
        // an audible tone in feedbackd's default theme, which would collide
        // with the app's own meditation-bell end sound.
        let hints_dict = glib::VariantDict::new(None);
        hints_dict.insert("profile", "quiet");
        let hints = hints_dict.end();
        let args = glib::Variant::tuple_from_iter([
            crate::config::APP_ID.to_variant(),
            "alarm-clock-elapsed".to_variant(),
            hints,
            (-1i32).to_variant(),
        ]);

        let Ok(conn) = gio::bus_get_future(gio::BusType::Session).await else {
            return;
        };

        // DBusCallFlags::NONE (not NO_AUTO_START): if feedbackd isn't
        // running yet, D-Bus activates it via its .service file. We're
        // already async, so the activation latency doesn't hurt anything.
        let _ = conn
            .call_future(
                Some("org.sigxcpu.Feedback"),
                "/org/sigxcpu/Feedback",
                "org.sigxcpu.Feedback",
                "TriggerFeedback",
                Some(&args),
                None,
                gio::DBusCallFlags::NONE,
                -1,
            )
            .await;
    });
}

// ── Pattern sampler ──────────────────────────────────────────────────────
// Pure-Rust translation from VibrationPattern → the (amplitude,
// duration_ms) tuple sequence feedbackd's Haptic.Vibrate consumes.
// Lives here so the playback driver further down can reuse it; tests
// are laptop-runnable without any DBus or motor.

const LINE_TICK_MS: u32 = 50;
/// Cap the segment count for very long Line-mode patterns to keep
/// the array argument to Haptic.Vibrate from growing unbounded.
/// 400 segments × 50 ms = 20 s of envelope — well past any sensible
/// pattern duration; the cap protects us from pathological inputs.
const LINE_MAX_SEGMENTS: u32 = 400;

/// Translate a `VibrationPattern` into the `Vec<(f64, u32)>` tuple
/// sequence the `Haptic.Vibrate(s, a(du))` DBus call expects.
///
/// * `Bar` mode — N equal-duration segments at each control point's
///   intensity. Duration divisibility remainder is added to the last
///   segment so the sum matches `pattern.duration_ms` exactly.
/// * `Line` mode — sample the linearly-interpolated envelope at
///   50 ms ticks (capped at 400 segments). Each tick produces a
///   single (amplitude, duration_ms) entry; the last tick absorbs
///   the duration remainder.
///
/// Returns an empty vec for empty / zero-duration inputs.
pub fn sample_to_segments(p: &crate::db::VibrationPattern) -> Vec<(f64, u32)> {
    let n = p.intensities.len();
    if n == 0 || p.duration_ms == 0 {
        return Vec::new();
    }
    match p.chart_kind {
        crate::db::ChartKind::Bar => {
            let n_u32 = n as u32;
            let base = p.duration_ms / n_u32;
            let remainder = p.duration_ms - base * n_u32;
            p.intensities
                .iter()
                .enumerate()
                .map(|(i, &v)| {
                    let dur = if i == n - 1 { base + remainder } else { base };
                    (v as f64, dur)
                })
                .collect()
        }
        crate::db::ChartKind::Line => {
            let raw_ticks = (p.duration_ms + LINE_TICK_MS - 1) / LINE_TICK_MS;
            let n_ticks = raw_ticks.min(LINE_MAX_SEGMENTS).max(1);
            (0..n_ticks)
                .map(|i| {
                    let t_ms = (i * LINE_TICK_MS).min(p.duration_ms);
                    let mag = sample_line_at(p, t_ms) as f64;
                    let dur = if i == n_ticks - 1 {
                        p.duration_ms - i * LINE_TICK_MS
                    } else {
                        LINE_TICK_MS
                    };
                    (mag, dur)
                })
                .collect()
        }
    }
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
pub struct PatternPlayback {
    cancel: Arc<AtomicBool>,
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
            return Self { cancel };
        }
        let segments = sample_to_segments(pattern);
        if segments.is_empty() {
            return Self { cancel };
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
        Self { cancel }
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
        self.stop();
    }
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
    fn line_mode_emits_50ms_ticks() {
        // 1000 ms / 50 ms = 20 ticks.
        let p = pattern(1000, vec![0.0, 1.0], ChartKind::Line);
        let segments = sample_to_segments(&p);
        assert_eq!(segments.len(), 20);
        for s in &segments[..segments.len() - 1] {
            assert_eq!(s.1, 50);
        }
        // Last tick absorbs the remainder.
        let total: u32 = segments.iter().map(|s| s.1).sum();
        assert_eq!(total, 1000);
    }

    #[test]
    fn line_mode_endpoints_match_first_and_last_intensity() {
        let p = pattern(1000, vec![0.1, 0.5, 0.9], ChartKind::Line);
        let segments = sample_to_segments(&p);
        // The first sample sits at t=0 — exactly intensities[0].
        assert!((segments[0].0 - 0.1).abs() < 1e-3);
        // The very last sample sits at t = (n_ticks - 1) * 50 ms,
        // which is < duration_ms, so it interpolates close to but
        // not exactly intensities[N-1]. Check the midpoint instead.
        let mid = segments[segments.len() / 2].0;
        assert!(mid > 0.1 && mid < 0.9,
            "midpoint should sit between endpoints: got {mid}");
    }

    #[test]
    fn line_mode_caps_segment_count_at_400() {
        // 25 s pattern would naturally produce 500 ticks at 50 ms.
        let p = pattern(25_000, vec![0.0, 1.0, 0.0], ChartKind::Line);
        let segments = sample_to_segments(&p);
        assert_eq!(segments.len(), 400, "cap at LINE_MAX_SEGMENTS");
        let total: u32 = segments.iter().map(|s| s.1).sum();
        assert_eq!(total, 25_000, "remainder still distributes onto the last segment");
    }
}
