//! Haptic feedback on session end, routed through feedbackd's D-Bus API
//! (org.sigxcpu.Feedback). No-op on systems without feedbackd, so desktop
//! users with the toggle accidentally enabled just get silence.

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

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
