use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{glib, CompositeTemplate};

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

        // Refresh streak once the window is fully realized (get_app() is
        // not guaranteed to succeed earlier, during GObject construction).
        let obj = self.obj();
        obj.connect_map(glib::clone!(
            #[weak] obj,
            move |_| { obj.imp().timer_view.refresh_streak(); }
        ));
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
            .label(&format_time(self.timer_view.current_display_secs()))
            .css_classes(["large-title"])
            .halign(gtk::Align::Center)
            .build();
        self.timer_view.set_running_label(time_label.clone());

        let pause_btn = gtk::Button::builder()
            .label("Pause")
            .css_classes(["suggested-action", "pill"])
            .tooltip_text("Pause the timer")
            .build();
        let stop_btn = gtk::Button::builder()
            .label("Stop")
            .css_classes(["pill"])
            .tooltip_text("Stop and save the session")
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
            .margin_start(18).margin_end(18)
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
        let builder = gtk::Builder::from_resource(
            "/io/github/janekbt/Meditate/ui/shortcuts.ui",
        );
        if let Some(overlay) = builder.object::<gtk::ShortcutsWindow>("help_overlay") {
            self.obj().set_help_overlay(Some(&overlay));
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
