//! Per-device trackers for the WebDAV sync layer:
//! - `known_remote_files` records ingested-or-pushed batch_uuids so
//!   the puller can skip GET on files it already replayed and the
//!   pusher doesn't re-fetch its own uploads.
//! - `known_remote_sounds` does the same job at per-bell-sound
//!   granularity for the custom-audio sync.

use rusqlite::params;

use super::{Database, Result};

impl Database {
    /// Return every remote file_uuid that this device has already
    /// ingested or pushed. The puller queries this BEFORE issuing a GET
    /// on each remote file, so it can skip files it already pulled.
    /// The pusher inserts its own batch_uuid into this table on
    /// successful PUT.
    pub fn known_remote_file_uuids(&self) -> Result<std::collections::HashSet<String>> {
        let mut stmt = self.conn.prepare("SELECT file_uuid FROM known_remote_files")?;
        let ids = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<std::collections::HashSet<_>>>()?;
        Ok(ids)
    }

    /// Record a single batch_uuid as ingested. Idempotent (uses
    /// INSERT OR IGNORE) so callers don't have to check membership
    /// first.
    pub fn record_known_remote_file(&self, file_uuid: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO known_remote_files (file_uuid) VALUES (?1)",
            params![file_uuid],
        )?;
        Ok(())
    }

    /// Clear every recorded remote file_uuid. Two callers:
    /// - Account swap: when the user changes URL or username, the
    ///   previously-known remote files belong to a different store
    ///   entirely; clearing prevents a phantom "remote data lost"
    ///   trigger against the new account.
    /// - Push-local-after-wipe: after the user resolves a "remote data
    ///   lost" prompt by re-uploading, we wipe + re-anchor against
    ///   the now-empty remote.
    pub fn wipe_known_remote_files(&self) -> Result<()> {
        self.conn.execute("DELETE FROM known_remote_files", [])?;
        Ok(())
    }

    /// Per-bell-sound version of `known_remote_file_uuids` for the
    /// B.6 audio-file sync layer. Returns every bell uuid this device
    /// has either pushed or pulled; the orchestrator's push side
    /// skips files in this set and the pull side dittos.
    pub fn known_remote_sound_uuids(&self) -> Result<std::collections::HashSet<String>> {
        let mut stmt = self.conn.prepare("SELECT bell_uuid FROM known_remote_sounds")?;
        let ids = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<std::collections::HashSet<_>>>()?;
        Ok(ids)
    }

    /// INSERT-OR-IGNORE on the known-sound tracker. Idempotent so a
    /// retry after a half-completed PUT can re-call without fuss.
    pub fn record_known_remote_sound(&self, bell_uuid: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO known_remote_sounds (bell_uuid) VALUES (?1)",
            params![bell_uuid],
        )?;
        Ok(())
    }

    /// Clear the known-sound tracker. Same callers as
    /// wipe_known_remote_files: account swap, push-after-wipe.
    pub fn wipe_known_remote_sounds(&self) -> Result<()> {
        self.conn.execute("DELETE FROM known_remote_sounds", [])?;
        Ok(())
    }
}
