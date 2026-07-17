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
const IMPORT_CLASS_DOTTED: &str =
    "io.github.janekbt.Meditate.MeditateGuidedImport";
/// Touched by `MeditateGuidedImport` when the background transcode
/// finishes: "ok" or "err:<message>". Single-consumption.
const IMPORT_RESULT_FILENAME: &str = "guided_import_result";
/// Continuously rewritten by `MeditateGuidedImport` with the
/// transcode percent (0–99). Polled every tick (NOT consumed —
/// it's overwritten in place); removed at finalize.
const IMPORT_PROGRESS_FILENAME: &str = "guided_import_progress";
/// Written by Rust when the user taps Cancel mid-transcode; the
/// Kotlin worker polls it each loop and aborts (deleting the
/// partial dest), mirroring GTK's `cancel: &AtomicBool`.
const IMPORT_CANCEL_FILENAME: &str = "guided_import_cancel";
/// Drop-file the picker Activity writes: 3 lines —
/// absolute path / display name / duration in whole seconds.
const PICK_FILENAME: &str = "guided_pick";
/// Same 3-line format, written when the picker was opened with
/// target="bell" (BI custom-sound import) — separate file so the
/// two import flows can't consume each other's picks.
const SOUND_PICK_FILENAME: &str = "sound_pick";
/// Touched by `MeditateGuided`'s onCompletion/onError — the
/// audio reached its natural end; the tick loop forces the
/// session into Overtime (robust to a probe-vs-real mismatch).
const EOS_FILENAME: &str = "guided_eos";

/// Launch the system audio picker (fire-and-forget). The result
/// arrives asynchronously via the drop-file; poll
/// `take_pending_pick`. Best-effort: a JNI hiccup is logged, the
/// Guided row just stays unset.
pub fn open_picker(app: &AndroidApp) {
    if let Err(e) = invoke_open(app, "guided") {
        meditate_core::log(
            "guided",
            &format!("open_picker FAILED: {e:?}"),
        );
    }
}

/// Same picker, bell-import route: the transient copy lands in
/// `sounds/` and the result in the `sound_pick` drop-file (BI).
pub fn open_sound_picker(app: &AndroidApp) {
    if let Err(e) = invoke_open(app, "bell") {
        meditate_core::log(
            "guided",
            &format!("open_sound_picker FAILED: {e:?}"),
        );
    }
}

/// Bell-import twin of `take_pending_pick` — reads + removes the
/// `sound_pick` drop-file.
pub fn take_pending_sound_pick(
    app: &AndroidApp,
) -> Option<(String, String, u32)> {
    let data_root = app.internal_data_path()?;
    let path = data_root.join("meditate").join(SOUND_PICK_FILENAME);
    let raw = std::fs::read_to_string(&path).ok()?;
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

fn invoke_open(
    app: &AndroidApp,
    target: &str,
) -> Result<(), jni::errors::Error> {
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }?;
    let mut env = vm.attach_current_thread()?;
    let activity =
        unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
    let jtarget = env.new_string(target)?;
    let class = resolve_class(&mut env, &activity, PICKER_CLASS_DOTTED)?;
    env.call_static_method(
        class,
        "openFor",
        "(Landroid/content/Context;Ljava/lang/String;)V",
        &[(&activity).into(), (&jtarget).into()],
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

// ── Guided import transcode (MeditateGuidedImport) ──────────────────

/// Kick off the background transcode (or wav/ogg passthrough copy)
/// of `src` → `dest` (`<data>/meditate/guided/<uuid>.ogg`). Fire-
/// and-forget: the result lands in the `guided_import_result`
/// drop-file; poll `take_import_result`. Mirrors GTK's
/// `spawn_blocking(do_import_io)`.
pub fn start_import(
    app: &AndroidApp,
    src: &str,
    dest: &str,
    duration_secs: u32,
) {
    if let Err(e) = invoke_import(app, src, dest, duration_secs) {
        meditate_core::log(
            "guided",
            &format!("start_import FAILED: {e:?}"),
        );
    }
}

/// Current transcode percent (0–99) the worker is rewriting, or
/// `None` if no progress file exists yet. Not consumed — the file
/// is overwritten in place by the worker and removed at finalize.
pub fn take_import_progress(app: &AndroidApp) -> Option<u8> {
    let data_root = app.internal_data_path()?;
    let path = data_root
        .join("meditate")
        .join(IMPORT_PROGRESS_FILENAME);
    let raw = std::fs::read_to_string(&path).ok()?;
    raw.trim().parse::<u8>().ok().map(|p| p.min(100))
}

/// Remove the progress drop-file once the import has been
/// finalized (success or failure) so a stale value can't bleed
/// into the next import's button fill.
pub fn clear_import_progress(app: &AndroidApp) {
    if let Some(data_root) = app.internal_data_path() {
        let path = data_root
            .join("meditate")
            .join(IMPORT_PROGRESS_FILENAME);
        let _ = std::fs::remove_file(&path);
    }
}

/// Signal the running transcode worker to abort. The Kotlin
/// loop polls this file every iteration and, on seeing it,
/// deletes the partial dest and exits without writing "ok".
/// Mirrors GTK's `cancel.store(true)`.
pub fn request_import_cancel(app: &AndroidApp) {
    if let Some(data_root) = app.internal_data_path() {
        let path = data_root
            .join("meditate")
            .join(IMPORT_CANCEL_FILENAME);
        let _ = std::fs::write(&path, b"1");
    }
}

/// Remove the cancel flag before a fresh import so a prior
/// cancellation can't abort the new run instantly.
pub fn clear_import_cancel(app: &AndroidApp) {
    if let Some(data_root) = app.internal_data_path() {
        let path = data_root
            .join("meditate")
            .join(IMPORT_CANCEL_FILENAME);
        let _ = std::fs::remove_file(&path);
    }
}

/// Discard any pending transcode result without acting on it —
/// used by Cancel so a worker that finished a hair before the
/// abort lands doesn't leave an "ok" that the next poll adopts.
pub fn clear_import_result(app: &AndroidApp) {
    if let Some(data_root) = app.internal_data_path() {
        let path = data_root
            .join("meditate")
            .join(IMPORT_RESULT_FILENAME);
        let _ = std::fs::remove_file(&path);
    }
}

/// Take the transcode outcome — `Ok(())` on success, `Err(msg)` on
/// failure — and delete the drop-file (single consumption). `None`
/// while the worker is still running / nothing pending.
pub fn take_import_result(
    app: &AndroidApp,
) -> Option<Result<(), String>> {
    let data_root = app.internal_data_path()?;
    let path =
        data_root.join("meditate").join(IMPORT_RESULT_FILENAME);
    let raw = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    let trimmed = raw.trim();
    if trimmed == "ok" {
        Some(Ok(()))
    } else {
        Some(Err(trimmed
            .strip_prefix("err:")
            .unwrap_or(trimmed)
            .to_string()))
    }
}

fn invoke_import(
    app: &AndroidApp,
    src: &str,
    dest: &str,
    duration_secs: u32,
) -> Result<(), jni::errors::Error> {
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }?;
    let mut env = vm.attach_current_thread()?;
    let activity =
        unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
    let jsrc = env.new_string(src)?;
    let jdest = env.new_string(dest)?;
    let class = resolve_class(&mut env, &activity, IMPORT_CLASS_DOTTED)?;
    env.call_static_method(
        class,
        "startImport",
        "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;J)V",
        &[
            (&activity).into(),
            (&jsrc).into(),
            (&jdest).into(),
            jni::objects::JValue::Long(i64::from(duration_secs)),
        ],
    )?;
    if env.exception_check()? {
        env.exception_clear()?;
    }
    Ok(())
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
