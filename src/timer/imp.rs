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
    #[template_child] pub hours_spin:            TemplateChild<gtk::SpinButton>,
    #[template_child] pub minutes_spin:          TemplateChild<gtk::SpinButton>,
    #[template_child] pub hm_box:                TemplateChild<gtk::Box>,
    #[template_child] pub presets_box:           TemplateChild<gtk::Box>,
    #[template_child] pub stopwatch_idle_label:  TemplateChild<gtk::Label>,
    #[template_child] pub preset_5:              TemplateChild<gtk::Button>,
    #[template_child] pub preset_10:             TemplateChild<gtk::Button>,
    #[template_child] pub preset_15:             TemplateChild<gtk::Button>,
    #[template_child] pub preset_20:             TemplateChild<gtk::Button>,
    #[template_child] pub preset_30:             TemplateChild<gtk::Button>,
    #[template_child] pub paused_time_label:     TemplateChild<gtk::Label>,
    #[template_child] pub start_btn:             TemplateChild<gtk::Button>,
    #[template_child] pub resume_btn:            TemplateChild<gtk::Button>,
    #[template_child] pub stop_from_pause_btn:   TemplateChild<gtk::Button>,
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
    /// Labels fetched from DB when entering Done state.
    db_labels: RefCell<Vec<Label>>,
    /// True while show_done/repopulate_label_combo is rebuilding the model,
    /// to suppress the notify::selected handler from opening the new-label dialog.
    populating_labels: Cell<bool>,
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

        // Preset buttons set H:M spin values
        for (btn, mins) in [
            (&*self.preset_5, 5u64),
            (&*self.preset_10, 10),
            (&*self.preset_15, 15),
            (&*self.preset_20, 20),
            (&*self.preset_30, 30),
        ] {
            btn.connect_clicked(glib::clone!(
                #[weak(rename_to = this)] obj,
                move |_| {
                    this.imp().hours_spin.set_value(0.0);
                    this.imp().minutes_spin.set_value(mins as f64);
                }
            ));
        }

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
    }
}

// ── Mode switching ────────────────────────────────────────────────────────────

impl TimerView {
    /// Called whenever the mode toggle fires. `to_stopwatch` is true when the
    /// user switched TO stopwatch (false = switched to countdown).
    fn on_mode_switched(&self, to_stopwatch: bool) {
        // Show / hide the countdown-specific input widgets
        self.hm_box.set_visible(!to_stopwatch);
        self.presets_box.set_visible(!to_stopwatch);
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
            let h = self.hours_spin.value() as u64;
            let m_val = self.minutes_spin.value() as u64;
            if h == 0 && m_val == 0 {
                return;
            }
            let target = h * 3600 + m_val * 60;
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
        self.repopulate_label_combo(None);
        self.view_stack.set_visible_child_name("done");
    }

    fn on_save(&self) {
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
                    obj.emit_by_name::<()>("timer-stopped", &[]);
                    imp.show_done(new_secs); // new_secs == target here
                    if let Some(app) = imp.get_app() {
                        crate::sound::play_end_sound(&app);
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

    /// Show a dialog to create a new label, then select it in the combo.
    fn show_new_label_dialog(&self) {
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

        // Enable "Create" only when the entry is non-empty
        entry.connect_changed(glib::clone!(
            #[weak] dialog,
            move |e| dialog.set_response_enabled("create", !e.text().trim().is_empty())
        ));

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
                if name.is_empty() {
                    imp.label_row.set_selected(1);
                    return;
                }
                let new_label = imp.get_app()
                    .and_then(|app| app.with_db(|db| db.create_label(&name)))
                    .and_then(|r| r.ok());
                imp.repopulate_label_combo(new_label.map(|l| l.id));
            }
        });

        if let Some(win) = self.obj().root()
            .and_then(|r| r.downcast::<gtk::Window>().ok())
        {
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

fn tr(s: &'static str) -> &'static str { s }
