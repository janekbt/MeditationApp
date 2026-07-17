//! About + diagnostics bridge — the Android analogue of GTK's
//! `AdwAboutDialog` glue (version string, copy/share the diag log,
//! open the project links). Thin JNI wrappers over
//! `MeditateAbout`, same app-classloader pattern as `screen.rs`.
//!
//! `#[cfg(target_os = "android")]`-gated.

#![cfg(target_os = "android")]

use android_activity::AndroidApp;
use jni::objects::{JClass, JObject, JString};
use jni::JavaVM;

const ABOUT_CLASS_DOTTED: &str =
    "io.github.janekbt.Meditate.MeditateAbout";

/// The installed APK's versionName (source of truth:
/// build.gradle), "?" on any JNI hiccup.
pub fn version_name(app: &AndroidApp) -> String {
    invoke_version_name(app).unwrap_or_else(|e| {
        meditate_core::log(
            "about",
            &format!("version_name FAILED: {e:?}"),
        );
        "?".into()
    })
}

/// Copy `text` to the system clipboard under `label`.
pub fn copy_text(app: &AndroidApp, label: &str, text: &str) {
    if let Err(e) = invoke_two_strings(app, "copyText", label, text) {
        meditate_core::log("about", &format!("copy_text FAILED: {e:?}"));
    }
}

/// Open the system share sheet with `text` (subject `subject`).
pub fn share_text(app: &AndroidApp, subject: &str, text: &str) {
    if let Err(e) = invoke_two_strings(app, "shareText", subject, text)
    {
        meditate_core::log(
            "about",
            &format!("share_text FAILED: {e:?}"),
        );
    }
}

/// Open `url` in the default browser.
pub fn open_url(app: &AndroidApp, url: &str) {
    if let Err(e) = invoke_one_string(app, "openUrl", url) {
        meditate_core::log("about", &format!("open_url FAILED: {e:?}"));
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

fn invoke_version_name(
    app: &AndroidApp,
) -> Result<String, jni::errors::Error> {
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }?;
    let mut env = vm.attach_current_thread()?;
    let activity =
        unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
    let class = resolve_class(&mut env, &activity, ABOUT_CLASS_DOTTED)?;
    let result = env
        .call_static_method(
            class,
            "versionName",
            "(Landroid/content/Context;)Ljava/lang/String;",
            &[(&activity).into()],
        )?
        .l()?;
    if env.exception_check()? {
        env.exception_clear()?;
        return Ok("?".into());
    }
    let jstr = JString::from(result);
    let s: String = env.get_string(&jstr)?.into();
    Ok(s)
}

fn invoke_one_string(
    app: &AndroidApp,
    method: &str,
    a: &str,
) -> Result<(), jni::errors::Error> {
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }?;
    let mut env = vm.attach_current_thread()?;
    let activity =
        unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
    let ja = env.new_string(a)?;
    let class = resolve_class(&mut env, &activity, ABOUT_CLASS_DOTTED)?;
    env.call_static_method(
        class,
        method,
        "(Landroid/content/Context;Ljava/lang/String;)V",
        &[(&activity).into(), (&ja).into()],
    )?;
    if env.exception_check()? {
        env.exception_clear()?;
    }
    Ok(())
}

fn invoke_two_strings(
    app: &AndroidApp,
    method: &str,
    a: &str,
    b: &str,
) -> Result<(), jni::errors::Error> {
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }?;
    let mut env = vm.attach_current_thread()?;
    let activity =
        unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
    let ja = env.new_string(a)?;
    let jb = env.new_string(b)?;
    let class = resolve_class(&mut env, &activity, ABOUT_CLASS_DOTTED)?;
    env.call_static_method(
        class,
        method,
        "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;)V",
        &[(&activity).into(), (&ja).into(), (&jb).into()],
    )?;
    if env.exception_check()? {
        env.exception_clear()?;
    }
    Ok(())
}
