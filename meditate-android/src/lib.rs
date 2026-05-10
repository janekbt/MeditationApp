pub mod app;

slint::include_modules!();

use app::{format_mmss, AppState};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

// Default session length until milestone 5 brings in a duration
// picker. Matches the GTK shell's default starting position for
// a freshly opened Timer mode.
const SESSION_DURATION: Duration = Duration::from_secs(10 * 60);

// Tick interval driving the mm:ss redraw + Running→Finished detection.
// 200ms keeps the seconds digit visually responsive without burning
// CPU on a phone display.
const TICK: Duration = Duration::from_millis(200);

/// Monotonic seconds since process start. CLOCK_BOOTTIME-backed on
/// Android (Rust 1.79+) and CLOCK_MONOTONIC on desktop Linux — the
/// latter is fine for dev-iteration runs that never see a real
/// suspend.
fn now_since_epoch() -> Duration {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    Instant::now().duration_since(*EPOCH.get_or_init(Instant::now))
}

fn refresh(ui: &MainWindow, state: &AppState, now: Duration) {
    ui.set_remaining_text(format_mmss(state.remaining(SESSION_DURATION, now)).into());
    ui.set_action_label(state.primary_label().into());
    ui.set_stop_visible(state.can_stop());
    ui.set_running_page(state.is_running_page());
}

fn build_ui() -> MainWindow {
    let ui = MainWindow::new().unwrap();
    let state = Rc::new(RefCell::new(AppState::idle()));

    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_action_tap(move || {
            let now = now_since_epoch();
            let mut s = state.borrow_mut();
            let next = std::mem::replace(&mut *s, AppState::idle())
                .toggle(SESSION_DURATION, now);
            *s = next;
            if let Some(ui) = weak.upgrade() {
                refresh(&ui, &s, now);
            }
        });
    }

    {
        let weak = ui.as_weak();
        let state = state.clone();
        ui.on_stop_tap(move || {
            let mut s = state.borrow_mut();
            *s = std::mem::replace(&mut *s, AppState::idle()).stop();
            if let Some(ui) = weak.upgrade() {
                refresh(&ui, &s, now_since_epoch());
            }
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
            *s = std::mem::replace(&mut *s, AppState::idle()).tick(now);
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
    slint::android::init(android_app).unwrap();
    let ui = build_ui();
    MaterialWindowAdapter::get(&ui).set_disable_hover(true);
    ui.run().unwrap();
}
