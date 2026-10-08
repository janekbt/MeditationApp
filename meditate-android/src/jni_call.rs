//! The one way the bridges call into Java.
//!
//! Every bridge (`audio`, `guided`, `service`, …) runs its JNI work
//! through [`with_env`], which handles the two things a bridge on
//! the long-lived UI thread must never forget:
//!
//! - **Pending exceptions.** jni-rs returns `Err(JavaException)` when
//!   a Java call throws but leaves the exception pending. A bridge
//!   that propagates the error with `?` never reaches its own
//!   cleanup, and the next JNI call on this thread (any bridge, or
//!   Slint's own) then runs with an exception pending: an abort in
//!   a debuggable build, undefined behaviour in release. `with_env`
//!   prints and clears it on every path, success or error.
//! - **Local references.** The UI thread is attached for the life of
//!   the process, so local refs (the class loader, strings, the
//!   class) are never freed by a detach. `with_env` runs the call in
//!   its own local frame, which frees them on return.
//!
//! The closure gets the env and the activity (the `Context` every
//! Kotlin helper takes). It must return plain Rust values: Java
//! objects made inside it die with the frame.

#![cfg(target_os = "android")]

use android_activity::AndroidApp;
use jni::objects::{JClass, JObject};
use jni::{JNIEnv, JavaVM};

/// Room for the handful of refs one bridge call makes (class loader,
/// class name, class, a few argument strings, the result). The JVM
/// grows the frame if a call needs more.
const LOCAL_FRAME_CAPACITY: i32 = 16;

pub fn with_env<T>(
    app: &AndroidApp,
    f: impl FnOnce(&mut JNIEnv, &JObject) -> Result<T, jni::errors::Error>,
) -> Result<T, jni::errors::Error> {
    // SAFETY: `vm_as_ptr` is the JavaVM android-activity received at
    // process start and stays valid for the process lifetime.
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }?;
    let mut env = vm.attach_current_thread()?;
    // SAFETY: android-activity holds a global ref to the activity
    // for as long as the AndroidApp lives.
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
    let result = env.with_local_frame(LOCAL_FRAME_CAPACITY, |env| f(env, &activity));
    if env.exception_check().unwrap_or(false) {
        // Stack trace to logcat, then clear so the next call is clean.
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }
    result
}

/// Load an app class (dotted name) through the activity's class
/// loader. `FindClass` on a native-attached thread only sees system
/// classes, so the app's Kotlin helpers need this route.
pub fn load_class<'a>(
    env: &mut JNIEnv<'a>,
    activity: &JObject,
    dotted: &str,
) -> Result<JClass<'a>, jni::errors::Error> {
    let loader = env
        .call_method(activity, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])?
        .l()?;
    let name = env.new_string(dotted)?;
    let class = env
        .call_method(
            &loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[(&name).into()],
        )?
        .l()?;
    Ok(class.into())
}
