//! JNI bridge to the Kotlin haptics helper
//! (`kotlin/MeditateHaptics.kt`). Phase 5 bell-cue vibration.
//!
//! Same app-classloader escape hatch as `service.rs`: a native
//! thread attached via `JavaVM::attach_current_thread` only sees the
//! system classloader, so resolving `MeditateHaptics` via
//! `find_class` fails with a pending `ClassNotFoundException`. We go
//! through the activity's classloader (`loadClass`, dotted name) —
//! the app classloader knows every class in our APK.
//!
//! B-1 exposes `vibrate_oneshot` only (the minimal end-to-end JNI +
//! Vibrator proof). B-2 will add a waveform call carrying the
//! `meditate_core::vibration` envelope.
//!
//! Module is `#[cfg(target_os = "android")]`-gated everywhere; the
//! host `cargo run` path never compiles this file or its deps.

#![cfg(target_os = "android")]

use android_activity::AndroidApp;
use jni::objects::{JClass, JObject};
use jni::JavaVM;

const HAPTICS_CLASS_DOTTED: &str = "io.github.janekbt.Meditate.MeditateHaptics";

/// Fire a single `durationMs` vibration. Errors land in logcat
/// rather than propagating — a JNI hiccup or a device with no
/// vibration motor must not break the session flow (haptics are an
/// enhancement, not a guarantee).
pub fn vibrate_oneshot(app: &AndroidApp, duration_ms: i64) {
    // Failures go to the diagnostics log (reaches the in-app log +
    // logcat reliably, unlike `eprintln` → stderr which the
    // Android runtime doesn't always route). Success is silent —
    // haptics are an enhancement; a JNI hiccup or a motorless
    // device must not break the session flow.
    if let Err(e) = invoke_oneshot(app, duration_ms) {
        meditate_core::log(
            "haptics.vibrate",
            &format!("vibrateOneShot FAILED duration_ms={duration_ms}: {e:?}"),
        );
    }
}

fn invoke_oneshot(
    app: &AndroidApp,
    duration_ms: i64,
) -> Result<(), jni::errors::Error> {
    // SAFETY: identical contract to `service.rs::invoke` — the
    // JavaVM pointer android-activity received at process start is
    // valid for the process lifetime; the cast yields the C-level
    // `*mut sys::JavaVM` `from_raw` expects.
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }?;
    let mut env = vm.attach_current_thread()?;

    let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };

    let classloader = env
        .call_method(&activity, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])?
        .l()?;
    let class_name = env.new_string(HAPTICS_CLASS_DOTTED)?;
    let class_obj = env
        .call_method(
            &classloader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[(&class_name).into()],
        )?
        .l()?;
    let class: JClass = class_obj.into();

    env.call_static_method(
        class,
        "vibrateOneShot",
        "(Landroid/content/Context;J)V",
        &[(&activity).into(), jni::objects::JValue::Long(duration_ms)],
    )?;

    // Same defensive exception clear as the service bridge: a stray
    // pending exception left for this thread's next JNI cycle aborts
    // the JVM. The clean path won't leave one (the `?`s propagate
    // Java exceptions as Err), but explicit clearing keeps a future
    // regression from being a process death.
    if env.exception_check()? {
        env.exception_clear()?;
    }

    Ok(())
}
