//! JNI bridge to the Kotlin audio helper
//! (`kotlin/MeditateAudio.kt`). Phase 5 bell-cue + preview sound
//! playback.
//!
//! Same app-classloader escape hatch as `haptics.rs` /
//! `service.rs`: a native thread attached via
//! `JavaVM::attach_current_thread` only sees the system
//! classloader, so `MeditateAudio` is resolved through the
//! activity's classloader (`loadClass`, dotted name).
//!
//! `play` plays a bundled OGG by absolute file path (the path
//! `sounds::extract_and_seed` wrote into `bell_sounds`); `stop`
//! halts any in-flight playback (preview Stop / supersede).
//!
//! Module is `#[cfg(target_os = "android")]`-gated everywhere;
//! the host `cargo run` path never compiles this file.

#![cfg(target_os = "android")]

use android_activity::AndroidApp;
use jni::objects::{JClass, JObject};
use jni::JavaVM;

const AUDIO_CLASS_DOTTED: &str = "io.github.janekbt.Meditate.MeditateAudio";

/// Play the OGG at `path` (an absolute file path from
/// `bell_sounds.file_path`). Supersedes any sound already
/// playing — preview and the session-end cue share one slot, so
/// a new tap stops the previous like the GTK preview slot does.
/// Failures are logged, never propagated (a missing file or a
/// JNI hiccup must not break the session flow).
pub fn play(app: &AndroidApp, path: &str) {
    if path.is_empty() {
        return;
    }
    if let Err(e) = invoke_play(app, path) {
        meditate_core::log(
            "audio.play",
            &format!("play FAILED path={path}: {e:?}"),
        );
    }
}

/// Stop + release any in-flight playback (preview Stop /
/// supersede).
pub fn stop(app: &AndroidApp) {
    if let Err(e) = invoke_no_arg(app, "stop") {
        meditate_core::log("audio.play", &format!("stop FAILED: {e:?}"));
    }
}

fn resolve_class<'a>(
    env: &mut jni::JNIEnv<'a>,
    activity: &JObject,
) -> Result<JClass<'a>, jni::errors::Error> {
    let classloader = env
        .call_method(activity, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])?
        .l()?;
    let class_name = env.new_string(AUDIO_CLASS_DOTTED)?;
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

fn invoke_play(
    app: &AndroidApp,
    path: &str,
) -> Result<(), jni::errors::Error> {
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }?;
    let mut env = vm.attach_current_thread()?;
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };

    let jpath = env.new_string(path)?;
    let class = resolve_class(&mut env, &activity)?;
    env.call_static_method(
        class,
        "play",
        "(Landroid/content/Context;Ljava/lang/String;)V",
        &[(&activity).into(), (&jpath).into()],
    )?;

    if env.exception_check()? {
        env.exception_clear()?;
    }
    Ok(())
}

fn invoke_no_arg(
    app: &AndroidApp,
    method: &str,
) -> Result<(), jni::errors::Error> {
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }?;
    let mut env = vm.attach_current_thread()?;
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
    let class = resolve_class(&mut env, &activity)?;
    env.call_static_method(
        class,
        method,
        "(Landroid/content/Context;)V",
        &[(&activity).into()],
    )?;
    if env.exception_check()? {
        env.exception_clear()?;
    }
    Ok(())
}
