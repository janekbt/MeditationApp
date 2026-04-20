use std::cell::{Cell, RefCell};
use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{glib, CompositeTemplate};
use glib::subclass::Signal;
use std::sync::OnceLock;

use crate::db::{Label, SessionData, SessionMode};

// ── Per-mode independent state ────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimerState {
    #[default]
    Idle,
    Running,
    Paused,
    Done,
}

/// All state that belongs to one timer mode (countdown or stopwatch).
#[derive(Debug, Clone, Default)]
struct ModeState {
    timer_state: TimerState,
    /// Seconds remaining (countdown) or elapsed (stopwatch).
    display_secs: u64,
    /// Original target in seconds — countdown only.
    target_secs: u64,
    /// Unix timestamp when this mode's current session started.
    session_start_time: i64,
}

// ── GObject impl ──────────────────────────────────────────────────────────────

#[derive(Debug, Default, CompositeTemplate)]
#[template(resource = "/io/github/janekbt/Meditate/ui/timer_view.ui")]
pub struct TimerView {
    // Template children
    #[template_child] pub view_stack:            TemplateChild<gtk::Stack>,
    #[template_child] pub streak_label:          TemplateChild<gtk::Label>,
    #[template_child] pub countdown_btn:         TemplateChild<gtk::ToggleButton>,
    #[template_child] pub stopwatch_btn:         TemplateChild<gtk::ToggleButton>,
    #[template_child] pub inputs_stack:          TemplateChild<gtk::Stack>,
    #[template_child] pub big_time_label:         TemplateChild<gtk::Label>,
    #[template_child] pub countdown_inputs:       TemplateChild<gtk::Box>,
    #[template_child] pub presets_box:           TemplateChild<gtk::FlowBox>,
    #[template_child] pub stopwatch_idle_label:  TemplateChild<gtk::Label>,
    #[template_child] pub paused_time_label:     TemplateChild<gtk::Label>,
    #[template_child] pub start_btn:             TemplateChild<gtk::Button>,
    #[template_child] pub resume_btn:            TemplateChild<gtk::Button>,
    #[template_child] pub stop_from_pause_btn:   TemplateChild<gtk::Button>,
    #[template_child] pub setup_label_row:        TemplateChild<adw::ComboRow>,
    #[template_child] pub setup_sound_row:        TemplateChild<adw::ComboRow>,
    #[template_child] pub time_unit_label:        TemplateChild<gtk::Label>,
    #[template_child] pub done_duration_label:   TemplateChild<gtk::Label>,
    #[template_child] pub note_row:              TemplateChild<adw::EntryRow>,
    #[template_child] pub label_row:             TemplateChild<adw::ComboRow>,
    #[template_child] pub discard_btn:           TemplateChild<gtk::Button>,
    #[template_child] pub save_btn:              TemplateChild<gtk::Button>,

    // ── Per-mode state (fully independent) ───────────────────────────
    countdown_mode: RefCell<ModeState>,
    stopwatch_mode: RefCell<ModeState>,

    /// Whether the active tick belongs to the stopwatch mode.
    /// Only meaningful while tick_source is Some.
    tick_is_stopwatch: Cell<bool>,

    /// Active glib timeout handle (at most one mode runs at a time).
    tick_source: RefCell<Option<glib::SourceId>>,
    /// Weak ref to the running-page time label for live updates.
    running_label: RefCell<Option<gtk::Label>>,
    /// Labels fetched from DB for the setup-page combo.
    setup_db_labels: RefCell<Vec<Label>>,
    /// True while refresh_setup_labels is rebuilding the setup combo model.
    setup_populating: Cell<bool>,
    /// Labels fetched from DB when entering Done state.
    db_labels: RefCell<Vec<Label>>,
    /// True while show_done/repopulate_label_combo is rebuilding the model,
    /// to suppress the notify::selected handler from opening the new-label dialog.
    populating_labels: Cell<bool>,
    /// True while refresh_streak is populating the setup sound combo.
    sound_populating: Cell<bool>,
    /// Currently-selected countdown duration in seconds, set by preset
    /// chips or the "Custom" dialog. Default 10 min; used as the target
    /// when the user taps Start.
    countdown_target_secs: Cell<u64>,
}

#[glib::object_subclass]
impl ObjectSubclass for TimerView {
    const NAME: &'static str = "TimerView";
    type Type = super::TimerView;
    type ParentType = gtk::Widget;

    fn class_init(klass: &mut Self::Class) {
        klass.bind_template();
        klass.set_layout_manager_type::<gtk::BinLayout>();
    }

    fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
        obj.init_template();
    }
}

impl ObjectImpl for TimerView {
    fn signals() -> &'static [Signal] {
        static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
        SIGNALS.get_or_init(|| {
            vec![
                Signal::builder("timer-started").build(),
                Signal::builder("timer-paused").build(),
                Signal::builder("timer-stopped").build(),
            ]
        })
    }

    fn constructed(&self) {
        self.parent_constructed();
        // Default countdown target: 10 min — matches the hero label that's
        // set to "00:10" in the blueprint.
        self.countdown_target_secs.set(10 * 60);
        self.setup_buttons();
    }

    fn dispose(&self) {
        self.cancel_tick();
        self.obj().first_child().map(|w| w.unparent());
    }
}

impl WidgetImpl for TimerView {}

// ── Button wiring ─────────────────────────────────────────────────────────────

impl TimerView {
    fn setup_buttons(&self) {
        let obj = self.obj();

        // Mode toggle — update UI to reflect the destination mode's state
        self.stopwatch_btn.connect_toggled(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |btn| this.imp().on_mode_switched(btn.is_active())
        ));

        self.start_btn.connect_clicked(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |_| this.imp().on_start()
        ));
        self.resume_btn.connect_clicked(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |_| this.imp().on_resume()
        ));
        self.stop_from_pause_btn.connect_clicked(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |_| this.imp().on_stop()
        ));
        self.save_btn.connect_clicked(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |_| this.imp().on_save()
        ));
        self.discard_btn.connect_clicked(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |_| this.imp().on_discard()
        ));

        // "＋ New label" is index 0; show creation dialog when selected.
        self.label_row.connect_notify_local(
            Some("selected"),
            glib::clone!(
                #[weak(rename_to = this)] obj,
                move |_, _| {
                    let imp = this.imp();
                    if imp.populating_labels.get() { return; }
                    if imp.label_row.selected() == 0 {
                        imp.show_new_label_dialog();
                    }
                }
            ),
        );

        // Completion Sound row on the setup page — mirrors the Preferences sound setting.
        self.setup_sound_row.connect_notify_local(
            Some("selected"),
            glib::clone!(
                #[weak(rename_to = this)] obj,
                move |row, _| {
                    let imp = this.imp();
                    if imp.sound_populating.get() { return; }
                    let key = match row.selected() {
                        1 => "bowl",
                        2 => "bell",
                        3 => "gong",
                        4 => "custom",
                        _ => "none",
                    };
                    if let Some(app) = imp.get_app() {
                        app.with_db(|db| db.set_setting("end_sound", key));
                        crate::sound::preload_end_sound(&app);
                    }
                }
            ),
        );

        // Same for the pre-start label selector.
        self.setup_label_row.connect_notify_local(
            Some("selected"),
            glib::clone!(
                #[weak(rename_to = this)] obj,
                move |_, _| {
                    let imp = this.imp();
                    if imp.setup_populating.get() { return; }
                    if imp.setup_label_row.selected() == 0 {
                        imp.show_new_label_dialog_for_setup();
                    }
                }
            ),
        );
    }
}

// ── Mode switching ────────────────────────────────────────────────────────────

impl TimerView {
    /// Called whenever the mode toggle fires. `to_stopwatch` is true when the
    /// user switched TO stopwatch (false = switched to countdown).
    fn on_mode_switched(&self, to_stopwatch: bool) {
        // Show / hide the countdown-specific input widgets
        self.big_time_label.set_visible(!to_stopwatch);
        self.time_unit_label.set_visible(!to_stopwatch);
        self.countdown_inputs.set_visible(!to_stopwatch);
        self.stopwatch_idle_label.set_visible(to_stopwatch);

        let (timer_state, display_secs) = {
            let mode = if to_stopwatch {
                self.stopwatch_mode.borrow()
            } else {
                self.countdown_mode.borrow()
            };
            (mode.timer_state, mode.display_secs)
        };

        match timer_state {
            TimerState::Idle => self.show_idle_ui(),
            TimerState::Paused => {
                self.paused_time_label.set_label(&format_time(display_secs));
                self.show_paused_ui();
            }
            TimerState::Done => {
                // Done panel is already populated; just make sure it's showing.
                self.view_stack.set_visible_child_name("done");
            }
            TimerState::Running => {
                // Can't normally reach here (running nav page blocks the toggle).
                self.show_idle_ui();
            }
        }
    }

    fn show_idle_ui(&self) {
        self.inputs_stack.set_visible_child_name("inputs");
        self.start_btn.set_visible(true);
        self.resume_btn.set_visible(false);
        self.stop_from_pause_btn.set_visible(false);
        self.view_stack.set_visible_child_name("setup");
    }

    fn show_paused_ui(&self) {
        self.inputs_stack.set_visible_child_name("paused");
        self.start_btn.set_visible(false);
        self.resume_btn.set_visible(true);
        self.stop_from_pause_btn.set_visible(true);
        self.view_stack.set_visible_child_name("setup");
    }
}

// ── Timer state machine ───────────────────────────────────────────────────────

impl TimerView {
    fn on_start(&self) {
        let is_stopwatch = self.stopwatch_btn.is_active();

        if is_stopwatch {
            let mut m = self.stopwatch_mode.borrow_mut();
            m.timer_state = TimerState::Running;
            m.display_secs = 0;
            m.session_start_time = unix_now();
        } else {
            let target = self.countdown_target_secs.get();
            if target == 0 {
                return;
            }
            let mut m = self.countdown_mode.borrow_mut();
            m.timer_state = TimerState::Running;
            m.target_secs = target;
            m.display_secs = target;
            m.session_start_time = unix_now();
        }

        self.tick_is_stopwatch.set(is_stopwatch);
        self.start_tick();
        self.obj().emit_by_name::<()>("timer-started", &[]);
    }

    fn on_resume(&self) {
        let is_stopwatch = self.stopwatch_btn.is_active();

        {
            let mut m = if is_stopwatch {
                self.stopwatch_mode.borrow_mut()
            } else {
                self.countdown_mode.borrow_mut()
            };
            m.timer_state = TimerState::Running;
        }

        self.tick_is_stopwatch.set(is_stopwatch);
        self.start_tick();
        self.obj().emit_by_name::<()>("timer-started", &[]);
    }

    /// Called by the window when the running page's Pause button is pressed.
    pub fn on_pause(&self) {
        self.cancel_tick();

        let is_stopwatch = self.tick_is_stopwatch.get();
        let display_secs = {
            let mut m = if is_stopwatch {
                self.stopwatch_mode.borrow_mut()
            } else {
                self.countdown_mode.borrow_mut()
            };
            m.timer_state = TimerState::Paused;
            m.display_secs
        };

        self.paused_time_label.set_label(&format_time(display_secs));
        self.show_paused_ui();
        self.obj().emit_by_name::<()>("timer-paused", &[]);
    }

    /// Called by the window when Stop is pressed (from running page or paused state).
    pub fn on_stop(&self) {
        self.cancel_tick();

        // If the tick was running, use tick_is_stopwatch; otherwise use the toggle.
        let is_stopwatch = self.stopwatch_btn.is_active();

        let elapsed = {
            let mut m = if is_stopwatch {
                self.stopwatch_mode.borrow_mut()
            } else {
                self.countdown_mode.borrow_mut()
            };
            m.timer_state = TimerState::Done;
            if is_stopwatch {
                m.display_secs
            } else {
                m.target_secs.saturating_sub(m.display_secs)
            }
        };

        self.obj().emit_by_name::<()>("timer-stopped", &[]);
        self.show_done(elapsed);
    }

    fn show_done(&self, elapsed_secs: u64) {
        self.done_duration_label.set_label(&format_time(elapsed_secs));
        self.note_row.set_text("");
        self.repopulate_label_combo(self.setup_selected_label_id());
        self.view_stack.set_visible_child_name("done");
    }

    fn on_save(&self) {
        crate::sound::stop_current();
        let is_stopwatch = self.stopwatch_btn.is_active();

        let (elapsed, start_time) = {
            let m = if is_stopwatch {
                self.stopwatch_mode.borrow()
            } else {
                self.countdown_mode.borrow()
            };
            let elapsed = if is_stopwatch {
                m.display_secs
            } else {
                m.target_secs.saturating_sub(m.display_secs)
            };
            (elapsed, m.session_start_time)
        };

        if elapsed == 0 {
            self.reset_mode(is_stopwatch);
            return;
        }

        let note = {
            let t = self.note_row.text();
            if t.is_empty() { None } else { Some(t.to_string()) }
        };
        // Index 0 = "+ New label" (shouldn't reach Save), 1 = "None", 2+ = labels
        let selected = self.label_row.selected() as usize;
        let label_id = match selected {
            0 | 1 => None,
            n => self.db_labels.borrow().get(n - 2).map(|l| l.id),
        };

        let data = SessionData {
            start_time:    start_time,
            duration_secs: elapsed as i64,
            mode:          if is_stopwatch { SessionMode::Stopwatch } else { SessionMode::Countdown },
            label_id,
            note,
        };

        if let Some(app) = self.get_app() {
            app.with_db(|db| db.create_session(&data));
        }

        self.reset_mode(is_stopwatch);
    }

    fn on_discard(&self) {
        crate::sound::stop_current();
        let note = self.note_row.text();
        if !note.is_empty() {
            let dialog = adw::AlertDialog::builder()
                .heading(tr("Discard session?"))
                .body(tr("Your note will be lost."))
                .close_response("cancel")
                .default_response("discard")
                .build();
            dialog.add_response("cancel", tr("Cancel"));
            dialog.add_response("discard", tr("Discard"));
            dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);

            let obj = self.obj().clone();
            let is_stopwatch = self.stopwatch_btn.is_active();
            dialog.connect_response(None, move |_, id| {
                if id == "discard" {
                    obj.imp().reset_mode(is_stopwatch);
                }
            });

            if let Some(win) = self.obj().root()
                .and_then(|r| r.downcast::<gtk::Window>().ok())
            {
                dialog.present(Some(&win));
            }
        } else {
            self.reset_mode(self.stopwatch_btn.is_active());
        }
    }

    /// Reset a single mode back to Idle and update the UI if it's currently shown.
    fn reset_mode(&self, is_stopwatch: bool) {
        {
            let mut m = if is_stopwatch {
                self.stopwatch_mode.borrow_mut()
            } else {
                self.countdown_mode.borrow_mut()
            };
            *m = ModeState::default();
        }

        // Only update the visible UI if this mode is the one currently shown.
        if is_stopwatch == self.stopwatch_btn.is_active() {
            self.show_idle_ui();
            self.refresh_streak();
        }
    }

    fn start_tick(&self) {
        self.cancel_tick();
        let obj = self.obj().clone();
        let is_stopwatch = self.tick_is_stopwatch.get();

        let source_id = glib::timeout_add_local(
            std::time::Duration::from_secs(1),
            move || {
                let imp = obj.imp();

                // Read + update the correct mode state
                let (new_secs, done) = {
                    let mut m = if is_stopwatch {
                        imp.stopwatch_mode.borrow_mut()
                    } else {
                        imp.countdown_mode.borrow_mut()
                    };

                    if m.timer_state != TimerState::Running {
                        return glib::ControlFlow::Break;
                    }

                    if is_stopwatch {
                        m.display_secs += 1;
                        (m.display_secs, false)
                    } else {
                        if m.display_secs == 0 {
                            m.timer_state = TimerState::Done;
                            let elapsed = m.target_secs;
                            (elapsed, true)
                        } else {
                            m.display_secs -= 1;
                            (m.display_secs, false)
                        }
                    }
                };

                if done {
                    // Clear the SourceId before GLib removes it. If we leave it
                    // set, cancel_tick() in dispose() will call src.remove() on
                    // an already-removed source and panic.
                    *imp.tick_source.borrow_mut() = None;
                    *imp.running_label.borrow_mut() = None;

                    obj.emit_by_name::<()>("timer-stopped", &[]);
                    imp.show_done(new_secs);
                    if let Some(app) = imp.get_app() {
                        crate::sound::play_end_sound(&app);
                        // Only send a system notification when the app is not
                        // the focused window — the done screen is already shown
                        // in-app, so a notification would be redundant noise.
                        if !app.active_window().map(|w| w.is_active()).unwrap_or(false) {
                            let n = gtk::gio::Notification::new("Meditation Complete");
                            n.set_body(Some(&format!("Session: {}", format_time(new_secs))));
                            app.send_notification(Some("timer-done"), &n);
                        }
                    }
                    return glib::ControlFlow::Break;
                }

                if let Some(label) = imp.running_label.borrow().as_ref() {
                    label.set_label(&format_time(new_secs));
                }

                glib::ControlFlow::Continue
            },
        );
        *self.tick_source.borrow_mut() = Some(source_id);
    }

    fn cancel_tick(&self) {
        if let Some(src) = self.tick_source.borrow_mut().take() {
            src.remove();
        }
        *self.running_label.borrow_mut() = None;
    }

    pub fn refresh_streak(&self) {
        let Some(app) = self.get_app() else {
            // No app yet (shouldn't happen in practice) — use defaults.
            self.refresh_presets();
            self.refresh_setup_labels(self.setup_selected_label_id());
            return;
        };

        // Batch all three DB reads into a single borrow: one get_app() walk,
        // one RefCell lock, three SQL queries instead of three separate calls.
        let (streak, presets, labels) = app
            .with_db(|db| {
                let streak  = db.get_streak().unwrap_or(0);
                let presets = db.get_presets().unwrap_or_else(|_| vec![5, 10, 15, 20, 30]);
                let labels  = db.list_labels().unwrap_or_default();
                (streak, presets, labels)
            })
            .unwrap_or_else(|| (0, vec![5, 10, 15, 20, 30], vec![]));

        // Update streak label
        let text = match streak {
            0 => String::from("Start your streak today"),
            1 => String::from("🔥 1 day"),
            n => format!("🔥 {n} days"),
        };
        self.streak_label.set_label(&text);

        // Rebuild preset buttons with the data we already fetched
        self.rebuild_preset_chips(&presets);

        // Populate setup page sound row from DB setting
        let current_sound = app
            .with_db(|db| db.get_setting("end_sound", "none"))
            .and_then(|r| r.ok())
            .unwrap_or_else(|| "none".to_string());
        self.sound_populating.set(true);
        self.setup_sound_row.set_selected(match current_sound.as_str() {
            "bowl"   => 1,
            "bell"   => 2,
            "gong"   => 3,
            "custom" => 4,
            _        => 0,
        });
        self.sound_populating.set(false);

        // Rebuild setup label combo with the data we already fetched
        let select_id = self.setup_selected_label_id();
        let select_idx = select_id
            .and_then(|id| labels.iter().position(|l| l.id == id))
            .map(|pos| (pos + 2) as u32)
            .unwrap_or(1);
        let names: Vec<String> = std::iter::once("+ New label".to_string())
            .chain(std::iter::once("None".to_string()))
            .chain(labels.iter().map(|l| l.name.clone()))
            .collect();
        let name_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
        *self.setup_db_labels.borrow_mut() = labels;
        self.setup_populating.set(true);
        self.setup_label_row.set_model(Some(&gtk::StringList::new(&name_refs)));
        self.setup_label_row.set_selected(select_idx);
        self.setup_populating.set(false);
    }

    pub fn refresh_presets(&self) {
        let presets = self.get_app()
            .and_then(|app| app.with_db(|db| db.get_presets()))
            .and_then(|r| r.ok())
            .unwrap_or_else(|| vec![5, 10, 15, 20, 30]);
        self.rebuild_preset_chips(&presets);
    }

    /// Rebuild the preset FlowBox: one pill per DB preset (each tapping
    /// it selects that duration), plus a trailing "Custom" pill that
    /// opens a dialog to pick an arbitrary H:M value.
    fn rebuild_preset_chips(&self, presets: &[u32]) {
        while let Some(child) = self.presets_box.first_child() {
            self.presets_box.remove(&child);
        }
        let obj = self.obj();
        for &mins in presets {
            let (label, tooltip) = if mins < 60 {
                (format!("{mins}m"), format!("{mins} minutes"))
            } else {
                let h = mins / 60;
                let m = mins % 60;
                if m == 0 {
                    (format!("{h}h"), format!("{h} hour{}", if h == 1 { "" } else { "s" }))
                } else {
                    (format!("{h}h {m}m"), format!("{h}h {m}min"))
                }
            };
            let btn = gtk::Button::builder()
                .label(&label)
                .tooltip_text(&tooltip)
                .css_classes(["preset-chip"])
                .build();
            btn.connect_clicked(glib::clone!(
                #[weak(rename_to = this)] obj,
                move |_| {
                    this.imp().set_countdown_target((mins as u64) * 60);
                }
            ));
            self.presets_box.append(&btn);
        }

        // Trailing "Custom" pill — opens a dialog to pick an H:M value.
        let custom_btn = gtk::Button::builder()
            .label("Custom")
            .tooltip_text("Set a custom time")
            .css_classes(["preset-chip"])
            .build();
        custom_btn.connect_clicked(glib::clone!(
            #[weak(rename_to = this)] obj,
            move |_| this.imp().show_custom_time_dialog()
        ));
        self.presets_box.append(&custom_btn);
    }

    /// Update the countdown target + hero label together.
    fn set_countdown_target(&self, secs: u64) {
        self.countdown_target_secs.set(secs);
        let h = secs / 3600;
        let m = (secs % 3600) / 60;
        self.big_time_label.set_label(&format!("{h:02}:{m:02}"));
    }

    /// Show a dialog with H:M spin buttons; apply result to the countdown
    /// target on "Set".
    fn show_custom_time_dialog(&self) {
        let current = self.countdown_target_secs.get();
        let cur_h = (current / 3600) as f64;
        let cur_m = ((current % 3600) / 60) as f64;

        let hours_spin = gtk::SpinButton::builder()
            .orientation(gtk::Orientation::Vertical)
            .numeric(true)
            .width_chars(2)
            .adjustment(&gtk::Adjustment::new(cur_h, 0.0, 23.0, 1.0, 1.0, 0.0))
            .build();
        let minutes_spin = gtk::SpinButton::builder()
            .orientation(gtk::Orientation::Vertical)
            .numeric(true)
            .width_chars(2)
            .adjustment(&gtk::Adjustment::new(cur_m, 0.0, 59.0, 1.0, 5.0, 0.0))
            .build();

        let colon = gtk::Label::builder()
            .label(":")
            .css_classes(["title-2"])
            .valign(gtk::Align::Center)
            .build();
        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(6)
            .halign(gtk::Align::Center)
            .margin_top(6)
            .margin_bottom(6)
            .build();
        row.append(&hours_spin);
        row.append(&colon);
        row.append(&minutes_spin);

        let dialog = adw::AlertDialog::builder()
            .heading(tr("Custom Time"))
            .body(tr("Hours : Minutes"))
            .close_response("cancel")
            .default_response("set")
            .extra_child(&row)
            .build();
        dialog.add_response("cancel", tr("Cancel"));
        dialog.add_response("set", tr("Set"));
        dialog.set_response_appearance("set", adw::ResponseAppearance::Suggested);

        let obj = self.obj().clone();
        dialog.connect_response(None, move |_, response| {
            if response != "set" { return; }
            let h = hours_spin.value() as u64;
            let m = minutes_spin.value() as u64;
            let total = h * 3600 + m * 60;
            if total == 0 { return; }
            obj.imp().set_countdown_target(total);
        });

        if let Some(win) = self.obj().root().and_then(|r| r.downcast::<gtk::Window>().ok()) {
            dialog.present(Some(&win));
        }
    }

    /// Rebuild the label combo from the DB.
    /// `select_id`: if Some, auto-selects that label; otherwise selects "None" (index 1).
    fn repopulate_label_combo(&self, select_id: Option<i64>) {
        let mut labels = Vec::new();
        if let Some(app) = self.get_app() {
            if let Some(fetched) = app.with_db(|db| db.list_labels()) {
                labels = fetched.unwrap_or_default();
            }
        }

        let select_idx = select_id
            .and_then(|id| labels.iter().position(|l| l.id == id))
            .map(|pos| (pos + 2) as u32) // +2 for "+ New label" and "None"
            .unwrap_or(1);              // default = "None"

        let names: Vec<String> = std::iter::once("+ New label".to_string())
            .chain(std::iter::once("None".to_string()))
            .chain(labels.iter().map(|l| l.name.clone()))
            .collect();
        let name_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();

        *self.db_labels.borrow_mut() = labels;

        self.populating_labels.set(true);
        self.label_row.set_model(Some(&gtk::StringList::new(&name_refs)));
        self.label_row.set_selected(select_idx);
        self.populating_labels.set(false);
    }

    /// Populate the pre-start label combo from the DB.
    /// `select_id`: if Some, keeps that label selected; otherwise selects "None".
    fn refresh_setup_labels(&self, select_id: Option<i64>) {
        let labels = self.get_app()
            .and_then(|app| app.with_db(|db| db.list_labels()))
            .and_then(|r| r.ok())
            .unwrap_or_default();

        let select_idx = select_id
            .and_then(|id| labels.iter().position(|l| l.id == id))
            .map(|pos| (pos + 2) as u32)
            .unwrap_or(1); // default: "None"

        let names: Vec<String> = std::iter::once("+ New label".to_string())
            .chain(std::iter::once("None".to_string()))
            .chain(labels.iter().map(|l| l.name.clone()))
            .collect();
        let name_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();

        *self.setup_db_labels.borrow_mut() = labels;
        self.setup_populating.set(true);
        self.setup_label_row.set_model(Some(&gtk::StringList::new(&name_refs)));
        self.setup_label_row.set_selected(select_idx);
        self.setup_populating.set(false);
    }

    /// Returns the label ID currently selected in the pre-start combo, if any.
    fn setup_selected_label_id(&self) -> Option<i64> {
        let selected = self.setup_label_row.selected() as usize;
        match selected {
            0 | 1 => None,
            n => self.setup_db_labels.borrow().get(n - 2).map(|l| l.id),
        }
    }

    /// Show the new-label dialog, selecting the result in the pre-start combo.
    fn show_new_label_dialog_for_setup(&self) {
        let (entry, dialog) = build_new_label_dialog();
        let obj = self.obj().clone();
        dialog.connect_response(None, {
            let entry = entry.clone();
            move |_, response| {
                let imp = obj.imp();
                if response != "create" {
                    imp.setup_label_row.set_selected(1); // revert to "None"
                    return;
                }
                let name = entry.text().trim().to_string();
                if name.is_empty() { imp.setup_label_row.set_selected(1); return; }
                let new_label = imp.get_app()
                    .and_then(|app| app.with_db(|db| db.create_label(&name)))
                    .and_then(|r| r.ok());
                imp.refresh_setup_labels(new_label.map(|l| l.id));
            }
        });
        if let Some(win) = self.obj().root().and_then(|r| r.downcast::<gtk::Window>().ok()) {
            dialog.present(Some(&win));
        }
    }

    /// Show a dialog to create a new label, then select it in the done-page combo.
    fn show_new_label_dialog(&self) {
        let (entry, dialog) = build_new_label_dialog();
        let obj = self.obj().clone();
        dialog.connect_response(None, {
            let entry = entry.clone();
            move |_, response| {
                let imp = obj.imp();
                if response != "create" {
                    imp.label_row.set_selected(1); // revert to "None"
                    return;
                }
                let name = entry.text().trim().to_string();
                if name.is_empty() { imp.label_row.set_selected(1); return; }
                let new_label = imp.get_app()
                    .and_then(|app| app.with_db(|db| db.create_label(&name)))
                    .and_then(|r| r.ok());
                imp.repopulate_label_combo(new_label.map(|l| l.id));
            }
        });
        if let Some(win) = self.obj().root().and_then(|r| r.downcast::<gtk::Window>().ok()) {
            dialog.present(Some(&win));
        }
    }

    fn get_app(&self) -> Option<crate::application::MeditateApplication> {
        self.obj()
            .root()
            .and_then(|r| r.downcast::<gtk::Window>().ok())
            .and_then(|w| w.application())
            .and_then(|a| a.downcast::<crate::application::MeditateApplication>().ok())
    }

    pub fn current_display_secs(&self) -> u64 {
        // Return the display value for whichever mode is about to go running.
        let is_stopwatch = self.tick_is_stopwatch.get();
        if is_stopwatch {
            self.stopwatch_mode.borrow().display_secs
        } else {
            self.countdown_mode.borrow().display_secs
        }
    }

    pub fn set_running_label(&self, label: gtk::Label) {
        *self.running_label.borrow_mut() = Some(label);
    }

    pub fn toggle_playback(&self) {
        let is_stopwatch = self.stopwatch_btn.is_active();
        let state = {
            let m = if is_stopwatch {
                self.stopwatch_mode.borrow()
            } else {
                self.countdown_mode.borrow()
            };
            m.timer_state
        };
        match state {
            TimerState::Idle    => self.on_start(),
            TimerState::Running => self.on_pause(),
            TimerState::Paused  => self.on_resume(),
            TimerState::Done    => {}
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

pub fn format_time(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Build the shared "New Label" alert dialog + text entry.
fn build_new_label_dialog() -> (gtk::Entry, adw::AlertDialog) {
    let entry = gtk::Entry::builder()
        .placeholder_text("Label name")
        .activates_default(true)
        .build();
    let dialog = adw::AlertDialog::builder()
        .heading("New Label")
        .close_response("cancel")
        .default_response("create")
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("create", "Create");
    dialog.set_response_appearance("create", adw::ResponseAppearance::Suggested);
    dialog.set_response_enabled("create", false);
    dialog.set_extra_child(Some(&entry));
    entry.connect_changed(glib::clone!(
        #[weak] dialog,
        move |e| dialog.set_response_enabled("create", !e.text().trim().is_empty())
    ));
    (entry, dialog)
}

fn tr(s: &'static str) -> &'static str { s }
