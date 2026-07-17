//! Nextcloud app-password bridge (Phase 7 SY-1) — the Android
//! analogue of the GTK shell's `keychain.rs` over oo7/libsecret.
//! The Kotlin side (`MeditateKeychain`) holds the Android-Keystore
//! AES key and the encrypted secret file; this bridge is the same
//! app-classloader JNI escape hatch as `screen.rs` / `guided.rs`.
//!
//! Empty-string from Kotlin = "no password stored" → `None` here
//! (a real Nextcloud app-password is never empty; core's
//! `prepare_save` rejects empty input before storage is reached).
//!
//! `#[cfg(target_os = "android")]`-gated.

#![cfg(target_os = "android")]

use android_activity::AndroidApp;
use jni::objects::{JClass, JObject, JString};
use jni::JavaVM;

const KEYCHAIN_CLASS_DOTTED: &str =
    "io.github.janekbt.Meditate.MeditateKeychain";

/// Store the app password for `url`+`username`. Returns false on
/// any Keystore/IO failure (caller surfaces GTK's KeyringFailed
/// copy).
pub fn store_password(
    app: &AndroidApp,
    url: &str,
    username: &str,
    password: &str,
) -> bool {
    match invoke_store(app, url, username, password) {
        Ok(ok) => ok,
        Err(e) => {
            meditate_core::log(
                "keychain",
                &format!("store_password FAILED: {e:?}"),
            );
            false
        }
    }
}

/// Read the stored password, `None` if absent, mismatched
/// account, or undecryptable (Keystore key rotated) — the caller
/// treats all three as "re-enter your password".
pub fn read_password(
    app: &AndroidApp,
    url: &str,
    username: &str,
) -> Option<String> {
    match invoke_read(app, url, username) {
        Ok(s) if s.is_empty() => None,
        Ok(s) => Some(s),
        Err(e) => {
            meditate_core::log(
                "keychain",
                &format!("read_password FAILED: {e:?}"),
            );
            None
        }
    }
}

// NOTE: no `clear_password` — neither shell has an account-remove
// flow (GTK never calls core's `clear_nextcloud_account` either).
// The Kotlin side keeps `clearPassword` as API surface for when
// one lands; the Rust bridge stays consumer-driven (dead-code-
// warning-free) per the no-suppression rule.

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

fn invoke_store(
    app: &AndroidApp,
    url: &str,
    username: &str,
    password: &str,
) -> Result<bool, jni::errors::Error> {
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }?;
    let mut env = vm.attach_current_thread()?;
    let activity =
        unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
    let jurl = env.new_string(url)?;
    let juser = env.new_string(username)?;
    let jpw = env.new_string(password)?;
    let class = resolve_class(&mut env, &activity, KEYCHAIN_CLASS_DOTTED)?;
    let result = env
        .call_static_method(
            class,
            "storePassword",
            "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Z",
            &[
                (&activity).into(),
                (&jurl).into(),
                (&juser).into(),
                (&jpw).into(),
            ],
        )?
        .z()?;
    if env.exception_check()? {
        env.exception_clear()?;
        return Ok(false);
    }
    Ok(result)
}

fn invoke_read(
    app: &AndroidApp,
    url: &str,
    username: &str,
) -> Result<String, jni::errors::Error> {
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }?;
    let mut env = vm.attach_current_thread()?;
    let activity =
        unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
    let jurl = env.new_string(url)?;
    let juser = env.new_string(username)?;
    let class = resolve_class(&mut env, &activity, KEYCHAIN_CLASS_DOTTED)?;
    let result = env
        .call_static_method(
            class,
            "readPassword",
            "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
            &[(&activity).into(), (&jurl).into(), (&juser).into()],
        )?
        .l()?;
    if env.exception_check()? {
        env.exception_clear()?;
        return Ok(String::new());
    }
    let jstr = JString::from(result);
    let s: String = env.get_string(&jstr)?.into();
    Ok(s)
}

