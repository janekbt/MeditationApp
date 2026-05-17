pub mod app;
#[cfg(target_os = "android")]
mod haptics;
#[cfg(target_os = "android")]
mod service;
#[cfg(target_os = "android")]
mod sounds;
#[cfg(target_os = "android")]
mod audio;
// android: the live JNI/fs widget bridge. test: the host
// `cargo test --workspace` compiles + runs the pure
// `build_projection_json` unit tests (strict-TDD). A plain host
// build uses neither, so the module is absent there — keeps it
// off the host dead-code path without an `#[allow]`.
#[cfg(any(target_os = "android", test))]
mod widget;

slint::include_modules!();

use app::{AppState, TimerMode};
#[cfg(target_os = "android")]
use app::{signal_mode_from_chip_index, signal_mode_to_chip_index};
use std::cell::Cell;
use std::cell::RefCell;
use std::rc::Rc;
// `RECOVERED_SESSION` is the only remaining bare `OnceLock` user
// (android-gated); `DATABASE` spells out `std::sync::OnceLock`.
// So on the host build this import is dead — gate it rather
// than carry an unused-import warning.
#[cfg(target_os = "android")]
use std::sync::OnceLock;
use std::time::Duration;

// Process-wide handle to the AndroidApp, reachable from the
// AppState transition closures (which don't own it) for the JNI
// bridges.
//
// NOT a write-once OnceLock: android-activity calls `android_main`
// *again* every time the NativeActivity is destroyed and remade
// while the process survives (USB attach/detach, low-memory
// activity kill, back-then-relaunch, a config change we don't
// list in `android:configChanges`). A OnceLock would pin the
// FIRST AndroidApp; after a recreate its `internalDataPath` /
// activity pointer is null, so `internal_data_path()` returns
// None ("after NativeActivity has been destroyed") and every JNI
// call targets a dead activity — the intermittent
// `[widget] publish: no internal_data_path` and silently broken
// service/audio/haptics. So the handle must be *refreshed* each
// android_main.
//
// AndroidApp is `Clone + Send + Sync` and cheap (Arc inside). We
// `Box::leak` the latest clone and publish the `&'static`
// pointer through an `AtomicPtr`, so `android_app()` still hands
// out `Option<&'static AndroidApp>` and the ~16 call sites stay
// borrow-identical. The leak is bounded — one Arc-sized handle
// per activity-recreate, a handful over a process lifetime — and
// deliberate (the old handle's activity is dead anyway); it is
// not a leak we could reclaim safely while `&'static` refs may
// still be in flight on other threads.
#[cfg(target_os = "android")]
static ANDROID_APP: std::sync::atomic::AtomicPtr<slint::android::AndroidApp> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

/// Publish (or replace) the process-wide AndroidApp handle.
/// Called at the top of every `android_main`.
#[cfg(target_os = "android")]
fn set_android_app(app: slint::android::AndroidApp) {
    let leaked: &'static slint::android::AndroidApp =
        Box::leak(Box::new(app));
    ANDROID_APP.store(
        leaked as *const _ as *mut _,
        std::sync::atomic::Ordering::Release,
    );
}

/// Current AndroidApp handle, or None before the first
/// `android_main`. The `&'static` is sound because the backing
/// box is intentionally leaked (never freed).
#[cfg(target_os = "android")]
fn android_app() -> Option<&'static slint::android::AndroidApp> {
    let p = ANDROID_APP.load(std::sync::atomic::Ordering::Acquire);
    if p.is_null() {
        None
    } else {
        Some(unsafe { &*p })
    }
}

/// Single-shot hand-off for a crash-recovered session (L-6).
/// `open_database` runs `finalize_session_in_progress` at
/// `android_main` entry — before `build_ui` exists — so a
/// rescued `(session_uuid, duration_secs)` is parked here and
/// drained once by `build_ui` to raise the recovery Undo
/// snackbar. Mirrors GTK's `pending_recovery_toast` stash at
/// `meditate-gtk/src/application.rs:374`.
#[cfg(target_os = "android")]
static RECOVERED_SESSION: OnceLock<std::sync::Mutex<Option<(String, u32)>>> =
    OnceLock::new();


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
    if let Some(app) = android_app() {
        app.hide_soft_input(false);
    }
}

/// React to an AppState transition's service side. If the session
/// just started (Idle/Finished → Active), kick the foreground
/// service. If it just ended (Active → Idle/Finished), tear it
/// down. Bell / vibration cues are NOT handled here — they flow
/// through `dispatch_effects` off the core `Session` effects, so
/// the "ring on natural completion, stay silent on Stop" decision
/// lives in core (mirrors GTK). No-op on host builds.
fn on_state_changed(was_active: bool, is_active: bool) {
    #[cfg(target_os = "android")]
    {
        if !was_active && is_active {
            if let Some(app) = android_app() {
                service::start(app);
            }
        } else if was_active && !is_active {
            if let Some(app) = android_app() {
                service::stop(app);
            }
        }
    }
    // Touched the args so the host build doesn't complain about
    // unused parameters under cfg-disabled code.
    let _ = (was_active, is_active);
}

/// Run portable core `Session` effects through the Android native
/// layer — the direct analogue of GTK's
/// `dispatch_session_effects` / `dispatch_fire_route`.
///
/// `StopActiveSignals` cuts any in-flight sound + vibration. Each
/// `Fire*` effect resolves via `Effect::fire_route()` to an
/// already-effective `signal_mode` (Session has AND'd per-cue
/// with the per-mode override), so the shell just plays the sound
/// when it `includes_sound()` and the vibration pattern when it
/// `includes_vibration()` — no extra gating. Every other effect
/// (UpdateDisplay / EndSession / EnterOvertime / …) is consumed
/// at its tick callsite, not here.
///
/// This is why Stop is silent but a natural countdown finish
/// rings: `stop` emits only `StopActiveSignals`, while `tick`
/// emits `FireEndBell` at the zero-crossing.
#[cfg(target_os = "android")]
fn dispatch_effects(effects: &[meditate_core::session::Effect]) {
    use meditate_core::session::Effect;
    let Some(app) = android_app() else { return; };
    for effect in effects {
        if matches!(effect, Effect::StopActiveSignals) {
            audio::stop(app);
            haptics::cancel(app);
        }
        let Some(route) = effect.fire_route() else { continue; };
        if route.signal_mode.includes_sound() {
            let path = bell_sound_path(route.sound_uuid);
            audio::stop(app);
            audio::play(app, &path);
        }
        if route.signal_mode.includes_vibration() {
            if let Some(db_arc) = DATABASE.get() {
                if let Ok(guard) = db_arc.lock() {
                    if let Some(db) = guard.as_ref() {
                        if let Ok(Some(p)) =
                            meditate_core::db::find_vibration_pattern_by_uuid_from_db(
                                db,
                                route.vibration_pattern_uuid,
                            )
                        {
                            let env =
                                meditate_core::vibration::build_master_envelope(&p);
                            haptics::cancel(app);
                            haptics::vibrate_waveform(app, &env);
                        }
                    }
                }
            }
        }
        meditate_core::log(
            route.log_tag,
            &format!(
                "signal_mode={} channel={:?}",
                route.signal_mode.as_db_str(),
                route.channel,
            ),
        );
    }
}

/// Assemble the full `SessionSettings` from the persisted DB
/// rows — the Android analogue of GTK's `build_timer_settings`
/// (`meditate-gtk/src/timer/imp.rs`). Every cue decision lives in
/// `meditate_core::bells::*_from_db`; this only wires the shell
/// context (shape, stopwatch flag, mode) into those helpers. Prep
/// is Timer-only (mirrors GTK gating prep to the timer path);
/// box-breath cues only attach for BoxBreath. Falls back to a
/// bare default session if the DB isn't available (shouldn't
/// happen post-startup, but a no-cue session beats a panic).
#[cfg(target_os = "android")]
fn build_session_settings(
    shape: meditate_core::session::SessionShape,
    stopwatch_on: bool,
    mode: meditate_core::SessionMode,
) -> meditate_core::session::SessionSettings {
    use meditate_core::bells;
    use meditate_core::session::SessionSettings;

    let display = bells::DisplayMode::from_stopwatch_flag(stopwatch_on);
    let target = shape.target_secs().map(u64::from);

    let Some(db_arc) = DATABASE.get() else {
        return SessionSettings { shape, ..Default::default() };
    };
    let Ok(guard) = db_arc.lock() else {
        return SessionSettings { shape, ..Default::default() };
    };
    let Some(db) = guard.as_ref() else {
        return SessionSettings { shape, ..Default::default() };
    };

    // Prep is Timer-only (GTK gates it to the timer path) and the
    // core helper AND-gates `preparation_time_active &&
    // starting_bell_active` — no starting bell ⇒ no prep. Routing
    // through `prep_plan_from_db` (the exact call GTK's on_start
    // uses) keeps that decision in core for both shells; the
    // earlier direct `preparation_time_active` read skipped the
    // starting-bell gate (the bug Janek hit).
    let prep_secs = if matches!(mode, meditate_core::SessionMode::Timer) {
        meditate_core::format::prep_plan_from_db(db)
            .map(|d| d.as_secs() as u32)
    } else {
        None
    };

    let (session_bells, bell_rng_seed) =
        bells::session_bells_from_db(db, target, display);
    let box_breath_cues =
        if matches!(mode, meditate_core::SessionMode::BoxBreath) {
            Some(bells::box_breath_cues_from_db(db))
        } else {
            None
        };

    SessionSettings {
        shape,
        prep_secs,
        bells: session_bells,
        bell_rng_seed,
        signal_mode_override: bells::signal_mode_override_from_db(db, mode),
        starting_bell: bells::starting_bell_cue_from_db(db),
        end_bell: bells::end_bell_cue_from_db(db, display),
        box_breath_cues,
    }
}

// Host-only fallback duration (no DB on the desktop dev build).
// On Android the Timer length is restored from the persisted
// `timer_session_secs` (see `read_timer_session_secs`), so these
// are unused there and cfg-scoped to the host to stay warning-
// clean. 10 min mirrors GTK's default opening position.
#[cfg(not(target_os = "android"))]
const DEFAULT_HOURS: i32 = 0;
#[cfg(not(target_os = "android"))]
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

/// Suspend-resilient monotonic time (Duration since boot) via
/// `meditate_core::time::boot_time_now()` — libc
/// `clock_gettime(CLOCK_BOOTTIME)`, the same clock GTK uses.
///
/// Earlier this used `std::time::Instant`, on the (wrong) belief
/// that Rust's `Instant` is CLOCK_BOOTTIME on Android. It is NOT
/// — `Instant` is CLOCK_MONOTONIC, which FREEZES during system
/// suspend, so a session with the screen off in a pocket only
/// counted awake time (e.g. a ~34 min stopwatch recorded ~15
/// min). Core `Session` computes elapsed as `now - start`, so a
/// since-boot origin is fine and the fix flows through session
/// elapsed, the snapshot heartbeat, and the tick loop alike.
fn now_since_epoch() -> Duration {
    meditate_core::time::boot_time_now()
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
    ui.set_overtime_active(state.is_overtime());
    ui.set_running_page(state.is_running_page());
    ui.set_done_page(state.is_done_page());
}

/// Pull the latest `UpdateOvertimeLabel { overtime }` out of a
/// tick's effects and render the Add-button text the GTK way:
/// `"Add MM:SS ?"` via `format::format_time` (ASCII "?" — the
/// FP5 font lacks fancier glyphs). Returns `None` when this tick
/// had no overtime update (so the caller leaves the label as-is).
#[cfg(target_os = "android")]
fn overtime_add_label(effects: &[meditate_core::session::Effect]) -> Option<String> {
    use meditate_core::session::Effect;
    effects.iter().rev().find_map(|e| match e {
        Effect::UpdateOvertimeLabel { overtime } => Some(format!(
            "Add {} ?",
            meditate_core::format::format_time(*overtime),
        )),
        _ => None,
    })
}

/// The `duration_secs` core wants recorded for this end —
/// `finish_overtime` carries the planned target, `add_overtime_
/// and_finish` the full elapsed. Used so the saved row matches
/// the user's Finish-vs-Add choice exactly (mirrors GTK reading
/// the same EndSession effect).
#[cfg(target_os = "android")]
fn end_session_duration(
    effects: &[meditate_core::session::Effect],
) -> Option<u64> {
    use meditate_core::session::Effect;
    effects.iter().rev().find_map(|e| match e {
        Effect::EndSession { duration_secs } => Some(*duration_secs),
        _ => None,
    })
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

// ── Preset read/validate wrappers (Phase 6 / P-1) ───────────
// Thin DATABASE-lock shims over `meditate_core::db` preset
// queries, mirroring GTK's `presets.rs` data flow exactly:
// the home chip list uses *starred* presets gated on
// `mode_supports_presets` (Guided has its own files library,
// no presets — `timer/imp.rs:3159`); the Manage / chooser list
// uses *all* presets for the mode (`presets.rs:209`); name
// validation is `is_preset_name_taken(name, except_uuid)` with
// `""` on create and the preset's own uuid on rename
// (`presets.rs:461,520`). Apply / Save / Manage plumbing builds
// on these in P-2..P-5; decisions stay in core
// (`preset_config::{snapshot, apply, mode_supports_presets}`).

/// Whether `mode` exposes presets at all. Mirrors GTK gating
/// the whole presets section on this.
#[cfg(target_os = "android")]
fn presets_supported(mode: meditate_core::SessionMode) -> bool {
    meditate_core::preset_config::mode_supports_presets(mode)
}

/// Starred presets for the home chip list. Empty (chips hidden)
/// when the mode doesn't support presets or none are starred —
/// same as GTK's `list_starred_presets_for_mode` behind the
/// `mode_supports_presets` guard.
#[cfg(target_os = "android")]
fn list_starred_presets_for_mode(
    mode: meditate_core::SessionMode,
) -> Vec<PresetItem> {
    if !presets_supported(mode) {
        return Vec::new();
    }
    let Some(db_arc) = DATABASE.get() else { return Vec::new(); };
    let Ok(guard) = db_arc.lock() else { return Vec::new(); };
    let Some(db) = guard.as_ref() else { return Vec::new(); };
    // One labels roundtrip per rebuild → O(1) subtitle label
    // lookup, exactly like GTK's `label_names` map.
    let label_names: std::collections::HashMap<String, String> =
        meditate_core::db::list_labels_from_db(db)
            .unwrap_or_default()
            .into_iter()
            .map(|l| (l.uuid.to_string(), l.name))
            .collect();
    meditate_core::db::list_starred_presets_for_mode_from_db(db, mode)
        .unwrap_or_default()
        .into_iter()
        .map(|p| PresetItem {
            uuid: p.uuid.to_string().into(),
            subtitle: preset_subtitle(&p.config_json, &label_names).into(),
            name: p.name.into(),
            is_starred: p.is_starred,
        })
        .collect()
}

/// Compose a preset-row subtitle — the Android mirror of GTK's
/// `crate::preset_subtitle::preset_subtitle`. The structural
/// decomposition is core's (`format::preset_subtitle_parts`);
/// this stitches the parts with " · ", inline English (Android
/// i18n is shell-deferred like the rest of the port). Empty
/// string on unparseable config_json (matches GTK).
#[cfg(target_os = "android")]
fn preset_subtitle(
    config_json: &str,
    label_names: &std::collections::HashMap<String, String>,
) -> String {
    use meditate_core::format::{
        BellsCountKey, BoxBreathAfter, TimingKey,
    };
    let Some(parts) = meditate_core::format::preset_subtitle_parts(config_json)
    else {
        return String::new();
    };
    let mins = |m: u32| {
        if m == 1 {
            "1 min".to_string()
        } else {
            format!("{m} min")
        }
    };
    let mut out: Vec<String> = Vec::new();
    match parts.timing {
        TimingKey::Stopwatch => out.push("Stopwatch".to_string()),
        TimingKey::Duration { mins: m } => out.push(mins(m)),
        TimingKey::BoxBreath {
            inhale_secs,
            hold_full_secs,
            exhale_secs,
            hold_empty_secs,
            after,
        } => {
            out.push(format!(
                "{inhale_secs}-{hold_full_secs}-{exhale_secs}-{hold_empty_secs}"
            ));
            match after {
                BoxBreathAfter::Stopwatch => {
                    out.push("Stopwatch".to_string())
                }
                BoxBreathAfter::Duration { mins: m } => out.push(mins(m)),
            }
        }
    }
    if let Some(uuid) = parts.label_uuid.as_ref() {
        if let Some(name) = label_names.get(uuid.as_str()) {
            out.push(name.clone());
        }
    }
    match parts.bells {
        Some(BellsCountKey::One) => out.push("1 bell".to_string()),
        Some(BellsCountKey::Many(n)) => out.push(format!("{n} bells")),
        None => {}
    }
    out.join(" · ")
}

/// Push the active mode's starred presets into the Setup
/// `presets-list` (empty for Guided / no-presets → the section
/// hides itself). Mirrors GTK's `rebuild_starred_preset_rows`
/// on mode switch + after preset CRUD.
#[cfg(target_os = "android")]
fn refresh_preset_chips(ui: &MainWindow, mode: meditate_core::SessionMode) {
    // `presets-supported` shows the Save/Manage buttons even with
    // zero presets (GTK keeps the button box visible while the
    // section is); the row list hides itself when empty.
    ui.set_presets_supported(presets_supported(mode));
    let items = list_starred_presets_for_mode(mode);
    ui.set_presets_list(
        std::rc::Rc::new(slint::VecModel::from(items)).into(),
    );
}

/// Every preset for the mode (Save/Manage chooser list — starred
/// and unstarred). Mirrors GTK's `list_presets_for_mode` in
/// `rebuild_chooser_rows`.
#[cfg(target_os = "android")]
fn list_presets_for_mode(
    mode: meditate_core::SessionMode,
) -> Vec<PresetItem> {
    if !presets_supported(mode) {
        return Vec::new();
    }
    let Some(db_arc) = DATABASE.get() else { return Vec::new(); };
    let Ok(guard) = db_arc.lock() else { return Vec::new(); };
    let Some(db) = guard.as_ref() else { return Vec::new(); };
    let label_names: std::collections::HashMap<String, String> =
        meditate_core::db::list_labels_from_db(db)
            .unwrap_or_default()
            .into_iter()
            .map(|l| (l.uuid.to_string(), l.name))
            .collect();
    meditate_core::db::list_presets_for_mode_from_db(db, mode)
        .unwrap_or_default()
        .into_iter()
        .map(|p| PresetItem {
            uuid: p.uuid.to_string().into(),
            subtitle: preset_subtitle(&p.config_json, &label_names).into(),
            name: p.name.into(),
            is_starred: p.is_starred,
        })
        .collect()
}

/// Fill the shared preset-chooser overlay list for `mode`.
#[cfg(target_os = "android")]
fn populate_preset_chooser(
    ui: &MainWindow,
    mode: meditate_core::SessionMode,
) {
    let items = list_presets_for_mode(mode);
    ui.set_preset_chooser_items(
        std::rc::Rc::new(slint::VecModel::from(items)).into(),
    );
}

/// Modes whose starred presets the home-screen widget lists.
/// Fixed order → the widget list is stable across refreshes
/// (the `RemoteViewsFactory` preserves array order). Guided is
/// excluded by `mode_supports_presets`, so it never appears here;
/// the explicit list keeps it that way without a runtime filter.
#[cfg(target_os = "android")]
const WIDGET_PRESET_MODES: [meditate_core::SessionMode; 2] = [
    meditate_core::SessionMode::Timer,
    meditate_core::SessionMode::BoxBreath,
];

/// Flatten every mode's starred presets into the widget
/// projection. Reuses `preset_subtitle` (one labels roundtrip,
/// shared across modes) so a widget row reads identically to its
/// in-app chip. Empty when the DB is unopened or nothing is
/// starred — the widget then shows its empty state.
#[cfg(target_os = "android")]
fn widget_presets_snapshot() -> Vec<widget::WidgetPreset> {
    let Some(db_arc) = DATABASE.get() else { return Vec::new(); };
    let Ok(guard) = db_arc.lock() else { return Vec::new(); };
    let Some(db) = guard.as_ref() else { return Vec::new(); };
    let label_names: std::collections::HashMap<String, String> =
        meditate_core::db::list_labels_from_db(db)
            .unwrap_or_default()
            .into_iter()
            .map(|l| (l.uuid.to_string(), l.name))
            .collect();
    let mut out = Vec::new();
    for mode in WIDGET_PRESET_MODES {
        if !meditate_core::preset_config::mode_supports_presets(mode) {
            continue;
        }
        let rows = meditate_core::db::list_starred_presets_for_mode_from_db(
            db, mode,
        )
        .unwrap_or_default();
        for p in rows {
            out.push(widget::WidgetPreset {
                uuid: p.uuid.to_string(),
                subtitle: preset_subtitle(&p.config_json, &label_names),
                name: p.name,
                mode: mode.as_db_str(),
            });
        }
    }
    out
}

/// Rebuild + push the widget projection. Call after every preset
/// mutation that can change the starred set (create / override /
/// star toggle / rename / delete and their snackbar Undos) and
/// once at startup. No-op when the AndroidApp handle isn't set
/// yet (never the case post-`android_main`) or no widget is
/// installed (the JNI side short-circuits). Cheap enough to call
/// unconditionally — two indexed `WHERE is_starred` queries.
#[cfg(target_os = "android")]
fn refresh_widget() {
    if let Some(app) = android_app() {
        widget::publish(app, widget_presets_snapshot());
    }
}

/// Consume a pending widget-tap deep-link, if one is waiting
/// (W-3/W-4). The widget's broadcast receiver dropped the tapped
/// preset's uuid in `<files>/meditate/widget_launch` and
/// foregrounded us; this picks it up, switches to that preset's
/// mode, applies it, and starts the session — driving the exact
/// same `invoke_mode_changed` / `invoke_action_tap` a manual
/// chip-switch + Start tap would, so there is no behaviour fork.
///
/// Called from two places, both reading the single-consumption
/// file so they can't double-fire: `build_ui` once at startup
/// (cold launch — app was dead) and the tick loop every frame
/// (warm — NativeActivity gives native code no `onNewIntent`,
/// so polling the drop file is the only channel). Returns
/// `true` when it consumed a tap *this call* (the tick loop
/// then skips its normal body for that frame to avoid touching
/// the `state` borrow `invoke_action_tap` just took).
///
/// A tap arriving mid-session is dropped (logged): silently
/// pausing a running meditation because a stray widget tap
/// landed would be worse than ignoring it.
#[cfg(target_os = "android")]
fn try_widget_deep_link(
    ui: &MainWindow,
    timer_session_secs: &std::rc::Rc<std::cell::Cell<u32>>,
    state: &std::rc::Rc<std::cell::RefCell<AppState>>,
) -> bool {
    let Some(app) = android_app() else { return false; };
    let Some(uuid) = widget::take_pending_launch(app) else {
        return false;
    };
    if state.borrow().is_active() {
        meditate_core::log(
            "widget",
            &format!("deep-link ignored (session active) uuid={uuid}"),
        );
        return true;
    }
    let Some(preset) = find_preset_by_uuid(&uuid) else {
        meditate_core::log(
            "widget",
            &format!("deep-link preset gone uuid={uuid}"),
        );
        return true;
    };
    let tmode = match preset.mode {
        meditate_core::SessionMode::Timer => Some(TimerMode::Timer),
        meditate_core::SessionMode::BoxBreath => Some(TimerMode::Breathing),
        // Guided has no presets (mode_supports_presets); a uuid
        // landing here is a corrupt link — ignore, don't mis-start.
        meditate_core::SessionMode::Guided => None,
    };
    match tmode {
        Some(tmode) if presets_supported(preset.mode) => {
            let idx = tmode.to_chip_index();
            ui.set_setup_mode(idx);
            ui.invoke_mode_changed(idx);
            if apply_preset_json(
                ui,
                &preset.config_json,
                preset.mode,
                timer_session_secs,
            ) {
                ui.invoke_action_tap();
                meditate_core::log(
                    "widget",
                    &format!("deep-link autostart uuid={uuid}"),
                );
            } else {
                meditate_core::log(
                    "widget",
                    &format!("deep-link apply FAILED uuid={uuid}"),
                );
            }
        }
        _ => meditate_core::log(
            "widget",
            &format!("deep-link ignored (mode) uuid={uuid}"),
        ),
    }
    true
}

/// Case-insensitive preset-name collision check for the create
/// dialog's live validation. `except_uuid` = `""` on create
/// (rename's own-uuid exception lands with P-5). Mirrors GTK's
/// `is_preset_name_taken` call in `present_create_preset_dialog`.
#[cfg(target_os = "android")]
fn preset_name_taken(name: &str, except_uuid: &str) -> bool {
    let Some(db_arc) = DATABASE.get() else { return false; };
    let Ok(guard) = db_arc.lock() else { return false; };
    let Some(db) = guard.as_ref() else { return false; };
    meditate_core::db::is_preset_name_taken_from_db(db, name, except_uuid)
        .unwrap_or(false)
}

/// Resolve a preset by uuid to the core row (carries the
/// `config_json` Apply needs + the `mode` guard). `None` when
/// the row is gone (raced a peer delete) — caller bails like
/// GTK's `on_preset_row_activated` match.
#[cfg(target_os = "android")]
fn find_preset_by_uuid(uuid: &str) -> Option<meditate_core::db::Preset> {
    let db_arc = DATABASE.get()?;
    let guard = db_arc.lock().ok()?;
    let db = guard.as_ref()?;
    meditate_core::db::find_preset_by_uuid_from_db(db, uuid)
        .ok()
        .flatten()
}

/// Apply a `PresetConfig` JSON blob for `mode` and re-read the
/// whole Setup surface — the Android analogue of GTK's
/// `apply_config`. Used by both the row-tap (forward apply) and
/// the snackbar Undo (re-apply the pre-apply snapshot), exactly
/// as GTK's Undo calls the same `apply_config(&snapshot)`.
///
/// `apply()` does all the DB writes; the lock is dropped before
/// the UI refreshes (they re-lock). The returned `PresetTiming`
/// is shell-side reactive state (duration / stopwatch / BB
/// pattern), persisted here like GTK's `set_countdown_target` /
/// `set_breathing_duration_secs`. Returns `false` on parse or
/// `ApplyError` (SyncPending is unreachable until the Phase-7
/// sync loop — seeded presets reference bundled UUIDs).
#[cfg(target_os = "android")]
fn apply_preset_json(
    ui: &MainWindow,
    json: &str,
    mode: meditate_core::SessionMode,
    timer_session_secs: &std::rc::Rc<std::cell::Cell<u32>>,
) -> bool {
    use meditate_core::preset_config::{apply, PresetConfig, PresetTiming};
    let Ok(cfg) = PresetConfig::from_json(json) else {
        meditate_core::log("preset.apply", "config_json parse failed");
        return false;
    };
    let outcome = {
        let Some(db_arc) = DATABASE.get() else { return false; };
        let Ok(guard) = db_arc.lock() else { return false; };
        let Some(db) = guard.as_ref() else { return false; };
        apply(db, &cfg, mode)
    };
    let timing = match outcome {
        Ok(t) => t,
        Err(e) => {
            meditate_core::log(
                "preset.apply",
                &format!("apply FAILED: {e:?}"),
            );
            return false;
        }
    };
    match timing {
        PresetTiming::Timer {
            stopwatch,
            duration_secs,
        } => {
            timer_session_secs.set(duration_secs);
            write_timer_session_secs(duration_secs);
            ui.set_stopwatch_on(stopwatch);
            push_session_length_to_ui(ui, duration_secs);
        }
        PresetTiming::BoxBreath {
            stopwatch,
            inhale_secs,
            hold_full_secs,
            exhale_secs,
            hold_empty_secs,
            duration_secs,
        } => {
            let pat = meditate_core::breath::BreathPattern::clamp_from_raw(
                inhale_secs,
                hold_full_secs,
                exhale_secs,
                hold_empty_secs,
            );
            write_breathing_pattern(pat);
            write_breathing_session_secs(duration_secs);
            ui.set_stopwatch_on(stopwatch);
            push_session_length_to_ui(ui, duration_secs);
            refresh_breathing_tiles(ui, pat);
        }
    }
    // Re-read everything apply() persisted (settings / bells /
    // box-breath cues / label / cues override / keep-awake).
    ui.set_keep_awake_on(read_keep_awake_for_mode(mode));
    ui.set_cues_mode(signal_mode_to_chip_index(
        read_signal_mode_for_mode(mode),
    ));
    ui.set_label_active(read_label_active_for_mode(mode));
    ui.set_label_name(
        resolved_label_for_mode(mode)
            .map(|(name, _)| name)
            .unwrap_or_default()
            .into(),
    );
    refresh_bell_rows(ui);
    refresh_preset_chips(ui, mode);
    true
}

/// Snapshot the current Setup into a `PresetConfig` JSON blob —
/// the pre-apply state the snackbar Undo re-applies (mirrors
/// GTK's `snapshot_current_setup`). Timing comes from live
/// shell state (stopwatch + duration / BB pattern); core's
/// `snapshot` reads everything else from the DB.
#[cfg(target_os = "android")]
fn snapshot_setup_json(
    ui: &MainWindow,
    mode: meditate_core::SessionMode,
    timer_session_secs: u32,
) -> Option<String> {
    use meditate_core::preset_config::{snapshot, PresetTiming};
    let timing = match mode {
        meditate_core::SessionMode::BoxBreath => {
            let p = read_breathing_pattern();
            PresetTiming::BoxBreath {
                stopwatch: ui.get_stopwatch_on(),
                inhale_secs: p.in_secs,
                hold_full_secs: p.hold_in,
                exhale_secs: p.out_secs,
                hold_empty_secs: p.hold_out,
                duration_secs: read_breathing_session_secs(),
            }
        }
        _ => PresetTiming::Timer {
            stopwatch: ui.get_stopwatch_on(),
            duration_secs: timer_session_secs,
        },
    };
    let db_arc = DATABASE.get()?;
    let guard = db_arc.lock().ok()?;
    let db = guard.as_ref()?;
    Some(snapshot(db, mode, timing).to_json())
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

/// Read the persisted sync state and push it into the
/// sync-indicator Slint properties. `is_syncing` is hard-false
/// on Android — the sync loop lands in Phase 7, so there's no
/// in-flight sync to report yet; the indicator still resolves
/// to Hidden / Error / Ok from the DB-persisted last-sync
/// fields once an account exists. Mirrors GTK's
/// `refresh_sync_status` at `meditate-gtk/src/window/imp.rs:630`
/// + `sync_indicator_state_now`. Maps the core
/// `SyncIndicatorState` enum to the int the Slint surface
/// switches on (0 Hidden / 1 Syncing / 2 Error / 3 Ok).
#[cfg(target_os = "android")]
fn refresh_sync_indicator(ui: &MainWindow) {
    use meditate_core::sync::indicator::{state_from_db, SyncIndicatorState};
    let Some(db_arc) = DATABASE.get() else {
        ui.set_sync_indicator_state(0);
        return;
    };
    let Ok(guard) = db_arc.lock() else {
        ui.set_sync_indicator_state(0);
        return;
    };
    let Some(db) = guard.as_ref() else {
        ui.set_sync_indicator_state(0);
        return;
    };
    let (state, tooltip) = match state_from_db(db, false) {
        SyncIndicatorState::Hidden => (0, String::new()),
        SyncIndicatorState::Syncing => {
            (1, "Syncing with Nextcloud…".to_string())
        }
        SyncIndicatorState::Error { detail, .. } => {
            (2, format!("Last sync failed — tap to retry\n{detail}"))
        }
        SyncIndicatorState::OkWithTs(ts) => {
            (3, sync_ago_text(ts))
        }
        SyncIndicatorState::OkNoTs => {
            (3, "Sync configured (waiting for first run)".to_string())
        }
    };
    ui.set_sync_indicator_state(state);
    ui.set_sync_indicator_tooltip(tooltip.into());
}

/// "Synced N ago" tooltip. Bucket decision lives in core
/// (`meditate_core::format::synced_ago_key`); this is the
/// Android-side English renderer (i18n isn't wired on Android
/// yet — same situation as the delete / recovery snackbars).
/// Mirrors GTK's `format_synced_ago` at
/// `meditate-gtk/src/window/imp.rs:713`.
#[cfg(target_os = "android")]
fn sync_ago_text(unix_ts: i64) -> String {
    use meditate_core::format::SyncedAgoKey;
    let secs_ago = meditate_core::time::unix_now() - unix_ts;
    match meditate_core::format::synced_ago_key(secs_ago) {
        SyncedAgoKey::JustNow => "Synced just now".to_string(),
        SyncedAgoKey::Minutes(n) if n == 1 => "Synced 1 minute ago".to_string(),
        SyncedAgoKey::Minutes(n) => format!("Synced {n} minutes ago"),
        SyncedAgoKey::Hours(n) if n == 1 => "Synced 1 hour ago".to_string(),
        SyncedAgoKey::Hours(n) => format!("Synced {n} hours ago"),
        SyncedAgoKey::Days(n) if n == 1 => "Synced 1 day ago".to_string(),
        SyncedAgoKey::Days(n) => format!("Synced {n} days ago"),
    }
}

/// Android-side renderer for `meditate_core::format::HmKey`
/// ("1h 4m" / "5m" / "–"). Mirrors GTK's `render_hm` at
/// `meditate-gtk/src/format.rs:18` but inlines English — i18n
/// isn't wired on Android yet (same deferral as the snackbar /
/// sync-indicator text).
#[cfg(target_os = "android")]
fn render_hm(key: meditate_core::format::HmKey) -> String {
    use meditate_core::format::HmKey;
    match key {
        HmKey::Empty => "–".to_string(),
        HmKey::MinsOnly(m) => format!("{m}m"),
        HmKey::HoursOnly(h) => format!("{h}h"),
        HmKey::HoursMins(h, m) => format!("{h}h {m}m"),
    }
}

/// Refresh the Stats page values. S-1 covers the Mini-stats
/// row only: best-streak ("Nd" / "–"), all-time total via the
/// HmKey renderer, and session count via
/// `meditate_core::format::mini_stat_or_dash`. Mirrors GTK's
/// `reload_mini_stats` at `meditate-gtk/src/stats/imp.rs:533`.
#[cfg(target_os = "android")]
fn refresh_stats(ui: &MainWindow) {
    let Some(db_arc) = DATABASE.get() else { return; };
    let Ok(guard) = db_arc.lock() else { return; };
    let Some(db) = guard.as_ref() else { return; };

    let streak = meditate_core::db::get_best_streak_from_db(db).unwrap_or(0);
    let total = meditate_core::db::total_seconds_from_db(db).unwrap_or(0);
    let count = meditate_core::db::count_sessions_from_db(db).unwrap_or(0);

    ui.set_stat_streak(
        if streak == 0 {
            "–".to_string()
        } else {
            format!("{streak}d")
        }
        .into(),
    );
    ui.set_stat_total(
        render_hm(meditate_core::format::hm_compact_key(
            std::time::Duration::from_secs(total.max(0) as u64),
        ))
        .into(),
    );
    ui.set_stat_sessions(
        meditate_core::format::mini_stat_or_dash(count).into(),
    );

    // Weekly-goal hero (S-2). Week-start = today minus
    // `days_since_week_start` (locale week-start dow; Android's
    // bionic falls back to Monday — see
    // `date_math::locale_week_start_dow`). Mirrors GTK's
    // `reload_goal_ring` at `meditate-gtk/src/stats/imp.rs:149`.
    use chrono::Datelike;
    let today = meditate_core::time::today_local();
    let week_start_dow = meditate_core::date_math::locale_week_start_dow();
    let today_dow = today.weekday().number_from_monday() as i32;
    let back = meditate_core::date_math::days_since_week_start(
        today_dow,
        week_start_dow,
    );
    let week_start = today - chrono::Duration::days(back as i64);
    let week_secs =
        meditate_core::db::total_secs_since_from_db(db, week_start)
            .unwrap_or(0);
    let goal_mins = meditate_core::goal::weekly_goal_mins_from_db(db);
    let g = meditate_core::goal::compute(week_secs, goal_mins);

    let mins_dur =
        |m: i64| std::time::Duration::from_secs((m.max(0) as u64) * 60);
    let week_str = render_hm(meditate_core::format::hm_mins_key(
        mins_dur(g.week_mins),
    ));
    let goal_str = render_hm(meditate_core::format::hm_mins_key(
        mins_dur(g.goal_mins),
    ));

    ui.set_stat_goal_pct(g.arc_pct as f32);
    ui.set_stat_goal_pct_label(format!("{}%", g.display_pct).into());
    ui.set_stat_goal_progress(format!("{week_str} / {goal_str}").into());
    let sub = match g.status {
        // GTK appends a "✓" here, but the Android 15 system
        // font has no U+2713 glyph (renders as a tofu box —
        // same coverage gap that bit the labels chooser
        // check-mark). Drop it; the copy reads fine without.
        meditate_core::goal::GoalStatus::Reached => format!(
            "Goal reached · {week_str} this week"
        ),
        meditate_core::goal::GoalStatus::InProgress => {
            let remaining = render_hm(meditate_core::format::hm_mins_key(
                mins_dur(g.remaining_mins),
            ));
            format!("{remaining} to go this week")
        }
    };
    ui.set_stat_goal_sub(sub.into());

    // Insights (S-3). Batch every insight-driving read inside
    // this same DB borrow, run the portable compute pass, then
    // render each typed key to a card. Mirrors GTK's
    // `reload_insights` + `render_insight` at
    // `meditate-gtk/src/stats/imp.rs:275`.
    let (ty, tm) = (today.year(), today.month());
    let (ly, lm) = if tm == 1 { (ty - 1, 12) } else { (ty, tm - 1) };
    let fourteen_since = today - chrono::Duration::days(13);
    let daily_totals: Vec<(String, i64)> =
        meditate_core::db::get_daily_totals_since_from_db(db, fourteen_since)
            .unwrap_or_default()
            .into_iter()
            .map(|(d, secs)| (d.format("%Y-%m-%d").to_string(), secs))
            .collect();
    let input = meditate_core::insights::InsightInput {
        current_streak: meditate_core::db::get_streak_from_db(db, today)
            .unwrap_or(0),
        best_streak: meditate_core::db::get_best_streak_from_db(db)
            .unwrap_or(0),
        this_month_secs: meditate_core::db::month_total_secs_from_db(
            db, ty, tm,
        )
        .unwrap_or(0),
        last_month_secs: meditate_core::db::month_total_secs_from_db(
            db, ly, lm,
        )
        .unwrap_or(0),
        daily_totals,
        longest: meditate_core::db::get_longest_session_from_db(db)
            .unwrap_or(None)
            .map(|(_rowid, s)| {
                (s.duration_secs as i64, s.start_unix())
            }),
        typical_secs: meditate_core::db::get_median_duration_secs_from_db(db)
            .unwrap_or(None)
            .unwrap_or(0) as i64,
        avg_secs_7d: meditate_core::db::get_running_average_secs_from_db(
            db, today, 7,
        )
        .unwrap_or(0.0) as i64,
        hour_buckets: meditate_core::db::hour_buckets_from_db(db)
            .unwrap_or((0, 0, 0)),
        session_count: count,
    };
    let keys = meditate_core::insights::compute(
        &input,
        meditate_core::time::unix_now(),
        meditate_core::date_math::locale_week_start_dow(),
    );
    let rows: Vec<InsightRow> = keys
        .into_iter()
        .map(|k| {
            let accent = k.is_accent();
            let icon_id = insight_icon_id(&k);
            let (title, body) = render_insight(&k);
            InsightRow {
                title: title.into(),
                body: body.into(),
                accent,
                icon_id,
            }
        })
        .collect();
    ui.set_stat_insights(
        std::rc::Rc::new(slint::VecModel::from(rows)).into(),
    );

    // By-label totals (S-4). Empty Vec ⇒ the Slint section
    // hides itself (`stat-label-totals.length > 0` gate),
    // mirroring GTK's `reload_label_totals` visibility logic
    // at `meditate-gtk/src/stats/imp.rs:553`.
    let label_rows: Vec<LabelTotalRow> =
        meditate_core::db::label_totals_seconds_from_db(db)
            .unwrap_or_default()
            .into_iter()
            .map(|(name, secs, n)| {
                let dur = render_hm(meditate_core::format::hm_secs_key(
                    std::time::Duration::from_secs(secs.max(0) as u64),
                ));
                let subtitle = if n == 1 {
                    format!("{dur} · 1 session")
                } else {
                    format!("{dur} · {n} sessions")
                };
                LabelTotalRow {
                    name: name.into(),
                    subtitle: subtitle.into(),
                }
            })
            .collect();
    ui.set_stat_label_totals(
        std::rc::Rc::new(slint::VecModel::from(label_rows)).into(),
    );

    // Contribution heatmap (S-6). 91 cells column-major from
    // core; chunk into 13 week-columns of 7. Mirrors GTK's
    // `reload_contrib_grid` at
    // `meditate-gtk/src/stats/imp.rs:191`.
    {
        let totals: std::collections::HashMap<chrono::NaiveDate, i64> =
            meditate_core::db::get_daily_totals_from_db(db)
                .unwrap_or_default()
                .into_iter()
                .collect();
        let daily_expected =
            meditate_core::goal::daily_expected_mins(goal_mins);
        let core_cells = meditate_core::contrib::build_grid(
            today,
            meditate_core::date_math::locale_week_start_dow(),
            &totals,
            daily_expected,
        );
        let cols: Vec<ContribCol> = core_cells
            .chunks(7)
            .map(|week| {
                let cells: Vec<ContribCellData> = week
                    .iter()
                    .map(|c| ContribCellData {
                        level: c.level as i32,
                        future: c.is_future,
                        today: c.is_today,
                    })
                    .collect();
                ContribCol {
                    cells: std::rc::Rc::new(slint::VecModel::from(cells))
                        .into(),
                }
            })
            .collect();
        // Range caption: oldest-cell month – current month.
        let month_abbr = |iso: &str| {
            chrono::NaiveDate::parse_from_str(iso, "%Y-%m-%d")
                .ok()
                .map(|d| d.format("%b").to_string())
                .unwrap_or_default()
        };
        let range = match core_cells.first() {
            Some(first) => format!(
                "{} – {}",
                month_abbr(&first.date_iso),
                today.format("%b"),
            ),
            None => String::new(),
        };
        ui.set_stat_contrib_cols(
            std::rc::Rc::new(slint::VecModel::from(cols)).into(),
        );
        ui.set_stat_contrib_range(range.into());
    }

    // Chart (S-5a). Drop the DB guard first — `refresh_chart`
    // re-locks it itself, and holding two nested locks on the
    // same Mutex would deadlock.
    drop(guard);
    refresh_chart(ui);
}

/// Map the Stats chart SegmentedButton index (0 Week / 1
/// Month / 2 3-Months / 3 Year) to a `ChartPeriod`. Order
/// matches the Slint `items` array.
#[cfg(target_os = "android")]
fn chart_period_from_index(i: i32) -> meditate_core::date_math::ChartPeriod {
    use meditate_core::date_math::ChartPeriod;
    match i {
        1 => ChartPeriod::FourWeeks,
        2 => ChartPeriod::ThreeMonths,
        3 => ChartPeriod::OneYear,
        _ => ChartPeriod::Week,
    }
}

/// Sparse x-axis caption for chart bar `i`. Mirrors GTK's
/// `x_label_text` at `meditate-gtk/src/stats/imp.rs:742`: the
/// `x_label_kind` decision lives in core, the locale-aware
/// rendering is shell-side. Android uses chrono's English
/// `%a` / `%b` (i18n shell-deferred, as elsewhere).
#[cfg(target_os = "android")]
fn chart_x_label(
    date_str: &str,
    i: usize,
    days: u32,
    months: &[u32],
) -> String {
    use meditate_core::date_math::{x_label_kind, XLabelKind};
    let parse = || chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok();
    match x_label_kind(i, days, months) {
        XLabelKind::Empty => String::new(),
        XLabelKind::Weekday => parse()
            .map(|d| d.format("%a").to_string())
            .unwrap_or_default(),
        XLabelKind::MonthShortDay => parse()
            .map(|d| d.format("%b %-d").to_string())
            .unwrap_or_default(),
        XLabelKind::MonthLetter => parse()
            .and_then(|d| {
                d.format("%b").to_string().chars().next().map(|c| c.to_string())
            })
            .unwrap_or_default(),
    }
}

/// Recompute the Stats chart from the active period. Builds a
/// dense (0-filled) daily series for the trailing window,
/// aggregates it (monthly ≥1y, weekly ≥3m, else daily) via
/// core, derives the y-axis ticks, and emits one `ChartBar`
/// per data point. Mirrors GTK's `reload_chart` at
/// `meditate-gtk/src/stats/imp.rs:437` (S-5a = bars only; the
/// line variant is S-5b).
#[cfg(target_os = "android")]
fn refresh_chart(ui: &MainWindow) {
    let Some(db_arc) = DATABASE.get() else { return; };
    let Ok(guard) = db_arc.lock() else { return; };
    let Some(db) = guard.as_ref() else { return; };

    let period = chart_period_from_index(ui.get_stat_chart_period());
    let days = period.days();
    let today = meditate_core::time::today_local();
    let since = today - chrono::Duration::days(i64::from(days) - 1);

    let sparse: std::collections::HashMap<String, i64> =
        meditate_core::db::get_daily_totals_since_from_db(db, since)
            .unwrap_or_default()
            .into_iter()
            .map(|(d, secs)| (d.format("%Y-%m-%d").to_string(), secs))
            .collect();

    let daily: Vec<(String, i64)> = (0..i64::from(days))
        .map(|i| {
            let dt = since + chrono::Duration::days(i);
            let key = dt.format("%Y-%m-%d").to_string();
            let secs = sparse.get(&key).copied().unwrap_or(0);
            (key, secs)
        })
        .collect();

    let data = meditate_core::date_math::aggregate_for_chart_period(
        &daily, days,
    );
    let series: Vec<i64> = data.iter().map(|(_, d)| *d).collect();
    let ticks = meditate_core::date_math::chart_y_axis_ticks(&series);
    let months: Vec<u32> = data
        .iter()
        .map(|(d, _)| d.get(5..7).and_then(|s| s.parse().ok()).unwrap_or(0))
        .collect();

    let bars: Vec<ChartBar> = data
        .iter()
        .enumerate()
        .map(|(i, (date_str, v))| ChartBar {
            ratio: (*v as f32 / ticks.max as f32).clamp(0.0, 1.0),
            label: chart_x_label(date_str, i, days, &months).into(),
        })
        .collect();

    // Line + area SVG paths (S-5b) against a 1000×100 viewbox
    // (Slint scales to the plot). Geometry mirrors GTK's Cairo
    // line path at `meditate-gtk/src/stats/imp.rs:646`: point
    // x = slot-centre, y inverted from the height ratio; the
    // area is the same polyline closed down to the baseline.
    // Empty when there's no point (Path then draws nothing).
    let n = bars.len();
    let (line_cmd, area_cmd) = if n == 0 {
        (String::new(), String::new())
    } else {
        const VBW: f32 = 1000.0;
        const VBH: f32 = 100.0;
        let px = |i: usize| (i as f32 + 0.5) / n as f32 * VBW;
        let py = |r: f32| VBH - r.clamp(0.0, 1.0) * VBH;
        let mut line = String::new();
        let mut area = format!("M {:.2} {:.2}", px(0), VBH);
        for (i, b) in bars.iter().enumerate() {
            let (x, y) = (px(i), py(b.ratio));
            if i == 0 {
                line.push_str(&format!("M {x:.2} {y:.2}"));
            } else {
                line.push_str(&format!(" L {x:.2} {y:.2}"));
            }
            area.push_str(&format!(" L {x:.2} {y:.2}"));
        }
        area.push_str(&format!(" L {:.2} {:.2} Z", px(n - 1), VBH));
        (line, area)
    };

    let hm = |secs: i64| {
        render_hm(meditate_core::format::hm_secs_key(
            std::time::Duration::from_secs(secs.max(0) as u64),
        ))
    };
    // Axis caption tracks the aggregation tier (the
    // decision lives in core; this is just the English
    // render — i18n shell-deferred, as elsewhere on Android).
    use meditate_core::date_math::ChartUnit;
    ui.set_stat_chart_unit(
        match meditate_core::date_math::chart_unit_for_days(days) {
            ChartUnit::Day => "MINUTES / DAY",
            ChartUnit::Week => "MINUTES / WEEK",
            ChartUnit::Month => "MINUTES / MONTH",
        }
        .into(),
    );
    ui.set_stat_chart_ymax(hm(ticks.max).into());
    ui.set_stat_chart_ymid(hm(ticks.mid).into());
    ui.set_stat_chart_line_cmd(line_cmd.into());
    ui.set_stat_chart_area_cmd(area_cmd.into());
    ui.set_stat_chart_bars(
        std::rc::Rc::new(slint::VecModel::from(bars)).into(),
    );
}

/// Bundled-SVG selector for an insight row, replacing GTK's
/// per-variant glyph (which tofus on the FP5 Android-15 font —
/// see [[feedback_android_no_unicode_glyphs]]). Trend variants
/// pick the up/down icon by sign, matching GTK's "↑"/"↓" glyph
/// switch at `meditate-core/src/insights.rs:104`. IDs are
/// decoded to `@image-url`s in main.slint's InsightRow.
#[cfg(target_os = "android")]
fn insight_icon_id(key: &meditate_core::insights::InsightKey) -> i32 {
    use meditate_core::insights::InsightKey;
    match key {
        InsightKey::CurrentStreak { .. } => 0,
        InsightKey::WeekOverWeek { pct, .. }
        | InsightKey::MonthTrend { pct, .. } => {
            if *pct >= 0 { 1 } else { 2 }
        }
        InsightKey::PreferredTime { .. } => 3,
        InsightKey::TypicalSession { .. } => 4,
        InsightKey::LongestSession { .. } => 5,
        InsightKey::NextMilestone { .. } => 6,
        InsightKey::DailyRhythm { .. } => 7,
        InsightKey::NoData => 8,
    }
}

/// Map a `meditate_core::insights::InsightKey` to its rendered
/// (title, body) pair. Inline English — i18n is shell-deferred
/// on Android (same as the snackbars / sync / goal copy).
/// Mirrors GTK's `render_insight` at
/// `meditate-gtk/src/stats/imp.rs:317`; durations go through
/// `render_hm(hm_secs_key(..))` (the seconds-precision variant
/// GTK uses via `format_hm_secs`). The leading per-variant
/// glyph GTK draws is intentionally dropped — those code
/// points tofu on the FP5 Android-15 font.
#[cfg(target_os = "android")]
fn render_insight(
    key: &meditate_core::insights::InsightKey,
) -> (String, String) {
    use meditate_core::insights::{HourBucket, InsightKey};
    let hm = |secs: i64| {
        render_hm(meditate_core::format::hm_secs_key(
            std::time::Duration::from_secs(secs.max(0) as u64),
        ))
    };
    match key {
        InsightKey::CurrentStreak { days, is_record, best } => {
            let body = if *is_record {
                if *days == 1 {
                    "1 day — new record".to_string()
                } else {
                    format!("{days} days — new record")
                }
            } else if *best > *days {
                if *days == 1 {
                    format!("1 day · best was {best}")
                } else {
                    format!("{days} days · best was {best}")
                }
            } else {
                "1 day · keep going".to_string()
            };
            ("Current streak".to_string(), body)
        }
        InsightKey::WeekOverWeek { pct, this_secs, last_secs } => {
            let dir = if *pct >= 0 { "up" } else { "down" };
            (
                "This week's practice".to_string(),
                format!(
                    "{}% {dir} vs last week ({} vs {})",
                    pct.abs(),
                    hm(*this_secs),
                    hm(*last_secs),
                ),
            )
        }
        InsightKey::MonthTrend { pct, this_secs, last_secs } => {
            let title = if *pct >= 0 {
                "Practising more"
            } else {
                "Practising less"
            };
            (
                title.to_string(),
                format!(
                    "{pct:+}% vs last month ({} vs {})",
                    hm(*this_secs),
                    hm(*last_secs),
                ),
            )
        }
        InsightKey::PreferredTime { bucket, pct } => {
            let when = match bucket {
                HourBucket::Morning => "morning",
                HourBucket::Afternoon => "afternoon",
                HourBucket::Evening => "evening",
            };
            (
                "Preferred time".to_string(),
                format!("{pct}% of sessions are in the {when}"),
            )
        }
        InsightKey::TypicalSession { duration_secs } => (
            "Typical session".to_string(),
            format!("About {}", hm(*duration_secs)),
        ),
        InsightKey::LongestSession { duration_secs, start_unix } => {
            use chrono::TimeZone;
            let when = chrono::Local
                .timestamp_opt(*start_unix, 0)
                .single()
                .map(|d| d.format("%b %-d").to_string());
            let body = match when {
                Some(d) => format!("{} on {d}", hm(*duration_secs)),
                None => hm(*duration_secs),
            };
            ("Longest session".to_string(), body)
        }
        InsightKey::NextMilestone { target, remaining } => {
            let body = if *remaining == 1 {
                format!("1 session to your {target}th")
            } else {
                format!("{remaining} sessions to your {target}th")
            };
            ("Next milestone".to_string(), body)
        }
        InsightKey::DailyRhythm { avg_secs } => (
            "Daily rhythm".to_string(),
            format!("{} average over last 7 days", hm(*avg_secs)),
        ),
        InsightKey::NoData => (
            "No sessions yet".to_string(),
            "Complete a meditation to start seeing insights here"
                .to_string(),
        ),
    }
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

/// Persisted Timer-mode countdown length. Mirrors GTK's
/// `timer_session_secs` settings key + `set_countdown_target` /
/// `load_timer_settings` (`meditate-gtk/src/timer/imp.rs`):
/// every duration commit writes it, startup restores it.
/// Defaults to `TIMER_DEFAULT_SECS` = 10 min when missing.
/// (Timer mode previously kept this in-memory only, so it reset
/// on every app restart — the bug Janek hit.)
#[cfg(target_os = "android")]
fn read_timer_session_secs() -> u32 {
    let Some(db_arc) = DATABASE.get() else {
        return meditate_core::session::TIMER_DEFAULT_SECS;
    };
    let Ok(guard) = db_arc.lock() else {
        return meditate_core::session::TIMER_DEFAULT_SECS;
    };
    let Some(db) = guard.as_ref() else {
        return meditate_core::session::TIMER_DEFAULT_SECS;
    };
    meditate_core::settings_keys::read_u32(
        db,
        "timer_session_secs",
        meditate_core::session::TIMER_DEFAULT_SECS,
    )
}

#[cfg(target_os = "android")]
fn write_timer_session_secs(secs: u32) {
    let Some(db_arc) = DATABASE.get() else { return; };
    let Ok(guard) = db_arc.lock() else { return; };
    let Some(db) = guard.as_ref() else { return; };
    let _ = db.set_setting("timer_session_secs", &secs.to_string());
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
    label_id: Option<i64>,
) -> (Vec<(i64, meditate_core::db::Session)>, bool) {
    let Some(db_arc) = DATABASE.get() else { return (Vec::new(), false); };
    let Ok(guard) = db_arc.lock() else { return (Vec::new(), false); };
    let Some(db) = guard.as_ref() else { return (Vec::new(), false); };
    let filter = meditate_core::db::SessionFilter {
        limit: Some(LOG_PAGE_SIZE),
        offset: Some(offset),
        only_with_notes: notes_only,
        label_id,
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
    let (rows, full) = load_log_page(
        0,
        ui.get_filter_notes_only(),
        filter_label_id(ui),
    );
    *loaded.borrow_mut() = rows;
    ui.set_log_has_more(full);
    render_log_feed(ui, loaded, pending);
}

/// Resolve the Slint `filter-label-id` property (0 = "All
/// labels") into the `Option<i64>` the `SessionFilter` expects.
#[cfg(target_os = "android")]
fn filter_label_id(ui: &MainWindow) -> Option<i64> {
    let id = ui.get_filter_label_id() as i64;
    if id > 0 { Some(id) } else { None }
}

/// Recompute `filter-has-active` from the live filter
/// properties. Drives the "No Sessions Yet" vs "No Matching
/// Sessions" empty-state split. Mirrors GTK's `has_filter`
/// (`notes_only || label_id.is_some()`) at
/// `meditate-gtk/src/log/imp.rs:144`.
#[cfg(target_os = "android")]
fn sync_filter_has_active(ui: &MainWindow) {
    ui.set_filter_has_active(
        ui.get_filter_notes_only() || filter_label_id(ui).is_some(),
    );
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
    let (rows, full) = load_log_page(
        offset,
        ui.get_filter_notes_only(),
        filter_label_id(ui),
    );
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

/// Ordered (rowid, name) list of all labels — index 0 in the
/// filter dropdown is the synthetic "All labels" entry, so the
/// returned Vec maps 1:1 onto dropdown indices 1..=N. Mirrors
/// GTK's `refresh_filter_labels` model build at
/// `meditate-gtk/src/log/imp.rs:305`.
#[cfg(target_os = "android")]
fn all_labels_ordered() -> Vec<(i64, String)> {
    let Some(db_arc) = DATABASE.get() else { return Vec::new(); };
    let Ok(guard) = db_arc.lock() else { return Vec::new(); };
    let Some(db) = guard.as_ref() else { return Vec::new(); };
    meditate_core::db::list_labels_from_db(db)
        .unwrap_or_default()
        .into_iter()
        .map(|l| (l.id, l.name))
        .collect()
}

/// Rebuild the Log-filter label dropdown: a synthetic "All
/// labels" row followed by every label. Also re-syncs
/// `filter-label-index` so the dropdown shows the currently
/// active filter when the sheet reopens.
#[cfg(target_os = "android")]
fn refresh_filter_label_items(ui: &MainWindow) {
    let labels = all_labels_ordered();
    let active_id = ui.get_filter_label_id() as i64;
    let mut items: Vec<MenuItem> = Vec::with_capacity(labels.len() + 1);
    items.push(MenuItem {
        text: "All labels".into(),
        enabled: true,
        ..Default::default()
    });
    let mut active_index = 0;
    for (i, (id, name)) in labels.iter().enumerate() {
        if *id == active_id {
            active_index = (i + 1) as i32;
        }
        items.push(MenuItem {
            text: name.clone().into(),
            enabled: true,
            ..Default::default()
        });
    }
    ui.set_filter_label_items(
        std::rc::Rc::new(slint::VecModel::from(items)).into(),
    );
    ui.set_filter_label_index(active_index);
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

/// Read a global (non-per-mode) string setting. Bell settings
/// (`starting_bell_active` / `_sound`, `end_bell_active` /
/// `_sound`) are app-wide, not per-mode — same as the GTK
/// shell's plain `db.get_setting` keys.
#[cfg(target_os = "android")]
fn read_global_setting(key: &str, default: &str) -> String {
    let Some(db_arc) = DATABASE.get() else { return default.to_string(); };
    let Ok(guard) = db_arc.lock() else { return default.to_string(); };
    let Some(db) = guard.as_ref() else { return default.to_string(); };
    db.get_setting(key, default)
        .unwrap_or_else(|_| default.to_string())
}

#[cfg(target_os = "android")]
fn write_global_setting(key: &str, value: &str) {
    if let Some(db_arc) = DATABASE.get() {
        if let Ok(guard) = db_arc.lock() {
            if let Some(db) = guard.as_ref() {
                let _ = db.set_setting(key, value);
            }
        }
    }
}

/// Resolve a bell-sound uuid to its display name (empty string
/// if the row is gone — a deleted custom sound that a setting
/// still points at; the chooser re-pick fixes it).
#[cfg(target_os = "android")]
fn bell_sound_name(uuid: &str) -> String {
    let Some(db_arc) = DATABASE.get() else { return String::new(); };
    let Ok(guard) = db_arc.lock() else { return String::new(); };
    let Some(db) = guard.as_ref() else { return String::new(); };
    db.list_bell_sounds()
        .ok()
        .and_then(|v| {
            v.into_iter()
                .find(|b| b.uuid.to_string() == uuid)
                .map(|b| b.name)
        })
        .unwrap_or_default()
}

/// Resolve a bell-sound uuid to its on-disk file path (the
/// absolute `<data_dir>/sounds/*.ogg` `sounds::extract_and_seed`
/// wrote). Empty when the row is gone — `audio::play` no-ops on
/// an empty path. Sibling of `bell_sound_name`.
#[cfg(target_os = "android")]
fn bell_sound_path(uuid: &str) -> String {
    let Some(db_arc) = DATABASE.get() else { return String::new(); };
    let Ok(guard) = db_arc.lock() else { return String::new(); };
    let Some(db) = guard.as_ref() else { return String::new(); };
    db.list_bell_sounds()
        .ok()
        .and_then(|v| {
            v.into_iter()
                .find(|b| b.uuid.to_string() == uuid)
                .map(|b| b.file_path)
        })
        .unwrap_or_default()
}

/// Resolve a vibration-pattern uuid to its display name (empty
/// when the row is gone). Pattern sibling of `bell_sound_name`.
#[cfg(target_os = "android")]
fn pattern_name(uuid: &str) -> String {
    let Some(db_arc) = DATABASE.get() else { return String::new(); };
    let Ok(guard) = db_arc.lock() else { return String::new(); };
    let Some(db) = guard.as_ref() else { return String::new(); };
    match meditate_core::bells::resolve_pattern_name(db, uuid) {
        meditate_core::bells::ResolvedName::Resolved(n) => n,
        meditate_core::bells::ResolvedName::Missing => String::new(),
    }
}

/// SignalMode db-string → CompactToggle index (0 Sound /
/// 1 Vibration / 2 Both — the GTK ToggleGroup order). Unknown
/// values fall back to Sound, matching `read_signal_mode`'s
/// default contract.
#[cfg(target_os = "android")]
fn signal_mode_index(db_str: &str) -> i32 {
    use meditate_core::bells::SignalMode;
    match SignalMode::from_db_str(db_str) {
        Some(SignalMode::Vibration) => 1,
        Some(SignalMode::Both) => 2,
        _ => 0,
    }
}

/// CompactToggle index → SignalMode db string. Inverse of
/// `signal_mode_index`; out-of-range indices clamp to Sound.
#[cfg(target_os = "android")]
fn signal_mode_db_str(index: i32) -> &'static str {
    use meditate_core::bells::SignalMode;
    match index {
        1 => SignalMode::Vibration.as_db_str(),
        2 => SignalMode::Both.as_db_str(),
        _ => SignalMode::Sound.as_db_str(),
    }
}

/// CompactToggle index → `SignalMode`. The interval-bell editor
/// stores the mode as the enum (DB column), not a settings
/// string, so it needs the typed value rather than `*_db_str`.
#[cfg(target_os = "android")]
fn signal_mode_from_index(index: i32) -> meditate_core::bells::SignalMode {
    use meditate_core::bells::SignalMode;
    match index {
        1 => SignalMode::Vibration,
        2 => SignalMode::Both,
        _ => SignalMode::Sound,
    }
}

/// Push the current bell settings into the Setup Bells-group
/// props: enable switches from `*_bell_active`, body subtitles
/// from the resolved `*_bell_sound` name (defaulting to the
/// bundled bowl). Mirrors GTK's `refresh_*_bell_sound_subtitle`
/// + enable-expansion init.
#[cfg(target_os = "android")]
fn refresh_bell_rows(ui: &MainWindow) {
    ui.set_starting_bell_active(
        read_global_setting("starting_bell_active", "false") == "true",
    );
    ui.set_end_bell_active(
        read_global_setting("end_bell_active", "false") == "true",
    );
    let ss = read_global_setting(
        "starting_bell_sound",
        meditate_core::seeds::BUNDLED_BOWL_UUID,
    );
    let es = read_global_setting(
        "end_bell_sound",
        meditate_core::seeds::BUNDLED_BOWL_UUID,
    );
    ui.set_starting_bell_sound_name(bell_sound_name(&ss).into());
    ui.set_end_bell_sound_name(bell_sound_name(&es).into());

    // Signal Type + Pattern (B-2b). Defaults mirror core's
    // `read_signal_mode(.., SignalMode::Sound)` and the
    // `*_bell_pattern` → BUNDLED_PATTERN_PULSE_UUID fallback.
    ui.set_starting_bell_signal_mode(signal_mode_index(&read_global_setting(
        "starting_bell_signal_mode",
        meditate_core::bells::SignalMode::Sound.as_db_str(),
    )));
    ui.set_end_bell_signal_mode(signal_mode_index(&read_global_setting(
        "end_bell_signal_mode",
        meditate_core::bells::SignalMode::Sound.as_db_str(),
    )));
    let sp = read_global_setting(
        "starting_bell_pattern",
        meditate_core::seeds::BUNDLED_PATTERN_PULSE_UUID,
    );
    let ep = read_global_setting(
        "end_bell_pattern",
        meditate_core::seeds::BUNDLED_PATTERN_PULSE_UUID,
    );
    ui.set_starting_bell_pattern_name(pattern_name(&sp).into());
    ui.set_end_bell_pattern_name(pattern_name(&ep).into());

    // Preparation Time (B-5b).
    ui.set_prep_time_active(
        read_global_setting("preparation_time_active", "false") == "true",
    );
    let secs = meditate_core::format::parse_prep_secs(&read_global_setting(
        "preparation_time_secs",
        &meditate_core::format::PREP_SECS_DEFAULT.to_string(),
    ));
    ui.set_prep_time_secs(secs as i32);

    // Interval Bells enable (B-5c-1) + count subtitle (B-5c-2).
    ui.set_interval_bells_active(
        read_global_setting("interval_bells_active", "false") == "true",
    );
    if let Some(db_arc) = DATABASE.get() {
        if let Ok(guard) = db_arc.lock() {
            if let Some(db) = guard.as_ref() {
                ui.set_interval_bells_summary(
                    interval_bells_summary(db).into(),
                );
            }
        }
    }

    // Box Breath per-phase cues (B-7).
    refresh_boxbreath_cues(ui);
}

/// Map a Box-Breath phase tag to its core id. Tags match the
/// Slint `bbc-<tag>-*` property prefixes.
#[cfg(target_os = "android")]
fn bb_phase(tag: &str) -> meditate_core::db::BoxBreathPhaseId {
    use meditate_core::db::BoxBreathPhaseId as P;
    match tag {
        "holdin" => P::HoldIn,
        "out" => P::Out,
        "holdout" => P::HoldOut,
        _ => P::In,
    }
}

/// Read a Box-Breath phase's persisted sound / pattern uuid (for
/// seeding the chooser check-mark). Defaults to the bundled
/// bowl / pulse when the row is somehow missing.
#[cfg(target_os = "android")]
fn bb_phase_uuids(
    phase: meditate_core::db::BoxBreathPhaseId,
) -> (String, String) {
    let bowl = meditate_core::seeds::BUNDLED_BOWL_UUID.to_string();
    let pulse = meditate_core::seeds::BUNDLED_PATTERN_PULSE_UUID.to_string();
    let Some(db_arc) = DATABASE.get() else { return (bowl, pulse); };
    let Ok(guard) = db_arc.lock() else { return (bowl, pulse); };
    let Some(db) = guard.as_ref() else { return (bowl, pulse); };
    match db.get_box_breath_phase(phase) {
        Ok(Some(r)) => {
            (r.sound_uuid.to_string(), r.pattern_uuid.to_string())
        }
        _ => (bowl, pulse),
    }
}

/// Read-modify-write one Box-Breath phase row. `set_box_breath_
/// phase` is a whole-row setter, so we load the current row and
/// override only the `Some(_)` fields — the Android analogue of
/// GTK's per-field phase writes. Missing row falls back to the
/// schema defaults (seeded, so this is just defensive).
#[cfg(target_os = "android")]
fn write_bb_phase(
    phase: meditate_core::db::BoxBreathPhaseId,
    enabled: Option<bool>,
    signal_mode: Option<meditate_core::bells::SignalMode>,
    sound_uuid: Option<&str>,
    pattern_uuid: Option<&str>,
) {
    let Some(db_arc) = DATABASE.get() else { return; };
    let Ok(guard) = db_arc.lock() else { return; };
    let Some(db) = guard.as_ref() else { return; };
    let (ce, csm, csu, cpu) = match db.get_box_breath_phase(phase) {
        Ok(Some(r)) => (
            r.enabled,
            r.signal_mode,
            r.sound_uuid.to_string(),
            r.pattern_uuid.to_string(),
        ),
        _ => (
            false,
            meditate_core::bells::SignalMode::Sound,
            meditate_core::seeds::BUNDLED_BOWL_UUID.to_string(),
            meditate_core::seeds::BUNDLED_PATTERN_PULSE_UUID.to_string(),
        ),
    };
    let e = enabled.unwrap_or(ce);
    let sm = signal_mode.unwrap_or(csm);
    let su = sound_uuid.map_or(csu, str::to_string);
    let pu = pattern_uuid.map_or(cpu, str::to_string);
    let _ = db.set_box_breath_phase(phase, e, sm, &su, &pu);
}

/// Push every Box-Breath phase's persisted cue state into the
/// Setup props (master toggle + per-phase enable / Type / sound
/// + pattern names). Names are resolved through the *locked* db
/// (`resolve_*_name`) — calling `bell_sound_name` / `pattern_name`
/// here would re-lock the same Mutex and deadlock.
#[cfg(target_os = "android")]
fn refresh_boxbreath_cues(ui: &MainWindow) {
    use meditate_core::bells::ResolvedName;
    use meditate_core::db::BoxBreathPhaseId as P;
    ui.set_boxbreath_cues_active(
        read_global_setting("boxbreath_cues_active", "false") == "true",
    );
    let Some(db_arc) = DATABASE.get() else { return; };
    let Ok(guard) = db_arc.lock() else { return; };
    let Some(db) = guard.as_ref() else { return; };
    let name = |n: ResolvedName| match n {
        ResolvedName::Resolved(s) => s,
        ResolvedName::Missing => String::new(),
    };
    for p in [P::In, P::HoldIn, P::Out, P::HoldOut] {
        let Ok(Some(r)) = db.get_box_breath_phase(p) else { continue; };
        let si = signal_mode_index(r.signal_mode.as_db_str());
        let sn = name(meditate_core::bells::resolve_sound_name(
            db,
            &r.sound_uuid.to_string(),
        ));
        let pn = name(meditate_core::bells::resolve_pattern_name(
            db,
            &r.pattern_uuid.to_string(),
        ));
        match p {
            P::In => {
                ui.set_bbc_in_active(r.enabled);
                ui.set_bbc_in_signal_mode(si);
                ui.set_bbc_in_sound_name(sn.into());
                ui.set_bbc_in_pattern_name(pn.into());
            }
            P::HoldIn => {
                ui.set_bbc_holdin_active(r.enabled);
                ui.set_bbc_holdin_signal_mode(si);
                ui.set_bbc_holdin_sound_name(sn.into());
                ui.set_bbc_holdin_pattern_name(pn.into());
            }
            P::Out => {
                ui.set_bbc_out_active(r.enabled);
                ui.set_bbc_out_signal_mode(si);
                ui.set_bbc_out_sound_name(sn.into());
                ui.set_bbc_out_pattern_name(pn.into());
            }
            P::HoldOut => {
                ui.set_bbc_holdout_active(r.enabled);
                ui.set_bbc_holdout_signal_mode(si);
                ui.set_bbc_holdout_sound_name(sn.into());
                ui.set_bbc_holdout_pattern_name(pn.into());
            }
        }
    }
}

/// "N enabled" / "1 enabled" / "None enabled" for the Manage
/// Bells row subtitle. Mirrors GTK's `intervals_count_subtitle`
/// (`meditate-gtk/src/timer/imp.rs:4031`); the count +
/// bucketing live in core. `DisplayMode::Countdown` — the
/// subtitle is informational and the per-mode stopwatch state
/// isn't wired into this surface yet (FixedFromEnd bells only
/// drop out of the count in a stopwatch session).
#[cfg(target_os = "android")]
fn interval_bells_summary(db: &meditate_core::db::Database) -> String {
    use meditate_core::format::IntervalsCountKey;
    let n = meditate_core::bells::interval_bells_count(
        db,
        meditate_core::bells::DisplayMode::Countdown,
    );
    match meditate_core::format::intervals_count_key(n) {
        IntervalsCountKey::None => "None enabled".to_string(),
        IntervalsCountKey::One => "1 enabled".to_string(),
        IntervalsCountKey::Many(n) => format!("{n} enabled"),
    }
}

/// Render an interval bell's summary title. Inline English —
/// i18n is shell-deferred on Android (same as the snackbar /
/// stats copy). The bucket decision lives in core
/// (`meditate_core::bells::bell_title_key`); GTK uses "±" but
/// the FP5 Android-15 font's symbol coverage is unreliable, so
/// Android uses ASCII "+/-" (see the no-Unicode-glyphs rule).
/// Mirrors GTK's `bell_title` at `meditate-gtk/src/bells.rs`.
#[cfg(target_os = "android")]
fn render_bell_title(bell: &meditate_core::db::IntervalBell) -> String {
    use meditate_core::bells::BellTitleKey;
    match meditate_core::bells::bell_title_key(bell) {
        BellTitleKey::EveryNMin { minutes } => {
            format!("Every {minutes} min")
        }
        BellTitleKey::EveryNMinWithJitter { minutes, jitter_pct } => {
            format!("Every {minutes} min +/-{jitter_pct}%")
        }
        BellTitleKey::AtNMin { minutes } => format!("At {minutes} min"),
        BellTitleKey::NMinBeforeEnd { minutes } => {
            if minutes == 1 {
                "1 min before end".to_string()
            } else {
                format!("{minutes} min before end")
            }
        }
    }
}

/// Fill the Interval Bells library list (read-only, B-5c-1).
/// Mirrors GTK's `rebuild_list` over `db.list_interval_bells`.
#[cfg(target_os = "android")]
fn populate_interval_bells(ui: &MainWindow) {
    let Some(db_arc) = DATABASE.get() else { return; };
    let Ok(guard) = db_arc.lock() else { return; };
    let Some(db) = guard.as_ref() else { return; };
    let items: Vec<IntervalBellRow> = db
        .list_interval_bells()
        .unwrap_or_default()
        .into_iter()
        .map(|b| {
            let sound = db
                .list_bell_sounds()
                .unwrap_or_default()
                .into_iter()
                .find(|s| s.uuid.to_string() == b.sound_uuid.to_string())
                .map(|s| s.name)
                .unwrap_or_default();
            IntervalBellRow {
                uuid: b.uuid.to_string().into(),
                title: render_bell_title(&b).into(),
                sound: sound.into(),
            }
        })
        .collect();
    ui.set_interval_bell_items(
        std::rc::Rc::new(slint::VecModel::from(items)).into(),
    );
    ui.set_interval_bells_summary(interval_bells_summary(db).into());
}

/// Fill the bell-chooser overlay with the sounds in `category`,
/// the row matching `current_uuid` check-marked. The Starting /
/// End / interval-editor pickers pass `General`; Box-Breath
/// phase pickers pass `BoxBreath` — mirrors GTK filtering its
/// phase Sound chooser to `BellSoundCategory::BoxBreath` (that
/// category is currently empty in both shells until voice-cue
/// audio is sourced — a documented TODO, not a bug).
#[cfg(target_os = "android")]
fn populate_bell_chooser(
    ui: &MainWindow,
    current_uuid: &str,
    category: meditate_core::db::BellSoundCategory,
) {
    let Some(db_arc) = DATABASE.get() else { return; };
    let Ok(guard) = db_arc.lock() else { return; };
    let Some(db) = guard.as_ref() else { return; };
    let items: Vec<NameChoice> = db
        .list_bell_sounds_for_category(category)
        .unwrap_or_default()
        .into_iter()
        .map(|b| {
            let u = b.uuid.to_string();
            NameChoice {
                selected: u == current_uuid,
                uuid: u.into(),
                name: b.name.into(),
            }
        })
        .collect();
    ui.set_bell_chooser_items(
        std::rc::Rc::new(slint::VecModel::from(items)).into(),
    );
}

/// Fill the pattern-chooser overlay from
/// `list_vibration_patterns_from_db` (custom rows first, then
/// the bundled set — the list helper already orders it), the
/// row matching `current_uuid` check-marked. Per-row Play/Stop
/// preview is wired separately via `pattern-preview-toggle`.
#[cfg(target_os = "android")]
fn populate_pattern_chooser(ui: &MainWindow, current_uuid: &str) {
    let Some(db_arc) = DATABASE.get() else { return; };
    let Ok(guard) = db_arc.lock() else { return; };
    let Some(db) = guard.as_ref() else { return; };
    let items: Vec<NameChoice> =
        meditate_core::db::list_vibration_patterns_from_db(db)
            .unwrap_or_default()
            .into_iter()
            .map(|p| {
                let u = p.uuid.to_string();
                NameChoice {
                    selected: u == current_uuid,
                    uuid: u.into(),
                    name: p.name.into(),
                }
            })
            .collect();
    ui.set_pattern_chooser_items(
        std::rc::Rc::new(slint::VecModel::from(items)).into(),
    );
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
    let timer_session_secs: Rc<Cell<u32>> = Rc::new(Cell::new({
        #[cfg(target_os = "android")]
        {
            read_timer_session_secs()
        }
        #[cfg(not(target_os = "android"))]
        {
            (DEFAULT_HOURS as u32) * 3600 + (DEFAULT_MINUTES as u32) * 60
        }
    }));

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

    // Which action the currently-shown Snackbar's Undo button
    // performs (L-6). The Snackbar surface is shared between the
    // delete-undo flow (L-3) and the crash-recovery flow; the
    // Undo handler branches on this. `Some(uuid)` ⇒ a recovery
    // snackbar is up and Undo deletes that session by uuid;
    // `None` ⇒ the delete-undo flow (restore pending deletes).
    // A trash tap clears this back to None so a delete snackbar
    // raised while a recovery one is visible behaves correctly.
    #[cfg(target_os = "android")]
    let recovery_uuid: Rc<RefCell<Option<String>>> =
        Rc::new(RefCell::new(None));

    // Third discriminator for the shared single-slot snackbar
    // (P-3): a pending preset-apply Undo. `Some((snapshot_json,
    // mode))` ⇒ a "'X' applied" snackbar is up and Undo
    // re-applies that pre-apply snapshot via `apply_preset_json`
    // (mirrors GTK's apply-toast Undo calling
    // `apply_config(&snapshot)`). The Undo handler checks this
    // FIRST; raising a delete / recovery snackbar clears it so
    // the newest flow owns the single slot's Undo.
    #[cfg(target_os = "android")]
    let pending_preset_undo: Rc<
        RefCell<Option<(String, meditate_core::SessionMode)>>,
    > = Rc::new(RefCell::new(None));

    // P-4 Save flow: the snapshot captured when "Save Settings"
    // opened the chooser (GTK's `ChooserMode::Save { snapshot }`).
    // Consumed by create-confirm (new preset) and override-confirm
    // (replace an existing preset's config).
    #[cfg(target_os = "android")]
    let pending_save_snapshot: Rc<
        RefCell<Option<(String, meditate_core::SessionMode)>>,
    > = Rc::new(RefCell::new(None));
    // Which preset the override-confirm dialog will replace.
    #[cfg(target_os = "android")]
    let pending_override_uuid: Rc<RefCell<Option<String>>> =
        Rc::new(RefCell::new(None));

    // P-5 Manage state. `rename_preset_uuid` / `delete_preset_uuid`
    // pin which row the rename / delete dialog targets. The
    // shared single-slot snackbar gains two more discriminators
    // (same pattern as `pending_preset_undo`): a deleted preset
    // to re-insert on Undo (full row, GTK's
    // `insert_preset_with_uuid` resurrection), and an override's
    // prior config_json to restore on Undo (the P-4 deferral,
    // folded in here). The undo handler checks them in priority
    // order; every snackbar raise clears the others (single slot).
    #[cfg(target_os = "android")]
    let rename_preset_uuid: Rc<RefCell<Option<String>>> =
        Rc::new(RefCell::new(None));
    #[cfg(target_os = "android")]
    let delete_preset_uuid: Rc<RefCell<Option<String>>> =
        Rc::new(RefCell::new(None));
    #[cfg(target_os = "android")]
    let pending_preset_delete: Rc<
        RefCell<
            Option<(
                String,
                String,
                meditate_core::SessionMode,
                bool,
                String,
            )>,
        >,
    > = Rc::new(RefCell::new(None));
    #[cfg(target_os = "android")]
    let pending_override_restore: Rc<
        RefCell<Option<(String, String, meditate_core::SessionMode)>>,
    > = Rc::new(RefCell::new(None));

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

    // Seed the stepper-driven duration from the persisted Timer
    // length (mirrors GTK's `load_timer_settings` restoring
    // `timer_session_secs` at startup) rather than the bare
    // default — Timer mode is the launch mode, so this is what
    // the user last set. The tick loop further down refreshes the
    // Setup hero every 200 ms so stepper changes flow into the
    // big mm:ss readout without a dedicated change-notification path.
    {
        let secs = timer_session_secs.get();
        ui.set_setup_hours((secs / 3600) as i32);
        ui.set_setup_minutes(((secs % 3600) / 60) as i32);
    }

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
            let prev = std::mem::replace(&mut *s, AppState::idle());
            let transition = if was_active {
                // Active → pause / resume (shape unused — Session
                // remembers its own shape).
                prev.toggle(shape, now)
            } else {
                // Idle/Finished → start. Build the real
                // SessionSettings from the DB so core has the
                // actual bell config and emits FireEndBell /
                // FireBell / FireStartingBell — mirrors GTK's
                // build_timer_settings. Host build keeps the bare
                // default-settings start (no DB).
                #[cfg(target_os = "android")]
                {
                    let _ = prev;
                    let settings = build_session_settings(
                        shape,
                        ui.get_stopwatch_on(),
                        TimerMode::from_chip_index(ui.get_setup_mode())
                            .into(),
                    );
                    AppState::start_session(settings, now)
                }
                #[cfg(not(target_os = "android"))]
                {
                    prev.toggle(shape, now)
                }
            };
            #[cfg(target_os = "android")]
            dispatch_effects(&transition.effects);
            *s = transition.state;
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
            let transition = std::mem::replace(&mut *s, AppState::idle()).stop();
            #[cfg(target_os = "android")]
            dispatch_effects(&transition.effects);
            *s = transition.state;
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

    // Overtime Finish / Add (B-6c). Both end the session from the
    // Overtime phase and land on the Done screen — same
    // post-processing as Stop (pending_done stash, snapshot
    // teardown, elapsed readout, label mirror, service stop) but
    // the recorded duration comes from core's EndSession effect:
    // Finish = planned target, Add = full elapsed incl. overtime.
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
        ui.on_finish_tap(move || {
            let now = now_since_epoch();
            let mut s = state.borrow_mut();
            let pre_elapsed = match &*s {
                AppState::Active(session) => session.elapsed(now).as_secs() as i64,
                _ => 0,
            };
            let was_active = s.is_active();
            let transition =
                std::mem::replace(&mut *s, AppState::idle()).finish_overtime();
            #[cfg(target_os = "android")]
            dispatch_effects(&transition.effects);
            #[cfg(target_os = "android")]
            let final_secs = end_session_duration(&transition.effects)
                .map_or(pre_elapsed, |d| d as i64);
            #[cfg(not(target_os = "android"))]
            let final_secs = pre_elapsed;
            *s = transition.state;
            let is_active = s.is_active();
            if was_active && !is_active {
                #[cfg(target_os = "android")]
                if let Some(unix_start) = session_start_unix.take() {
                    pending_done.set(Some((unix_start, final_secs)));
                }
                #[cfg(target_os = "android")]
                {
                    snapshot_timer_ref.stop();
                    clear_session_in_progress_snapshot();
                }
                if let Some(ui) = weak.upgrade() {
                    ui.set_elapsed_text(
                        meditate_core::format::format_time(
                            Duration::from_secs(final_secs.max(0) as u64),
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
        ui.on_add_tap(move || {
            let now = now_since_epoch();
            let mut s = state.borrow_mut();
            let pre_elapsed = match &*s {
                AppState::Active(session) => session.elapsed(now).as_secs() as i64,
                _ => 0,
            };
            let was_active = s.is_active();
            let transition =
                std::mem::replace(&mut *s, AppState::idle()).add_overtime(now);
            #[cfg(target_os = "android")]
            dispatch_effects(&transition.effects);
            #[cfg(target_os = "android")]
            let final_secs = end_session_duration(&transition.effects)
                .map_or(pre_elapsed, |d| d as i64);
            #[cfg(not(target_os = "android"))]
            let final_secs = pre_elapsed;
            *s = transition.state;
            let is_active = s.is_active();
            if was_active && !is_active {
                #[cfg(target_os = "android")]
                if let Some(unix_start) = session_start_unix.take() {
                    pending_done.set(Some((unix_start, final_secs)));
                }
                #[cfg(target_os = "android")]
                {
                    snapshot_timer_ref.stop();
                    clear_session_in_progress_snapshot();
                }
                if let Some(ui) = weak.upgrade() {
                    ui.set_elapsed_text(
                        meditate_core::format::format_time(
                            Duration::from_secs(final_secs.max(0) as u64),
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
        #[cfg(target_os = "android")]
        let timer_session_secs = timer_session_secs.clone();
        timer.start(slint::TimerMode::Repeated, TICK, move || {
            // Warm-process widget deep-link (W-4). Polled here
            // because NativeActivity never hands native code an
            // onNewIntent; the drop file is single-consumption so
            // this is a no-op syscall on every normal frame. Done
            // before the `state` borrow below — the helper's
            // invoke_action_tap takes its own borrow — and the
            // frame is skipped when it consumed a tap.
            #[cfg(target_os = "android")]
            if let Some(ui) = weak.upgrade() {
                if try_widget_deep_link(&ui, &timer_session_secs, &state) {
                    return;
                }
            }
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
            let transition = std::mem::replace(&mut *s, AppState::idle()).tick(now);
            #[cfg(target_os = "android")]
            dispatch_effects(&transition.effects);
            // Overtime Add-button label tracks the running
            // overtime delta (core's UpdateOvertimeLabel each
            // tick), exactly like GTK's per-tick relabel.
            #[cfg(target_os = "android")]
            if let Some(lbl) = overtime_add_label(&transition.effects) {
                if let Some(ui) = weak.upgrade() {
                    ui.set_overtime_add_label(lbl.into());
                }
            }
            *s = transition.state;
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
                // Starred presets are per-mode (P-2).
                refresh_preset_chips(&ui, core_mode);
            }
            let _ = (weak.clone(), timer_session_secs.clone());
        });
    }

    // Preset row tap → apply (P-3). Mirrors GTK's
    // `on_preset_row_activated`: mode-guard, snapshot the
    // pre-apply state, `apply_preset_json`, then raise the
    // shared single-slot snackbar ("'X' applied" + Undo) exactly
    // the way the delete / recovery flows use it — `delete_timer`
    // restarted single-shot for auto-dismiss, `pending_preset_undo`
    // discriminator routing the shared Undo handler. Undo
    // re-applies the snapshot through the same `apply_preset_json`
    // (GTK's Undo calls `apply_config(&snapshot)`).
    {
        let weak = ui.as_weak();
        let current_mode = current_mode.clone();
        #[cfg(target_os = "android")]
        let timer_session_secs = timer_session_secs.clone();
        #[cfg(target_os = "android")]
        let pending_preset_undo = pending_preset_undo.clone();
        #[cfg(target_os = "android")]
        let pending_preset_delete = pending_preset_delete.clone();
        #[cfg(target_os = "android")]
        let pending_override_restore = pending_override_restore.clone();
        #[cfg(target_os = "android")]
        let recovery_uuid = recovery_uuid.clone();
        #[cfg(target_os = "android")]
        let delete_timer: &'static slint::Timer = delete_timer;
        ui.on_preset_chip_tap(move |uuid| {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                let core_mode: meditate_core::SessionMode =
                    current_mode.get().into();
                if !presets_supported(core_mode) {
                    return;
                }
                let Some(preset) = find_preset_by_uuid(uuid.as_str())
                else {
                    return;
                };
                // Stale cross-mode tap (callback retained across a
                // mode switch) — refuse rather than mutate Setup.
                if preset.mode != core_mode {
                    return;
                }
                // Snapshot BEFORE apply so Undo can restore it.
                let snapshot = snapshot_setup_json(
                    &ui,
                    core_mode,
                    timer_session_secs.get(),
                );
                if !apply_preset_json(
                    &ui,
                    &preset.config_json,
                    core_mode,
                    &timer_session_secs,
                ) {
                    return;
                }
                meditate_core::log(
                    "preset.apply",
                    &format!("applied uuid={uuid}"),
                );

                // Raise the shared snackbar with the preset-Undo
                // discriminator (clears the recovery one — single
                // slot, newest flow owns Undo). No snapshot ⇒ show
                // the message without Undo wiring rather than skip.
                recovery_uuid.borrow_mut().take();
                pending_preset_delete.borrow_mut().take();
                pending_override_restore.borrow_mut().take();
                *pending_preset_undo.borrow_mut() =
                    snapshot.map(|j| (j, core_mode));
                ui.set_snackbar_text(
                    format!("'{}' applied", preset.name).into(),
                );
                ui.set_snackbar_visible(true);
                let weak_inner = ui.as_weak();
                let ppu = pending_preset_undo.clone();
                delete_timer.start(
                    slint::TimerMode::SingleShot,
                    std::time::Duration::from_secs(5),
                    move || {
                        ppu.borrow_mut().take();
                        if let Some(ui) = weak_inner.upgrade() {
                            ui.set_snackbar_visible(false);
                        }
                    },
                );
            }
            let _ = (weak.clone(), uuid, current_mode.get());
        });
    }

    // ── Save / Manage presets (P-4) ─────────────────────────────
    // Save Settings: snapshot current Setup, open the shared
    // chooser in Save mode (Create row + tap-to-Override).
    // Manage: same chooser, Manage mode (rename/delete/star are
    // P-5; rows inert here). Mirrors GTK's save/manage btn →
    // push_presets_chooser(ChooserMode::{Save,Manage}).
    {
        let weak = ui.as_weak();
        let current_mode = current_mode.clone();
        #[cfg(target_os = "android")]
        let timer_session_secs = timer_session_secs.clone();
        #[cfg(target_os = "android")]
        let pending_save_snapshot = pending_save_snapshot.clone();
        ui.on_save_settings_tap(move || {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                let core_mode: meditate_core::SessionMode =
                    current_mode.get().into();
                if !presets_supported(core_mode) {
                    return;
                }
                let snap = snapshot_setup_json(
                    &ui,
                    core_mode,
                    timer_session_secs.get(),
                );
                *pending_save_snapshot.borrow_mut() =
                    snap.map(|j| (j, core_mode));
                populate_preset_chooser(&ui, core_mode);
                ui.set_preset_chooser_save_mode(true);
                ui.set_preset_chooser_page(true);
            }
            let _ = (weak.clone(), current_mode.get());
        });
    }
    {
        let weak = ui.as_weak();
        let current_mode = current_mode.clone();
        ui.on_manage_presets_tap(move || {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                let core_mode: meditate_core::SessionMode =
                    current_mode.get().into();
                if !presets_supported(core_mode) {
                    return;
                }
                populate_preset_chooser(&ui, core_mode);
                ui.set_preset_chooser_save_mode(false);
                ui.set_preset_chooser_page(true);
            }
            let _ = (weak.clone(), current_mode.get());
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_preset_chooser_back(move || {
            #[cfg(target_os = "android")]
            if let Some(ui) = weak.upgrade() {
                ui.set_preset_chooser_page(false);
            }
            let _ = weak.clone();
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_preset_chooser_create(move || {
            #[cfg(target_os = "android")]
            if let Some(ui) = weak.upgrade() {
                ui.set_create_preset_text(slint::SharedString::new());
                ui.set_create_preset_valid(false);
                ui.set_create_preset_dialog_open(true);
            }
            let _ = weak.clone();
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_create_preset_changed(move |name| {
            #[cfg(target_os = "android")]
            if let Some(ui) = weak.upgrade() {
                let trimmed = name.trim();
                let v = meditate_core::naming::validate(trimmed, |n| {
                    preset_name_taken(n, "")
                });
                ui.set_create_preset_valid(v.is_savable());
            }
            let _ = (weak.clone(), name);
        });
    }
    {
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let pending_save_snapshot = pending_save_snapshot.clone();
        ui.on_create_preset_confirm(move || {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                let name = ui.get_create_preset_text().trim().to_string();
                // Re-validate (defends a stale Enter / race).
                let v = meditate_core::naming::validate(&name, |n| {
                    preset_name_taken(n, "")
                });
                if !v.is_savable() {
                    return;
                }
                let Some((json, mode)) =
                    pending_save_snapshot.borrow_mut().take()
                else {
                    return;
                };
                let starred =
                    meditate_core::preset_config::default_starred_on_save();
                let res = {
                    let Some(db_arc) = DATABASE.get() else { return; };
                    let Ok(guard) = db_arc.lock() else { return; };
                    let Some(db) = guard.as_ref() else { return; };
                    db.insert_preset(&name, mode, starred, &json)
                };
                match res {
                    Ok(_) => {
                        ui.set_create_preset_dialog_open(false);
                        ui.set_preset_chooser_page(false);
                        refresh_preset_chips(&ui, mode);
                        // New preset is starred by default → it
                        // enters the widget projection. Lock-free
                        // here: the `db` guard above dropped with
                        // the `let res = { … }` scope.
                        refresh_widget();
                    }
                    Err(e) => {
                        // GTK surfaces a duplicate toast; the
                        // generic-message snackbar isn't wired for
                        // a no-Undo info message yet, so log (the
                        // create dialog stays open for a retry).
                        meditate_core::log(
                            "preset.save",
                            &format!("insert_preset FAILED: {e:?}"),
                        );
                    }
                }
            }
            let _ = weak.clone();
        });
    }
    {
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let pending_override_uuid = pending_override_uuid.clone();
        ui.on_preset_chooser_override(move |uuid, name| {
            #[cfg(target_os = "android")]
            if let Some(ui) = weak.upgrade() {
                *pending_override_uuid.borrow_mut() =
                    Some(uuid.to_string());
                ui.set_override_preset_name(name.clone());
                ui.set_override_preset_dialog_open(true);
            }
            let _ = (weak.clone(), uuid, name);
        });
    }
    {
        let weak = ui.as_weak();
        let current_mode = current_mode.clone();
        #[cfg(target_os = "android")]
        let pending_save_snapshot = pending_save_snapshot.clone();
        #[cfg(target_os = "android")]
        let pending_override_uuid = pending_override_uuid.clone();
        #[cfg(target_os = "android")]
        let pending_override_restore = pending_override_restore.clone();
        #[cfg(target_os = "android")]
        let pending_preset_undo = pending_preset_undo.clone();
        #[cfg(target_os = "android")]
        let pending_preset_delete = pending_preset_delete.clone();
        #[cfg(target_os = "android")]
        let recovery_uuid = recovery_uuid.clone();
        #[cfg(target_os = "android")]
        let delete_timer: &'static slint::Timer = delete_timer;
        ui.on_override_preset_confirm(move || {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                let core_mode: meditate_core::SessionMode =
                    current_mode.get().into();
                let Some(uuid) =
                    pending_override_uuid.borrow_mut().take()
                else {
                    return;
                };
                let Some((json, _mode)) =
                    pending_save_snapshot.borrow_mut().take()
                else {
                    return;
                };
                // Capture the prior config BEFORE overwriting so
                // Undo can restore it (GTK's override-Undo).
                let prior =
                    find_preset_by_uuid(&uuid).map(|p| p.config_json);
                {
                    let Some(db_arc) = DATABASE.get() else { return; };
                    let Ok(guard) = db_arc.lock() else { return; };
                    let Some(db) = guard.as_ref() else { return; };
                    if let Err(e) = db.update_preset_config(&uuid, &json) {
                        meditate_core::log(
                            "preset.override",
                            &format!("update_preset_config FAILED: {e:?}"),
                        );
                    }
                }
                ui.set_override_preset_dialog_open(false);
                ui.set_preset_chooser_page(false);
                refresh_preset_chips(&ui, core_mode);
                // Overriding a starred preset rewrites its
                // subtitle on the widget too.
                refresh_widget();

                // Shared snackbar with Undo (restore prior cfg).
                // Single slot → clear the other discriminators.
                recovery_uuid.borrow_mut().take();
                pending_preset_undo.borrow_mut().take();
                pending_preset_delete.borrow_mut().take();
                *pending_override_restore.borrow_mut() =
                    prior.map(|p| (uuid.clone(), p, core_mode));
                ui.set_snackbar_text("Preset overridden".into());
                ui.set_snackbar_visible(true);
                let weak_inner = ui.as_weak();
                let por = pending_override_restore.clone();
                delete_timer.start(
                    slint::TimerMode::SingleShot,
                    std::time::Duration::from_secs(5),
                    move || {
                        por.borrow_mut().take();
                        if let Some(ui) = weak_inner.upgrade() {
                            ui.set_snackbar_visible(false);
                        }
                    },
                );
            }
            let _ = (weak.clone(), current_mode.get());
        });
    }

    // ── Manage-mode row actions (P-5): star / rename / delete ───
    // Mirrors GTK's per-row star toggle + rename/delete buttons.
    {
        let weak = ui.as_weak();
        let current_mode = current_mode.clone();
        ui.on_preset_star_tap(move |uuid| {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                let core_mode: meditate_core::SessionMode =
                    current_mode.get().into();
                let Some(p) = find_preset_by_uuid(uuid.as_str()) else {
                    return;
                };
                let next = meditate_core::db::StarredState::from_flag(
                    !p.is_starred,
                );
                {
                    let Some(db_arc) = DATABASE.get() else { return; };
                    let Ok(guard) = db_arc.lock() else { return; };
                    let Some(db) = guard.as_ref() else { return; };
                    let _ = db.update_preset_starred(uuid.as_str(), next);
                }
                populate_preset_chooser(&ui, core_mode);
                refresh_preset_chips(&ui, core_mode);
                // The whole point of the widget: star/unstar is
                // exactly what adds/removes a widget row.
                refresh_widget();
            }
            let _ = (weak.clone(), uuid, current_mode.get());
        });
    }
    {
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let rename_preset_uuid = rename_preset_uuid.clone();
        ui.on_preset_rename_tap(move |uuid, name| {
            #[cfg(target_os = "android")]
            if let Some(ui) = weak.upgrade() {
                *rename_preset_uuid.borrow_mut() = Some(uuid.to_string());
                ui.set_rename_preset_text(name.clone());
                // Unchanged name is valid (own-uuid exception).
                let v = meditate_core::naming::validate(
                    name.trim(),
                    |n| preset_name_taken(n, uuid.as_str()),
                );
                ui.set_rename_preset_valid(v.is_savable());
                ui.set_rename_preset_dialog_open(true);
            }
            let _ = (weak.clone(), uuid, name);
        });
    }
    {
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let rename_preset_uuid = rename_preset_uuid.clone();
        ui.on_rename_preset_changed(move |name| {
            #[cfg(target_os = "android")]
            if let Some(ui) = weak.upgrade() {
                let uuid =
                    rename_preset_uuid.borrow().clone().unwrap_or_default();
                let v = meditate_core::naming::validate(
                    name.trim(),
                    |n| preset_name_taken(n, &uuid),
                );
                ui.set_rename_preset_valid(v.is_savable());
            }
            let _ = (weak.clone(), name);
        });
    }
    {
        let weak = ui.as_weak();
        let current_mode = current_mode.clone();
        #[cfg(target_os = "android")]
        let rename_preset_uuid = rename_preset_uuid.clone();
        ui.on_rename_preset_confirm(move || {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                let core_mode: meditate_core::SessionMode =
                    current_mode.get().into();
                let Some(uuid) =
                    rename_preset_uuid.borrow_mut().take()
                else {
                    return;
                };
                let name =
                    ui.get_rename_preset_text().trim().to_string();
                let v = meditate_core::naming::validate(&name, |n| {
                    preset_name_taken(n, &uuid)
                });
                if !v.is_savable() {
                    return;
                }
                {
                    let Some(db_arc) = DATABASE.get() else { return; };
                    let Ok(guard) = db_arc.lock() else { return; };
                    let Some(db) = guard.as_ref() else { return; };
                    if let Err(e) = db.update_preset_name(&uuid, &name) {
                        meditate_core::log(
                            "preset.rename",
                            &format!("update_preset_name FAILED: {e:?}"),
                        );
                    }
                }
                ui.set_rename_preset_dialog_open(false);
                populate_preset_chooser(&ui, core_mode);
                refresh_preset_chips(&ui, core_mode);
                // Renaming a starred preset changes its widget
                // title.
                refresh_widget();
            }
            let _ = (weak.clone(), current_mode.get());
        });
    }
    {
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let delete_preset_uuid = delete_preset_uuid.clone();
        ui.on_preset_delete_tap(move |uuid| {
            #[cfg(target_os = "android")]
            if let Some(ui) = weak.upgrade() {
                let Some(p) = find_preset_by_uuid(uuid.as_str()) else {
                    return;
                };
                *delete_preset_uuid.borrow_mut() = Some(uuid.to_string());
                ui.set_delete_preset_name(p.name.into());
                ui.set_delete_preset_dialog_open(true);
            }
            let _ = (weak.clone(), uuid);
        });
    }
    {
        let weak = ui.as_weak();
        let current_mode = current_mode.clone();
        #[cfg(target_os = "android")]
        let delete_preset_uuid = delete_preset_uuid.clone();
        #[cfg(target_os = "android")]
        let pending_preset_delete = pending_preset_delete.clone();
        #[cfg(target_os = "android")]
        let pending_preset_undo = pending_preset_undo.clone();
        #[cfg(target_os = "android")]
        let pending_override_restore = pending_override_restore.clone();
        #[cfg(target_os = "android")]
        let recovery_uuid = recovery_uuid.clone();
        #[cfg(target_os = "android")]
        let delete_timer: &'static slint::Timer = delete_timer;
        ui.on_delete_preset_confirm(move || {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                let core_mode: meditate_core::SessionMode =
                    current_mode.get().into();
                let Some(uuid) =
                    delete_preset_uuid.borrow_mut().take()
                else {
                    return;
                };
                // Capture the full row so Undo can resurrect it
                // identically (GTK's insert_preset_with_uuid).
                let Some(row) = find_preset_by_uuid(&uuid) else {
                    return;
                };
                {
                    let Some(db_arc) = DATABASE.get() else { return; };
                    let Ok(guard) = db_arc.lock() else { return; };
                    let Some(db) = guard.as_ref() else { return; };
                    if let Err(e) = db.delete_preset(&uuid) {
                        meditate_core::log(
                            "preset.delete",
                            &format!("delete_preset FAILED: {e:?}"),
                        );
                    }
                }
                ui.set_delete_preset_dialog_open(false);
                ui.set_preset_chooser_page(false);
                populate_preset_chooser(&ui, core_mode);
                refresh_preset_chips(&ui, core_mode);
                // Deleting a starred preset drops its widget row.
                refresh_widget();

                // Shared snackbar + Undo (re-insert). Single slot
                // → clear the other discriminators.
                recovery_uuid.borrow_mut().take();
                pending_preset_undo.borrow_mut().take();
                pending_override_restore.borrow_mut().take();
                *pending_preset_delete.borrow_mut() = Some((
                    row.uuid.to_string(),
                    row.name.clone(),
                    row.mode,
                    row.is_starred,
                    row.config_json.clone(),
                ));
                ui.set_snackbar_text(
                    format!("'{}' deleted", row.name).into(),
                );
                ui.set_snackbar_visible(true);
                let weak_inner = ui.as_weak();
                let ppd = pending_preset_delete.clone();
                delete_timer.start(
                    slint::TimerMode::SingleShot,
                    std::time::Duration::from_secs(5),
                    move || {
                        ppd.borrow_mut().take();
                        if let Some(ui) = weak_inner.upgrade() {
                            ui.set_snackbar_visible(false);
                        }
                    },
                );
            }
            let _ = (weak.clone(), current_mode.get());
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
                _ => {
                    timer_session_secs.set(total_secs);
                    #[cfg(target_os = "android")]
                    write_timer_session_secs(total_secs);
                }
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

    // ── Bells group (B-5a) ──────────────────────────────────────
    // Starting / End bell enable + sound-pick wiring. Settings
    // are global (not per-mode). `bell_chooser_target` records
    // which caller opened the chooser so the pick routes back
    // correctly: 0 = Starting Bell, 1 = End Bell, 2 = the
    // interval-bell editor (B-5c-2).
    #[cfg(target_os = "android")]
    let bell_chooser_target: Rc<Cell<u8>> = Rc::new(Cell::new(0));

    // Interval-bell editor mode (B-5c-3): `Some(original)` when
    // editing an existing bell (Save → `update_interval_bell`,
    // preserving uuid / created_iso / enabled / pattern),
    // `None` when creating (Save → `insert_interval_bell`).
    #[cfg(target_os = "android")]
    let editing_ib: Rc<RefCell<Option<meditate_core::db::IntervalBell>>> =
        Rc::new(RefCell::new(None));

    // Vibration-editor target (Phase 6, mirrors GTK Editor's
    // `edit_uuid`): `None` = create / editing a bundled pattern
    // (Save inserts a new row — bundled is immutable in core);
    // `Some(uuid)` = editing a custom pattern (Save updates it).
    #[cfg(target_os = "android")]
    let ve_edit_uuid: Rc<RefCell<Option<String>>> =
        Rc::new(RefCell::new(None));

    // Shared intensities model — Rust owns it so a drag can
    // `set_row_data` a single sample and the chart re-renders
    // live (mirrors GTK mutating `editor.intensities` +
    // `queue_draw`). `ve_drag_idx` = the control point grabbed
    // by the current drag (-1 = none).
    #[cfg(target_os = "android")]
    let ve_model: Rc<slint::VecModel<f32>> =
        Rc::new(slint::VecModel::default());
    #[cfg(target_os = "android")]
    let ve_drag_idx: Rc<Cell<i32>> = Rc::new(Cell::new(-1));

    // Plot pixel size, reported by the chart's `ve-plot-size`.
    // Rust builds the Path commands in these coords so the curve
    // registers 1:1 with the handle dots (Slint Path viewbox
    // scaling does not preserve the dots' aspect ratio).
    #[cfg(target_os = "android")]
    let ve_plot_size: Rc<Cell<(f32, f32)>> = Rc::new(Cell::new((0.0, 0.0)));

    // Pattern-chooser routing (B-2b): 0 = Starting Bell,
    // 1 = End Bell (the interval-bell editor's pattern is B-2c).
    #[cfg(target_os = "android")]
    let pattern_chooser_target: Rc<Cell<u8>> = Rc::new(Cell::new(0));

    // Pattern-preview arbiter (B-2b). `PreviewToggle` keeps a
    // single pattern buzzing at a time: tapping the active row's
    // pill stops it, tapping another switches. Shared with the
    // chooser's back/pick handlers so leaving the overlay always
    // cancels an in-flight preview.
    #[cfg(target_os = "android")]
    let pattern_preview: Rc<RefCell<meditate_core::preview::PreviewToggle>> =
        Rc::new(RefCell::new(meditate_core::preview::PreviewToggle::new()));

    // Bell-sound preview arbiter (B-4) — same PreviewToggle
    // protocol as the pattern preview, but routed through the
    // audio MediaPlayer. Shared with the bell chooser's back /
    // pick so leaving the overlay silences any preview.
    #[cfg(target_os = "android")]
    let bell_preview: Rc<RefCell<meditate_core::preview::PreviewToggle>> =
        Rc::new(RefCell::new(meditate_core::preview::PreviewToggle::new()));

    {
        ui.on_starting_bell_toggled(move |value| {
            #[cfg(target_os = "android")]
            write_global_setting(
                "starting_bell_active",
                if value { "true" } else { "false" },
            );
            let _ = value;
        });
    }
    {
        ui.on_end_bell_toggled(move |value| {
            #[cfg(target_os = "android")]
            write_global_setting(
                "end_bell_active",
                if value { "true" } else { "false" },
            );
            let _ = value;
        });
    }
    {
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let bell_chooser_target = bell_chooser_target.clone();
        ui.on_starting_bell_sound_tap(move || {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                bell_chooser_target.set(0);
                let cur = read_global_setting(
                    "starting_bell_sound",
                    meditate_core::seeds::BUNDLED_BOWL_UUID,
                );
                populate_bell_chooser(&ui, &cur, meditate_core::db::BellSoundCategory::General);
                ui.set_bell_chooser_page(true);
            }
            let _ = weak.clone();
        });
    }
    {
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let bell_chooser_target = bell_chooser_target.clone();
        ui.on_end_bell_sound_tap(move || {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                bell_chooser_target.set(1);
                let cur = read_global_setting(
                    "end_bell_sound",
                    meditate_core::seeds::BUNDLED_BOWL_UUID,
                );
                populate_bell_chooser(&ui, &cur, meditate_core::db::BellSoundCategory::General);
                ui.set_bell_chooser_page(true);
            }
            let _ = weak.clone();
        });
    }

    // ── Box-Breath per-phase cues (B-7) ─────────────────────────
    // Master toggle + per-phase enable / Type / Sound / Pattern.
    // Cue firing is already wired (B-6); these only persist the
    // config via boxbreath_cues_active + db.set_box_breath_phase.
    {
        ui.on_boxbreath_cues_toggled(move |v| {
            #[cfg(target_os = "android")]
            write_global_setting(
                "boxbreath_cues_active",
                if v { "true" } else { "false" },
            );
            let _ = v;
        });
    }
    // Per-phase handlers. `tag` selects the core phase; the
    // chooser-target ints (3=In 4=HoldIn 5=Out 6=HoldOut) match
    // the `t @ 3..=6` arms in the pick handlers.
    macro_rules! bbc_phase_handlers {
        ($tag:literal, $tgt:literal,
         $on_tog:ident, $on_sm:ident, $on_snd:ident, $on_pat:ident) => {
            {
                ui.$on_tog(move |v| {
                    #[cfg(target_os = "android")]
                    write_bb_phase(bb_phase($tag), Some(v), None, None, None);
                    let _ = v;
                });
            }
            {
                ui.$on_sm(move |i| {
                    #[cfg(target_os = "android")]
                    write_bb_phase(
                        bb_phase($tag),
                        None,
                        Some(signal_mode_from_index(i)),
                        None,
                        None,
                    );
                    let _ = i;
                });
            }
            {
                let weak = ui.as_weak();
                #[cfg(target_os = "android")]
                let bell_chooser_target = bell_chooser_target.clone();
                ui.$on_snd(move || {
                    #[cfg(target_os = "android")]
                    {
                        let Some(ui) = weak.upgrade() else { return; };
                        bell_chooser_target.set($tgt);
                        let (su, _) = bb_phase_uuids(bb_phase($tag));
                        populate_bell_chooser(&ui, &su, meditate_core::db::BellSoundCategory::BoxBreath);
                        ui.set_bell_chooser_page(true);
                    }
                    let _ = weak.clone();
                });
            }
            {
                let weak = ui.as_weak();
                #[cfg(target_os = "android")]
                let pattern_chooser_target = pattern_chooser_target.clone();
                ui.$on_pat(move || {
                    #[cfg(target_os = "android")]
                    {
                        let Some(ui) = weak.upgrade() else { return; };
                        pattern_chooser_target.set($tgt);
                        let (_, pu) = bb_phase_uuids(bb_phase($tag));
                        populate_pattern_chooser(&ui, &pu);
                        ui.set_pattern_chooser_page(true);
                    }
                    let _ = weak.clone();
                });
            }
        };
    }
    bbc_phase_handlers!(
        "in", 3,
        on_bbc_in_toggled, on_bbc_in_signal_mode_changed,
        on_bbc_in_sound_tap, on_bbc_in_pattern_tap
    );
    bbc_phase_handlers!(
        "holdin", 4,
        on_bbc_holdin_toggled, on_bbc_holdin_signal_mode_changed,
        on_bbc_holdin_sound_tap, on_bbc_holdin_pattern_tap
    );
    bbc_phase_handlers!(
        "out", 5,
        on_bbc_out_toggled, on_bbc_out_signal_mode_changed,
        on_bbc_out_sound_tap, on_bbc_out_pattern_tap
    );
    bbc_phase_handlers!(
        "holdout", 6,
        on_bbc_holdout_toggled, on_bbc_holdout_signal_mode_changed,
        on_bbc_holdout_sound_tap, on_bbc_holdout_pattern_tap
    );

    {
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let bell_chooser_target = bell_chooser_target.clone();
        #[cfg(target_os = "android")]
        let bell_preview = bell_preview.clone();
        ui.on_bell_chooser_pick(move |uuid| {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                // Leaving the overlay always silences a preview.
                let _ = bell_preview.borrow_mut().stop();
                if let Some(app) = android_app() {
                    audio::stop(app);
                }
                ui.set_bell_preview_uuid(slint::SharedString::new());
                match bell_chooser_target.get() {
                    2 => {
                        // Interval-bell editor: just stage the
                        // pick into the editor fields; it's
                        // committed when the editor's Save runs.
                        ui.set_ie_sound_uuid(uuid.clone());
                        ui.set_ie_sound_name(
                            bell_sound_name(uuid.as_str()).into(),
                        );
                    }
                    1 => {
                        write_global_setting(
                            "end_bell_sound",
                            uuid.as_str(),
                        );
                        refresh_bell_rows(&ui);
                    }
                    t @ 3..=6 => {
                        // Box-Breath phase sound (B-7).
                        let phase = match t {
                            3 => bb_phase("in"),
                            4 => bb_phase("holdin"),
                            5 => bb_phase("out"),
                            _ => bb_phase("holdout"),
                        };
                        write_bb_phase(
                            phase,
                            None,
                            None,
                            Some(uuid.as_str()),
                            None,
                        );
                        refresh_bell_rows(&ui);
                    }
                    _ => {
                        write_global_setting(
                            "starting_bell_sound",
                            uuid.as_str(),
                        );
                        refresh_bell_rows(&ui);
                    }
                }
                ui.set_bell_chooser_page(false);
            }
            let _ = (weak.clone(), uuid);
        });
    }
    {
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let bell_preview = bell_preview.clone();
        ui.on_bell_chooser_back(move || {
            #[cfg(target_os = "android")]
            if let Some(ui) = weak.upgrade() {
                let _ = bell_preview.borrow_mut().stop();
                if let Some(app) = android_app() {
                    audio::stop(app);
                }
                ui.set_bell_preview_uuid(slint::SharedString::new());
                ui.set_bell_chooser_page(false);
            }
            let _ = weak.clone();
        });
    }

    // ── Signal Type + Pattern (B-2b) ────────────────────────────
    // Type toggle persists `*_bell_signal_mode` then re-reads the
    // Bells group so the conditional Sound/Pattern rows update.
    {
        let weak = ui.as_weak();
        ui.on_starting_bell_signal_mode_changed(move |idx| {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                write_global_setting(
                    "starting_bell_signal_mode",
                    signal_mode_db_str(idx),
                );
                refresh_bell_rows(&ui);
            }
            let _ = (weak.clone(), idx);
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_end_bell_signal_mode_changed(move |idx| {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                write_global_setting(
                    "end_bell_signal_mode",
                    signal_mode_db_str(idx),
                );
                refresh_bell_rows(&ui);
            }
            let _ = (weak.clone(), idx);
        });
    }
    {
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let pattern_chooser_target = pattern_chooser_target.clone();
        ui.on_starting_bell_pattern_tap(move || {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                pattern_chooser_target.set(0);
                let cur = read_global_setting(
                    "starting_bell_pattern",
                    meditate_core::seeds::BUNDLED_PATTERN_PULSE_UUID,
                );
                populate_pattern_chooser(&ui, &cur);
                ui.set_pattern_chooser_page(true);
            }
            let _ = weak.clone();
        });
    }
    {
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let pattern_chooser_target = pattern_chooser_target.clone();
        ui.on_end_bell_pattern_tap(move || {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                pattern_chooser_target.set(1);
                let cur = read_global_setting(
                    "end_bell_pattern",
                    meditate_core::seeds::BUNDLED_PATTERN_PULSE_UUID,
                );
                populate_pattern_chooser(&ui, &cur);
                ui.set_pattern_chooser_page(true);
            }
            let _ = weak.clone();
        });
    }
    {
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let pattern_chooser_target = pattern_chooser_target.clone();
        #[cfg(target_os = "android")]
        let pattern_preview = pattern_preview.clone();
        ui.on_pattern_chooser_pick(move |uuid| {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                // Leaving the overlay always silences a preview.
                let _ = pattern_preview.borrow_mut().stop();
                if let Some(app) = android_app() {
                    haptics::cancel(app);
                }
                ui.set_pattern_preview_uuid(slint::SharedString::new());
                match pattern_chooser_target.get() {
                    2 => {
                        // Interval-bell editor: stage only; the
                        // pick is committed on the editor's Save.
                        ui.set_ie_pattern_uuid(uuid.clone());
                        ui.set_ie_pattern_name(
                            pattern_name(uuid.as_str()).into(),
                        );
                    }
                    1 => {
                        write_global_setting(
                            "end_bell_pattern",
                            uuid.as_str(),
                        );
                        refresh_bell_rows(&ui);
                    }
                    t @ 3..=6 => {
                        // Box-Breath phase pattern (B-7).
                        let phase = match t {
                            3 => bb_phase("in"),
                            4 => bb_phase("holdin"),
                            5 => bb_phase("out"),
                            _ => bb_phase("holdout"),
                        };
                        write_bb_phase(
                            phase,
                            None,
                            None,
                            None,
                            Some(uuid.as_str()),
                        );
                        refresh_bell_rows(&ui);
                    }
                    _ => {
                        write_global_setting(
                            "starting_bell_pattern",
                            uuid.as_str(),
                        );
                        refresh_bell_rows(&ui);
                    }
                }
                ui.set_pattern_chooser_page(false);
            }
            let _ = (weak.clone(), uuid);
        });
    }
    {
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let pattern_preview = pattern_preview.clone();
        ui.on_pattern_chooser_back(move || {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                let _ = pattern_preview.borrow_mut().stop();
                if let Some(app) = android_app() {
                    haptics::cancel(app);
                }
                ui.set_pattern_preview_uuid(slint::SharedString::new());
                ui.set_pattern_chooser_page(false);
            }
            let _ = weak.clone();
        });
    }

    // ── Vibration-pattern curve editor (Phase 6, V-2/V-3) ───────
    // Seed defaults / load existing into the `ve-*` fields
    // (mirrors GTK `Editor::new`); the chart is interactive — the
    // shared `ve_model` backs the handle dots and Rust rebuilds
    // the Path commands on every drag (mirrors GTK draw_chart +
    // drag-update). Save / Preview land in V-4.
    #[cfg(target_os = "android")]
    ui.set_ve_intensities(ve_model.clone().into());

    #[cfg(target_os = "android")]
    fn ve_kind_index(k: meditate_core::db::ChartKind) -> i32 {
        match k {
            meditate_core::db::ChartKind::Bar => 0,
            meditate_core::db::ChartKind::Line => 1,
        }
    }
    // Rebuild the area + line/step Path commands in the plot's
    // PIXEL space (`size` = its reported px w/h; the Path viewbox
    // is pinned to the same size so scale is 1:1 and the curve
    // sits exactly on the handle dots). No-op until the plot has
    // a real size (init/resize fills it). Empty model → cleared.
    // Plot edge inset (px). Shared by the Path builder, the
    // Slint handle-dot positions, and the drag hit-test so all
    // three use one coordinate space (mirrors GTK chart_rect).
    #[cfg(target_os = "android")]
    const VE_PLOT_PAD: f64 = 14.0;
    #[cfg(target_os = "android")]
    fn ve_rebuild_curve(
        ui: &MainWindow,
        model: &slint::VecModel<f32>,
        size: (f32, f32),
    ) {
        use slint::Model;
        let (w, h) = (f64::from(size.0), f64::from(size.1));
        if w < 1.0 || h < 1.0 {
            return; // plot not measured yet — keep prior commands
        }
        let v: Vec<f32> = model.iter().collect();
        let n = v.len();
        if n == 0 {
            ui.set_ve_curve_cmd("".into());
            ui.set_ve_area_cmd("".into());
            return;
        }
        // Inset the control points off the plot edges (mirrors
        // GTK's `chart_rect` padding): otherwise the first/last
        // handle dots clip at x=0/x=w (half-dot + white-border
        // sliver) and are awkward to grab at the screen edge.
        // VE_PLOT_PAD is shared with the dot positions (Slint)
        // and the hit-test, so the curve, dots and picking all
        // line up. Bars still tile the full [0, w] (no gaps).
        let pad = VE_PLOT_PAD;
        let iw = (w - 2.0 * pad).max(1.0);
        let ih = (h - 2.0 * pad).max(1.0);
        let py = |val: f32| pad + (1.0 - f64::from(val)) * ih;
        let px = |i: usize| {
            pad + meditate_core::vibration::point_x_fraction(i, n) * iw
        };
        let mut line = String::new();
        let mut area = String::new();
        if ui.get_ve_chart_kind() == 0 {
            // Bar: sample-and-hold. ONE closed stepped polygon
            // (baseline → up at x=0 → stair profile holding each
            // level until the midpoint to its neighbour → down at
            // x=w → close), filled solid. NO separate stroke
            // (`line` stays empty): a 2px outline over the same
            // shape left a white hairline where its AA met the
            // fill on the tall first/last edges — and GTK bar
            // mode is just filled rects anyway. Cells tile [0, w].
            let xs: Vec<f64> = (0..n).map(px).collect();
            let ys: Vec<f64> = v.iter().map(|&val| py(val)).collect();
            let mid = |i: usize| (xs[i] + xs[i + 1]) / 2.0;
            area.push_str(&format!(
                "M {:.2} {:.2} L {:.2} {:.2}",
                0.0, h, 0.0, ys[0]
            ));
            for i in 0..n {
                let xr = if i == n - 1 { w } else { mid(i) };
                area.push_str(&format!(" L {xr:.2} {:.2}", ys[i]));
                if i < n - 1 {
                    area.push_str(&format!(
                        " L {:.2} {:.2}",
                        mid(i),
                        ys[i + 1]
                    ));
                }
            }
            area.push_str(&format!(" L {w:.2} {h:.2} Z"));
        } else {
            // Line: polyline through the points; area = same,
            // closed down to the baseline.
            area.push_str(&format!("M {:.2} {:.2}", px(0), h));
            for (i, &val) in v.iter().enumerate() {
                let (x, y) = (px(i), py(val));
                if i == 0 {
                    line.push_str(&format!("M {x:.2} {y:.2}"));
                } else {
                    line.push_str(&format!(" L {x:.2} {y:.2}"));
                }
                area.push_str(&format!(" L {x:.2} {y:.2}"));
            }
            area.push_str(&format!(" L {:.2} {:.2} Z", px(n - 1), h));
        }
        ui.set_ve_curve_cmd(line.into());
        ui.set_ve_area_cmd(area.into());
    }
    {
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let ve_edit_uuid = ve_edit_uuid.clone();
        #[cfg(target_os = "android")]
        let ve_model = ve_model.clone();
        #[cfg(target_os = "android")]
        let ve_plot_size = ve_plot_size.clone();
        ui.on_pattern_create_tap(move || {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                ve_edit_uuid.borrow_mut().take();
                let pts = meditate_core::vibration::EDITOR_DEFAULT_POINTS;
                ui.set_ve_title("New Pattern".into());
                ui.set_ve_name("".into());
                ui.set_ve_save_enabled(false);
                ui.set_ve_points_max(
                    meditate_core::vibration::max_points_for_duration_s(
                        meditate_core::vibration::EDITOR_DEFAULT_DURATION_S,
                    ) as i32,
                );
                ui.set_ve_duration_tenths(
                    (meditate_core::vibration::EDITOR_DEFAULT_DURATION_S
                        * 10.0) as i32,
                );
                ui.set_ve_points(pts as i32);
                ui.set_ve_chart_kind(1); // Line
                ve_model.set_vec(
                    meditate_core::vibration::resample_envelope(&[], pts),
                );
                ve_rebuild_curve(&ui, &ve_model, ve_plot_size.get());
                ui.set_vibration_editor_page(true);
            }
            let _ = weak.clone();
        });
    }
    {
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let ve_edit_uuid = ve_edit_uuid.clone();
        #[cfg(target_os = "android")]
        let ve_model = ve_model.clone();
        #[cfg(target_os = "android")]
        let ve_plot_size = ve_plot_size.clone();
        ui.on_pattern_edit(move |uuid| {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                let found = {
                    let Some(db_arc) = DATABASE.get() else { return; };
                    let Ok(guard) = db_arc.lock() else { return; };
                    let Some(db) = guard.as_ref() else { return; };
                    meditate_core::db::find_vibration_pattern_by_uuid_from_db(
                        db,
                        uuid.as_str(),
                    )
                    .ok()
                    .flatten()
                };
                let Some(p) = found else { return; };
                ui.set_ve_points_max(
                    meditate_core::vibration::max_points_for_duration_s(
                        f64::from(p.duration_ms) / 1000.0,
                    ) as i32,
                );
                ui.set_ve_duration_tenths((p.duration_ms / 100) as i32);
                ui.set_ve_points(p.intensities.len() as i32);
                ui.set_ve_chart_kind(ve_kind_index(p.chart_kind));
                ve_model.set_vec(p.intensities.clone());
                ve_rebuild_curve(&ui, &ve_model, ve_plot_size.get());
                if p.is_bundled {
                    // Bundled is immutable in core — open a copy
                    // (Save inserts a new row). Suffix the name so
                    // it doesn't instantly collide.
                    ve_edit_uuid.borrow_mut().take();
                    ui.set_ve_title("New Pattern".into());
                    ui.set_ve_name(format!("{} copy", p.name).into());
                } else {
                    *ve_edit_uuid.borrow_mut() = Some(uuid.to_string());
                    ui.set_ve_title("Edit Pattern".into());
                    ui.set_ve_name(p.name.clone().into());
                }
                ui.set_ve_save_enabled(true);
                ui.set_vibration_editor_page(true);
            }
            let _ = (weak.clone(), uuid);
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_vibration_editor_cancel(move || {
            #[cfg(target_os = "android")]
            if let Some(ui) = weak.upgrade() {
                // Just close — the pattern chooser is still
                // underneath (it stays `page=true`).
                ui.set_vibration_editor_page(false);
            }
            let _ = weak.clone();
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_ve_name_changed(move |t| {
            #[cfg(target_os = "android")]
            if let Some(ui) = weak.upgrade() {
                // V-3: empty-name gate; full collision check V-4.
                ui.set_ve_save_enabled(!t.trim().is_empty());
            }
            let _ = (weak.clone(), t);
        });
    }
    {
        // Duration change → recompute the Points cap; clamping
        // ve-points fires `changed ve-points` → resample.
        let weak = ui.as_weak();
        ui.on_ve_duration_changed(move |tenths| {
            #[cfg(target_os = "android")]
            if let Some(ui) = weak.upgrade() {
                let secs = f64::from(tenths) / 10.0;
                let max =
                    meditate_core::vibration::max_points_for_duration_s(
                        secs,
                    ) as i32;
                ui.set_ve_points_max(max);
                if ui.get_ve_points() > max {
                    ui.set_ve_points(max);
                }
            }
            let _ = (weak.clone(), tenths);
        });
    }
    {
        // Points change → resample onto the new grid (mirrors
        // GTK's points spin-row `resample_envelope`).
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let ve_model = ve_model.clone();
        #[cfg(target_os = "android")]
        let ve_plot_size = ve_plot_size.clone();
        ui.on_ve_points_changed(move |n| {
            #[cfg(target_os = "android")]
            if let Some(ui) = weak.upgrade() {
                use slint::Model;
                let n = n.max(1) as usize;
                if ve_model.row_count() != n {
                    let old: Vec<f32> = ve_model.iter().collect();
                    ve_model.set_vec(
                        meditate_core::vibration::resample_envelope(
                            &old, n,
                        ),
                    );
                    ve_rebuild_curve(&ui, &ve_model, ve_plot_size.get());
                }
            }
            let _ = (weak.clone(), n);
        });
    }
    {
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let ve_model = ve_model.clone();
        #[cfg(target_os = "android")]
        let ve_plot_size = ve_plot_size.clone();
        ui.on_ve_kind_changed(move |_idx| {
            #[cfg(target_os = "android")]
            if let Some(ui) = weak.upgrade() {
                ve_rebuild_curve(&ui, &ve_model, ve_plot_size.get());
            }
            let _ = weak.clone();
        });
    }
    {
        // Drag begin → core 2-D nearest-handle pick + first move.
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let ve_model = ve_model.clone();
        #[cfg(target_os = "android")]
        let ve_plot_size = ve_plot_size.clone();
        #[cfg(target_os = "android")]
        let ve_drag_idx = ve_drag_idx.clone();
        ui.on_ve_drag_begin(move |px, py, w, h| {
            #[cfg(target_os = "android")]
            if let Some(ui) = weak.upgrade() {
                use slint::Model;
                let v: Vec<f32> = ve_model.iter().collect();
                // Translate the touch into the inset region so
                // the pick matches the rendered points (same
                // VE_PLOT_PAD as the Path / dots). Pure
                // translation → distances stay px, hit radius
                // unchanged.
                let iw = (f64::from(w) - 2.0 * VE_PLOT_PAD).max(1.0);
                let ih = (f64::from(h) - 2.0 * VE_PLOT_PAD).max(1.0);
                let tx = f64::from(px) - VE_PLOT_PAD;
                let ty = f64::from(py) - VE_PLOT_PAD;
                match meditate_core::vibration::nearest_point(
                    &v,
                    tx,
                    ty,
                    iw,
                    ih,
                    meditate_core::vibration::EDITOR_HIT_RADIUS_PX,
                ) {
                    Some(i) => {
                        ve_drag_idx.set(i as i32);
                        ve_model.set_row_data(
                            i,
                            meditate_core::vibration::intensity_from_y(
                                ty, ih,
                            ),
                        );
                        ve_rebuild_curve(&ui, &ve_model, ve_plot_size.get());
                    }
                    None => ve_drag_idx.set(-1),
                }
            }
            let _ = (weak.clone(), px, py, w, h);
        });
    }
    {
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let ve_model = ve_model.clone();
        #[cfg(target_os = "android")]
        let ve_plot_size = ve_plot_size.clone();
        #[cfg(target_os = "android")]
        let ve_drag_idx = ve_drag_idx.clone();
        ui.on_ve_drag_move(move |py, h| {
            #[cfg(target_os = "android")]
            if let Some(ui) = weak.upgrade() {
                use slint::Model;
                let i = ve_drag_idx.get();
                if i >= 0 && (i as usize) < ve_model.row_count() {
                    // Same inset as the pick / Path mapping.
                    let ih =
                        (f64::from(h) - 2.0 * VE_PLOT_PAD).max(1.0);
                    let ty = f64::from(py) - VE_PLOT_PAD;
                    ve_model.set_row_data(
                        i as usize,
                        meditate_core::vibration::intensity_from_y(
                            ty, ih,
                        ),
                    );
                    ve_rebuild_curve(&ui, &ve_model, ve_plot_size.get());
                }
            }
            let _ = (weak.clone(), py, h);
        });
    }
    {
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let ve_drag_idx = ve_drag_idx.clone();
        ui.on_ve_drag_end(move || {
            #[cfg(target_os = "android")]
            ve_drag_idx.set(-1);
            let _ = weak.clone();
        });
    }
    {
        // Plot reported its pixel size — stash it and rebuild the
        // curve in those coords (fires on init + every resize).
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let ve_model = ve_model.clone();
        #[cfg(target_os = "android")]
        let ve_plot_size = ve_plot_size.clone();
        ui.on_ve_plot_size(move |w, h| {
            #[cfg(target_os = "android")]
            if let Some(ui) = weak.upgrade() {
                ve_plot_size.set((w, h));
                ve_rebuild_curve(&ui, &ve_model, ve_plot_size.get());
            }
            let _ = (weak.clone(), w, h);
        });
    }
    {
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let ve_edit_uuid = ve_edit_uuid.clone();
        ui.on_vibration_editor_save(move || {
            #[cfg(target_os = "android")]
            if let Some(ui) = weak.upgrade() {
                // V-4 wires insert/update + chooser repopulate.
                meditate_core::log(
                    "vibration.editor",
                    &format!(
                        "TODO V-4 save edit_uuid={:?}",
                        ve_edit_uuid.borrow()
                    ),
                );
                ui.set_vibration_editor_page(false);
            }
            let _ = weak.clone();
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_vibration_editor_preview_toggle(move || {
            #[cfg(target_os = "android")]
            meditate_core::log(
                "vibration.editor",
                "TODO V-4 preview toggle",
            );
            let _ = weak.clone();
        });
    }
    {
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let pattern_preview = pattern_preview.clone();
        ui.on_pattern_preview_toggle(move |uuid| {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                let action = pattern_preview.borrow_mut().request(uuid.as_str());
                match action {
                    meditate_core::preview::PreviewAction::StopAndStart {
                        id,
                        generation,
                    } => {
                        let mut dur_ms: u32 = 0;
                        if let Some(app) = android_app() {
                            // Cancel the outgoing buzz before the
                            // new one so they don't overlap.
                            haptics::cancel(app);
                            if let Some(db_arc) = DATABASE.get() {
                                if let Ok(guard) = db_arc.lock() {
                                    if let Some(db) = guard.as_ref() {
                                        if let Ok(Some(p)) =
                                            meditate_core::db::find_vibration_pattern_by_uuid_from_db(
                                                db, &id,
                                            )
                                        {
                                            dur_ms = p.duration_ms;
                                            let env =
                                                meditate_core::vibration::build_master_envelope(&p);
                                            haptics::vibrate_waveform(app, &env);
                                        }
                                    }
                                }
                            }
                        }
                        ui.set_pattern_preview_uuid(id.into());

                        // The waveform is finite — nothing tells the
                        // UI when it stops, so the pill would stay
                        // "Stop" forever. Schedule a revert after the
                        // pattern's own duration; `timer_should_revert`
                        // no-ops if the user already stopped it or
                        // started a different one (generation guard),
                        // mirroring GTK's preview-timer logic.
                        if dur_ms > 0 {
                            let weak2 = weak.clone();
                            let pp = pattern_preview.clone();
                            slint::Timer::single_shot(
                                std::time::Duration::from_millis(dur_ms as u64),
                                move || {
                                    if pp
                                        .borrow_mut()
                                        .timer_should_revert(generation)
                                    {
                                        if let Some(ui) = weak2.upgrade() {
                                            ui.set_pattern_preview_uuid(
                                                slint::SharedString::new(),
                                            );
                                        }
                                    }
                                },
                            );
                        }
                    }
                    meditate_core::preview::PreviewAction::StopOnly => {
                        if let Some(app) = android_app() {
                            haptics::cancel(app);
                        }
                        ui.set_pattern_preview_uuid(slint::SharedString::new());
                    }
                    meditate_core::preview::PreviewAction::NoOp => {}
                }
            }
            let _ = (weak.clone(), uuid);
        });
    }
    {
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let bell_preview = bell_preview.clone();
        ui.on_bell_preview_toggle(move |uuid| {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                let action = bell_preview.borrow_mut().request(uuid.as_str());
                match action {
                    meditate_core::preview::PreviewAction::StopAndStart {
                        id,
                        generation,
                    } => {
                        let mut dur_ms: i64 = 0;
                        if let Some(app) = android_app() {
                            // Single audio slot — play() supersedes
                            // any in-flight clip; stop() first keeps
                            // the swap clean.
                            audio::stop(app);
                            let path = bell_sound_path(&id);
                            dur_ms = audio::play(app, &path);
                        }
                        ui.set_bell_preview_uuid(id.into());

                        // Revert the pill when the clip finishes —
                        // the Android analogue of GTK reverting the
                        // Play icon on the MediaFile's notify::ended.
                        // `play()` returned the clip length;
                        // `timer_should_revert` no-ops if the user
                        // already stopped it / started another
                        // (generation guard).
                        if dur_ms > 0 {
                            let weak2 = weak.clone();
                            let bp = bell_preview.clone();
                            slint::Timer::single_shot(
                                std::time::Duration::from_millis(dur_ms as u64),
                                move || {
                                    if bp
                                        .borrow_mut()
                                        .timer_should_revert(generation)
                                    {
                                        if let Some(ui) = weak2.upgrade() {
                                            ui.set_bell_preview_uuid(
                                                slint::SharedString::new(),
                                            );
                                        }
                                    }
                                },
                            );
                        }
                    }
                    meditate_core::preview::PreviewAction::StopOnly => {
                        if let Some(app) = android_app() {
                            audio::stop(app);
                        }
                        ui.set_bell_preview_uuid(slint::SharedString::new());
                    }
                    meditate_core::preview::PreviewAction::NoOp => {}
                }
            }
            let _ = (weak.clone(), uuid);
        });
    }

    // Preparation Time (B-5b): enable toggle + seconds modal.
    {
        ui.on_prep_time_toggled(move |value| {
            #[cfg(target_os = "android")]
            write_global_setting(
                "preparation_time_active",
                if value { "true" } else { "false" },
            );
            let _ = value;
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_prep_time_tap(move || {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                // Seed the working value from the committed one.
                ui.set_prep_dialog_secs(ui.get_prep_time_secs());
                ui.set_prep_dialog_open(true);
            }
            let _ = weak.clone();
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_prep_secs_committed(move || {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                // `parse_prep_secs` re-clamps to [MIN, MAX] even
                // though the spinbox already bounds it — keeps the
                // persisted string canonical.
                let v = meditate_core::format::parse_prep_secs(
                    &ui.get_prep_time_secs().to_string(),
                );
                write_global_setting(
                    "preparation_time_secs",
                    &v.to_string(),
                );
                ui.set_prep_time_secs(v as i32);
            }
            let _ = weak.clone();
        });
    }

    // Interval Bells (B-5c-1): enable toggle + Manage Bells →
    // read-only library page.
    {
        ui.on_interval_bells_toggled(move |value| {
            #[cfg(target_os = "android")]
            write_global_setting(
                "interval_bells_active",
                if value { "true" } else { "false" },
            );
            let _ = value;
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_manage_bells_tap(move || {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                populate_interval_bells(&ui);
                ui.set_interval_bells_page(true);
            }
            let _ = weak.clone();
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_interval_bells_back(move || {
            #[cfg(target_os = "android")]
            if let Some(ui) = weak.upgrade() {
                ui.set_interval_bells_page(false);
            }
            let _ = weak.clone();
        });
    }

    // ── Interval-bell editor (B-5c-2/3): create / edit / delete ─
    {
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let editing_ib = editing_ib.clone();
        ui.on_create_interval_bell_tap(move || {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                // Create mode — no original to preserve.
                editing_ib.borrow_mut().take();
                ui.set_ie_kind(0);
                ui.set_ie_minutes(5);
                ui.set_ie_jitter(0);
                ui.set_ie_sound_uuid(
                    meditate_core::seeds::BUNDLED_BOWL_UUID.into(),
                );
                ui.set_ie_sound_name(
                    bell_sound_name(meditate_core::seeds::BUNDLED_BOWL_UUID)
                        .into(),
                );
                // B-2c: new bells default to Sound; the bundled
                // Pulse pattern stages the (initially inert)
                // vibration choice, matching the insert default.
                ui.set_ie_signal_mode(0);
                ui.set_ie_pattern_uuid(
                    meditate_core::seeds::BUNDLED_PATTERN_PULSE_UUID.into(),
                );
                ui.set_ie_pattern_name(
                    pattern_name(meditate_core::seeds::BUNDLED_PATTERN_PULSE_UUID)
                        .into(),
                );
                ui.set_interval_editor_page(true);
            }
            let _ = weak.clone();
        });
    }
    {
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let editing_ib = editing_ib.clone();
        ui.on_interval_bell_edit(move |uuid| {
            #[cfg(target_os = "android")]
            {
                use meditate_core::db::IntervalBellKind;
                let Some(ui) = weak.upgrade() else { return; };
                let found = {
                    let Some(db_arc) = DATABASE.get() else { return; };
                    let Ok(guard) = db_arc.lock() else { return; };
                    let Some(db) = guard.as_ref() else { return; };
                    db.list_interval_bells()
                        .unwrap_or_default()
                        .into_iter()
                        .find(|b| b.uuid.to_string() == uuid.as_str())
                };
                let Some(bell) = found else { return; };
                ui.set_ie_kind(match bell.kind {
                    IntervalBellKind::Interval => 0,
                    IntervalBellKind::FixedFromStart => 1,
                    IntervalBellKind::FixedFromEnd => 2,
                });
                ui.set_ie_minutes(bell.minutes as i32);
                ui.set_ie_jitter(bell.jitter_pct as i32);
                let su = bell.sound_uuid.to_string();
                ui.set_ie_sound_name(bell_sound_name(&su).into());
                ui.set_ie_sound_uuid(su.into());
                // B-2c: load the bell's persisted Type + pattern.
                ui.set_ie_signal_mode(signal_mode_index(
                    bell.signal_mode.as_db_str(),
                ));
                let pu = bell.vibration_pattern_uuid.to_string();
                ui.set_ie_pattern_name(pattern_name(&pu).into());
                ui.set_ie_pattern_uuid(pu.into());
                *editing_ib.borrow_mut() = Some(bell);
                ui.set_interval_editor_page(true);
            }
            let _ = (weak.clone(), uuid);
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_interval_bell_delete(move |uuid| {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                if let Some(db_arc) = DATABASE.get() {
                    if let Ok(guard) = db_arc.lock() {
                        if let Some(db) = guard.as_ref() {
                            if let Err(e) =
                                db.delete_interval_bell(uuid.as_str())
                            {
                                meditate_core::log(
                                    "interval_bell.delete.failed",
                                    &format!("{uuid}: {e:?}"),
                                );
                            }
                        }
                    }
                }
                populate_interval_bells(&ui);
            }
            let _ = (weak.clone(), uuid);
        });
    }
    {
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let bell_chooser_target = bell_chooser_target.clone();
        ui.on_interval_editor_sound_tap(move || {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                bell_chooser_target.set(2);
                populate_bell_chooser(&ui, ui.get_ie_sound_uuid().as_str(), meditate_core::db::BellSoundCategory::General);
                ui.set_bell_chooser_page(true);
            }
            let _ = weak.clone();
        });
    }
    {
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let pattern_chooser_target = pattern_chooser_target.clone();
        ui.on_interval_editor_pattern_tap(move || {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                // Target 2 = interval-bell editor: the pick is
                // staged into the `ie_pattern_*` fields and only
                // committed when the editor's Save runs (same
                // contract as the sound chooser's target 2).
                pattern_chooser_target.set(2);
                populate_pattern_chooser(
                    &ui,
                    ui.get_ie_pattern_uuid().as_str(),
                );
                ui.set_pattern_chooser_page(true);
            }
            let _ = weak.clone();
        });
    }
    {
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let editing_ib = editing_ib.clone();
        ui.on_interval_editor_cancel(move || {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                // Drop edit context so the next "Create" starts
                // clean.
                editing_ib.borrow_mut().take();
                ui.set_interval_editor_page(false);
            }
            let _ = weak.clone();
        });
    }
    {
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let editing_ib = editing_ib.clone();
        ui.on_interval_editor_save(move || {
            #[cfg(target_os = "android")]
            {
                use meditate_core::db::IntervalBellKind;
                let Some(ui) = weak.upgrade() else { return; };
                let kind = match ui.get_ie_kind() {
                    1 => IntervalBellKind::FixedFromStart,
                    2 => IntervalBellKind::FixedFromEnd,
                    _ => IntervalBellKind::Interval,
                };
                let minutes = (ui.get_ie_minutes().max(1) as u32).min(120);
                // Jitter is only meaningful for the Interval
                // kind (mirrors GTK gating it on kind).
                let jitter = if kind == IntervalBellKind::Interval {
                    (ui.get_ie_jitter().max(0) as u32).min(50)
                } else {
                    0
                };
                let sound = ui.get_ie_sound_uuid().to_string();
                let signal_mode =
                    signal_mode_from_index(ui.get_ie_signal_mode());
                let pattern = ui.get_ie_pattern_uuid().to_string();
                let original = editing_ib.borrow_mut().take();
                if let Some(db_arc) = DATABASE.get() {
                    if let Ok(guard) = db_arc.lock() {
                        if let Some(db) = guard.as_ref() {
                            let res = match original {
                                Some(mut bell) => {
                                    // Edit: preserve uuid /
                                    // created_iso / enabled; swap
                                    // every editable field incl.
                                    // the B-2c Type + pattern.
                                    bell.kind = kind;
                                    bell.minutes = minutes;
                                    bell.jitter_pct = jitter;
                                    bell.sound_uuid = sound.clone().into();
                                    bell.signal_mode = signal_mode;
                                    bell.vibration_pattern_uuid =
                                        pattern.clone().into();
                                    db.update_interval_bell(&bell)
                                        .map(|()| 0)
                                }
                                None => {
                                    // Create with the chosen Type
                                    // + pattern (B-2c).
                                    db.insert_interval_bell(
                                        kind,
                                        minutes,
                                        jitter,
                                        &sound,
                                        &pattern,
                                        signal_mode,
                                    )
                                }
                            };
                            if let Err(e) = res {
                                meditate_core::log(
                                    "interval_bell.save.failed",
                                    &format!("{e:?}"),
                                );
                            }
                        }
                    }
                }
                ui.set_interval_editor_page(false);
                populate_interval_bells(&ui);
            }
            let _ = weak.clone();
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
        refresh_sync_indicator(&ui);
        refresh_stats(&ui);
        refresh_bell_rows(&ui);
        refresh_preset_chips(&ui, core_mode);
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
        #[cfg(target_os = "android")]
        let recovery_uuid = recovery_uuid.clone();
        #[cfg(target_os = "android")]
        let pending_preset_undo = pending_preset_undo.clone();
        #[cfg(target_os = "android")]
        let pending_preset_delete = pending_preset_delete.clone();
        #[cfg(target_os = "android")]
        let pending_override_restore = pending_override_restore.clone();
        ui.on_delete_tap(move |rowid| {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                // A trash tap replaces any in-flight recovery /
                // preset snackbar with the delete one — clear every
                // other discriminator so the shared Undo handler
                // routes to the delete-restore branch (single slot).
                recovery_uuid.borrow_mut().take();
                pending_preset_undo.borrow_mut().take();
                pending_preset_delete.borrow_mut().take();
                pending_override_restore.borrow_mut().take();
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
        #[cfg(target_os = "android")]
        let recovery_uuid = recovery_uuid.clone();
        #[cfg(target_os = "android")]
        let pending_preset_undo = pending_preset_undo.clone();
        #[cfg(target_os = "android")]
        let pending_preset_delete = pending_preset_delete.clone();
        #[cfg(target_os = "android")]
        let pending_override_restore = pending_override_restore.clone();
        #[cfg(target_os = "android")]
        let current_mode = current_mode.clone();
        #[cfg(target_os = "android")]
        let timer_session_secs = timer_session_secs.clone();
        ui.on_snackbar_undo_tap(move || {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                delete_timer.stop();
                let core_mode: meditate_core::SessionMode =
                    current_mode.get().into();
                // Preset-apply Undo branch (P-3) — checked FIRST
                // (single slot; the preset snackbar, if up, owns
                // Undo). Re-apply the pre-apply snapshot through
                // the same path the forward apply used, exactly
                // like GTK's Undo calling `apply_config(&snapshot)`.
                if let Some((json, mode)) =
                    pending_preset_undo.borrow_mut().take()
                {
                    apply_preset_json(
                        &ui,
                        &json,
                        mode,
                        &timer_session_secs,
                    );
                    ui.set_snackbar_visible(false);
                    return;
                }
                // Preset-delete Undo (P-5): re-insert the captured
                // row with its original uuid — GTK's
                // insert_preset_with_uuid resurrection.
                if let Some((u, name, mode, starred, json)) =
                    pending_preset_delete.borrow_mut().take()
                {
                    {
                        let Some(db_arc) = DATABASE.get() else { return; };
                        let Ok(g) = db_arc.lock() else { return; };
                        let Some(db) = g.as_ref() else { return; };
                        let _ = db.insert_preset_with_uuid(
                            &u, &name, mode, starred, &json,
                        );
                    }
                    populate_preset_chooser(&ui, core_mode);
                    refresh_preset_chips(&ui, core_mode);
                    // Undone delete resurrects the widget row.
                    refresh_widget();
                    ui.set_snackbar_visible(false);
                    return;
                }
                // Preset-override Undo (P-5 / P-4 deferral):
                // restore the preset's prior config_json.
                if let Some((u, prior, mode)) =
                    pending_override_restore.borrow_mut().take()
                {
                    {
                        let Some(db_arc) = DATABASE.get() else { return; };
                        let Ok(g) = db_arc.lock() else { return; };
                        let Some(db) = g.as_ref() else { return; };
                        let _ = db.update_preset_config(&u, &prior);
                    }
                    refresh_preset_chips(&ui, mode);
                    // Undone override reverts the widget subtitle.
                    refresh_widget();
                    ui.set_snackbar_visible(false);
                    return;
                }
                if let Some(uuid) = recovery_uuid.borrow_mut().take() {
                    // Recovery-undo branch (L-6): the rescued
                    // session row already exists; delete it by
                    // uuid so the tombstoning `session_delete`
                    // event propagates to sync peers too.
                    // Mirrors GTK's recovery toast Undo at
                    // `meditate-gtk/src/application.rs:408`.
                    if let Some(db_arc) = DATABASE.get() {
                        if let Ok(guard) = db_arc.lock() {
                            if let Some(db) = guard.as_ref() {
                                if let Err(e) =
                                    db.delete_session_by_uuid(&uuid)
                                {
                                    meditate_core::log(
                                        "session.recovery",
                                        &format!(
                                            "undo delete failed uuid={uuid}: {e:?}"
                                        ),
                                    );
                                }
                            }
                        }
                    }
                    ui.set_snackbar_visible(false);
                    reset_log_feed(
                        &ui,
                        &loaded_log_sessions,
                        &pending_deletes,
                    );
                } else {
                    // Delete-undo branch (L-3): restore every
                    // hidden card, drop the pending batch.
                    pending_deletes.borrow_mut().clear();
                    ui.set_snackbar_visible(false);
                    render_log_feed(
                        &ui,
                        &loaded_log_sessions,
                        &pending_deletes,
                    );
                }
            }
            let _ = weak.clone();
        });
    }

    // Log "Add Session" button → open the Edit-Session overlay
    // in create mode: clear `editing_session_id` (None ⇒ the
    // Save handler inserts instead of updating), seed sensible
    // defaults (empty note, 0h0m, start = now, label off).
    // Mirrors GTK's `log_add_btn` → `show_session_dialog(None)`
    // at `meditate-gtk/src/log/imp.rs:315`.
    {
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let editing_session_id = editing_session_id.clone();
        ui.on_log_add_tap(move || {
            #[cfg(target_os = "android")]
            {
                use chrono::{Datelike, Local, Timelike};
                let Some(ui) = weak.upgrade() else { return; };
                editing_session_id.set(None);
                ui.set_edit_session_title("Add Session".into());
                ui.set_edit_note_text("".into());
                ui.set_edit_duration_hours(0);
                ui.set_edit_duration_minutes(0);
                let now = Local::now();
                ui.set_edit_start_date(Date {
                    year: now.year(),
                    month: now.month() as i32,
                    day: now.day() as i32,
                });
                ui.set_edit_start_time(Time {
                    hour: now.hour() as i32,
                    minute: now.minute() as i32,
                    second: now.second() as i32,
                });
                ui.set_edit_label_enabled(false);
                ui.set_edit_label_id(0);
                ui.set_edit_label_name("".into());
                ui.set_edit_session_page(true);
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
                ui.set_edit_session_title("Edit Session".into());
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

                // Field values are read the same way regardless
                // of edit vs create; only the persistence call
                // differs (`update_session` by rowid vs
                // `insert_session` of a fresh row). Mirrors
                // GTK's single `save_btn` handler that branches
                // `session_id` Some/None at `log/imp.rs:1100`.
                let new_note_raw = ui.get_edit_note_text().to_string();
                let new_note = if new_note_raw.is_empty() {
                    None
                } else {
                    Some(new_note_raw)
                };
                // Duration: recompose from the two SpinRows.
                // GTK clamps with `.max(0)` (`log/imp.rs:1093`);
                // the SpinRow min-value guards already keep both
                // factors non-negative, but mirror the clamp on
                // the product to stay defensive against future
                // signed-typed inputs.
                let hours = ui.get_edit_duration_hours().max(0) as i64;
                let mins = ui.get_edit_duration_minutes().max(0) as i64;
                let duration_secs = (hours * 3600 + mins * 60).max(0);
                // Recompose start_time from the picker outputs.
                // Falls back to "now" if the user-edited Date /
                // Time can't be turned into a valid Local moment
                // (e.g., a date inside a DST gap). Mirrors GTK's
                // `glib::DateTime::new(...).map_or_else(unix_now,
                // |d| d.to_unix())` at `log/imp.rs:1072`.
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
                    .unwrap_or_else(meditate_core::time::unix_now);
                // Label (L-4d). Mirrors GTK's
                // `label_expander.enables_expansion() ?
                // selected_label_id : None` branch at
                // `log/imp.rs:1079`.
                let label_id = if ui.get_edit_label_enabled() {
                    let lid = ui.get_edit_label_id() as i64;
                    if lid > 0 { Some(lid) } else { None }
                } else {
                    None
                };

                if let Some(db_arc) = DATABASE.get() {
                    if let Ok(guard) = db_arc.lock() {
                        if let Some(db) = guard.as_ref() {
                            match editing_session_id.get() {
                                Some(id) => {
                                    // Edit: clone the live row,
                                    // swap the edited fields,
                                    // keep mode + guided-file ref
                                    // untouched (the overlay
                                    // can't change them — matches
                                    // GTK's `original_mode` /
                                    // `original_guided_file_uuid`
                                    // preservation).
                                    let original = loaded_log_sessions
                                        .borrow()
                                        .iter()
                                        .find(|(id_, _)| *id_ == id)
                                        .map(|(_, s)| s.clone());
                                    if let Some(mut session) = original {
                                        session.notes = new_note;
                                        session.duration_secs =
                                            duration_secs as u32;
                                        session.start_iso =
                                            meditate_core::time::unix_to_local_iso(
                                                new_start_unix,
                                            );
                                        session.label_id = label_id;
                                        if let Err(err) =
                                            db.update_session(id, &session)
                                        {
                                            meditate_core::log(
                                                "log.edit.save.failed",
                                                &format!("rowid {id}: {err:?}"),
                                            );
                                        }
                                    }
                                }
                                None => {
                                    // Create: a fresh manual
                                    // entry. GTK's add-dialog
                                    // defaults the mode to Timer
                                    // (`original_mode = session
                                    // .map_or(Timer, ...)` at
                                    // `log/imp.rs:771`) and never
                                    // carries a guided-file ref.
                                    let session =
                                        meditate_core::db::Session::from_unix(
                                            new_start_unix,
                                            duration_secs,
                                            label_id,
                                            new_note,
                                            meditate_core::SessionMode::Timer,
                                            None,
                                        );
                                    if let Err(err) =
                                        db.insert_session(&session)
                                    {
                                        meditate_core::log(
                                            "log.add.save.failed",
                                            &format!("{err:?}"),
                                        );
                                    }
                                }
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
                // No explicit `set_edit_label_enabled` — the
                // ExpanderRow's `active <=> root.edit-label-enabled`
                // two-way binding owns the flag (same as the
                // Setup-screen Label expander, which only
                // persists in its toggled handler). This callback
                // just mirrors GTK's
                // `connect_enable_expansion_notify`: adopt the
                // first label when switched on with no selection.
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
                refresh_filter_label_items(&ui);
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
                sync_filter_has_active(&ui);
                reset_log_feed(&ui, &loaded_log_sessions, &pending_deletes);
                ui.set_filter_sheet_open(false);
            }
            let _ = (weak.clone(), _on);
        });
    }

    // Label filter pick (L-5c). Index 0 is the synthetic "All
    // labels" row → clear the filter; index N → the Nth label
    // in `all_labels_ordered`. Instant-apply + sheet close,
    // mirroring GTK's `filter_label_row` notify handler at
    // `meditate-gtk/src/window/imp.rs:799`.
    {
        let weak = ui.as_weak();
        #[cfg(target_os = "android")]
        let loaded_log_sessions = loaded_log_sessions.clone();
        #[cfg(target_os = "android")]
        let pending_deletes = pending_deletes.clone();
        ui.on_filter_label_selected(move |idx| {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                let resolved = if idx <= 0 {
                    0
                } else {
                    all_labels_ordered()
                        .get((idx - 1) as usize)
                        .map(|(id, _)| *id as i32)
                        .unwrap_or(0)
                };
                ui.set_filter_label_id(resolved);
                sync_filter_has_active(&ui);
                reset_log_feed(&ui, &loaded_log_sessions, &pending_deletes);
                ui.set_filter_sheet_open(false);
            }
            let _ = (weak.clone(), idx);
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

    // Sync-indicator tap — no-op placeholder. GTK routes via
    // `meditate_core::sync::indicator::action_for` to the
    // recovery dialog / prefs-data page / retry-sync, none of
    // which exist on Android until Phase 7. Logging the derived
    // action keeps the eventual wiring obvious.
    ui.on_sync_indicator_tap(move || {
        #[cfg(target_os = "android")]
        meditate_core::log(
            "ui.sync_indicator_tap",
            "sync action pending (phase 7: recovery / prefs / retry)",
        );
    });

    // Chart period toggle (S-5a). Recompute just the chart —
    // mirrors GTK's `period_toggle_group` notify → `reload_chart`
    // (the other stats sections don't depend on the period).
    {
        let weak = ui.as_weak();
        ui.on_chart_period_changed(move |_idx| {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                refresh_chart(&ui);
            }
            let _ = (weak.clone(), _idx);
        });
    }

    // Chart Bars/Line toggle (S-5b). The data is identical
    // between modes — `refresh_chart` always builds both the
    // bar ratios and the line/area path strings — so a kind
    // switch just needs a recompute (the Slint `if` picks the
    // right renderer off `stat-chart-kind`).
    {
        let weak = ui.as_weak();
        ui.on_chart_kind_changed(move |_idx| {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                refresh_chart(&ui);
            }
            let _ = (weak.clone(), _idx);
        });
    }

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
                // Tab order mirrors GTK: 0 Timer, 1 Stats,
                // 2 Log.
                if idx == 2 {
                    // Entering Log — reload from DB so a session
                    // saved before opening the tab shows up.
                    reset_log_feed(&ui, &loaded_log_sessions, &pending_deletes);
                }
                if idx == 1 {
                    // Entering Stats — recompute from the DB so
                    // a session saved this run is reflected.
                    // Mirrors GTK's `reload_all` on stats-tab
                    // entry.
                    refresh_stats(&ui);
                }
                // Cheap re-read on every tab switch — mirrors
                // GTK refreshing the indicator on view change
                // (`wire_log_signals` visible-child notify).
                refresh_sync_indicator(&ui);
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
        // The system back gesture closes the chooser overlays
        // here, bypassing their in-app back-button handlers — so
        // it must silence any in-flight preview too, or the
        // bell / vibration keeps playing after the page is gone.
        #[cfg(target_os = "android")]
        let bell_preview = bell_preview.clone();
        #[cfg(target_os = "android")]
        let pattern_preview = pattern_preview.clone();
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
            #[cfg(target_os = "android")]
            if ui.get_prep_dialog_open() {
                ui.set_prep_dialog_open(false);
                return;
            }
            // Preset modals first, then the preset chooser
            // overlay (innermost-out, like the other layers).
            #[cfg(target_os = "android")]
            if ui.get_rename_preset_dialog_open() {
                ui.set_rename_preset_dialog_open(false);
                return;
            }
            #[cfg(target_os = "android")]
            if ui.get_delete_preset_dialog_open() {
                ui.set_delete_preset_dialog_open(false);
                return;
            }
            #[cfg(target_os = "android")]
            if ui.get_create_preset_dialog_open() {
                ui.set_create_preset_dialog_open(false);
                return;
            }
            #[cfg(target_os = "android")]
            if ui.get_override_preset_dialog_open() {
                ui.set_override_preset_dialog_open(false);
                return;
            }
            #[cfg(target_os = "android")]
            if ui.get_preset_chooser_page() {
                ui.set_preset_chooser_page(false);
                return;
            }
            #[cfg(target_os = "android")]
            if ui.get_bell_chooser_page() {
                let _ = bell_preview.borrow_mut().stop();
                if let Some(app) = android_app() {
                    audio::stop(app);
                }
                ui.set_bell_preview_uuid(slint::SharedString::new());
                ui.set_bell_chooser_page(false);
                return;
            }
            #[cfg(target_os = "android")]
            if ui.get_pattern_chooser_page() {
                let _ = pattern_preview.borrow_mut().stop();
                if let Some(app) = android_app() {
                    haptics::cancel(app);
                }
                ui.set_pattern_preview_uuid(slint::SharedString::new());
                ui.set_pattern_chooser_page(false);
                return;
            }
            #[cfg(target_os = "android")]
            if ui.get_interval_editor_page() {
                ui.set_interval_editor_page(false);
                return;
            }
            #[cfg(target_os = "android")]
            if ui.get_interval_bells_page() {
                ui.set_interval_bells_page(false);
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
            if ui.get_nav_page() != 0 {
                // Any non-Timer base tab (Stats / Log) → back
                // navigates to Timer, the start destination —
                // canonical bottom-nav back behaviour on Android.
                ui.set_nav_page(0);
                return;
            }
            // Running page back is swallowed by the FocusScope
            // accepting the event but doing nothing here — keeps
            // a session safe from a stray back gesture.
            // Idle setup page: nothing to do.
        });
    }

    // Crash-recovery Undo snackbar (L-6). Drain the single-shot
    // stash `open_database` parked the rescued session in; if
    // present, raise the shared Snackbar in recovery mode.
    // Mirrors GTK's `present_recovery_toast_if_pending` at
    // `meditate-gtk/src/application.rs:374`: the row already
    // exists, the snackbar just narrates it + offers a one-tap
    // undo. 8 s timeout matches GTK's recovery toast (vs the
    // 5 s delete toast); on expiry the session is simply kept.
    #[cfg(target_os = "android")]
    if let Some(slot) = RECOVERED_SESSION.get() {
        let rescued = slot.lock().ok().and_then(|mut g| g.take());
        if let Some((uuid, secs)) = rescued {
            let minutes = secs / 60;
            // Same wording as GTK's
            // `Announcement::SessionRecovered` renderer at
            // `meditate-gtk/src/announcement.rs:17`. i18n isn't
            // wired on Android yet (see the delete-snackbar
            // note); inline English until it is.
            let text = if minutes == 1 {
                "Recovered 1 min session".to_string()
            } else {
                format!("Recovered {minutes} min session")
            };
            *recovery_uuid.borrow_mut() = Some(uuid);
            // Single slot: a recovery snackbar supersedes any
            // in-flight preset Undo context.
            pending_preset_undo.borrow_mut().take();
            pending_preset_delete.borrow_mut().take();
            pending_override_restore.borrow_mut().take();
            ui.set_snackbar_text(text.into());
            ui.set_snackbar_visible(true);
            let weak_inner = ui.as_weak();
            delete_timer.start(
                slint::TimerMode::SingleShot,
                std::time::Duration::from_secs(8),
                move || {
                    let Some(ui) = weak_inner.upgrade() else { return; };
                    // Timed out without Undo → keep the session,
                    // just dismiss. (The recovery context is
                    // cleared lazily by the next delete tap or
                    // an Undo press; leaving it set is harmless
                    // since the snackbar is hidden.)
                    ui.set_snackbar_visible(false);
                },
            );
        }
    }

    // Cold-start widget deep-link: all callbacks are registered
    // by here, so the helper's `invoke_*` calls run the real
    // chip-switch + Start closures. Warm starts go through the
    // tick loop instead (NativeActivity gives native code no
    // onNewIntent — see `try_widget_deep_link`).
    #[cfg(target_os = "android")]
    try_widget_deep_link(&ui, &timer_session_secs, &state);

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
    // Publish the AndroidApp before slint::android::init consumes
    // it: the AppState transition closures reach the JNI bridges
    // through it later. `set_` (not OnceLock) because android_main
    // re-runs on every NativeActivity recreate within a surviving
    // process — each run must replace the handle or the JNI
    // bridges keep targeting the destroyed activity.
    set_android_app(android_app.clone());
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
            // B-3a: bell-sound audio is the Android-specific bit
            // the core seed deliberately omits — extract the
            // bundled OGGs to disk and seed `bell_sounds` with
            // their absolute paths (idempotent extraction,
            // one-shot seed).
            sounds::extract_and_seed(&db, &dir);
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
                Ok(Some(finalized)) => {
                    meditate_core::log(
                        "session.recovery",
                        &format!(
                            "finalized uuid={} duration_secs={}",
                            finalized.session_uuid, finalized.duration_secs,
                        ),
                    );
                    // Park for `build_ui` to raise the recovery
                    // Undo snackbar (L-6). Single-shot.
                    let slot = RECOVERED_SESSION
                        .get_or_init(|| std::sync::Mutex::new(None));
                    if let Ok(mut guard) = slot.lock() {
                        *guard = Some((
                            finalized.session_uuid,
                            finalized.duration_secs,
                        ));
                    }
                }
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
    // Seed the widget projection once at startup so a widget
    // added while the app was dead still renders the current
    // starred set on first pull (the seeded default presets are
    // starred). ANDROID_APP is set in `android_main` before this
    // call, so the JNI poke resolves; it no-ops when no widget is
    // installed. Must run after DATABASE.set — the snapshot locks
    // it.
    refresh_widget();
}
