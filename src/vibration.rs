//! Haptic feedback on session end, routed through feedbackd's D-Bus API
//! (org.sigxcpu.Feedback). No-op on systems without feedbackd, so desktop
//! users with the toggle accidentally enabled just get silence.

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

/// Fire a one-shot haptic if the user enabled it. The actual D-Bus call
/// is fire-and-forget — we don't wait for the reply or surface errors.
pub fn trigger_if_enabled(app: &crate::application::MeditateApplication) {
    let enabled = app
        .with_db(|db| db.get_setting("vibrate_on_end", "false"))
        .and_then(|r| r.ok())
        .map(|s| s == "true")
        .unwrap_or(false);
    if !enabled { return; }

    let Ok(conn) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)
    else { return; };

    // TriggerFeedback(app_id, event, hints, timeout) — feedbackd picks the
    // per-event pattern from the active theme. "alarm-clock-elapsed" matches
    // the semantic of a timer ending and is part of the standard event set.
    let hints = glib::VariantDict::new(None).end();
    let args = (crate::config::APP_ID, "alarm-clock-elapsed", &hints, -1i32).to_variant();

    let _ = conn.call_sync(
        Some("org.sigxcpu.Feedback"),
        "/org/sigxcpu/Feedback",
        "org.sigxcpu.Feedback",
        "TriggerFeedback",
        Some(&args),
        None,
        gio::DBusCallFlags::NO_AUTO_START,
        2000,
        gio::Cancellable::NONE,
    );
}
