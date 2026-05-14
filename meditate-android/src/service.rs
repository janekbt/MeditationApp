//! JNI bridge to the Kotlin foreground service. Called from the
//! AppState transition hooks in `lib.rs`: Idle → Active fires
//! `start(app)`, Active → Idle / Finished fires `stop(app)`.
//!
//! The Kotlin side lives at `kotlin/MeditateSessionService.kt`;
//! `start`/`stop` here mirror its two `@JvmStatic` companion-object
//! helpers exactly, so the JNI invocation is one static-method call
//! per transition. No PostNotificationPermission handling yet —
//! Android 13+ surfaces a system prompt the first time, and
//! declining leaves the service running silently (acceptable
//! degraded mode for Phase 1).
//!
//! ## Classloader caveat (the bug that ate our first attempt)
//!
//! Native threads attached via `JavaVM::attach_current_thread` see
//! only the *system* classloader, which knows about
//! `java.lang.*` / Android framework classes but NOT our app
//! classes. A naive `env.find_class("io/github/janekbt/Meditate/
//! MeditateSessionService")` therefore returns null with a pending
//! `ClassNotFoundException`. The first JNI call after that aborts
//! the process — we saw both symptoms (silent failure on start +
//! crash on stop) before this fix.
//!
//! The escape hatch: ask the activity (not the thread) for its
//! classloader. The activity's classloader is the app classloader,
//! which knows about every class in our APK. Call `loadClass` on
//! it directly.
//!
//! Module is `#[cfg(target_os = "android")]`-gated everywhere; the
//! host `cargo run` path never compiles this file or its deps.

#![cfg(target_os = "android")]

use android_activity::AndroidApp;
use jni::objects::{JClass, JObject};
use jni::JavaVM;

const SERVICE_CLASS_DOTTED: &str = "io.github.janekbt.Meditate.MeditateSessionService";

/// Start the foreground service. Called when AppState transitions
/// Idle → Active. Errors land in logcat rather than propagating —
/// failing to start the service still leaves the UI usable, just
/// without screen-off survival; we don't want a JNI hiccup to brick
/// the Start button.
pub fn start(app: &AndroidApp) {
    if let Err(e) = invoke(app, "start") {
        // eprintln! forwards to logcat via stderr — no log facade
        // wired up in this crate yet (Phase 8's polish pass will
        // hook android_logger or similar).
        eprintln!("MeditateSessionService.start failed: {e}");
    }
}

/// Stop the foreground service. Called when AppState transitions
/// Active → Idle or Active → Finished. Same swallow-error policy
/// as `start`.
pub fn stop(app: &AndroidApp) {
    if let Err(e) = invoke(app, "stop") {
        eprintln!("MeditateSessionService.stop failed: {e}");
    }
}

/// Resolve `MeditateSessionService` through the app classloader,
/// then call its `<method>(Context)` static helper with the
/// activity as the Context argument. Both `start`/`stop` share the
/// signature, so this single helper covers both transitions.
fn invoke(app: &AndroidApp, method: &str) -> Result<(), jni::errors::Error> {
    // SAFETY: `app.vm_as_ptr()` is the JavaVM pointer android-activity
    // received at process start; it stays valid for the process
    // lifetime. JavaVM::from_raw expects a raw `*mut sys::JavaVM`,
    // which is the C-level type the cast yields here.
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }?;
    let mut env = vm.attach_current_thread()?;

    let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };

    // Activity → app classloader → service Class. Going through
    // `loadClass` (dotted name) instead of `find_class`
    // (slash-separated name) is what makes the lookup hit the app
    // classloader; `find_class` would use the thread's classloader,
    // which is the system one on a native-attached thread.
    let classloader = env
        .call_method(&activity, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])?
        .l()?;
    let class_name = env.new_string(SERVICE_CLASS_DOTTED)?;
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
        method,
        "(Landroid/content/Context;)V",
        &[(&activity).into()],
    )?;

    // Defensive: if any earlier call somehow left a pending
    // exception, clear it before this thread's next JNI cycle —
    // otherwise the JVM aborts the process. The clean path through
    // this fn won't leave one behind (call_method / call_static_method
    // both return Err on Java exceptions and the `?` propagates),
    // but explicit clearing keeps a future regression from being a
    // process death.
    if env.exception_check()? {
        env.exception_clear()?;
    }

    Ok(())
}
