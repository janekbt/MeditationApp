//! Pull/push/sync orchestration on top of a `WebDav` transport. Speaks
//! to a personal Nextcloud (or to `FakeWebDav` in tests) and the local
//! `Database`'s event log; the cache is updated as a side effect of
//! `replay_events` calls.
//!
//! Wire layout: every push writes a single bulk file at
//! `<base>/events/<min_lamport:014>__<batch_uuid>.json` containing a
//! JSON array of `Event` objects (length 1+). The two-component
//! filename is unambiguous to parse: lamport is zero-padded digits,
//! batch_uuid is a v4 UUID. Pull lists the directory, dedups against
//! `known_remote_files` (so already-ingested batches aren't re-GET'd),
//! and replays the contents. Per-event dedup still happens via
//! `events.event_uuid UNIQUE` so a peer re-uploading our events is
//! a no-op locally.

use crate::db::{Database, DbError, Event};
use super::backoff::BackoffState;
use super::webdav::{WebDav, WebDavError};
use std::error::Error;
use std::fmt;

/// Per-PUT cap on consecutive 429 retries before giving up. With
/// exponential backoff (1+2+4+8+16+30+30+30 s), eight retries cover
/// ~2 minutes — long enough to ride a transient burst, short enough
/// that a permanently-throttled server surfaces failure rather than
/// hanging the sync forever.
const MAX_429_RETRIES: u32 = 8;

#[derive(Debug)]
pub enum SyncError {
    WebDav(WebDavError),
    Db(DbError),
    /// A remote event file couldn't be parsed back into a `Vec<Event>`.
    /// String is the underlying serde_json error, plus the filename
    /// for diagnostics.
    InvalidEvent(String),
    /// The remote folder was previously populated by this device but
    /// is now empty of any file we recognise — likely wiped by the
    /// user (or on the server) since the last successful sync. The
    /// orchestrator stops before push so the shell can ask the user
    /// what to do (push local back up, wipe local to match, or cancel)
    /// rather than silently re-uploading everything.
    RemoteDataLost,
}

impl fmt::Display for SyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WebDav(e) => write!(f, "sync: webdav: {e}"),
            Self::Db(e) => write!(f, "sync: db: {e:?}"),
            Self::InvalidEvent(s) => write!(f, "sync: invalid event: {s}"),
            Self::RemoteDataLost => write!(
                f, "sync: remote data lost — every batch this device \
                    previously synced is missing from the Nextcloud folder",
            ),
        }
    }
}

impl Error for SyncError {}

impl From<WebDavError> for SyncError {
    fn from(e: WebDavError) -> Self { Self::WebDav(e) }
}

impl From<DbError> for SyncError {
    fn from(e: DbError) -> Self { Self::Db(e) }
}

pub type SyncResult<T> = Result<T, SyncError>;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PullStats {
    /// Number of NEW events fetched and applied this pull. Excludes
    /// remote files we already had locally.
    pub new_events: usize,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PushStats {
    /// Number of pending local events successfully PUT to remote.
    pub pushed: usize,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SyncStats {
    pub pulled: usize,
    pub pushed: usize,
}

pub struct Sync<'a, W: WebDav> {
    db: &'a Database,
    webdav: &'a W,
    /// Path under the WebDAV root that holds this app's data, e.g.
    /// "Meditate". No leading or trailing slash — joined explicitly.
    base_path: String,
}

impl<'a, W: WebDav> Sync<'a, W> {
    pub fn new(db: &'a Database, webdav: &'a W, base_path: &str) -> Self {
        Self {
            db,
            webdav,
            base_path: base_path.trim_matches('/').to_string(),
        }
    }

    fn events_dir(&self) -> String {
        format!("{}/events", self.base_path)
    }

    /// Build the path for a bulk file. `min_lamport` is the smallest
    /// lamport_ts among the events bundled inside; sorting filenames
    /// alphabetically thus orders them roughly chronologically — useful
    /// when a human browses the remote dir.
    fn batch_path(&self, min_lamport: i64, batch_uuid: &str) -> String {
        format!(
            "{}/{:014}__{}.json",
            self.events_dir(),
            min_lamport,
            batch_uuid,
        )
    }

    pub fn pull(&self) -> SyncResult<PullStats> {
        // Per the plan: pull is non-destructive — only adds events.
        // First sync against an empty remote dir is just an empty list.
        let events_dir = self.events_dir();
        let listing: Vec<String> = match self.webdav.list_collection(&events_dir) {
            Ok(names) => names,
            // Treat NotFound as an empty listing so the same code path
            // handles "first sync against an empty Nextcloud" and
            // "remote dir was deleted since last sync". The remote-
            // data-lost check below distinguishes the two via
            // known_remote_files: a non-empty known set is the
            // signal that we've previously synced.
            Err(WebDavError::NotFound) => Vec::new(),
            Err(e) => return Err(e.into()),
        };

        let known_files = self.db.known_remote_file_uuids()?;

        // Remote-data-lost fail-safe. Trigger when this device has
        // previously synced (known set non-empty) but the current
        // listing contains zero of those batch_uuids. Bail BEFORE any
        // local writes so the shell can surface a dialog without us
        // having modified state.
        if !known_files.is_empty() {
            let listing_uuids: std::collections::HashSet<String> = listing
                .iter()
                .filter_map(|n| parse_batch_uuid_from_filename(n))
                .collect();
            let any_match = known_files.iter()
                .any(|uuid| listing_uuids.contains(uuid));
            if !any_match {
                return Err(SyncError::RemoteDataLost);
            }
        }

        let known_events = self.db.known_event_uuids()?;
        let mut new_events: Vec<Event> = Vec::new();
        let mut newly_ingested_files: Vec<String> = Vec::new();
        for name in &listing {
            let Some(batch_uuid) = parse_batch_uuid_from_filename(name) else {
                // Unrecognised filename — skip silently so a stray
                // file in the dir doesn't block sync.
                continue;
            };
            if known_files.contains(&batch_uuid) { continue; }
            let path = format!("{}/{}", events_dir, name);
            let body = self.webdav.get(&path)?;
            let events: Vec<Event> = serde_json::from_slice(&body)
                .map_err(|e| SyncError::InvalidEvent(format!("{name}: {e}")))?;
            for event in events {
                if !known_events.contains(&event.event_uuid) {
                    new_events.push(event);
                }
            }
            newly_ingested_files.push(batch_uuid);
        }

        let count = new_events.len();
        if !new_events.is_empty() {
            self.db.replay_events(&new_events)?;
        }
        // Record ingested batch_uuids only AFTER a successful replay,
        // so a partial replay doesn't leave us thinking we're done with
        // a file we didn't fully process.
        for batch_uuid in newly_ingested_files {
            self.db.record_known_remote_file(&batch_uuid)?;
        }
        Ok(PullStats { new_events: count })
    }

    pub fn push(&self) -> SyncResult<PushStats> {
        // Default: no progress reporting. Most callers (tests, the
        // happy path) don't need it. Long-running pushes (batch
        // import) wire `push_with_progress` so the diagnostics log
        // gets a completion event with timing.
        self.push_with_progress(|_, _| {})
    }

    /// Same as `push`, but invokes `progress(pushed, total)` after the
    /// (single) bulk PUT completes. The bulk-file design means there's
    /// just one progress notification per push, fired on success.
    pub fn push_with_progress<F>(&self, mut progress: F) -> SyncResult<PushStats>
    where F: FnMut(usize, usize),
    {
        // Always ensure the collection exists, even when there are
        // zero pending events. Two reasons:
        // - First-sync UX: after the user saves credentials and the
        //   trigger fires, the Meditate folder appears on Nextcloud
        //   even before any sessions have been authored. That's
        //   meaningful confirmation that URL+credentials work.
        // - Recovery: if a peer wiped the folder (or it never
        //   existed), repopulation works on next sync without
        //   requiring a fresh local mutation to wake the MKCOL path.
        self.ensure_events_dir_exists()?;

        let pending = self.db.pending_events()?;
        if pending.is_empty() {
            return Ok(PushStats::default());
        }

        // Bundle every pending event into a single Vec<Event>. Mint a
        // fresh batch_uuid for the file's filename. The min lamport
        // becomes the filename prefix so a directory listing browsed
        // by hand sorts chronologically.
        let event_ids: Vec<i64> = pending.iter().map(|(id, _)| *id).collect();
        let events: Vec<Event> = pending.into_iter().map(|(_, e)| e).collect();
        let min_lamport = events.iter().map(|e| e.lamport_ts).min().unwrap_or(0);
        let batch_uuid = uuid::Uuid::new_v4().to_string();
        let path = self.batch_path(min_lamport, &batch_uuid);
        let body = serde_json::to_vec(&events)
            .map_err(|e| SyncError::InvalidEvent(
                format!("can't serialise batch with {} events: {e}", events.len())))?;

        // Single PUT covers the whole batch. Built-in 429 handling so
        // a transient rate-limit doesn't surface as a failed sync.
        put_with_rate_limit_retry(self.webdav, &path, &body)?;

        // Atomically: mark every event in the batch synced, AND
        // record the batch_uuid as known so a future pull doesn't
        // re-GET our own upload. Done via batch + record helpers
        // (each is its own transaction; under WAL+NORMAL these are
        // cheap, and grouping them in one outer transaction would
        // require new plumbing).
        self.db.mark_events_synced(&event_ids)?;
        self.db.record_known_remote_file(&batch_uuid)?;

        let pushed = events.len();
        progress(pushed, pushed);
        Ok(PushStats { pushed })
    }

    /// Pull-then-push, in that order. Per the plan: pull is non-
    /// destructive (only adds events), and push only happens after a
    /// successful pull. The "I've seen everything you have, here's
    /// mine" semantics: every device strictly converges over enough
    /// rounds, regardless of timing.
    pub fn sync(&self) -> SyncResult<SyncStats> {
        self.sync_with_progress(|_, _| {})
    }

    /// Same as `sync`, but the push phase forwards completion progress
    /// via the callback. Pull doesn't report — its time is dominated
    /// by the single PROPFIND.
    pub fn sync_with_progress<F>(&self, progress: F) -> SyncResult<SyncStats>
    where F: FnMut(usize, usize),
    {
        let pull_stats = self.pull()?;
        let push_stats = self.push_with_progress(progress)?;
        Ok(SyncStats {
            pulled: pull_stats.new_events,
            pushed: push_stats.pushed,
        })
    }

    fn ensure_events_dir_exists(&self) -> SyncResult<()> {
        // MKCOL each segment from base downwards. Conflict (= already
        // exists) is the success case here.
        for path in [&self.base_path, &self.events_dir()] {
            match self.webdav.mkcol(path) {
                Ok(()) | Err(WebDavError::Conflict) => continue,
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }
}

/// PUT with cooperative rate-limit handling. On `RateLimited`, the
/// `BackoffState` updates the next-allowed instant per the server's
/// `Retry-After` (or exponential fallback), sleeps, and retries.
/// Bounded by `MAX_429_RETRIES` so a permanently-throttled server
/// surfaces failure rather than hanging the sync.
fn put_with_rate_limit_retry<W: WebDav>(
    webdav: &W,
    path: &str,
    body: &[u8],
) -> Result<(), WebDavError> {
    let mut backoff = BackoffState::new();
    let mut attempts: u32 = 0;
    loop {
        if let Some(d) = backoff.wait_until_now() {
            if !d.is_zero() {
                std::thread::sleep(d);
            }
        }
        match webdav.put(path, body) {
            Ok(()) => {
                backoff.note_success();
                return Ok(());
            }
            Err(WebDavError::RateLimited { retry_after }) => {
                attempts = attempts.saturating_add(1);
                if attempts >= MAX_429_RETRIES {
                    return Err(WebDavError::RateLimited { retry_after });
                }
                backoff.note_429(retry_after);
                continue;
            }
            Err(other) => return Err(other),
        }
    }
}

/// Extract the batch_uuid portion of a remote filename. Returns `None`
/// for files that don't match the expected
/// `<min_lamport:014>__<batch_uuid>.json` shape — strays / future
/// formats / `snapshot.json` / etc are skipped on pull.
fn parse_batch_uuid_from_filename(name: &str) -> Option<String> {
    let stem = name.strip_suffix(".json")?;
    let parts: Vec<&str> = stem.split("__").collect();
    if parts.len() != 2 { return None; }
    Some(parts[1].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Session, SessionMode};
    use crate::sync::fake::FakeWebDav;
    use crate::sync::webdav::WebDavResult;

    /// Convenience: build a fresh DB + fake remote pair, returning
    /// both. Each test sets up its own state.
    fn setup() -> (Database, FakeWebDav) {
        (Database::open_in_memory().unwrap(), FakeWebDav::new())
    }

    fn insert_session(db: &Database, start_iso: &str, secs: u32) -> i64 {
        db.insert_session(&Session {
            start_iso: start_iso.into(),
            duration_secs: secs,
            label_id: None,
            notes: None,
            mode: SessionMode::Countdown,
            uuid: String::new(),
        }).unwrap()
    }

    // ── Filename parser ──────────────────────────────────────────────────

    #[test]
    fn parse_batch_uuid_handles_canonical_name() {
        // The canonical filename is two zero-padded numbers / uuid:
        // <14-digit lamport>__<batch_uuid>.json.
        let name = "00000000000005__abcdef-1234.json";
        assert_eq!(parse_batch_uuid_from_filename(name),
            Some("abcdef-1234".to_string()));
    }

    #[test]
    fn parse_batch_uuid_rejects_non_json() {
        // No .json suffix → not our file. Defensive against, e.g., a
        // user dropping a README into the events dir.
        assert_eq!(parse_batch_uuid_from_filename("snapshot"), None);
    }

    #[test]
    fn parse_batch_uuid_rejects_wrong_field_count() {
        // 1 part, 3 parts: neither matches the 2-part contract.
        assert_eq!(parse_batch_uuid_from_filename("snapshot.json"), None);
        assert_eq!(parse_batch_uuid_from_filename("a__b__c.json"), None);
    }

    #[test]
    fn parse_batch_uuid_handles_real_uuid_shape() {
        // UUIDs contain single dashes; the parser must NOT confuse
        // them with the field separator (which is __).
        let name = "00000000000005__\
            11111111-1111-4111-8111-111111111111.json";
        assert_eq!(
            parse_batch_uuid_from_filename(name),
            Some("11111111-1111-4111-8111-111111111111".to_string()),
        );
    }

    // ── Sync::push — bulk format ─────────────────────────────────────────

    #[test]
    fn push_on_empty_pending_uploads_zero_files_but_creates_events_dir() {
        // First-sync UX: after saving credentials with no sessions yet,
        // push runs with zero pending events. It must still MKCOL the
        // events collection so the Meditate folder visibly appears on
        // the user's Nextcloud.
        let (db, fs) = setup();
        let sync = Sync::new(&db, &fs, "Meditate");
        let stats = sync.push().unwrap();
        assert_eq!(stats.pushed, 0,
            "no pending events → zero files uploaded");
        assert!(
            fs.list_collection("/Meditate/events/").unwrap().is_empty(),
            "no event files created on empty pending",
        );
    }

    #[test]
    fn push_uploads_all_pending_events_in_a_single_bulk_file() {
        // Three pending events bundle into ONE remote file, not three.
        let (db, fs) = setup();
        for i in 0..3 {
            insert_session(&db, &format!("s-{i}"), 100 + i as u32);
        }
        let stats = Sync::new(&db, &fs, "Meditate").push().unwrap();
        assert_eq!(stats.pushed, 3);
        let listing = fs.list_collection("/Meditate/events/").unwrap();
        assert_eq!(listing.len(), 1,
            "all events bundle into one bulk file, not one per event");
        assert!(listing[0].ends_with(".json"));
    }

    #[test]
    fn push_filename_includes_min_lamport_and_a_batch_uuid() {
        // Filename layout: <min_lamport:014>__<batch_uuid>.json. Verify
        // the lamport prefix matches the lowest lamport_ts among
        // bundled events, and the batch_uuid is parseable.
        let (db, fs) = setup();
        insert_session(&db, "first",  100);
        insert_session(&db, "second", 200);
        insert_session(&db, "third",  300);
        let pending = db.pending_events().unwrap();
        let min_lamport = pending.iter().map(|(_, e)| e.lamport_ts).min().unwrap();

        Sync::new(&db, &fs, "Meditate").push().unwrap();
        let listing = fs.list_collection("/Meditate/events/").unwrap();
        assert_eq!(listing.len(), 1);
        let name = &listing[0];
        // Prefix must be the 14-digit zero-padded min lamport.
        let expected_prefix = format!("{:014}__", min_lamport);
        assert!(name.starts_with(&expected_prefix),
            "filename `{name}` must begin with the min lamport prefix `{expected_prefix}`");
        // Suffix portion before .json must be parseable as a batch_uuid.
        let batch_uuid = parse_batch_uuid_from_filename(name).expect(
            "filename must parse as a batch uuid via the canonical parser");
        assert!(!batch_uuid.is_empty());
    }

    #[test]
    fn push_marks_events_synced_so_a_second_push_is_a_noop() {
        // After a successful push, nothing should be in pending. A
        // second push must do zero remote writes.
        let (db, fs) = setup();
        insert_session(&db, "2026-04-30T10:00:00", 600);
        Sync::new(&db, &fs, "Meditate").push().unwrap();
        assert!(db.pending_events().unwrap().is_empty(),
            "successful push must clear the pending queue");
        let count_after_first = fs.file_count();
        let stats = Sync::new(&db, &fs, "Meditate").push().unwrap();
        assert_eq!(stats.pushed, 0);
        assert_eq!(fs.file_count(), count_after_first,
            "second push must not write a new file");
    }

    #[test]
    fn push_records_its_own_batch_uuid_in_known_remote_files() {
        // Self-knowledge: after a successful push, the batch_uuid we
        // just wrote MUST be recorded locally. Otherwise the very next
        // pull on this same device would re-GET its own upload —
        // wasted bandwidth and a confusing trace in the logs.
        let (db, fs) = setup();
        insert_session(&db, "x", 100);
        Sync::new(&db, &fs, "Meditate").push().unwrap();
        let listing = fs.list_collection("/Meditate/events/").unwrap();
        let batch_uuid = parse_batch_uuid_from_filename(&listing[0]).unwrap();
        assert!(db.known_remote_file_uuids().unwrap().contains(&batch_uuid),
            "push must record the freshly-written batch_uuid as known");
    }

    #[test]
    fn push_serialises_events_as_a_json_array() {
        // Wire format is `Vec<Event>`. Pull on the other side parses
        // it as such; if push writes a single `Event` (not wrapped in
        // an array) it'd silently produce an unparseable file. Lock
        // the contract.
        let (db, fs) = setup();
        insert_session(&db, "2026-04-30T10:00:00", 600);
        let original_events: Vec<Event> = db.pending_events().unwrap()
            .into_iter().map(|(_, e)| e).collect();

        Sync::new(&db, &fs, "Meditate").push().unwrap();
        let listing = fs.list_collection("/Meditate/events/").unwrap();
        let body = fs.get(&format!("/Meditate/events/{}", listing[0])).unwrap();
        // Must parse as Vec<Event>.
        let parsed: Vec<Event> = serde_json::from_slice(&body)
            .expect("body must be a JSON array of events");
        assert_eq!(parsed, original_events);
    }

    #[test]
    fn push_payload_field_round_trips_through_the_wire_format() {
        // Pin the payload field's presence — it carries the actual
        // session/label data that replay_events parses on the
        // receiving side. A future serde(skip) mistake would silently
        // drop it; this catches that.
        let (db, fs) = setup();
        insert_session(&db, "2026-04-30T10:00:00", 600);
        Sync::new(&db, &fs, "Meditate").push().unwrap();
        let listing = fs.list_collection("/Meditate/events/").unwrap();
        let body = fs.get(&format!("/Meditate/events/{}", listing[0])).unwrap();
        let body_str = String::from_utf8(body).unwrap();
        assert!(body_str.contains("\"payload\""),
            "uploaded JSON must carry the payload field, got: {body_str}");
    }

    // ── Sync::pull — bulk format ─────────────────────────────────────────

    #[test]
    fn pull_against_empty_remote_dir_is_a_noop() {
        // First-ever sync: the remote /Meditate/events/ doesn't exist
        // yet, so PROPFIND returns NotFound. Pull must treat that as
        // "nothing upstream" rather than an error.
        let (db, fs) = setup();
        let stats = Sync::new(&db, &fs, "Meditate").pull().unwrap();
        assert_eq!(stats.new_events, 0);
    }

    #[test]
    fn pull_fetches_events_from_a_peer_bulk_file() {
        // Peer pushes 3 events as a single bulk file. We pull once →
        // we have all 3. The basic shape of cross-device sync.
        let (db_peer, fs) = setup();
        insert_session(&db_peer, "peer-a", 100);
        insert_session(&db_peer, "peer-b", 200);
        insert_session(&db_peer, "peer-c", 300);
        Sync::new(&db_peer, &fs, "Meditate").push().unwrap();

        let (db_us, _) = setup();
        let stats = Sync::new(&db_us, &fs, "Meditate").pull().unwrap();
        assert_eq!(stats.new_events, 3);
        let our_sessions = db_us.list_sessions().unwrap();
        assert_eq!(our_sessions.len(), 3);
    }

    #[test]
    fn pull_skips_a_remote_file_we_have_already_ingested() {
        // The dedup contract: once we've pulled a batch_uuid, a
        // subsequent pull MUST skip it (no GET, no replay overhead).
        // Verified by counting GETs via a counting fake.
        struct CountingGet {
            inner: FakeWebDav,
            gets: std::sync::atomic::AtomicUsize,
        }
        impl WebDav for CountingGet {
            fn list_collection(&self, p: &str) -> WebDavResult<Vec<String>> {
                self.inner.list_collection(p)
            }
            fn get(&self, p: &str) -> WebDavResult<Vec<u8>> {
                self.gets.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                self.inner.get(p)
            }
            fn put(&self, p: &str, b: &[u8]) -> WebDavResult<()> {
                self.inner.put(p, b)
            }
            fn mkcol(&self, p: &str) -> WebDavResult<()> {
                self.inner.mkcol(p)
            }
            fn delete(&self, p: &str) -> WebDavResult<()> {
                self.inner.delete(p)
            }
        }

        let fs = FakeWebDav::new();
        // Peer pushes; we pull once.
        let db_peer = Database::open_in_memory().unwrap();
        insert_session(&db_peer, "from peer", 100);
        Sync::new(&db_peer, &fs, "Meditate").push().unwrap();

        let counting = CountingGet {
            inner: fs.clone(),
            gets: std::sync::atomic::AtomicUsize::new(0),
        };
        let db_us = Database::open_in_memory().unwrap();
        Sync::new(&db_us, &counting, "Meditate").pull().unwrap();
        assert_eq!(
            counting.gets.load(std::sync::atomic::Ordering::SeqCst), 1,
            "first pull must GET the file");

        // Second pull on the same device: no GETs.
        Sync::new(&db_us, &counting, "Meditate").pull().unwrap();
        assert_eq!(
            counting.gets.load(std::sync::atomic::Ordering::SeqCst), 1,
            "second pull must skip the already-ingested file via known_remote_files");
    }

    #[test]
    fn pull_dedups_events_inside_a_bulk_file_against_local_known_event_uuids() {
        // Defense-in-depth: even if a forged remote file contains
        // an event whose UUID is already in our local log (e.g. a
        // peer re-uploading data, or a buggy peer), the per-event
        // dedup must prevent double-application.
        //
        // Setup: a peer pushes one event. We pull it. We then forge
        // a SECOND remote file (different batch_uuid) containing the
        // same event the peer published. Our second pull GETs the
        // forged file (different batch_uuid → known_remote_files miss)
        // but per-event dedup must skip the duplicate.
        let (db_peer, fs) = setup();
        insert_session(&db_peer, "shared", 100);
        let original_event = db_peer.pending_events().unwrap()
            .into_iter().map(|(_, e)| e).next().unwrap();
        Sync::new(&db_peer, &fs, "Meditate").push().unwrap();

        let (db_us, _) = setup();
        Sync::new(&db_us, &fs, "Meditate").pull().unwrap();
        assert_eq!(db_us.list_sessions().unwrap().len(), 1);

        // Forge a second remote file with the same event under a new
        // batch_uuid, bypassing our stored known_remote_files.
        let forged = vec![original_event];
        let body = serde_json::to_vec(&forged).unwrap();
        fs.put("/Meditate/events/00000000000099__forged-batch.json", &body).unwrap();

        let stats = Sync::new(&db_us, &fs, "Meditate").pull().unwrap();
        assert_eq!(stats.new_events, 0,
            "events whose UUIDs we already know must not be reapplied");
        assert_eq!(db_us.list_sessions().unwrap().len(), 1,
            "row count must not grow under duplicate ingestion");
    }

    #[test]
    fn pull_skips_files_with_unrecognised_filename_shapes() {
        // The events dir might contain a `snapshot.json` (compaction
        // artefact, future) or a stray file someone uploaded by hand.
        // Pull must not bail on these — just skip and move on.
        let (db, fs) = setup();
        fs.put("/Meditate/events/snapshot.json", b"[]").unwrap();
        fs.put("/Meditate/events/random_garbage", b"junk").unwrap();
        let stats = Sync::new(&db, &fs, "Meditate").pull().unwrap();
        assert_eq!(stats.new_events, 0);
    }

    #[test]
    fn pull_propagates_invalid_event_json_as_typed_error() {
        // A correctly-named but corrupt batch file is a legit error
        // signal — surface it so the caller can log/notify, rather
        // than silently dropping events.
        let (db, fs) = setup();
        fs.put(
            "/Meditate/events/00000000000001__some-batch.json",
            b"this is not JSON",
        ).unwrap();
        let err = Sync::new(&db, &fs, "Meditate").pull().unwrap_err();
        assert!(matches!(err, SyncError::InvalidEvent(_)),
            "corrupt remote batch must surface as InvalidEvent, got {err:?}");
    }

    #[test]
    fn pull_after_peer_push_advances_local_lamport() {
        // The Lamport observation rule must fire through the pull path:
        // after pulling a peer's event with lamport=N, any local event
        // we author next must have lamport > N.
        let (db_peer, fs) = setup();
        for _ in 0..20 { db_peer.bump_lamport_clock().unwrap(); }
        insert_session(&db_peer, "peer-session", 100);
        let peer_lamport = db_peer.pending_events().unwrap()
            .iter().map(|(_, e)| e.lamport_ts).max().unwrap();
        Sync::new(&db_peer, &fs, "Meditate").push().unwrap();

        let (db_us, _) = setup();
        Sync::new(&db_us, &fs, "Meditate").pull().unwrap();
        assert!(db_us.lamport_clock().unwrap() > peer_lamport,
            "local clock {} must exceed observed peer lamport {}",
            db_us.lamport_clock().unwrap(), peer_lamport);
    }

    // ── Sync::sync — end-to-end two-device convergence ───────────────────

    #[test]
    fn sync_against_empty_remote_pushes_local_events_first() {
        // First-device-online scenario: nothing upstream yet, we have
        // local writes. sync() should surface them via push.
        let (db, fs) = setup();
        insert_session(&db, "2026-04-30T10:00:00", 600);
        let stats = Sync::new(&db, &fs, "Meditate").sync().unwrap();
        assert_eq!(stats.pulled, 0);
        assert_eq!(stats.pushed, 1);
        assert_eq!(fs.file_count(), 1);
    }

    #[test]
    fn sync_two_devices_via_one_round_each_converges() {
        // Phone authors. Phone syncs (push). Laptop syncs (pull). Both
        // now have the same state.
        let (phone_db, fs) = setup();
        insert_session(&phone_db, "phone-session", 600);
        Sync::new(&phone_db, &fs, "Meditate").sync().unwrap();

        let (laptop_db, _) = setup();
        Sync::new(&laptop_db, &fs, "Meditate").sync().unwrap();

        let phone_sessions = phone_db.list_sessions().unwrap();
        let laptop_sessions = laptop_db.list_sessions().unwrap();
        assert_eq!(phone_sessions.len(), 1);
        assert_eq!(laptop_sessions.len(), 1);
        assert_eq!(phone_sessions[0].1.uuid, laptop_sessions[0].1.uuid,
            "the same row uuid must survive the wire round-trip");
    }

    #[test]
    fn sync_concurrent_authoring_converges_after_two_rounds() {
        // Both devices author offline, both sync. After both have
        // synced twice, both have everything.
        let (a_db, fs) = setup();
        let (b_db, _) = setup();
        insert_session(&a_db, "from A", 100);
        insert_session(&b_db, "from B", 200);

        Sync::new(&a_db, &fs, "Meditate").sync().unwrap();
        Sync::new(&b_db, &fs, "Meditate").sync().unwrap();
        Sync::new(&a_db, &fs, "Meditate").sync().unwrap();
        Sync::new(&b_db, &fs, "Meditate").sync().unwrap();

        let starts_a: std::collections::HashSet<String> = a_db.list_sessions()
            .unwrap().iter().map(|(_, s)| s.start_iso.clone()).collect();
        let starts_b: std::collections::HashSet<String> = b_db.list_sessions()
            .unwrap().iter().map(|(_, s)| s.start_iso.clone()).collect();
        assert_eq!(starts_a, starts_b, "both devices must converge");
        let expected: std::collections::HashSet<_> =
            ["from A", "from B"].iter().map(|s| s.to_string()).collect();
        assert_eq!(starts_a, expected);
    }

    #[test]
    fn sync_propagates_tombstones_across_devices() {
        // A authors, both have the row, A deletes, both sync, B no
        // longer has the row.
        let (a_db, fs) = setup();
        let (b_db, _) = setup();
        let session_id = insert_session(&a_db, "to-be-deleted", 100);
        Sync::new(&a_db, &fs, "Meditate").sync().unwrap();
        Sync::new(&b_db, &fs, "Meditate").sync().unwrap();
        assert_eq!(b_db.list_sessions().unwrap().len(), 1);

        a_db.delete_session(session_id).unwrap();
        Sync::new(&a_db, &fs, "Meditate").sync().unwrap();
        assert!(a_db.list_sessions().unwrap().is_empty());

        Sync::new(&b_db, &fs, "Meditate").sync().unwrap();
        assert!(b_db.list_sessions().unwrap().is_empty(),
            "tombstone must propagate via pull");
    }

    #[test]
    fn sync_is_idempotent_under_repeated_runs() {
        // Calling sync() N times after the same authoring activity
        // must not duplicate state on either side.
        let (a_db, fs) = setup();
        let (b_db, _) = setup();
        insert_session(&a_db, "from A", 100);
        insert_session(&b_db, "from B", 200);

        for _ in 0..4 {
            Sync::new(&a_db, &fs, "Meditate").sync().unwrap();
            Sync::new(&b_db, &fs, "Meditate").sync().unwrap();
        }
        assert_eq!(a_db.list_sessions().unwrap().len(), 2);
        assert_eq!(b_db.list_sessions().unwrap().len(), 2);

        for _ in 0..2 {
            Sync::new(&a_db, &fs, "Meditate").sync().unwrap();
            Sync::new(&b_db, &fs, "Meditate").sync().unwrap();
        }
        assert_eq!(a_db.list_sessions().unwrap().len(), 2);
        assert_eq!(b_db.list_sessions().unwrap().len(), 2);
    }

    #[test]
    fn sync_pulls_before_pushing() {
        // Pull-before-push semantics ensure local events authored AFTER
        // observing peer state inherit a lamport that exceeds the peer's.
        let (peer_db, fs) = setup();
        for _ in 0..50 { peer_db.bump_lamport_clock().unwrap(); }
        insert_session(&peer_db, "peer", 100);
        Sync::new(&peer_db, &fs, "Meditate").sync().unwrap();
        let peer_max_lamport = peer_db.lamport_clock().unwrap();

        let (us_db, _) = setup();
        Sync::new(&us_db, &fs, "Meditate").sync().unwrap();
        insert_session(&us_db, "ours", 200);
        let our_event_lamport = us_db.pending_events().unwrap()
            .iter()
            .find(|(_, e)| e.kind == "session_insert"
                       && e.device_id == us_db.device_id().unwrap())
            .map(|(_, e)| e.lamport_ts)
            .unwrap();
        assert!(our_event_lamport > peer_max_lamport,
            "post-sync local event at lamport {} must exceed peer's max {}",
            our_event_lamport, peer_max_lamport);
    }

    // ── Remote data lost — fail-safe on wiped Nextcloud ──────────────────
    //
    // After a successful sync this device has recorded one or more
    // batch_uuids in `known_remote_files`. If a later sync sees a
    // remote whose listing contains zero of those known uuids, the
    // remote was wiped (intentionally or not). Pushing local state in
    // that situation is a destructive surprise — the user might have
    // wiped the Nextcloud with the intent of also wiping this device.
    // The fail-safe surfaces this via `SyncError::RemoteDataLost` so
    // the shell can put up a "what do you want to do?" dialog.

    #[test]
    fn sync_does_not_trigger_remote_data_lost_on_first_ever_sync() {
        // First-ever sync: known_remote_files is empty, remote is
        // empty. Cannot trigger because the precondition (we've
        // previously synced) isn't met.
        let (db, fs) = setup();
        let result = Sync::new(&db, &fs, "Meditate").sync();
        assert!(result.is_ok(),
            "fresh sync against empty remote must not trigger; got {result:?}");
    }

    #[test]
    fn sync_does_not_trigger_remote_data_lost_when_we_pull_a_peer_into_an_empty_local() {
        // Common scenario: laptop already populated Nextcloud, phone
        // is fresh. Phone's known_remote_files is empty (precondition
        // false), so pulling the peer's files must not trigger.
        let (peer_db, fs) = setup();
        insert_session(&peer_db, "peer", 100);
        Sync::new(&peer_db, &fs, "Meditate").sync().unwrap();

        let (us_db, _) = setup();
        let result = Sync::new(&us_db, &fs, "Meditate").sync();
        assert!(result.is_ok(),
            "first-time pull from a populated remote must not trigger");
    }

    #[test]
    fn sync_does_not_trigger_remote_data_lost_when_some_of_our_files_are_still_present() {
        // Partial wipe: known_remote_files has A, B, C; listing has
        // A, B (C deleted). At least one of ours survives → not the
        // "everything got wiped" pattern. Don't trigger.
        let (db, fs) = setup();
        insert_session(&db, "first", 100);
        Sync::new(&db, &fs, "Meditate").sync().unwrap();
        insert_session(&db, "second", 200);
        Sync::new(&db, &fs, "Meditate").sync().unwrap();
        // Now the remote has 2 files; both batch_uuids are in
        // known_remote_files. Manually delete just one.
        let listing: Vec<String> = fs
            .list_collection("/Meditate/events/").unwrap();
        assert_eq!(listing.len(), 2);
        fs.delete(&format!("/Meditate/events/{}", listing[0])).unwrap();

        let result = Sync::new(&db, &fs, "Meditate").sync();
        assert!(result.is_ok(),
            "partial wipe must not trigger fail-safe; got {result:?}");
    }

    #[test]
    fn sync_triggers_remote_data_lost_when_every_known_file_is_gone() {
        // Setup: device pushes once → known_remote_files contains the
        // batch_uuid. Wipe the remote folder. Next sync: listing is
        // empty, but known_remote_files isn't → trigger.
        let (db, fs) = setup();
        insert_session(&db, "first", 100);
        Sync::new(&db, &fs, "Meditate").sync().unwrap();
        assert_eq!(db.known_remote_file_uuids().unwrap().len(), 1);
        // Wipe the remote.
        for name in fs.list_collection("/Meditate/events/").unwrap() {
            fs.delete(&format!("/Meditate/events/{}", name)).unwrap();
        }

        let err = Sync::new(&db, &fs, "Meditate").sync().unwrap_err();
        assert!(matches!(err, SyncError::RemoteDataLost),
            "wiped remote must surface RemoteDataLost; got {err:?}");
    }

    #[test]
    fn sync_triggers_remote_data_lost_even_when_a_peer_repopulated_with_new_files() {
        // Subtle case: our previous remote was wiped AND a peer wrote
        // new files. The listing is non-empty but contains none of our
        // known_remote_files entries — still a wipe from our point of
        // view. Trigger so the user sees the prompt.
        let (db, fs) = setup();
        insert_session(&db, "ours", 100);
        Sync::new(&db, &fs, "Meditate").sync().unwrap();
        // Wipe the remote.
        for name in fs.list_collection("/Meditate/events/").unwrap() {
            fs.delete(&format!("/Meditate/events/{}", name)).unwrap();
        }
        // Peer authors and pushes its own data.
        let (peer_db, _) = setup();
        insert_session(&peer_db, "peer", 200);
        Sync::new(&peer_db, &fs, "Meditate").sync().unwrap();

        // We sync. Remote has ONE file (the peer's), but it's not one
        // we've previously seen. Our known set has entries → trigger.
        let err = Sync::new(&db, &fs, "Meditate").sync().unwrap_err();
        assert!(matches!(err, SyncError::RemoteDataLost),
            "peer-repopulated wipe must still trigger; got {err:?}");
    }

    #[test]
    fn sync_does_not_trigger_remote_data_lost_after_an_explicit_wipe_known_remote_files() {
        // The "push local to remote" recovery path: after the user
        // resolves the dialog by saying "push my local up", the shell
        // calls `wipe_known_remote_files` and re-runs sync. The next
        // sync sees an empty remote and an empty known set →
        // precondition false → no trigger.
        let (db, fs) = setup();
        insert_session(&db, "first", 100);
        Sync::new(&db, &fs, "Meditate").sync().unwrap();
        for name in fs.list_collection("/Meditate/events/").unwrap() {
            fs.delete(&format!("/Meditate/events/{}", name)).unwrap();
        }
        // Operator action: clear the dedup tracker.
        db.wipe_known_remote_files().unwrap();
        // Mark events un-synced so push has work to do.
        for name in fs.list_collection("/Meditate/events/").unwrap() {
            fs.delete(&format!("/Meditate/events/{}", name)).unwrap();
        }

        let result = Sync::new(&db, &fs, "Meditate").sync();
        assert!(result.is_ok(),
            "after wipe_known_remote_files the next sync must succeed; got {result:?}");
    }

    #[test]
    fn sync_remote_data_lost_does_not_modify_local_state() {
        // The fail-safe gates the destructive write. When it fires,
        // pending events must remain pending and known_remote_files
        // must be untouched — the user hasn't decided yet whether to
        // push or wipe, so we leave everything as-is.
        let (db, fs) = setup();
        insert_session(&db, "first", 100);
        Sync::new(&db, &fs, "Meditate").sync().unwrap();
        // Author a new pending event AFTER the successful sync.
        insert_session(&db, "second", 200);
        let pending_before = db.pending_events().unwrap().len();
        let known_before = db.known_remote_file_uuids().unwrap();

        // Wipe remote.
        for name in fs.list_collection("/Meditate/events/").unwrap() {
            fs.delete(&format!("/Meditate/events/{}", name)).unwrap();
        }

        let _ = Sync::new(&db, &fs, "Meditate").sync().unwrap_err();
        let pending_after = db.pending_events().unwrap().len();
        let known_after = db.known_remote_file_uuids().unwrap();
        assert_eq!(pending_after, pending_before,
            "RemoteDataLost must not flip the pending flag on any event");
        assert_eq!(known_after, known_before,
            "RemoteDataLost must not modify known_remote_files");
    }

    #[test]
    fn sync_error_remote_data_lost_displays_a_clear_message() {
        // The Display string flows into the diagnostics log and (later)
        // the user-facing dialog body. Pin the wording so a future copy
        // edit doesn't accidentally make it ambiguous.
        let s = SyncError::RemoteDataLost.to_string();
        assert!(s.contains("remote") || s.contains("Nextcloud"),
            "must mention what was lost (remote/Nextcloud), got: {s}");
        assert!(s.contains("data lost") || s.contains("wiped") || s.contains("missing"),
            "must indicate the loss explicitly, got: {s}");
    }

    // ── Bulk-import scale ────────────────────────────────────────────────

    #[test]
    fn push_2700_events_uploads_a_single_bulk_file() {
        // The motivating use case: a 2700-event Insight Timer import
        // becomes ONE PUT. Without bundling this would have been 2700
        // PUTs, each 3-15 s on a slow Nextcloud — hours of total time.
        // We use 2700 here matching the real import; the assertion is
        // structural (one file regardless of count).
        let (db, fs) = setup();
        for i in 0..2700 {
            insert_session(&db, &format!("import-{i:04}"), 100);
        }
        let stats = Sync::new(&db, &fs, "Meditate").push().unwrap();
        assert_eq!(stats.pushed, 2700);
        assert_eq!(fs.list_collection("/Meditate/events/").unwrap().len(), 1,
            "bulk import must produce exactly one remote file");
        assert!(db.pending_events().unwrap().is_empty());
    }

    #[test]
    fn push_2700_events_then_peer_pull_reconstructs_every_session() {
        // End-to-end bulk-import convergence: 2700 events go up as one
        // file; a fresh peer's pull replays all 2700 sessions locally.
        let (db_a, fs) = setup();
        for i in 0..2700 {
            insert_session(&db_a, &format!("a-{i:04}"), 100);
        }
        Sync::new(&db_a, &fs, "Meditate").push().unwrap();

        let (db_b, _) = setup();
        let stats = Sync::new(&db_b, &fs, "Meditate").pull().unwrap();
        assert_eq!(stats.new_events, 2700);
        assert_eq!(db_b.list_sessions().unwrap().len(), 2700);
    }

    // ── Push-side dedup: PUT failure modes ────────────────────────────────

    #[test]
    fn push_propagates_a_non_rate_limit_error() {
        // 401 Unauthorized is unrecoverable from this side — the user
        // needs to fix credentials. Push must surface it as a SyncError
        // so the runner records it on `last_sync_error`.
        struct PutFails(FakeWebDav);
        impl WebDav for PutFails {
            fn list_collection(&self, p: &str) -> WebDavResult<Vec<String>> {
                self.0.list_collection(p)
            }
            fn get(&self, p: &str) -> WebDavResult<Vec<u8>> { self.0.get(p) }
            fn put(&self, _: &str, _: &[u8]) -> WebDavResult<()> {
                Err(WebDavError::Unauthorized)
            }
            fn mkcol(&self, p: &str) -> WebDavResult<()> { self.0.mkcol(p) }
            fn delete(&self, p: &str) -> WebDavResult<()> { self.0.delete(p) }
        }
        let (db, _) = setup();
        for i in 0..5 { insert_session(&db, &format!("e-{i}"), 100); }
        let bad = PutFails(FakeWebDav::new());
        let err = Sync::new(&db, &bad, "Meditate").push().unwrap_err();
        match err {
            SyncError::WebDav(WebDavError::Unauthorized) => {}
            other => panic!("expected SyncError::WebDav(Unauthorized), got {other:?}"),
        }
        // No events get marked synced after a failure (we didn't reach
        // the post-PUT mark step). Pending stays full so the next sync
        // retries the whole batch.
        assert_eq!(db.pending_events().unwrap().len(), 5);
    }

    #[test]
    fn push_retries_through_a_burst_of_429s_until_success() {
        // The rate-limit recovery contract. Custom fake returns 429
        // with Retry-After: 0 the first 3 PUT attempts, then succeeds.
        // Push must back off and retry without surfacing failure.
        struct FlakyRateLimit {
            inner: FakeWebDav,
            remaining_429: std::sync::atomic::AtomicUsize,
        }
        impl WebDav for FlakyRateLimit {
            fn list_collection(&self, p: &str) -> WebDavResult<Vec<String>> {
                self.inner.list_collection(p)
            }
            fn get(&self, p: &str) -> WebDavResult<Vec<u8>> { self.inner.get(p) }
            fn put(&self, p: &str, b: &[u8]) -> WebDavResult<()> {
                use std::sync::atomic::Ordering;
                let prev = self.remaining_429.fetch_update(
                    Ordering::SeqCst, Ordering::SeqCst,
                    |c| if c > 0 { Some(c - 1) } else { None },
                );
                if prev.is_ok() {
                    Err(WebDavError::RateLimited { retry_after: Some(0) })
                } else {
                    self.inner.put(p, b)
                }
            }
            fn mkcol(&self, p: &str) -> WebDavResult<()> { self.inner.mkcol(p) }
            fn delete(&self, p: &str) -> WebDavResult<()> { self.inner.delete(p) }
        }

        let (db, fs) = setup();
        for i in 0..4 { insert_session(&db, &format!("r-{i}"), 100); }
        let throttled = FlakyRateLimit {
            inner: fs.clone(),
            remaining_429: std::sync::atomic::AtomicUsize::new(3),
        };
        let stats = Sync::new(&db, &throttled, "Meditate").push().unwrap();
        assert_eq!(stats.pushed, 4);
        assert!(db.pending_events().unwrap().is_empty());
    }

    #[test]
    fn push_gives_up_after_max_429_retries_and_surfaces_rate_limited() {
        // A permanently-throttled server must eventually surface
        // failure. MAX_429_RETRIES caps retry count so a sync attempt
        // can't hang forever.
        struct AlwaysRateLimited(FakeWebDav);
        impl WebDav for AlwaysRateLimited {
            fn list_collection(&self, p: &str) -> WebDavResult<Vec<String>> {
                self.0.list_collection(p)
            }
            fn get(&self, p: &str) -> WebDavResult<Vec<u8>> { self.0.get(p) }
            fn put(&self, _: &str, _: &[u8]) -> WebDavResult<()> {
                Err(WebDavError::RateLimited { retry_after: Some(0) })
            }
            fn mkcol(&self, p: &str) -> WebDavResult<()> { self.0.mkcol(p) }
            fn delete(&self, p: &str) -> WebDavResult<()> { self.0.delete(p) }
        }
        let (db, _) = setup();
        insert_session(&db, "x", 100);
        let bad = AlwaysRateLimited(FakeWebDav::new());
        let err = Sync::new(&db, &bad, "Meditate").push().unwrap_err();
        assert!(matches!(err,
            SyncError::WebDav(WebDavError::RateLimited { .. })),
            "after MAX_429_RETRIES the error must surface as RateLimited, got {err:?}");
    }
}
