//! Keep-screen-awake bridge (the Android analogue of GTK's
//! `gtk_application_inhibit`). Sets / clears the NativeActivity
//! window's `FLAG_KEEP_SCREEN_ON` via the same app-classloader
//! JNI escape hatch as `guided.rs` / `audio.rs`. Driven from the
//! session lifecycle in `on_state_changed`: on when a session
//! starts in a mode whose `*_keep_screen_awake` setting is true,
//! off on every session-end path.
//!
//! `#[cfg(target_os = "android")]`-gated.

#![cfg(target_os = "android")]

use android_activity::AndroidApp;
use jni::objects::{JClass, JObject, JValue};
use jni::JavaVM;

const SCREEN_CLASS_DOTTED: &str =
    "io.github.janekbt.Meditate.MeditateScreen";

/// Add / clear the activity window's keep-screen-on flag.
/// Best-effort: a JNI hiccup is logged and the session simply
/// runs without the inhibitor (same posture as the guided
/// bridge).
pub fn set_keep_awake(app: &AndroidApp, on: bool) {
    if let Err(e) = invoke_set_keep_awake(app, on) {
        meditate_core::log(
            "screen",
            &format!("set_keep_awake({on}) FAILED: {e:?}"),
        );
    }
}

fn resolve_class<'a>(
    env: &mut jni::JNIEnv<'a>,
    activity: &JObject,
    dotted: &str,
) -> Result<JClass<'a>, jni::errors::Error> {
    let classloader = env
        .call_method(
            activity,
            "getClassLoader",
            "()Ljava/lang/ClassLoader;",
            &[],
        )?
        .l()?;
    let class_name = env.new_string(dotted)?;
    let class_obj = env
        .call_method(
            &classloader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[(&class_name).into()],
        )?
        .l()?;
    Ok(class_obj.into())
}

fn invoke_set_keep_awake(
    app: &AndroidApp,
    on: bool,
) -> Result<(), jni::errors::Error> {
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }?;
    let mut env = vm.attach_current_thread()?;
    let activity =
        unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
    let class = resolve_class(&mut env, &activity, SCREEN_CLASS_DOTTED)?;
    // NativeActivity IS an android.app.Activity, so it doubles as
    // both the receiver context and the Activity arg.
    env.call_static_method(
        class,
        "setKeepAwake",
        "(Landroid/app/Activity;Z)V",
        &[(&activity).into(), JValue::Bool(u8::from(on))],
    )?;
    if env.exception_check()? {
        env.exception_clear()?;
    }
    Ok(())
}
