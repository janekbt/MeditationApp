mod imp;

use glib::prelude::IsA;
use gtk::{gio, glib};

glib::wrapper! {
    pub struct MeditateWindow(ObjectSubclass<imp::MeditateWindow>)
        @extends adw::ApplicationWindow, gtk::ApplicationWindow, gtk::Window, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable,
                    gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl MeditateWindow {
    pub fn new(app: &impl IsA<adw::Application>) -> Self {
        glib::Object::builder()
            .property("application", app)
            .build()
    }

    pub fn add_toast(&self, toast: adw::Toast) {
        use glib::subclass::prelude::ObjectSubclassIsExt;
        self.imp().add_toast(toast);
    }

    /// Push the interval-bell library page onto the navigation view.
    /// Triggered by the "Interval Bells" row in the timer setup; this
    /// wrapper keeps the timer module from having to know about the
    /// window's internal layout. The on_changed callback runs after
    /// any add / toggle inside the library so the timer's count
    /// subtitle stays in sync without the user having to leave + return.
    pub fn push_bells_page(&self, app: &crate::application::MeditateApplication) {
        use glib::subclass::prelude::ObjectSubclassIsExt;
        let timer_view = self.imp().timer_view.clone();
        crate::bells::push_bells_page(
            &self.imp().nav_view,
            app,
            move || timer_view.refresh_interval_bells_count(),
        );
    }

    /// Push the bell-sound chooser onto the navigation view.
    /// Triggered by every bell-sound row in the app — Starting Bell,
    /// per-bell row in the interval-bell edit page, End Bell. The
    /// caller's on_selected callback fires when the user taps a sound
    /// row, receives the chosen UUID, and the page pops automatically.
    pub fn push_sound_chooser(
        &self,
        app: &crate::application::MeditateApplication,
        category: crate::db::BellSoundCategory,
        current_uuid: Option<String>,
        on_selected: impl Fn(String) + 'static,
    ) {
        use glib::subclass::prelude::ObjectSubclassIsExt;
        clear_focus_before_push(self);
        crate::sounds::push_sounds_chooser(
            &self.imp().nav_view,
            app,
            category,
            current_uuid,
            on_selected,
        );
    }

    /// Push the vibration-pattern chooser onto the navigation view.
    /// Triggered by every per-bell Pattern row + each Box Breath
    /// phase row. The caller's `on_selected` callback fires with
    /// the chosen UUID and the page pops automatically.
    pub fn push_vibrations_chooser(
        &self,
        app: &crate::application::MeditateApplication,
        current_uuid: Option<String>,
        on_selected: impl Fn(String) + 'static,
    ) {
        use glib::subclass::prelude::ObjectSubclassIsExt;
        clear_focus_before_push(self);
        crate::vibrations::push_vibrations_chooser(
            &self.imp().nav_view,
            app,
            current_uuid,
            on_selected,
        );
    }

    /// Push the label chooser onto the navigation view. Triggered
    /// by both label-selection rows (Setup view + Done view). The
    /// caller's `on_selected` callback fires when the user taps a
    /// label row, receives the chosen `Label`, and the page pops
    /// automatically.
    pub fn push_label_chooser(
        &self,
        app: &crate::application::MeditateApplication,
        current_label_id: Option<i64>,
        on_selected: impl Fn(crate::db::Label) + 'static,
    ) {
        use glib::subclass::prelude::ObjectSubclassIsExt;
        crate::labels::push_labels_chooser(
            &self.imp().nav_view,
            app,
            current_label_id,
            on_selected,
        );
    }

    /// Push the preset chooser. Save mode carries the live Setup
    /// snapshot to write into the chosen preset; Manage mode is
    /// view + rename + delete + star toggle. `on_changed` is called
    /// after any DB write so the home-view starred-list refreshes
    /// without the user having to leave + return.
    pub fn push_presets_chooser(
        &self,
        app: &crate::application::MeditateApplication,
        mode: crate::db::SessionMode,
        chooser_mode: crate::presets::ChooserMode,
        on_changed: impl Fn() + 'static,
    ) {
        use glib::subclass::prelude::ObjectSubclassIsExt;
        crate::presets::push_presets_chooser(
            &self.imp().nav_view,
            app,
            mode,
            chooser_mode,
            on_changed,
        );
    }

    /// Push the guided-meditation Manage Files chooser. Triggered by
    /// the timer Setup view's Manage Files button under Guided mode.
    /// `on_changed` fires after every DB write inside the chooser
    /// (rename / delete / star toggle / import) so the Setup view's
    /// starred-files list refreshes without requiring a back-and-
    /// forth navigation.
    pub fn push_guided_files_chooser(
        &self,
        app: &crate::application::MeditateApplication,
        on_changed: impl Fn() + Clone + 'static,
    ) {
        use glib::subclass::prelude::ObjectSubclassIsExt;
        crate::guided::push_guided_files_chooser(
            &self.imp().nav_view,
            app,
            on_changed,
        );
    }
}

/// Drop the window's currently-focused widget before pushing a
/// chooser onto the nav view. Otherwise GtkScrolledWindow's built-in
/// scroll-into-view-on-focus-change behaviour will, on the chooser's
/// pop, scroll the previously-focused row back into view — and when
/// that row is wrapped in a Gtk.Revealer (the End Bell / Starting
/// Bell signal-mode revealers), the calculation shifts the page
/// downward by a few px every time. Clearing focus before the push
/// removes the focused-target the scrolled window would otherwise
/// chase. Keyboard navigation isn't affected; the user's next Tab
/// re-establishes focus from scratch.
fn clear_focus_before_push(window: &MeditateWindow) {
    use gtk::prelude::GtkWindowExt;
    window.set_focus(None::<&gtk::Widget>);
}
