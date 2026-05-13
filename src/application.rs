mod imp {
    use adw::prelude::*;
    use adw::subclass::prelude::*;
    use gtk::{gdk, gio, glib};
    use std::path::PathBuf;
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

        /// Whether the device exposes feedbackd's `Haptic` interface
        /// (i.e. has a vibration motor and feedbackd reachable on the
        /// session bus). Set once at startup by the synchronous probe;
        /// UI consumers read it via `app.has_haptic()` to gate
        /// vibration affordances. `false` until the probe runs.
        pub has_haptic: std::cell::Cell<bool>,

        /// Path to the SQLite file. Cached here so the sync worker
        /// thread can open its OWN connection (rusqlite::Connection
        /// is !Send so the main-thread DB can't be shared). `None`
        /// before `startup` runs.
        pub db_path: Mutex<Option<PathBuf>>,

        /// At-most-one-sync coordination. `request()` returns the
        /// action the trigger should take; `start_pass` /
        /// `should_run_again_after_pass` drive the worker's drain
        /// loop; `release` frees the in-flight slot on exit. The
        /// AtomicBool choreography (and the ordering invariants that
        /// keep a re-trigger from being lost across a pass boundary)
        /// live in core.
        pub sync_coordinator: Arc<meditate_core::sync::coordinator::SyncCoordinator>,

        /// Set by `startup` if `Database::open` failed. `activate`
        /// reads it to present an error window instead of the main
        /// window — without this, the user lands on an inert UI
        /// whose every action silently no-ops.
        pub last_open_error: Mutex<Option<meditate_core::format::DbOpenFailureKey>>,

        /// Set by `startup` when `finalize_session_in_progress` rescued
        /// an in-flight session left behind by a crash / OOM / battery
        /// death. `activate` reads it once the window exists and
        /// surfaces an Undo toast — startup itself can't toast because
        /// no window exists yet.
        pub pending_recovery_toast: Mutex<Option<meditate_core::db::FinalizedSession>>,
    }

    impl Default for MeditateApplication {
        fn default() -> Self {
            Self {
                db: Arc::default(),
                stats_dirty: std::cell::Cell::new(true),
                log_dirty:   std::cell::Cell::new(true),
                has_haptic: std::cell::Cell::new(false),
                db_path: Mutex::new(None),
                last_open_error: Mutex::new(None),
                pending_recovery_toast: Mutex::new(None),
                sync_coordinator: Arc::new(
                    meditate_core::sync::coordinator::SyncCoordinator::new(),
                ),
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

            // If the DB couldn't be opened in startup, show a recovery
            // window instead of the main UI. Without this the user
            // lands on an inert MeditateWindow whose every action
            // silently no-ops because `with_db*` returns None.
            if let Some(key) = self.last_open_error.lock().unwrap().clone() {
                if app.active_window().is_none() {
                    present_db_open_error_window(&app, &key);
                }
                return;
            }

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

            // If startup's finalize_session_in_progress rescued a
            // crash-leftover session, surface the Undo toast now that
            // the window exists. The session is already in the log
            // (and in pending sync events); the toast just tells the
            // user it happened and gives them a one-tap undo.
            self.present_recovery_toast_if_pending();
        }

        fn startup(&self) {
            self.parent_startup();

            // Open (or create) the SQLite database in the user data directory.
            let db_path = glib::user_data_dir()
                .join("meditate")
                .join("meditate.db");
            match Database::open(&db_path) {
                Ok(db) => {
                    // Crash-recovery finalize. A session-in-progress
                    // row left behind by a kernel OOM / battery-death
                    // / panic mid-session becomes one session_insert
                    // event here, so the work the user already did
                    // shows up in the log as soon as activate runs.
                    // The Undo toast happens later in activate
                    // (startup is too early — no window exists yet).
                    match db.finalize_session_in_progress() {
                        Ok(Some(finalized)) => {
                            meditate_core::log(&format!(
                                "session_recovery: finalised in-flight session uuid={} duration_secs={}",
                                finalized.session_uuid, finalized.duration_secs,
                            ));
                            *self.pending_recovery_toast.lock().unwrap() = Some(finalized);
                        }
                        Ok(None) => {}
                        Err(e) => {
                            meditate_core::log(&format!(
                                "session_recovery: finalize FAILED at startup: {e}"
                            ));
                        }
                    }
                    *self.db.lock().unwrap() = Some(db);
                    meditate_core::log(&format!("db open ok: {}", db_path.display()));
                }
                Err(e) => {
                    let key = meditate_core::format::db_open_failure_key(&e);
                    eprintln!("Failed to open database: {e:?}");
                    meditate_core::log(&format!(
                        "db open FAILED at {}: {e:?}", db_path.display()
                    ));
                    *self.last_open_error.lock().unwrap() = Some(key);
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

            // Probe whether feedbackd's Haptic interface is reachable
            // before any UI assembles. Synchronous so app.has_haptic()
            // returns the true answer the first time anything reads
            // it. Worst-case 500 ms; typical <50 ms.
            let has_haptic = crate::vibration::probe_haptic();
            self.has_haptic.set(has_haptic);
            meditate_core::log(&format!("haptic probe: {}", has_haptic));

            self.setup_actions();
            self.setup_accels();
        }
    }

    impl GtkApplicationImpl for MeditateApplication {}
    impl AdwApplicationImpl for MeditateApplication {}

    /// Present a recovery window for the case where `Database::open`
    /// failed at startup. Renders an AdwStatusPage with mode-specific
    /// copy plus an "Open Data Folder" affordance and a "Quit" button.
    fn present_db_open_error_window(
        app: &super::MeditateApplication,
        key: &meditate_core::format::DbOpenFailureKey,
    ) {
        use meditate_core::format::DbOpenFailureKey;
        let (title_txt, body_txt) = match key {
            DbOpenFailureKey::SchemaTooNew { db, build } => (
                crate::i18n::gettext("Database is newer than this app"),
                crate::i18n::gettext(
                    "The local database (version {db}) was written by a \
                     newer build than this one (version {build}). Install \
                     a matching version, or move the file aside to start \
                     fresh."
                )
                .replace("{db}", &db.to_string())
                .replace("{build}", &build.to_string()),
            ),
            DbOpenFailureKey::Other => (
                crate::i18n::gettext("Couldn't open database"),
                crate::i18n::gettext(
                    "An unexpected error prevented opening the local \
                     database. Check the diagnostics log in About → \
                     Troubleshooting for details."
                ),
            ),
        };

        let status = adw::StatusPage::builder()
            .icon_name("dialog-error-symbolic")
            .title(&title_txt)
            .description(&body_txt)
            .build();

        let buttons = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .halign(gtk::Align::Center)
            .build();

        let open_folder = gtk::Button::builder()
            .label(crate::i18n::gettext("Open Data Folder"))
            .build();
        open_folder.connect_clicked(|_| {
            let dir = glib::user_data_dir().join("meditate");
            let _ = std::process::Command::new("xdg-open").arg(&dir).spawn();
        });

        let quit_btn = gtk::Button::builder()
            .label(crate::i18n::gettext("Quit"))
            .css_classes(["suggested-action"])
            .build();
        let app_for_quit = app.clone();
        quit_btn.connect_clicked(move |_| app_for_quit.quit());

        buttons.append(&open_folder);
        buttons.append(&quit_btn);
        status.set_child(Some(&buttons));

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title(crate::i18n::gettext("Meditate"))
            .default_width(480)
            .default_height(380)
            .content(&status)
            .build();
        window.present();
    }

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
                        .debug_info(meditate_core::diag::read_all())
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

        /// If startup's `finalize_session_in_progress` rescued an
        /// in-flight session, render an Undo toast on the active
        /// window. Single-shot: takes the stashed FinalizedSession
        /// out, so a subsequent re-activate doesn't re-toast the same
        /// recovery.
        ///
        /// The recovered row already lives in `sessions` (and an
        /// authored `session_insert` event is in the pending queue)
        /// — the toast just narrates that to the user and offers a
        /// one-tap undo. Tapping Undo calls `delete_session_by_uuid`,
        /// which emits a tombstoning `session_delete` event so peers
        /// converge on the deletion too.
        fn present_recovery_toast_if_pending(&self) {
            let Some(finalized) = self.pending_recovery_toast.lock().unwrap().take()
            else { return; };
            let app = self.obj();
            let Some(win) = app
                .active_window()
                .and_then(|w| w.downcast::<MeditateWindow>().ok())
            else {
                // No window yet (shouldn't happen since we're called
                // from activate after present, but defensive). Put the
                // FinalizedSession back so the next activate gets it.
                *self.pending_recovery_toast.lock().unwrap() = Some(finalized);
                return;
            };

            // Refresh the log view + stats so the recovered session
            // shows up immediately. invalidate-on-stats was already
            // done implicitly via the event log mutation, but the
            // log feed needs an explicit prepend.
            app.invalidate(crate::application::InvalidateScope::ALL);

            let minutes = finalized.duration_secs / 60;
            let title = crate::i18n::gettext(
                "Recovered {n} min session"
            ).replace("{n}", &minutes.to_string());

            let toast = adw::Toast::builder()
                .title(&title)
                .button_label(&crate::i18n::gettext("Undo"))
                .timeout(8)
                .build();

            let app_for_undo = app.clone();
            let session_uuid = finalized.session_uuid.clone();
            toast.connect_button_clicked(move |_| {
                app_for_undo.with_db_mut(|db| {
                    if let Err(e) = db.delete_session_by_uuid(&session_uuid) {
                        meditate_core::log(&format!(
                            "session_recovery: Undo delete failed for uuid={session_uuid}: {e}"
                        ));
                    }
                });
                app_for_undo.invalidate(crate::application::InvalidateScope::ALL);
            });
            win.add_toast(toast);
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
    /// Whether the device exposes feedbackd's `Haptic` interface (set
    /// by the synchronous probe in `startup`). UI consumers gate
    /// vibration affordances on this — Vibrate / Both segments of
    /// Sound/Vibration/Both ToggleGroups go insensitive when false,
    /// the per-mode "what plays" toggle is forced to `'sound'`,
    /// per-bell vibration playback short-circuits to a no-op.
    /// `false` until `startup` finishes (probe runs synchronously
    /// before any UI assembles, so any read after that point sees
    /// the real answer).
    pub fn has_haptic(&self) -> bool {
        use glib::subclass::prelude::ObjectSubclassIsExt;
        self.imp().has_haptic.get()
    }

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

    /// True while a sync attempt is in flight on a worker thread. Used
    /// by the headerbar status indicator so it can show a spinner /
    /// syncing icon during the run.
    pub fn is_syncing(&self) -> bool {
        use glib::subclass::prelude::ObjectSubclassIsExt;
        self.imp().sync_coordinator.is_in_flight()
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
        use meditate_core::sync::coordinator::CoordinatorAction;

        // Fast-path: if sync isn't set up, skip everything below.
        // Saves spawning a worker (and pulling in the keychain D-Bus
        // round-trip) just to find out we have no account configured.
        let configured = self
            .with_db(|db| {
                meditate_core::sync::settings::nextcloud_account_from_db(db.core())
                    .map(|opt| opt.is_some())
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        if !configured {
            return;
        }

        let imp = self.imp();
        // Core coordinator handles the at-most-one-in-flight
        // choreography. Caller-side: if we don't get the Spawn
        // action, another worker is running and will see the
        // re-trigger we just set.
        match imp.sync_coordinator.request() {
            CoordinatorAction::Spawn => {}
            CoordinatorAction::AlreadyRunning => return,
        }

        let Some(db_path) = imp.db_path.lock().unwrap().clone() else {
            // No DB path → startup never ran or failed; release the
            // slot we just took and bail.
            imp.sync_coordinator.abort();
            return;
        };

        let coord = Arc::clone(&imp.sync_coordinator);

        std::thread::spawn(move || {
            // Run sync attempts in a loop while the re-trigger flag
            // is set. The coordinator's `start_pass` clears the flag
            // BEFORE each pass so a trigger arriving during the pass
            // survives to schedule another.
            loop {
                coord.start_pass();
                let result = crate::sync_runner::run_sync_attempt(&db_path);
                if let Err(e) = &result {
                    meditate_core::log(&format!("sync: {e}"));
                }
                if !coord.should_run_again_after_pass() {
                    break;
                }
            }
            // Release the in-flight slot before we hop back to the
            // main loop, so a trigger arriving on the main thread
            // *during* the invoke can spawn a fresh worker if needed.
            coord.release();

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
