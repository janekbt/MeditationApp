//! Pull/push/sync orchestration on top of a `WebDav` transport. Speaks
//! to a personal Nextcloud (or to `FakeWebDav` in tests) and the local
//! `Database`'s event log; the cache is updated as a side effect of
//! `replay_events` calls.
//!
//! Wire layout: every event lives at
//! `<base>/events/<lamport:014>__<device_uuid>__<event_uuid>.json`.
//! The triple-underscore-separated triple is unambiguous to parse:
//! UUIDs contain single dashes, never `__`.

use crate::db::{Database, DbError, Event};
use super::webdav::{WebDav, WebDavError};
use std::error::Error;
use std::fmt;

#[derive(Debug)]
pub enum SyncError {
    WebDav(WebDavError),
    Db(DbError),
    /// A remote event file couldn't be parsed back into an `Event`.
    /// String is the underlying serde_json error, plus the filename
    /// for diagnostics.
    InvalidEvent(String),
}

impl fmt::Display for SyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WebDav(e) => write!(f, "sync: webdav: {e}"),
            Self::Db(e) => write!(f, "sync: db: {e:?}"),
            Self::InvalidEvent(s) => write!(f, "sync: invalid event: {s}"),
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

    fn event_path(&self, event: &Event) -> String {
        format!(
            "{}/{:014}__{}__{}.json",
            self.events_dir(),
            event.lamport_ts,
            event.device_id,
            event.event_uuid,
        )
    }

    pub fn pull(&self) -> SyncResult<PullStats> {
        // Per the plan: pull is non-destructive — only adds events.
        // First sync against an empty remote dir is just an empty list.
        let events_dir = self.events_dir();
        let listing = match self.webdav.list_collection(&events_dir) {
            Ok(names) => names,
            Err(WebDavError::NotFound) => {
                // Remote dir absent — first-ever sync, nothing upstream.
                return Ok(PullStats::default());
            }
            Err(e) => return Err(e.into()),
        };

        let known = self.db.known_event_uuids()?;
        let mut new_events = Vec::new();
        for name in &listing {
            let Some(event_uuid) = parse_event_uuid_from_filename(name) else {
                // Unrecognised filename — could be a future format, a
                // snapshot.json, or a stray file. Skip silently so we
                // don't block sync on garbage in the directory.
                continue;
            };
            if known.contains(&event_uuid) { continue; }
            let path = format!("{}/{}", events_dir, name);
            let body = self.webdav.get(&path)?;
            let event: Event = serde_json::from_slice(&body)
                .map_err(|e| SyncError::InvalidEvent(format!("{name}: {e}")))?;
            new_events.push(event);
        }

        let count = new_events.len();
        if !new_events.is_empty() {
            self.db.replay_events(&new_events)?;
        }
        Ok(PullStats { new_events: count })
    }

    pub fn push(&self) -> SyncResult<PushStats> {
        // Always ensure the collection exists, even when we have zero
        // pending events. Two reasons:
        // - First-sync UX: after the user saves credentials and the
        //   trigger fires, the Meditate folder appears on Nextcloud
        //   even before any sessions have been authored. That's
        //   meaningful confirmation that URL+credentials work.
        // - Recovery: if a peer wiped the folder (or it never
        //   existed), repopulation works on next sync without
        //   requiring a fresh local mutation to wake the MKCOL path.
        //
        // Idempotent on the wire: 405 (Conflict-existing) is treated
        // as success in `ensure_events_dir_exists`. The cost is one
        // MKCOL round-trip per sync (~50-200 ms) which is small
        // compared to the actual upload work; for offline / errored
        // hosts, MKCOL fails the same way PUT would, so this also
        // surfaces the auth/connectivity error one step earlier.
        self.ensure_events_dir_exists()?;

        let pending = self.db.pending_events()?;
        if pending.is_empty() {
            return Ok(PushStats::default());
        }

        let mut pushed = 0;
        for (id, event) in pending {
            let path = self.event_path(&event);
            let body = serde_json::to_vec(&event)
                .map_err(|e| SyncError::InvalidEvent(
                    format!("can't serialise event {}: {e}", event.event_uuid)))?;
            // PUT errors halt the push so the caller sees the failure
            // and pending events stay marked unsynced for next attempt.
            self.webdav.put(&path, &body)?;
            self.db.mark_event_synced(id)?;
            pushed += 1;
        }
        Ok(PushStats { pushed })
    }

    /// Pull-then-push, in that order. Per the plan: pull is non-
    /// destructive (only adds events), and push only happens after a
    /// successful pull. The "I've seen everything you have, here's
    /// mine" semantics: every device strictly converges over enough
    /// rounds, regardless of timing.
    pub fn sync(&self) -> SyncResult<SyncStats> {
        let pull_stats = self.pull()?;
        let push_stats = self.push()?;
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

/// Extract the event_uuid portion of a remote filename. Returns `None`
/// for files that don't match the expected
/// `<lamport>__<device_uuid>__<event_uuid>.json` shape — any such
/// files (snapshot.json, future formats, strays) are skipped on pull.
fn parse_event_uuid_from_filename(name: &str) -> Option<String> {
    let stem = name.strip_suffix(".json")?;
    let parts: Vec<&str> = stem.split("__").collect();
    if parts.len() != 3 { return None; }
    Some(parts[2].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Session, SessionMode};
    use crate::sync::fake::FakeWebDav;

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
    fn parse_event_uuid_handles_canonical_name() {
        let name = "00000000000005__d-uuid__e-uuid.json";
        assert_eq!(parse_event_uuid_from_filename(name), Some("e-uuid".to_string()));
    }

    #[test]
    fn parse_event_uuid_rejects_non_json() {
        assert_eq!(parse_event_uuid_from_filename("snapshot"), None);
    }

    #[test]
    fn parse_event_uuid_rejects_wrong_field_count() {
        // Future format with 4 fields, or a stray file with 2: skip.
        assert_eq!(parse_event_uuid_from_filename("a__b.json"), None);
        assert_eq!(parse_event_uuid_from_filename("a__b__c__d.json"), None);
    }

    #[test]
    fn parse_event_uuid_handles_real_uuid_shapes() {
        // UUIDs contain single dashes; the parser must NOT confuse them
        // with field separators (which are __).
        let name = "00000000000005__\
            00000000-0000-4000-8000-aaaaaaaaaaaa__\
            11111111-1111-4111-8111-111111111111.json";
        assert_eq!(
            parse_event_uuid_from_filename(name),
            Some("11111111-1111-4111-8111-111111111111".to_string()),
        );
    }

    // ── Sync::push ───────────────────────────────────────────────────────────

    #[test]
    fn push_on_empty_pending_uploads_zero_files_but_creates_events_dir() {
        // First-sync UX: after saving credentials with no sessions yet,
        // push runs with zero pending events. It must still MKCOL the
        // events collection so the Meditate folder visibly appears on
        // the user's Nextcloud — that's confirmation the credentials
        // work. (FakeWebDav models directories implicitly, so we can't
        // observe the MKCOL directly, but `pushed` should still be 0
        // and no files should appear under events/.)
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
    fn push_uploads_each_pending_event_as_a_file_under_events() {
        let (db, fs) = setup();
        insert_session(&db, "2026-04-30T10:00:00", 600);
        let sync = Sync::new(&db, &fs, "Meditate");
        let stats = sync.push().unwrap();
        assert_eq!(stats.pushed, 1);

        // The event is now a file in /Meditate/events/.
        let listing = fs.list_collection("/Meditate/events/").unwrap();
        assert_eq!(listing.len(), 1);
        assert!(listing[0].ends_with(".json"));
    }

    #[test]
    fn push_uses_canonical_filename_with_lamport_prefix_and_event_uuid() {
        // Filename contract: `<lamport:014>__<device>__<event_uuid>.json`.
        // Lock this down — peers parse against the same convention.
        let (db, fs) = setup();
        let device_id = db.device_id().unwrap();
        insert_session(&db, "2026-04-30T10:00:00", 600);
        let pending = db.pending_events().unwrap();
        let event_uuid = pending[0].1.event_uuid.clone();
        let lamport = pending[0].1.lamport_ts;

        Sync::new(&db, &fs, "Meditate").push().unwrap();
        let listing = fs.list_collection("/Meditate/events/").unwrap();
        assert_eq!(listing[0],
            format!("{:014}__{}__{}.json", lamport, device_id, event_uuid),
            "filename layout must match the documented convention");
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
            "second push must not re-upload anything");
    }

    #[test]
    fn push_serialises_event_envelope_as_json() {
        // The wire format is the JSON-encoded `Event` struct. Verify
        // round-trip through the FakeWebDav store: PUT-as-JSON, GET,
        // parse, compare. Guards against accidental changes to the
        // envelope shape.
        let (db, fs) = setup();
        insert_session(&db, "2026-04-30T10:00:00", 600);
        let original_events: Vec<Event> = db.pending_events().unwrap()
            .into_iter().map(|(_, e)| e).collect();

        Sync::new(&db, &fs, "Meditate").push().unwrap();

        let listing = fs.list_collection("/Meditate/events/").unwrap();
        let body = fs.get(&format!("/Meditate/events/{}", listing[0])).unwrap();
        let parsed: Event = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed, original_events[0],
            "uploaded JSON must round-trip back to the same Event");
    }

    #[test]
    fn push_includes_payload_field_in_uploaded_json() {
        // Defensive: the payload string is what `apply_event` parses on
        // the receiving side, and a future `serde(skip)` mistake would
        // silently drop it. Pin its presence.
        let (db, fs) = setup();
        insert_session(&db, "2026-04-30T10:00:00", 600);
        Sync::new(&db, &fs, "Meditate").push().unwrap();
        let listing = fs.list_collection("/Meditate/events/").unwrap();
        let body = fs.get(&format!("/Meditate/events/{}", listing[0])).unwrap();
        let body_str = String::from_utf8(body).unwrap();
        assert!(body_str.contains("\"payload\""),
            "uploaded JSON must carry the payload field, got: {body_str}");
    }

    // ── Sync::pull ───────────────────────────────────────────────────────────

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
    fn pull_against_remote_with_no_files_is_a_noop() {
        // Remote dir exists but is empty (e.g. another device created
        // it via MKCOL but never PUT anything). Still nothing to do.
        let (db, fs) = setup();
        // Can't directly create a directory in our flat fake — but we
        // can simulate "exists with no children" by putting a file
        // elsewhere and then listing the events dir, which returns [].
        fs.put("/something/else.json", b"x").unwrap();
        let stats = Sync::new(&db, &fs, "Meditate").pull().unwrap();
        assert_eq!(stats.new_events, 0);
    }

    #[test]
    fn pull_fetches_events_uploaded_by_a_peer() {
        // Peer pushes; we pull; we have their state. The basic shape
        // of every cross-device sync.
        let (db_peer, fs) = setup();
        insert_session(&db_peer, "2026-04-30T10:00:00", 600);
        Sync::new(&db_peer, &fs, "Meditate").push().unwrap();

        let (db_us, _) = setup();
        let stats = Sync::new(&db_us, &fs, "Meditate").pull().unwrap();
        assert_eq!(stats.new_events, 1);
        let our_sessions = db_us.list_sessions().unwrap();
        assert_eq!(our_sessions.len(), 1);
        assert_eq!(our_sessions[0].1.start_iso, "2026-04-30T10:00:00");
    }

    #[test]
    fn pull_skips_events_already_in_local_log() {
        // Already-known event_uuids (from a prior pull, or our own
        // events that we previously pushed) must NOT be re-fetched.
        // The dedup check via known_event_uuids is the optimisation
        // that makes incremental sync cheap.
        let db = Database::open_in_memory().unwrap();
        let fs = FakeWebDav::new();
        // Author + push two sessions from us.
        insert_session(&db, "first",  100);
        insert_session(&db, "second", 200);
        Sync::new(&db, &fs, "Meditate").push().unwrap();
        // Now pull: we should see zero NEW events because all uploaded
        // files correspond to event_uuids we already authored.
        let stats = Sync::new(&db, &fs, "Meditate").pull().unwrap();
        assert_eq!(stats.new_events, 0,
            "pulling our own events back must be a no-op");
    }

    #[test]
    fn pull_is_idempotent_under_repeat_calls() {
        // Two pulls in a row must produce the same total state — no
        // double-application of any event.
        let (db_peer, fs) = setup();
        insert_session(&db_peer, "ping", 100);
        Sync::new(&db_peer, &fs, "Meditate").push().unwrap();

        let (db_us, _) = setup();
        Sync::new(&db_us, &fs, "Meditate").pull().unwrap();
        let after_first = db_us.list_sessions().unwrap().len();
        Sync::new(&db_us, &fs, "Meditate").pull().unwrap();
        let after_second = db_us.list_sessions().unwrap().len();
        assert_eq!(after_first, after_second);
        assert_eq!(after_first, 1);
    }

    #[test]
    fn pull_skips_files_with_unrecognised_filename_shapes() {
        // The events dir might contain a `snapshot.json` (compaction
        // artefact, future) or a stray file someone uploaded by hand.
        // Pull must not bail on these — just skip and move on.
        let (db, fs) = setup();
        fs.put("/Meditate/events/snapshot.json", b"{}").unwrap();
        fs.put("/Meditate/events/random_garbage", b"junk").unwrap();
        let stats = Sync::new(&db, &fs, "Meditate").pull().unwrap();
        assert_eq!(stats.new_events, 0);
    }

    #[test]
    fn pull_propagates_invalid_event_json_as_typed_error() {
        // A correctly-named but corrupt event file is a legit error
        // signal — surface it so the caller can log/notify, rather
        // than silently dropping events.
        let (db, fs) = setup();
        fs.put(
            "/Meditate/events/00000000000001__some-device__some-event.json",
            b"this is not JSON",
        ).unwrap();
        let err = Sync::new(&db, &fs, "Meditate").pull().unwrap_err();
        assert!(matches!(err, SyncError::InvalidEvent(_)),
            "corrupt remote event must surface as InvalidEvent, got {err:?}");
    }

    #[test]
    fn pull_after_peer_authors_advances_local_lamport() {
        // The Lamport observation rule must fire through the pull path
        // too: after pulling a peer's event with lamport=N, any local
        // event we author next must have lamport > N.
        let (db_peer, fs) = setup();
        // Bump peer's lamport up so its event isn't at lamport 1.
        for _ in 0..20 { db_peer.bump_lamport_clock().unwrap(); }
        insert_session(&db_peer, "peer-session", 100);
        let peer_lamport = db_peer.pending_events().unwrap()
            .iter().map(|(_, e)| e.lamport_ts).max().unwrap();
        Sync::new(&db_peer, &fs, "Meditate").push().unwrap();

        let (db_us, _) = setup();
        Sync::new(&db_us, &fs, "Meditate").pull().unwrap();
        // Now our local clock must be at least peer_lamport + 1 so
        // any subsequent local event sorts after the observed one.
        assert!(db_us.lamport_clock().unwrap() > peer_lamport,
            "local clock {} must exceed observed peer lamport {}",
            db_us.lamport_clock().unwrap(), peer_lamport);
    }

    #[test]
    fn push_uploads_in_lamport_order() {
        // pending_events orders by lamport_ts ASC; push iterates that
        // order. End result: filenames sort chronologically too — peers
        // can browse the events/ dir in the natural authoring order.
        let (db, fs) = setup();
        insert_session(&db, "first",  100);
        insert_session(&db, "second", 200);
        insert_session(&db, "third",  300);
        Sync::new(&db, &fs, "Meditate").push().unwrap();
        let mut listing = fs.list_collection("/Meditate/events/").unwrap();
        listing.sort();
        let lamports: Vec<&str> = listing.iter()
            .map(|n| n.split("__").next().unwrap())
            .collect();
        let mut sorted = lamports.clone();
        sorted.sort();
        assert_eq!(lamports, sorted,
            "filenames sort by their lamport prefix; chronological order on disk");
    }

    // ── Sync::sync — end-to-end two-device convergence ───────────────────────

    #[test]
    fn sync_against_empty_remote_pushes_local_events_first() {
        // First-device-online scenario: nothing upstream yet, we have
        // local writes. sync() should surface them via push.
        let (db, fs) = setup();
        insert_session(&db, "2026-04-30T10:00:00", 600);
        let stats = Sync::new(&db, &fs, "Meditate").sync().unwrap();
        assert_eq!(stats.pulled, 0,
            "empty remote → nothing to pull");
        assert_eq!(stats.pushed, 1,
            "local writes go up");
        assert_eq!(fs.file_count(), 1);
    }

    #[test]
    fn sync_two_devices_via_one_round_each_converges() {
        // Phone authors. Phone syncs (push). Laptop syncs (pull). Both
        // now have the same state. The classic "I author offline, sync
        // when online, you pull and have it" flow.
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
        // Both devices author offline, both sync. Each device's first
        // sync pushes its own work; the second sync pulls the other's.
        // After both have synced twice, both have everything.
        let (a_db, fs) = setup();
        let (b_db, _) = setup();
        insert_session(&a_db, "from A", 100);
        insert_session(&b_db, "from B", 200);

        // Round 1 — each pushes its own. Each pulls nothing because
        // the other hasn't pushed yet (the timing race we're testing).
        Sync::new(&a_db, &fs, "Meditate").sync().unwrap();
        Sync::new(&b_db, &fs, "Meditate").sync().unwrap();

        // Round 2 — each device sees the other's events upstream.
        let stats_a = Sync::new(&a_db, &fs, "Meditate").sync().unwrap();
        let stats_b = Sync::new(&b_db, &fs, "Meditate").sync().unwrap();
        assert_eq!(stats_a.pulled, 1, "A pulls B's event in round 2");
        // B pulled A's event in round 1 (A had pushed before B's sync),
        // so by round 2 it has nothing left to fetch.
        assert_eq!(stats_b.pulled, 0,
            "B already saw A's event in round 1 — nothing new in round 2");

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
        // A authors, both have the row, A deletes, A syncs, B syncs,
        // B no longer has the row. The end-to-end tombstone path.
        let (a_db, fs) = setup();
        let (b_db, _) = setup();
        let session_id = insert_session(&a_db, "to-be-deleted", 100);
        Sync::new(&a_db, &fs, "Meditate").sync().unwrap();
        Sync::new(&b_db, &fs, "Meditate").sync().unwrap();
        assert_eq!(b_db.list_sessions().unwrap().len(), 1,
            "sanity: B has the session after first sync");

        // A deletes and syncs.
        a_db.delete_session(session_id).unwrap();
        Sync::new(&a_db, &fs, "Meditate").sync().unwrap();
        assert!(a_db.list_sessions().unwrap().is_empty());

        // B pulls and the session is gone.
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

        // Pump syncs until convergence (max 4 rounds — 2 should suffice).
        for _ in 0..4 {
            Sync::new(&a_db, &fs, "Meditate").sync().unwrap();
            Sync::new(&b_db, &fs, "Meditate").sync().unwrap();
        }
        assert_eq!(a_db.list_sessions().unwrap().len(), 2);
        assert_eq!(b_db.list_sessions().unwrap().len(), 2);

        // Two more pumps after convergence: state stays the same.
        for _ in 0..2 {
            Sync::new(&a_db, &fs, "Meditate").sync().unwrap();
            Sync::new(&b_db, &fs, "Meditate").sync().unwrap();
        }
        assert_eq!(a_db.list_sessions().unwrap().len(), 2);
        assert_eq!(b_db.list_sessions().unwrap().len(), 2);
    }

    #[test]
    fn sync_propagates_label_renames_with_correct_winner() {
        // Two devices race to rename a label. After both sync twice,
        // both have the same name (the higher-(lamport, device_id)
        // event wins per the conflict-resolution rules from Phase B).
        let (a_db, fs) = setup();
        let (b_db, _) = setup();

        // A creates the label, both sync so both have it.
        let label_id_a = a_db.insert_label("Original").unwrap();
        Sync::new(&a_db, &fs, "Meditate").sync().unwrap();
        Sync::new(&b_db, &fs, "Meditate").sync().unwrap();
        let label_id_b = b_db.list_labels().unwrap()
            .iter().find(|l| l.name == "Original").map(|l| l.id).unwrap();

        // Both rename concurrently.
        a_db.update_label(label_id_a, "From A").unwrap();
        b_db.update_label(label_id_b, "From B").unwrap();

        // Two rounds of cross-sync to converge.
        for _ in 0..2 {
            Sync::new(&a_db, &fs, "Meditate").sync().unwrap();
            Sync::new(&b_db, &fs, "Meditate").sync().unwrap();
        }
        let a_name = a_db.list_labels().unwrap()[0].name.clone();
        let b_name = b_db.list_labels().unwrap()[0].name.clone();
        assert_eq!(a_name, b_name, "both devices must agree on the label name");
        assert!(a_name == "From A" || a_name == "From B",
            "winner must be one of the two renames, got `{a_name}`");
    }

    #[test]
    fn sync_pulls_before_pushing() {
        // The "I've seen everything you have, here's mine" semantics:
        // sync() = pull then push. If we pushed first, we might
        // emit events tagged with a stale lamport that ignores the
        // remote's clock. Verify the order by checking that a local
        // event authored AFTER pulling is tagged with a lamport_ts
        // that reflects the observation rule.
        let (peer_db, fs) = setup();
        // Peer's clock is artificially high.
        for _ in 0..50 { peer_db.bump_lamport_clock().unwrap(); }
        insert_session(&peer_db, "peer", 100);
        Sync::new(&peer_db, &fs, "Meditate").sync().unwrap();
        let peer_max_lamport = fs.list_collection("/Meditate/events/")
            .unwrap()
            .iter()
            .filter_map(|n| n.split("__").next())
            .filter_map(|s| s.parse::<i64>().ok())
            .max()
            .unwrap();

        // We're a fresh device that's never authored. Insert AFTER
        // syncing — sync() must pull peer's events first, observation
        // rule advances our clock, then our local insert at clock+1.
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

    #[test]
    fn sync_returns_combined_stats() {
        // After B pulls A's event, B's events table holds both A's
        // (received, synced=0) and B's own (authored, synced=0). The
        // next push uploads both — events forwarded through any device
        // that has them. Wasteful in bytes but resilient against
        // single-device data loss (any peer can re-seed the network).
        let (a_db, fs) = setup();
        let (b_db, _) = setup();
        insert_session(&a_db, "from A", 100);
        Sync::new(&a_db, &fs, "Meditate").sync().unwrap();

        insert_session(&b_db, "from B", 200);
        let stats = Sync::new(&b_db, &fs, "Meditate").sync().unwrap();
        assert_eq!(stats.pulled, 1, "B pulls A's one event");
        assert_eq!(stats.pushed, 2,
            "B re-uploads A's event AND its own — events ripple through peers");
    }
}
