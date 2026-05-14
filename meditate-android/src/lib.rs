pub mod app;
#[cfg(target_os = "android")]
mod service;

slint::include_modules!();

use app::AppState;
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

fn build_ui() -> MainWindow {
    let ui = MainWindow::new().unwrap();
    let state = Rc::new(RefCell::new(AppState::idle()));

    // Seed the stepper-driven duration with the same default the
    // GTK shell opens at. The tick loop further down refreshes the
    // Setup hero every 200 ms so stepper changes flow into the
    // big mm:ss readout without a dedicated change-notification path.
    ui.set_setup_hours(DEFAULT_HOURS);
    ui.set_setup_minutes(DEFAULT_MINUTES);

    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_action_tap(move || {
            let now = now_since_epoch();
            let Some(ui) = weak.upgrade() else { return; };
            let target = configured_duration(&ui);
            let mut s = state.borrow_mut();
            let was_active = s.is_active();
            let next = std::mem::replace(&mut *s, AppState::idle())
                .toggle(target, now);
            *s = next;
            on_state_changed(was_active, s.is_active());
            refresh(&ui, &s, now);
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_stop_tap(move || {
            let mut s = state.borrow_mut();
            let was_active = s.is_active();
            *s = std::mem::replace(&mut *s, AppState::idle()).stop();
            on_state_changed(was_active, s.is_active());
            if let Some(ui) = weak.upgrade() {
                refresh(&ui, &s, now_since_epoch());
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
        timer.start(slint::TimerMode::Repeated, TICK, move || {
            let now = now_since_epoch();
            let mut s = state.borrow_mut();
            // Capture before/after active-state so an auto-finish
            // (Active → Finished on Overtime cross) tears down the
            // foreground service. tick on an inactive state is a
            // no-op, so the equality check is the cheap path.
            let was_active = s.is_active();
            *s = std::mem::replace(&mut *s, AppState::idle()).tick(now);
            on_state_changed(was_active, s.is_active());
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
    slint::android::init(android_app).unwrap();
    let ui = build_ui();
    MaterialWindowAdapter::get(&ui).set_disable_hover(true);
    ui.run().unwrap();
}
