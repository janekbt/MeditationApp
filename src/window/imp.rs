use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{glib, CompositeTemplate};

use gtk::gio;

use crate::log::LogView;
use crate::stats::StatsView;
use crate::timer::{format_time, TimerView};

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
                            crate::sound::preload_end_sound(&app);
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
        self.timer_view.connect_timer_paused(glib::clone!(
            #[weak] obj,
            move |_| { obj.imp().nav_view.pop(); }
        ));
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

        let time_label = gtk::Label::builder()
            .label(format_time(self.timer_view.current_display_secs()))
            .css_classes(["timer-setup-display"])
            .halign(gtk::Align::Center)
            .build();
        self.timer_view.set_running_label(time_label.clone());

        // Pause is a regular action (toggle-ish, non-destructive); don't
        // accent-tint it or it visually outranks Stop. Stop, per HIG, is
        // the consequential action in this view — but not destructive
        // enough for .destructive-action either, so leave it as a plain
        // .pill button.
        let pause_btn = gtk::Button::builder()
            .label("Pause")
            .css_classes(["pill"])
            .tooltip_text("Pause Timer")
            .build();
        let stop_btn = gtk::Button::builder()
            .label("Stop")
            .css_classes(["pill"])
            .tooltip_text("Stop and Save Session")
            .build();

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
        content.append(&btn_box);

        let header = adw::HeaderBar::builder().show_back_button(false).build();
        let toolbar_view = adw::ToolbarView::new();
        toolbar_view.add_top_bar(&header);
        toolbar_view.set_content(Some(&content));

        let page = adw::NavigationPage::builder()
            .tag("running").title("Meditating")
            .child(&toolbar_view)
            .build();

        let obj = self.obj().clone();
        pause_btn.connect_clicked(move |_| obj.imp().timer_view.pause());
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
            eprintln!(
                "note: GSettings schema '{}' not found — window size won't persist. \
                 Set GSETTINGS_SCHEMA_DIR=build/data for dev builds.",
                crate::config::APP_ID,
            );
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
