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
use jni::objects::JObject;

const PICKER_CLASS_DOTTED: &str =
    "io.github.janekbt.Meditate.MeditateGuidedPicker";
const PLAYER_CLASS_DOTTED: &str =
    "io.github.janekbt.Meditate.MeditateGuided";
const IMPORT_CLASS_DOTTED: &str =
    "io.github.janekbt.Meditate.MeditateGuidedImport";
const LOCAL_CLASS_DOTTED: &str =
    "io.github.janekbt.Meditate.MeditateLocalFolder";
/// Local Folder drop files (MeditateFilePickerActivity's "folder"
/// route and MeditateLocalFolder): the chosen tree's address, the
/// walk, and the lengths read so far.
const LOCAL_PICK_FILENAME: &str = "local_folder_pick";
const LOCAL_SCAN_FILENAME: &str = "local_folder_scan";
const LOCAL_LENGTHS_FILENAME: &str = "local_folder_lengths";
// The import files carry the import's uuid (`<name>.<uuid>`, the
// dest's file stem), so a cancelled worker that is still copying can
// never read the next import's cancel flag or write its result.
/// Written by `MeditateGuidedImport` when the background transcode
/// finishes: "ok" or "err:<message>". Single-consumption.
const IMPORT_RESULT_FILENAME: &str = "guided_import_result";
/// Rewritten by `MeditateGuidedImport` with the transcode percent
/// (0–99). Polled every tick, not consumed; the worker removes it.
const IMPORT_PROGRESS_FILENAME: &str = "guided_import_progress";
/// Written by Rust when the user taps Cancel mid-transcode; the
/// Kotlin worker polls it each loop and aborts (deleting the
/// partial dest), mirroring GTK's `cancel: &AtomicBool`.
const IMPORT_CANCEL_FILENAME: &str = "guided_import_cancel";
/// Drop-file the picker Activity writes: absolute path / display
/// name / duration in whole seconds / optional no-audio marker —
/// parsed by `meditate_core::sound::parse_pick`.
const PICK_FILENAME: &str = "guided_pick";
/// Same format, written when the picker was opened with
/// target="bell" (BI custom-sound import) — separate file so the
/// two import flows can't consume each other's picks.
const SOUND_PICK_FILENAME: &str = "sound_pick";
/// Written by the export CREATE_DOCUMENT flow: "ok" / "err:<msg>".
const EXPORT_RESULT_FILENAME: &str = "export_result";
/// Written by the CSV-import OPEN_DOCUMENT flow:
/// "<path>\n<kind>" with kind = meditate | insight.
const CSV_PICK_FILENAME: &str = "csv_pick";
/// Touched by `MeditateGuided`'s onCompletion/onError — the
/// audio reached its natural end; the tick loop forces the
/// session into Overtime (robust to a probe-vs-real mismatch).
const EOS_FILENAME: &str = "guided_eos";
const FOCUS_LOSS_FILENAME: &str = "guided_focus_loss";

/// Launch the system audio picker (fire-and-forget). The result
/// arrives asynchronously via the drop-file; poll
/// `take_pending_pick`. Best-effort: a JNI hiccup is logged, the
/// Guided row just stays unset.
pub fn open_picker(app: &AndroidApp) {
    if let Err(e) = invoke_open(
        app,
        "guided",
        meditate_core::sound::ANDROID_PICKER_MIME_TYPES,
    ) {
        meditate_core::log(
            "guided",
            &format!("open_picker FAILED: {e:?}"),
        );
    }
}

/// Same picker, bell-import route: the transient copy lands in
/// `sounds/` and the result in the `sound_pick` drop-file (BI).
pub fn open_sound_picker(app: &AndroidApp) {
    if let Err(e) = invoke_open(
        app,
        "bell",
        meditate_core::sound::ANDROID_PICKER_MIME_TYPES,
    ) {
        meditate_core::log(
            "guided",
            &format!("open_sound_picker FAILED: {e:?}"),
        );
    }
}

/// Bell-import twin of `take_pending_pick` — reads + removes the
/// `sound_pick` drop-file.
pub fn take_pending_sound_pick(app: &AndroidApp) -> Option<PickResult> {
    take_pick_file(app, SOUND_PICK_FILENAME)
}

/// Take the pending pick and delete the drop-file (single
/// consumption, so the tick poll doesn't re-apply it). `None`
/// when nothing is pending / the file is malformed / blank path;
/// `Some(Err(NoAudioTrack))` when the picked file has no audio.
pub fn take_pending_pick(app: &AndroidApp) -> Option<PickResult> {
    take_pick_file(app, PICK_FILENAME)
}

pub type PickResult = Result<
    meditate_core::sound::PickedFile,
    meditate_core::sound::AudioFileError,
>;

fn take_pick_file(app: &AndroidApp, filename: &str) -> Option<PickResult> {
    meditate_core::sound::parse_pick(&crate::drop_file::take(app, filename)?)
}

/// CSV-import picker route (DP): target "import-meditate" or
/// "import-insight"; the landed copy arrives via `take_csv_pick`.
pub fn open_picker_for_csv(app: &AndroidApp, target: &str) {
    if let Err(e) = invoke_open(app, target, &[]) {
        meditate_core::log(
            "data.import",
            &format!("open_picker_for_csv FAILED: {e:?}"),
        );
    }
}

/// Launch the export CREATE_DOCUMENT flow (DP): the CSV is
/// pre-written to `src_path`; the Activity copies it to the pick
/// and reports via `take_export_result`. `false` when the dialog
/// couldn't be launched.
pub fn open_export(app: &AndroidApp, src_path: &str, suggested: &str) -> bool {
    match invoke_open_export(app, src_path, suggested) {
        Ok(()) => true,
        Err(e) => {
            meditate_core::log("data.export", &format!("open_export FAILED: {e:?}"));
            false
        }
    }
}

/// Take the export outcome (single consumption).
pub fn take_export_result(
    app: &AndroidApp,
) -> Option<Result<(), String>> {
    let raw = crate::drop_file::take(app, EXPORT_RESULT_FILENAME)?;
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

/// Take a landed CSV import pick: `Ok((abs path, kind))` with kind
/// "meditate" | "insight", or `Err(message)` when the copy into app
/// storage failed (see `app::parse_csv_pick`). Single consumption.
pub fn take_csv_pick(app: &AndroidApp) -> Option<Result<(String, String), String>> {
    crate::app::parse_csv_pick(&crate::drop_file::take(app, CSV_PICK_FILENAME)?)
}

fn invoke_open_export(
    app: &AndroidApp,
    src_path: &str,
    suggested: &str,
) -> Result<(), jni::errors::Error> {
    crate::jni_call::with_env(app, |env, activity| {
        let jsrc = env.new_string(src_path)?;
        let jname = env.new_string(suggested)?;
        let class = crate::jni_call::load_class(env, activity, PICKER_CLASS_DOTTED)?;
        env.call_static_method(
            class,
            "openExport",
            "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;)V",
            &[activity.into(), (&jsrc).into(), (&jname).into()],
        )?;
        Ok(())
    })
}

fn invoke_open(
    app: &AndroidApp,
    target: &str,
    mime_types: &[&str],
) -> Result<(), jni::errors::Error> {
    crate::jni_call::with_env(app, |env, activity| {
        let jtarget = env.new_string(target)?;
        let jmimes = env.new_object_array(
            i32::try_from(mime_types.len()).unwrap_or(0),
            "java/lang/String",
            JObject::null(),
        )?;
        for (i, m) in mime_types.iter().enumerate() {
            let jm = env.new_string(m)?;
            env.set_object_array_element(
                &jmimes,
                i32::try_from(i).unwrap_or(0),
                jm,
            )?;
        }
        let class = crate::jni_call::load_class(env, activity, PICKER_CLASS_DOTTED)?;
        env.call_static_method(
            class,
            "openFor",
            "(Landroid/content/Context;Ljava/lang/String;[Ljava/lang/String;)V",
            &[activity.into(), (&jtarget).into(), (&jmimes).into()],
        )?;
        Ok(())
    })
}

// ── Guided audio playback (MeditateGuided) ──────────────────────────

/// Start playing the selected file. Supersedes any prior guided
/// playback. Returns whether the track started; the caller starts
/// no session when it didn't (GTK: "Couldn't start playback").
pub fn play(app: &AndroidApp, path: &str) -> bool {
    match invoke_play(app, path) {
        Ok(true) => true,
        Ok(false) => {
            meditate_core::log("guided", &format!("play FAILED: unplayable {path}"));
            false
        }
        Err(e) => {
            meditate_core::log("guided", &format!("play FAILED: {e:?}"));
            false
        }
    }
}

/// Pause / resume / stop the guided track in step with the
/// session's pause/resume/stop. `stop` releases the player.
pub fn pause(app: &AndroidApp) {
    if let Err(e) = invoke_player_noarg(app, "pauseAudio") {
        meditate_core::log("guided", &format!("pauseAudio FAILED: {e:?}"));
    }
}
pub fn resume(app: &AndroidApp) {
    if let Err(e) = invoke_player_noarg(app, "resumeAudio") {
        meditate_core::log("guided", &format!("resumeAudio FAILED: {e:?}"));
    }
}
pub fn stop(app: &AndroidApp) {
    if let Err(e) = invoke_player_noarg(app, "stopAudio") {
        meditate_core::log("guided", &format!("stopAudio FAILED: {e:?}"));
    }
}

/// Take the natural-end flag the player wrote on
/// onCompletion/onError. Single-consumption (drop-file removed),
/// so the tick loop forces Overtime exactly once.
/// Single-shot poll of the audio-focus-loss drop-file
/// (`MeditateGuided.markFocusLoss` writes it when a call or
/// another media app takes focus). The tick loop routes a `true`
/// through the normal pause transition.
pub fn take_focus_loss(app: &AndroidApp) -> bool {
    crate::drop_file::take(app, FOCUS_LOSS_FILENAME).is_some()
}

pub fn take_eos(app: &AndroidApp) -> bool {
    crate::drop_file::take(app, EOS_FILENAME).is_some()
}

// ── Guided import transcode (MeditateGuidedImport) ──────────────────

/// Kick off the background transcode (or wav/ogg passthrough copy)
/// of `src` → `dest` (`<data>/meditate/guided/<uuid>.ogg`). The
/// result lands in the `guided_import_result.<uuid>` drop-file; poll
/// `take_import_result`. `false` when the worker never started.
/// Mirrors GTK's `spawn_blocking(do_import_io)`.
pub fn start_import(
    app: &AndroidApp,
    src: &str,
    dest: &str,
    duration_secs: u32,
) -> bool {
    match invoke_import(app, src, dest, duration_secs) {
        Ok(()) => true,
        Err(e) => {
            meditate_core::log("guided", &format!("start_import FAILED: {e:?}"));
            false
        }
    }
}

/// Current transcode percent (0–99) of import `uuid`, or `None` if
/// no progress file exists yet. Not consumed: the worker rewrites it
/// in place and removes it when done.
pub fn import_progress(app: &AndroidApp, uuid: &str) -> Option<u8> {
    let path = app
        .internal_data_path()?
        .join("meditate")
        .join(format!("{IMPORT_PROGRESS_FILENAME}.{uuid}"));
    let raw = std::fs::read_to_string(&path).ok()?;
    raw.trim().parse::<u8>().ok().map(|p| p.min(100))
}

/// Signal the worker of import `uuid` to abort. The Kotlin loop
/// polls this file every iteration and, on seeing it, deletes the
/// partial dest and exits without writing "ok". Mirrors GTK's
/// `cancel.store(true)`.
pub fn request_import_cancel(app: &AndroidApp, uuid: &str) {
    if let Some(data_root) = app.internal_data_path() {
        let path = data_root
            .join("meditate")
            .join(format!("{IMPORT_CANCEL_FILENAME}.{uuid}"));
        let _ = std::fs::write(&path, b"1");
    }
}

/// Take the outcome of import `uuid` (single consumption). `None`
/// while the worker is still running.
pub fn take_import_result(
    app: &AndroidApp,
    uuid: &str,
) -> Option<Result<(), meditate_core::sound::AudioFileError>> {
    let raw = crate::drop_file::take(app, &format!("{IMPORT_RESULT_FILENAME}.{uuid}"))?;
    Some(meditate_core::sound::parse_import_result(&raw))
}

fn invoke_import(
    app: &AndroidApp,
    src: &str,
    dest: &str,
    duration_secs: u32,
) -> Result<(), jni::errors::Error> {
    crate::jni_call::with_env(app, |env, activity| {
        let jsrc = env.new_string(src)?;
        let jdest = env.new_string(dest)?;
        let class = crate::jni_call::load_class(env, activity, IMPORT_CLASS_DOTTED)?;
        env.call_static_method(
            class,
            "startImport",
            "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;J)V",
            &[
                activity.into(),
                (&jsrc).into(),
                (&jdest).into(),
                jni::objects::JValue::Long(i64::from(duration_secs)),
            ],
        )?;
        Ok(())
    })
}

fn invoke_play(
    app: &AndroidApp,
    path: &str,
) -> Result<bool, jni::errors::Error> {
    crate::jni_call::with_env(app, |env, activity| {
        let jpath = env.new_string(path)?;
        let class = crate::jni_call::load_class(env, activity, PLAYER_CLASS_DOTTED)?;
        let started = env.call_static_method(
            class,
            "startAudio",
            "(Landroid/content/Context;Ljava/lang/String;)Z",
            &[activity.into(), (&jpath).into()],
        )?;
        started.z()
    })
}

fn invoke_player_noarg(
    app: &AndroidApp,
    method: &str,
) -> Result<(), jni::errors::Error> {
    crate::jni_call::with_env(app, |env, activity| {
        let class = crate::jni_call::load_class(env, activity, PLAYER_CLASS_DOTTED)?;
        env.call_static_method(
            class,
            method,
            "(Landroid/content/Context;)V",
            &[activity.into()],
        )?;
        Ok(())
    })
}

// ── Guided Local Folder (MeditateLocalFolder) ───────────────────────

/// Open the system folder picker; the chosen folder's address
/// arrives through `take_folder_pick`. `false` when it couldn't open.
pub fn open_folder_picker(app: &AndroidApp) -> bool {
    let opened = invoke_open(app, "folder", &[]);
    if let Err(e) = &opened {
        meditate_core::log("guided", &format!("open_folder_picker FAILED: {e:?}"));
    }
    opened.is_ok()
}

pub fn take_folder_pick(app: &AndroidApp) -> Option<String> {
    let raw = crate::drop_file::take(app, LOCAL_PICK_FILENAME)?;
    let tree = raw.trim();
    (!tree.is_empty()).then(|| tree.to_string())
}

/// Walk `tree` on a worker thread; the result arrives through
/// `take_local_scan`. `false` when the walk never started.
pub fn start_local_scan(app: &AndroidApp, tree: &str) -> bool {
    let started = crate::jni_call::with_env(app, |env, activity| {
        let jtree = env.new_string(tree)?;
        let class = crate::jni_call::load_class(env, activity, LOCAL_CLASS_DOTTED)?;
        env.call_static_method(
            class,
            "scan",
            "(Landroid/content/Context;Ljava/lang/String;)V",
            &[activity.into(), (&jtree).into()],
        )?;
        Ok(())
    });
    if let Err(e) = &started {
        meditate_core::log("guided", &format!("start_local_scan FAILED: {e:?}"));
    }
    started.is_ok()
}

pub fn take_local_scan(app: &AndroidApp) -> Option<meditate_core::db::FolderScan> {
    Some(crate::app::parse_local_scan(&crate::drop_file::take(app, LOCAL_SCAN_FILENAME)?))
}

/// Read the lengths of `files` (id, path under `tree`) on a worker
/// thread; results arrive through `take_local_lengths`. `false` when
/// the worker never started.
pub fn read_local_lengths(app: &AndroidApp, tree: &str, files: &[(i64, String)]) -> bool {
    let started = crate::jni_call::with_env(app, |env, activity| {
        let jtree = env.new_string(tree)?;
        let len = i32::try_from(files.len()).unwrap_or(0);
        let jids = env.new_long_array(len)?;
        let ids: Vec<i64> = files.iter().map(|(id, _)| *id).collect();
        env.set_long_array_region(&jids, 0, &ids)?;
        let jrels = env.new_object_array(len, "java/lang/String", JObject::null())?;
        for (i, (_, rel)) in files.iter().enumerate() {
            let jrel = env.new_string(rel)?;
            env.set_object_array_element(&jrels, i32::try_from(i).unwrap_or(0), jrel)?;
        }
        let class = crate::jni_call::load_class(env, activity, LOCAL_CLASS_DOTTED)?;
        env.call_static_method(
            class,
            "readLengths",
            "(Landroid/content/Context;Ljava/lang/String;[J[Ljava/lang/String;)V",
            &[activity.into(), (&jtree).into(), (&jids).into(), (&jrels).into()],
        )?;
        Ok(())
    });
    if let Err(e) = &started {
        meditate_core::log("guided", &format!("read_local_lengths FAILED: {e:?}"));
    }
    started.is_ok()
}

pub fn take_local_lengths(app: &AndroidApp) -> Option<Vec<(i64, Option<u32>)>> {
    Some(crate::app::parse_local_lengths(&crate::drop_file::take(app, LOCAL_LENGTHS_FILENAME)?))
}

/// The content address of `rel` under `tree`, for `play`; `None`
/// when the file is gone or the folder can't be read.
pub fn local_document_address(app: &AndroidApp, tree: &str, rel: &str) -> Option<String> {
    let found = crate::jni_call::with_env(app, |env, activity| {
        let jtree = env.new_string(tree)?;
        let jrel = env.new_string(rel)?;
        let class = crate::jni_call::load_class(env, activity, LOCAL_CLASS_DOTTED)?;
        let address = env
            .call_static_method(
                class,
                "documentAddress",
                "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
                &[activity.into(), (&jtree).into(), (&jrel).into()],
            )?
            .l()?;
        if address.is_null() {
            return Ok(None);
        }
        let address = jni::objects::JString::from(address);
        let address: String = env.get_string(&address)?.into();
        Ok(Some(address))
    });
    match found {
        Ok(address) => address,
        Err(e) => {
            meditate_core::log("guided", &format!("local_document_address FAILED: {e:?}"));
            None
        }
    }
}
