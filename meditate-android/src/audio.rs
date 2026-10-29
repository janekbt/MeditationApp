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
use crate::alarm_volume::Levels;
use meditate_core::bell_volume::AlarmLevel;

const AUDIO_CLASS_DOTTED: &str = "io.github.janekbt.Meditate.MeditateAudio";

/// Play the OGG at `path` (an absolute file path from
/// `bell_sounds.file_path`). Supersedes any sound already
/// playing — preview and the session-end cue share one slot, so
/// a new tap stops the previous like the GTK preview slot does.
/// Failures are logged, never propagated (a missing file or a
/// JNI hiccup must not break the session flow). Returns the clip
/// duration in ms (0 if unknown / on failure) so the chooser
/// preview can schedule its pill auto-revert; session-cue
/// callers just ignore the value.
pub fn play(app: &AndroidApp, path: &str, level: &Levels) -> i64 {
    if path.is_empty() {
        return 0;
    }
    match invoke_play(app, path, level) {
        Ok(ms) => ms,
        Err(e) => {
            meditate_core::log(
                "audio.play",
                &format!("play FAILED path={path}: {e:?}"),
            );
            0
        }
    }
}

/// Ring a session bell on its player slot (`app::bell_channel_slot`
/// of core's `FireChannel`): it replaces only the previous bell on
/// the same Starting / End slot, and interval bells stack, so bells
/// no longer cut each other off. Failures are logged, never
/// propagated.
pub fn play_bell(app: &AndroidApp, slot: i32, path: &str, level: &Levels) {
    if path.is_empty() {
        return;
    }
    if let Err(e) = invoke_play_bell(app, slot, path, level) {
        meditate_core::log(
            "audio.play",
            &format!("play_bell FAILED slot={slot} path={path}: {e:?}"),
        );
    }
}

/// Apply a new bell level to the preview ringing right now, if any.
pub fn set_volume(app: &AndroidApp, level: &Levels) {
    if let Err(e) = invoke_set_volume(app, level) {
        meditate_core::log("audio.play", &format!("set_volume FAILED: {e:?}"));
    }
}

/// The alarm stream's steps and their loudness on the speaker:
/// `[min, max, dB of each step...]` (no dB before Android 9); `None`
/// on failure.
pub fn alarm_steps(app: &AndroidApp) -> Option<Vec<f32>> {
    report_steps(app, "alarmSteps")
}

/// The media stream's steps and their loudness on the speaker, as
/// `alarm_steps` does for the alarm.
pub fn media_steps(app: &AndroidApp) -> Option<Vec<f32>> {
    report_steps(app, "mediaSteps")
}

/// The media stream's steps and their loudness in wired headphones,
/// which stand for every kind of headphones.
pub fn headphone_steps(app: &AndroidApp) -> Option<Vec<f32>> {
    report_steps(app, "headphoneSteps")
}

fn report_steps(app: &AndroidApp, method: &str) -> Option<Vec<f32>> {
    match invoke_steps(app, method) {
        Ok(steps) => {
            meditate_core::log("audio.steps", &format!("{method} {steps:?}"));
            Some(steps)
        }
        Err(e) => {
            meditate_core::log("audio.steps", &format!("{method} FAILED: {e:?}"));
            None
        }
    }
}

/// Put the user's alarm and media volume back if the app died while a
/// bell had it raised.
pub fn recover_alarm_volume(app: &AndroidApp) {
    if let Err(e) = invoke_no_arg(app, "recoverAlarmVolume") {
        meditate_core::log("audio.play", &format!("recoverAlarmVolume FAILED: {e:?}"));
    }
}

/// Stop + release everything playing: the preview and every session
/// bell.
pub fn stop(app: &AndroidApp) {
    if let Err(e) = invoke_no_arg(app, "stop") {
        meditate_core::log("audio.play", &format!("stop FAILED: {e:?}"));
    }
}

/// Whether the preview still plays (it ends on its own, or the player
/// stops it when headphones are plugged in or pulled). True when the
/// player can't be asked, so the pill waits out the sound's length.
pub fn preview_playing(app: &AndroidApp) -> bool {
    crate::jni_call::with_env(app, |env, activity| {
        let class = crate::jni_call::load_class(env, activity, AUDIO_CLASS_DOTTED)?;
        env.call_static_method(class, "previewPlaying", "()Z", &[])?.z()
    })
    .unwrap_or(true)
}

fn invoke_play(
    app: &AndroidApp,
    path: &str,
    level: &Levels,
) -> Result<i64, jni::errors::Error> {
    crate::jni_call::with_env(app, |env, activity| {
        let jpath = env.new_string(path)?;
        let gains = gains_array(env, &level.main)?;
        let head_gains = gains_array(env, &level.headphones)?;
        let class = crate::jni_call::load_class(env, activity, AUDIO_CLASS_DOTTED)?;
        let ret = env.call_static_method(
            class,
            "play",
            "(Landroid/content/Context;Ljava/lang/String;ZI[FI[F)J",
            &[
                activity.into(), (&jpath).into(), level.as_media.into(),
                level.main.step.into(), (&gains).into(), level.headphones.step.into(), (&head_gains).into(),
            ],
        )?;

        ret.j()
    })
}

fn invoke_play_bell(
    app: &AndroidApp,
    slot: i32,
    path: &str,
    level: &Levels,
) -> Result<(), jni::errors::Error> {
    crate::jni_call::with_env(app, |env, activity| {
        let jpath = env.new_string(path)?;
        let gains = gains_array(env, &level.main)?;
        let head_gains = gains_array(env, &level.headphones)?;
        let class = crate::jni_call::load_class(env, activity, AUDIO_CLASS_DOTTED)?;
        env.call_static_method(
            class,
            "playBell",
            "(Landroid/content/Context;ILjava/lang/String;ZI[FI[F)V",
            &[
                activity.into(), slot.into(), (&jpath).into(), level.as_media.into(),
                level.main.step.into(), (&gains).into(), level.headphones.step.into(), (&head_gains).into(),
            ],
        )?;
        Ok(())
    })
}

fn invoke_set_volume(app: &AndroidApp, level: &Levels) -> Result<(), jni::errors::Error> {
    crate::jni_call::with_env(app, |env, activity| {
        let gains = gains_array(env, &level.main)?;
        let head_gains = gains_array(env, &level.headphones)?;
        let class = crate::jni_call::load_class(env, activity, AUDIO_CLASS_DOTTED)?;
        env.call_static_method(
            class,
            "setVolume",
            "(Landroid/content/Context;ZI[FI[F)V",
            &[
                activity.into(), level.as_media.into(), level.main.step.into(), (&gains).into(),
                level.headphones.step.into(), (&head_gains).into(),
            ],
        )?;
        Ok(())
    })
}

fn gains_array<'local>(
    env: &mut jni::JNIEnv<'local>,
    level: &AlarmLevel,
) -> Result<jni::objects::JFloatArray<'local>, jni::errors::Error> {
    let len = i32::try_from(level.gains.len()).unwrap_or(0);
    let array = env.new_float_array(len)?;
    env.set_float_array_region(&array, 0, &level.gains)?;
    Ok(array)
}

fn invoke_steps(app: &AndroidApp, method: &str) -> Result<Vec<f32>, jni::errors::Error> {
    crate::jni_call::with_env(app, |env, activity| {
        let class = crate::jni_call::load_class(env, activity, AUDIO_CLASS_DOTTED)?;
        let ret = env
            .call_static_method(
                class,
                method,
                "(Landroid/content/Context;)[F",
                &[activity.into()],
            )?
            .l()?;
        let array = jni::objects::JFloatArray::from(ret);
        let len = usize::try_from(env.get_array_length(&array)?).unwrap_or(0);
        let mut steps = vec![0.0_f32; len];
        env.get_float_array_region(&array, 0, &mut steps)?;
        Ok(steps)
    })
}

fn invoke_no_arg(
    app: &AndroidApp,
    method: &str,
) -> Result<(), jni::errors::Error> {
    crate::jni_call::with_env(app, |env, activity| {
        let class = crate::jni_call::load_class(env, activity, AUDIO_CLASS_DOTTED)?;
        env.call_static_method(
            class,
            method,
            "(Landroid/content/Context;)V",
            &[activity.into()],
        )?;
        Ok(())
    })
}
