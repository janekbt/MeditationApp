//! Home-screen widget bridge (W-series).
//!
//! The widget is a `RemoteViews` collection — it cannot run Slint,
//! and its `RemoteViewsFactory` lives in a separate process slice
//! that has no access to the rusqlite handle. So the data path is:
//! the app writes a flat JSON projection of the *starred* presets
//! (across every preset-supporting mode — the widget is not
//! mode-scoped the way the in-app chip list is) into the app-
//! private files dir, then pokes `AppWidgetManager` over JNI to
//! re-pull it. Mirrors nothing in the GTK shell (no desktop
//! widget); the *content* still goes through the same
//! `preset_subtitle` formatter the in-app list uses, so a row
//! reads identically on the widget and in Setup.
//!
//! The struct + `build_projection_json` are intentionally NOT
//! `#[cfg(android)]`-gated: the serializer is the one piece with
//! real correctness risk (escaping arbitrary user-entered preset
//! names) so it stays host-`cargo test`-able (strict-TDD). Only
//! the JNI/fs side is android-only.
//!
//! JNI uses the same app-classloader escape hatch as `audio.rs` /
//! `haptics.rs` / `service.rs` (a JNI-attached native thread only
//! sees the system classloader, so `MeditateWidget` is resolved
//! through the activity's classloader by dotted name).

#[cfg(target_os = "android")]
use android_activity::AndroidApp;
#[cfg(target_os = "android")]
use jni::objects::{JClass, JObject, JString};
#[cfg(target_os = "android")]
use jni::JavaVM;

#[cfg(target_os = "android")]
const WIDGET_CLASS_DOTTED: &str = "io.github.janekbt.Meditate.MeditateWidget";

/// File the `RemoteViewsFactory` reads on every
/// `notifyAppWidgetViewDataChanged`. Sits next to `meditate.db`
/// in the app-private files dir, so the widget process slice
/// (same UID) can `File(filesDir, "meditate/widget_presets.json")`
/// it without any cross-process IPC.
#[cfg(target_os = "android")]
const PROJECTION_FILENAME: &str = "widget_presets.json";

/// One starred preset as the widget needs it: the display lines
/// plus the uuid the tap fill-in-intent carries back for the
/// deep-link apply (W-3). `mode` is informational — W-3 re-reads
/// the authoritative row from the DB by uuid, so a stale widget
/// (preset deleted between render and tap) fails closed there.
pub struct WidgetPreset {
    pub uuid: String,
    pub name: String,
    pub subtitle: String,
    pub mode: &'static str,
}

/// Serialize the projection. Pure (no JNI, no fs) so the shape
/// and escaping are unit-testable without a device. Order is
/// preserved exactly as passed — the caller concatenates per
/// mode in a fixed order, so the widget list is stable across
/// refreshes. `serde_json` handles quote/backslash/control-char
/// escaping in arbitrary user-entered preset names.
pub fn build_projection_json(presets: &[WidgetPreset]) -> String {
    let arr: Vec<serde_json::Value> = presets
        .iter()
        .map(|p| {
            serde_json::json!({
                "uuid": p.uuid,
                "name": p.name,
                "subtitle": p.subtitle,
                "mode": p.mode,
            })
        })
        .collect();
    serde_json::json!({ "presets": arr }).to_string()
}

/// Write the projection atomically (tmp + rename in the same dir
/// → the widget never reads a half-written file) then ask
/// `AppWidgetManager` to re-pull it. Best-effort: a missing data
/// path, an fs error, or a JNI hiccup is logged and swallowed —
/// the widget is an accessory, it must never break the app. No-op
/// when there is no installed widget instance (the JNI helper
/// short-circuits on an empty id list).
#[cfg(target_os = "android")]
pub fn publish(app: &AndroidApp, presets: Vec<WidgetPreset>) {
    let Some(data_root) = app.internal_data_path() else {
        meditate_core::log("widget", "publish: no internal_data_path");
        return;
    };
    let dir = data_root.join("meditate");
    let final_path = dir.join(PROJECTION_FILENAME);
    let tmp_path = dir.join(format!("{PROJECTION_FILENAME}.tmp"));
    let json = build_projection_json(&presets);
    if let Err(e) = std::fs::write(&tmp_path, json.as_bytes()) {
        meditate_core::log("widget", &format!("publish write FAILED: {e}"));
        return;
    }
    if let Err(e) = std::fs::rename(&tmp_path, &final_path) {
        meditate_core::log("widget", &format!("publish rename FAILED: {e}"));
        let _ = std::fs::remove_file(&tmp_path);
        return;
    }
    if let Err(e) = invoke_refresh(app) {
        meditate_core::log("widget", &format!("refresh JNI FAILED: {e:?}"));
    }
}

/// Read the `preset_uuid` extra off the activity's launch
/// Intent (the widget tap's fill-in, merged into the launcher
/// template). `None` when launched normally / no extra / blank.
/// Cold-start only: `android_main` calls this once before the
/// Slint loop; a warm process is just foregrounded by the
/// launcher Intent and `onNewIntent` is not plumbed (documented
/// W-3 limitation — a homescreen tap on a dead app is the case
/// that matters). Best-effort: any JNI error → `None`, never a
/// panic on the startup path.
#[cfg(target_os = "android")]
pub fn launch_preset_uuid(app: &AndroidApp) -> Option<String> {
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }.ok()?;
    let mut env = vm.attach_current_thread().ok()?;
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
    let intent = env
        .call_method(&activity, "getIntent", "()Landroid/content/Intent;", &[])
        .ok()?
        .l()
        .ok()?;
    if intent.is_null() {
        return None;
    }
    let key = env.new_string("preset_uuid").ok()?;
    let val = env
        .call_method(
            &intent,
            "getStringExtra",
            "(Ljava/lang/String;)Ljava/lang/String;",
            &[(&key).into()],
        )
        .ok()?
        .l()
        .ok()?;
    if val.is_null() {
        return None;
    }
    let s: String = env.get_string(&JString::from(val)).ok()?.into();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

#[cfg(target_os = "android")]
fn resolve_class<'a>(
    env: &mut jni::JNIEnv<'a>,
    activity: &JObject,
) -> Result<JClass<'a>, jni::errors::Error> {
    let classloader = env
        .call_method(activity, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])?
        .l()?;
    let class_name = env.new_string(WIDGET_CLASS_DOTTED)?;
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

#[cfg(target_os = "android")]
fn invoke_refresh(app: &AndroidApp) -> Result<(), jni::errors::Error> {
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }?;
    let mut env = vm.attach_current_thread()?;
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
    let class = resolve_class(&mut env, &activity)?;
    env.call_static_method(
        class,
        "refresh",
        "(Landroid/content/Context;)V",
        &[(&activity).into()],
    )?;
    if env.exception_check()? {
        env.exception_clear()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(uuid: &str, name: &str, subtitle: &str) -> WidgetPreset {
        WidgetPreset {
            uuid: uuid.into(),
            name: name.into(),
            subtitle: subtitle.into(),
            mode: "timer",
        }
    }

    #[test]
    fn empty_projection_is_empty_array() {
        assert_eq!(build_projection_json(&[]), r#"{"presets":[]}"#);
    }

    #[test]
    fn order_is_preserved() {
        let json = build_projection_json(&[
            p("u1", "Beta", "10 min"),
            p("u2", "Alpha", "Stopwatch"),
        ]);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let arr = v["presets"].as_array().unwrap();
        assert_eq!(arr[0]["uuid"], "u1");
        assert_eq!(arr[1]["uuid"], "u2");
        assert_eq!(arr[0]["name"], "Beta");
        assert_eq!(arr[1]["subtitle"], "Stopwatch");
    }

    #[test]
    fn user_strings_are_escaped() {
        // A preset literally named  «"x"\»  with a newline must
        // round-trip — hand-rolled string concatenation would
        // have produced invalid JSON and an empty widget.
        let json = build_projection_json(&[p("u", "\"x\"\\\n", "a\tb")]);
        let v: serde_json::Value =
            serde_json::from_str(&json).expect("must be valid JSON");
        assert_eq!(v["presets"][0]["name"], "\"x\"\\\n");
        assert_eq!(v["presets"][0]["subtitle"], "a\tb");
    }

    #[test]
    fn mode_tag_is_carried() {
        let json = build_projection_json(&[WidgetPreset {
            uuid: "u".into(),
            name: "n".into(),
            subtitle: "s".into(),
            mode: "box_breath",
        }]);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["presets"][0]["mode"], "box_breath");
    }
}
