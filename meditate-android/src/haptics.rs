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
//! `vibrate_waveform` plays a `build_master_envelope`
//! `(amplitude, duration_ms)` sequence; `cancel` stops an
//! in-flight vibration (preview Stop / supersede).
//!
//! Module is `#[cfg(target_os = "android")]`-gated everywhere; the
//! host `cargo run` path never compiles this file or its deps.

#![cfg(target_os = "android")]

use android_activity::AndroidApp;
use jni::objects::{JClass, JObject};
use jni::JavaVM;

const HAPTICS_CLASS_DOTTED: &str = "io.github.janekbt.Meditate.MeditateHaptics";

/// Play a vibration waveform (B-2a). `segments` is the
/// `(amplitude 0.0..=1.0, duration_ms)` envelope
/// `meditate_core::vibration::build_master_envelope` produces.
/// Mapped to Android's `createWaveform(long[] timings, int[]
/// amplitudes, -1)`: durations clamped ≥1 ms (0-length
/// segments are rejected by the platform), amplitude →
/// 0 (off) or 1..=255. Failures are logged, never propagated.
pub fn vibrate_waveform(app: &AndroidApp, segments: &[(f64, u32)]) {
    if segments.is_empty() {
        return;
    }
    if let Err(e) = invoke_waveform(app, segments) {
        meditate_core::log(
            "haptics.vibrate",
            &format!("vibrateWaveform FAILED ({} seg): {e:?}", segments.len()),
        );
    }
}

/// Stop any in-flight vibration (preview Stop / supersede).
pub fn cancel(app: &AndroidApp) {
    if let Err(e) = invoke_no_arg(app, "cancel") {
        meditate_core::log(
            "haptics.vibrate",
            &format!("cancel FAILED: {e:?}"),
        );
    }
}

fn resolve_class<'a>(
    env: &mut jni::JNIEnv<'a>,
    activity: &JObject,
) -> Result<JClass<'a>, jni::errors::Error> {
    let classloader = env
        .call_method(activity, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])?
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
    Ok(class_obj.into())
}

fn invoke_waveform(
    app: &AndroidApp,
    segments: &[(f64, u32)],
) -> Result<(), jni::errors::Error> {
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }?;
    let mut env = vm.attach_current_thread()?;
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };

    let timings: Vec<i64> = segments
        .iter()
        .map(|(_, ms)| i64::from((*ms).max(1)))
        .collect();
    let amplitudes: Vec<i32> = segments
        .iter()
        .map(|(a, _)| {
            if *a <= 0.0 {
                0
            } else {
                ((a * 255.0).round() as i32).clamp(1, 255)
            }
        })
        .collect();

    let t_arr = env.new_long_array(timings.len() as i32)?;
    env.set_long_array_region(&t_arr, 0, &timings)?;
    let a_arr = env.new_int_array(amplitudes.len() as i32)?;
    env.set_int_array_region(&a_arr, 0, &amplitudes)?;

    let class = resolve_class(&mut env, &activity)?;
    env.call_static_method(
        class,
        "vibrateWaveform",
        "(Landroid/content/Context;[J[I)V",
        &[
            (&activity).into(),
            jni::objects::JValue::Object(&t_arr),
            jni::objects::JValue::Object(&a_arr),
        ],
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
