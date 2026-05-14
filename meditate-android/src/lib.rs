pub mod app;
#[cfg(target_os = "android")]
mod service;

slint::include_modules!();

use app::{AppState, TimerMode};
#[cfg(target_os = "android")]
use app::{signal_mode_from_chip_index, signal_mode_to_chip_index};
use std::cell::Cell;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

// Process-wide handle to the AndroidApp. Stored at android_main
// entry so the AppState transition callbacks (which live inside
// closures and don't own the AndroidApp) can reach the JNI bridge.
// android-activity's AndroidApp is `Clone + Send + Sync` per its
// docs, so OnceLock storage is sound.
#[cfg(target_os = "android")]
static ANDROID_APP: OnceLock<slint::android::AndroidApp> = OnceLock::new();

/// React to an AppState transition. If the session just started
/// (Idle/Finished → Active), kick the foreground service. If it
/// just ended (Active → Idle/Finished), tear it down. No-op on
/// host builds — the foreground service only exists on Android.
fn on_state_changed(was_active: bool, is_active: bool) {
    #[cfg(target_os = "android")]
    {
        if !was_active && is_active {
            if let Some(app) = ANDROID_APP.get() {
                service::start(app);
            }
        } else if was_active && !is_active {
            if let Some(app) = ANDROID_APP.get() {
                service::stop(app);
            }
        }
    }
    // Touched the args so the host build doesn't complain about
    // unused parameters under cfg-disabled code.
    let _ = (was_active, is_active);
}

// Default duration the steppers seed with on first open. 10 minutes
// mirrors the GTK shell's default starting position for a freshly
// opened Timer mode. After phase 3's DB persistence lands, this
// becomes the last-used duration loaded from settings.
const DEFAULT_HOURS: i32 = 0;
const DEFAULT_MINUTES: i32 = 10;

// Tick interval driving the mm:ss redraw + Running→Finished detection.
// 200ms keeps the seconds digit visually responsive without burning
// CPU on a phone display.
const TICK: Duration = Duration::from_millis(200);

/// Resolve the configured target duration from the Slint steppers.
/// The setup hours / minutes properties drive both the Setup hero
/// readout and `AppState::toggle`'s start path.
fn configured_duration(ui: &MainWindow) -> Duration {
    let hours = ui.get_setup_hours().max(0) as u64;
    let minutes = ui.get_setup_minutes().max(0) as u64;
    Duration::from_secs(hours * 3600 + minutes * 60)
}

/// Monotonic seconds since process start. CLOCK_BOOTTIME-backed on
/// Android (Rust 1.79+) and CLOCK_MONOTONIC on desktop Linux — the
/// latter is fine for dev-iteration runs that never see a real
/// suspend.
fn now_since_epoch() -> Duration {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    Instant::now().duration_since(*EPOCH.get_or_init(Instant::now))
}

fn refresh(ui: &MainWindow, state: &AppState, now: Duration) {
    let target = configured_duration(ui);
    let stopwatch_on = ui.get_stopwatch_on();
    ui.set_remaining_text(state.hero_label(target, now, stopwatch_on).into());
    // Duration-row value suffix: always shows the configured target
    // in HH:MM (mirrors the GTK Adw.ActionRow value label). On Setup
    // it matches the hero; while Running the row is hidden, so it's
    // safe for the formats to diverge.
    ui.set_duration_text(
        meditate_core::format::format_hhmm(target.as_secs() as u32).into(),
    );
    ui.set_action_label(state.primary_label().into());
    ui.set_stop_visible(state.can_stop());
    ui.set_running_page(state.is_running_page());
    ui.set_done_page(state.is_done_page());
}

/// Snapshot of an in-flight session that the persistence layer needs
/// at end time. `unix_start` is captured at the Idle/Finished → Active
/// transition (mirrors the GTK shell's `session_start_time` cell);
/// elapsed comes from the live core::Session and is captured BEFORE
/// the AppState mutation drops the session.
// Per-mode setting helpers — wrap the DATABASE lock + the
// `settings_keys` dispatchers in one place. `read_*` returns the
// stored value or the meditate-core default; `write_*` is fire-
// and-forget (errors land in the diag log). Mirrors the GTK
// shell's pattern of "thin wrapper around `db.set_setting` /
// `read_*_from_db` with a fallback for missing rows".
#[cfg(target_os = "android")]
fn read_keep_awake_for_mode(mode: meditate_core::SessionMode) -> bool {
    let Some(db_arc) = DATABASE.get() else { return false; };
    let Ok(guard) = db_arc.lock() else { return false; };
    let Some(db) = guard.as_ref() else { return false; };
    meditate_core::settings_keys::keep_screen_awake_from_db(db, mode)
}

#[cfg(target_os = "android")]
fn write_keep_awake_for_mode(mode: meditate_core::SessionMode, value: bool) {
    let Some(db_arc) = DATABASE.get() else { return; };
    let Ok(guard) = db_arc.lock() else { return; };
    let Some(db) = guard.as_ref() else { return; };
    let key = meditate_core::settings_keys::keep_screen_awake_key_for_mode(mode);
    if let Err(e) = db.set_setting(key, meditate_core::format_bool(value)) {
        meditate_core::log(
            "settings.keep_awake",
            &format!("write FAILED mode={mode:?} value={value} err={e:?}"),
        );
    }
}

#[cfg(target_os = "android")]
fn read_signal_mode_for_mode(mode: meditate_core::SessionMode) -> meditate_core::SignalMode {
    let Some(db_arc) = DATABASE.get() else { return meditate_core::SignalMode::Both; };
    let Ok(guard) = db_arc.lock() else { return meditate_core::SignalMode::Both; };
    let Some(db) = guard.as_ref() else { return meditate_core::SignalMode::Both; };
    let key = meditate_core::settings_keys::signal_mode_key_for_mode(mode);
    meditate_core::settings_keys::read_signal_mode(
        db,
        key,
        meditate_core::SignalMode::Both,
    )
}

#[cfg(target_os = "android")]
fn read_label_active_for_mode(mode: meditate_core::SessionMode) -> bool {
    let Some(db_arc) = DATABASE.get() else { return false; };
    let Ok(guard) = db_arc.lock() else { return false; };
    let Some(db) = guard.as_ref() else { return false; };
    meditate_core::labels::label_active_from_db(db, mode)
}

#[cfg(target_os = "android")]
fn write_label_active_for_mode(mode: meditate_core::SessionMode, value: bool) {
    let Some(db_arc) = DATABASE.get() else { return; };
    let Ok(guard) = db_arc.lock() else { return; };
    let Some(db) = guard.as_ref() else { return; };
    if let Err(e) = meditate_core::labels::persist_active_for_mode(db, mode, value) {
        meditate_core::log(
            "settings.label_active",
            &format!("write FAILED mode={mode:?} value={value} err={e:?}"),
        );
    }
}

/// Resolves the label name + id for `mode` via core's
/// `resolve_label_for_mode`. Returns `(name, id)` so the Slint
/// inner row can display the name and `finalize_session` can pass
/// the id. None when the row was deleted and no default exists.
#[cfg(target_os = "android")]
fn resolved_label_for_mode(
    mode: meditate_core::SessionMode,
) -> Option<(String, i64)> {
    let db_arc = DATABASE.get()?;
    let guard = db_arc.lock().ok()?;
    let db = guard.as_ref()?;
    let label = meditate_core::labels::resolve_label_for_mode(db, mode)?;
    Some((label.name, label.id))
}

/// Push the chooser's `label-items` array (with `selected`
/// flagged on `current_id`'s row) into the Slint window. Used
/// on every chooser open + after a CRUD action so the user
/// sees the post-change state.
#[cfg(target_os = "android")]
fn refresh_chooser_items(ui: &MainWindow, current_id: Option<i64>) {
    let items: Vec<LabelItem> = list_labels_with_selection(current_id)
        .into_iter()
        .map(|(id, name, selected)| LabelItem {
            id: id as i32,
            name: name.into(),
            selected,
        })
        .collect();
    ui.set_label_items(
        std::rc::Rc::new(slint::VecModel::from(items)).into(),
    );
}

/// Pull the active mode's resolved label name out of the DB and
/// write it into the Setup ExpanderRow's `label-name` subtitle.
#[cfg(target_os = "android")]
fn refresh_setup_label_name(ui: &MainWindow, mode: meditate_core::SessionMode) {
    ui.set_label_name(
        resolved_label_for_mode(mode)
            .map(|(name, _)| name)
            .unwrap_or_default()
            .into(),
    );
}

/// Initialise the Done expander state from Setup's current pick.
/// Called when entering the Done screen (Stop tap or auto-finish).
/// Mirrors `show_done`'s `done_selected_label_id.set(setup_selected_label_id())`
/// at `meditate-gtk/src/timer/imp.rs:2296`.
#[cfg(target_os = "android")]
fn mirror_setup_label_into_done(ui: &MainWindow, mode: meditate_core::SessionMode) {
    let active = read_label_active_for_mode(mode);
    if active {
        if let Some((name, id)) = resolved_label_for_mode(mode) {
            ui.set_done_label_active(true);
            ui.set_done_label_name(name.into());
            ui.set_done_label_id(id as i32);
            return;
        }
    }
    // Master off OR mode default got deleted — start the Done
    // expander in the off state with no pick.
    ui.set_done_label_active(false);
    ui.set_done_label_name("".into());
    ui.set_done_label_id(0);
}

/// Convenience: refresh BOTH the chooser list (with the mode's
/// resolved id as the selection) AND the Setup row's subtitle.
/// Most CRUD callsites in the Setup flow want this combined
/// behaviour. The Done flow uses the two helpers separately so
/// the chooser list reflects `done-label-id` instead.
#[cfg(target_os = "android")]
fn refresh_label_state(ui: &MainWindow, mode: meditate_core::SessionMode) {
    let current_id = if read_label_active_for_mode(mode) {
        resolved_label_for_mode(mode).map(|(_, id)| id)
    } else {
        None
    };
    refresh_chooser_items(ui, current_id);
    refresh_setup_label_name(ui, mode);
}

/// Read the persisted Box-Breath phase pattern from the DB
/// settings keys the GTK shell uses (`breathing_in` /
/// `breathing_hold_in` / `breathing_out` / `breathing_hold_out`).
/// Defaults to `BreathPattern::box_breath()` (4-4-4-4) when no
/// row exists yet. Runs `clamp_from_raw` so a stored value that
/// drifts out of the 1..=20 / 0..=20 ranges still produces a
/// well-formed pattern.
#[cfg(target_os = "android")]
fn read_breathing_pattern() -> meditate_core::breath::BreathPattern {
    use meditate_core::breath::BreathPattern;
    let Some(db_arc) = DATABASE.get() else { return BreathPattern::box_breath(); };
    let Ok(guard) = db_arc.lock() else { return BreathPattern::box_breath(); };
    let Some(db) = guard.as_ref() else { return BreathPattern::box_breath(); };
    let read = |k: &str, default: u32| -> u32 {
        meditate_core::settings_keys::read_u32(db, k, default)
    };
    BreathPattern::clamp_from_raw(
        read("breathing_in", 4),
        read("breathing_hold_in", 4),
        read("breathing_out", 4),
        read("breathing_hold_out", 4),
    )
}

#[cfg(target_os = "android")]
fn write_breathing_pattern(pattern: meditate_core::breath::BreathPattern) {
    let Some(db_arc) = DATABASE.get() else { return; };
    let Ok(guard) = db_arc.lock() else { return; };
    let Some(db) = guard.as_ref() else { return; };
    let _ = db.set_setting("breathing_in", &pattern.in_secs.to_string());
    let _ = db.set_setting("breathing_hold_in", &pattern.hold_in.to_string());
    let _ = db.set_setting("breathing_out", &pattern.out_secs.to_string());
    let _ = db.set_setting("breathing_hold_out", &pattern.hold_out.to_string());
}

/// Box-Breath session length in seconds. Persisted alongside the
/// phase pattern so toggling between modes restores the
/// per-mode last value (mirrors GTK's `breathing_session_secs`
/// Cell + the `breathing_session_secs` settings key at
/// `imp.rs:4256`). Defaults to `BREATHING_DEFAULT_SECS` = 5 min.
#[cfg(target_os = "android")]
fn read_breathing_session_secs() -> u32 {
    let Some(db_arc) = DATABASE.get() else {
        return meditate_core::session::BREATHING_DEFAULT_SECS;
    };
    let Ok(guard) = db_arc.lock() else {
        return meditate_core::session::BREATHING_DEFAULT_SECS;
    };
    let Some(db) = guard.as_ref() else {
        return meditate_core::session::BREATHING_DEFAULT_SECS;
    };
    meditate_core::settings_keys::read_u32(
        db,
        "breathing_session_secs",
        meditate_core::session::BREATHING_DEFAULT_SECS,
    )
}

#[cfg(target_os = "android")]
fn write_breathing_session_secs(secs: u32) {
    let Some(db_arc) = DATABASE.get() else { return; };
    let Ok(guard) = db_arc.lock() else { return; };
    let Some(db) = guard.as_ref() else { return; };
    let _ = db.set_setting("breathing_session_secs", &secs.to_string());
}

/// Update the Setup view's hours / minutes Slint properties from
/// a total seconds value. Used on mode change to swap the
/// otherwise-shared Duration row to the new mode's stored length.
#[cfg(target_os = "android")]
fn push_session_length_to_ui(ui: &MainWindow, total_secs: u32) {
    ui.set_setup_hours((total_secs / 3600) as i32);
    ui.set_setup_minutes(((total_secs % 3600) / 60) as i32);
}

/// Position on a rounded-rect perimeter for the given phase +
/// intra-phase progress `t ∈ [0, 1]`. `pad` is the inset, `side`
/// the square's side length, `radius` the corner radius.
///
/// Each phase covers one full edge (the straight middle) plus
/// the quarter-arc at its trailing corner, so consecutive phases
/// connect tangentially and the dot follows the visible rounded
/// outline instead of cutting through the corner with a sharp
/// 90° pivot (which is what core's `Phase::perimeter_point` does
/// — that one is a pure axis-aligned path, used by the GTK shell
/// as-is).
///
/// Path per phase, all clockwise from the bottom-left corner:
/// * `In`      — left edge bottom→top + top-left arc
/// * `HoldIn`  — top edge left→right  + top-right arc
/// * `Out`     — right edge top→bottom + bottom-right arc
/// * `HoldOut` — bottom edge right→left + bottom-left arc
#[cfg(target_os = "android")]
fn rounded_perimeter_point(
    phase: meditate_core::breath::Phase,
    t: f64,
    pad: f64,
    side: f64,
    radius: f64,
) -> (f64, f64) {
    use meditate_core::breath::Phase;
    use std::f64::consts::FRAC_PI_2;
    let t = t.clamp(0.0, 1.0);
    let straight_len = (side - 2.0 * radius).max(0.0);
    let arc_len = FRAC_PI_2 * radius;
    let total = straight_len + arc_len;
    let s = t * total;
    // Returns (cx, cy, start_angle) for the trailing arc of each
    // phase. Arc sweeps `FRAC_PI_2` from start_angle clockwise.
    let arc_param = |phase: Phase| -> (f64, f64, f64) {
        match phase {
            // Top-left corner: angle sweeps π → 3π/2.
            Phase::In => (pad + radius, pad + radius, std::f64::consts::PI),
            // Top-right corner: 3π/2 → 2π.
            Phase::HoldIn => (pad + side - radius, pad + radius, 3.0 * FRAC_PI_2),
            // Bottom-right corner: 0 → π/2.
            Phase::Out => (pad + side - radius, pad + side - radius, 0.0),
            // Bottom-left corner: π/2 → π.
            Phase::HoldOut => (pad + radius, pad + side - radius, FRAC_PI_2),
        }
    };
    if s < straight_len {
        // Straight portion of this phase's edge.
        match phase {
            Phase::In => (pad, pad + side - radius - s),
            Phase::HoldIn => (pad + radius + s, pad),
            Phase::Out => (pad + side, pad + radius + s),
            Phase::HoldOut => (pad + side - radius - s, pad + side),
        }
    } else {
        let arc_t = ((s - straight_len) / arc_len).clamp(0.0, 1.0);
        let (cx, cy, start_angle) = arc_param(phase);
        let angle = start_angle + arc_t * FRAC_PI_2;
        (cx + radius * angle.cos(), cy + radius * angle.sin())
    }
}

/// Push the four phase seconds into the Slint `bb-*` properties.
/// Called on launch + after every stepper-driven mutation so the
/// tiles stay in sync with the in-memory pattern.
#[cfg(target_os = "android")]
fn refresh_breathing_tiles(ui: &MainWindow, pattern: meditate_core::breath::BreathPattern) {
    ui.set_bb_in(pattern.in_secs as i32);
    ui.set_bb_hold_in(pattern.hold_in as i32);
    ui.set_bb_out(pattern.out_secs as i32);
    ui.set_bb_hold_out(pattern.hold_out as i32);
}

/// Build the row list for the label chooser overlay. Each row
/// carries the local rowid (id), display name, and a `selected`
/// flag that mirrors `current_id`. Returned as a plain Vec the
/// caller wraps in a `slint::VecModel`.
#[cfg(target_os = "android")]
fn list_labels_with_selection(current_id: Option<i64>) -> Vec<(i64, String, bool)> {
    let Some(db_arc) = DATABASE.get() else { return Vec::new(); };
    let Ok(guard) = db_arc.lock() else { return Vec::new(); };
    let Some(db) = guard.as_ref() else { return Vec::new(); };
    meditate_core::db::list_labels_from_db(db)
        .unwrap_or_default()
        .into_iter()
        .map(|l| (l.id, l.name, Some(l.id) == current_id))
        .collect()
}

/// Look up a label's UUID by local rowid — used by the chooser's
/// pick handler to persist the user's selection via the per-mode
/// UUID setting key (`label_uuid_key_for_mode`).
#[cfg(target_os = "android")]
fn lookup_label_uuid(id: i64) -> Option<String> {
    let db_arc = DATABASE.get()?;
    let guard = db_arc.lock().ok()?;
    let db = guard.as_ref()?;
    meditate_core::db::list_labels_from_db(db)
        .ok()?
        .into_iter()
        .find(|l| l.id == id)
        .map(|l| l.uuid.0)
}

#[cfg(target_os = "android")]
fn write_label_uuid_for_mode(mode: meditate_core::SessionMode, uuid: &str) {
    let Some(db_arc) = DATABASE.get() else { return; };
    let Ok(guard) = db_arc.lock() else { return; };
    let Some(db) = guard.as_ref() else { return; };
    if let Err(e) = meditate_core::labels::persist_uuid_for_mode(db, mode, uuid) {
        meditate_core::log(
            "settings.label_uuid",
            &format!("write FAILED mode={mode:?} uuid={uuid} err={e:?}"),
        );
    }
}

/// Live name validation for the Create-Label dialog. Trims the
/// candidate, then runs `meditate_core::validate` against the
/// `is_label_name_taken_from_db` predicate (case-insensitive
/// collision check). Returns `true` only on
/// `NameValidity::Ok` — empty input + collisions disable Create.
#[cfg(target_os = "android")]
fn validate_label_name(name: &str) -> bool {
    let trimmed = name.trim();
    let Some(db_arc) = DATABASE.get() else { return false; };
    let Ok(guard) = db_arc.lock() else { return false; };
    let Some(db) = guard.as_ref() else { return false; };
    let validity = meditate_core::validate(trimmed, |n| {
        meditate_core::db::is_label_name_taken_from_db(db, n, 0).unwrap_or(false)
    });
    validity.is_savable()
}

/// Rename-flavour of `validate_label_name`: pass the label's own
/// id as `except_id` so the unchanged name (or a case-only edit
/// of it) doesn't trip the collision check. Mirrors GTK's
/// `is_label_name_taken(name, label_id)` call inside
/// `present_rename_label_dialog`.
#[cfg(target_os = "android")]
fn validate_rename_label_name(name: &str, except_id: i64) -> bool {
    let trimmed = name.trim();
    let Some(db_arc) = DATABASE.get() else { return false; };
    let Ok(guard) = db_arc.lock() else { return false; };
    let Some(db) = guard.as_ref() else { return false; };
    let validity = meditate_core::validate(trimmed, |n| {
        meditate_core::db::is_label_name_taken_from_db(db, n, except_id).unwrap_or(false)
    });
    validity.is_savable()
}

/// Look up a label's current name by rowid — used to pre-fill
/// the Rename dialog's text entry. Mirrors the GTK shell's
/// `row.title()` read at `present_rename_label_dialog`'s call site
/// (`labels.rs:204`).
#[cfg(target_os = "android")]
fn lookup_label_name(id: i64) -> Option<String> {
    let db_arc = DATABASE.get()?;
    let guard = db_arc.lock().ok()?;
    let db = guard.as_ref()?;
    meditate_core::db::list_labels_from_db(db)
        .ok()?
        .into_iter()
        .find(|l| l.id == id)
        .map(|l| l.name)
}

/// Body text for the Delete-Label dialog. Mirrors GTK's
/// `present_delete_label_dialog` body composition: pluralised
/// "N sessions will be un-labelled" when the label tags any
/// rows, "not used by any sessions" otherwise. Routes through
/// `meditate_core::labels::delete_impact_key` so the count→variant
/// boundary stays in core.
#[cfg(target_os = "android")]
fn delete_label_impact_text(id: i64) -> String {
    use meditate_core::labels::DeleteImpactKey;
    let Some(db_arc) = DATABASE.get() else { return String::new(); };
    let Ok(guard) = db_arc.lock() else { return String::new(); };
    let Some(db) = guard.as_ref() else { return String::new(); };
    let count = meditate_core::db::label_session_count_from_db(db, id).unwrap_or(0);
    match meditate_core::labels::delete_impact_key(count) {
        DeleteImpactKey::InUse(1) => {
            "1 session tagged with this label will be un-labelled.".to_string()
        }
        DeleteImpactKey::InUse(n) => {
            format!("{n} sessions tagged with this label will be un-labelled.")
        }
        DeleteImpactKey::Unused => {
            "This label is not used by any sessions.".to_string()
        }
    }
}

#[cfg(target_os = "android")]
fn delete_label_in_db(id: i64) -> bool {
    let Some(db_arc) = DATABASE.get() else { return false; };
    let Ok(guard) = db_arc.lock() else { return false; };
    let Some(db) = guard.as_ref() else { return false; };
    match db.delete_label(id) {
        Ok(()) => {
            meditate_core::log("labels.delete", &format!("ok id={id}"));
            true
        }
        Err(e) => {
            meditate_core::log(
                "labels.delete",
                &format!("delete FAILED id={id} err={e:?}"),
            );
            false
        }
    }
}

#[cfg(target_os = "android")]
fn rename_label_in_db(id: i64, name: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return false;
    }
    let Some(db_arc) = DATABASE.get() else { return false; };
    let Ok(guard) = db_arc.lock() else { return false; };
    let Some(db) = guard.as_ref() else { return false; };
    match db.update_label(id, trimmed) {
        Ok(()) => {
            meditate_core::log(
                "labels.rename",
                &format!("ok id={id} new_name={trimmed}"),
            );
            true
        }
        Err(e) => {
            meditate_core::log(
                "labels.rename",
                &format!("update FAILED id={id} new_name={trimmed} err={e:?}"),
            );
            false
        }
    }
}

/// Insert a new label row + return `(rowid, uuid)`. Mirrors the
/// GTK shell's `create_label` wrapper (`meditate-gtk/src/db/mod.rs`):
/// `insert_label` returns the rowid; the freshly inserted UUID is
/// then read back via `list_labels_from_db` so the caller can
/// persist it as the active mode's selection.
#[cfg(target_os = "android")]
fn create_label_in_db(name: &str) -> Option<(i64, String)> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return None;
    }
    let Some(db_arc) = DATABASE.get() else { return None; };
    let Ok(guard) = db_arc.lock() else { return None; };
    let Some(db) = guard.as_ref() else { return None; };
    let id = match db.insert_label(trimmed) {
        Ok(id) => id,
        Err(e) => {
            // Duplicate raced past live validation (UNIQUE
            // constraint slipped through) — GTK surfaces this via
            // a toast; we just log it. Acceptable since live
            // validation already blocks the common path.
            meditate_core::log(
                "labels.create",
                &format!("insert FAILED name={trimmed} err={e:?}"),
            );
            return None;
        }
    };
    let uuid = meditate_core::db::list_labels_from_db(db)
        .ok()?
        .into_iter()
        .find(|l| l.id == id)
        .map(|l| l.uuid.0)?;
    meditate_core::log(
        "labels.create",
        &format!("ok name={trimmed} id={id} uuid={uuid}"),
    );
    Some((id, uuid))
}

#[cfg(target_os = "android")]
fn write_signal_mode_for_mode(
    mode: meditate_core::SessionMode,
    value: meditate_core::SignalMode,
) {
    let Some(db_arc) = DATABASE.get() else { return; };
    let Ok(guard) = db_arc.lock() else { return; };
    let Some(db) = guard.as_ref() else { return; };
    let key = meditate_core::settings_keys::signal_mode_key_for_mode(mode);
    if let Err(e) = db.set_setting(key, value.as_db_str()) {
        meditate_core::log(
            "settings.cues",
            &format!("write FAILED mode={mode:?} value={value:?} err={e:?}"),
        );
    }
}

#[cfg(target_os = "android")]
fn finalize_session(
    unix_start: i64,
    elapsed_secs: i64,
    note: Option<String>,
    mode: meditate_core::SessionMode,
    label_id: Option<i64>,
) {
    if elapsed_secs <= 0 {
        // Drop sessions that ended before any seconds elapsed —
        // matches the GTK shell, which also filters zero-duration
        // rows out of insert. Avoids noise in stats from accidental
        // Start→Stop double-taps.
        return;
    }
    let Some(db_arc) = DATABASE.get() else { return; };
    let Ok(guard) = db_arc.lock() else { return; };
    let Some(db) = guard.as_ref() else {
        meditate_core::log(
            "session.insert",
            "skipped: db not open (open failed at startup)",
        );
        return;
    };
    let session = meditate_core::db::Session::from_unix(
        unix_start,
        elapsed_secs,
        // `label_id` arrives resolved from the per-mode label
        // toggle + persisted UUID; None when the master switch is
        // off OR the seeded default row got deleted.
        label_id,
        note,
        mode,
        // Guided file UUID: only meaningful when the user picked a
        // library file via the Guided audio picker (Phase 5). Until
        // then this is always None — both Timer and Box Breath
        // sessions don't carry a guided-file reference, and even
        // future transient "Open File" guided picks log None.
        None,
    );
    match db.insert_session(&session) {
        Ok(rowid) => meditate_core::log(
            "session.insert",
            &format!("ok rowid={rowid} duration_secs={elapsed_secs} start_unix={unix_start}"),
        ),
        Err(e) => meditate_core::log(
            "session.insert",
            &format!(
                "FAILED err={e:?} duration_secs={elapsed_secs} start_unix={unix_start}"
            ),
        ),
    }
}

fn build_ui() -> MainWindow {
    let ui = MainWindow::new().unwrap();
    let state = Rc::new(RefCell::new(AppState::idle()));
    // Active mode chip — drives both the Setup body content and
    // the `SessionMode` recorded on Save. Defaults to Timer at
    // launch; persistence across launches lands when settings
    // wiring arrives. Shared with the mode-changed callback and
    // the Save handler so both read the same source of truth.
    let current_mode = Rc::new(Cell::new(TimerMode::default()));

    // Per-mode session-length cells. Mirrors the GTK shell's
    // pair of state Cells backing the shared `duration_row`
    // widget (`countdown_target_secs` for Timer + Guided's
    // duration / `breathing_session_secs` for Box Breath).
    // Timer mode keeps its value in-memory only (defaults to the
    // 0h 10m boot value); Box Breath persists to the DB so
    // round-trips across launches.
    let timer_session_secs: Rc<Cell<u32>> = Rc::new(Cell::new(
        (DEFAULT_HOURS as u32) * 3600 + (DEFAULT_MINUTES as u32) * 60,
    ));

    // Box Breath target for the in-flight session. Set in
    // `on_action_tap` when starting a `BoxBreathCountdown` (the
    // cycle-aligned seconds); `None` for stopwatch shapes or
    // when no BB session is in flight. Read each tick to feed
    // `box_breath_counter_label` the right "elapsed / target" vs
    // "elapsed only" branch.
    #[cfg(target_os = "android")]
    let bb_target_secs: Rc<Cell<Option<u32>>> = Rc::new(Cell::new(None));
    // Unix timestamp captured at session start, taken at end.
    // Mirrors the GTK shell's `Timer::session_start_time` cell.
    // Holds None while idle; Some(unix_secs) while a session is
    // in flight. The core::Session itself uses monotonic boot-time
    // durations, so wall-clock start has to be carried separately.
    // android-only — host has no DB to persist into, so we don't
    // even allocate the cell on the desktop preview path.
    #[cfg(target_os = "android")]
    let session_start_unix: Rc<Cell<Option<i64>>> = Rc::new(Cell::new(None));

    // When a session ends (Stop tap or auto-finish), we stash the
    // (start_unix, elapsed_secs) pair here so the Done-screen
    // Save / Discard handler can either commit it as a DB row or
    // drop it. None while the Done screen isn't up. Mirrors the
    // GTK shell's `Timer::pending_save_data` deferral: persistence
    // is the *user's* call on the Done screen, not an automatic
    // side-effect of Stop.
    #[cfg(target_os = "android")]
    let pending_done: Rc<Cell<Option<(i64, i64)>>> = Rc::new(Cell::new(None));

    // Seed the stepper-driven duration with the same default the
    // GTK shell opens at. The tick loop further down refreshes the
    // Setup hero every 200 ms so stepper changes flow into the
    // big mm:ss readout without a dedicated change-notification path.
    ui.set_setup_hours(DEFAULT_HOURS);
    ui.set_setup_minutes(DEFAULT_MINUTES);

    {
        let weak = ui.as_weak();
        let state = state.clone();
        #[cfg(target_os = "android")]
        let session_start_unix = session_start_unix.clone();
        #[cfg(target_os = "android")]
        let bb_target_secs = bb_target_secs.clone();
        ui.on_action_tap(move || {
            let now = now_since_epoch();
            let Some(ui) = weak.upgrade() else { return; };
            let target = configured_duration(&ui);
            // Shape picked from (mode chip × Stopwatch-Mode switch).
            // Same shell-side choice the GTK `on_start` makes via
            // `current_mode()` + `stopwatch_toggle_on`. Guided is
            // still gated upstream by the disabled Start button
            // until the audio engine ships (phase 5).
            use meditate_core::session::SessionShape;
            let shape = match TimerMode::from_chip_index(ui.get_setup_mode()) {
                TimerMode::Breathing => {
                    let pattern = meditate_core::breath::BreathPattern::clamp_from_raw(
                        ui.get_bb_in().max(0) as u32,
                        ui.get_bb_hold_in().max(0) as u32,
                        ui.get_bb_out().max(0) as u32,
                        ui.get_bb_hold_out().max(0) as u32,
                    );
                    if ui.get_stopwatch_on() {
                        SessionShape::BoxBreathStopwatch { pattern }
                    } else {
                        // Cycle-aligned target so the session always
                        // ends on a phase boundary, not mid-cycle.
                        let raw = target.as_secs();
                        let aligned = pattern.cycle_aligned_target_secs(raw) as u32;
                        SessionShape::BoxBreathCountdown {
                            pattern,
                            target_secs: aligned,
                        }
                    }
                }
                _ => {
                    if ui.get_stopwatch_on() {
                        SessionShape::TimerStopwatch
                    } else {
                        SessionShape::TimerCountdown {
                            target_secs: target.as_secs() as u32,
                        }
                    }
                }
            };
            let mut s = state.borrow_mut();
            // No live elapsed capture needed here: action_tap on Active
            // pauses/resumes — both stay Active, so the session never
            // ends through this path. Only Idle/Finished → Active
            // matters for persistence wiring.
            // Stash the BB target (if any) before consuming the
            // shape — the tick loop reads it every frame to feed
            // `box_breath_counter_label`.
            #[cfg(target_os = "android")]
            {
                bb_target_secs.set(match &shape {
                    SessionShape::BoxBreathCountdown { target_secs, .. } => {
                        Some(*target_secs)
                    }
                    _ => None,
                });
            }
            let was_active = s.is_active();
            let next = std::mem::replace(&mut *s, AppState::idle())
                .toggle(shape, now);
            *s = next;
            let is_active = s.is_active();
            #[cfg(target_os = "android")]
            if !was_active && is_active {
                session_start_unix.set(Some(meditate_core::time::unix_now()));
            }
            on_state_changed(was_active, is_active);
            refresh(&ui, &s, now);
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        let current_mode = current_mode.clone();
        #[cfg(target_os = "android")]
        let session_start_unix = session_start_unix.clone();
        #[cfg(target_os = "android")]
        let pending_done = pending_done.clone();
        ui.on_stop_tap(move || {
            let now = now_since_epoch();
            let mut s = state.borrow_mut();
            // Capture elapsed BEFORE the mutation — once stop()
            // advances to Finished the Box<Session> is dropped, so
            // we can't ask it for elapsed afterwards.
            let elapsed_secs = match &*s {
                AppState::Active(session) => session.elapsed(now).as_secs() as i64,
                _ => 0,
            };
            let was_active = s.is_active();
            *s = std::mem::replace(&mut *s, AppState::idle()).stop();
            let is_active = s.is_active();
            // Active → Finished: stash the (start, elapsed) pair
            // so the Save / Discard handler knows what to do, push
            // the elapsed readout into the Done view, and mirror
            // Setup's resolved label into the Done expander state.
            // Mirrors `show_done`'s `done_selected_label_id.set(setup_selected_label_id())`
            // call at `meditate-gtk/src/timer/imp.rs:2296`.
            if was_active && !is_active {
                #[cfg(target_os = "android")]
                if let Some(unix_start) = session_start_unix.take() {
                    pending_done.set(Some((unix_start, elapsed_secs)));
                }
                if let Some(ui) = weak.upgrade() {
                    ui.set_elapsed_text(
                        meditate_core::format::format_time(
                            Duration::from_secs(elapsed_secs.max(0) as u64),
                        )
                        .into(),
                    );
                    ui.set_note_text("".into());
                    #[cfg(target_os = "android")]
                    mirror_setup_label_into_done(&ui, current_mode.get().into());
                }
            }
            on_state_changed(was_active, is_active);
            if let Some(ui) = weak.upgrade() {
                refresh(&ui, &s, now);
            }
            let _ = current_mode.get();
        });
    }

    // Duration row tap: seed the dialog's edit-state copies from the
    // currently configured target, then open the dialog. The Slint
    // side commits dialog → setup on Set; the next tick refresh
    // picks up the new HH:MM in both the hero and the row value.
    {
        let weak = ui.as_weak();
        ui.on_duration_tap(move || {
            let Some(ui) = weak.upgrade() else { return; };
            ui.set_dialog_hours(ui.get_setup_hours());
            ui.set_dialog_minutes(ui.get_setup_minutes());
            ui.set_duration_dialog_open(true);
        });
    }

    // Tick loop — drives the live countdown and the auto-finish
    // edge. Leaked so it lives for the lifetime of the process; the
    // app has exactly one timer and we never want to drop it.
    let timer = Box::leak(Box::new(slint::Timer::default()));
    {
        let weak = ui.as_weak();
        let state = state.clone();
        let current_mode = current_mode.clone();
        #[cfg(target_os = "android")]
        let session_start_unix = session_start_unix.clone();
        #[cfg(target_os = "android")]
        let pending_done = pending_done.clone();
        #[cfg(target_os = "android")]
        let bb_target_secs = bb_target_secs.clone();
        timer.start(slint::TimerMode::Repeated, TICK, move || {
            let now = now_since_epoch();
            let mut s = state.borrow_mut();
            // Capture before/after active-state so an auto-finish
            // (Active → Finished on Overtime cross) tears down the
            // foreground service AND stashes the session for the
            // Done screen. tick on an inactive state is a no-op,
            // so the equality check is the cheap path.
            #[cfg(target_os = "android")]
            let elapsed_secs = match &*s {
                AppState::Active(session) => session.elapsed(now).as_secs() as i64,
                _ => 0,
            };
            let was_active = s.is_active();
            *s = std::mem::replace(&mut *s, AppState::idle()).tick(now);
            let is_active = s.is_active();
            #[cfg(target_os = "android")]
            if was_active && !is_active {
                if let Some(unix_start) = session_start_unix.take() {
                    pending_done.set(Some((unix_start, elapsed_secs)));
                }
                bb_target_secs.set(None);
                if let Some(ui) = weak.upgrade() {
                    ui.set_elapsed_text(
                        meditate_core::format::format_time(
                            Duration::from_secs(elapsed_secs.max(0) as u64),
                        )
                        .into(),
                    );
                    ui.set_note_text("".into());
                    mirror_setup_label_into_done(&ui, current_mode.get().into());
                    ui.set_bb_running_active(false);
                }
            }
            // While a Box-Breath session is running, push the
            // per-frame visualisation properties (phase label,
            // remaining seconds, dot perimeter coordinates,
            // counter text). `box_breath_phase_info` returns
            // None for non-BB or non-Running shapes, so this is
            // a single guarded path that handles all the
            // "not BB" cases correctly.
            #[cfg(target_os = "android")]
            if let AppState::Active(session) = &*s {
                if let Some(info) = session.box_breath_phase_info(now) {
                    if let Some(ui) = weak.upgrade() {
                        let elapsed = session.elapsed(now);
                        let target = bb_target_secs
                            .get()
                            .map(|s| Duration::from_secs(u64::from(s)));
                        let counter = meditate_core::format::box_breath_counter_label(
                            elapsed, target,
                        );
                        let label = match info.phase.running_label_key() {
                            meditate_core::breath::PhaseRunningLabelKey::BreatheIn => {
                                "Breathe in"
                            }
                            meditate_core::breath::PhaseRunningLabelKey::Hold => "Hold",
                            meditate_core::breath::PhaseRunningLabelKey::BreatheOut => {
                                "Breathe out"
                            }
                        };
                        let phase_secs = info
                            .remaining
                            .as_secs_f64()
                            .ceil()
                            .max(0.0) as i32;
                        let t = if info.total.as_secs_f64() > 0.0 {
                            (info.elapsed_in_phase.as_secs_f64()
                                / info.total.as_secs_f64())
                            .clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        // pad=12, side=196 matches the Slint
                        // 220×220 container's 12 px inset + 196 px
                        // inner square; radius=20 matches the
                        // `border-radius: 20px` on the frame
                        // Rectangle so the dot trajectory hugs the
                        // visible corners instead of cutting them.
                        let (x, y) = rounded_perimeter_point(
                            info.phase, t, 12.0, 196.0, 20.0,
                        );
                        ui.set_bb_running_active(true);
                        ui.set_bb_counter_text(counter.into());
                        ui.set_bb_phase_label(label.into());
                        ui.set_bb_phase_seconds(phase_secs);
                        ui.set_bb_dot_x(x as f32);
                        ui.set_bb_dot_y(y as f32);
                    }
                } else if let Some(ui) = weak.upgrade() {
                    // Active but not BB (Timer mode) — make sure
                    // the BB layout is hidden.
                    ui.set_bb_running_active(false);
                }
            }
            on_state_changed(was_active, is_active);
            if let Some(ui) = weak.upgrade() {
                refresh(&ui, &s, now);
            }
            let _ = current_mode.get();
        });
    }

    // Save tap on the Done screen: commit the pending session as a
    // DB row (with the note text the user entered, and the mode
    // that was active at session start), then dismiss to Idle.
    // The slide-off-right animation already revealed Done under
    // Running on Stop; Save / Discard just instantly swap the base
    // layer back to Setup ("the done page just disappears" per the
    // GTK pattern).
    {
        let weak = ui.as_weak();
        let state = state.clone();
        let current_mode = current_mode.clone();
        #[cfg(target_os = "android")]
        let pending_done = pending_done.clone();
        ui.on_save_tap(move || {
            let Some(ui) = weak.upgrade() else { return; };
            #[cfg(target_os = "android")]
            if let Some((unix_start, elapsed_secs)) = pending_done.take() {
                let note = ui.get_note_text().to_string();
                let note = if note.trim().is_empty() { None } else { Some(note) };
                let mode: meditate_core::SessionMode = current_mode.get().into();
                // The Done expander's per-session pick drives both
                // the session row's label_id AND the persist-back
                // to the active mode's UUID setting (so the user's
                // pick on Done becomes next session's default).
                // Mirrors GTK's Save flow at `imp.rs:2336-2383`.
                let picked: Option<i64> = if ui.get_done_label_active() {
                    let id = ui.get_done_label_id() as i64;
                    if id > 0 { Some(id) } else { None }
                } else {
                    None
                };
                // Apply the persist action — but read the labels
                // list inside the same DB lock as the writes.
                if let (Some(db_arc), Some(_)) = (DATABASE.get(), Some(())) {
                    if let Ok(guard) = db_arc.lock() {
                        if let Some(db) = guard.as_ref() {
                            let labels = meditate_core::db::list_labels_from_db(db)
                                .unwrap_or_default();
                            match meditate_core::labels::resolve_persist_action(picked, &labels) {
                                meditate_core::labels::PersistAction::SetUuidAndActivate { uuid } => {
                                    let _ = meditate_core::labels::persist_uuid_for_mode(
                                        db, mode, uuid.as_str(),
                                    );
                                    let _ = meditate_core::labels::persist_active_for_mode(
                                        db, mode, true,
                                    );
                                }
                                meditate_core::labels::PersistAction::Deactivate => {
                                    let _ = meditate_core::labels::persist_active_for_mode(
                                        db, mode, false,
                                    );
                                }
                                meditate_core::labels::PersistAction::NoOp => {}
                            }
                        }
                    }
                }
                finalize_session(unix_start, elapsed_secs, note, mode, picked);
                // Refresh the Setup row so when Done slides off and
                // reveals Setup, the ExpanderRow's master toggle +
                // subtitle reflect the post-Save mode state.
                refresh_setup_label_name(&ui, mode);
                ui.set_label_active(read_label_active_for_mode(mode));
            }
            #[cfg(not(target_os = "android"))]
            let _ = current_mode.get();
            let mut s = state.borrow_mut();
            *s = std::mem::replace(&mut *s, AppState::idle()).dismiss();
            refresh(&ui, &s, now_since_epoch());
        });
    }

    // Per-phase stepper. Mirrors GTK's `adjust_phase(index, delta,
    // min_val)` at `meditate-gtk/src/timer/imp.rs:4175`: read the
    // current pattern, mutate the addressed phase, clamp into
    // [min, PHASE_MAX_SECS], skip on no-op, persist + push back
    // to the Slint tiles. min-policy lives in
    // `BreathPattern::phase_min_secs(index)` so the same rule
    // (inhale/exhale ≥ 1, holds ≥ 0) applies across both shells.
    {
        let weak = ui.as_weak();
        ui.on_bb_adjust(move |index, delta| {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                let mut pattern = read_breathing_pattern();
                let min = meditate_core::breath::BreathPattern::phase_min_secs(
                    index.max(0) as u8,
                );
                let slot: &mut u32 = match index {
                    0 => &mut pattern.in_secs,
                    1 => &mut pattern.hold_in,
                    2 => &mut pattern.out_secs,
                    3 => &mut pattern.hold_out,
                    _ => return,
                };
                let new_val = ((*slot as i32) + delta).clamp(
                    min as i32,
                    meditate_core::breath::PHASE_MAX_SECS as i32,
                ) as u32;
                if new_val == *slot { return; }
                *slot = new_val;
                write_breathing_pattern(pattern);
                refresh_breathing_tiles(&ui, pattern);
            }
            let _ = (weak.clone(), index, delta);
        });
    }

    // Mode chip group changed — update the shared mode cell so the
    // next Save records the right SessionMode, and load any per-
    // mode persisted state into the Slint properties. Mirrors the
    // GTK shell's `on_mode_switched`, which also refreshes per-mode
    // settings rows (Cues + Keep Awake here; Stopwatch / Label /
    // others join as those rows land).
    {
        let weak = ui.as_weak();
        let current_mode = current_mode.clone();
        let timer_session_secs = timer_session_secs.clone();
        ui.on_mode_changed(move |idx| {
            let new_mode = TimerMode::from_chip_index(idx);
            current_mode.set(new_mode);
            #[cfg(target_os = "android")]
            if let Some(ui) = weak.upgrade() {
                let core_mode: meditate_core::SessionMode = new_mode.into();
                ui.set_keep_awake_on(read_keep_awake_for_mode(core_mode));
                ui.set_cues_mode(signal_mode_to_chip_index(
                    read_signal_mode_for_mode(core_mode),
                ));
                ui.set_label_active(read_label_active_for_mode(core_mode));
                ui.set_label_name(
                    resolved_label_for_mode(core_mode)
                        .map(|(name, _)| name)
                        .unwrap_or_default()
                        .into(),
                );
                // Swap the Duration row's displayed value to the
                // mode's stored session length. Guided's length
                // comes from the audio file (phase 5); for now
                // it falls back to the Timer cell so the row
                // isn't blank.
                let new_secs = match new_mode {
                    TimerMode::Breathing => read_breathing_session_secs(),
                    _ => timer_session_secs.get(),
                };
                push_session_length_to_ui(&ui, new_secs);
            }
            let _ = (weak.clone(), timer_session_secs.clone());
        });
    }

    // Duration dialog Set committed — route the new total
    // seconds to the active mode's storage. Timer mode keeps it
    // in the in-memory cell; Box Breath persists to the DB
    // (`breathing_session_secs`) so the value survives across
    // launches, matching GTK's `set_breathing_duration_secs`.
    {
        let weak = ui.as_weak();
        let current_mode = current_mode.clone();
        let timer_session_secs = timer_session_secs.clone();
        ui.on_duration_committed(move || {
            let Some(ui) = weak.upgrade() else { return; };
            let total_secs = (ui.get_setup_hours().max(0) as u32) * 3600
                + (ui.get_setup_minutes().max(0) as u32) * 60;
            match current_mode.get() {
                TimerMode::Breathing => {
                    #[cfg(target_os = "android")]
                    write_breathing_session_secs(total_secs);
                }
                _ => timer_session_secs.set(total_secs),
            }
        });
    }

    // Keep-Screen-Awake toggle — write to the current mode's
    // persisted setting. Real WakeLock acquisition is a phase-8
    // platform-edge job; this slice just persists the user's
    // pick so the eventual WakeLock wiring has a value to read.
    {
        let current_mode = current_mode.clone();
        ui.on_keep_awake_toggled(move |value| {
            #[cfg(target_os = "android")]
            write_keep_awake_for_mode(current_mode.get().into(), value);
            // Mirror the GTK behaviour on host builds — the
            // property already updated via the in-out binding;
            // nothing else to do without a DB. Touching the
            // captures keeps the closure non-empty.
            let _ = (current_mode.get(), value);
        });
    }

    // Cues SegmentedButton change — same persistence shape as
    // Keep-Awake. The audio + haptic engines that actually consume
    // the SignalMode value land in platform-edge phase 5; this
    // slice just keeps the user's pick alive across launches.
    {
        let current_mode = current_mode.clone();
        ui.on_cues_changed(move |idx| {
            #[cfg(target_os = "android")]
            write_signal_mode_for_mode(
                current_mode.get().into(),
                signal_mode_from_chip_index(idx),
            );
            let _ = (current_mode.get(), idx);
        });
    }

    // Label master switch — persist the active flag per mode.
    // Mirrors GTK's `connect_enable_expansion_notify` handler
    // (which writes via `meditate_core::labels::persist_active_for_mode`).
    {
        let current_mode = current_mode.clone();
        ui.on_label_active_toggled(move |value| {
            #[cfg(target_os = "android")]
            write_label_active_for_mode(current_mode.get().into(), value);
            let _ = (current_mode.get(), value);
        });
    }

    // Label inner-row tap (Setup) — load the labels list with the
    // active mode's current selection marked, set chooser-target=0
    // so picks route back to the mode setting, then open the
    // chooser. Mirrors GTK's `setup_label_chooser_row.activated`.
    {
        let weak = ui.as_weak();
        let current_mode = current_mode.clone();
        ui.on_label_tap(move || {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                ui.set_chooser_target(0);
                refresh_label_state(&ui, current_mode.get().into());
                ui.set_labels_page(true);
            }
            let _ = (weak.clone(), current_mode.get());
        });
    }

    // Done expander master toggle — local state only (does NOT
    // write the mode setting; the persist-back happens on Save
    // via `resolve_persist_action`). Mirrors GTK's
    // `done_label_enabled_row.connect_enable_expansion_notify`
    // at `imp.rs:578-597`: toggling off clears the pick;
    // toggling on adopts the mode-default when no pick is set.
    {
        let weak = ui.as_weak();
        let current_mode = current_mode.clone();
        ui.on_done_label_active_toggled(move |on| {
            let Some(ui) = weak.upgrade() else { return; };
            if !on {
                ui.set_done_label_id(0);
                ui.set_done_label_name("".into());
                return;
            }
            #[cfg(target_os = "android")]
            if ui.get_done_label_id() == 0 {
                let mode: meditate_core::SessionMode = current_mode.get().into();
                if let Some((name, id)) = resolved_label_for_mode(mode) {
                    ui.set_done_label_id(id as i32);
                    ui.set_done_label_name(name.into());
                }
            }
            let _ = current_mode.get();
        });
    }

    // Done inner-row tap — open the same label chooser, but with
    // chooser-target=1 so picks update Done state rather than the
    // mode's UUID setting. The check-mark inside the chooser
    // reflects `done-label-id`. Mirrors GTK's
    // `done_label_chooser_row.connect_activated` at `imp.rs:598`.
    {
        let weak = ui.as_weak();
        ui.on_done_label_tap(move || {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                ui.set_chooser_target(1);
                let id = ui.get_done_label_id() as i64;
                let current_id = if id > 0 { Some(id) } else { None };
                refresh_chooser_items(&ui, current_id);
                ui.set_labels_page(true);
            }
            let _ = weak.clone();
        });
    }

    // Chooser back arrow — just close the overlay. The selection
    // hasn't changed, so no Slint property refresh is needed.
    {
        let weak = ui.as_weak();
        ui.on_labels_back(move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_labels_page(false);
            }
        });
    }

    // Synthetic "Create new label…" row tap — clear any prior
    // entry text, mark Create disabled, and open the dialog.
    // Mirrors the GTK `create_row.activated` handler at
    // `labels.rs:118`.
    {
        let weak = ui.as_weak();
        ui.on_create_label_tap(move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_create_label_text("".into());
                ui.set_create_label_valid(false);
                ui.set_create_label_dialog_open(true);
            }
        });
    }

    // Create-label text changed — revalidate against
    // `is_label_name_taken_from_db` and update Create's enabled
    // state. Mirrors the GTK `entry.connect_changed` validate
    // closure at `labels.rs:267`.
    {
        let weak = ui.as_weak();
        ui.on_create_label_changed(move |text| {
            if let Some(ui) = weak.upgrade() {
                #[cfg(target_os = "android")]
                ui.set_create_label_valid(validate_label_name(&text));
                #[cfg(not(target_os = "android"))]
                let _ = (ui, text);
            }
        });
    }

    // Create button pressed — insert the new label, then route
    // by `chooser-target`:
    //   0 = Setup flow → persist UUID, refresh Setup state, close.
    //   1 = Done flow → adopt the new label as the Done pick,
    //       close. Mode setting unchanged (Save will persist via
    //       resolve_persist_action).
    // Treating creation as selection mirrors GTK's
    // `labels.rs:125-134`.
    {
        let weak = ui.as_weak();
        let current_mode = current_mode.clone();
        ui.on_create_label_confirm(move || {
            let Some(ui) = weak.upgrade() else { return; };
            #[cfg(target_os = "android")]
            {
                let text = ui.get_create_label_text().to_string();
                if let Some((id, uuid)) = create_label_in_db(&text) {
                    if ui.get_chooser_target() == 1 {
                        // Done flow
                        ui.set_done_label_id(id as i32);
                        ui.set_done_label_name(text.trim().into());
                        ui.set_done_label_active(true);
                    } else {
                        // Setup flow
                        let mode: meditate_core::SessionMode = current_mode.get().into();
                        write_label_uuid_for_mode(mode, &uuid);
                        refresh_label_state(&ui, mode);
                    }
                }
            }
            ui.set_create_label_dialog_open(false);
            ui.set_labels_page(false);
            let _ = current_mode.get();
        });
    }

    // Rename pencil tap — pre-fill the dialog with the row's
    // current name + open it. `rename-label-valid` starts true
    // (the unchanged name is always valid against the row's own
    // id, thanks to `except_id`). Mirrors GTK's
    // `present_rename_label_dialog`'s initial `validate()` call
    // at `labels.rs:333`.
    {
        let weak = ui.as_weak();
        ui.on_rename_label_tap(move |id| {
            let Some(ui) = weak.upgrade() else { return; };
            #[cfg(target_os = "android")]
            {
                let current = lookup_label_name(id as i64).unwrap_or_default();
                ui.set_rename_label_id(id);
                ui.set_rename_label_text(current.into());
                ui.set_rename_label_valid(true);
                ui.set_rename_label_dialog_open(true);
            }
            #[cfg(not(target_os = "android"))]
            let _ = (ui, id);
        });
    }

    // Rename-label text changed — revalidate against the same
    // collision check Create uses, but with the row's id as
    // `except_id` so the user can keep typing the existing name
    // (case-only edits, undo a typo, etc.). Mirrors GTK's
    // `entry.connect_changed` at `labels.rs:335`.
    {
        let weak = ui.as_weak();
        ui.on_rename_label_changed(move |text| {
            if let Some(ui) = weak.upgrade() {
                #[cfg(target_os = "android")]
                {
                    let id = ui.get_rename_label_id() as i64;
                    ui.set_rename_label_valid(validate_rename_label_name(&text, id));
                }
                #[cfg(not(target_os = "android"))]
                let _ = (ui, text);
            }
        });
    }

    // Rename button pressed — call `db.update_label` and refresh
    // the chooser + ExpanderRow. The active-mode UUID setting
    // doesn't need a write — the row's UUID is unchanged, only
    // the name is. Mirrors GTK's `update_label + rebuilder()` at
    // `labels.rs:344-350`.
    {
        let weak = ui.as_weak();
        let current_mode = current_mode.clone();
        ui.on_rename_label_confirm(move || {
            let Some(ui) = weak.upgrade() else { return; };
            #[cfg(target_os = "android")]
            {
                let id = ui.get_rename_label_id() as i64;
                let text = ui.get_rename_label_text().to_string();
                if rename_label_in_db(id, &text) {
                    let mode: meditate_core::SessionMode = current_mode.get().into();
                    refresh_label_state(&ui, mode);
                }
            }
            ui.set_rename_label_dialog_open(false);
            let _ = current_mode.get();
        });
    }

    // Delete X-button tap — compose the impact body and open
    // the confirmation dialog. Mirrors the open path in GTK's
    // `present_delete_label_dialog` (labels.rs:361-407).
    {
        let weak = ui.as_weak();
        ui.on_delete_label_tap(move |id| {
            let Some(ui) = weak.upgrade() else { return; };
            #[cfg(target_os = "android")]
            {
                ui.set_delete_label_id(id);
                ui.set_delete_label_body(delete_label_impact_text(id as i64).into());
                ui.set_delete_label_dialog_open(true);
            }
            #[cfg(not(target_os = "android"))]
            let _ = (ui, id);
        });
    }

    // Delete button pressed — call `db.delete_label` and
    // refresh. `resolve_label_for_mode` falls back to the
    // mode's seeded default UUID when the per-mode UUID setting
    // points at a now-gone row, so deleting the currently-
    // selected label just rolls the ExpanderRow subtitle back
    // to the default (e.g. "Meditation" for Timer).
    {
        let weak = ui.as_weak();
        let current_mode = current_mode.clone();
        ui.on_delete_label_confirm(move || {
            let Some(ui) = weak.upgrade() else { return; };
            #[cfg(target_os = "android")]
            {
                let id = ui.get_delete_label_id() as i64;
                if delete_label_in_db(id) {
                    let mode: meditate_core::SessionMode = current_mode.get().into();
                    refresh_label_state(&ui, mode);
                }
            }
            ui.set_delete_label_dialog_open(false);
            let _ = current_mode.get();
        });
    }

    // User picked a label row — route based on `chooser-target`:
    //   0 = Setup flow → persist UUID to the active mode's setting,
    //       refresh the Setup ExpanderRow's subtitle. Mirrors GTK's
    //       Setup `on_selected` at `imp.rs:744-749`.
    //   1 = Done flow → update Done state ONLY; persistence to the
    //       mode setting happens on Save via `resolve_persist_action`.
    //       Mirrors GTK's Done `on_selected` at `imp.rs:608-612`.
    {
        let weak = ui.as_weak();
        let current_mode = current_mode.clone();
        ui.on_label_picked(move |id| {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                if ui.get_chooser_target() == 1 {
                    // Done flow
                    if let Some(name) = lookup_label_name(id as i64) {
                        ui.set_done_label_id(id);
                        ui.set_done_label_name(name.into());
                        ui.set_done_label_active(true);
                    }
                } else {
                    // Setup flow (existing behavior)
                    let mode: meditate_core::SessionMode = current_mode.get().into();
                    if let Some(uuid) = lookup_label_uuid(id as i64) {
                        write_label_uuid_for_mode(mode, &uuid);
                    }
                    refresh_setup_label_name(&ui, mode);
                }
                ui.set_labels_page(false);
            }
            let _ = (weak.clone(), current_mode.get(), id);
        });
    }

    // Discard tap: drop the pending session without writing a row,
    // then dismiss to Idle. Mirrors the GTK shell's `on_discard`.
    {
        let weak = ui.as_weak();
        let state = state.clone();
        #[cfg(target_os = "android")]
        let pending_done = pending_done.clone();
        ui.on_discard_tap(move || {
            #[cfg(target_os = "android")]
            {
                pending_done.set(None);
            }
            let mut s = state.borrow_mut();
            *s = std::mem::replace(&mut *s, AppState::idle()).dismiss();
            if let Some(ui) = weak.upgrade() {
                refresh(&ui, &s, now_since_epoch());
            }
        });
    }

    // Seed per-mode persisted settings into Slint properties for
    // the default mode (Timer). Subsequent mode flips refresh the
    // same properties via the `on_mode_changed` handler above.
    #[cfg(target_os = "android")]
    {
        let core_mode: meditate_core::SessionMode = current_mode.get().into();
        ui.set_keep_awake_on(read_keep_awake_for_mode(core_mode));
        ui.set_cues_mode(signal_mode_to_chip_index(
            read_signal_mode_for_mode(core_mode),
        ));
        ui.set_label_active(read_label_active_for_mode(core_mode));
        ui.set_label_name(
            resolved_label_for_mode(core_mode)
                .map(|(name, _)| name)
                .unwrap_or_default()
                .into(),
        );
        refresh_breathing_tiles(&ui, read_breathing_pattern());
    }

    // Android back gesture / hardware back button — Slint maps
    // `Keycode::Back` to `Key.Back`, dispatched via the normal
    // key-event path. We catch it in the root FocusScope inside
    // main.slint and surface it here so "back" consistently means
    // "go up one level":
    //   * Labels chooser open → close the chooser
    //   * Done screen open → discard (same as the Discard button)
    //   * Running overlay up → swallow (don't kill an in-flight
    //     session via stray gesture)
    //   * Otherwise: do nothing (the OS will close the app at the
    //     next press if no Slint handler accepted — or the user
    //     can swipe-up to Home).
    {
        let weak = ui.as_weak();
        let state = state.clone();
        #[cfg(target_os = "android")]
        let pending_done = pending_done.clone();
        ui.on_back_pressed(move || {
            let Some(ui) = weak.upgrade() else { return; };
            if ui.get_labels_page() {
                ui.set_labels_page(false);
                return;
            }
            if ui.get_done_page() {
                #[cfg(target_os = "android")]
                pending_done.set(None);
                let mut s = state.borrow_mut();
                *s = std::mem::replace(&mut *s, AppState::idle()).dismiss();
                refresh(&ui, &s, now_since_epoch());
                return;
            }
            // Running page back is swallowed by the FocusScope
            // accepting the event but doing nothing here — keeps
            // a session safe from a stray back gesture.
            // Idle setup page: nothing to do.
        });
    }

    refresh(&ui, &state.borrow(), now_since_epoch());
    ui
}

pub fn main() {
    let ui = build_ui();
    ui.run().unwrap();
}

// Android entry point. android-activity calls this after JNI init;
// `slint::android::init` hooks the activity into Slint's event loop.
// `set_disable_hover` mirrors the canonical Slint Material template —
// hover effects synthesised from touch events look wrong on a
// touchscreen, so we turn them off.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(android_app: slint::android::AndroidApp) {
    // Stash the AndroidApp before consuming it: slint::android::init
    // takes it by value, but the AppState transition callbacks need
    // it (cloned) later to fire the foreground-service start/stop
    // JNI calls. AndroidApp is Clone + Send + Sync, so this is sound.
    let _ = ANDROID_APP.set(android_app.clone());
    open_database(&android_app);
    slint::android::init(android_app).unwrap();
    let ui = build_ui();
    MaterialWindowAdapter::get(&ui).set_disable_hover(true);
    ui.run().unwrap();
}

// First slice of DB persistence: open (or create) the SQLite DB in
// the app's internal_data_path, store the handle in a OnceLock so
// later slices (session-finish write, stats query, crash-recovery
// finalize) can reach it. Mirrors the GTK shell's `Application::
// startup` pattern: a fixed `meditate.db` filename inside a
// `meditate/` subdirectory of the per-app data dir.
//
// internal_data_path is already per-app-private on Android
// (/data/data/<pkg>/files), so the `meditate/` nesting is purely
// for parity with the GTK layout — keeps export/import tooling
// simple if we ever need it. The handle is `Arc<Mutex<...>>`-able
// via meditate_core::Database if a future thread (sync worker)
// needs concurrent access; for now nothing else touches it.
// Arc<Mutex<Option<Database>>> mirrors the GTK shell's
// `application::imp::Application::db` field: rusqlite's Connection
// is !Sync (it caches prepared statements via RefCell), so the
// Mutex is mandatory for a process-wide handle. Option<> models the
// "open failed at startup, still alive without persistence" state
// that Phase 3's recovery surface will resolve.
#[cfg(target_os = "android")]
static DATABASE: std::sync::OnceLock<
    std::sync::Arc<std::sync::Mutex<Option<meditate_core::Database>>>,
> = std::sync::OnceLock::new();

#[cfg(target_os = "android")]
fn open_database(android_app: &slint::android::AndroidApp) {
    // The GTK shell's `Application::startup` mirrors this exactly,
    // just with `glib::user_data_dir()` standing in for
    // internal_data_path. Keeping the file layout identical
    // (<data>/meditate/{diagnostics.log,meditate.db}) means
    // future export/import tooling works across both shells.
    let Some(data_root) = android_app.internal_data_path() else {
        // No logger yet at this point, so eprintln + early return —
        // this branch only fires on a wholly broken android-activity
        // wiring, never in normal use.
        eprintln!("db.open FAILED: internal_data_path unavailable");
        return;
    };
    let dir = data_root.join("meditate");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("db.open FAILED creating {}: {e}", dir.display());
        return;
    }
    // Init the diag log first so the open-result line below has a
    // place to land. diag::init creates dir if missing (we already
    // did, but the call is idempotent) and installs a panic hook.
    meditate_core::diag::init(&dir);
    let db_path = dir.join("meditate.db");
    let opened = match meditate_core::Database::open(&db_path) {
        Ok(db) => {
            // Mirror the GTK shell's `Database::open` (in
            // `meditate-gtk/src/db/mod.rs`): seed non-audio rows
            // (default labels, presets, vibration patterns,
            // box-breath phases) on every open. Idempotent —
            // each seed gates on a `*_seeded` settings flag, so a
            // user-deleted row stays deleted. Bell-sound seeding
            // stays shell-side (Android bundles its own asset
            // paths in phase 5).
            if let Err(e) = db.seed_all_non_audio() {
                meditate_core::log(
                    "db.seed",
                    &format!("seed_all_non_audio FAILED err={e:?}"),
                );
            }
            meditate_core::log("db.open", &format!("ok path={}", db_path.display()));
            Some(db)
        }
        Err(e) => {
            // Phase 3 will add a Slint recovery surface mirroring the
            // GTK shell's recovery window. For Phase 1's open-only
            // slice the log line is the whole user-facing signal —
            // a failed open just means stats / persistence is
            // unavailable for this run; the timer still works.
            meditate_core::log(
                "db.open",
                &format!("FAILED path={} err={e:?}", db_path.display()),
            );
            None
        }
    };
    let _ = DATABASE.set(std::sync::Arc::new(std::sync::Mutex::new(opened)));
}
