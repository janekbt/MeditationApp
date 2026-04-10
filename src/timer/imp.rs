use std::cell::{Cell, RefCell};
use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{glib, CompositeTemplate};
use glib::subclass::Signal;
use std::sync::OnceLock;

use crate::db::{SessionData, SessionMode};

// ── State ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimerState {
    #[default]
    Idle,
    Running,
    Paused,
    Done,
}

// ── GObject impl ──────────────────────────────────────────────────────────────

#[derive(Debug, Default, CompositeTemplate)]
#[template(resource = "/io/github/janekbt/Meditate/ui/timer_view.ui")]
pub struct TimerView {
    // Template children
    #[template_child] pub view_stack:          TemplateChild<gtk::Stack>,
    #[template_child] pub streak_label:        TemplateChild<gtk::Label>,
    #[template_child] pub countdown_btn:       TemplateChild<gtk::ToggleButton>,
    #[template_child] pub stopwatch_btn:       TemplateChild<gtk::ToggleButton>,
    #[template_child] pub inputs_stack:        TemplateChild<gtk::Stack>,
    #[template_child] pub hours_spin:          TemplateChild<gtk::SpinButton>,
    #[template_child] pub minutes_spin:        TemplateChild<gtk::SpinButton>,
    #[template_child] pub hm_box:             TemplateChild<gtk::Box>,
    #[template_child] pub presets_box:        TemplateChild<gtk::Box>,
    #[template_child] pub stopwatch_idle_label: TemplateChild<gtk::Label>,
    #[template_child] pub preset_5:            TemplateChild<gtk::Button>,
    #[template_child] pub preset_10:           TemplateChild<gtk::Button>,
    #[template_child] pub preset_15:           TemplateChild<gtk::Button>,
    #[template_child] pub preset_20:           TemplateChild<gtk::Button>,
    #[template_child] pub preset_30:           TemplateChild<gtk::Button>,
    #[template_child] pub paused_time_label:   TemplateChild<gtk::Label>,
    #[template_child] pub start_btn:           TemplateChild<gtk::Button>,
    #[template_child] pub resume_btn:          TemplateChild<gtk::Button>,
    #[template_child] pub stop_from_pause_btn: TemplateChild<gtk::Button>,
    #[template_child] pub done_duration_label: TemplateChild<gtk::Label>,
    #[template_child] pub note_row:            TemplateChild<adw::EntryRow>,
    #[template_child] pub label_row:           TemplateChild<adw::ComboRow>,
    #[template_child] pub discard_btn:         TemplateChild<gtk::Button>,
    #[template_child] pub save_btn:            TemplateChild<gtk::Button>,

    // Timer state
    pub state:              Cell<TimerState>,
    /// Seconds remaining (countdown) or elapsed (stopwatch) — updated each tick.
    pub display_secs:       Cell<u64>,
    /// Target duration in seconds (countdown mode only).
    pub target_secs:        Cell<u64>,
    /// Unix timestamp when the session started.
    pub session_start_time: Cell<i64>,
    /// Active glib timeout handle.
    pub tick_source:        RefCell<Option<glib::SourceId>>,
    /// Weak ref to the running-page time label for live updates.
    pub running_label:      RefCell<Option<gtk::Label>>,
    /// Labels fetched from DB when entering Done state.
    pub db_labels:          RefCell<Vec<crate::db::Label>>,
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
                // Emitted when the user presses Start or Resume.
                Signal::builder("timer-started").build(),
                // Emitted when pause() is called (window pops running page).
                Signal::builder("timer-paused").build(),
                // Emitted when stop() is called (window pops running page).
                Signal::builder("timer-stopped").build(),
            ]
        })
    }

    fn constructed(&self) {
        self.parent_constructed();
        self.setup_buttons();
    }

    fn dispose(&self) {
        self.cancel_tick();
        self.obj().first_child().map(|w| w.unparent());
    }
}

impl WidgetImpl for TimerView {}

// ── Internal helpers ──────────────────────────────────────────────────────────

impl TimerView {
    fn setup_buttons(&self) {
        let obj = self.obj();

        // Preset buttons set minutes_spin
        for (btn, mins) in [
            (&*self.preset_5, 5u64),
            (&*self.preset_10, 10),
            (&*self.preset_15, 15),
            (&*self.preset_20, 20),
            (&*self.preset_30, 30),
        ] {
            btn.connect_clicked(glib::clone!(
                #[weak(rename_to = this)]
                obj,
                move |_| {
                    let imp = this.imp();
                    imp.hours_spin.set_value(0.0);
                    imp.minutes_spin.set_value(mins as f64);
                }
            ));
        }

        // Mode toggle: swap H:M inputs / presets for "00:00" in stopwatch mode
        self.stopwatch_btn.connect_toggled(glib::clone!(
            #[weak(rename_to = this)]
            obj,
            move |btn| {
                let imp = this.imp();
                let is_stopwatch = btn.is_active();
                imp.hm_box.set_visible(!is_stopwatch);
                imp.presets_box.set_visible(!is_stopwatch);
                imp.stopwatch_idle_label.set_visible(is_stopwatch);
            }
        ));

        // Start button
        self.start_btn.connect_clicked(glib::clone!(
            #[weak(rename_to = this)]
            obj,
            move |_| this.imp().on_start()
        ));

        // Resume button
        self.resume_btn.connect_clicked(glib::clone!(
            #[weak(rename_to = this)]
            obj,
            move |_| this.imp().on_resume()
        ));

        // Stop from paused state
        self.stop_from_pause_btn.connect_clicked(glib::clone!(
            #[weak(rename_to = this)]
            obj,
            move |_| this.imp().on_stop()
        ));

        // Save session
        self.save_btn.connect_clicked(glib::clone!(
            #[weak(rename_to = this)]
            obj,
            move |_| this.imp().on_save()
        ));

        // Discard
        self.discard_btn.connect_clicked(glib::clone!(
            #[weak(rename_to = this)]
            obj,
            move |_| this.imp().on_discard()
        ));
    }

    fn on_start(&self) {
        let h = self.hours_spin.value() as u64;
        let m = self.minutes_spin.value() as u64;

        let is_stopwatch = self.stopwatch_btn.is_active();

        if !is_stopwatch && h == 0 && m == 0 {
            // Nothing to count down — shake the spin row?
            return;
        }

        let target = if is_stopwatch { 0 } else { h * 3600 + m * 60 };
        self.target_secs.set(target);
        self.display_secs.set(target);
        self.state.set(TimerState::Running);
        self.session_start_time.set(unix_now());
        self.start_tick();
        self.obj().emit_by_name::<()>("timer-started", &[]);
    }

    fn on_resume(&self) {
        self.state.set(TimerState::Running);
        self.start_tick();
        // Switch back to inputs_stack "inputs" page isn't needed here —
        // the window will push the running nav page on timer-started.
        self.obj().emit_by_name::<()>("timer-started", &[]);
    }

    pub fn on_stop(&self) {
        self.cancel_tick();
        let elapsed = if self.countdown_btn.is_active() {
            self.target_secs.get().saturating_sub(self.display_secs.get())
        } else {
            self.display_secs.get()
        };
        self.state.set(TimerState::Done);
        self.obj().emit_by_name::<()>("timer-stopped", &[]);
        self.show_done(elapsed);
    }

    pub fn on_pause(&self) {
        self.cancel_tick();
        self.state.set(TimerState::Paused);
        self.paused_time_label.set_label(&format_time(self.display_secs.get()));
        self.inputs_stack.set_visible_child_name("paused");
        self.start_btn.set_visible(false);
        self.resume_btn.set_visible(true);
        self.stop_from_pause_btn.set_visible(true);
        self.obj().emit_by_name::<()>("timer-paused", &[]);
    }

    fn show_done(&self, elapsed_secs: u64) {
        self.done_duration_label.set_label(&format_time(elapsed_secs));

        // Populate label combo from DB
        let mut labels = Vec::new();
        if let Some(app) = self.get_app() {
            if let Some(fetched) = app.with_db(|db| db.list_labels()) {
                labels = fetched.unwrap_or_default();
            }
        }
        let names: Vec<&str> = std::iter::once("None")
            .chain(labels.iter().map(|l| l.name.as_str()))
            .collect();
        let model = gtk::StringList::new(&names);
        self.label_row.set_model(Some(&model));
        self.label_row.set_selected(0);
        *self.db_labels.borrow_mut() = labels;

        // Clear note
        self.note_row.set_text("");

        self.view_stack.set_visible_child_name("done");
    }

    fn on_save(&self) {
        let elapsed = if self.countdown_btn.is_active() {
            self.target_secs.get().saturating_sub(self.display_secs.get())
        } else {
            self.display_secs.get()
        };

        if elapsed == 0 {
            self.reset();
            return;
        }

        let note = {
            let t = self.note_row.text();
            if t.is_empty() { None } else { Some(t.to_string()) }
        };

        let selected = self.label_row.selected() as usize;
        let label_id = if selected == 0 {
            None
        } else {
            self.db_labels.borrow().get(selected - 1).map(|l| l.id)
        };

        let mode = if self.stopwatch_btn.is_active() {
            SessionMode::Stopwatch
        } else {
            SessionMode::Countdown
        };

        let data = SessionData {
            start_time:    self.session_start_time.get(),
            duration_secs: elapsed as i64,
            mode,
            label_id,
            note,
        };

        if let Some(app) = self.get_app() {
            app.with_db(|db| db.create_session(&data));

            // Show a toast on the window
            if let Some(win) = self.obj().root()
                .and_then(|r| r.downcast::<adw::ApplicationWindow>().ok())
            {
                let toast = adw::Toast::builder()
                    .title(&format!("Session saved — {}", format_time(elapsed)))
                    .timeout(3)
                    .build();
                // AdwApplicationWindow doesn't have add_toast directly;
                // we need AdwToastOverlay. Use the window's overlay if accessible.
                // For now just print — wired up when we add the toast overlay.
                let _ = (win, toast);
            }
        }

        self.reset();
    }

    fn on_discard(&self) {
        let note = self.note_row.text();
        if !note.is_empty() {
            // Confirm discard if note was typed
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
            dialog.connect_response(None, move |_, id| {
                if id == "discard" {
                    obj.imp().reset();
                }
            });

            if let Some(win) = self.obj().root()
                .and_then(|r| r.downcast::<gtk::Window>().ok())
            {
                dialog.present(Some(&win));
            }
        } else {
            self.reset();
        }
    }

    fn reset(&self) {
        self.cancel_tick();
        self.state.set(TimerState::Idle);
        self.display_secs.set(0);
        self.inputs_stack.set_visible_child_name("inputs");
        self.start_btn.set_visible(true);
        self.resume_btn.set_visible(false);
        self.stop_from_pause_btn.set_visible(false);
        self.view_stack.set_visible_child_name("setup");
        self.note_row.set_text("");
        self.refresh_streak();
    }

    fn start_tick(&self) {
        self.cancel_tick();
        let obj = self.obj().clone();
        let source_id = glib::timeout_add_local(
            std::time::Duration::from_secs(1),
            move || {
                let imp = obj.imp();
                if imp.state.get() != TimerState::Running {
                    return glib::ControlFlow::Break;
                }

                let is_countdown = imp.countdown_btn.is_active();
                let current = imp.display_secs.get();

                if is_countdown {
                    if current == 0 {
                        // Timer finished
                        imp.cancel_tick();
                        imp.state.set(TimerState::Done);
                        obj.emit_by_name::<()>("timer-stopped", &[]);
                        let elapsed = imp.target_secs.get();
                        imp.show_done(elapsed);
                        return glib::ControlFlow::Break;
                    }
                    let new_secs = current - 1;
                    imp.display_secs.set(new_secs);
                    if let Some(label) = imp.running_label.borrow().as_ref() {
                        label.set_label(&format_time(new_secs));
                    }
                } else {
                    let new_secs = current + 1;
                    imp.display_secs.set(new_secs);
                    if let Some(label) = imp.running_label.borrow().as_ref() {
                        label.set_label(&format_time(new_secs));
                    }
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
        if let Some(app) = self.get_app() {
            let streak = app
                .with_db(|db| db.get_streak())
                .and_then(|r| r.ok())
                .unwrap_or(0);
            let text = match streak {
                0 => String::from("Start your streak today"),
                1 => String::from("1-day streak"),
                n => format!("{n}-day streak"),
            };
            self.streak_label.set_label(&text);
        }
    }

    fn get_app(&self) -> Option<crate::application::MeditateApplication> {
        use gtk::prelude::*;
        self.obj()
            .root()
            .and_then(|r| r.downcast::<gtk::Window>().ok())
            .and_then(|w| w.application())
            .and_then(|a| a.downcast::<crate::application::MeditateApplication>().ok())
    }

    pub fn current_display_secs(&self) -> u64 {
        self.display_secs.get()
    }

    pub fn set_running_label(&self, label: gtk::Label) {
        *self.running_label.borrow_mut() = Some(label);
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

// Tiny gettext stub — real i18n wired up later via gettextrs.
fn tr(s: &'static str) -> &'static str { s }
