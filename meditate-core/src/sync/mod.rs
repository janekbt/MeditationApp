//! Nextcloud sync for the append-only event log.
//!
//! Layered:
//! - `webdav` — the transport (PROPFIND/GET/PUT/MKCOL/DELETE), abstracted
//!   behind the `WebDav` trait so the `Sync` orchestration in Phase D can
//!   be unit-tested against an in-memory fake without speaking HTTP.

pub mod webdav;

pub use webdav::{HttpWebDav, WebDav, WebDavError, WebDavResult};
