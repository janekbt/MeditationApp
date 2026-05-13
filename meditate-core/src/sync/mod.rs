//! Nextcloud sync for the append-only event log.
//!
//! Layered:
//! - `webdav` — the transport (PROPFIND/GET/PUT/MKCOL/DELETE), abstracted
//!   behind the `WebDav` trait so the `Sync` orchestration can be unit-
//!   tested against an in-memory fake without speaking HTTP.
//! - `orchestrator` — pull/push/sync semantics on top of `WebDav` and
//!   the local `Database`'s event log.
//! - `fake` — in-memory `WebDav` impl used by the sync tests; crate-
//!   private until we have an external need for it.

pub mod webdav;
pub mod orchestrator;
pub mod fake;
pub mod backoff;
pub mod coordinator;
pub mod indicator;
pub mod settings;

/// Remote folder name the orchestrator uses as the base path for
/// every WebDAV operation. Pinned across all shells so multiple
/// devices syncing to the same Nextcloud account converge on the
/// same `/Meditate/…` tree.
pub const REMOTE_BASE_PATH: &str = "Meditate";

pub use webdav::{HttpWebDav, WebDav, WebDavError, WebDavResult};
pub use orchestrator::{Sync, SyncError, SyncResult, SyncStats, PullStats, PushStats};
pub use fake::FakeWebDav;

/// "Is sync set up at all?" predicate. Returns true iff a Nextcloud
/// account is persisted (both URL and username are non-empty). The
/// shell uses this as the fast-path gate before spawning a sync
/// worker — skipping the keychain D-Bus round-trip when there's no
/// account to authenticate against. Any DB read failure is treated
/// as "not configured" so the predicate never raises.
pub fn should_attempt(db: &crate::db::Database) -> bool {
    settings::nextcloud_account_from_db(db)
        .map(|opt| opt.is_some())
        .unwrap_or(false)
}
