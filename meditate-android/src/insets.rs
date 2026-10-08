//! System window-inset bridge (Phase 8) — replaces the hardcoded
//! status/nav-bar guesses with real `WindowInsets`, converted to
//! Slint logical px (dp) on the Kotlin side. Same app-classloader
//! JNI pattern as `screen.rs`.
//!
//! `#[cfg(target_os = "android")]`-gated.

#![cfg(target_os = "android")]

use android_activity::AndroidApp;
use jni::objects::JString;

const INSETS_CLASS_DOTTED: &str =
    "io.github.janekbt.Meditate.MeditateInsets";

/// `(top, bottom)` system insets in logical px, `None` when the
/// window isn't attached yet or on any JNI hiccup — the caller
/// keeps its previous (or built-in fallback) values.
pub fn get_insets(app: &AndroidApp) -> Option<(f32, f32)> {
    match invoke_get_insets(app) {
        Ok(s) if !s.is_empty() => {
            let mut it = s.split(',');
            let top: f32 = it.next()?.trim().parse().ok()?;
            let bottom: f32 = it.next()?.trim().parse().ok()?;
            Some((top, bottom))
        }
        Ok(_) => None,
        Err(e) => {
            meditate_core::log(
                "insets",
                &format!("get_insets FAILED: {e:?}"),
            );
            None
        }
    }
}

fn invoke_get_insets(
    app: &AndroidApp,
) -> Result<String, jni::errors::Error> {
    crate::jni_call::with_env(app, |env, activity| {
        let class = crate::jni_call::load_class(env, activity, INSETS_CLASS_DOTTED)?;
        let result = env
            .call_static_method(
                class,
                "getInsets",
                "(Landroid/app/Activity;)Ljava/lang/String;",
                &[activity.into()],
            )?
            .l()?;
        let jstr = JString::from(result);
        let s: String = env.get_string(&jstr)?.into();
        Ok(s)
    })
}
