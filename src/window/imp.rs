use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{glib, CompositeTemplate};

use crate::timer::{format_time, TimerView};

#[derive(Debug, Default, CompositeTemplate)]
#[template(resource = "/io/github/janekbt/Meditate/ui/window.ui")]
pub struct MeditateWindow {
    #[template_child] pub view_stack:  TemplateChild<adw::ViewStack>,
    #[template_child] pub switcher_bar: TemplateChild<adw::ViewSwitcherBar>,
    #[template_child] pub nav_view:    TemplateChild<adw::NavigationView>,
    #[template_child] pub timer_view:  TemplateChild<TimerView>,
}

#[glib::object_subclass]
impl ObjectSubclass for MeditateWindow {
    const NAME: &'static str = "MeditateWindow";
    type Type = super::MeditateWindow;
    type ParentType = adw::ApplicationWindow;

    fn class_init(klass: &mut Self::Class) {
        // Ensure TimerView type is registered before we bind the template.
        TimerView::ensure_type();
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
        self.timer_view.refresh_streak();
    }
}

impl WidgetImpl for MeditateWindow {}
impl WindowImpl for MeditateWindow {}
impl ApplicationWindowImpl for MeditateWindow {}
impl AdwApplicationWindowImpl for MeditateWindow {}

// ── Timer signal wiring ───────────────────────────────────────────────────────

impl MeditateWindow {
    fn wire_timer_signals(&self) {
        let obj = self.obj();

        self.timer_view.connect_timer_started(glib::clone!(
            #[weak]
            obj,
            move |_| obj.imp().push_running_page()
        ));

        self.timer_view.connect_timer_paused(glib::clone!(
            #[weak]
            obj,
            move |_| {
                obj.imp().nav_view.pop();
            }
        ));

        self.timer_view.connect_timer_stopped(glib::clone!(
            #[weak]
            obj,
            move |_| {
                // Pop the running page if it's still on the stack.
                if obj.imp().nav_view.find_page("running").is_some() {
                    obj.imp().nav_view.pop();
                }
            }
        ));
    }

    pub fn push_running_page(&self) {
        // Don't double-push.
        if self.nav_view.find_page("running").is_some() {
            return;
        }

        let initial_secs = self.timer_view.current_display_secs();

        // ── Time label ────────────────────────────────────────────────
        let time_label = gtk::Label::builder()
            .label(&format_time(initial_secs))
            .css_classes(["large-title"])
            .halign(gtk::Align::Center)
            .build();

        // Give the timer view a weak ref to this label for live updates.
        self.timer_view.set_running_label(time_label.clone());

        // ── Buttons ───────────────────────────────────────────────────
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

        // ── Layout ────────────────────────────────────────────────────
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(32)
            .valign(gtk::Align::Center)
            .vexpand(true)
            .margin_top(24)
            .margin_bottom(24)
            .margin_start(18)
            .margin_end(18)
            .build();
        content.append(&time_label);
        content.append(&btn_box);

        let header = adw::HeaderBar::builder()
            .show_back_button(false)
            .build();

        let toolbar_view = adw::ToolbarView::new();
        toolbar_view.add_top_bar(&header);
        toolbar_view.set_content(Some(&content));

        let page = adw::NavigationPage::builder()
            .tag("running")
            .title("Meditating")
            .child(&toolbar_view)
            .build();

        // ── Button callbacks ──────────────────────────────────────────
        let obj = self.obj().clone();
        pause_btn.connect_clicked(move |_| obj.imp().on_running_pause());

        let obj2 = self.obj().clone();
        stop_btn.connect_clicked(move |_| obj2.imp().on_running_stop());

        self.nav_view.push(&page);
    }

    fn on_running_pause(&self) {
        self.timer_view.pause();
        // Signal handler pops the nav page.
    }

    fn on_running_stop(&self) {
        self.timer_view.stop();
        // Signal handler pops the nav page.
    }
}
