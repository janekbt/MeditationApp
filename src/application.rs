mod imp {
    use adw::prelude::*;
    use adw::subclass::prelude::*;
    use gtk::{gdk, gio, glib};
    use std::path::PathBuf;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};

    use crate::config;
    use crate::db::Database;
    use crate::window::MeditateWindow;

    // The Database lives behind an Arc<Mutex<_>> so it can be accessed
    // either from the GTK main thread (cheap sync `with_db`) or from the
    // GIO blocking pool (async `with_db_blocking` for writes on eMMC).
    // Main-thread contention is effectively zero — the only concurrent
    // access is when a worker task holds the lock during a write.
    #[derive(Debug)]
    pub struct MeditateApplication {
        pub db: Arc<Mutex<Option<Database>>>,
        // Dirty flags consumed by StatsView::refresh / LogView::refresh so
        // the aggregations re-run only when data actually changed — not on
        // every tab switch. Start `true` so the first show populates them.
        pub stats_dirty: std::cell::Cell<bool>,
        pub log_dirty:   std::cell::Cell<bool>,

        /// Path to the SQLite file. Cached here so the sync worker
        /// thread can open its OWN connection (rusqlite::Connection
        /// is !Send so the main-thread DB can't be shared). `None`
        /// before `startup` runs.
        pub db_path: Mutex<Option<PathBuf>>,

        /// True while a sync attempt is running. Triggers that arrive
        /// during this window set `sync_re_trigger` instead of
        /// spawning a second worker — at most one sync runs at a time.
        pub sync_in_flight: Arc<AtomicBool>,

        /// Set by `trigger_sync` when a sync is already in flight; the
        /// worker checks this on completion and runs another pass if
        /// it's true. Bulk-mutation flurries (the user deleting 10
        /// sessions in a row) trigger one sync, queue a re-trigger,
        /// and end with one follow-up sync that captures everything
        /// that arrived during the first.
        pub sync_re_trigger: Arc<AtomicBool>,
    }

    impl Default for MeditateApplication {
        fn default() -> Self {
            Self {
                db: Arc::default(),
                stats_dirty: std::cell::Cell::new(true),
                log_dirty:   std::cell::Cell::new(true),
                db_path: Mutex::new(None),
                sync_in_flight: Arc::new(AtomicBool::new(false)),
                sync_re_trigger: Arc::new(AtomicBool::new(false)),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MeditateApplication {
        const NAME: &'static str = "MeditateApplication";
        type Type = super::MeditateApplication;
        type ParentType = adw::Application;
    }

    impl ObjectImpl for MeditateApplication {}

    impl ApplicationImpl for MeditateApplication {
        fn activate(&self) {
            self.parent_activate();
            let app = self.obj();

            if let Some(window) = app.active_window() {
                window.present();
                // Re-activation (user clicked the launcher again with
                // the app already running) is a "user opened the app"
                // signal — pull anything new from Nextcloud in the
                // background.
                app.trigger_sync();
                return;
            }

            MeditateWindow::new(&*app).present();
            // First activation after startup: pull whatever a peer
            // device authored while we were closed.
            app.trigger_sync();
        }

        fn startup(&self) {
            self.parent_startup();

            // Open (or create) the SQLite database in the user data directory.
            let db_path = glib::user_data_dir()
                .join("meditate")
                .join("meditate.db");
            match Database::open(&db_path) {
                Ok(db) => {
                    *self.db.lock().unwrap() = Some(db);
                    crate::diag::log(&format!("db open ok: {}", db_path.display()));
                }
                Err(e) => {
                    eprintln!("Failed to open database: {e}");
                    crate::diag::log(&format!("db open FAILED at {}: {e}", db_path.display()));
                }
            }
            // Cache the path so the sync worker thread can open its own
            // connection later. We do this even if the open above failed —
            // a successful retry by the worker would be a nice surprise,
            // and there's no harm in handing it the path either way.
            *self.db_path.lock().unwrap() = Some(db_path);

            // Register the bundled app icon so the About dialog and GNOME Shell
            // can find it in development builds (installed builds use the
            // hicolor theme path; GResource acts as a fallback).
            gtk::IconTheme::for_display(&gdk::Display::default().expect("No display"))
                .add_resource_path("/io/github/janekbt/Meditate/icons");

            // Load application CSS (chart bar styles, etc.)
            let provider = gtk::CssProvider::new();
            provider.load_from_resource("/io/github/janekbt/Meditate/style.css");
            #[allow(deprecated)]
            gtk::style_context_add_provider_for_display(
                &gdk::Display::default().expect("No display"),
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );

            self.setup_actions();
            self.setup_accels();
        }
    }

    impl GtkApplicationImpl for MeditateApplication {}
    impl AdwApplicationImpl for MeditateApplication {}

    impl MeditateApplication {
        fn setup_actions(&self) {
            let app = self.obj();

            // app.preferences — opens AdwPreferencesWindow (Phase 6)
            let preferences_action = gio::SimpleAction::new("preferences", None);
            preferences_action.connect_activate(glib::clone!(
                #[weak]
                app,
                move |_, _| {
                    crate::preferences::show_preferences(&app);
                }
            ));
            app.add_action(&preferences_action);

            // app.about
            let about_action = gio::SimpleAction::new("about", None);
            about_action.connect_activate(glib::clone!(
                #[weak]
                app,
                move |_, _| {
                    use crate::i18n::gettext;
                    // Keep this mirror in sync with the first <release> in
                    // data/io.github.janekbt.Meditate.metainfo.xml.in on
                    // every version bump — the metainfo is the source of
                    // truth for release-notes copy (Flathub uses it), this
                    // is the in-app echo for the About dialog.
                    let notes = format!(
                        "<p>{}</p><ul><li>{}</li><li>{}</li><li>{}</li><li>{}</li><li>{}</li><li>{}</li></ul>",
                        gettext("A new Box Breath timer mode, per-label stats, and an in-app diagnostics view for bug reporting — plus a handful of polish fixes."),
                        gettext("New Box Breath mode alongside Countdown and Stopwatch: pick a pattern (4-4-4-4, 4-7-8-0, 5-5-5-5) or dial in each phase, set a session length, and the running view traces a dot around an accent-tinted square as you breathe in, hold, out, hold"),
                        gettext("Stats tab shows a \"By label\" breakdown below the streak / total / sessions row, so you can see how much meditation time you've spent under each label"),
                        gettext("About dialog has a new Troubleshooting view with a rolling diagnostics log (startup, database open, import/export results, panics) — copy or save to attach to a bug report"),
                        gettext("Each timer mode remembers its own last-chosen label, so a Box Breath session no longer changes the default label for Countdown or Stopwatch"),
                        gettext("Release notes shown in the About dialog now reflect the current version instead of staying pinned to the initial release"),
                        gettext("Log-edit calendar picker shows the proper icon again; post-session note field no longer steals focus on phones and hides the Save / Discard buttons behind the on-screen keyboard"),
                    );
                    let dialog = adw::AboutDialog::builder()
                        .application_name("Meditate")
                        .application_icon(config::APP_ID)
                        .version(config::VERSION)
                        .developer_name("Janek")
                        .website("https://github.com/janekbt/MeditationApp")
                        .issue_url("https://github.com/janekbt/MeditationApp/issues")
                        .license_type(gtk::License::Gpl30)
                        .release_notes_version(config::VERSION)
                        .release_notes(&notes)
                        // Debug Info view in AdwAboutDialog has built-in
                        // Copy + Save buttons, so wiring the diag log here
                        // gives us the "Copy diagnostics" UX for free.
                        .debug_info(crate::diag::read_all())
                        .debug_info_filename("meditate-diagnostics.log")
                        .build();

                    dialog.present(app.active_window().as_ref());
                }
            ));
            app.add_action(&about_action);

            // app.quit — HIG-standard Ctrl+Q action. Without this the
            // accel below mapped to a non-existent action (silent no-op).
            let quit_action = gio::SimpleAction::new("quit", None);
            quit_action.connect_activate(glib::clone!(
                #[weak] app,
                move |_, _| app.quit()
            ));
            app.add_action(&quit_action);
        }

        fn setup_accels(&self) {
            let app = self.obj();
            app.set_accels_for_action("app.preferences", &["<Control>comma"]);
            app.set_accels_for_action("win.show-help-overlay", &["<Control>question"]);
            // Per HIG these are two distinct actions: Ctrl+W closes the
            // current window, Ctrl+Q quits the whole application.
            app.set_accels_for_action("app.quit", &["<Control>q"]);
            app.set_accels_for_action("win.close", &["<Control>w"]);
            app.set_accels_for_action("win.timer-toggle", &["space"]);
        }
    }
}

use gtk::glib;

use adw::prelude::*;
use gtk::gio;
use std::sync::Arc;

glib::wrapper! {
    pub struct MeditateApplication(ObjectSubclass<imp::MeditateApplication>)
        @extends adw::Application, gtk::Application, gtk::gio::Application,
        @implements gtk::gio::ActionGroup, gtk::gio::ActionMap;
}

impl MeditateApplication {
    pub fn new() -> Self {
        glib::Object::builder()
            .property("application-id", crate::config::APP_ID)
            .property("flags", gtk::gio::ApplicationFlags::FLAGS_NONE)
            .build()
    }
}

impl Default for MeditateApplication {
    fn default() -> Self {
        Self::new()
    }
}

impl MeditateApplication {
    /// Run a closure with a reference to the open database, on the current
    /// thread. Holds the DB mutex for the duration of the closure, so keep
    /// the work short — SQLite PRAGMAs tune this for single-writer use.
    /// Returns `None` if the database failed to open at startup.
    pub fn with_db<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&crate::db::Database) -> R,
    {
        use glib::subclass::prelude::ObjectSubclassIsExt;
        let guard = self.imp().db.lock().unwrap();
        guard.as_ref().map(f)
    }

    /// Run a DB operation on the GIO blocking thread pool. Use for writes
    /// (fsync-heavy on eMMC) so the main thread keeps servicing frames.
    /// Returns a future that resolves with the closure's return value —
    /// awaiting this from the main thread never blocks the frame clock.
    pub async fn with_db_blocking<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&crate::db::Database) -> R + Send + 'static,
        R: Send + 'static,
    {
        use glib::subclass::prelude::ObjectSubclassIsExt;
        let db_arc = std::sync::Arc::clone(&self.imp().db);
        gtk::gio::spawn_blocking(move || {
            let guard = db_arc.lock().unwrap();
            guard.as_ref().map(f)
        })
        .await
        .ok()
        .flatten()
    }

    /// Mark aggregated views stale after a data mutation. Callers pick
    /// which views are affected so we avoid redundant full refreshes —
    /// session-save updates the log incrementally and only dirties stats;
    /// a bulk delete dirties everything.
    pub fn invalidate(&self, scope: InvalidateScope) {
        use glib::subclass::prelude::ObjectSubclassIsExt;
        let imp = self.imp();
        if scope.stats { imp.stats_dirty.set(true); }
        if scope.log   { imp.log_dirty.set(true); }
    }

    /// Whether the stats view needs to re-run its aggregations; cleared
    /// by `StatsView::refresh` after the work completes.
    pub fn stats_dirty(&self) -> bool {
        use glib::subclass::prelude::ObjectSubclassIsExt;
        self.imp().stats_dirty.get()
    }

    pub fn clear_stats_dirty(&self) {
        use glib::subclass::prelude::ObjectSubclassIsExt;
        self.imp().stats_dirty.set(false);
    }

    pub fn log_dirty(&self) -> bool {
        use glib::subclass::prelude::ObjectSubclassIsExt;
        self.imp().log_dirty.get()
    }

    pub fn clear_log_dirty(&self) {
        use glib::subclass::prelude::ObjectSubclassIsExt;
        self.imp().log_dirty.set(false);
    }

    /// `with_db` + a follow-up `trigger_sync()`. Use when the closure
    /// MUTATES the database — the trigger pushes the new event(s) to
    /// Nextcloud (when configured) without callers having to remember
    /// the separate trigger call. Read-only closures should keep
    /// using plain `with_db`.
    pub fn with_db_mut<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&crate::db::Database) -> R,
    {
        let result = self.with_db(f);
        self.trigger_sync();
        result
    }

    /// Async variant of `with_db_mut`. Triggers AFTER the blocking
    /// write finishes — calling `trigger_sync` before the await would
    /// race the worker against the writer for the SQLite file.
    pub async fn with_db_blocking_mut<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&crate::db::Database) -> R + Send + 'static,
        R: Send + 'static,
    {
        let result = self.with_db_blocking(f).await;
        self.trigger_sync();
        result
    }

    /// Spawn a Nextcloud sync attempt on a worker thread. Returns
    /// immediately. While a sync is in flight, additional triggers
    /// just set the re-trigger flag — the running worker spots it on
    /// completion and runs another pass. This collapses bursts of
    /// rapid mutations (bulk delete, log import) into at most two
    /// sync rounds total.
    ///
    /// On completion the worker schedules a callback on the GTK main
    /// loop that invalidates UI state and re-fires if the dirty flag
    /// is set. Errors are recorded to `sync_state` so the (future)
    /// status indicator can surface them; callers see fire-and-forget
    /// semantics here.
    pub fn trigger_sync(&self) {
        use glib::subclass::prelude::ObjectSubclassIsExt;
        use std::sync::atomic::Ordering;

        // Fast-path: if sync isn't set up, skip everything below.
        // Saves spawning a worker (and pulling in the keychain D-Bus
        // round-trip) just to find out we have no account configured.
        let configured = self
            .with_db(|db| {
                crate::sync_settings::get_nextcloud_account(db)
                    .map(|opt| opt.is_some())
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        if !configured {
            return;
        }

        let imp = self.imp();
        // Mark the re-trigger flag first so a sync that finishes
        // RIGHT NOW (before our `swap` below) still picks us up via
        // the worker's completion check.
        imp.sync_re_trigger.store(true, Ordering::SeqCst);

        // Try to take the in-flight slot. If someone else already has
        // it, we're done — they'll see our re-trigger and re-fire.
        if imp.sync_in_flight.swap(true, Ordering::SeqCst) {
            return;
        }

        let Some(db_path) = imp.db_path.lock().unwrap().clone() else {
            // No DB path → startup never ran or failed; clear the flag
            // we just took and bail.
            imp.sync_in_flight.store(false, Ordering::SeqCst);
            return;
        };

        let in_flight = Arc::clone(&imp.sync_in_flight);
        let re_trigger = Arc::clone(&imp.sync_re_trigger);

        std::thread::spawn(move || {
            // Run sync attempts in a loop while the re-trigger flag
            // is set. Clearing it BEFORE each pass means a trigger
            // arriving during the pass survives to schedule another.
            loop {
                re_trigger.store(false, Ordering::SeqCst);
                let result = crate::sync_runner::run_sync_attempt(&db_path);
                if let Err(e) = &result {
                    crate::diag::log(&format!("sync: {e}"));
                }
                if !re_trigger.load(Ordering::SeqCst) {
                    break;
                }
            }
            // Release the in-flight slot before we hop back to the
            // main loop, so a trigger arriving on the main thread
            // *during* the invoke can spawn a fresh worker if needed.
            in_flight.store(false, Ordering::SeqCst);

            // Hop back to the GTK main loop to refresh UI. The closure
            // is Send (captures nothing); we look the application up
            // via the gio default registry on the main thread, which
            // avoids having to send a !Send GObject across.
            glib::MainContext::default().invoke(|| {
                if let Some(app) = gio::Application::default()
                    .and_then(|a| a.downcast::<crate::application::MeditateApplication>().ok())
                {
                    app.invalidate(InvalidateScope::ALL);
                    // Force a redraw of the visible views so the user
                    // sees pulled changes immediately rather than on
                    // their next tab switch.
                    if let Some(win) = app.active_window()
                        .and_then(|w| w.downcast::<crate::window::MeditateWindow>().ok())
                    {
                        use glib::subclass::prelude::ObjectSubclassIsExt;
                        win.imp().timer_view.refresh_streak();
                        win.imp().stats_view.refresh();
                        win.imp().log_view.refresh();
                    }
                }
            });
        });
    }
}

/// Selector for `MeditateApplication::invalidate`. Callers construct an
/// instance via the helper constants — `InvalidateScope::ALL` after bulk
/// mutations, `InvalidateScope::STATS` after an incremental log update.
#[derive(Debug, Clone, Copy)]
pub struct InvalidateScope {
    pub stats: bool,
    pub log:   bool,
}

impl InvalidateScope {
    pub const ALL:   Self = Self { stats: true, log: true };
    pub const STATS: Self = Self { stats: true, log: false };
    pub const LOG:   Self = Self { stats: false, log: true };
}
