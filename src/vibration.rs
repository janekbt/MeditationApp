//! Haptic feedback on session end, routed through feedbackd's D-Bus API
//! (org.sigxcpu.Feedback). No-op on systems without feedbackd, so desktop
//! users with the toggle accidentally enabled just get silence.

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
