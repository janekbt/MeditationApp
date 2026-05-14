pub mod app;
#[cfg(target_os = "android")]
mod service;

slint::include_modules!();

use app::AppState;
#[cfg(target_os = "android")]
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
    ui.set_remaining_text(state.hero_label(target, now).into());
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
}

/// Snapshot of an in-flight session that the persistence layer needs
/// at end time. `unix_start` is captured at the Idle/Finished → Active
/// transition (mirrors the GTK shell's `session_start_time` cell);
/// elapsed comes from the live core::Session and is captured BEFORE
/// the AppState mutation drops the session.
#[cfg(target_os = "android")]
fn finalize_session(unix_start: i64, elapsed_secs: i64) {
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
        // Phase 2 will add a label picker; for Phase 1 every session
        // lands unlabeled. Same goes for notes (no notes editor yet).
        None,
        None,
        // Box Breath + Guided live behind their own Slint Setup mode
        // chips — UI Phase 2 / Phase 5 work. Until they ship the only
        // mode the shell can author is plain Timer.
        meditate_core::SessionMode::Timer,
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
    // Unix timestamp captured at session start, read+cleared at end.
    // Mirrors the GTK shell's `Timer::session_start_time` cell.
    // Holds None while idle; Some(unix_secs) while a session is
    // in flight. The core::Session itself uses monotonic boot-time
    // durations, so wall-clock start has to be carried separately.
    // android-only — host has no DB to persist into, so we don't
    // even allocate the cell on the desktop preview path.
    #[cfg(target_os = "android")]
    let session_start_unix: Rc<Cell<Option<i64>>> = Rc::new(Cell::new(None));

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
            let mut s = state.borrow_mut();
            // No live elapsed capture needed here: action_tap on Active
            // pauses/resumes — both stay Active, so the session never
            // ends through this path. Only Idle/Finished → Active
            // matters for persistence wiring.
            let was_active = s.is_active();
            let next = std::mem::replace(&mut *s, AppState::idle())
                .toggle(target, now);
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
        ui.on_stop_tap(move || {
            let now = now_since_epoch();
            let mut s = state.borrow_mut();
            // Capture elapsed BEFORE the mutation — AppState::stop
            // returns Self::Idle and drops the core::Session, so we
            // can't ask it for elapsed afterwards.
            #[cfg(target_os = "android")]
            let elapsed_secs = match &*s {
                AppState::Active(session) => session.elapsed(now).as_secs() as i64,
                _ => 0,
            };
            let was_active = s.is_active();
            *s = std::mem::replace(&mut *s, AppState::idle()).stop();
            let is_active = s.is_active();
            #[cfg(target_os = "android")]
            if was_active && !is_active {
                if let Some(unix_start) = session_start_unix.take() {
                    finalize_session(unix_start, elapsed_secs);
                }
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
        timer.start(slint::TimerMode::Repeated, TICK, move || {
            let now = now_since_epoch();
            let mut s = state.borrow_mut();
            // Capture before/after active-state so an auto-finish
            // (Active → Finished on Overtime cross) tears down the
            // foreground service AND persists the session. tick on
            // an inactive state is a no-op, so the equality check
            // is the cheap path.
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
                    finalize_session(unix_start, elapsed_secs);
                }
            }
            on_state_changed(was_active, is_active);
            if let Some(ui) = weak.upgrade() {
                refresh(&ui, &s, now);
            }
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
