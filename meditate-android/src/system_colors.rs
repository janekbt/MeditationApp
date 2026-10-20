//! The phone's Material 3 colour scheme (Android 14+), read from the
//! `android` package's `system_*` colour resources by name, so the
//! build needs no API 34 constants.

#![cfg(target_os = "android")]

use android_activity::AndroidApp;
use slint::Color;

use crate::MaterialSchemes;

/// `baseline` with the phone's roles, `None` below Android 14, when a
/// role is missing, or on a JNI failure (logged).
pub fn read(app: &AndroidApp, baseline: &MaterialSchemes) -> Option<MaterialSchemes> {
    if AndroidApp::sdk_version() < 34 {
        return None;
    }
    let result = crate::jni_call::with_env(app, |env, activity| {
        let resources = env
            .call_method(activity, "getResources", "()Landroid/content/res/Resources;", &[])?
            .l()?;
        let kind = env.new_string("color")?;
        let package = env.new_string("android")?;
        crate::theme::system_schemes(baseline, |name| {
            let jname = env.new_string(name)?;
            let id = env
                .call_method(
                    &resources,
                    "getIdentifier",
                    "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)I",
                    &[(&jname).into(), (&kind).into(), (&package).into()],
                )?
                .i()?;
            env.delete_local_ref(jname)?;
            if id == 0 {
                return Ok(None);
            }
            let argb = env.call_method(activity, "getColor", "(I)I", &[id.into()])?.i()?;
            Ok(Some(Color::from_argb_encoded(argb as u32)))
        })
    });
    result.unwrap_or_else(|e| {
        meditate_core::log("theme.system", &format!("reading the system colours FAILED: {e:?}"));
        None
    })
}
