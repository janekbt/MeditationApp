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

/// Hide the soft keyboard. Slint's `clear-focus()` on a
/// TextInput is supposed to dismiss the IME via the
/// `InputMethodRequest::Disable` path, but on this slint+android-
/// activity stack (1.16.1 / 0.6.1) tapping a non-input button
/// like Save / Cancel doesn't dispatch a focus-lost event for the
/// already-focused TextInput, so the IME stays parked. Going
/// through `AndroidApp::hide_soft_input` directly side-steps
/// Slint and asks Android to drop the IME unconditionally.
#[cfg(target_os = "android")]
fn hide_soft_keyboard() {
    if let Some(app) = ANDROID_APP.get() {
        app.hide_soft_input(false);
    }
}

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

/// Per-mode Stopwatch flag — flips `*Countdown` ↔ `*Stopwatch`
/// at session start. Same shape as the Keep-Awake / Cues
/// readers; persists via `stopwatch_key_for_mode` ("timer
/// _stopwatch_active" / "guided_stopwatch_active" /
/// "boxbreath_stopwatch_active"). Defaults to `false`.
#[cfg(target_os = "android")]
fn read_stopwatch_for_mode(mode: meditate_core::SessionMode) -> bool {
    let Some(db_arc) = DATABASE.get() else { return false; };
    let Ok(guard) = db_arc.lock() else { return false; };
    let Some(db) = guard.as_ref() else { return false; };
    let key = meditate_core::settings_keys::stopwatch_key_for_mode(mode);
    meditate_core::settings_keys::read_bool(db, key, false)
}

#[cfg(target_os = "android")]
fn write_stopwatch_for_mode(mode: meditate_core::SessionMode, value: bool) {
    let Some(db_arc) = DATABASE.get() else { return; };
    let Ok(guard) = db_arc.lock() else { return; };
    let Some(db) = guard.as_ref() else { return; };
    let key = meditate_core::settings_keys::stopwatch_key_for_mode(mode);
    if let Err(e) = db.set_setting(key, meditate_core::format_bool(value)) {
        meditate_core::log(
            "settings.stopwatch",
            &format!("write FAILED mode={mode:?} value={value} err={e:?}"),
        );
    }
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

/// Page size for the Log feed. Mirrors GTK's
/// `meditate-gtk/src/log/imp.rs::load_page::PAGE_SIZE`.
#[cfg(target_os = "android")]
const LOG_PAGE_SIZE: u32 = 15;

/// Fetch one page of sessions from the DB. Returns `(rows,
/// returned_full_page)`; the caller uses the boolean to decide
/// whether to show "Load more".
#[cfg(target_os = "android")]
fn load_log_page(
    offset: u32,
    notes_only: bool,
) -> (Vec<(i64, meditate_core::db::Session)>, bool) {
    let Some(db_arc) = DATABASE.get() else { return (Vec::new(), false); };
    let Ok(guard) = db_arc.lock() else { return (Vec::new(), false); };
    let Some(db) = guard.as_ref() else { return (Vec::new(), false); };
    let filter = meditate_core::db::SessionFilter {
        limit: Some(LOG_PAGE_SIZE),
        offset: Some(offset),
        only_with_notes: notes_only,
        ..meditate_core::db::SessionFilter::default()
    };
    let rows = meditate_core::db::query_sessions_from_db(db, &filter)
        .unwrap_or_default();
    let full = rows.len() == LOG_PAGE_SIZE as usize;
    (rows, full)
}

/// Group an already-ordered (newest-first) flat list of
/// sessions into day sections. Pure function — no DB / no
/// global state — so the pagination handler can call it on
/// every "Load more" press over the cumulative loaded list.
/// `hidden_ids` is the set of rowids currently being deleted
/// (the in-flight undo-toast window) — they're skipped during
/// grouping so section counts/totals stay consistent with the
/// rendered cards. Mirrors GTK's `set_visible(false)` on the
/// card widget — the row stays in `loaded_log_sessions` (so the
/// undo path can restore it) but doesn't show up in the
/// rendered feed until the snackbar dismisses without Undo.
#[cfg(target_os = "android")]
fn group_log_sessions(
    rows: &[(i64, meditate_core::db::Session)],
    label_name_by_id: &std::collections::HashMap<i64, String>,
    hidden_ids: &std::collections::HashSet<i64>,
) -> Vec<LogDaySectionData> {
    let mut sections: Vec<LogDaySectionData> = Vec::new();
    for (rowid, s) in rows {
        if hidden_ids.contains(rowid) { continue; }
        let date_key = s.start_iso.get(..10).unwrap_or("").to_string();
        let label_name = s
            .label_id
            .and_then(|id| label_name_by_id.get(&id).cloned())
            .unwrap_or_default();
        let color_index = if label_name.is_empty() {
            -1
        } else {
            meditate_core::format::label_color_class_index(&label_name) as i32
        };
        let item = LogCardItemData {
            id: *rowid as i32,
            minutes: meditate_core::format::log_card_minutes(
                s.duration_secs as i64,
            ) as i32,
            time_of_day: format_time_of_day(&s.start_iso),
            label_name,
            note: truncate_note_for_card(
                s.notes.as_deref().unwrap_or_default(),
            ),
            color_index,
        };
        let duration_secs_i64 = i64::from(s.duration_secs);
        if let Some(last) = sections.last_mut() {
            if last.date_key == date_key {
                last.count += 1;
                last.total_secs += duration_secs_i64;
                last.items.push(item);
                continue;
            }
        }
        sections.push(LogDaySectionData {
            date_key,
            date_display: format_date_group_display(&s.start_iso),
            count: 1,
            total_secs: duration_secs_i64,
            items: vec![item],
        });
    }
    sections
}

/// Load the label name lookup once per refresh. Cheap (a few
/// rows) and keeps the grouping fn free of DB access.
#[cfg(target_os = "android")]
fn load_label_name_map() -> std::collections::HashMap<i64, String> {
    let Some(db_arc) = DATABASE.get() else { return Default::default(); };
    let Ok(guard) = db_arc.lock() else { return Default::default(); };
    let Some(db) = guard.as_ref() else { return Default::default(); };
    meditate_core::db::list_labels_from_db(db)
        .unwrap_or_default()
        .into_iter()
        .map(|l| (l.id, l.name))
        .collect()
}

#[cfg(target_os = "android")]
struct LogCardItemData {
    id: i32,
    minutes: i32,
    time_of_day: String,
    label_name: String,
    note: String,
    color_index: i32,
}

#[cfg(target_os = "android")]
struct LogDaySectionData {
    date_key: String,
    date_display: String,
    count: i64,
    total_secs: i64,
    items: Vec<LogCardItemData>,
}

/// Truncate a session note to ~120 characters + "…" so a long
/// rant doesn't unbound-expand its Log card. Slint's `overflow:
/// elide` doesn't reliably clip multi-line wrapped Text against
/// a `max-height`, so we cap at the data level instead. GTK's
/// `lines: 2; ellipsize: end` solves the same problem at the
/// view layer; an Android-only deviation that yields the same
/// outcome.
#[cfg(target_os = "android")]
fn truncate_note_for_card(note: &str) -> String {
    const MAX_CHARS: usize = 120;
    let chars: Vec<char> = note.chars().collect();
    if chars.len() <= MAX_CHARS {
        return note.to_string();
    }
    let mut head: String = chars.into_iter().take(MAX_CHARS - 1).collect();
    head.push('…');
    head
}

/// Extract the time-of-day portion ("14:32") from a local-ISO
/// string ("2026-05-15T14:32:18+02:00"). Skips the date prefix
/// + TZ offset suffix; safe for malformed inputs (returns "").
#[cfg(target_os = "android")]
fn format_time_of_day(start_iso: &str) -> String {
    start_iso
        .get(11..16)
        .unwrap_or("")
        .to_string()
}

/// Date-section header label for the Log feed. Today /
/// Yesterday / weekday-or-full-date logic lives in core
/// eventually; for L-1 we just show the YYYY-MM-DD prefix.
#[cfg(target_os = "android")]
fn format_date_group_display(start_iso: &str) -> String {
    start_iso
        .get(..10)
        .unwrap_or("")
        .to_string()
}

/// Reload the Log feed from scratch: clears the shadow list,
/// fetches the first page, groups + pushes to UI. Called on
/// app launch, on every Save, and on nav-changed-into-Log.
#[cfg(target_os = "android")]
fn reset_log_feed(
    ui: &MainWindow,
    loaded: &std::rc::Rc<std::cell::RefCell<Vec<(i64, meditate_core::db::Session)>>>,
    pending: &std::rc::Rc<std::cell::RefCell<Vec<(i64, meditate_core::db::Session)>>>,
) {
    let (rows, full) = load_log_page(0, ui.get_filter_notes_only());
    *loaded.borrow_mut() = rows;
    ui.set_log_has_more(full);
    render_log_feed(ui, loaded, pending);
}

/// "Load more" — query the next page (offset = current loaded
/// count), append to the shadow list, re-group, push. Mirrors
/// GTK's `LogView::load_more` chain at `imp.rs:156-197`.
#[cfg(target_os = "android")]
fn extend_log_feed(
    ui: &MainWindow,
    loaded: &std::rc::Rc<std::cell::RefCell<Vec<(i64, meditate_core::db::Session)>>>,
    pending: &std::rc::Rc<std::cell::RefCell<Vec<(i64, meditate_core::db::Session)>>>,
) {
    let offset = loaded.borrow().len() as u32;
    let (rows, full) = load_log_page(offset, ui.get_filter_notes_only());
    if rows.is_empty() {
        // Defensive: button pressed but no more rows. Hide it.
        ui.set_log_has_more(false);
        return;
    }
    loaded.borrow_mut().extend(rows);
    ui.set_log_has_more(full);
    render_log_feed(ui, loaded, pending);
}

/// Drain `pending_deletes`, delete each row from the DB,
/// remove the matching rows from `loaded_log_sessions`, hide
/// the snackbar, and re-render. Mirrors GTK's
/// `commit_all_pending` at
/// `meditate-gtk/src/log/imp.rs:690`. Called from the 5 s
/// `delete_timer` callback (auto-commit). No-ops gracefully if
/// the DB lock can't be acquired — the rows stay queued and a
/// later trash-tap can re-arm the timer.
#[cfg(target_os = "android")]
fn commit_pending_deletes(
    ui: &MainWindow,
    loaded: &std::rc::Rc<std::cell::RefCell<Vec<(i64, meditate_core::db::Session)>>>,
    pending: &std::rc::Rc<std::cell::RefCell<Vec<(i64, meditate_core::db::Session)>>>,
) {
    let drained: Vec<(i64, meditate_core::db::Session)> =
        std::mem::take(&mut *pending.borrow_mut());
    if drained.is_empty() {
        ui.set_snackbar_visible(false);
        return;
    }
    if let Some(db_arc) = DATABASE.get() {
        if let Ok(guard) = db_arc.lock() {
            if let Some(db) = guard.as_ref() {
                for (id, _) in &drained {
                    if let Err(err) = db.delete_session(*id) {
                        meditate_core::log(
                            "log.delete.commit.failed",
                            &format!("rowid {id}: {err:?}"),
                        );
                    }
                }
            }
        }
    }
    // Drop the now-deleted rows from the in-memory shadow so
    // pagination offsets stay consistent on the next "Load
    // more".
    let drained_ids: std::collections::HashSet<i64> =
        drained.iter().map(|(id, _)| *id).collect();
    loaded.borrow_mut().retain(|(id, _)| !drained_ids.contains(id));
    ui.set_snackbar_visible(false);
    render_log_feed(ui, loaded, pending);
}

/// Re-render the Log feed from the current shadow state.
/// Hidden-ids = the rowids currently in `pending_deletes`
/// (in-flight undo window) — they're filtered out during
/// grouping so the cards visually disappear immediately on
/// delete-tap and reappear on Undo. `log-has-more` is left
/// untouched here; the page-fetching helpers (`reset_log_feed`
/// / `extend_log_feed`) own it because they're the only paths
/// that know whether the most recent page came back full.
#[cfg(target_os = "android")]
fn render_log_feed(
    ui: &MainWindow,
    loaded: &std::rc::Rc<std::cell::RefCell<Vec<(i64, meditate_core::db::Session)>>>,
    pending: &std::rc::Rc<std::cell::RefCell<Vec<(i64, meditate_core::db::Session)>>>,
) {
    let hidden: std::collections::HashSet<i64> = pending
        .borrow()
        .iter()
        .map(|(id, _)| *id)
        .collect();
    let label_map = load_label_name_map();
    let sections = group_log_sessions(&loaded.borrow(), &label_map, &hidden);
    push_log_sections_to_ui(ui, sections);
}

/// Push the cumulative loaded session list (already grouped)
/// into the Slint `log-sections` property. `log-has-more` is
/// set by the caller — see `reset_log_feed` / `extend_log_feed`,
/// which own the page-fetch full/non-full signal. Pure render
/// — does not touch the DB.
#[cfg(target_os = "android")]
fn push_log_sections_to_ui(
    ui: &MainWindow,
    sections: Vec<LogDaySectionData>,
) {
    let items: Vec<LogDaySection> = sections
        .into_iter()
        .map(|sec| {
            let cards: Vec<LogCardItem> = sec
                .items
                .into_iter()
                .map(|it| LogCardItem {
                    id: it.id,
                    minutes: it.minutes,
                    time_of_day: it.time_of_day.into(),
                    label_name: it.label_name.into(),
                    note: it.note.into(),
                    color_index: it.color_index,
                })
                .collect();
            LogDaySection {
                date_display: sec.date_display.into(),
                caption: format!(
                    "{} sessions · {} min",
                    sec.count,
                    sec.total_secs / 60,
                )
                .into(),
                items: std::rc::Rc::new(slint::VecModel::from(cards)).into(),
            }
        })
        .collect();
    ui.set_log_sections(std::rc::Rc::new(slint::VecModel::from(items)).into());
}

/// Write the crash-recovery snapshot row. Mirrors GTK's
/// `write_in_progress_snapshot` at `imp.rs:2536`: captures
/// (unix_start, accumulated_secs, mode, label_id) so a process
/// kill mid-session can be resurrected on the next launch via
/// `finalize_session_in_progress`. mode_payload is "{}" until
/// Box-Breath phase-progress capture lands (a v2 Resume feature,
/// not Phase 2 work).
#[cfg(target_os = "android")]
fn write_session_in_progress_snapshot(
    unix_start: i64,
    elapsed_secs: u32,
    mode: meditate_core::SessionMode,
    label_id: Option<i64>,
) {
    let Some(db_arc) = DATABASE.get() else { return; };
    let Ok(guard) = db_arc.lock() else { return; };
    let Some(db) = guard.as_ref() else { return; };
    let snapshot = meditate_core::db::SessionInProgress {
        start_iso: meditate_core::time::unix_to_local_iso(unix_start),
        accumulated_secs: elapsed_secs,
        mode,
        mode_payload: "{}".into(),
        label_id,
        guided_file_uuid: None,
    };
    if let Err(e) = db.set_session_in_progress(&snapshot) {
        meditate_core::log(
            "session.recovery",
            &format!("set snapshot FAILED err={e:?}"),
        );
    }
}

/// Start (or restart) the 60 s snapshot heartbeat. Mirrors GTK's
/// `start_snapshot_tick`: cancels any prior heartbeat then arms a
/// fresh `Repeated` Timer aligned to session start. Each tick
/// reads the live session's elapsed seconds and writes a
/// `SessionInProgress` row capturing (start, accumulated_secs,
/// mode, label_id).
#[cfg(target_os = "android")]
fn start_snapshot_heartbeat(
    timer: &'static slint::Timer,
    state: std::rc::Rc<std::cell::RefCell<AppState>>,
    current_mode: std::rc::Rc<std::cell::Cell<TimerMode>>,
    session_start_unix: std::rc::Rc<std::cell::Cell<Option<i64>>>,
) {
    timer.stop();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_secs(60),
        move || {
            let now = now_since_epoch();
            let s = state.borrow();
            let AppState::Active(session) = &*s else { return; };
            let Some(unix_start) = session_start_unix.get() else { return; };
            let elapsed = session.elapsed(now).as_secs() as u32;
            let mode: meditate_core::SessionMode = current_mode.get().into();
            let label_id = if read_label_active_for_mode(mode) {
                resolved_label_for_mode(mode).map(|(_, id)| id)
            } else {
                None
            };
            write_session_in_progress_snapshot(unix_start, elapsed, mode, label_id);
        },
    );
}

/// Drop the crash-recovery snapshot. Called on every transition
/// out of Active so a normal Stop / auto-finish leaves no row for
/// the next launch to "recover" (which would double-count). DB
/// errors are logged but swallowed — at worst the next launch
/// auto-finalizes a stale row, which is harmless without the
/// pending_done flow (we ignore it and move on).
#[cfg(target_os = "android")]
fn clear_session_in_progress_snapshot() {
    let Some(db_arc) = DATABASE.get() else { return; };
    let Ok(guard) = db_arc.lock() else { return; };
    let Some(db) = guard.as_ref() else { return; };
    if let Err(e) = db.clear_session_in_progress() {
        meditate_core::log(
            "session.recovery",
            &format!("clear snapshot FAILED err={e:?}"),
        );
    }
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

/// First label in the user's list, in DB-sort order. Used by
/// the Edit-Session label-expander master-switch toggle (L-4d)
/// to adopt a sensible default when flipping the switch on
/// without any existing selection. Mirrors GTK's
/// `labels_for_toggle.first()` at `log/imp.rs:897`.
#[cfg(target_os = "android")]
fn first_label() -> Option<(i64, String)> {
    let db_arc = DATABASE.get()?;
    let guard = db_arc.lock().ok()?;
    let db = guard.as_ref()?;
    meditate_core::db::list_labels_from_db(db)
        .ok()?
        .into_iter()
        .next()
        .map(|l| (l.id, l.name))
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

/// Wire the six callbacks Material's `DatePickerAdapter` global
/// expects. All implemented in terms of chrono::NaiveDate so the
/// picker renders the same calendar grid GTK gives us (Sunday-
/// first headers, locale-aware month names via strftime).
#[cfg(target_os = "android")]
fn wire_date_picker_adapter(ui: &MainWindow) {
    use chrono::{Datelike, Local, NaiveDate};
    use slint::ComponentHandle;

    let adapter = ui.global::<DatePickerAdapter>();

    adapter.on_month_day_count(|month, year| {
        // Last day of month = (1st of next month) − 1 day.
        let (ny, nm) = if month >= 12 {
            (year + 1, 1)
        } else {
            (year, month + 1)
        };
        let first_next = NaiveDate::from_ymd_opt(ny, nm as u32, 1);
        let first_this = NaiveDate::from_ymd_opt(year, month as u32, 1);
        match (first_this, first_next) {
            (Some(a), Some(b)) => b.signed_duration_since(a).num_days() as i32,
            // Defensive: any out-of-range input falls back to 30
            // so the calendar grid still renders.
            _ => 30,
        }
    });

    adapter.on_month_offset(|month, year| {
        // Day-of-week of the 1st of the month, Sunday-indexed
        // (0=Sun..6=Sat) to match the header row Material's
        // calendar renders.
        NaiveDate::from_ymd_opt(year, month as u32, 1)
            .map(|d| d.weekday().num_days_from_sunday() as i32)
            .unwrap_or(0)
    });

    adapter.on_format_date(|format, day, month, year| {
        NaiveDate::from_ymd_opt(year, month as u32, day as u32)
            .map(|d| d.format(format.as_str()).to_string())
            .unwrap_or_default()
            .into()
    });

    adapter.on_parse_date(|date, format| {
        // Returns [day, month, year] on success, empty on parse
        // failure. Material's input handler reads `.length == 3`
        // as "valid parse" so an empty Vec signals invalid.
        let parsed = NaiveDate::parse_from_str(date.as_str(), format.as_str())
            .ok()
            .map(|d| vec![d.day() as i32, d.month() as i32, d.year()])
            .unwrap_or_default();
        std::rc::Rc::new(slint::VecModel::from(parsed)).into()
    });

    adapter.on_valid_date(|date, format| {
        NaiveDate::parse_from_str(date.as_str(), format.as_str()).is_ok()
    });

    adapter.on_date_now(|| {
        let today = Local::now().date_naive();
        std::rc::Rc::new(slint::VecModel::from(vec![
            today.day() as i32,
            today.month() as i32,
            today.year(),
        ]))
        .into()
    });
}

fn build_ui() -> MainWindow {
    let ui = MainWindow::new().unwrap();

    // Wire Material's DatePickerAdapter globals (L-4c). The
    // calendar widget calls these six pure callbacks to compute
    // month-day-counts, render headers, and parse / format the
    // text input. All six are stateless arithmetic over
    // `chrono::NaiveDate`; the picker treats them as `pure` and
    // can re-invoke arbitrarily.
    #[cfg(target_os = "android")]
    wire_date_picker_adapter(&ui);

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

    // Cumulative flat list of sessions loaded into the Log feed.
    // The "Load more" button extends this; each page-load
    // re-groups the whole list into `LogDaySection`s and pushes
    // them to Slint. Mirrors GTK's `loaded_count` cell, just
    // shaped as a Vec for direct iteration. Box::leak isn't
    // strictly required (Rc<RefCell> would work) but keeping the
    // pattern consistent with `bb_target_secs` etc.
    #[cfg(target_os = "android")]
    let loaded_log_sessions: Rc<RefCell<Vec<(i64, meditate_core::db::Session)>>>
        = Rc::new(RefCell::new(Vec::new()));

    // In-flight delete batch — the rows the user has tapped trash
    // on but where the undo window hasn't yet expired. Mirrors
    // GTK's `pending_deletes` at `meditate-gtk/src/log/imp.rs:49`.
    // The 5-second timer below is the commit gate; until it fires
    // (or the user taps Undo) the rows stay in the DB and the
    // cards stay hidden from the rendered feed via the hidden-ids
    // filter in `group_log_sessions`.
    #[cfg(target_os = "android")]
    let pending_deletes: Rc<RefCell<Vec<(i64, meditate_core::db::Session)>>>
        = Rc::new(RefCell::new(Vec::new()));

    // 5-second auto-commit timer. Restarted on every trash-tap so
    // a burst of deletes coalesces into a single snackbar — the
    // GTK shell does the same coalescing via `dismiss()` +
    // `add_toast()` swap. `Box::leak` to keep the handle alive
    // across the whole window lifetime; we only ever call
    // `start()` / `stop()` on it.
    #[cfg(target_os = "android")]
    let delete_timer: &'static slint::Timer =
        Box::leak(Box::new(slint::Timer::default()));

    // Session being edited via the Log card → Edit-Session
    // overlay (L-4). Holds the rowid between `card-tap`
    // (populates the dialog) and `edit-save-tap` (reads the
    // dialog's `edit-note-text`, builds a Session with that
    // single field swapped, and writes it back). None whenever
    // the overlay is hidden. Mirrors GTK's `session_id` capture
    // inside `show_session_dialog` at
    // `meditate-gtk/src/log/imp.rs:765`.
    #[cfg(target_os = "android")]
    let editing_session_id: Rc<Cell<Option<i64>>> = Rc::new(Cell::new(None));

    // Crash-recovery snapshot timer handle. Heartbeat is started
    // on the Idle/Finished → Active transition in `on_action_tap`
    // and cancelled on every transition out of Active, mirroring
    // GTK's `start_snapshot_tick` + `cancel_snapshot_tick` at
    // `imp.rs:2600` / `imp.rs:2621`. `Box::leak` so the Timer
    // outlives `build_ui`'s frame; we never drop it, only
    // start/stop it.
    #[cfg(target_os = "android")]
    let snapshot_timer: &'static slint::Timer =
        Box::leak(Box::new(slint::Timer::default()));
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
        let current_mode = current_mode.clone();
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
                // Kick the snapshot heartbeat — fires every 60 s
                // of session-elapsed (since GTK's
                // `start_snapshot_tick` is also called from
                // session start). Cancelled on the
                // Active-out edges in stop_tap + tick.
                start_snapshot_heartbeat(
                    snapshot_timer,
                    state.clone(),
                    current_mode.clone(),
                    session_start_unix.clone(),
                );
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
        #[cfg(target_os = "android")]
        let snapshot_timer_ref: &'static slint::Timer = snapshot_timer;
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
                // Drop the recovery snapshot — the session ended
                // cleanly via user Stop. Done-screen Save / Discard
                // will decide whether it becomes a persisted row;
                // either way the next launch shouldn't recover.
                // Cancel the heartbeat so it doesn't re-write a
                // ghost row after we just cleared.
                #[cfg(target_os = "android")]
                {
                    snapshot_timer_ref.stop();
                    clear_session_in_progress_snapshot();
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
        #[cfg(target_os = "android")]
        let snapshot_timer_ref: &'static slint::Timer = snapshot_timer;
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
                // Cancel heartbeat + drop snapshot — see stop_tap.
                snapshot_timer_ref.stop();
                clear_session_in_progress_snapshot();
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
        #[cfg(target_os = "android")]
        let loaded_log_sessions = loaded_log_sessions.clone();
        #[cfg(target_os = "android")]
        let pending_deletes = pending_deletes.clone();
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
                // Push the freshly-inserted row into the Log feed
                // so a quick nav-to-Log shows it without restart.
                reset_log_feed(&ui, &loaded_log_sessions, &pending_deletes);
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
                ui.set_stopwatch_on(read_stopwatch_for_mode(core_mode));
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

    // Stopwatch Mode toggle — per-mode persistence via
    // `stopwatch_key_for_mode`. Toggling on flips the active
    // mode's session-shape choice to `*Stopwatch` at next
    // session start; the row visibility is identical across
    // modes (mirrors GTK's unconditional `set_visible(true)`).
    {
        let current_mode = current_mode.clone();
        ui.on_stopwatch_toggled(move |value| {
            #[cfg(target_os = "android")]
            write_stopwatch_for_mode(current_mode.get().into(), value);
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
                match ui.get_chooser_target() {
                    1 => {
                        // Done flow
                        if let Some(name) = lookup_label_name(id as i64) {
                            ui.set_done_label_id(id);
                            ui.set_done_label_name(name.into());
                            ui.set_done_label_active(true);
                        }
                    }
                    2 => {
                        // Edit-Session flow (L-4d) — write back to
                        // edit-label-* so the overlay re-appears
                        // showing the new selection.
                        if let Some(name) = lookup_label_name(id as i64) {
                            ui.set_edit_label_id(id);
                            ui.set_edit_label_name(name.into());
                            ui.set_edit_label_enabled(true);
                        }
                    }
                    _ => {
                        // Setup flow (existing behavior)
                        let mode: meditate_core::SessionMode = current_mode.get().into();
                        if let Some(uuid) = lookup_label_uuid(id as i64) {
                            write_label_uuid_for_mode(mode, &uuid);
                        }
                        refresh_setup_label_name(&ui, mode);
                    }
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
        ui.set_stopwatch_on(read_stopwatch_for_mode(core_mode));
        ui.set_label_active(read_label_active_for_mode(core_mode));
        ui.set_label_name(
            resolved_label_for_mode(core_mode)
                .map(|(name, _)| name)
                .unwrap_or_default()
                .into(),
        );
        refresh_breathing_tiles(&ui, read_breathing_pattern());
        reset_log_feed(&ui, &loaded_log_sessions, &pending_deletes);
    }

    // "Load more" tap on the Log feed — fetch the next page,
    // append + re-render. Mirrors GTK's `LogView::load_more`
    // at `meditate-gtk/src/log/imp.rs:156`.
    {
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let loaded_log_sessions = loaded_log_sessions.clone();
        #[cfg(target_os = "android")]
        let pending_deletes = pending_deletes.clone();
        ui.on_load_more_tap(move || {
            #[cfg(target_os = "android")]
            if let Some(ui) = weak.upgrade() {
                extend_log_feed(&ui, &loaded_log_sessions, &pending_deletes);
            }
            let _ = weak.clone();
        });
    }

    // Trash IconButton on a Log card was tapped — push the row
    // into `pending_deletes`, re-render the feed with that id
    // filtered out (optimistic hide), show / refresh the undo
    // snackbar, and (re)arm the 5-second auto-commit timer.
    // Mirrors GTK's `on_delete_clicked` at
    // `meditate-gtk/src/log/imp.rs:623`. The deviation is purely
    // mechanical — Slint timer vs. adw::Toast's built-in timer,
    // shadow-filter rebuild vs. `card.set_visible(false)` — the
    // user-facing flow is identical (tap → row vanishes → 5 s
    // window with "N sessions deleted · Undo" → dismiss commits,
    // Undo restores).
    {
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let loaded_log_sessions = loaded_log_sessions.clone();
        #[cfg(target_os = "android")]
        let pending_deletes = pending_deletes.clone();
        ui.on_delete_tap(move |rowid| {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                let id = rowid as i64;
                // Move row from `loaded_log_sessions` view into
                // `pending_deletes`. We don't actually remove
                // from `loaded` — Undo needs to find the Session
                // to restore. The hidden-ids filter in
                // `group_log_sessions` is what makes it disappear
                // visually.
                let session = loaded_log_sessions
                    .borrow()
                    .iter()
                    .find(|(id_, _)| *id_ == id)
                    .map(|(_, s)| s.clone());
                let Some(session) = session else { return; };
                pending_deletes.borrow_mut().push((id, session));

                // Snackbar text — uses the same string keys as
                // GTK's announcement renderer
                // (`meditate-gtk/src/announcement.rs:23`). i18n
                // isn't wired up on Android yet; once it is,
                // route through gettext like the GTK shell does.
                let count = pending_deletes.borrow().len();
                let text = if count == 1 {
                    "Session deleted".to_string()
                } else {
                    format!("{count} sessions deleted")
                };
                ui.set_snackbar_text(text.into());
                ui.set_snackbar_visible(true);

                render_log_feed(&ui, &loaded_log_sessions, &pending_deletes);

                // (Re)arm the commit timer — restart on every
                // tap so a burst of deletes coalesces into one
                // 5-second window.
                let weak_inner = ui.as_weak();
                let loaded_inner = loaded_log_sessions.clone();
                let pending_inner = pending_deletes.clone();
                delete_timer.start(
                    slint::TimerMode::SingleShot,
                    std::time::Duration::from_secs(5),
                    move || {
                        let Some(ui) = weak_inner.upgrade() else { return; };
                        commit_pending_deletes(&ui, &loaded_inner, &pending_inner);
                    },
                );
            }
            let _ = (weak.clone(), rowid);
        });
    }

    // Undo button on the snackbar — restore every hidden card
    // (clear `pending_deletes`), hide the snackbar, cancel the
    // commit timer, re-render. Mirrors GTK's
    // `new_toast.connect_button_clicked` block at
    // `meditate-gtk/src/log/imp.rs:649`.
    {
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let loaded_log_sessions = loaded_log_sessions.clone();
        #[cfg(target_os = "android")]
        let pending_deletes = pending_deletes.clone();
        ui.on_snackbar_undo_tap(move || {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                delete_timer.stop();
                pending_deletes.borrow_mut().clear();
                ui.set_snackbar_visible(false);
                render_log_feed(&ui, &loaded_log_sessions, &pending_deletes);
            }
            let _ = weak.clone();
        });
    }

    // Card tap on the Log feed → open the Edit-Session overlay
    // pre-filled with the tapped session's data. L-4a only wires
    // the Note field; future slices (L-4b/c/d) will populate
    // duration / start time / label here too. Mirrors GTK's
    // `show_edit_dialog` at `meditate-gtk/src/log/imp.rs:754`.
    {
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let loaded_log_sessions = loaded_log_sessions.clone();
        #[cfg(target_os = "android")]
        let editing_session_id = editing_session_id.clone();
        ui.on_card_tap(move |rowid| {
            #[cfg(target_os = "android")]
            {
                use chrono::{Datelike, Local, TimeZone, Timelike};
                let Some(ui) = weak.upgrade() else { return; };
                let id = rowid as i64;
                let session = loaded_log_sessions
                    .borrow()
                    .iter()
                    .find(|(id_, _)| *id_ == id)
                    .map(|(_, s)| s.clone());
                let Some(session) = session else { return; };
                editing_session_id.set(Some(id));
                ui.set_edit_note_text(
                    session.notes.clone().unwrap_or_default().into(),
                );
                let total = session.duration_secs as i64;
                ui.set_edit_duration_hours((total / 3600) as i32);
                ui.set_edit_duration_minutes(((total % 3600) / 60) as i32);
                // Decompose the session's unix start into a local
                // Date + Time so the Material pickers seed with
                // the user's wall-clock view of the session start.
                let dt = Local
                    .timestamp_opt(session.start_unix(), 0)
                    .single()
                    .unwrap_or_else(|| Local::now());
                ui.set_edit_start_date(Date {
                    year: dt.year(),
                    month: dt.month() as i32,
                    day: dt.day() as i32,
                });
                ui.set_edit_start_time(Time {
                    hour: dt.hour() as i32,
                    minute: dt.minute() as i32,
                    second: dt.second() as i32,
                });
                // Pre-fill label state (L-4d). `label_id` of None
                // = expander off; Some(id) = expander on, with
                // the row showing the resolved name. Mirrors
                // GTK's initial-state read at `log/imp.rs:856`.
                let (lbl_enabled, lbl_id, lbl_name) = match session.label_id {
                    Some(id) => (
                        true,
                        id as i32,
                        lookup_label_name(id).unwrap_or_default(),
                    ),
                    None => (false, 0, String::new()),
                };
                ui.set_edit_label_enabled(lbl_enabled);
                ui.set_edit_label_id(lbl_id);
                ui.set_edit_label_name(lbl_name.into());
                ui.set_edit_session_page(true);
            }
            let _ = (weak.clone(), rowid);
        });
    }

    // Cancel button on the Edit-Session overlay → discard edits,
    // close the overlay. Mirrors GTK's
    // `cancel_btn.connect_clicked` at `log/imp.rs:1050`.
    {
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let editing_session_id = editing_session_id.clone();
        ui.on_edit_cancel_tap(move || {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                hide_soft_keyboard();
                editing_session_id.set(None);
                ui.set_edit_session_page(false);
            }
            let _ = weak.clone();
        });
    }

    // Save button on the Edit-Session overlay → write the edited
    // Session row back to the DB and refresh the feed. L-4a only
    // mutates the `notes` column; all other fields are pulled
    // from the live shadow copy so they're preserved verbatim.
    // Mirrors GTK's `save_btn.connect_clicked` save path at
    // `log/imp.rs:1058`.
    {
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let loaded_log_sessions = loaded_log_sessions.clone();
        #[cfg(target_os = "android")]
        let pending_deletes = pending_deletes.clone();
        #[cfg(target_os = "android")]
        let editing_session_id = editing_session_id.clone();
        ui.on_edit_save_tap(move || {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                hide_soft_keyboard();
                let Some(id) = editing_session_id.get() else {
                    ui.set_edit_session_page(false);
                    return;
                };
                let new_note_raw = ui.get_edit_note_text().to_string();
                let new_note = if new_note_raw.is_empty() {
                    None
                } else {
                    Some(new_note_raw)
                };
                let original = loaded_log_sessions
                    .borrow()
                    .iter()
                    .find(|(id_, _)| *id_ == id)
                    .map(|(_, s)| s.clone());
                let Some(mut session) = original else {
                    ui.set_edit_session_page(false);
                    return;
                };
                session.notes = new_note;
                // Duration: recompose from the two SpinRows.
                // GTK clamps with `.max(0)` (`log/imp.rs:1093`);
                // the SpinRow min-value guards already keep both
                // factors non-negative, but mirror the clamp on
                // the product to stay defensive against future
                // signed-typed inputs.
                let hours = ui.get_edit_duration_hours().max(0) as i64;
                let mins = ui.get_edit_duration_minutes().max(0) as i64;
                session.duration_secs = (hours * 3600 + mins * 60).max(0) as u32;
                // Recompose start_time from the picker outputs.
                // Falls back to the original `start_unix` if the
                // user-edited Date / Time can't be turned into a
                // valid Local moment (e.g., a date inside a DST
                // gap). Mirrors GTK's `glib::DateTime::new(...)
                // .map_or_else(unix_now, |d| d.to_unix())` at
                // `log/imp.rs:1072`.
                use chrono::{Local, TimeZone};
                let d = ui.get_edit_start_date();
                let t = ui.get_edit_start_time();
                let new_start_unix = Local
                    .with_ymd_and_hms(
                        d.year,
                        d.month as u32,
                        d.day as u32,
                        t.hour as u32,
                        t.minute as u32,
                        t.second as u32,
                    )
                    .single()
                    .map(|dt| dt.timestamp())
                    .unwrap_or_else(|| session.start_unix());
                session.start_iso =
                    meditate_core::time::unix_to_local_iso(new_start_unix);
                // Label (L-4d). Mirrors GTK's
                // `label_expander.enables_expansion() ?
                // selected_label_id : None` branch at
                // `log/imp.rs:1079`.
                session.label_id = if ui.get_edit_label_enabled() {
                    let id = ui.get_edit_label_id() as i64;
                    if id > 0 { Some(id) } else { None }
                } else {
                    None
                };
                if let Some(db_arc) = DATABASE.get() {
                    if let Ok(guard) = db_arc.lock() {
                        if let Some(db) = guard.as_ref() {
                            if let Err(err) = db.update_session(id, &session) {
                                meditate_core::log(
                                    "log.edit.save.failed",
                                    &format!("rowid {id}: {err:?}"),
                                );
                            }
                        }
                    }
                }
                editing_session_id.set(None);
                ui.set_edit_session_page(false);
                reset_log_feed(&ui, &loaded_log_sessions, &pending_deletes);
            }
            let _ = weak.clone();
        });
    }

    // Tap on the "Selected" row inside the Edit-Session Label
    // group → push the labels chooser overlay with
    // `chooser-target = 2` so `on_label_picked` writes the pick
    // back to `edit-label-*` rather than the Setup-mode label.
    // Mirrors GTK's chooser-row activation at `log/imp.rs:1031`.
    {
        let weak = ui.as_weak();
        ui.on_edit_label_row_tap(move || {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                ui.set_chooser_target(2);
                let current = ui.get_edit_label_id() as i64;
                refresh_chooser_items(
                    &ui,
                    if current > 0 { Some(current) } else { None },
                );
                ui.set_labels_page(true);
            }
            let _ = weak.clone();
        });
    }

    // Master-switch toggle on the Edit-Session Label expander.
    // Mirrors GTK's `connect_enable_expansion_notify` at
    // `log/imp.rs:890`: when flipped on with no selection yet,
    // adopt the first available label so subsequent reads
    // resolve cleanly. When flipped off, the expander row hides
    // (handled Slint-side via `if root.edit-label-enabled`) and
    // Save will write `label_id = None`.
    {
        let weak = ui.as_weak();
        ui.on_edit_label_toggled(move |on| {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                if on && ui.get_edit_label_id() == 0 {
                    if let Some((id, name)) = first_label() {
                        ui.set_edit_label_id(id as i32);
                        ui.set_edit_label_name(name.into());
                    }
                }
            }
            let _ = (weak.clone(), on);
        });
    }

    // Filter funnel tap on the Log AppBar — open the filter
    // sheet. L-5a only flips the surface; L-5b/c will refresh
    // the label list and seed the switch state from the
    // current filter values before showing.
    {
        let weak = ui.as_weak();
        ui.on_log_filter_tap(move || {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                ui.set_filter_sheet_open(true);
            }
            let _ = weak.clone();
        });
    }

    // Has-Notes filter toggle (L-5b). Instant apply: update
    // `filter-has-active`, reload the feed from page 0 with the
    // new filter, then close the sheet — mirrors GTK's
    // `filter_notes_row` notify handler at
    // `meditate-gtk/src/window/imp.rs:783` (set filter →
    // refresh → popdown). The paginator reads the live
    // `filter-notes-only` property, so just calling
    // `reset_log_feed` after the toggle picks up the new value.
    {
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let loaded_log_sessions = loaded_log_sessions.clone();
        #[cfg(target_os = "android")]
        let pending_deletes = pending_deletes.clone();
        ui.on_filter_notes_toggled(move |_on| {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                ui.set_filter_has_active(ui.get_filter_notes_only());
                reset_log_feed(&ui, &loaded_log_sessions, &pending_deletes);
                ui.set_filter_sheet_open(false);
            }
            let _ = (weak.clone(), _on);
        });
    }

    // Preferences gear tap — no-op for now; Phase 7 hooks it up
    // to the eventual Preferences screen.
    ui.on_preferences_tap(move || {
        #[cfg(target_os = "android")]
        meditate_core::log(
            "ui.preferences_tap",
            "preferences screen pending (phase 7)",
        );
    });

    // Bottom NavigationBar selection changed — Rust-side
    // refresh of the page-specific state (just Log for now).
    {
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let loaded_log_sessions = loaded_log_sessions.clone();
        #[cfg(target_os = "android")]
        let pending_deletes = pending_deletes.clone();
        ui.on_nav_changed(move |idx| {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                if idx == 1 {
                    // Entering Log — reload from DB so a session
                    // saved before opening the tab shows up.
                    reset_log_feed(&ui, &loaded_log_sessions, &pending_deletes);
                }
            }
            let _ = (weak.clone(), idx);
        });
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
    //   * On the Log page → switch back to Timer
    //   * Otherwise: do nothing (the OS will close the app at the
    //     next press if no Slint handler accepted — or the user
    //     can swipe-up to Home).
    {
        let weak = ui.as_weak();
        let state = state.clone();
        #[cfg(target_os = "android")]
        let pending_done = pending_done.clone();
        #[cfg(target_os = "android")]
        let editing_session_id = editing_session_id.clone();
        ui.on_back_pressed(move || {
            let Some(ui) = weak.upgrade() else { return; };
            #[cfg(target_os = "android")]
            if ui.get_filter_sheet_open() {
                ui.set_filter_sheet_open(false);
                return;
            }
            #[cfg(target_os = "android")]
            if ui.get_edit_session_page() {
                hide_soft_keyboard();
                editing_session_id.set(None);
                ui.set_edit_session_page(false);
                return;
            }
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
            if ui.get_nav_page() == 1 {
                // Log page → back navigates to Timer (canonical
                // bottom-nav back behaviour on Android).
                ui.set_nav_page(0);
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
            // Crash-recovery finalize: if the previous run was
            // killed mid-session (kernel OOM, battery death,
            // panic), the `session_in_progress` row carries the
            // last snapshot. `finalize_session_in_progress` turns
            // it into a `sessions` row + clears the snapshot,
            // both inside one transaction. Mirrors GTK's
            // `Application::startup` recovery call. No Undo
            // toast yet (Snackbar surface is Phase 3 UI work);
            // the diag line is the user-visible signal.
            match db.finalize_session_in_progress() {
                Ok(Some(finalized)) => meditate_core::log(
                    "session.recovery",
                    &format!(
                        "finalized uuid={} duration_secs={}",
                        finalized.session_uuid, finalized.duration_secs,
                    ),
                ),
                Ok(None) => {} // Clean shutdown last run.
                Err(e) => meditate_core::log(
                    "session.recovery",
                    &format!("finalize FAILED at startup err={e:?}"),
                ),
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
