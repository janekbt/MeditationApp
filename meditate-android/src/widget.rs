//! Home-screen widget bridge (W-series).
//!
//! The widget is a `RemoteViews` collection — it cannot run Slint,
//! and its `RemoteViewsFactory` lives in a separate process slice
//! that has no access to the rusqlite handle. So the data path is:
//! the app writes a flat JSON projection of the *starred* presets
//! (across every preset-supporting mode — the widget is not
//! mode-scoped the way the in-app preset list is) into the app-
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
const WIDGET_CLASS_DOTTED: &str = "io.github.janekbt.Meditate.MeditateWidget";

/// File the `RemoteViewsFactory` reads on every
/// `notifyAppWidgetViewDataChanged`. Sits next to `meditate.db`
/// in the app-private files dir, so the widget process slice
/// (same UID) can `File(filesDir, "meditate/widget_presets.json")`
/// it without any cross-process IPC.
#[cfg(target_os = "android")]
const PROJECTION_FILENAME: &str = "widget_presets.json";

/// One widget row: a starred preset or a starred guided file, its
/// display lines and the value its tap hands back (`launch_value`).
/// The JSON key stays `uuid`: the widget's Kotlin side reads that key
/// and puts it in the tap's `preset_uuid` extra unchanged, so rows the
/// previous version drew still start their preset.
pub struct WidgetRow {
    pub launch: String,
    pub name: String,
    pub subtitle: String,
}

/// What a widget tap asks for.
#[derive(Debug, PartialEq, Eq)]
pub enum Launch {
    Preset(String),
    Guided(meditate_core::local_folder::StarredGuidedId),
}

const GUIDED_PREFIX: &str = "guided:";

/// The tap value of a preset row (its uuid) or a file row.
pub fn launch_value(launch: &Launch) -> String {
    match launch {
        Launch::Preset(uuid) => uuid.clone(),
        Launch::Guided(id) => format!("{GUIDED_PREFIX}{}", id.key()),
    }
}

/// Read a tap value back; a bare value is a preset uuid.
pub fn parse_launch(value: &str) -> Launch {
    match value.strip_prefix(GUIDED_PREFIX) {
        Some(key) => Launch::Guided(meditate_core::local_folder::StarredGuidedId::from_key(key)),
        None => Launch::Preset(value.to_string()),
    }
}

/// The widget's rows: each mode's own rows, Timer, Guided, Box Breath.
pub fn order_rows(rows: impl FnMut(&meditate_core::SessionMode) -> Vec<WidgetRow>) -> Vec<WidgetRow> {
    use meditate_core::SessionMode;
    [SessionMode::Timer, SessionMode::Guided, SessionMode::BoxBreath].iter().flat_map(rows).collect()
}

/// Serialize the projection. Pure (no JNI, no fs) so the shape
/// and escaping are unit-testable without a device. Order is
/// preserved exactly as passed. `serde_json` handles quote/backslash/
/// control-char escaping in arbitrary user-entered names.
pub fn build_projection_json(rows: &[WidgetRow]) -> String {
    let arr: Vec<serde_json::Value> = rows
        .iter()
        .map(|p| {
            serde_json::json!({
                "uuid": p.launch,
                "name": p.name,
                "subtitle": p.subtitle,
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
pub fn publish(app: &AndroidApp, rows: Vec<WidgetRow>) {
    let Some(data_root) = app.internal_data_path() else {
        meditate_core::log("widget", "publish: no internal_data_path");
        return;
    };
    let dir = data_root.join("meditate");
    let final_path = dir.join(PROJECTION_FILENAME);
    let tmp_path = dir.join(format!("{PROJECTION_FILENAME}.tmp"));
    let json = build_projection_json(&rows);
    // Unchanged rows: no write, no widget redraw (this runs on every
    // switch into Guided and after every sync).
    if std::fs::read_to_string(&final_path).is_ok_and(|old| old == json) {
        return;
    }
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

/// Drop file the widget tap leaves behind. `MeditateWidgetProvider.
/// onReceive` (fired by the tap's broadcast PendingIntent) writes
/// the tapped preset's uuid here, then foregrounds the app. This
/// is the cross-context channel: NativeActivity does not forward
/// `onNewIntent` to native code, so a warm process can't read the
/// tap off an Intent — but it can poll this file. Cold start reads
/// it the same way, so there is one path for both.
#[cfg(target_os = "android")]
const LAUNCH_FILENAME: &str = "widget_launch";

/// Take the pending widget-launch uuid, if any: read the drop
/// file and delete it (single-consumption — the next poll/start
/// won't re-fire the same tap). `None` when no tap is pending /
/// blank / unreadable. Called from `android_main` (cold start)
/// and the tick loop (warm process), so it must be cheap when
/// the common case is "no file": a failed open is one syscall.
#[cfg(target_os = "android")]
pub fn take_pending_launch(app: &AndroidApp) -> Option<String> {
    let uuid = crate::drop_file::take(app, LAUNCH_FILENAME)?;
    let trimmed = uuid.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(target_os = "android")]
fn invoke_refresh(app: &AndroidApp) -> Result<(), jni::errors::Error> {
    crate::jni_call::with_env(app, |env, activity| {
        let class = crate::jni_call::load_class(env, activity, WIDGET_CLASS_DOTTED)?;
        env.call_static_method(
            class,
            "refresh",
            "(Landroid/content/Context;)V",
            &[activity.into()],
        )?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use meditate_core::local_folder::StarredGuidedId;
    use meditate_core::SessionMode;

    fn p(uuid: &str, name: &str, subtitle: &str) -> WidgetRow {
        WidgetRow {
            launch: uuid.into(),
            name: name.into(),
            subtitle: subtitle.into(),
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
    fn the_widget_lists_files_between_timer_and_box_breath() {
        let rows = order_rows(|mode| match mode {
            SessionMode::Timer => vec![p("t1", "T1", ""), p("t2", "T2", "")],
            SessionMode::Guided => vec![p("g1", "G1", "")],
            SessionMode::BoxBreath => vec![p("b1", "B1", ""), p("b2", "B2", "")],
        });
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["T1", "T2", "G1", "B1", "B2"]);
    }

    #[test]
    fn a_widget_tap_tells_a_file_from_a_preset() {
        // A row drawn by the previous version carries the bare preset uuid.
        assert_eq!(parse_launch("6f1c2d3e-aaaa"), Launch::Preset("6f1c2d3e-aaaa".into()));
        let library = Launch::Guided(StarredGuidedId::Library("gf-1".into()));
        let local = Launch::Guided(StarredGuidedId::Local(7));
        assert_eq!(launch_value(&library), "guided:gf-1");
        assert_eq!(launch_value(&local), "guided:local:7");
        assert_eq!(parse_launch("guided:gf-1"), library);
        assert_eq!(parse_launch("guided:local:7"), local);
        assert_eq!(parse_launch(&launch_value(&Launch::Preset("u".into()))), Launch::Preset("u".into()));
        // Garbage after the prefix asks for a file no library has: "File not found".
        assert_eq!(parse_launch("guided:"), Launch::Guided(StarredGuidedId::Library(String::new())));
    }
}
