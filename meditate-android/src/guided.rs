//! Guided-mode SAF file-picker bridge (Phase 6.5 GM-2).
//!
//! `NativeActivity` never delivers `onActivityResult` to native
//! code (same wall as the widget `onNewIntent`), so a tiny Kotlin
//! Activity (`MeditateFilePickerActivity`) runs
//! `ACTION_OPEN_DOCUMENT`, copies the pick into app storage,
//! probes its duration, and writes a drop-file the Rust tick loop
//! polls — exactly the channel the widget launch uses.
//!
//! Same app-classloader JNI escape hatch as `widget.rs` /
//! `audio.rs`. `#[cfg(target_os = "android")]`-gated.

#![cfg(target_os = "android")]

use android_activity::AndroidApp;
use jni::objects::{JClass, JObject};
use jni::JavaVM;

const PICKER_CLASS_DOTTED: &str =
    "io.github.janekbt.Meditate.MeditateGuidedPicker";
const PLAYER_CLASS_DOTTED: &str =
    "io.github.janekbt.Meditate.MeditateGuided";
/// Drop-file the picker Activity writes: 3 lines —
/// absolute path / display name / duration in whole seconds.
const PICK_FILENAME: &str = "guided_pick";
/// Touched by `MeditateGuided`'s onCompletion/onError — the
/// audio reached its natural end; the tick loop forces the
/// session into Overtime (robust to a probe-vs-real mismatch).
const EOS_FILENAME: &str = "guided_eos";

/// Launch the system audio picker (fire-and-forget). The result
/// arrives asynchronously via the drop-file; poll
/// `take_pending_pick`. Best-effort: a JNI hiccup is logged, the
/// Guided row just stays unset.
pub fn open_picker(app: &AndroidApp) {
    if let Err(e) = invoke_open(app) {
        meditate_core::log(
            "guided",
            &format!("open_picker FAILED: {e:?}"),
        );
    }
}

/// Take the pending pick — `(abs file path, display name,
/// duration secs)` — and delete the drop-file (single
/// consumption, so the tick poll doesn't re-apply it). `None`
/// when nothing is pending / the file is malformed / blank path.
pub fn take_pending_pick(
    app: &AndroidApp,
) -> Option<(String, String, u32)> {
    let data_root = app.internal_data_path()?;
    let path = data_root.join("meditate").join(PICK_FILENAME);
    let raw = std::fs::read_to_string(&path).ok()?;
    // Remove first so a parse failure can't loop every tick.
    let _ = std::fs::remove_file(&path);
    let mut lines = raw.lines();
    let file = lines.next()?.trim().to_string();
    if file.is_empty() {
        return None;
    }
    let name = lines.next().unwrap_or("").trim().to_string();
    let dur: u32 =
        lines.next().unwrap_or("0").trim().parse().unwrap_or(0);
    Some((file, name, dur))
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

fn invoke_open(app: &AndroidApp) -> Result<(), jni::errors::Error> {
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }?;
    let mut env = vm.attach_current_thread()?;
    let activity =
        unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
    let class = resolve_class(&mut env, &activity, PICKER_CLASS_DOTTED)?;
    env.call_static_method(
        class,
        "open",
        "(Landroid/content/Context;)V",
        &[(&activity).into()],
    )?;
    if env.exception_check()? {
        env.exception_clear()?;
    }
    Ok(())
}

// ── Guided audio playback (MeditateGuided) ──────────────────────────

/// Start playing the selected file. Supersedes any prior guided
/// playback. Best-effort: a failure is logged (the session still
/// runs as a silent countdown of the probed length).
pub fn play(app: &AndroidApp, path: &str) {
    if let Err(e) = invoke_play(app, path) {
        meditate_core::log("guided", &format!("play FAILED: {e:?}"));
    }
}

/// Pause / resume / stop the guided track in step with the
/// session's pause/resume/stop. `stop` releases the player.
pub fn pause(app: &AndroidApp) {
    let _ = invoke_player_noarg(app, "pauseAudio");
}
pub fn resume(app: &AndroidApp) {
    let _ = invoke_player_noarg(app, "resumeAudio");
}
pub fn stop(app: &AndroidApp) {
    let _ = invoke_player_noarg(app, "stopAudio");
}

/// Take the natural-end flag the player wrote on
/// onCompletion/onError. Single-consumption (drop-file removed),
/// so the tick loop forces Overtime exactly once.
pub fn take_eos(app: &AndroidApp) -> bool {
    let Some(data_root) = app.internal_data_path() else {
        return false;
    };
    let path = data_root.join("meditate").join(EOS_FILENAME);
    if std::fs::metadata(&path).is_ok() {
        let _ = std::fs::remove_file(&path);
        true
    } else {
        false
    }
}

fn invoke_play(
    app: &AndroidApp,
    path: &str,
) -> Result<(), jni::errors::Error> {
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }?;
    let mut env = vm.attach_current_thread()?;
    let activity =
        unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
    let jpath = env.new_string(path)?;
    let class = resolve_class(&mut env, &activity, PLAYER_CLASS_DOTTED)?;
    env.call_static_method(
        class,
        "startAudio",
        "(Landroid/content/Context;Ljava/lang/String;)V",
        &[(&activity).into(), (&jpath).into()],
    )?;
    if env.exception_check()? {
        env.exception_clear()?;
    }
    Ok(())
}

fn invoke_player_noarg(
    app: &AndroidApp,
    method: &str,
) -> Result<(), jni::errors::Error> {
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }?;
    let mut env = vm.attach_current_thread()?;
    let activity =
        unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
    let class = resolve_class(&mut env, &activity, PLAYER_CLASS_DOTTED)?;
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
