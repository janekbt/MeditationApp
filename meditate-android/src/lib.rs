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
        ui.on_action_tap(move || {
            let now = now_since_epoch();
            let Some(ui) = weak.upgrade() else { return; };
            let target = configured_duration(&ui);
            // Shape picked from the Stopwatch-Mode switch — same
            // shell-side choice the GTK `on_start` makes via
            // `stopwatch_toggle_on`. Box-Breath and Guided shapes
            // arrive in the next phase-2 slices; for now only Timer
            // mode reaches Start (the chip group's Guided/Box Breath
            // selections disable the Start button).
            let shape = if ui.get_stopwatch_on() {
                meditate_core::session::SessionShape::TimerStopwatch
            } else {
                meditate_core::session::SessionShape::TimerCountdown {
                    target_secs: target.as_secs() as u32,
                }
            };
            let mut s = state.borrow_mut();
            // No live elapsed capture needed here: action_tap on Active
            // pauses/resumes — both stay Active, so the session never
            // ends through this path. Only Idle/Finished → Active
            // matters for persistence wiring.
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
            // so the Save / Discard handler knows what to do, and
            // push the elapsed readout into the Done view.
            #[cfg(target_os = "android")]
            if was_active && !is_active {
                if let Some(unix_start) = session_start_unix.take() {
                    pending_done.set(Some((unix_start, elapsed_secs)));
                }
            }
            if let Some(ui) = weak.upgrade() {
                ui.set_elapsed_text(
                    meditate_core::format::format_time(
                        Duration::from_secs(elapsed_secs.max(0) as u64),
                    )
                    .into(),
                );
                // Clear the note from any previous session so the
                // Done screen opens with an empty editor.
                ui.set_note_text("".into());
            }
            on_state_changed(was_active, is_active);
            if let Some(ui) = weak.upgrade() {
                refresh(&ui, &s, now);
            }
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
        #[cfg(target_os = "android")]
        let session_start_unix = session_start_unix.clone();
        #[cfg(target_os = "android")]
        let pending_done = pending_done.clone();
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
                if let Some(ui) = weak.upgrade() {
                    ui.set_elapsed_text(
                        meditate_core::format::format_time(
                            Duration::from_secs(elapsed_secs.max(0) as u64),
                        )
                        .into(),
                    );
                    ui.set_note_text("".into());
                }
            }
            on_state_changed(was_active, is_active);
            if let Some(ui) = weak.upgrade() {
                refresh(&ui, &s, now);
            }
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
                // Resolve the active-mode label only when the
                // master toggle is on — mirrors the GTK shell's
                // `setup_selected_label_id`. Done-screen-side
                // override (`done_selected_label_id` in GTK) lands
                // alongside C-2's chooser screen; until then Save
                // commits the Setup's resolved label.
                let label_id = if read_label_active_for_mode(mode) {
                    resolved_label_for_mode(mode).map(|(_, id)| id)
                } else {
                    None
                };
                finalize_session(unix_start, elapsed_secs, note, mode, label_id);
            }
            #[cfg(not(target_os = "android"))]
            let _ = current_mode.get();
            let mut s = state.borrow_mut();
            *s = std::mem::replace(&mut *s, AppState::idle()).dismiss();
            refresh(&ui, &s, now_since_epoch());
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
            }
            // Touch the weak handle so the host build doesn't flag
            // it unused (the android-only block above is the sole
            // consumer).
            let _ = weak;
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

    // Label inner-row tap — load the labels list with the active
    // mode's current selection marked, then open the chooser.
    // Mirrors the GTK chain `setup_label_chooser_row.activated → resolve_label_for_mode →
    // push_label_chooser(current_id, on_selected)`.
    {
        let weak = ui.as_weak();
        let current_mode = current_mode.clone();
        ui.on_label_tap(move || {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                let mode: meditate_core::SessionMode = current_mode.get().into();
                let current_id = if read_label_active_for_mode(mode) {
                    resolved_label_for_mode(mode).map(|(_, id)| id)
                } else {
                    None
                };
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
                ui.set_labels_page(true);
            }
            let _ = (weak.clone(), current_mode.get());
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

    // User picked a label row — persist the new UUID for the
    // active mode, refresh the ExpanderRow's resolved name, and
    // close the overlay. Mirrors the GTK chooser's `on_selected`
    // callback at `imp.rs:744-749`.
    {
        let weak = ui.as_weak();
        let current_mode = current_mode.clone();
        ui.on_label_picked(move |id| {
            #[cfg(target_os = "android")]
            {
                let Some(ui) = weak.upgrade() else { return; };
                let mode: meditate_core::SessionMode = current_mode.get().into();
                if let Some(uuid) = lookup_label_uuid(id as i64) {
                    write_label_uuid_for_mode(mode, &uuid);
                }
                ui.set_label_name(
                    resolved_label_for_mode(mode)
                        .map(|(name, _)| name)
                        .unwrap_or_default()
                        .into(),
                );
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
