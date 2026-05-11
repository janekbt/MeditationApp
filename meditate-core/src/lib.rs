//! Portable Rust logic core for the Meditate app.
//!
//! Everything here is GTK-free — the gtk-rs shell and the future
//! Slint/Android shell both consume this crate to share the
//! decision logic, persistence, and sync that aren't UI-specific.
//! Anything that needs a `gtk::` type stays in the shell.
//!
//! ## Architecture in one paragraph
//!
//! State is event-sourced into a single SQLite DB (`db.rs`). Every
//! mutation appends a row to `events` carrying a Lamport timestamp
//! and the authoring device's UUID; `recompute_*` family materialises
//! the cache tables from the log. The sync layer (`sync/`) bulk-PUTs
//! pending events to a WebDAV remote and pulls peer batches back,
//! with deduplication keyed by `event_uuid`. The cache tables are a
//! pure function of the event log, so a peer can drop them and
//! rebuild from the log at any time.
//!
//! ## Conventions
//!
//! - **Decisions in core, mechanisms in the shell.** The shell glues
//!   GTK widgets together; behaviour ("when do we fire a bell, what
//!   does the streak read") lives here. See
//!   `feedback_meditate_decisions_in_core`.
//! - **Typed keys for translatable copy.** Helpers that previously
//!   returned formatted English strings now return a typed
//!   `*Key` enum; the shell maps each variant to `gettext`. See
//!   `feedback_meditate_i18n_typed_keys`.
//! - **`_from_db` suffix** marks helpers that touch the DB so
//!   call sites read like reads vs. computations.
//! - **`apply_event_inner` is forward-compatible.** Unknown event
//!   kinds are recorded but not dispatched; on the next cache-
//!   schema bump (see `CACHE_SCHEMA_VERSION`) all events are
//!   replayed so newly-understood kinds materialise.
//!
//! ## Module map
//!
//! - `db`            — SQLite cache, event log, apply/replay,
//!                     recompute_* dispatch. Owns `Database`.
//! - `sync/`         — WebDAV push/pull engine, settings, coordinator.
//! - `session`       — session-mode state machine (`Session`,
//!                     `Effect`, `TickOutcome`).
//! - `bells`         — interval / starting / end bell scheduling.
//! - `breath`        — box-breath phases and perimeter math.
//! - `vibration`     — vibration pattern editor + envelope helpers.
//! - `format`        — translatable typed keys + plain formatters
//!                     (durations, counters, mini-stats).
//! - `goal`          — weekly-goal snapshot logic.
//! - `insights`      — derived stats (week-over-week, milestones).
//! - `contrib`       — contribution-heatmap data model.
//! - `preset_config` — JSON-encoded preset payload (Timer / BoxBreath /
//!                     Guided).
//! - `time`          — `boot_time_now` (suspend-resilient) +
//!                     ISO-to-unix helpers.
//! - `diag`          — ring-buffer log to `<data>/diagnostics.log`.
//! - `data_io`       — CSV import / export of sessions.
//! - `seeds`         — bundled vibration patterns + default presets.
//! - `labels`, `naming`, `paths`, `rng`, `settings_keys`, `sound`,
//!   `timer`, `date_math` — supporting utilities.

pub mod bells;
pub mod breath;
pub mod contrib;
pub mod data_io;
pub mod date_math;
pub mod db;
pub mod diag;
pub mod format;
pub mod goal;
pub mod insights;
pub mod labels;
pub mod naming;
pub mod paths;
pub mod preset_config;
pub mod rng;
pub mod seeds;
pub mod session;
pub mod sound;
pub mod settings_keys;
pub mod sync;
pub mod time;
pub mod timer;
pub mod vibration;
