//! Keep-screen-awake bridge (the Android analogue of GTK's
//! `gtk_application_inhibit`). Sets / clears the NativeActivity
//! window's `FLAG_KEEP_SCREEN_ON` via the same app-classloader
//! JNI escape hatch as `guided.rs` / `audio.rs`. Driven from the
//! session lifecycle in `on_state_changed`: on when a session
//! starts in a mode whose `*_keep_screen_awake` setting is true,
//! off on every session-end path. Also sets the system bar symbol
//! colour that `MainWindow.light-bars` asks for (#15).
//!
//! `#[cfg(target_os = "android")]`-gated.

#![cfg(target_os = "android")]

use android_activity::AndroidApp;
use jni::objects::JValue;

const SCREEN_CLASS_DOTTED: &str =
    "io.github.janekbt.Meditate.MeditateScreen";

/// Add / clear the activity window's keep-screen-on flag.
/// Best-effort: a JNI hiccup is logged and the session simply
/// runs without the inhibitor (same posture as the guided
/// bridge).
pub fn set_keep_awake(app: &AndroidApp, on: bool) {
    call_bool(app, "setKeepAwake", on);
}

/// Dark bar symbols over a light page, light ones over a dark one (#15).
/// Best-effort like `set_keep_awake`: a failure keeps today's bars.
pub fn match_system_bars(app: &AndroidApp, light: bool) {
    call_bool(app, "matchSystemBars", light);
}

/// `MeditateScreen.<method>(activity, value)`, logged on failure.
fn call_bool(app: &AndroidApp, method: &str, value: bool) {
    let result = crate::jni_call::with_env(app, |env, activity| {
        let class = crate::jni_call::load_class(env, activity, SCREEN_CLASS_DOTTED)?;
        // NativeActivity IS an android.app.Activity, so it doubles as
        // both the receiver context and the Activity arg.
        env.call_static_method(
            class,
            method,
            "(Landroid/app/Activity;Z)V",
            &[activity.into(), JValue::Bool(u8::from(value))],
        )?;
        Ok(())
    });
    if let Err(e) = result {
        meditate_core::log("screen", &format!("{method}({value}) FAILED: {e:?}"));
    }
}
