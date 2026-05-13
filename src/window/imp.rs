use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{glib, CompositeTemplate};
use std::time::Duration;

use gtk::gio;

use meditate_core::format::format_time;

use crate::log::LogView;
use crate::stats::StatsView;
use crate::timer::TimerView;

#[derive(Debug, Default, CompositeTemplate)]
#[template(resource = "/io/github/janekbt/Meditate/ui/window.ui")]
pub struct MeditateWindow {
    #[template_child] pub view_stack:       TemplateChild<adw::ViewStack>,
    #[template_child] pub switcher_bar:     TemplateChild<adw::ViewSwitcherBar>,
    #[template_child] pub nav_view:         TemplateChild<adw::NavigationView>,
    #[template_child] pub toast_overlay:    TemplateChild<adw::ToastOverlay>,
    #[template_child] pub timer_view:       TemplateChild<TimerView>,
    #[template_child] pub log_view:         TemplateChild<LogView>,
    #[template_child] pub stats_view:       TemplateChild<StatsView>,
    #[template_child] pub log_add_btn:      TemplateChild<gtk::Button>,
    #[template_child] pub log_filter_btn:   TemplateChild<gtk::MenuButton>,
    #[template_child] pub filter_notes_row: TemplateChild<adw::SwitchRow>,
    #[template_child] pub filter_label_row: TemplateChild<adw::ComboRow>,
    #[template_child] pub sync_status_btn:     TemplateChild<gtk::Button>,
    #[template_child] pub sync_status_stack:   TemplateChild<gtk::Stack>,
    #[template_child] pub sync_status_icon:    TemplateChild<gtk::Image>,
    #[template_child] pub sync_status_spinner: TemplateChild<gtk::Spinner>,
}

#[glib::object_subclass]
impl ObjectSubclass for MeditateWindow {
    const NAME: &'static str = "MeditateWindow";
    type Type = super::MeditateWindow;
    type ParentType = adw::ApplicationWindow;

    fn class_init(klass: &mut Self::Class) {
        TimerView::ensure_type();
        LogView::ensure_type();
        StatsView::ensure_type();
        klass.bind_template();
    }

    fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
        obj.init_template();
    }
}

impl ObjectImpl for MeditateWindow {
    fn constructed(&self) {
        self.parent_constructed();
        self.wire_timer_signals();
        self.wire_log_signals();
        self.wire_stats_signals();
        self.wire_sync_status();
        self.setup_help_overlay();
        self.setup_window_actions();
        self.bind_settings();

        // Blueprint may silently drop icon-name on AdwViewStackPage in some
        // compiler versions.  Set it explicitly here so we bypass that.
        self.view_stack.page(&*self.stats_view).set_icon_name(Some("chart-bar-symbolic"));

        // Refresh streak, pre-warm the stats/log views, and pre-load the
        // end-of-session audio once the window is mapped. Each step yields
        // back to the frame clock before the next runs, so the compositor
        // can commit frames in between and touch input stays responsive.
        // Previously all four ran inside one idle callback — ~290 ms of
        // Rust work on the main thread blocking frame 2 on Librem 5.
        let obj = self.obj();
        obj.connect_map(|window| {
            let weak = window.downgrade();
            // `idle_add_local_once` runs at GLib's DEFAULT_IDLE priority —
            // strictly lower than the frame-clock, so the future doesn't
            // start polling until frame 0 has been presented. Without this
            // outer defer, spawn_local kicks off inside the map handler
            // and runs its entire body before frame 0's paint phase,
            // cramming 300 ms of refresh work into the first visible frame.
            glib::idle_add_local_once(move || {
                glib::MainContext::default().spawn_local(async move {
                    use std::time::Duration;
                    // 16 ms = one frame clock tick; guarantees a yield
                    // past the current frame so the compositor can commit
                    // between each refresh step instead of batching them.
                    let yield_frame = || glib::timeout_future(Duration::from_millis(16));

                    if let Some(w) = weak.upgrade() {
                        w.imp().timer_view.refresh_streak();
                    }
                    yield_frame().await;

                    if let Some(w) = weak.upgrade() {
                        w.imp().stats_view.refresh();
                    }
                    yield_frame().await;

                    if let Some(w) = weak.upgrade() {
                        w.imp().log_view.refresh();
                    }
                    yield_frame().await;

                    if let Some(w) = weak.upgrade() {
                        if let Some(app) = w.application()
                            .and_then(|a| a.downcast::<crate::application::MeditateApplication>().ok())
                        {
                            crate::sound::preload_end_bell(&app);
                        }
                    }
                });
            });
        });
    }
}

impl WidgetImpl for MeditateWindow {}
impl WindowImpl for MeditateWindow {}
impl ApplicationWindowImpl for MeditateWindow {}
impl AdwApplicationWindowImpl for MeditateWindow {}

// ── Timer ─────────────────────────────────────────────────────────────────────

impl MeditateWindow {
    fn wire_timer_signals(&self) {
        let obj = self.obj();

        self.view_stack.connect_notify_local(
            Some("visible-child"),
            glib::clone!(
                #[weak] obj,
                move |stack, _| {
                    if stack.visible_child_name().as_deref() == Some("timer") {
                        obj.imp().timer_view.refresh_streak();
                    }
                }
            ),
        );

        self.timer_view.connect_timer_started(glib::clone!(
            #[weak] obj,
            move |_| obj.imp().push_running_page()
        ));
        // No timer-paused handler: the running page stays put on
        // pause now. The Pause button morphs to "Resume" in place
        // (see TimerView::on_pause) instead of popping the user
        // back to the dimmed setup view.
        self.timer_view.connect_timer_stopped(glib::clone!(
            #[weak] obj,
            move |_| {
                if obj.imp().nav_view.find_page("running").is_some() {
                    obj.imp().nav_view.pop();
                }
            }
        ));
    }

    pub fn push_running_page(&self) {
        if self.nav_view.find_page("running").is_some() {
            return;
        }
        if self.timer_view.is_breathing_mode() {
            self.push_breathing_running_page();
        } else {
            self.push_time_running_page();
        }
    }

    fn push_time_running_page(&self) {
        let time_label = gtk::Label::builder()
            .label(format_time(Duration::from_secs(self.timer_view.current_display_secs())))
            .css_classes(["timer-setup-display"])
            .halign(gtk::Align::Center)
            .build();
        self.timer_view.set_running_label(time_label.clone());

        // Pause is a regular action (non-destructive, reversible via Resume);
        // plain .pill. Stop ends the meditation early and surfaces the done
        // screen where the user must then pick Save or Discard — the action
        // is consequential enough that the HIG destructive tint is warranted
        // here, matching the Discard and Stop-from-Pause styling elsewhere.
        let pause_btn = gtk::Button::builder()
            .label("Pause")
            .css_classes(["pill"])
            .tooltip_text("Pause Timer")
            .build();
        let stop_btn = gtk::Button::builder()
            .label("Stop")
            .css_classes(["pill", "destructive-action"])
            .tooltip_text("Stop and Save Session")
            .build();

        // Sit-longer mode "Add MM:SS ?" — only visible after the
        // countdown reaches zero. Tapping it commits the elapsed
        // overtime as part of the session duration. Hidden in
        // Running, in stopwatch mode, and in Box Breath since none
        // of those have a countdown to overshoot. Wrapped in
        // Adw.Clamp so it stays at a comfortable pill width on
        // wide windows rather than stretching across the running
        // page; tightening-threshold lets it shrink gracefully on
        // phone-sized screens. Visibility lives on the Clamp (the
        // imp walks `add_btn.parent()` to flip it).
        let add_btn = gtk::Button::builder()
            .label("Add 00:00 ?")
            .css_classes(["suggested-action", "pill", "tabular-nums"])
            .tooltip_text("Include the elapsed overtime in the session")
            .build();
        let add_clamp = adw::Clamp::builder()
            .maximum_size(180)
            .tightening_threshold(120)
            .child(&add_btn)
            .visible(false)
            .build();
        self.timer_view.set_running_pause_btn(pause_btn.clone());
        self.timer_view.set_running_overtime_widgets(
            stop_btn.clone(),
            add_btn.clone(),
        );
        let obj_for_add = self.obj().clone();
        add_btn.connect_clicked(move |_| {
            obj_for_add.imp().timer_view.add_overtime_and_finish();
        });

        let btn_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .halign(gtk::Align::Center)
            .build();
        btn_box.append(&pause_btn);
        btn_box.append(&stop_btn);

        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(32)
            .valign(gtk::Align::Center)
            .vexpand(true)
            .margin_top(24).margin_bottom(24)
            .margin_start(12).margin_end(12)
            .build();
        content.append(&time_label);
        content.append(&add_clamp);
        content.append(&btn_box);

        self.push_running_page_with_content("Meditating", content, pause_btn, stop_btn);
    }

    /// Build the Box-Breath running page: session-countdown strip at top,
    /// animated square frame in the middle (cairo-drawn frame + perimeter
    /// dot), phase label + per-phase countdown inside, Pause/Stop below.
    fn push_breathing_running_page(&self) {
        use meditate_core::breath::BreathPattern;
        use std::cell::Cell;
        use std::rc::Rc;

        let pattern = self.timer_view.breathing_pattern();
        let target_secs = self.timer_view.breathing_target_secs();
        let stopwatch_active = self.timer_view.stopwatch_active();

        // ── Top strip: "BOX BREATHING" eyebrow + counter ──
        // Counter shows "elapsed / target" when stopwatch is off
        // and just "elapsed" when on (no fixed end).
        let eyebrow = gtk::Label::builder()
            .label(crate::i18n::gettext("Box Breathing"))
            .css_classes(["caption", "dimmed"])
            .halign(gtk::Align::Center)
            .build();
        let initial_counter = meditate_core::format::box_breath_counter_label(
            Duration::ZERO,
            if stopwatch_active { None } else { Some(Duration::from_secs(target_secs)) },
        );
        let counter_label = gtk::Label::builder()
            .label(&initial_counter)
            .css_classes(["title-3", "numeric"])
            .halign(gtk::Align::Center)
            .build();

        // ── Center: DrawingArea for the square + dot, overlaid with
        //    phase label + large per-phase countdown.
        let drawing_area = gtk::DrawingArea::builder()
            .content_width(220)
            .content_height(220)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .build();
        drawing_area.add_css_class("accent");

        let phase_label = gtk::Label::builder()
            .label("")
            .css_classes(["caption", "accent"])
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .build();
        let phase_seconds_label = gtk::Label::builder()
            .label("")
            .css_classes(["title-1", "numeric"])
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .build();

        // Labels are stacked vertically inside the square, both centred.
        let inner_stack = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .build();
        inner_stack.append(&phase_label);
        inner_stack.append(&phase_seconds_label);

        let overlay = gtk::Overlay::builder()
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .build();
        overlay.set_child(Some(&drawing_area));
        overlay.add_overlay(&inner_stack);

        // Cairo draw function: rounded-square frame with accent tint + stroke,
        // and a single white-filled dot with accent halo travelling the
        // perimeter. Per user request the progress-stroke trail is omitted —
        // only the dot moves.
        let pattern_cell: Rc<Cell<BreathPattern>> = Rc::new(Cell::new(pattern));
        {
            let pattern_cell = pattern_cell.clone();
            let obj = self.obj().clone();
            drawing_area.set_draw_func(move |widget, cr, w, h| {
                let size = w.min(h) as f64;
                let pad = 12.0;
                let side = size - 2.0 * pad;
                let radius = 20.0;

                // Accent RGBA from the style context, with alpha tweaks for
                // the filled tint vs the stroke. Falling back to a plain
                // grey if the accent lookup somehow fails.
                let (ar, ag, ab) = accent_rgb(widget);

                // Filled rounded-square (tint).
                rounded_rect(cr, pad, pad, side, side, radius);
                cr.set_source_rgba(ar, ag, ab, 0.08);
                let _ = cr.fill();

                // Stroked frame.
                rounded_rect(cr, pad, pad, side, side, radius);
                cr.set_source_rgba(ar, ag, ab, 0.45);
                cr.set_line_width(1.5);
                let _ = cr.stroke();

                // Dot position — only rendered while we actually have a
                // non-zero cycle; otherwise the widget just shows the empty
                // frame (which is what the user sees for a split-second at
                // start-of-session before the tick fires).
                let p = pattern_cell.get();
                if p.cycle().is_zero() {
                    return;
                }
                let elapsed = obj.imp().timer_view.breath_elapsed();
                let info = p.phase_at(elapsed);
                let phase = info.phase;
                let t = (info.elapsed_in_phase.as_secs_f64()
                    / info.total.as_secs_f64()).clamp(0.0, 1.0);

                // Perimeter geometry lives in core so the Android
                // running page draws the same path.
                let (x, y) = meditate_core::breath::perimeter_point(phase, t, pad, side);

                // Halo (semi-transparent accent).
                cr.set_source_rgba(ar, ag, ab, 0.30);
                cr.arc(x, y, 11.0, 0.0, std::f64::consts::TAU);
                let _ = cr.fill();
                // Dot body — white filled, accent-bordered.
                cr.set_source_rgb(1.0, 1.0, 1.0);
                cr.arc(x, y, 7.0, 0.0, std::f64::consts::TAU);
                let _ = cr.fill();
                cr.set_source_rgba(ar, ag, ab, 1.0);
                cr.set_line_width(2.5);
                cr.arc(x, y, 7.0, 0.0, std::f64::consts::TAU);
                let _ = cr.stroke();
            });
        }

        // ── Pause / Stop buttons ──────────────────────────────────────
        let pause_btn = gtk::Button::builder()
            .label(crate::i18n::gettext("Pause"))
            .css_classes(["pill"])
            .tooltip_text(crate::i18n::gettext("Pause Timer"))
            .build();
        let stop_btn = gtk::Button::builder()
            .label(crate::i18n::gettext("Stop"))
            .css_classes(["pill", "destructive-action"])
            .tooltip_text(crate::i18n::gettext("Stop and Save Session"))
            .build();

        let btn_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .halign(gtk::Align::Center)
            .build();
        btn_box.append(&pause_btn);
        btn_box.append(&stop_btn);
        // Mirror the timer-mode running page: stash the pause button
        // on TimerView so on_pause / on_resume can morph the label
        // (Pause ↔ Resume) in place without popping back to setup.
        self.timer_view.set_running_pause_btn(pause_btn.clone());

        let top_strip = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .halign(gtk::Align::Center)
            .build();
        top_strip.append(&eyebrow);
        top_strip.append(&counter_label);

        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(24)
            .valign(gtk::Align::Center)
            .vexpand(true)
            .margin_top(24).margin_bottom(24)
            .margin_start(12).margin_end(12)
            .build();
        content.append(&top_strip);
        content.append(&overlay);
        content.append(&btn_box);

        // Install the per-frame tick callback on the drawing area. Elapsed
        // time is now read from `meditate_core::timer::Stopwatch` via the
        // TimerView's `breath_elapsed()`; that value is wall-clock anchored
        // and freezes during pause, so the tick body just reads + renders.
        let da_weak = drawing_area.downgrade();
        let counter_weak = counter_label.downgrade();
        let phase_lbl_weak = phase_label.downgrade();
        let phase_sec_weak = phase_seconds_label.downgrade();
        let obj = self.obj().clone();
        let pattern_for_tick = pattern;
        // Stage 4 of item 13: phase-boundary cue firing + cycle-
        // aligned end are owned by the portable Session. The frame
        // tick still drives the visual rendering (phase label, dot,
        // counter), but the *decisions* now flow through Session
        // effects.
        drawing_area.add_tick_callback(move |_, _clock| {
            use meditate_core::session::Effect as CoreSessionEffect;
            let tv = obj.imp().timer_view.clone();

            let effects = tv.imp().box_breath_session_tick();
            let session_ended = effects
                .iter()
                .any(|e| matches!(e, CoreSessionEffect::EndBoxBreath { .. }));
            // FireBoxBreathCue + FireEndBell route through the shared
            // dispatcher; UpdateDisplay stays redundant (counter reads
            // breath_elapsed() at frame rate already).
            tv.imp().dispatch_session_effects(&effects);

            // Visual render. breath_elapsed() is wall-clock anchored
            // and freezes on pause, so it's the right input for both
            // the dot's perimeter position and the counter.
            let elapsed = tv.breath_elapsed();
            let cur = elapsed.as_secs_f64();
            let info = pattern_for_tick.phase_at(elapsed);
            let phase = info.phase;

            use meditate_core::breath::PhaseRunningLabelKey;
            let phase_name = match meditate_core::breath::phase_running_label_key(phase) {
                PhaseRunningLabelKey::BreatheIn => crate::i18n::gettext("Breathe in"),
                PhaseRunningLabelKey::Hold => crate::i18n::gettext("Hold"),
                PhaseRunningLabelKey::BreatheOut => crate::i18n::gettext("Breathe out"),
            };
            if let Some(l) = phase_lbl_weak.upgrade() {
                l.set_label(&phase_name);
            }
            if let Some(l) = phase_sec_weak.upgrade() {
                let remaining = info.remaining.as_secs_f64().ceil().max(0.0) as i64;
                l.set_label(&remaining.to_string());
            }
            if let Some(l) = counter_weak.upgrade() {
                l.set_label(&meditate_core::format::box_breath_counter_label(
                    Duration::from_secs(cur as u64),
                    if stopwatch_active { None } else { Some(Duration::from_secs(target_secs)) },
                ));
            }
            if let Some(da) = da_weak.upgrade() {
                da.queue_draw();
            }

            // Cycle-aligned stop, signalled by the Session's
            // EndBoxBreath effect above. Use finish_breath_session
            // (natural completion: chime, vibration, notification)
            // rather than stop() (user-initiated, silent).
            if session_ended {
                tv.finish_breath_session();
                return glib::ControlFlow::Break;
            }
            glib::ControlFlow::Continue
        });

        self.push_running_page_with_content("Box Breathing", content, pause_btn, stop_btn);
    }

    /// Shared scaffolding for both running pages: wraps the content in an
    /// AdwNavigationPage with a header, wires Pause/Stop/Esc, pushes onto
    /// the nav view.
    fn push_running_page_with_content(
        &self,
        title: &str,
        content: gtk::Box,
        pause_btn: gtk::Button,
        stop_btn: gtk::Button,
    ) {
        let header = adw::HeaderBar::builder().show_back_button(false).build();
        let toolbar_view = adw::ToolbarView::new();
        toolbar_view.add_top_bar(&header);
        toolbar_view.set_content(Some(&content));

        // Black background + light foreground for low-light
        // meditation comfort. Applied to the ToolbarView (NOT the
        // inner content Box) so the dark fills the entire content
        // region edge-to-edge — the content Box has side margins
        // and centred alignment, which would otherwise leave bright
        // gaps around the timer. The HeaderBar paints its own
        // theme-supplied opaque background on top, so the header
        // stays the user's normal theme.
        toolbar_view.add_css_class("running-view-dark");

        let page = adw::NavigationPage::builder()
            .tag("running").title(title)
            .child(&toolbar_view)
            .build();

        // toggle_playback dispatches by state — Running/Preparing →
        // pause, Overtime → finish-overtime-session. The Pause button
        // label morphs to "Finish" when we enter Overtime, so the
        // user-visible action stays consistent with what gets called.
        let obj = self.obj().clone();
        pause_btn.connect_clicked(move |_| obj.imp().timer_view.toggle_playback());
        let obj2 = self.obj().clone();
        stop_btn.connect_clicked(move |_| obj2.imp().timer_view.stop());

        // Esc on the running page pauses (not stop, not dismiss — stop
        // would commit an unintended save, dismiss would leave the timer
        // running off-screen). Matches common AdwNavigationPage UX.
        let esc = gtk::EventControllerKey::new();
        let obj3 = self.obj().clone();
        esc.connect_key_pressed(move |_, keyval, _, _| {
            if keyval == gtk::gdk::Key::Escape {
                obj3.imp().timer_view.pause();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        page.add_controller(esc);

        self.nav_view.push(&page);
    }
}

// ── Sync status indicator ─────────────────────────────────────────────────────

impl MeditateWindow {
    fn wire_sync_status(&self) {
        let obj = self.obj();

        // Click semantics depend on the current state:
        // - Warning AND last error was remote-data-lost: open the
        //   recovery dialog so the user picks Push-Local / Cancel.
        //   This case is destructive enough that auto-retrying
        //   silently would surprise the user.
        // - Warning (any other error): retry the sync. On Phosh
        //   especially, transient network/DNS failures stick the
        //   icon at warning until the next mutation; a manual retry
        //   path closes that gap without requiring the user to
        //   author something.
        // - Anything else: open Preferences → Data so the user can
        //   adjust settings or check status.
        self.sync_status_btn.connect_clicked(glib::clone!(
            #[weak] obj,
            move |_| {
                let Some(app) = obj.application()
                    .and_then(|a| a.downcast::<crate::application::MeditateApplication>().ok())
                else { return; };
                use meditate_core::sync::indicator::{action_for, SyncIndicatorAction};
                let state = sync_indicator_state_now(&app);
                match action_for(&state) {
                    SyncIndicatorAction::OpenRecovery => crate::recovery_dialog::show(&app),
                    SyncIndicatorAction::RetrySync => app.trigger_sync(),
                    SyncIndicatorAction::OpenPrefsData => {
                        crate::preferences::show_preferences_on_page(&app, Some("data"));
                    }
                }
            }
        ));

        // Defer the initial paint to `connect_map`. At `constructed()`
        // time the window isn't yet linked to its application, so
        // `obj.application()` returns None and the indicator stays
        // blank until the polling timer below catches up. `connect_map`
        // fires once the window is realised AND the application
        // binding is in place — the very first frame shows the
        // correct state.
        obj.connect_map(|window| {
            use glib::subclass::prelude::ObjectSubclassIsExt;
            window.imp().refresh_sync_status();
        });

        // Poll every 2s for state changes. The timer self-cancels via
        // the weak-ref upgrade failing once the window is destroyed;
        // no manual SourceId tracking. Skip the refresh when the
        // window is unmapped (minimised, hidden, app backgrounded on
        // Phosh / Android) — the user can't see the indicator anyway
        // and the DB read costs eMMC IO + battery. `connect_map`
        // above refreshes once on re-show, so the user sees the
        // current state immediately when the window comes back.
        let weak = obj.downgrade();
        glib::timeout_add_seconds_local(2, move || {
            match weak.upgrade() {
                Some(w) => {
                    if !w.is_mapped() {
                        return glib::ControlFlow::Continue;
                    }
                    use glib::subclass::prelude::ObjectSubclassIsExt;
                    w.imp().refresh_sync_status();
                    glib::ControlFlow::Continue
                }
                None => glib::ControlFlow::Break,
            }
        });
    }

    /// Recompute and apply the headerbar sync icon's state. Cheap: a
    /// few in-memory SQLite reads + an atomic load. Safe to call on
    /// every timer tick (every 2s) without measurable load.
    pub fn refresh_sync_status(&self) {
        use crate::i18n::gettext;
        use meditate_core::sync::indicator::SyncIndicatorState;
        let Some(app) = self.obj().application()
            .and_then(|a| a.downcast::<crate::application::MeditateApplication>().ok())
        else { return; };

        let state = sync_indicator_state_now(&app);

        let btn     = &*self.sync_status_btn;
        let stack   = &*self.sync_status_stack;
        let icon    = &*self.sync_status_icon;
        let spinner = &*self.sync_status_spinner;

        // Icon-tint classes (`success`, `warning`) are exclusive — at
        // most one applies at a time. Reset between transitions.
        btn.remove_css_class("success");
        btn.remove_css_class("warning");

        match state {
            SyncIndicatorState::Hidden => {
                // No account: hide the indicator. Nothing useful for
                // the user to see or click on.
                btn.set_visible(false);
                spinner.set_spinning(false);
            }
            SyncIndicatorState::Syncing => {
                btn.set_visible(true);
                // Animated Spinner is the only visual that
                // distinguishes "actively syncing" from "idle".
                spinner.set_spinning(true);
                stack.set_visible_child_name("syncing");
                btn.set_tooltip_text(Some(&gettext("Syncing with Nextcloud…")));
            }
            SyncIndicatorState::Error { detail, .. } => {
                btn.set_visible(true);
                spinner.set_spinning(false);
                stack.set_visible_child_name("idle");
                icon.set_icon_name(Some("dialog-warning-symbolic"));
                btn.add_css_class("warning");
                btn.set_tooltip_text(Some(
                    &format!("{}\n{}",
                        gettext("Last sync failed — click to retry"),
                        detail)));
            }
            SyncIndicatorState::OkWithTs(ts) => {
                btn.set_visible(true);
                spinner.set_spinning(false);
                stack.set_visible_child_name("idle");
                icon.set_icon_name(Some("object-select-symbolic"));
                btn.add_css_class("success");
                btn.set_tooltip_text(Some(&format_synced_ago(ts)));
            }
            SyncIndicatorState::OkNoTs => {
                btn.set_visible(true);
                spinner.set_spinning(false);
                stack.set_visible_child_name("idle");
                icon.set_icon_name(Some("object-select-symbolic"));
                btn.set_tooltip_text(Some(&gettext("Sync configured (waiting for first run)")));
            }
        }
    }
}

/// Read the persisted sync state from the DB and derive the
/// `SyncIndicatorState` via the core helper. Returns
/// `SyncIndicatorState::Hidden` when the DB is unavailable so the
/// button is silent rather than stale.
fn sync_indicator_state_now(
    app: &crate::application::MeditateApplication,
) -> meditate_core::sync::indicator::SyncIndicatorState {
    use meditate_core::sync::indicator;
    use meditate_core::sync::settings;
    let snapshot = app.with_db(|db| {
        let core_db = db.core();
        (
            settings::nextcloud_account_from_db(core_db).unwrap_or(None),
            settings::get_last_sync_unix_ts(core_db).unwrap_or(None),
            settings::get_last_sync_error(core_db).unwrap_or(None),
            settings::is_last_sync_remote_data_lost(core_db).unwrap_or(false),
        )
    });
    let Some((account, last_ts, last_error, is_data_lost)) = snapshot else {
        return indicator::SyncIndicatorState::Hidden;
    };
    indicator::derive(
        account.is_some(),
        app.is_syncing(),
        last_ts,
        last_error,
        is_data_lost,
    )
}

/// Render a unix timestamp as a human-friendly "synced N ago" tooltip.
/// Granularity steps up the further back the timestamp lies — minutes
/// for the first hour, hours within a day, days beyond. Doesn't
/// localise the count words; gettext takes care of that via the
/// surrounding translatable templates. The bucket decision lives in
/// `meditate_core::format::synced_ago_key`.
fn format_synced_ago(unix_ts: i64) -> String {
    use crate::i18n::gettext;
    use meditate_core::format::SyncedAgoKey;
    let secs_ago = meditate_core::time::unix_now() - unix_ts;
    match meditate_core::format::synced_ago_key(secs_ago) {
        SyncedAgoKey::JustNow => gettext("Synced just now"),
        SyncedAgoKey::Minutes(n) => gettext("Synced {n} minutes ago")
            .replace("{n}", &n.to_string()),
        SyncedAgoKey::Hours(n) => gettext("Synced {n} hours ago")
            .replace("{n}", &n.to_string()),
        SyncedAgoKey::Days(n) => gettext("Synced {n} days ago")
            .replace("{n}", &n.to_string()),
    }
}

#[cfg(test)]
mod sync_status_tests {
    //! `format_synced_ago` is the only piece of E.5 worth a unit test —
    //! the GTK glue (template wiring, tooltip text setting, CSS class
    //! flips) is verified by running the app. The "ago" formatting is
    //! pure logic and easy to pin: pick a fixed `now` via the formula
    //! and step `unix_ts` through each bucket boundary.
    use super::format_synced_ago;

    fn ago(unix_ts_offset_secs: i64) -> String {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        format_synced_ago(now + unix_ts_offset_secs)
    }

    #[test]
    fn under_a_minute_says_just_now() {
        assert!(ago(-30).contains("just now"),
            "30 s ago should fall into the 'just now' bucket, got `{}`", ago(-30));
        assert!(ago(0).contains("just now"));
    }

    #[test]
    fn between_one_minute_and_an_hour_uses_minutes() {
        assert!(ago(-90).contains("minute"));
        assert!(ago(-3540).contains("minute"),
            "59 minutes still falls in the minutes bucket, got `{}`", ago(-3540));
    }

    #[test]
    fn between_one_hour_and_a_day_uses_hours() {
        assert!(ago(-3600).contains("hour"));
        assert!(ago(-86399).contains("hour"));
    }

    #[test]
    fn beyond_a_day_uses_days() {
        assert!(ago(-86400).contains("day"));
        assert!(ago(-86400 * 7).contains("day"));
    }

    #[test]
    fn future_timestamps_clamp_to_just_now_rather_than_negative() {
        // Defensive: clock skew between two devices can land a
        // timestamp slightly in the future. Avoid showing "synced -3
        // minutes ago" via the saturating max(0) clamp.
        let s = ago(60);  // 60 s in the future
        assert!(s.contains("just now"),
            "future timestamps should clamp to 'just now', got `{s}`");
    }
}

/// Read the accent colour from libadwaita's StyleManager (respects the
/// user's chosen system accent on GNOME 46+). Falls back to the default
/// Adwaita blue only if the lookup somehow misses.
fn accent_rgb(_widget: &impl IsA<gtk::Widget>) -> (f64, f64, f64) {
    let rgba = adw::StyleManager::default().accent_color_rgba();
    (rgba.red() as f64, rgba.green() as f64, rgba.blue() as f64)
}

/// Append a rounded-rectangle path to the current cairo context.
fn rounded_rect(cr: &gtk::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    use std::f64::consts::PI;
    cr.new_sub_path();
    cr.arc(x + w - r, y + r,     r, -PI / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0,        PI / 2.0);
    cr.arc(x + r,     y + h - r, r, PI / 2.0,   PI);
    cr.arc(x + r,     y + r,     r, PI,         3.0 * PI / 2.0);
    cr.close_path();
}

// ── Log ───────────────────────────────────────────────────────────────────────

impl MeditateWindow {
    fn wire_log_signals(&self) {
        let obj = self.obj();

        // Show/hide log header buttons based on active view
        self.view_stack.connect_notify_local(
            Some("visible-child"),
            glib::clone!(
                #[weak] obj,
                move |stack, _| {
                    let is_log = stack.visible_child_name().as_deref() == Some("log");
                    let imp = obj.imp();
                    imp.log_add_btn.set_visible(is_log);
                    imp.log_filter_btn.set_visible(is_log);
                    if is_log {
                        imp.log_view.refresh();
                        imp.log_view.refresh_filter_labels(&imp.filter_label_row);
                    }
                }
            ),
        );

        // + Add button
        self.log_add_btn.connect_clicked(glib::clone!(
            #[weak] obj,
            move |_| obj.imp().log_view.show_add_dialog()
        ));

        // Filter: apply instantly, but only while the popover is open.
        // Guards against set_model() firing notify::selected during
        // programmatic initialization, which would cause a BorrowMutError.
        self.filter_notes_row.connect_notify_local(
            Some("active"),
            glib::clone!(
                #[weak] obj,
                move |row, _| {
                    let imp = obj.imp();
                    if !imp.log_filter_btn.is_active() {
                        return;
                    }
                    imp.log_view.set_filter_notes_only(row.is_active());
                    imp.log_view.refresh();
                    if let Some(p) = imp.log_filter_btn.popover() { p.popdown(); }
                }
            ),
        );

        self.filter_label_row.connect_notify_local(
            Some("selected"),
            glib::clone!(
                #[weak] obj,
                move |row, _| {
                    let imp = obj.imp();
                    if !imp.log_filter_btn.is_active() {
                        return;
                    }
                    let selected = row.selected() as usize;
                    let label_id = if selected == 0 {
                        None
                    } else {
                        let labels = imp.log_view.imp().labels.borrow();
                        labels.get(selected - 1).map(|l| l.id)
                    };
                    imp.log_view.set_filter_label_id(label_id);
                    imp.log_view.refresh();
                    if let Some(p) = imp.log_filter_btn.popover() { p.popdown(); }
                }
            ),
        );
    }

    pub fn add_toast(&self, toast: adw::Toast) {
        self.toast_overlay.add_toast(toast);
    }
}

// ── Help overlay & window actions ────────────────────────────────────────────

impl MeditateWindow {
    fn setup_help_overlay(&self) {
        // GtkShortcutsWindow + AdwApplicationWindow::set_help_overlay are
        // both deprecated since GTK 4.18 in favour of AdwShortcutsDialog
        // (libadwaita 1.8). Debian trixie only ships libadwaita 1.7 so we
        // can't switch yet; re-evaluate once pkg-config reports 1.8+.
        #[allow(deprecated)]
        {
            let builder = gtk::Builder::from_resource(
                "/io/github/janekbt/Meditate/ui/shortcuts.ui",
            );
            if let Some(overlay) = builder.object::<gtk::ShortcutsWindow>("help_overlay") {
                self.obj().set_help_overlay(Some(&overlay));
            }
        }
    }

    fn setup_window_actions(&self) {
        let obj = self.obj();
        let action = gtk::gio::SimpleAction::new("timer-toggle", None);
        action.connect_activate(glib::clone!(
            #[weak] obj,
            move |_, _| {
                obj.imp().timer_view.toggle_playback();
            }
        ));
        obj.add_action(&action);

        // HIG-standard `win.close` shortcut (Ctrl+W). Different from
        // `app.quit` (Ctrl+Q) which exits the whole process — a
        // distinction AccelMap previously collapsed to a single no-op.
        let close_action = gtk::gio::SimpleAction::new("close", None);
        close_action.connect_activate(glib::clone!(
            #[weak] obj,
            move |_, _| obj.close()
        ));
        obj.add_action(&close_action);
    }

    /// Bind the GSettings schema so the window size + maximised state
    /// persist across launches. Skips silently if the schema isn't
    /// installed (e.g. running a dev binary without
    /// `GSETTINGS_SCHEMA_DIR=build/data` set), so the app still boots.
    fn bind_settings(&self) {
        let Some(src) = gio::SettingsSchemaSource::default() else { return; };
        if src.lookup(crate::config::APP_ID, true).is_none() {
            let msg = format!(
                "GSettings schema '{}' not found — window size won't persist. \
                 Set GSETTINGS_SCHEMA_DIR=build/data for dev builds.",
                crate::config::APP_ID,
            );
            eprintln!("note: {msg}");
            meditate_core::diag::log(&msg);
            return;
        }
        let settings = gio::Settings::new(crate::config::APP_ID);
        let obj = self.obj();
        settings.bind("window-width",     &*obj, "default-width").build();
        settings.bind("window-height",    &*obj, "default-height").build();
        settings.bind("window-maximized", &*obj, "maximized").build();
    }
}

// ── Stats ─────────────────────────────────────────────────────────────────────

impl MeditateWindow {
    fn wire_stats_signals(&self) {
        let obj = self.obj();
        self.view_stack.connect_notify_local(
            Some("visible-child"),
            glib::clone!(
                #[weak] obj,
                move |stack, _| {
                    if stack.visible_child_name().as_deref() == Some("stats") {
                        obj.imp().stats_view.refresh();
                    }
                }
            ),
        );
    }
}
