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

pub use webdav::{HttpWebDav, WebDav, WebDavError, WebDavResult};
pub use orchestrator::{Sync, SyncError, SyncResult, SyncStats, PullStats, PushStats};
pub use fake::FakeWebDav;
