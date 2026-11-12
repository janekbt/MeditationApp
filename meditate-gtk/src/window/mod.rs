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

    /// Where a toast with no page of its own goes: the topmost open
    /// dialog with an overlay (Preferences, the Log editor), else the
    /// visible page's, else the main page's.
    pub fn overlay_in_view(&self) -> adw::ToastOverlay {
        use adw::prelude::*;
        use glib::subclass::prelude::ObjectSubclassIsExt;
        let dialogs = self.dialogs();
        let from_dialog = (0..dialogs.n_items()).rev()
            .filter_map(|i| dialogs.item(i).and_downcast::<gtk::Widget>())
            .find_map(|d| crate::db::first_overlay(&d));
        from_dialog
            .or_else(|| crate::db::first_overlay(self.imp().nav_view.upcast_ref()))
            .unwrap_or_else(|| self.imp().toast_overlay.get())
    }

    pub fn dismiss_toasts(&self) {
        use glib::subclass::prelude::ObjectSubclassIsExt;
        self.imp().toast_overlay.dismiss_all();
    }

    /// Push the interval-bell library page onto the navigation view.
    /// Triggered by the "Interval Bells" row in the timer setup; this
    /// wrapper keeps the timer module from having to know about the
    /// window's internal layout. The on_changed callback runs after
    /// any add / toggle inside the library so the timer's count
    /// subtitle stays in sync without the user having to leave + return.
    pub fn push_bells_page(&self, app: &crate::application::MeditateApplication) {
        use glib::subclass::prelude::ObjectSubclassIsExt;
        // A second tap during the push: the page is already there.
        if self.imp().nav_view.find_page("interval-bells").is_some() {
            return;
        }
        let timer_view = self.imp().timer_view.clone();
        let rebuild = crate::bells::push_bells_page(
            &self.imp().nav_view,
            app,
            &timer_view.build_start_bar(),
            move || timer_view.refresh_interval_bells_count(),
        );
        // Its start bar can change the length, which greys or frees bells.
        self.imp().timer_view.imp().manage_bells.replace(std::rc::Rc::downgrade(&rebuild));
    }

    /// Push one of the timer's settings pages (Session, Bells).
    pub fn push_page(&self, page: &adw::NavigationPage) {
        use glib::subclass::prelude::ObjectSubclassIsExt;
        self.imp().nav_view.push(page);
    }

    pub fn timer_view(&self) -> crate::timer::TimerView {
        use glib::subclass::prelude::ObjectSubclassIsExt;
        self.imp().timer_view.get()
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
        volume: meditate_core::bell_volume::BellVolume,
        on_selected: impl Fn(String) + 'static,
    ) {
        use glib::subclass::prelude::ObjectSubclassIsExt;
        clear_focus_before_push(self);
        crate::sounds::push_sounds_chooser(
            &self.imp().nav_view,
            app,
            category,
            current_uuid,
            volume,
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
    /// view + rename + delete + star toggle.
    pub fn push_presets_chooser(
        &self,
        app: &crate::application::MeditateApplication,
        mode: crate::db::SessionMode,
        chooser_mode: crate::presets::ChooserMode,
    ) {
        use glib::subclass::prelude::ObjectSubclassIsExt;
        crate::presets::push_presets_chooser(
            &self.imp().nav_view,
            app,
            mode,
            chooser_mode,
        );
    }

    /// Push the Guided Local Folder page; `on_pick` gets the tapped file.
    pub fn push_local_folder_page(
        &self,
        app: &crate::application::MeditateApplication,
        on_pick: std::rc::Rc<dyn Fn(crate::guided::GuidedFilePick)>,
    ) {
        use glib::subclass::prelude::ObjectSubclassIsExt;
        clear_focus_before_push(self);
        crate::local_folder::push_page(&self.imp().nav_view, app, on_pick);
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

/// A click or tap into a number field selects its number, as Tab
/// already does, so typing replaces it and leaving without typing
/// keeps it.
fn select_number_on_click(window: &impl IsA<gtk::Widget>) {
    use gtk::{gdk, prelude::*};
    // The field pressed, until its release.
    let pressed = std::cell::Cell::new(None);
    let events = gtk::EventControllerLegacy::new();
    events.set_name(Some("select-number"));
    // Capture: before the field's own press and release.
    events.set_propagation_phase(gtk::PropagationPhase::Capture);
    events.connect_event(move |events, event| {
        match event.event_type() {
            gdk::EventType::ButtonPress | gdk::EventType::TouchBegin => pressed.set(number_under(events, event)),
            gdk::EventType::ButtonRelease | gdk::EventType::TouchEnd => {
                if let Some(text) = pressed.take() { select_after_release(text); }
            }
            gdk::EventType::TouchCancel | gdk::EventType::GrabBroken => pressed.set(None),
            _ => {}
        }
        glib::Propagation::Proceed
    });
    window.add_controller(events);
}

/// The number field a plain primary press lands on, picked in the
/// whole window (the controller may sit on a dialog inside it), through
/// `number_pressed`. Shift+click extends a selection: GTK's.
fn number_under(events: &gtk::EventControllerLegacy, event: &gtk::gdk::Event) -> Option<gtk::Text> {
    use gtk::{gdk, prelude::*};
    let primary = event.downcast_ref::<gdk::ButtonEvent>().map_or(true, |b| b.button() == gdk::BUTTON_PRIMARY);
    if !primary || event.modifier_state().contains(gdk::ModifierType::SHIFT_MASK) { return None; }
    let (x, y) = event.position()?;
    let native = events.widget()?.native()?;
    // A popover's press comes in its own surface's coordinates.
    if event.surface() != native.surface() { return None; }
    let (dx, dy) = native.surface_transform();
    number_pressed(native.upcast_ref(), x - dx, y - dy)
}

/// The number field's text under a press at `x`, `y`, if the click
/// is to select it: the field gets the focus, or its whole number is
/// already selected (Tab, a dialog's first field). Mid-edit a click
/// just moves the cursor. Its selection is emptied: a press inside a
/// selection would drag it, or on a touchscreen pop the copy bubble;
/// outside it GTK just places the cursor.
fn number_pressed(window: &gtk::Widget, x: f64, y: f64) -> Option<gtk::Text> {
    use gtk::prelude::*;
    let text = window.pick(x, y, gtk::PickFlags::DEFAULT).and_downcast::<gtk::Text>()?;
    text.ancestor(gtk::SpinButton::static_type())?;
    if text.is_focus() && text.selection_bounds() != Some((0, text.text().chars().count() as i32)) { return None; }
    text.select_region(0, 0);
    Some(text)
}

/// Select the whole number once GTK's own release is done: that
/// release brings the on-screen keyboard back only while nothing is
/// selected. A selection GTK made since the press is the user's drag
/// and stays.
fn select_after_release(text: gtk::Text) {
    use gtk::prelude::*;
    glib::idle_add_local_once(move || {
        if text.is_focus() && text.selection_bounds().is_none() { text.select_region(0, -1); }
    });
}

/// `select_number_on_click` on the window and on every dialog it
/// shows: a dialog keeps its events to itself (libadwaita sets
/// `limit-events`), so the window's handler never sees them.
fn select_number_on_click_with_dialogs(window: &adw::ApplicationWindow) {
    use adw::prelude::*;
    select_number_on_click(window);
    window.connect_visible_dialog_notify(|window| {
        let Some(dialog) = window.visible_dialog() else { return };
        let ours = dialog.observe_controllers().iter::<glib::Object>()
            .any(|c| c.ok().and_downcast::<gtk::EventController>().is_some_and(|c| c.name().as_deref() == Some("select-number")));
        if !ours { select_number_on_click(&dialog); }
    });
}

#[cfg(test)]
mod tests {
    use adw::prelude::*;

    #[test]
    fn every_dialog_selects_numbers_on_click() {
        crate::gtk_test::run(|| {
            let ours = |w: &gtk::Widget| w.observe_controllers().iter::<glib::Object>()
                .filter_map(|c| c.ok()?.downcast::<gtk::EventController>().ok())
                .filter(|c| c.name().as_deref() == Some("select-number"))
                .count();
            let window = adw::ApplicationWindow::builder().content(&gtk::Button::with_label("x")).build();
            super::select_number_on_click_with_dialogs(&window);
            window.present();
            let first = adw::AlertDialog::new(Some("a"), None);
            first.present(Some(&window));
            let second = adw::AlertDialog::new(Some("b"), None);
            second.present(Some(&window));
            let ctx = glib::MainContext::default();
            while ctx.iteration(false) {}
            assert_eq!(ours(window.upcast_ref()), 1, "the window");
            assert_eq!(ours(first.upcast_ref()), 1, "a dialog");
            assert_eq!(ours(second.upcast_ref()), 1, "a dialog over a dialog");
            second.force_close();
            while ctx.iteration(false) {}
            assert_eq!(ours(first.upcast_ref()), 1, "shown again, still one");
            first.force_close();
            window.destroy();
        });
    }

    /// The text a click lands in: a spin row's sits inside its spin button.
    fn text_of(field: &gtk::Widget) -> Option<gtk::Text> {
        std::iter::successors(field.first_child(), |c| c.next_sibling())
            .find_map(|c| c.clone().downcast::<gtk::Text>().ok().or_else(|| text_of(&c)))
    }

    #[test]
    fn a_clicked_number_field_selects_its_number() {
        crate::gtk_test::run(|| {
            let spin = gtk::SpinButton::with_range(0.0, 59.0, 1.0);
            spin.set_value(10.0);
            let row = adw::SpinRow::with_range(0.0, 59.0, 1.0);
            row.set_value(10.0);
            let other = gtk::Button::with_label("x");
            let entry = gtk::Entry::builder().text("name").build();
            let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
            column.append(&spin);
            column.append(&row);
            column.append(&other);
            column.append(&entry);
            let window = gtk::Window::builder().child(&column).build();
            window.present();
            // Laid out, so a press finds the field under the pointer.
            let ctx = glib::MainContext::default();
            for _ in 0..500 {
                if row.width() > 0 { break; }
                std::thread::sleep(std::time::Duration::from_millis(2));
                while ctx.iteration(false) {}
            }
            let widget = window.upcast_ref::<gtk::Widget>();
            for field in [spin.upcast_ref::<gtk::Widget>(), row.upcast_ref()] {
                let name = field.type_().name();
                let editable = field.downcast_ref::<gtk::Editable>().unwrap();
                let text = text_of(field).unwrap();
                let at = text.compute_point(widget, &gtk::graphene::Point::new(2.0, 2.0)).unwrap();
                let (x, y) = (f64::from(at.x()), f64::from(at.y()));
                // A click: ours first (capture), then GTK's press (focus
                // without selecting, the cursor where the pointer is),
                // GTK's release, then ours.
                let click = || {
                    let pressed = super::number_pressed(widget, x, y).unwrap();
                    assert_eq!(editable.selection_bounds(), None, "{name}: a press finds no selection to drag");
                    if !text.is_focus() { text.grab_focus_without_selecting(); }
                    text.set_position(1);
                    super::select_after_release(pressed);
                    assert_eq!(editable.selection_bounds(), None, "{name}: nothing selected before GTK's release is done");
                    while ctx.iteration(false) {}
                };
                // Into a field without the focus.
                other.grab_focus();
                click();
                assert_eq!(editable.selection_bounds(), Some((0, 2)), "{name} from elsewhere");
                // Into a field that has it with its number selected (Tab,
                // a dialog's first field).
                text.grab_focus();
                click();
                assert_eq!(editable.selection_bounds(), Some((0, 2)), "{name} already focused");
                // Mid-edit (focused, cursor placed): a click just moves the cursor.
                text.set_position(1);
                assert!(super::number_pressed(widget, x, y).is_none(), "{name} mid-edit");
                text.select_region(0, -1);
                // A drag over the first digit keeps what it selected.
                let pressed = super::number_pressed(widget, x, y).unwrap();
                text.select_region(0, 1);
                super::select_after_release(pressed);
                while ctx.iteration(false) {}
                assert_eq!(editable.selection_bounds(), Some((0, 1)), "{name} drag");
            }
            // A text field that is not a number is left alone.
            let entry_text = text_of(entry.upcast_ref()).unwrap();
            let at = entry_text.compute_point(widget, &gtk::graphene::Point::new(2.0, 2.0)).unwrap();
            let (x, y) = (f64::from(at.x()), f64::from(at.y()));
            assert_eq!(widget.pick(x, y, gtk::PickFlags::DEFAULT).as_ref(), Some(entry_text.upcast_ref()), "the press lands in the entry");
            assert!(super::number_pressed(widget, x, y).is_none());
            window.destroy();
        });
    }
}
