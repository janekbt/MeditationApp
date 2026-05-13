//! Haptic feedback on session end, routed through feedbackd's D-Bus API
//! (org.sigxcpu.Feedback). No-op on systems without feedbackd, so desktop
//! users with the toggle accidentally enabled just get silence.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use meditate_core::vibration::{build_master_envelope, chunk_start_offset_ms, split_into_chunks};

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

// ── Playback driver ──────────────────────────────────────────────────────
// Pattern sampler / chunker / RLE encoder live in
// `meditate_core::vibration` so any shell sharing the haptic schema
// can reuse them. The driver below is the GTK-side glue that ships
// the resulting (amplitude, duration_ms) chunks through feedbackd's
// D-Bus interface.

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


