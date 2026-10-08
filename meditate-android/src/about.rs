//! About + diagnostics bridge — the Android analogue of GTK's
//! `AdwAboutDialog` glue (version string, copy/share the diag log,
//! open the project links). Thin JNI wrappers over
//! `MeditateAbout`, same app-classloader pattern as `screen.rs`.
//!
//! `#[cfg(target_os = "android")]`-gated.

#![cfg(target_os = "android")]

use android_activity::AndroidApp;
use jni::objects::JString;

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

/// Full system locale tag ("de-DE", "pt-BR", …) for bundled-
/// translation selection; "en" on any hiccup.
pub fn locale_tag(app: &AndroidApp) -> String {
    invoke_locale_tag(app).unwrap_or_else(|e| {
        meditate_core::log(
            "about",
            &format!("locale_tag FAILED: {e:?}"),
        );
        "en".into()
    })
}

/// System clock convention: "24", or "12|<AM>|<PM>" with the
/// locale's day-period markers; "24" on any hiccup.
pub fn time_format(app: &AndroidApp) -> String {
    invoke_time_format(app).unwrap_or_else(|e| {
        meditate_core::log(
            "about",
            &format!("time_format FAILED: {e:?}"),
        );
        "24".into()
    })
}

/// Locale digit-grouping separator; empty string (= no
/// grouping) on any hiccup.
pub fn grouping_separator(app: &AndroidApp) -> String {
    invoke_no_arg_string(app, "groupingSeparator")
        .unwrap_or_default()
}

/// `date` written the locale's way for an ICU skeleton ("MMMd" →
/// "Oct 9" / "9. Okt."); None on a JNI hiccup.
pub fn format_date(app: &AndroidApp, skeleton: &str, date: chrono::NaiveDate) -> Option<String> {
    use chrono::Datelike;
    let res = crate::jni_call::with_env(app, |env, activity| {
        let js = env.new_string(skeleton)?;
        let class = crate::jni_call::load_class(env, activity, ABOUT_CLASS_DOTTED)?;
        let result = env
            .call_static_method(
                class,
                "formatDate",
                "(Ljava/lang/String;III)Ljava/lang/String;",
                &[(&js).into(), date.year().into(), (date.month() as i32).into(), (date.day() as i32).into()],
            )?
            .l()?;
        let s: String = env.get_string(&JString::from(result))?.into();
        Ok(s)
    });
    match res {
        Ok(s) if !s.is_empty() => Some(s),
        Ok(_) => None,
        Err(e) => {
            meditate_core::log("about", &format!("format_date FAILED: {e:?}"));
            None
        }
    }
}

/// The locale's first weekday as java.util.Calendar numbers it
/// (1 = Sunday); 0 on a JNI hiccup.
pub fn first_day_of_week(app: &AndroidApp) -> i32 {
    crate::jni_call::with_env(app, |env, activity| {
        let class = crate::jni_call::load_class(env, activity, ABOUT_CLASS_DOTTED)?;
        env.call_static_method(class, "firstDayOfWeek", "()I", &[])?.i()
    })
    .unwrap_or_else(|e| {
        meditate_core::log("about", &format!("first_day_of_week FAILED: {e:?}"));
        0
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

fn invoke_locale_tag(
    app: &AndroidApp,
) -> Result<String, jni::errors::Error> {
    crate::jni_call::with_env(app, |env, activity| {
        let class = crate::jni_call::load_class(env, activity, ABOUT_CLASS_DOTTED)?;
        let result = env
            .call_static_method(
                class,
                "localeTag",
                "()Ljava/lang/String;",
                &[],
            )?
            .l()?;
        let jstr = JString::from(result);
        let s: String = env.get_string(&jstr)?.into();
        Ok(s)
    })
}

fn invoke_version_name(
    app: &AndroidApp,
) -> Result<String, jni::errors::Error> {
    crate::jni_call::with_env(app, |env, activity| {
        let class = crate::jni_call::load_class(env, activity, ABOUT_CLASS_DOTTED)?;
        let result = env
            .call_static_method(
                class,
                "versionName",
                "(Landroid/content/Context;)Ljava/lang/String;",
                &[activity.into()],
            )?
            .l()?;
        let jstr = JString::from(result);
        let s: String = env.get_string(&jstr)?.into();
        Ok(s)
    })
}

fn invoke_no_arg_string(
    app: &AndroidApp,
    method: &str,
) -> Result<String, jni::errors::Error> {
    crate::jni_call::with_env(app, |env, activity| {
        let class = crate::jni_call::load_class(env, activity, ABOUT_CLASS_DOTTED)?;
        let result = env
            .call_static_method(class, method, "()Ljava/lang/String;", &[])?
            .l()?;
        let jstr = JString::from(result);
        let s: String = env.get_string(&jstr)?.into();
        Ok(s)
    })
}

fn invoke_time_format(
    app: &AndroidApp,
) -> Result<String, jni::errors::Error> {
    crate::jni_call::with_env(app, |env, activity| {
        let class = crate::jni_call::load_class(env, activity, ABOUT_CLASS_DOTTED)?;
        let result = env
            .call_static_method(
                class,
                "timeFormat",
                "(Landroid/content/Context;)Ljava/lang/String;",
                &[activity.into()],
            )?
            .l()?;
        let jstr = JString::from(result);
        let s: String = env.get_string(&jstr)?.into();
        Ok(s)
    })
}

fn invoke_one_string(
    app: &AndroidApp,
    method: &str,
    a: &str,
) -> Result<(), jni::errors::Error> {
    crate::jni_call::with_env(app, |env, activity| {
        let ja = env.new_string(a)?;
        let class = crate::jni_call::load_class(env, activity, ABOUT_CLASS_DOTTED)?;
        env.call_static_method(
            class,
            method,
            "(Landroid/content/Context;Ljava/lang/String;)V",
            &[activity.into(), (&ja).into()],
        )?;
        Ok(())
    })
}

fn invoke_two_strings(
    app: &AndroidApp,
    method: &str,
    a: &str,
    b: &str,
) -> Result<(), jni::errors::Error> {
    crate::jni_call::with_env(app, |env, activity| {
        let ja = env.new_string(a)?;
        let jb = env.new_string(b)?;
        let class = crate::jni_call::load_class(env, activity, ABOUT_CLASS_DOTTED)?;
        env.call_static_method(
            class,
            method,
            "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;)V",
            &[activity.into(), (&ja).into(), (&jb).into()],
        )?;
        Ok(())
    })
}
