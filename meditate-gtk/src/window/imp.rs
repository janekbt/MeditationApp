use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{glib, CompositeTemplate};
use glib::clone;
use std::time::Duration;

use gtk::gio;

use meditate_core::format::format_time;

use crate::i18n::gettext;
use crate::log::LogView;
use crate::stats::StatsView;
use crate::timer::TimerView;
use meditate_core::breath::PhaseRunningLabelKey;
use meditate_core::sync::indicator::{action_for, SyncIndicatorAction};
use glib::subclass::prelude::ObjectSubclassIsExt;

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
                            crate::sound::preload_end_bell(
                                &app,
                                w.imp().timer_view.imp().current_mode().into(),
                            );
                        }
                    }
                });
            });
        });
    }
}

impl WidgetImpl for MeditateWindow {}
impl WindowImpl for MeditateWindow {
    fn close_request(&self) -> glib::Propagation {
        self.log_view.commit_all_pending();
        self.timer_view.save_snapshot_now();
        self.parent_close_request()
    }
}
impl ApplicationWindowImpl for MeditateWindow {}
impl AdwApplicationWindowImpl for MeditateWindow {}

// ── Timer ─────────────────────────────────────────────────────────────────────

impl MeditateWindow {
    fn wire_timer_signals(&self) {
        let obj = self.obj();

        self.view_stack.connect_notify_local(
            Some("visible-child"),
            clone!(
                #[weak] obj,
                move |stack, _| {
                    if stack.visible_child_name().as_deref() == Some("timer") {
                        obj.imp().timer_view.refresh_streak();
                    }
                }
            ),
        );

        self.timer_view.connect_timer_started(clone!(
            #[weak] obj,
            move |_| obj.imp().push_running_page()
        ));
        // No timer-paused handler: the running page stays put on
        // pause now. The Pause button morphs to "Resume" in place
        // (see TimerView::on_pause) instead of popping the user
        // back to the dimmed setup view.
        self.timer_view.connect_timer_stopped(clone!(
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

        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(32)
            .margin_start(12).margin_end(12)
            .build();
        content.append(&self.bell_ring(&time_label));
        let (controls, note) = self.end_controls();
        content.append(&controls);

        self.push_running_page_with_content(&gettext("Meditating"), content, note);
    }

    /// The running time inside the bell ring (Timer countdown, Guided),
    /// with the repeating bells' counters under it. Both follow the
    /// session every quarter second, so a counter moves with its bell.
    fn bell_ring(&self, time_label: &gtk::Label) -> gtk::Box {
        use std::{cell::RefCell, rc::Rc};
        let block = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(16).build();
        let counters = gtk::Box::builder().spacing(20).halign(gtk::Align::Center).visible(false).build();
        let now = self.timer_view.bell_ring();
        if now.ring.is_none() && now.counters.is_empty() {
            block.append(time_label);
            return block;
        }
        let ring = Rc::new(RefCell::new(now.ring.clone()));
        let area = gtk::DrawingArea::builder().content_width(300).content_height(300).halign(gtk::Align::Center).build();
        if now.ring.is_some() {
            area.set_draw_func(clone!(
                #[strong] ring,
                move |area, cr, w, h| {
                    if let Some(ring) = ring.borrow().as_ref() {
                        draw_bell_ring(area, cr, w, h, ring);
                    }
                }
            ));
            let overlay = gtk::Overlay::builder().child(&area).halign(gtk::Align::Center).build();
            time_label.set_valign(gtk::Align::Center);
            // Hours don't fit inside the ring at the full size.
            let fit = |label: &gtk::Label| {
                if label.label().len() > 5 {
                    label.add_css_class("in-ring-long");
                } else {
                    label.remove_css_class("in-ring-long");
                }
            };
            fit(time_label);
            time_label.connect_label_notify(fit);
            overlay.add_overlay(time_label);
            block.append(&overlay);
        } else {
            block.append(time_label);
        }
        block.append(&counters);
        let shown = RefCell::new(None::<Vec<meditate_core::session::BellCounter>>);
        let refresh = clone!(
            #[weak(rename_to = timer_view)] self.timer_view,
            // Held here: in the stopwatch it is on no page.
            #[strong] area,
            #[weak] counters,
            #[upgrade_or] glib::ControlFlow::Break,
            move || {
                let now = timer_view.bell_ring();
                if *ring.borrow() != now.ring {
                    *ring.borrow_mut() = now.ring;
                    area.queue_draw();
                }
                if shown.borrow().as_ref() != Some(&now.counters) {
                    while let Some(child) = counters.first_child() {
                        counters.remove(&child);
                    }
                    for c in &now.counters {
                        let text = match c.total {
                            Some(total) => gettext("{rung} of {total}")
                                .replace("{rung}", &c.rung.to_string())
                                .replace("{total}", &total.to_string()),
                            None => c.rung.to_string(),
                        };
                        let counter = gtk::Box::builder().spacing(6).build();
                        counter.append(&gtk::Label::builder()
                            .label("●")
                            .css_classes(["accent", "caption"])
                            .accessible_role(gtk::AccessibleRole::Presentation)
                            .build());
                        counter.append(&gtk::Label::builder().label(text).css_classes(["numeric", "dim-label"]).build());
                        counters.append(&counter);
                    }
                    counters.set_visible(!now.counters.is_empty());
                    *shown.borrow_mut() = Some(now.counters);
                }
                glib::ControlFlow::Continue
            }
        );
        refresh();
        glib::timeout_add_local(Duration::from_millis(250), refresh);
        block
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
            .label(gettext("Box Breathing"))
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
                let size = f64::from(w.min(h));
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
                let (x, y) = phase.perimeter_point(t, pad, side);

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
            .margin_start(12).margin_end(12)
            .build();
        content.append(&top_strip);
        content.append(&overlay);
        let (controls, note) = self.end_controls();
        content.append(&controls);

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
        // The frame callback only draws (phase label, dot, counter).
        // The session itself ticks on the TimerView's timer, which
        // keeps running while the window isn't drawn.
        drawing_area.add_tick_callback(move |_, _clock| {
            let tv = obj.imp().timer_view.clone();
            if !matches!(tv.imp().ui_state(), meditate_core::session::UiState::Running | meditate_core::session::UiState::Paused) {
                return glib::ControlFlow::Break;
            }

            // Visual render. breath_elapsed() is wall-clock anchored
            // and freezes on pause, so it's the right input for both
            // the dot's perimeter position and the counter.
            let elapsed = tv.breath_elapsed();
            let cur = elapsed.as_secs_f64();
            let info = pattern_for_tick.phase_at(elapsed);
            let phase = info.phase;

            let phase_name = match phase.running_label_key() {
                PhaseRunningLabelKey::BreatheIn => gettext("Breathe in"),
                PhaseRunningLabelKey::Hold => gettext("Hold"),
                PhaseRunningLabelKey::BreatheOut => gettext("Breathe out"),
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

            glib::ControlFlow::Continue
        });

        self.push_running_page_with_content(&gettext("Box Breathing"), content, note);
    }

    /// The running page's end of a session, for both running pages: a
    /// row of one or two buttons with Discard under it, and the note
    /// the page places below them. TimerView sets what each shows from
    /// core's button set.
    fn end_controls(&self) -> (gtk::Box, gtk::Widget) {
        let button = || gtk::Button::builder().css_classes(["pill"]).build();
        let (first, second, discard) = (button(), button(), button());
        discard.set_label(&gettext("Discard"));
        discard.add_css_class("destructive-action");
        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .halign(gtk::Align::Center)
            .build();
        row.append(&first);
        row.append(&second);
        discard.set_halign(gtk::Align::Center);

        let caption = gtk::Label::builder()
            .label(gettext("Note (optional)"))
            .css_classes(["caption", "dimmed"])
            .halign(gtk::Align::Start)
            .margin_start(12)
            .build();
        let note_view = gtk::TextView::builder()
            .wrap_mode(gtk::WrapMode::WordChar)
            .accepts_tab(false)
            .top_margin(8).bottom_margin(8).left_margin(12).right_margin(12)
            .build();
        note_view.update_relation(&[gtk::accessible::Relation::LabelledBy(&[caption.upcast_ref()])]);
        let frame = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .height_request(180)
            .css_classes(["log-note-editor"])
            .child(&note_view)
            .build();
        let note = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(6).build();
        note.append(&caption);
        note.append(&frame);
        let note_clamp = adw::Clamp::builder()
            .maximum_size(480)
            .margin_start(12).margin_end(12).margin_top(18)
            .child(&note)
            .build();

        let controls = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(18).build();
        controls.append(&row);
        controls.append(&discard);

        for (i, btn) in [&first, &second].into_iter().enumerate() {
            let obj = self.obj().clone();
            btn.connect_clicked(move |_| obj.imp().timer_view.end_button_clicked(i));
        }
        let obj = self.obj().clone();
        discard.connect_clicked(move |_| obj.imp().timer_view.discard());
        self.timer_view.set_running_controls(crate::timer::RunningControls {
            first, second, note: note_clamp.clone().upcast(), note_view,
        });
        (controls, note_clamp.upcast())
    }

    /// Shared scaffolding for both running pages: wraps the content in an
    /// AdwNavigationPage with a header, wires Pause/Stop/Esc, pushes onto
    /// the nav view.
    fn push_running_page_with_content(&self, title: &str, content: gtk::Box, note: gtk::Widget) {
        let header = adw::HeaderBar::builder().show_back_button(false).build();
        let toolbar_view = adw::ToolbarView::new();
        toolbar_view.add_top_bar(&header);
        // As on the phone: the time and the buttons stay where they are
        // whether the note shows or not. A spacer holds them centred in the
        // visible page; the note hangs below and the page scrolls when the
        // window has no room for it.
        let spacer = gtk::Box::builder().build();
        let column = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .margin_top(24).margin_bottom(24)
            .build();
        column.append(&spacer);
        column.append(&content);
        column.append(&note);
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&column)
            .build();
        scroll.vadjustment().connect_page_size_notify(clone!(
            #[weak] content,
            #[weak] spacer,
            #[weak] column,
            move |adj| {
                // ponytail: follows the window height only; a label that
                // wraps later grows the block downward, off centre by half a line.
                let (_, height, _, _) = content.measure(gtk::Orientation::Vertical, -1);
                let room = adj.page_size() as i32 - column.margin_top() - column.margin_bottom() - height;
                spacer.set_height_request((room / 2).max(0));
            }
        ));
        toolbar_view.set_content(Some(&scroll));

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
            // Back gestures and Alt+Left led to Setup, where Start
            // replaced the running session; code can still pop it.
            .can_pop(false)
            .child(&toolbar_view)
            .build();

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
        self.sync_status_btn.connect_clicked(clone!(
            #[weak] obj,
            move |_| {
                let Some(app) = obj.application()
                    .and_then(|a| a.downcast::<crate::application::MeditateApplication>().ok())
                else { return; };
                let state = sync_indicator_state_now(&app);
                match action_for(&state) {
                    SyncIndicatorAction::OpenRecovery => crate::recovery_dialog::show(&app),
                    SyncIndicatorAction::RetrySync => {
                        app.with_db(|db| meditate_core::sync::settings::clear_refused_uploads(db.core()));
                        app.trigger_sync();
                    }
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
        let mut day = meditate_core::time::DayWatch::new(meditate_core::time::today_local());
        glib::timeout_add_seconds_local(2, move || {
            match weak.upgrade() {
                Some(w) => {
                    if day.new_day(meditate_core::time::today_local()) {
                        w.imp().reload_for_new_day();
                    }
                    if !w.is_mapped() {
                        return glib::ControlFlow::Continue;
                    }
                    w.imp().refresh_sync_status();
                    glib::ControlFlow::Continue
                }
                None => glib::ControlFlow::Break,
            }
        });
    }

    /// Log headers and Stats' "today" were built for the old day.
    fn reload_for_new_day(&self) {
        let Some(app) = self.obj().application()
            .and_then(|a| a.downcast::<crate::application::MeditateApplication>().ok())
        else { return; };
        app.invalidate(crate::application::InvalidateScope::ALL);
        match self.view_stack.visible_child_name().as_deref() {
            Some("log") => self.log_view.refresh(),
            Some("stats") => self.stats_view.refresh(),
            _ => {}
        }
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
                        gettext("Last sync failed, click to retry"),
                        detail)));
            }
            SyncIndicatorState::NotUploaded(files) => {
                btn.set_visible(true);
                spinner.set_spinning(false);
                stack.set_visible_child_name("idle");
                icon.set_icon_name(Some("dialog-warning-symbolic"));
                btn.add_css_class("warning");
                let lines: Vec<String> = files.iter().map(not_uploaded_line).collect();
                btn.set_tooltip_text(Some(&format!(
                    "{}\n{}",
                    gettext("Some files weren't uploaded, click to retry"),
                    lines.join("\n"),
                )));
            }
            SyncIndicatorState::NeedsPassword => {
                btn.set_visible(true);
                spinner.set_spinning(false);
                stack.set_visible_child_name("idle");
                icon.set_icon_name(Some("dialog-warning-symbolic"));
                btn.add_css_class("warning");
                btn.set_tooltip_text(Some(&gettext("No password saved, click to enter it")));
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
    let is_syncing = app.is_syncing();
    app.with_db(|db| indicator::state_from_db(db.core(), is_syncing))
        .unwrap_or(indicator::SyncIndicatorState::Hidden)
}

/// Render a unix timestamp as a human-friendly "synced N ago" tooltip.
/// Granularity steps up the further back the timestamp lies — minutes
/// for the first hour, hours within a day, days beyond. Doesn't
/// localise the count words; gettext takes care of that via the
/// surrounding translatable templates. The bucket decision lives in
/// `meditate_core::format::synced_ago_key`.
/// One refused upload, as the status lists it.
fn not_uploaded_line(file: &meditate_core::sync::indicator::NotUploaded) -> String {
    use meditate_core::sync::settings::UploadRefusal;
    match file.reason {
        UploadRefusal::TooLargeToSync => gettext("{name}: too large to sync"),
        UploadRefusal::TooLargeForServer => gettext("{name}: too large for the server"),
        UploadRefusal::ServerFull => gettext("{name}: the server is full"),
        UploadRefusal::Refused(status) => {
            gettext("{name}: refused by the server ({status})").replace("{status}", &status.to_string())
        }
    }
    .replace("{name}", &file.name)
}

fn format_synced_ago(unix_ts: i64) -> String {
    use crate::i18n::{gettext, ngettext};
    use meditate_core::format::SyncedAgoKey;
    let secs_ago = meditate_core::time::unix_now() - unix_ts;
    match meditate_core::format::synced_ago_key(secs_ago) {
        SyncedAgoKey::JustNow => gettext("Synced just now"),
        SyncedAgoKey::Minutes(n) =>
            ngettext("Synced 1 minute ago", "Synced {n} minutes ago", n as u32)
                .replace("{n}", &n.to_string()),
        SyncedAgoKey::Hours(n) =>
            ngettext("Synced 1 hour ago", "Synced {n} hours ago", n as u32)
                .replace("{n}", &n.to_string()),
        SyncedAgoKey::Days(n) =>
            ngettext("Synced 1 day ago", "Synced {n} days ago", n as u32)
                .replace("{n}", &n.to_string()),
    }
}

/// Read the accent colour from libadwaita's StyleManager (respects the
/// user's chosen system accent on GNOME 46+). Falls back to the default
/// Adwaita blue only if the lookup somehow misses.
fn accent_rgb(_widget: &impl IsA<gtk::Widget>) -> (f64, f64, f64) {
    let rgba = adw::StyleManager::default().accent_color_rgba();
    (f64::from(rgba.red()), f64::from(rgba.green()), f64::from(rgba.blue()))
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
            clone!(
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
        self.log_add_btn.connect_clicked(clone!(
            #[weak] obj,
            move |_| obj.imp().log_view.show_add_dialog()
        ));

        // Filter: apply instantly, but only while the popover is open.
        // Guards against set_model() firing notify::selected during
        // programmatic initialization, which would cause a BorrowMutError.
        self.filter_notes_row.connect_notify_local(
            Some("active"),
            clone!(
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

        // Rebuilt on each open, so renamed labels show their new names.
        if let Some(popover) = self.log_filter_btn.popover() {
            popover.connect_show(clone!(
                #[weak] obj,
                move |_| {
                    let imp = obj.imp();
                    imp.log_view.refresh_filter_labels(&imp.filter_label_row);
                }
            ));
        }

        self.filter_label_row.connect_notify_local(
            Some("selected"),
            clone!(
                #[weak] obj,
                move |row, _| {
                    let imp = obj.imp();
                    if !imp.log_filter_btn.is_active() {
                        return;
                    }
                    let log = imp.log_view.imp();
                    if log.building_filter.get() {
                        return;
                    }
                    let selected = row.selected() as usize;
                    let label_id = if selected == 0 {
                        None
                    } else {
                        log.filter_label_ids.borrow().get(selected - 1).copied()
                    };
                    if label_id == log.filter_label_id.get() {
                        return;
                    }
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

        // Space toggles the timer, but not while typing or in a dialog.
        // Capture phase so a focused button doesn't take it first; an
        // app accelerator would also beat the text field to the key.
        let space = gtk::ShortcutController::new();
        space.set_propagation_phase(gtk::PropagationPhase::Capture);
        space.add_shortcut(gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string("space"),
            Some(gtk::CallbackAction::new(clone!(
                #[weak] obj,
                #[upgrade_or] glib::Propagation::Proceed,
                move |_, _| {
                    let typing = gtk::prelude::GtkWindowExt::focus(&obj)
                        .is_some_and(|w| w.is::<gtk::Text>() || w.is::<gtk::TextView>());
                    if typing || obj.visible_dialog().is_some() {
                        return glib::Propagation::Proceed;
                    }
                    obj.imp().timer_view.toggle_playback();
                    glib::Propagation::Stop
                }
            ))),
        ));
        obj.add_controller(space);

        // HIG-standard `win.close` shortcut (Ctrl+W). Different from
        // `app.quit` (Ctrl+Q) which exits the whole process — a
        // distinction AccelMap previously collapsed to a single no-op.
        let close_action = gtk::gio::SimpleAction::new("close", None);
        close_action.connect_activate(clone!(
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
            meditate_core::log("gsettings", &msg);
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
            clone!(
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
            .map_or(0, |d| d.as_secs() as i64);
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

#[cfg(test)]
mod day_tests {
    fn production() -> String {
        let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/window/imp.rs")).unwrap();
        src[..src.find("#[cfg(test)]").unwrap()].to_string()
    }

    /// Left open overnight, Stats and the Log kept yesterday as today.
    #[test]
    fn the_window_poll_reloads_both_pages_on_a_new_day() {
        let src = production();
        let poll = &src[src.find("glib::timeout_add_seconds_local(2,").expect("the 2 s poll")..];
        let poll = &poll[..poll.find("\n        });").unwrap()];
        let ask = poll.find("day.new_day(meditate_core::time::today_local())").expect("the poll asks the day watch");
        assert!(ask < poll.find("if !w.is_mapped()").unwrap(), "asked even while hidden, so wake-up catches up");
        assert!(poll[ask..].contains("reload_for_new_day()"));
        let reload = &src[src.find("fn reload_for_new_day(&self)").expect("reload")..];
        let reload = &reload[..reload.find("\n    }\n").unwrap()];
        assert!(reload.contains("InvalidateScope::ALL"), "both pages go stale");
        assert!(reload.contains("Some(\"log\") => self.log_view.refresh()"));
        assert!(reload.contains("Some(\"stats\") => self.stats_view.refresh()"));
    }

    /// Pausing used to move Discard and lift the whole column.
    #[test]
    fn the_session_buttons_keep_their_place() {
        let src = production();
        let page = &src[src.find("fn push_running_page_with_content(").expect("the shared page")..];
        let page = &page[..page.find("\n    }\n").unwrap()];
        // As on the phone: a spacer as tall as half the room the time and
        // buttons leave in the visible page keeps them centred; the note
        // hangs below and the page scrolls when there is no room for it.
        let order = ["column.append(&spacer)", "column.append(&content)", "column.append(&note)"].map(|a| page.find(a).unwrap_or_else(|| panic!("{a}")));
        assert!(order[0] < order[1] && order[1] < order[2], "spacer, time and buttons, note");
        let follow = &page[page.find("scroll.vadjustment().connect_page_size_notify(").expect("follows the visible height")..];
        assert!(follow.contains("adj.page_size()") && follow.contains("content.measure(gtk::Orientation::Vertical, -1)"));
        assert!(follow.contains("spacer.set_height_request("), "from the visible page, never from the note");
        assert!(!follow[..follow.find("));").unwrap()].contains("note"));
        for push in ["fn push_time_running_page(&self)", "fn push_breathing_running_page(&self)"] {
            let body = &src[src.find(push).unwrap()..];
            assert!(body[..body.find("\n    }\n").unwrap()].contains("let (controls, note) = self.end_controls();"), "{push}");
        }
        let timer = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/timer/imp.rs")).unwrap();
        let refresh = &timer[timer.find("fn refresh_end_buttons(&self)").unwrap()..];
        let refresh = &refresh[..refresh.find("\n    }\n").unwrap()];
        assert!(!refresh.contains("discard"), "Discard is always there");
        assert!(refresh.contains("c.second.set_visible(buttons.second.is_some())"));
    }
}

/// The bell ring round the running time: a faint track, the filled
/// part in the accent colour from the top clockwise, the start point,
/// the preparation's small ring round it, and a bead per bell.
fn draw_bell_ring(area: &gtk::DrawingArea, cr: &gtk::cairo::Context, w: i32, h: i32, ring: &meditate_core::session::Ring) {
    use std::f64::consts::PI;
    let (ar, ag, ab) = accent_rgb(area);
    let (cx, cy) = (f64::from(w) / 2.0, f64::from(h) / 2.0);
    // Room outside the track for the preparation's ring.
    let r = f64::from(w.min(h)) / 2.0 - 22.0;
    let angle = |at: f64| -PI / 2.0 + 2.0 * PI * at;
    cr.set_line_width(4.0);
    cr.set_line_cap(gtk::cairo::LineCap::Round);
    cr.set_source_rgba(ar, ag, ab, 0.25);
    cr.arc(cx, cy, r, 0.0, 2.0 * PI);
    let _ = cr.stroke();
    if ring.filled > 0.0 {
        cr.set_source_rgb(ar, ag, ab);
        cr.arc(cx, cy, r, angle(0.0), angle(ring.filled));
        let _ = cr.stroke();
    }
    let bead = |at: f64, radius: f64, filled: bool| {
        let (x, y) = (cx + r * angle(at).cos(), cy + r * angle(at).sin());
        cr.new_sub_path();
        cr.arc(x, y, radius, 0.0, 2.0 * PI);
        if filled {
            cr.set_source_rgb(ar, ag, ab);
            let _ = cr.fill();
        } else {
            // The running page is black.
            cr.set_source_rgb(0.0, 0.0, 0.0);
            let _ = cr.fill_preserve();
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.6);
            cr.set_line_width(2.0);
            let _ = cr.stroke();
        }
    };
    for b in &ring.beads {
        bead(b.at, if b.large { 7.0 } else { 5.0 }, b.rung);
    }
    bead(0.0, 9.0, ring.start_filled);
    if let Some(prep) = ring.prep {
        cr.set_source_rgb(ar, ag, ab);
        cr.set_line_width(2.0);
        cr.new_sub_path();
        cr.arc(cx, cy - r, 18.0, angle(0.0), angle(prep));
        let _ = cr.stroke();
    }
}
