use rusqlite::{params, Connection, OptionalExtension};
use std::io::{Read, Write};
use std::path::Path;

mod bell_sounds;
mod box_breath_phases;
mod device;
mod error;
mod guided_files;
mod interval_bells;
mod known_remote;
mod labels;
mod presets;
mod schema;
mod seeds;
mod settings;
mod sync_state;
mod vibration_patterns;

pub use bell_sounds::{BellSound, BellSoundCategory};
pub use box_breath_phases::{BoxBreathPhase, BoxBreathPhaseId};
pub use guided_files::GuidedFile;
pub use interval_bells::{IntervalBell, IntervalBellKind};
pub use labels::Label;
pub use presets::Preset;
pub use vibration_patterns::{ChartKind, VibrationPattern};
pub use error::{target_id_is_well_formed_for, DbError, Result};
pub use schema::{CACHE_SCHEMA_VERSION, CACHE_SCHEMA_VERSION_KEY, SCHEMA_VERSION};
use error::{conflict_suffixed_name, is_unique_constraint_error};
use schema::SCHEMA;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub start_iso: String,
    pub duration_secs: u32,
    pub label_id: Option<i64>,
    pub notes: Option<String>,
    pub mode: SessionMode,
    /// Stable cross-device identity, assigned by the DB at insert time.
    /// Callers may set this to `String::new()` before insert — the value
    /// is overwritten with a freshly generated v4 UUID. Always populated
    /// on read paths.
    pub uuid: String,
    /// Set on guided meditation rows that played a library-stored file
    /// (i.e. an entry in `guided_files`). `None` for non-Guided modes
    /// AND for transient one-off guided sessions where the user played
    /// a file without importing it. Lets stats and the log surface
    /// per-file aggregates on top of the per-mode breakdown.
    pub guided_file_uuid: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionMode {
    /// Generic timer session — covers both targeted countdowns and
    /// open-ended (stopwatch) runs. The distinction lives at the UI
    /// level (`current_target_secs: Option<u32>`) and isn't persisted:
    /// stats and the log already key off the recorded duration alone.
    Timer,
    BoxBreath,
    /// Guided meditation — the user picks an audio file (transient
    /// "Open File" or imported into the library); the session length
    /// is the file's natural duration. Pause / Stop / Add overtime
    /// mirror the Timer countdown's running view.
    Guided,
}

impl SessionMode {
    /// On-disk and CSV string representation. Exposed so callers
    /// (CSV import/export, debug logging) don't need to re-implement
    /// this match against the enum.
    pub fn as_db_str(self) -> &'static str {
        match self {
            SessionMode::Timer => "timer",
            SessionMode::BoxBreath => "box_breath",
            SessionMode::Guided => "guided",
        }
    }

    /// Inverse of `as_db_str`. Returns `None` for unknown / typo'd
    /// values; callers decide whether to hard-error or treat the row
    /// as corrupt. The DB column carries a CHECK constraint matching
    /// these strings, so reads off the local DB cannot legitimately
    /// hit the None branch — only out-of-band data (sync wire format,
    /// CSV import, hand-edited rows) needs to think about it.
    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "timer" => Some(SessionMode::Timer),
            "box_breath" => Some(SessionMode::BoxBreath),
            "guided" => Some(SessionMode::Guided),
            _ => None,
        }
    }
}

/// One audio file in the bell-sound library — bundled CC0 sounds the
/// app ships with, plus user-imported custom files. Referenced by
/// every bell-fire site (starting bell, interval bells, completion
/// sound) via the `uuid` column. The `is_bundled` flag distinguishes
/// what the audio system does with `file_path`: bundled rows hold a
/// GResource path the binary contains; custom rows hold a filesystem
/// path under `$XDG_DATA_HOME`. Bundled rows ride sync (so a peer
/// without the bundle inherits the same UUIDs from the seeding device)
/// but the audio itself doesn't — peers compile in their own copy.
///
/// `category` partitions the library by usage context: General sounds
/// (bells / gongs / chimes) feed the Starting / Interval / End bell
/// choosers; BoxBreath sounds (voice cues / phase markers) feed the
/// One configured bell entry in the user's interval-bell library.
/// What channels a bell or phase plays through. Mirrors the
/// Sound / Vibration / Both `Adw.ToggleGroup` segments in the
/// timer setup. Used as the persisted enum behind every per-bell
/// signal-mode setting key + the `interval_bells.signal_mode`
/// column + the `box_breath_phases.signal_mode` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalMode {
    Sound,
    Vibration,
    Both,
}

/// One row in `box_breath_phases` — the per-phase cue config for
impl SignalMode {
    pub fn as_db_str(self) -> &'static str {
        match self {
            SignalMode::Sound     => "sound",
            SignalMode::Vibration => "vibration",
            SignalMode::Both      => "both",
        }
    }
    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "sound"     => Some(SignalMode::Sound),
            "vibration" => Some(SignalMode::Vibration),
            "both"      => Some(SignalMode::Both),
            _           => None,
        }
    }

    /// Does this mode include the sound channel? `Sound | Both`.
    pub fn includes_sound(self) -> bool {
        matches!(self, SignalMode::Sound | SignalMode::Both)
    }

    /// Does this mode include the vibration channel? `Vibration | Both`.
    pub fn includes_vibration(self) -> bool {
        matches!(self, SignalMode::Vibration | SignalMode::Both)
    }
}


/// One entry in the append-only sync event log. A self-contained
/// description of a state-changing operation — sessions inserted /
/// updated / deleted, labels renamed, settings changed. Every field
/// is part of the cross-device identity or ordering contract:
///
/// - `event_uuid` is the dedup key. Receiving the same uuid twice
///   (retry, peer-forwarding) is a silent no-op.
/// - `lamport_ts` orders events; ties break on `device_id` per the
///   conflict-resolution rules.
/// - `device_id` records authorship.
/// - `kind` is the event type (e.g. `"session_insert"`); `payload`
///   is its JSON-encoded specifics. Both opaque at this layer.
/// - `target_id` denormalises the affected row's cross-device identity
///   (session/label uuid, or setting key) so replay queries can scan
///   "all events for X" without parsing JSON in SQL.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Event {
    pub event_uuid: String,
    pub lamport_ts: i64,
    pub device_id: String,
    pub kind: String,
    pub target_id: String,
    /// On-wire format is the JSON-encoded event body (e.g. session
    /// fields). Stored locally as a string so SQLite doesn't need a
    /// JSON-aware projection; the recompute helpers parse it on demand.
    /// Serialising the envelope as JSON gives JSON-in-JSON on the wire,
    /// which is uglier than nesting but trivially round-trips through
    /// `serde_json::to_vec` / `from_slice` without any custom shape.
    pub payload: String,
}

/// Pagination + filter for `query_sessions`. Default-constructed value
/// matches every session with no pagination.
#[derive(Debug, Clone, Default)]
pub struct SessionFilter {
    /// Only sessions referencing this label id. `None` ⇒ every label
    /// (and unlabeled).
    pub label_id: Option<i64>,
    /// Only sessions with a non-empty `notes` field.
    pub only_with_notes: bool,
    /// Hard cap on returned rows. `None` ⇒ no cap.
    pub limit: Option<u32>,
    /// Skip the first `offset` rows of the (filtered, ordered) result.
    /// `None` ⇒ no skip.
    pub offset: Option<u32>,
}

pub struct Database {
    conn: Connection,
}

/// Mint a fresh v4 UUID. Exposed so the shell (which doesn't have
/// the `uuid` crate as a direct dep) can generate UUIDs for places
/// where the id has to be known before the row is created — e.g.,
/// the custom-bell-import path that needs a UUID for the destination
/// filename before the DB insert.
pub fn mint_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}


impl Database {
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init(conn)
    }

    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        // For on-disk databases, enable WAL with synchronous=NORMAL.
        // The default (rollback journal + synchronous=FULL) does a
        // full fsync on every commit — autocommit UPDATEs become
        // ~50–200 ms each on phone eMMC, which bottlenecks any
        // hot-loop write. WAL+NORMAL fsyncs only on checkpoint and
        // the WAL header on commit, two orders of magnitude cheaper.
        // Durability tradeoff: a power loss between commit and
        // checkpoint may roll back a small number of recently
        // committed transactions. Acceptable here — events are
        // append-only and idempotent on re-sync.
        //
        // In-memory `open_in_memory` skips this — WAL on `:memory:` is
        // a no-op (the journal is also in memory) and synchronous
        // doesn't apply.
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        // Refuse to open a DB whose user_version exceeds what this
        // build knows how to read — a downgrade from a future build
        // could otherwise drop forward-only data silently. A fresh
        // DB reads user_version=0 and falls through to the stamp.
        let db_version: u32 = conn.query_row(
            "PRAGMA user_version", [], |row| row.get(0),
        )?;
        if db_version > SCHEMA_VERSION {
            return Err(DbError::SchemaVersionTooNew {
                db: db_version,
                build: SCHEMA_VERSION,
            });
        }
        // Explicit PRAGMAs — even when rusqlite enables them by default,
        // the intent is part of the source so it can't be silently
        // dropped by a dependency upgrade. The FK clause on
        // sessions.label_id only fires when this is ON.
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        conn.execute_batch(SCHEMA)?;
        // Stamp the current version. `execute_batch` is required because
        // PRAGMA values aren't bindable via params.
        conn.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION};"))?;
        // One-shot integrity check. `quick_check` returns "ok" on a
        // clean DB or one or more issue lines on corruption — we log
        // the first non-ok line via diag and continue, since refusing
        // to open would lock the user out without a recovery path.
        let integrity: String = conn.query_row(
            "PRAGMA quick_check", [], |row| row.get(0),
        ).unwrap_or_else(|_| "ok".to_string());
        if integrity != "ok" {
            crate::diag::log(&format!("db_integrity_check_failed: {integrity}"));
        }
        let db = Self { conn };
        db.maybe_walk_events_for_cache_upgrade()?;
        Ok(db)
    }

    /// If the stored cache-schema version is below `CACHE_SCHEMA_VERSION`,
    /// replay every event so any kind that this build understands but
    /// the previous build skipped (apply_event_inner's "unknown kind"
    /// branch) gets materialised into the cache. Idempotent: re-applying
    /// understood kinds is a no-op (their dispatchers are UPSERT-shaped),
    /// and lamport_clock isn't bumped because the events are not "fresh
    /// from a peer" (was_new=false on re-record).
    fn maybe_walk_events_for_cache_upgrade(&self) -> Result<()> {
        let stored = self
            .get_sync_state(CACHE_SCHEMA_VERSION_KEY, "0")?
            .parse::<u32>()
            .unwrap_or(0);
        if stored >= CACHE_SCHEMA_VERSION {
            return Ok(());
        }
        let tx = self.conn.unchecked_transaction()?;
        let events = {
            let mut stmt = self.conn.prepare(
                "SELECT event_uuid, lamport_ts, device_id, kind, target_id, payload
                 FROM events
                 ORDER BY lamport_ts ASC, device_id ASC",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(Event {
                    event_uuid: row.get(0)?,
                    lamport_ts: row.get(1)?,
                    device_id: row.get(2)?,
                    kind: row.get(3)?,
                    target_id: row.get(4)?,
                    payload: row.get(5)?,
                })
            })?;
            let mut out: Vec<Event> = Vec::new();
            for r in rows {
                out.push(r?);
            }
            out
        };
        let count = events.len();
        for event in events {
            self.apply_event_inner(&event)?;
        }
        self.set_sync_state(
            CACHE_SCHEMA_VERSION_KEY,
            &CACHE_SCHEMA_VERSION.to_string(),
        )?;
        tx.commit()?;
        if count > 0 {
            crate::diag::log(&format!(
                "cache_schema_upgrade: replayed {count} events from v{stored} to v{}",
                CACHE_SCHEMA_VERSION,
            ));
        }
        Ok(())
    }

    /// Append an event to the sync log. Returns the local rowid (the
    /// cache key inside this device — distinct from `event.event_uuid`,
    /// the cross-device identity). A second append with an
    /// `event_uuid` already present is a silent no-op; this makes
    /// delivery at-most-once on the local cache regardless of retries
    /// or peer forwarding.
    pub fn append_event(&self, event: &Event) -> Result<i64> {
        Ok(self.append_event_returning_newness(event)?.0)
    }

    /// Like `append_event` but also tells the caller whether the row
    /// was actually new (vs. silently ignored as a dup). `apply_event`
    /// uses this to avoid re-bumping the Lamport clock on a duplicate
    /// observation — the Lamport rule fires once per *new* observation,
    /// not once per call.
    fn append_event_returning_newness(&self, event: &Event) -> Result<(i64, bool)> {
        // INSERT OR IGNORE handles the dedup case without raising the
        // UNIQUE-constraint error to the caller. The number of rows
        // changed tells us which branch SQLite took: 1 = inserted,
        // 0 = ignored due to existing UNIQUE event_uuid.
        let rows_changed = self.conn.execute(
            "INSERT OR IGNORE INTO events
                (event_uuid, lamport_ts, device_id, kind, target_id, payload)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                event.event_uuid,
                event.lamport_ts,
                event.device_id,
                event.kind,
                event.target_id,
                event.payload,
            ],
        )?;
        let was_new = rows_changed > 0;
        let rowid = self.conn.query_row(
            "SELECT id FROM events WHERE event_uuid = ?1",
            params![event.event_uuid],
            |row| row.get::<_, i64>(0),
        )?;
        Ok((rowid, was_new))
    }

    /// All events not yet pushed to remote, ordered by `lamport_ts` ASC
    /// (then by local `id` as a stable tie-break). Sync's push phase
    /// drains this list in order; mark each entry with `mark_event_synced`
    /// once the WebDAV PUT succeeds.
    pub fn pending_events(&self) -> Result<Vec<(i64, Event)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, event_uuid, lamport_ts, device_id, kind, target_id, payload
             FROM events
             WHERE synced = 0
             ORDER BY lamport_ts ASC, id ASC",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    Event {
                        event_uuid: row.get(1)?,
                        lamport_ts: row.get(2)?,
                        device_id: row.get(3)?,
                        kind: row.get(4)?,
                        target_id: row.get(5)?,
                        payload: row.get(6)?,
                    },
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Every `event_uuid` we've seen, in a HashSet for fast existence
    /// checks. Sync's pull phase uses this to diff against a remote
    /// listing — only events we don't have get GETted. Cheap up to
    /// the order of (event count) — fine for personal use sizes.
    pub fn known_event_uuids(&self) -> Result<std::collections::HashSet<String>> {
        let mut stmt = self.conn.prepare("SELECT event_uuid FROM events")?;
        let ids = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<std::collections::HashSet<_>>>()?;
        Ok(ids)
    }

    /// Flip the `synced` flag on the event with this local rowid so it
    /// drops out of `pending_events`. Unknown ids are silently no-ops —
    /// SQLite's UPDATE-on-no-match behaviour, exposed verbatim so a
    /// stale id from a partial sync doesn't escalate to an error.
    pub fn mark_event_synced(&self, id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE events SET synced = 1 WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    /// Reset the synced flag on every event row to 0, putting all of
    /// them back into `pending_events`. Used by the "push local up"
    /// recovery path when the user has resolved a remote-data-lost
    /// prompt by re-uploading their local state — the next push must
    /// see every authored event as pending so it can bundle them all
    /// into a fresh batch file.
    ///
    /// Scoped to the events table only. The caller is responsible for
    /// also calling `wipe_known_remote_files` (so the dedup tracker
    /// doesn't claim the freshly-emptied remote already has them).
    pub fn flag_all_events_unsynced(&self) -> Result<()> {
        self.conn.execute("UPDATE events SET synced = 0", [])?;
        Ok(())
    }

    /// Erase every user-content row plus the dedup tracker, preserving
    /// settings, sync_state, and the device row (id + lamport clock).
    /// Used by the "wipe local to match remote" recovery path: the
    /// user has resolved a remote-data-lost prompt by saying "the
    /// authoritative state is the empty remote — drop my local copy."
    ///
    /// All four DELETEs run inside one transaction so the wipe is
    /// atomic — a crash mid-wipe leaves the DB in either the pre-wipe
    /// state or the post-wipe state, never half-and-half. Settings
    /// (end-sound, weekly goal, etc.) and sync_state (URL, username,
    /// last-sync timestamp) are deliberately preserved: they're not
    /// part of the event-log content the user is discarding.
    pub fn wipe_local_event_log(&self) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM known_remote_files", [])?;
        tx.execute("DELETE FROM known_remote_sounds", [])?;
        tx.execute("DELETE FROM events", [])?;
        tx.execute("DELETE FROM sessions", [])?;
        tx.execute("DELETE FROM labels", [])?;
        tx.execute("DELETE FROM bell_sounds", [])?;
        tx.execute("DELETE FROM interval_bells", [])?;
        tx.execute("DELETE FROM presets", [])?;
        tx.execute("DELETE FROM guided_files", [])?;
        tx.execute("DELETE FROM vibration_patterns", [])?;
        tx.execute("DELETE FROM box_breath_phases", [])?;
        for phase in BoxBreathPhaseId::all() {
            tx.execute(
                "INSERT OR IGNORE INTO box_breath_phases (phase) VALUES (?1)",
                params![phase.as_db_str()],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Same as `mark_event_synced`, but for a batch of ids in a single
    /// transaction. Used by the bulk-push path: after one PUT covers
    /// all pending events, a single transaction flips the synced flag
    /// on every contained event_id. Marking N rows one-at-a-time would
    /// fire N autocommit fsyncs; this batches them into the WAL's
    /// usual one-fsync-per-commit. Empty input is a no-op.
    pub fn mark_events_synced(&self, ids: &[i64]) -> Result<()> {
        if ids.is_empty() { return Ok(()); }
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare(
                "UPDATE events SET synced = 1 WHERE id = ?1")?;
            for id in ids {
                stmt.execute(params![id])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Emit a locally-authored event: bumps the Lamport clock, mints a
    /// fresh `event_uuid`, tags with this device's id, and appends to the
    /// log. Mutation methods call this AFTER the data write inside a
    /// shared transaction so the cache row and its event commit atomically.
    /// `target_id` is the affected row's cross-device identity (session
    /// or label uuid, or setting key) — denormalised onto the event so
    /// replay queries don't need to parse the JSON payload.
    fn emit_event(&self, kind: &str, target_id: &str, payload: String) -> Result<()> {
        let device_id = self.device_id()?;
        let lamport_ts = self.bump_lamport_clock()?;
        let event = Event {
            event_uuid: uuid::Uuid::new_v4().to_string(),
            lamport_ts,
            device_id,
            kind: kind.to_string(),
            target_id: target_id.to_string(),
            payload,
        };
        self.append_event(&event)?;
        Ok(())
    }

    /// Look up a label's cross-device UUID by its local rowid. Used at
    /// event-emission time to translate from the cache key (rowid) to
    /// the cross-device identity. Errors when the rowid is unknown —
    /// callers should already have validated this via the FK constraint
    /// or by reading the row's `label_id` from a known-good source.
    fn label_uuid_by_id(&self, id: i64) -> Result<String> {
        Ok(self.conn.query_row(
            "SELECT uuid FROM labels WHERE id = ?1",
            params![id],
            |row| row.get::<_, String>(0),
        )?)
    }

    /// Apply a single event to the materialized cache. Idempotent on
    /// `event.event_uuid` (a duplicate is a silent no-op). Order-
    /// independent: out-of-order delivery converges because the cache
    /// is recomputed from MAX-lamport queries against the events table,
    /// not from incremental application of just-this-event's payload.
    ///
    /// Conflict-resolution rules (per Nextcloud-Sync.md):
    /// - Same event observed twice → idempotent.
    /// - Two devices update same target → higher `lamport_ts` wins;
    ///   tie breaks on lex-larger `device_id`.
    /// - Update + delete on same target → delete wins on tie (≥).
    /// - Insert + delete out of order → tombstone wins if its lamport
    ///   ≥ the mutate's, regardless of arrival sequence.
    pub fn apply_event(&self, event: &Event) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        self.apply_event_inner(event)?;
        tx.commit()?;
        Ok(())
    }

    /// The transaction-less core of `apply_event`. Extracted so
    /// `replay_events` can apply many events under a single outer
    /// transaction — opening a SAVEPOINT per event would be correct but
    /// pointlessly slow.
    fn apply_event_inner(&self, event: &Event) -> Result<()> {
        // Record first — the recompute query reads from events, so the
        // freshly-arrived event needs to be visible.
        let (_, was_new) = self.append_event_returning_newness(event)?;

        // Lamport's observation rule: when we accept a fresh event from
        // a peer, advance our local clock to `max(local, remote) + 1`
        // so any event we author next strictly orders after the one we
        // just observed. We skip this for our own device's events
        // (re-applying our own event must not bump the clock — that
        // would break the idempotency the user-facing API depends on)
        // and for duplicates (we already observed this one).
        if was_new && event.device_id != self.device_id()? {
            self.observe_remote_lamport(event.lamport_ts)?;
        }

        // Validate target_id BEFORE dispatch. A malicious peer could
        // ship a bell_sound_insert with target_id="../../../etc/passwd",
        // which would write that string into bell_sounds.uuid and then
        // be used as a path component in pull_custom_sound_files —
        // arbitrary file-write primitive. Skip the dispatch on invalid
        // target_id but keep the event row recorded; future-compat
        // peers may have a different validator, and skipping is
        // soft-fail-don't-rollback so one bad event doesn't poison
        // the whole replay batch.
        if !target_id_is_well_formed_for(&event.kind, &event.target_id) {
            crate::diag::log(&format!(
                "apply_event_inner: rejected event kind={} target_id={:?} (invalid)",
                event.kind, event.target_id,
            ));
            return Ok(());
        }

        match event.kind.as_str() {
            "session_insert" | "session_update" | "session_delete" => {
                self.recompute_session(&event.target_id)?;
            }
            "label_insert" | "label_rename" | "label_delete" => {
                self.recompute_label(&event.target_id)?;
            }
            "interval_bell_insert" | "interval_bell_update" | "interval_bell_delete" => {
                self.recompute_interval_bell(&event.target_id)?;
            }
            "bell_sound_insert" | "bell_sound_update" | "bell_sound_delete" => {
                self.recompute_bell_sound(&event.target_id)?;
            }
            "preset_insert" | "preset_update" | "preset_delete" => {
                self.recompute_preset(&event.target_id)?;
            }
            "guided_file_insert" | "guided_file_update" | "guided_file_delete" => {
                self.recompute_guided_file(&event.target_id)?;
            }
            "vibration_pattern_insert"
            | "vibration_pattern_update"
            | "vibration_pattern_delete" => {
                self.recompute_vibration_pattern(&event.target_id)?;
            }
            "box_breath_phase_update" => {
                self.recompute_box_breath_phase(&event.target_id)?;
            }
            "setting_changed" => {
                self.recompute_setting(&event.target_id)?;
            }
            _ => {
                // Unknown kind — record for forwards-compat (a later
                // build may know how to apply it) but don't mutate the
                // cache from a payload shape we don't understand.
            }
        }
        Ok(())
    }

    /// Apply a batch of events to the materialized cache. Events are
    /// sorted by `(lamport_ts, device_id, event_uuid)` for a stable
    /// deterministic order before dispatch — this matches the canonical
    /// replay order across peers (the plan's tie-break rule). The whole
    /// batch runs inside one transaction so a partial failure rolls back.
    /// Idempotent on `event_uuid`: repeat calls with the same input are
    /// no-ops on the cache.
    pub fn replay_events(&self, events: &[Event]) -> Result<()> {
        if events.is_empty() { return Ok(()); }
        let tx = self.conn.unchecked_transaction()?;
        let mut sorted: Vec<&Event> = events.iter().collect();
        sorted.sort_by(|a, b| {
            a.lamport_ts.cmp(&b.lamport_ts)
                .then_with(|| a.device_id.cmp(&b.device_id))
                .then_with(|| a.event_uuid.cmp(&b.event_uuid))
        });
        for event in sorted {
            self.apply_event_inner(event)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Recompute the `sessions` row for `session_uuid` from the events
    /// table: tombstone wins if its lamport ≥ the latest mutate; else
    /// the highest-lamport mutate event drives the row's values
    /// (tie-breaking on lex-larger device_id).
    fn recompute_session(&self, session_uuid: &str) -> Result<()> {
        let delete_ts: Option<i64> = self.conn.query_row(
            "SELECT MAX(lamport_ts) FROM events
             WHERE target_id = ?1 AND kind = 'session_delete'",
            params![session_uuid],
            |row| row.get::<_, Option<i64>>(0),
        )?;

        // Latest mutate event by (lamport_ts, device_id) DESC. We pull
        // the payload too, since it carries the field values to write.
        let mutate: Option<(i64, String)> = self.conn.query_row(
            "SELECT lamport_ts, payload FROM events
             WHERE target_id = ?1
               AND kind IN ('session_insert', 'session_update')
             ORDER BY lamport_ts DESC, device_id DESC
             LIMIT 1",
            params![session_uuid],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        ).optional()?;

        let row_should_exist = match (mutate.as_ref(), delete_ts) {
            (Some(_), None) => true,
            (None, _) => false,
            // Delete wins on tie: only mutate > delete keeps the row.
            (Some((m_ts, _)), Some(d_ts)) => *m_ts > d_ts,
        };

        if let Some((_, payload)) = mutate.filter(|_| row_should_exist) {
            let v: serde_json::Value = serde_json::from_str(&payload)
                .map_err(|e| DbError::Csv(
                    format!("session event payload not valid JSON: {e}")))?;
            let start_iso = v["start_iso"].as_str().unwrap_or_default();
            let duration_secs = v["duration_secs"].as_u64().unwrap_or(0) as u32;
            let label_uuid = v["label_uuid"].as_str();
            let label_id: Option<i64> = match label_uuid {
                Some(luuid) => self.conn.query_row(
                    "SELECT id FROM labels WHERE uuid = ?1",
                    params![luuid],
                    |row| row.get::<_, i64>(0),
                ).optional()?,
                None => None,
            };
            let notes = v["notes"].as_str();
            let mode = v["mode"].as_str().unwrap_or("timer");
            // Optional — old payloads (pre-guided-meditation feature)
            // and non-Guided sessions don't carry this key. as_str()
            // returns None for both missing-key and null-value cases.
            let guided_file_uuid = v["guided_file_uuid"].as_str();

            // UPSERT — first time materialising creates the row, later
            // recomputes overwrite every field with the winning event's
            // values. The local rowid stays stable across recomputes
            // because the UNIQUE column we conflict on is `uuid`.
            self.conn.execute(
                "INSERT INTO sessions (uuid, start_iso, duration_secs, label_id, notes, mode, guided_file_uuid)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(uuid) DO UPDATE SET
                    start_iso        = excluded.start_iso,
                    duration_secs    = excluded.duration_secs,
                    label_id         = excluded.label_id,
                    notes            = excluded.notes,
                    mode             = excluded.mode,
                    guided_file_uuid = excluded.guided_file_uuid",
                params![session_uuid, start_iso, duration_secs, label_id, notes, mode, guided_file_uuid],
            )?;
        } else {
            // Tombstoned (or no mutate event yet) → ensure absent.
            self.conn.execute(
                "DELETE FROM sessions WHERE uuid = ?1",
                params![session_uuid],
            )?;
        }
        Ok(())
    }




    pub fn count_sessions(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))?)
    }

    pub fn insert_session(&self, session: &Session) -> Result<i64> {
        let tx = self.conn.unchecked_transaction()?;
        let session_uuid = uuid::Uuid::new_v4().to_string();
        self.conn.execute(
            "INSERT INTO sessions (start_iso, duration_secs, label_id, notes, mode, uuid, guided_file_uuid)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                session.start_iso,
                session.duration_secs,
                session.label_id,
                session.notes,
                session.mode.as_db_str(),
                session_uuid,
                session.guided_file_uuid,
            ],
        )?;
        let rowid = self.conn.last_insert_rowid();

        // Translate label_id (local rowid) → label_uuid (cross-device).
        // The peer applying this event has a different rowid space.
        let label_uuid = match session.label_id {
            Some(id) => Some(self.label_uuid_by_id(id)?),
            None => None,
        };
        let payload = serde_json::json!({
            "uuid": session_uuid,
            "start_iso": session.start_iso,
            "duration_secs": session.duration_secs,
            "label_uuid": label_uuid,
            "notes": session.notes,
            "mode": session.mode.as_db_str(),
            "guided_file_uuid": session.guided_file_uuid,
        }).to_string();
        self.emit_event("session_insert", &session_uuid, payload)?;

        tx.commit()?;
        Ok(rowid)
    }

    /// Insert many sessions inside a single transaction — orders of
    /// magnitude faster than calling `insert_session` in a loop. Atomic:
    /// if any row fails a constraint, the whole batch is rolled back and
    /// the caller never sees a partially-imported DB. Each row also
    /// emits its own `session_insert` event — peers replay them
    /// independently, there is no "bulk" event kind.
    pub fn bulk_insert_sessions(&self, sessions: &[Session]) -> Result<usize> {
        let tx = self.conn.unchecked_transaction()?;
        // Cache each row's freshly-minted uuid so we can build the event
        // payload without re-reading from disk after the INSERT.
        let mut session_uuids: Vec<String> = Vec::with_capacity(sessions.len());
        {
            let mut stmt = tx.prepare(
                "INSERT INTO sessions (start_iso, duration_secs, label_id, notes, mode, uuid, guided_file_uuid)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for s in sessions {
                let uuid = uuid::Uuid::new_v4().to_string();
                stmt.execute(params![
                    s.start_iso,
                    s.duration_secs,
                    s.label_id,
                    s.notes,
                    s.mode.as_db_str(),
                    uuid,
                    s.guided_file_uuid,
                ])?;
                session_uuids.push(uuid);
            }
        }
        for (s, session_uuid) in sessions.iter().zip(session_uuids) {
            let label_uuid = match s.label_id {
                Some(id) => Some(self.label_uuid_by_id(id)?),
                None => None,
            };
            let payload = serde_json::json!({
                "uuid": session_uuid,
                "start_iso": s.start_iso,
                "duration_secs": s.duration_secs,
                "label_uuid": label_uuid,
                "notes": s.notes,
                "mode": s.mode.as_db_str(),
                "guided_file_uuid": s.guided_file_uuid,
            }).to_string();
            self.emit_event("session_insert", &session_uuid, payload)?;
        }
        tx.commit()?;
        Ok(sessions.len())
    }

    /// Remove the row with `id`. Unknown ids are silently no-ops AND
    /// emit no event — otherwise peers would see a tombstone for a
    /// session they never knew existed.
    pub fn delete_session(&self, id: i64) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        // Look up the uuid before deleting — the row's gone after the
        // DELETE, but the event needs the cross-device identity.
        let row_uuid: Option<String> = self.conn.query_row(
            "SELECT uuid FROM sessions WHERE id = ?1",
            params![id],
            |row| row.get::<_, String>(0),
        ).optional()?;
        let Some(uuid) = row_uuid else {
            // Unknown id → no row to delete, no event to emit.
            return Ok(());
        };
        self.conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        let payload = serde_json::json!({ "uuid": uuid }).to_string();
        self.emit_event("session_delete", &uuid, payload)?;
        tx.commit()?;
        Ok(())
    }

    /// Remove every session row. Returns how many rows were deleted.
    /// Labels and settings are untouched. Emits one `session_delete`
    /// event per row that was actually present, so peers tombstone the
    /// same set we cleared locally.
    pub fn delete_all_sessions(&self) -> Result<usize> {
        let tx = self.conn.unchecked_transaction()?;
        // Capture the uuids before the DELETE — afterwards there's
        // nothing to read.
        let row_uuids: Vec<String> = {
            let mut stmt = self.conn.prepare("SELECT uuid FROM sessions")?;
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };
        let n = self.conn.execute("DELETE FROM sessions", [])?;
        for uuid in &row_uuids {
            let payload = serde_json::json!({ "uuid": uuid }).to_string();
            self.emit_event("session_delete", uuid, payload)?;
        }
        tx.commit()?;
        Ok(n)
    }

    /// Replace every field of the row with `id`. Unknown ids are silently
    /// no-ops AND emit no event — peers would otherwise receive an update
    /// referencing a uuid we don't have.
    pub fn update_session(&self, id: i64, session: &Session) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        // Resolve the existing row's uuid first; absence means "unknown
        // id" and we drop out without writing or logging.
        let row_uuid: Option<String> = self.conn.query_row(
            "SELECT uuid FROM sessions WHERE id = ?1",
            params![id],
            |row| row.get::<_, String>(0),
        ).optional()?;
        let Some(session_uuid) = row_uuid else {
            return Ok(());
        };
        self.conn.execute(
            "UPDATE sessions
             SET start_iso = ?1, duration_secs = ?2, label_id = ?3,
                 notes = ?4, mode = ?5, guided_file_uuid = ?6
             WHERE id = ?7",
            params![
                session.start_iso,
                session.duration_secs,
                session.label_id,
                session.notes,
                session.mode.as_db_str(),
                session.guided_file_uuid,
                id,
            ],
        )?;
        let label_uuid = match session.label_id {
            Some(id) => Some(self.label_uuid_by_id(id)?),
            None => None,
        };
        let payload = serde_json::json!({
            "uuid": session_uuid,
            "start_iso": session.start_iso,
            "duration_secs": session.duration_secs,
            "label_uuid": label_uuid,
            "notes": session.notes,
            "mode": session.mode.as_db_str(),
            "guided_file_uuid": session.guided_file_uuid,
        }).to_string();
        self.emit_event("session_update", &session_uuid, payload)?;
        tx.commit()?;
        Ok(())
    }

    pub fn get_best_streak(&self) -> Result<i64> {
        self.best_streak_filtered(None)
    }

    pub fn get_best_streak_for_label(&self, label_id: i64) -> Result<i64> {
        self.best_streak_filtered(Some(label_id))
    }

    fn best_streak_filtered(&self, label_filter: Option<i64>) -> Result<i64> {
        let days = self.distinct_session_days_ascending(label_filter)?;
        if days.is_empty() {
            return Ok(0);
        }
        let mut best = 1i64;
        let mut current = 1i64;
        for window in days.windows(2) {
            if window[1] == window[0].succ_opt().expect("date overflow") {
                current += 1;
                best = best.max(current);
            } else {
                current = 1;
            }
        }
        Ok(best)
    }

    pub fn import_sessions_csv<R: Read>(&self, reader: R) -> Result<usize> {
        let mut rdr = csv::Reader::from_reader(reader);
        let mut count = 0;
        for record in rdr.records() {
            let record = record.map_err(|e| DbError::Csv(e.to_string()))?;
            let start_iso = record
                .get(0)
                .ok_or_else(|| DbError::Csv("missing start_iso".to_string()))?
                .to_string();
            let duration_secs: u32 = record
                .get(1)
                .unwrap_or("")
                .parse()
                .map_err(|_| DbError::Csv("bad duration_secs".to_string()))?;
            let label = record
                .get(2)
                .map(str::to_string)
                .filter(|s| !s.is_empty());
            let notes = record
                .get(3)
                .map(str::to_string)
                .filter(|s| !s.is_empty());
            let mode_str = record.get(4).unwrap_or("timer");
            let mode = SessionMode::from_db_str(mode_str)
                .ok_or_else(|| DbError::Csv(format!("unknown mode: {mode_str}")))?;

            let label_id = match label {
                Some(name) => Some(self.find_or_create_label(&name)?),
                None => None,
            };

            self.insert_session(&Session {
                start_iso,
                duration_secs,
                label_id,
                notes,
                mode,
                uuid: String::new(),
                guided_file_uuid: None,
            })?;
            count += 1;
        }
        Ok(count)
    }

    pub fn export_sessions_csv<W: Write>(&self, writer: W) -> Result<()> {
        let mut wtr = csv::Writer::from_writer(writer);
        wtr.write_record(["start_iso", "duration_secs", "label", "notes", "mode"])
            .map_err(|e| DbError::Csv(e.to_string()))?;

        let mut stmt = self.conn.prepare(
            "SELECT s.start_iso, s.duration_secs, l.name, s.notes, s.mode
             FROM sessions s
             LEFT JOIN labels l ON s.label_id = l.id
             ORDER BY s.id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, u32>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;
        for row in rows {
            let (start, dur, label, notes, mode) = row?;
            wtr.write_record([
                &start,
                &dur.to_string(),
                label.as_deref().unwrap_or(""),
                notes.as_deref().unwrap_or(""),
                &mode,
            ])
            .map_err(|e| DbError::Csv(e.to_string()))?;
        }
        wtr.flush().map_err(|e| DbError::Csv(e.to_string()))?;
        Ok(())
    }

    /// Lower-median session duration in seconds, or `None` when the
    /// sessions table is empty. The `Option` distinguishes "no data
    /// to report" from a real zero — useful for any shell rendering
    /// "n/a" vs "0s".
    pub fn get_median_duration_secs(&self) -> Result<Option<u32>> {
        let mut stmt = self
            .conn
            .prepare("SELECT duration_secs FROM sessions ORDER BY duration_secs")?;
        let durations: Vec<u32> = stmt
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if durations.is_empty() {
            return Ok(None);
        }
        // Lower-median: index (len-1)/2 hits the lower middle on even counts,
        // and the exact middle on odd counts.
        Ok(Some(durations[(durations.len() - 1) / 2]))
    }

    pub fn get_running_average_secs(&self, today: chrono::NaiveDate, days: i64) -> Result<f64> {
        if days <= 0 {
            return Ok(0.0);
        }
        let cutoff = today - chrono::Duration::days(days - 1);
        let cutoff_str = cutoff.format("%Y-%m-%d").to_string();
        let total: i64 = self.conn.query_row(
            "SELECT COALESCE(SUM(duration_secs), 0) FROM sessions
             WHERE SUBSTR(start_iso, 1, 10) >= ?1",
            [cutoff_str],
            |row| row.get(0),
        )?;
        Ok(total as f64 / days as f64)
    }

    pub fn get_daily_totals(&self) -> Result<Vec<(chrono::NaiveDate, i64)>> {
        self.daily_totals_filtered(None)
    }

    pub fn get_daily_totals_for_label(
        &self,
        label_id: i64,
    ) -> Result<Vec<(chrono::NaiveDate, i64)>> {
        self.daily_totals_filtered(Some(label_id))
    }

    fn daily_totals_filtered(
        &self,
        label_filter: Option<i64>,
    ) -> Result<Vec<(chrono::NaiveDate, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT SUBSTR(start_iso, 1, 10) AS day, SUM(duration_secs)
             FROM sessions
             WHERE ?1 IS NULL OR label_id = ?1
             GROUP BY day
             ORDER BY day",
        )?;
        let totals = stmt
            .query_map(params![label_filter], |row| {
                let day_str: String = row.get(0)?;
                let total_secs: i64 = row.get(1)?;
                Ok((day_str, total_secs))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .filter_map(|(s, secs)| {
                chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d")
                    .ok()
                    .map(|d| (d, secs))
            })
            .collect();
        Ok(totals)
    }

    fn distinct_session_days_ascending(
        &self,
        label_filter: Option<i64>,
    ) -> Result<Vec<chrono::NaiveDate>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT SUBSTR(start_iso, 1, 10) FROM sessions
             WHERE ?1 IS NULL OR label_id = ?1
             ORDER BY 1",
        )?;
        let days = stmt
            .query_map(params![label_filter], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .filter_map(|s| chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok())
            .collect();
        Ok(days)
    }

    pub fn get_streak(&self, today: chrono::NaiveDate) -> Result<i64> {
        self.streak_filtered(today, None)
    }

    pub fn get_streak_for_label(&self, today: chrono::NaiveDate, label_id: i64) -> Result<i64> {
        self.streak_filtered(today, Some(label_id))
    }

    fn streak_filtered(
        &self,
        today: chrono::NaiveDate,
        label_filter: Option<i64>,
    ) -> Result<i64> {
        let days = self.distinct_session_days_ascending(label_filter)?;
        let Some(&most_recent) = days.last() else {
            return Ok(0);
        };
        let yesterday = today.pred_opt().expect("date underflow");
        let mut expected = if most_recent == today {
            today
        } else if most_recent == yesterday {
            yesterday
        } else {
            return Ok(0);
        };

        let mut count = 0;
        for day in days.iter().rev() {
            if *day == expected {
                count += 1;
                expected = expected.pred_opt().expect("date underflow");
            } else {
                break;
            }
        }
        Ok(count)
    }

    /// The longest single session — `(id, Session)`, or None on empty DB.
    /// Tie-break is unspecified (whichever SQLite returns first); callers
    /// should not depend on the order of equal-duration rows.
    pub fn get_longest_session(&self) -> Result<Option<(i64, Session)>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, start_iso, duration_secs, label_id, notes, mode, uuid, guided_file_uuid
             FROM sessions
             ORDER BY duration_secs DESC LIMIT 1",
        )?;
        let mut rows = stmt.query([])?;
        match rows.next()? {
            None => Ok(None),
            Some(row) => {
                let mode_str: String = row.get(5)?;
                let mode = SessionMode::from_db_str(&mode_str)
                    .expect("DB CHECK constraint should restrict mode");
                Ok(Some((
                    row.get::<_, i64>(0)?,
                    Session {
                        start_iso: row.get(1)?,
                        duration_secs: row.get(2)?,
                        label_id: row.get(3)?,
                        notes: row.get(4)?,
                        mode,
                        uuid: row.get(6)?,
                        guided_file_uuid: row.get(7)?,
                    },
                )))
            }
        }
    }

    /// Counts of sessions bucketed by start hour: morning < 12 (hours
    /// 0-11), afternoon 12-17, evening ≥ 18 (18-23). Returns
    /// `(morning, afternoon, evening)`. Every session lands in exactly
    /// one bucket.
    pub fn hour_buckets(&self) -> Result<(i64, i64, i64)> {
        // Hour is at chars 12-13 of start_iso (0-indexed in SQL it's 12).
        // Cast to integer once and bucket in a single pass.
        let mut stmt = self.conn.prepare_cached(
            "SELECT
               COALESCE(SUM(CASE WHEN h <  12 THEN 1 ELSE 0 END), 0),
               COALESCE(SUM(CASE WHEN h >= 12 AND h < 18 THEN 1 ELSE 0 END), 0),
               COALESCE(SUM(CASE WHEN h >= 18 THEN 1 ELSE 0 END), 0)
             FROM (
               SELECT CAST(SUBSTR(start_iso, 12, 2) AS INTEGER) AS h
               FROM sessions
             )",
        )?;
        Ok(stmt.query_row([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?)
    }

    /// Distinct (year, month) pairs that have at least one session,
    /// ordered most-recent first. Used by the calendar-picker dropdown.
    pub fn active_months(&self) -> Result<Vec<(i32, u32)>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT DISTINCT
                 CAST(SUBSTR(start_iso, 1, 4) AS INTEGER),
                 CAST(SUBSTR(start_iso, 6, 2) AS INTEGER)
             FROM sessions
             ORDER BY 1 DESC, 2 DESC",
        )?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Day-of-month numbers in `(year, month)` that have at least one
    /// session, ascending. Caller maps these directly to calendar cells.
    /// December rolls cleanly to next-year January for the upper bound.
    pub fn active_days_in_month(&self, year: i32, month: u32) -> Result<Vec<u32>> {
        let start = format!("{year:04}-{month:02}-01");
        let (next_year, next_month) =
            if month == 12 { (year + 1, 1) } else { (year, month + 1) };
        let end = format!("{next_year:04}-{next_month:02}-01");
        let mut stmt = self.conn.prepare_cached(
            "SELECT DISTINCT CAST(SUBSTR(start_iso, 9, 2) AS INTEGER)
             FROM sessions
             WHERE start_iso >= ?1 AND start_iso < ?2
             ORDER BY 1",
        )?;
        let rows = stmt.query_map(params![start, end], |row| row.get::<_, u32>(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Sum of `duration_secs` for sessions inside a calendar month
    /// (`year`, `month` 1-12). Boundaries are at local midnight on the
    /// first and last day of the month. December rolls cleanly into
    /// January of the next year.
    pub fn month_total_secs(&self, year: i32, month: u32) -> Result<i64> {
        let start = format!("{year:04}-{month:02}-01");
        let (next_year, next_month) =
            if month == 12 { (year + 1, 1) } else { (year, month + 1) };
        let end = format!("{next_year:04}-{next_month:02}-01");
        Ok(self.conn.query_row(
            "SELECT COALESCE(SUM(duration_secs), 0)
             FROM sessions
             WHERE start_iso >= ?1 AND start_iso < ?2",
            params![start, end],
            |row| row.get(0),
        )?)
    }

    /// Sum of `duration_secs` for sessions whose `start_iso` is on or
    /// after the start of `since` (interpreted as the user's local
    /// midnight). Returns 0 if no sessions match.
    ///
    /// Lexicographic comparison on ISO 8601 strings works because the
    /// format sorts chronologically as ASCII text. The cut-off is at
    /// the START of the date — a session at 00:00:00 on `since` is
    /// included.
    pub fn total_secs_since(&self, since: chrono::NaiveDate) -> Result<i64> {
        let prefix = since.format("%Y-%m-%d").to_string();
        Ok(self.conn.query_row(
            "SELECT COALESCE(SUM(duration_secs), 0)
             FROM sessions
             WHERE start_iso >= ?1",
            params![prefix],
            |row| row.get(0),
        )?)
    }

    /// Total of `duration_secs` across every session (no filter). Returns
    /// 0 on an empty DB. Use this when you want the underlying precision
    /// (e.g. weekly-goal ring, longest-session display); use
    /// `total_minutes` for stats lines that show "X min".
    pub fn total_seconds(&self) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COALESCE(SUM(duration_secs), 0) FROM sessions",
            [],
            |row| row.get(0),
        )?)
    }

    pub fn total_minutes(&self) -> Result<i64> {
        Ok(self.total_seconds()? / 60)
    }

    /// Per-label session count. `None` represents unlabeled sessions.
    pub fn count_sessions_by_label(&self) -> Result<Vec<(Option<String>, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT l.name, COUNT(*)
             FROM sessions s
             LEFT JOIN labels l ON s.label_id = l.id
             GROUP BY l.name
             ORDER BY l.name",
        )?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Per-label `(name, total_secs, session_count)` ordered by total
    /// seconds DESC, ties broken by name NOCASE ASC. Excludes unlabeled
    /// sessions AND labels with zero sessions (INNER JOIN drops both).
    /// Used by the stats panel's per-label breakdown.
    pub fn label_totals_seconds(&self) -> Result<Vec<(String, i64, i64)>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT labels.name,
                    SUM(sessions.duration_secs) AS total,
                    COUNT(sessions.id) AS n
             FROM labels
             INNER JOIN sessions ON sessions.label_id = labels.id
             GROUP BY labels.id, labels.name
             ORDER BY total DESC, labels.name COLLATE NOCASE ASC",
        )?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Per-label total minutes. `None` represents unlabeled sessions.
    pub fn total_minutes_by_label(&self) -> Result<Vec<(Option<String>, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT l.name, SUM(s.duration_secs) / 60
             FROM sessions s
             LEFT JOIN labels l ON s.label_id = l.id
             GROUP BY l.name
             ORDER BY l.name",
        )?;
        let rows = stmt
            .query_map([], |row| {
                let name: Option<String> = row.get(0)?;
                let mins: i64 = row.get(1)?;
                Ok((name, mins))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Rich-filter session query for the log feed: pagination, label
    /// filter, notes-only. Rows are ordered `start_iso DESC` so the
    /// caller's first page is the newest sessions.
    ///
    /// SQLite quirks handled here:
    /// - `LIMIT -1` means "no limit" (used when `filter.limit` is None).
    /// - `OFFSET 0` is the no-skip default.
    /// - The four (notes × label) combinations get distinct static
    ///   queries so each is independently cached by `prepare_cached`.
    pub fn query_sessions(&self, filter: &SessionFilter) -> Result<Vec<(i64, Session)>> {
        let limit_val: i64 = filter.limit.map(|n| n as i64).unwrap_or(-1);
        let offset_val: i64 = filter.offset.map(|n| n as i64).unwrap_or(0);

        let map_row = |row: &rusqlite::Row<'_>| -> rusqlite::Result<(i64, Session)> {
            let mode_str: String = row.get(5)?;
            let mode = SessionMode::from_db_str(&mode_str)
                .expect("DB CHECK constraint should restrict mode to known values");
            Ok((
                row.get::<_, i64>(0)?,
                Session {
                    start_iso: row.get(1)?,
                    duration_secs: row.get(2)?,
                    label_id: row.get(3)?,
                    notes: row.get(4)?,
                    mode,
                    uuid: row.get(6)?,
                        guided_file_uuid: row.get(7)?,
                },
            ))
        };

        let rows: rusqlite::Result<Vec<(i64, Session)>> = match (filter.only_with_notes, filter.label_id) {
            (false, None) => {
                let mut s = self.conn.prepare_cached(
                    "SELECT id, start_iso, duration_secs, label_id, notes, mode, uuid, guided_file_uuid
                     FROM sessions
                     ORDER BY start_iso DESC
                     LIMIT ?1 OFFSET ?2",
                )?;
                let it = s.query_map(params![limit_val, offset_val], map_row)?;
                it.collect()
            }
            (true, None) => {
                let mut s = self.conn.prepare_cached(
                    "SELECT id, start_iso, duration_secs, label_id, notes, mode, uuid, guided_file_uuid
                     FROM sessions
                     WHERE notes IS NOT NULL AND notes != ''
                     ORDER BY start_iso DESC
                     LIMIT ?1 OFFSET ?2",
                )?;
                let it = s.query_map(params![limit_val, offset_val], map_row)?;
                it.collect()
            }
            (false, Some(lid)) => {
                let mut s = self.conn.prepare_cached(
                    "SELECT id, start_iso, duration_secs, label_id, notes, mode, uuid, guided_file_uuid
                     FROM sessions
                     WHERE label_id = ?1
                     ORDER BY start_iso DESC
                     LIMIT ?2 OFFSET ?3",
                )?;
                let it = s.query_map(params![lid, limit_val, offset_val], map_row)?;
                it.collect()
            }
            (true, Some(lid)) => {
                let mut s = self.conn.prepare_cached(
                    "SELECT id, start_iso, duration_secs, label_id, notes, mode, uuid, guided_file_uuid
                     FROM sessions
                     WHERE label_id = ?1 AND notes IS NOT NULL AND notes != ''
                     ORDER BY start_iso DESC
                     LIMIT ?2 OFFSET ?3",
                )?;
                let it = s.query_map(params![lid, limit_val, offset_val], map_row)?;
                it.collect()
            }
        };
        Ok(rows?)
    }

    pub fn list_sessions(&self) -> Result<Vec<(i64, Session)>> {
        self.list_sessions_filtered(None)
    }

    pub fn list_sessions_for_label(&self, label_id: i64) -> Result<Vec<(i64, Session)>> {
        self.list_sessions_filtered(Some(label_id))
    }

    fn list_sessions_filtered(&self, label_filter: Option<i64>) -> Result<Vec<(i64, Session)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, start_iso, duration_secs, label_id, notes, mode, uuid, guided_file_uuid FROM sessions
             WHERE ?1 IS NULL OR label_id = ?1
             ORDER BY id",
        )?;
        let sessions = stmt
            .query_map(params![label_filter], |row| {
                let mode_str: String = row.get(5)?;
                let mode = SessionMode::from_db_str(&mode_str).expect(
                    "DB CHECK constraint should restrict mode to known values",
                );
                Ok((
                    row.get::<_, i64>(0)?,
                    Session {
                        start_iso: row.get(1)?,
                        duration_secs: row.get(2)?,
                        label_id: row.get(3)?,
                        notes: row.get(4)?,
                        mode,
                        uuid: row.get(6)?,
                        guided_file_uuid: row.get(7)?,
                    },
                ))
            })?
            .collect::<rusqlite::Result<Vec<(i64, Session)>>>()?;
        Ok(sessions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Cache schema version + walk-on-upgrade ────────────────────────

    #[test]
    fn fresh_open_in_memory_stamps_cache_schema_version_to_current() {
        // First-ever open: no events, but the marker still lands so
        // subsequent opens take the fast path and skip the walk.
        let db = Database::open_in_memory().unwrap();
        let stored = db
            .get_sync_state(CACHE_SCHEMA_VERSION_KEY, "missing")
            .unwrap();
        assert_eq!(stored, CACHE_SCHEMA_VERSION.to_string());
    }

    #[test]
    fn re_open_with_old_cache_version_walks_events_and_rematerialises_cache() {
        // Simulate a DB that was last opened by a build whose
        // apply_event_inner skipped some kind we now understand. The
        // walk-on-upgrade must re-apply every event so the cache
        // catches up to the current dispatch, then stamp the new
        // cache version to gate future fast paths.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("walk.db");
        {
            let db = Database::open(&path).unwrap();
            // Author some events the normal way.
            db.insert_label("Focus").unwrap();
            db.insert_label("Calm").unwrap();
            assert_eq!(db.list_labels().unwrap().len(), 2);
            // Manually delete the cache rows AND roll the cache
            // version back to 0 — pretending the previous build had
            // recorded these events without materialising them.
            db.conn.execute("DELETE FROM labels", []).unwrap();
            db.set_sync_state(CACHE_SCHEMA_VERSION_KEY, "0").unwrap();
            assert!(db.list_labels().unwrap().is_empty(),
                "labels cache must be empty before the re-open");
        }
        // Reopen. init must walk and re-materialise the labels.
        let db = Database::open(&path).unwrap();
        let labels = db.list_labels().unwrap();
        assert_eq!(labels.len(), 2, "labels must be re-materialised from event log");
        let mut names: Vec<_> = labels.iter().map(|l| l.name.clone()).collect();
        names.sort();
        assert_eq!(names, vec!["Calm".to_string(), "Focus".to_string()]);
        // Marker advances so the next open skips the walk.
        assert_eq!(
            db.get_sync_state(CACHE_SCHEMA_VERSION_KEY, "missing").unwrap(),
            CACHE_SCHEMA_VERSION.to_string(),
        );
    }

    #[test]
    fn re_open_at_current_cache_version_does_not_re_walk() {
        // Fast path: a DB already at the current cache version must
        // skip the walk so re-opens stay O(1) regardless of how many
        // events the log holds.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fast.db");
        {
            let db = Database::open(&path).unwrap();
            db.insert_label("Focus").unwrap();
            // Manually corrupt the cache and leave the marker at
            // current — a re-walk would fix this; the fast path must
            // therefore leave it broken (proving the walk was
            // skipped).
            db.conn.execute("DELETE FROM labels", []).unwrap();
            assert_eq!(
                db.get_sync_state(CACHE_SCHEMA_VERSION_KEY, "missing").unwrap(),
                CACHE_SCHEMA_VERSION.to_string(),
            );
        }
        let db = Database::open(&path).unwrap();
        assert!(
            db.list_labels().unwrap().is_empty(),
            "fast path must NOT have re-walked (labels stay empty)",
        );
    }

    // ── Schema version sentinel ───────────────────────────────────────

    #[test]
    fn open_in_memory_stamps_current_schema_version() {
        // A fresh DB starts at user_version=0; init must apply schema
        // and stamp SCHEMA_VERSION so a future reopen finds a matching
        // value rather than treating the DB as fresh again.
        let db = Database::open_in_memory().unwrap();
        let v: u32 = db.conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(v, SCHEMA_VERSION);
    }

    #[test]
    fn init_refuses_db_written_by_a_future_build() {
        // A DB whose user_version exceeds our SCHEMA_VERSION was written
        // by a build that may have added forward-only columns; opening
        // it risks silent corruption on subsequent writes.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(&format!(
            "PRAGMA user_version = {};", SCHEMA_VERSION + 1
        )).unwrap();
        match Database::init(conn) {
            Err(DbError::SchemaVersionTooNew { db, build }) => {
                assert_eq!(db, SCHEMA_VERSION + 1);
                assert_eq!(build, SCHEMA_VERSION);
            }
            Err(other) => panic!("expected SchemaVersionTooNew, got {other:?}"),
            Ok(_) => panic!("expected error, init succeeded"),
        }
    }

    #[test]
    fn init_accepts_db_already_at_current_schema_version() {
        // Reopening a previously-stamped DB is the common case after
        // the first launch; init must accept it without re-stamping
        // or rejecting.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(&format!(
            "PRAGMA user_version = {};", SCHEMA_VERSION
        )).unwrap();
        Database::init(conn).expect("init succeeds at current version");
    }

    // ── Name-collision suffix on sync merge ───────────────────────────

    #[test]
    fn label_name_collision_during_apply_renames_loser_with_uuid_suffix() {
        // Two peers offline both name a (different-uuid) label
        // "Morning". On sync merge, the UPSERT-on-uuid for the
        // second label hits UNIQUE-on-name. Without the retry,
        // recompute_label returns Err and replay_events rolls back
        // — sync hard-stuck on the poison event forever.
        // With the retry, the second row materialises under a
        // uuid-suffixed name and sync keeps moving.
        let db = Database::open_in_memory().unwrap();
        let l1 = "550e8400-e29b-41d4-a716-44665544aaaa";
        let l2 = "550e8400-e29b-41d4-a716-44665544bbbb";
        for (uuid, lamport, device) in [
            (l1, 5_i64, "dev-A"),
            (l2, 6_i64, "dev-B"),
        ] {
            db.apply_event(&Event {
                event_uuid: uuid::Uuid::new_v4().to_string(),
                lamport_ts: lamport,
                device_id: device.to_string(),
                kind: "label_insert".to_string(),
                target_id: uuid.to_string(),
                payload: serde_json::json!({
                    "uuid": uuid, "name": "Morning",
                }).to_string(),
            }).unwrap();
        }
        let labels = db.list_labels().unwrap();
        assert_eq!(labels.len(), 2,
            "both label rows must materialise — neither poisons the apply");
        let names: Vec<_> = labels.iter().map(|l| l.name.as_str()).collect();
        // The first-arriving label keeps the bare name; the second
        // gets the uuid-prefix suffix.
        assert!(names.contains(&"Morning"),
            "first label must keep its original name; got {names:?}");
        assert!(
            names.iter().any(|n| n.starts_with("Morning (conflict-")),
            "second label must carry the conflict suffix; got {names:?}"
        );
    }

    #[test]
    fn label_name_collision_is_idempotent_on_replay() {
        // Re-applying the same collision sequence must land on the
        // same suffixed name, not invent a different one each time
        // (the suffix is derived from the uuid prefix, which is
        // event-deterministic).
        let db1 = Database::open_in_memory().unwrap();
        let db2 = Database::open_in_memory().unwrap();
        let l1 = "550e8400-e29b-41d4-a716-44665544aaaa";
        let l2 = "550e8400-e29b-41d4-a716-44665544bbbb";
        let events: Vec<Event> = [(l1, 5_i64, "A"), (l2, 6_i64, "B")]
            .into_iter()
            .map(|(uuid, lamport, device)| Event {
                event_uuid: uuid::Uuid::new_v4().to_string(),
                lamport_ts: lamport,
                device_id: device.to_string(),
                kind: "label_insert".to_string(),
                target_id: uuid.to_string(),
                payload: serde_json::json!({
                    "uuid": uuid, "name": "Morning",
                }).to_string(),
            })
            .collect();
        for db in [&db1, &db2] {
            for e in &events { db.apply_event(e).unwrap(); }
        }
        let mut names1: Vec<_> = db1.list_labels().unwrap()
            .into_iter().map(|l| l.name).collect();
        let mut names2: Vec<_> = db2.list_labels().unwrap()
            .into_iter().map(|l| l.name).collect();
        names1.sort(); names2.sort();
        assert_eq!(names1, names2,
            "two devices applying the same events must converge on the same names");
    }

    // ── recompute_label re-links orphaned sessions ────────────────────

    #[test]
    fn session_arriving_before_label_gets_relinked_when_label_arrives() {
        // Out-of-order event delivery: a peer pushes label_insert and
        // session_insert in separate batches. Pull processes the
        // session batch first; recompute_session looks up the label
        // uuid and finds nothing (label_id = None — orphan). Later
        // the label batch arrives; recompute_label must re-link the
        // orphan sessions or they stay label-less forever even after
        // the label exists locally.
        let db = Database::open_in_memory().unwrap();
        let label_uuid = "550e8400-e29b-41d4-a716-446655440001";
        let session_uuid = "550e8400-e29b-41d4-a716-446655440002";

        // Step 1: session_insert arrives, label doesn't exist yet.
        let session_payload = serde_json::json!({
            "uuid": session_uuid,
            "start_iso": "2026-05-01T10:00:00",
            "duration_secs": 600,
            "label_uuid": label_uuid,
            "notes": null,
            "mode": "timer",
            "guided_file_uuid": null,
        }).to_string();
        db.apply_event(&Event {
            event_uuid: uuid::Uuid::new_v4().to_string(),
            lamport_ts: 5,
            device_id: "peer".to_string(),
            kind: "session_insert".to_string(),
            target_id: session_uuid.to_string(),
            payload: session_payload,
        }).unwrap();

        // Session exists but label_id is None — the orphan state.
        let sessions = db.list_sessions().unwrap();
        assert_eq!(sessions.len(), 1);
        assert!(
            sessions[0].1.label_id.is_none(),
            "session must be orphaned before label arrives"
        );

        // Step 2: label_insert arrives. Re-link must happen inside
        // recompute_label so the orphan resolves.
        let label_payload = serde_json::json!({
            "uuid": label_uuid,
            "name": "Focus",
        }).to_string();
        db.apply_event(&Event {
            event_uuid: uuid::Uuid::new_v4().to_string(),
            lamport_ts: 6,
            device_id: "peer".to_string(),
            kind: "label_insert".to_string(),
            target_id: label_uuid.to_string(),
            payload: label_payload,
        }).unwrap();

        let sessions = db.list_sessions().unwrap();
        let s = &sessions[0].1;
        assert!(s.label_id.is_some(), "session must be re-linked after label arrives");
        let labels = db.list_labels().unwrap();
        let focus_id = labels.iter().find(|l| l.name == "Focus").unwrap().id;
        assert_eq!(s.label_id, Some(focus_id));
    }

    #[test]
    fn label_relink_only_targets_sessions_whose_latest_event_references_this_label() {
        // A session that was inserted with label_uuid=L1 and later
        // updated to label_uuid=L2 must stay linked to L2 when L1
        // arrives — the re-link query must check the LATEST event's
        // label_uuid, not just any event in history.
        let db = Database::open_in_memory().unwrap();
        let l1 = "550e8400-e29b-41d4-a716-44665544aaaa";
        let l2 = "550e8400-e29b-41d4-a716-44665544bbbb";
        let s = "550e8400-e29b-41d4-a716-44665544cccc";

        // session_insert references L1 at lamport 5
        db.apply_event(&Event {
            event_uuid: uuid::Uuid::new_v4().to_string(),
            lamport_ts: 5, device_id: "peer".to_string(),
            kind: "session_insert".to_string(),
            target_id: s.to_string(),
            payload: serde_json::json!({
                "uuid": s, "start_iso": "2026-05-01T10:00:00",
                "duration_secs": 600, "label_uuid": l1,
                "notes": null, "mode": "timer", "guided_file_uuid": null,
            }).to_string(),
        }).unwrap();
        // session_update points to L2 at lamport 7 (the latest)
        db.apply_event(&Event {
            event_uuid: uuid::Uuid::new_v4().to_string(),
            lamport_ts: 7, device_id: "peer".to_string(),
            kind: "session_update".to_string(),
            target_id: s.to_string(),
            payload: serde_json::json!({
                "uuid": s, "start_iso": "2026-05-01T10:00:00",
                "duration_secs": 600, "label_uuid": l2,
                "notes": null, "mode": "timer", "guided_file_uuid": null,
            }).to_string(),
        }).unwrap();
        // Now L2 arrives — should link the session.
        db.apply_event(&Event {
            event_uuid: uuid::Uuid::new_v4().to_string(),
            lamport_ts: 8, device_id: "peer".to_string(),
            kind: "label_insert".to_string(),
            target_id: l2.to_string(),
            payload: serde_json::json!({"uuid": l2, "name": "L2"}).to_string(),
        }).unwrap();
        let labels = db.list_labels().unwrap();
        let l2_id = labels.iter().find(|l| l.name == "L2").unwrap().id;
        let session_label = db.list_sessions().unwrap()[0].1.label_id;
        assert_eq!(session_label, Some(l2_id), "linked to L2 (latest)");

        // L1 arrives — must NOT re-steal the session away from L2.
        db.apply_event(&Event {
            event_uuid: uuid::Uuid::new_v4().to_string(),
            lamport_ts: 9, device_id: "peer".to_string(),
            kind: "label_insert".to_string(),
            target_id: l1.to_string(),
            payload: serde_json::json!({"uuid": l1, "name": "L1"}).to_string(),
        }).unwrap();
        let session_label = db.list_sessions().unwrap()[0].1.label_id;
        assert_eq!(session_label, Some(l2_id),
            "session must remain on L2, not re-linked to L1 by the stale event");
    }

    // ── target_id_is_well_formed_for ──────────────────────────────────

    #[test]
    fn target_id_validator_accepts_uuid_and_short_identifiers() {
        // The validator is path-traversal-focused, not UUID-strict —
        // legitimate non-UUID identifiers (short test IDs, opaque
        // keys) pass through unless they carry path separators.
        for kind in [
            "session_insert", "label_rename", "interval_bell_update",
            "bell_sound_insert", "preset_delete", "guided_file_insert",
            "vibration_pattern_update",
        ] {
            assert!(target_id_is_well_formed_for(
                kind, "550e8400-e29b-41d4-a716-446655440000"
            ));
            assert!(target_id_is_well_formed_for(kind, "bell-1"));
            assert!(target_id_is_well_formed_for(kind, "u-1"));
        }
    }

    #[test]
    fn target_id_validator_rejects_path_traversal() {
        // These are the strings that would let a peer write outside
        // sounds_dir when the value is later interpolated into a
        // filename component.
        for bad in [
            "../../../etc/passwd",
            "../sibling.wav",
            "/absolute/path",
            "with/slash",
            "back\\slash",
            "null\0byte",
            "",
        ] {
            assert!(
                !target_id_is_well_formed_for("bell_sound_insert", bad),
                "expected reject for target_id={bad:?}"
            );
        }
    }

    #[test]
    fn target_id_validator_accepts_phase_strings_for_box_breath() {
        for phase in ["in", "holdin", "out", "holdout"] {
            assert!(target_id_is_well_formed_for("box_breath_phase_update", phase));
        }
    }

    #[test]
    fn target_id_validator_rejects_unknown_phase_or_traversal_for_box_breath() {
        for bad in ["inhale", "", "../etc", "IN"] {
            assert!(
                !target_id_is_well_formed_for("box_breath_phase_update", bad),
                "expected reject for box_breath target_id={bad:?}"
            );
        }
    }

    #[test]
    fn target_id_validator_passes_unknown_kinds_through() {
        // Forward-compat: a future entity type's event would be
        // recorded-not-applied; the validator must not over-block.
        assert!(target_id_is_well_formed_for("future_kind", "anything"));
        // ...except path-traversal, which is universal.
        assert!(!target_id_is_well_formed_for("future_kind", "../etc"));
        assert!(!target_id_is_well_formed_for("future_kind", ""));
    }

    #[test]
    fn apply_event_inner_skips_dispatch_on_invalid_target_id() {
        // Peer ships a bell_sound_insert with target_id that's a path-
        // traversal string. The event row records, but no bell_sounds
        // row materialises — preventing the downstream file-write
        // primitive in pull_custom_sound_files.
        let db = Database::open_in_memory().unwrap();
        let device_id = "peer-device".to_string();
        let evil = "../../../etc/passwd";
        let payload = serde_json::json!({
            "uuid": evil,
            "name": "Trojan",
            "file_path": "/p/x.wav",
            "is_bundled": false,
            "mime_type": "audio/wav",
            "category": "general",
            "created_iso": "2026-05-11T00:00:00",
        }).to_string();
        let event = Event {
            event_uuid: uuid::Uuid::new_v4().to_string(),
            lamport_ts: 1,
            device_id,
            kind: "bell_sound_insert".to_string(),
            target_id: evil.to_string(),
            payload,
        };
        db.apply_event(&event).unwrap();
        // Event row recorded for forward-compat, but no row in the
        // bell_sounds cache — the harm is the dispatch.
        let bells = db.list_bell_sounds().unwrap();
        assert!(
            bells.iter().all(|b| b.uuid != evil),
            "evil target_id must NOT land in bell_sounds.uuid"
        );
    }

    // ── SessionMode serialization ─────────────────────────────────────────────

    #[test]
    fn session_mode_as_db_str_returns_canonical_strings() {
        // These are the values that go into the sessions.mode column AND
        // the CSV mode column — pinning them so a refactor that quietly
        // changes one (e.g. 'box_breath' → 'breath') gets caught.
        assert_eq!(SessionMode::Timer.as_db_str(), "timer");
        assert_eq!(SessionMode::BoxBreath.as_db_str(), "box_breath");
        assert_eq!(SessionMode::Guided.as_db_str(), "guided");
    }

    #[test]
    fn session_mode_from_db_str_parses_canonical_strings() {
        assert_eq!(SessionMode::from_db_str("timer"), Some(SessionMode::Timer));
        assert_eq!(SessionMode::from_db_str("box_breath"), Some(SessionMode::BoxBreath));
        assert_eq!(SessionMode::from_db_str("guided"), Some(SessionMode::Guided));
    }

    #[test]
    fn session_mode_from_db_str_returns_none_for_unknown() {
        // No legacy fallback — "countdown" and "stopwatch" deliberately
        // map to None. Callers decide what to do (existing data_io /
        // log paths default to Timer via unwrap_or, which makes legacy
        // rows readable without us adding a compat shim).
        assert_eq!(SessionMode::from_db_str(""), None);
        assert_eq!(SessionMode::from_db_str("countdown"), None);
        assert_eq!(SessionMode::from_db_str("stopwatch"), None);
        assert_eq!(SessionMode::from_db_str("TIMER"), None);  // case-sensitive
        assert_eq!(SessionMode::from_db_str("breathing"), None);  // old name
        assert_eq!(SessionMode::from_db_str("box-breath"), None); // dash, not underscore
        assert_eq!(SessionMode::from_db_str("Guided"), None);     // case-sensitive
        assert_eq!(SessionMode::from_db_str("garbage"), None);
    }

    #[test]
    fn session_mode_db_str_round_trip() {
        for &mode in &[SessionMode::Timer, SessionMode::BoxBreath, SessionMode::Guided] {
            assert_eq!(SessionMode::from_db_str(mode.as_db_str()), Some(mode));
        }
    }

    #[test]
    fn inserting_label_increases_count() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        assert_eq!(db.count_labels().unwrap(), 1);
    }

    #[test]
    fn inserting_two_distinct_labels_yields_count_of_two() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        db.insert_label("Evening").unwrap();
        assert_eq!(db.count_labels().unwrap(), 2);
    }

    #[test]
    fn inserting_duplicate_label_returns_err() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let second = db.insert_label("Morning");
        assert!(second.is_err(), "second insert of same label should fail");
        // The first insert is preserved; no duplicate row is created.
        assert_eq!(db.count_labels().unwrap(), 1);
    }

    // ── label_totals_seconds (name, secs, count) ─────────────────────────────

    #[test]
    fn label_totals_seconds_is_empty_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.label_totals_seconds().unwrap().is_empty());
    }

    #[test]
    fn label_totals_seconds_groups_secs_and_counts_per_label() {
        // (name, total_secs, session_count) per label. Unlabeled sessions
        // and labels with zero sessions are excluded — INNER JOIN drops
        // them at the SQL level. Sort: total_secs DESC, name ASC NOCASE.
        let db = Database::open_in_memory().unwrap();
        let morning = db.insert_label("Morning").unwrap();
        let evening = db.insert_label("Evening").unwrap();
        // An extra label with no sessions — must NOT appear in output.
        let _unused = db.insert_label("Unused").unwrap();

        // Morning: 2 sessions, 900s total.
        db.insert_session(&Session {
            start_iso: "2026-04-27T07:00:00".to_string(),
            duration_secs: 600, label_id: Some(morning), notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-28T07:00:00".to_string(),
            duration_secs: 300, label_id: Some(morning), notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        // Evening: 1 session, 1200s total — larger total, should sort first.
        db.insert_session(&Session {
            start_iso: "2026-04-27T20:00:00".to_string(),
            duration_secs: 1200, label_id: Some(evening), notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        // Unlabeled session — must NOT appear.
        db.insert_session(&Session {
            start_iso: "2026-04-27T12:00:00".to_string(),
            duration_secs: 500, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();

        let got = db.label_totals_seconds().unwrap();
        assert_eq!(got.len(), 2,
            "Unused label and unlabeled session must be excluded: {got:?}");
        assert_eq!(got[0], ("Evening".to_string(), 1200, 1));
        assert_eq!(got[1], ("Morning".to_string(), 900, 2));
    }

    #[test]
    fn label_totals_seconds_ties_break_case_insensitive_alphabetic() {
        // Same total ⇒ secondary sort by name, NOCASE.
        let db = Database::open_in_memory().unwrap();
        let zebra = db.insert_label("Zebra").unwrap();
        let alpha = db.insert_label("alpha").unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T12:00:00".to_string(),
            duration_secs: 600, label_id: Some(zebra), notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-28T12:00:00".to_string(),
            duration_secs: 600, label_id: Some(alpha), notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let got = db.label_totals_seconds().unwrap();
        // 'alpha' (lowercase) sorts before 'Zebra' under NOCASE collation.
        assert_eq!(got[0].0, "alpha");
        assert_eq!(got[1].0, "Zebra");
    }

    #[test]
    fn label_totals_seconds_preserves_full_seconds_precision() {
        // total_minutes_by_label returns minutes (lossy integer division).
        // This variant must NOT lose sub-minute precision.
        let db = Database::open_in_memory().unwrap();
        let lid = db.insert_label("Morning").unwrap();
        // 90s + 45s = 135s — would round to 2 minutes (=120s) under
        // the minutes-then-converted approach.
        db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00".to_string(),
            duration_secs: 90, label_id: Some(lid), notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-28T10:00:00".to_string(),
            duration_secs: 45, label_id: Some(lid), notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let got = db.label_totals_seconds().unwrap();
        assert_eq!(got[0], ("Morning".to_string(), 135, 2));
    }

    // ── hour_buckets ─────────────────────────────────────────────────────────

    #[test]
    fn hour_buckets_is_zero_zero_zero_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.hour_buckets().unwrap(), (0, 0, 0));
    }

    #[test]
    fn hour_buckets_assigns_each_session_to_exactly_one_bucket() {
        // Boundaries: morning < 12 (00:00–11:59), afternoon 12–17,
        // evening ≥ 18 (18:00–23:59). Pin every boundary explicitly.
        let db = Database::open_in_memory().unwrap();
        let make = |hh: u32, mm: u32| Session {
            start_iso: format!("2026-04-27T{hh:02}:{mm:02}:00"),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        };
        // Morning (5 sessions, hours 0, 6, 11:00, 11:59).
        db.insert_session(&make(0, 0)).unwrap();
        db.insert_session(&make(6, 30)).unwrap();
        db.insert_session(&make(11, 0)).unwrap();
        db.insert_session(&make(11, 59)).unwrap();
        db.insert_session(&make(8, 15)).unwrap();
        // Afternoon (3 sessions, hours 12:00, 15:30, 17:59).
        db.insert_session(&make(12, 0)).unwrap();  // boundary into afternoon
        db.insert_session(&make(15, 30)).unwrap();
        db.insert_session(&make(17, 59)).unwrap(); // last minute of afternoon
        // Evening (2 sessions, hours 18:00, 23:59).
        db.insert_session(&make(18, 0)).unwrap();  // boundary into evening
        db.insert_session(&make(23, 59)).unwrap();

        let (morning, afternoon, evening) = db.hour_buckets().unwrap();
        assert_eq!(morning, 5, "five sessions in 00:00–11:59");
        assert_eq!(afternoon, 3, "three sessions in 12:00–17:59");
        assert_eq!(evening, 2, "two sessions in 18:00–23:59");
    }

    #[test]
    fn hour_buckets_total_equals_session_count() {
        // Defensive: every session lands in exactly one bucket, no
        // sessions are dropped or double-counted.
        let db = Database::open_in_memory().unwrap();
        let hours = [3u32, 7, 11, 12, 13, 17, 18, 22];
        for &h in &hours {
            db.insert_session(&Session {
                start_iso: format!("2026-04-27T{h:02}:00:00"),
                duration_secs: 600, label_id: None, notes: None,
                mode: SessionMode::Timer,
                uuid: String::new(),
                guided_file_uuid: None,
            }).unwrap();
        }
        let (m, a, e) = db.hour_buckets().unwrap();
        assert_eq!(m + a + e, hours.len() as i64);
        assert_eq!(m + a + e, db.count_sessions().unwrap());
    }

    // ── active_months ────────────────────────────────────────────────────────

    #[test]
    fn active_months_is_empty_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.active_months().unwrap().is_empty());
    }

    #[test]
    fn active_months_returns_distinct_year_month_pairs_descending() {
        // Each session contributes its (year, month) — duplicates within
        // the same month collapse to one entry. Order is most-recent first
        // (the calendar picker shows latest months at the top).
        let db = Database::open_in_memory().unwrap();
        // Three sessions in 2026-04, two in 2026-03, one in 2025-12.
        for d in 1..=3 {
            db.insert_session(&Session {
                start_iso: format!("2026-04-{d:02}T10:00:00"),
                duration_secs: 600, label_id: None, notes: None,
                mode: SessionMode::Timer,
                uuid: String::new(),
                guided_file_uuid: None,
            }).unwrap();
        }
        for d in 5..=6 {
            db.insert_session(&Session {
                start_iso: format!("2026-03-{d:02}T10:00:00"),
                duration_secs: 600, label_id: None, notes: None,
                mode: SessionMode::Timer,
                uuid: String::new(),
                guided_file_uuid: None,
            }).unwrap();
        }
        db.insert_session(&Session {
            start_iso: "2025-12-25T10:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();

        let got = db.active_months().unwrap();
        // Three distinct months, newest first.
        assert_eq!(got, vec![(2026, 4), (2026, 3), (2025, 12)]);
    }

    #[test]
    fn active_months_orders_correctly_across_year_boundary() {
        // 2025-12 must sort BEFORE 2026-01 in newest-first ordering.
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-01-15T10:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        db.insert_session(&Session {
            start_iso: "2025-12-15T10:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let got = db.active_months().unwrap();
        assert_eq!(got, vec![(2026, 1), (2025, 12)]);
    }

    // ── active_days_in_month ─────────────────────────────────────────────────

    #[test]
    fn active_days_in_month_is_empty_for_silent_month() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.active_days_in_month(2026, 4).unwrap().is_empty());
    }

    #[test]
    fn active_days_in_month_returns_distinct_days_ascending() {
        // Each day with at least one session contributes once. Multiple
        // sessions on the same day collapse to one entry. Returned in
        // ascending order (1, 2, 3, …) so callers can directly map to
        // calendar cells.
        let db = Database::open_in_memory().unwrap();
        // Two sessions on day 5, one on day 12, one on day 28.
        for hr in 9..=10 {
            db.insert_session(&Session {
                start_iso: format!("2026-04-05T{hr:02}:00:00"),
                duration_secs: 600, label_id: None, notes: None,
                mode: SessionMode::Timer,
                uuid: String::new(),
                guided_file_uuid: None,
            }).unwrap();
        }
        db.insert_session(&Session {
            start_iso: "2026-04-12T10:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-28T10:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        // A session in March — must NOT appear in April's days.
        db.insert_session(&Session {
            start_iso: "2026-03-15T10:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();

        let got = db.active_days_in_month(2026, 4).unwrap();
        assert_eq!(got, vec![5u32, 12, 28]);
    }

    #[test]
    fn active_days_in_month_handles_december() {
        // The 'next month' boundary in code must roll to next-year-Jan
        // for December queries.
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-12-31T23:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        // Jan 1 next year — must NOT contribute.
        db.insert_session(&Session {
            start_iso: "2027-01-01T00:30:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let got = db.active_days_in_month(2026, 12).unwrap();
        assert_eq!(got, vec![31u32]);
    }

    // ── month_total_secs ─────────────────────────────────────────────────────

    #[test]
    fn month_total_secs_is_zero_for_empty_month() {
        let db = Database::open_in_memory().unwrap();
        // Far past — guaranteed empty.
        assert_eq!(db.month_total_secs(1999, 1).unwrap(), 0);
        // Mid-future — also empty.
        assert_eq!(db.month_total_secs(2099, 12).unwrap(), 0);
    }

    #[test]
    fn month_total_secs_sums_only_target_month() {
        // Adjacent-month boundary edges: last second of March and first
        // second of May must NOT count toward April.
        let db = Database::open_in_memory().unwrap();
        // March 31, very late.
        db.insert_session(&Session {
            start_iso: "2026-03-31T23:59:59".to_string(),
            duration_secs: 9999, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        // April 1, midnight — INCLUDED in April.
        db.insert_session(&Session {
            start_iso: "2026-04-01T00:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        // April 30, late evening — INCLUDED.
        db.insert_session(&Session {
            start_iso: "2026-04-30T23:59:59".to_string(),
            duration_secs: 1200, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        // May 1, midnight — EXCLUDED.
        db.insert_session(&Session {
            start_iso: "2026-05-01T00:00:00".to_string(),
            duration_secs: 8888, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();

        assert_eq!(db.month_total_secs(2026, 4).unwrap(), 600 + 1200);
    }

    #[test]
    fn month_total_secs_handles_december_year_rollover() {
        // The "next month" boundary is built in code; December must
        // roll to next-year-January cleanly.
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-12-15T10:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        // Jan 1, 2027 — must NOT count toward Dec 2026.
        db.insert_session(&Session {
            start_iso: "2027-01-01T00:00:00".to_string(),
            duration_secs: 9999, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        assert_eq!(db.month_total_secs(2026, 12).unwrap(), 600);
    }

    // ── total_secs_since: weekly goal ring etc. ──────────────────────────────

    #[test]
    fn total_secs_since_is_zero_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        let since = chrono::NaiveDate::from_ymd_opt(2026, 4, 27).unwrap();
        assert_eq!(db.total_secs_since(since).unwrap(), 0);
    }

    #[test]
    fn total_secs_since_includes_sessions_on_or_after_date() {
        // Cut-off is at the START of the local-naive `since` date — a
        // session at 00:00:00 on `since` IS included.
        let db = Database::open_in_memory().unwrap();
        // On the cut-off date.
        db.insert_session(&Session {
            start_iso: "2026-04-27T00:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        // Later that day.
        db.insert_session(&Session {
            start_iso: "2026-04-27T18:00:00".to_string(),
            duration_secs: 1200, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        // Following day.
        db.insert_session(&Session {
            start_iso: "2026-04-28T10:00:00".to_string(),
            duration_secs: 300, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let since = chrono::NaiveDate::from_ymd_opt(2026, 4, 27).unwrap();
        assert_eq!(db.total_secs_since(since).unwrap(), 600 + 1200 + 300);
    }

    #[test]
    fn total_secs_since_excludes_sessions_before_date() {
        let db = Database::open_in_memory().unwrap();
        // Day before the cut-off.
        db.insert_session(&Session {
            start_iso: "2026-04-26T23:59:59".to_string(),
            duration_secs: 9999, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        // On / after cut-off — counted.
        db.insert_session(&Session {
            start_iso: "2026-04-27T00:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let since = chrono::NaiveDate::from_ymd_opt(2026, 4, 27).unwrap();
        assert_eq!(db.total_secs_since(since).unwrap(), 600);
    }

    #[test]
    fn total_secs_since_far_future_date_returns_zero() {
        // Asking for a date past every session's start returns 0.
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let since = chrono::NaiveDate::from_ymd_opt(2099, 1, 1).unwrap();
        assert_eq!(db.total_secs_since(since).unwrap(), 0);
    }

    // ── get_longest_session ──────────────────────────────────────────────────

    #[test]
    fn get_longest_session_is_none_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.get_longest_session().unwrap().is_none());
    }

    #[test]
    fn get_longest_session_returns_only_session_for_single_row_db() {
        let db = Database::open_in_memory().unwrap();
        let mut session = Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        };
        let id = db.insert_session(&session).unwrap();
        let (got_id, got) = db.get_longest_session().unwrap().unwrap();
        assert!(looks_like_uuid_v4(&got.uuid),
            "longest-session result must carry a v4 uuid");
        session.uuid = got.uuid.clone();
        assert_eq!((got_id, got), (id, session));
    }

    #[test]
    fn get_longest_session_returns_largest_duration() {
        // The longest among many — every other session must be shorter,
        // and the returned Session is the LONG one with all its fields
        // intact (not just the duration).
        let db = Database::open_in_memory().unwrap();
        for &secs in &[300u32, 600, 900, 1200, 450] {
            db.insert_session(&Session {
                start_iso: format!("2026-04-2{secs}T10:00:00Z"),
                duration_secs: secs,
                label_id: None,
                notes: None,
                mode: SessionMode::Timer,
                uuid: String::new(),
                guided_file_uuid: None,
            }).unwrap();
        }
        let mut longest_session = Session {
            start_iso: "2026-04-30T20:00:00Z".to_string(),
            duration_secs: 3600,
            label_id: None,
            notes: Some("the long one".to_string()),
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        };
        let longest_id = db.insert_session(&longest_session).unwrap();
        // Add one more shorter after — the order of insertion must not
        // affect which row wins.
        db.insert_session(&Session {
            start_iso: "2026-05-01T10:00:00Z".to_string(),
            duration_secs: 700,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();

        let (got_id, got) = db.get_longest_session().unwrap().unwrap();
        assert!(looks_like_uuid_v4(&got.uuid));
        longest_session.uuid = got.uuid.clone();
        assert_eq!(got_id, longest_id);
        assert_eq!(got, longest_session,
            "the returned Session must have every field of the long row, not just duration");
    }

    // ── total_seconds: precision-preserving aggregate ─────────────────────────

    #[test]
    fn total_seconds_is_zero_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.total_seconds().unwrap(), 0);
    }

    #[test]
    fn total_seconds_sums_all_durations() {
        // Sums every session, regardless of label / mode / notes.
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T11:00:00Z".to_string(),
            duration_secs: 1245, label_id: None, notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        // Sub-minute remainder must NOT be lost — the whole point of
        // having a seconds aggregate alongside total_minutes.
        db.insert_session(&Session {
            start_iso: "2026-04-27T12:00:00Z".to_string(),
            duration_secs: 17, label_id: None, notes: None,
            mode: SessionMode::BoxBreath,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        assert_eq!(db.total_seconds().unwrap(), 600 + 1245 + 17);
    }

    #[test]
    fn total_minutes_agrees_with_total_seconds_div_60() {
        // After refactoring total_minutes to delegate to total_seconds,
        // the contract is: minutes = seconds / 60 (integer division).
        let db = Database::open_in_memory().unwrap();
        for &secs in &[59i64, 60, 61, 119, 120, 600, 1245] {
            db.insert_session(&Session {
                start_iso: format!("2026-04-27T10:{:02}:00Z", secs % 60),
                duration_secs: secs as u32, label_id: None, notes: None,
                mode: SessionMode::Timer,
                uuid: String::new(),
                guided_file_uuid: None,
            }).unwrap();
        }
        let secs = db.total_seconds().unwrap();
        let mins = db.total_minutes().unwrap();
        assert_eq!(mins, secs / 60);
    }

    // ── query_sessions: rich filter for the log feed ──────────────────────────

    #[test]
    fn query_sessions_default_filter_returns_all_newest_first() {
        // Default-constructed SessionFilter: no filter, no pagination —
        // every session, ordered start_iso DESC (newest first), to match
        // the log feed UX.
        let db = Database::open_in_memory().unwrap();
        let make = |iso: &str| Session {
            start_iso: iso.to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        };
        let _id_old = db.insert_session(&make("2026-04-25T10:00:00Z")).unwrap();
        let _id_new = db.insert_session(&make("2026-04-27T10:00:00Z")).unwrap();
        let _id_mid = db.insert_session(&make("2026-04-26T10:00:00Z")).unwrap();

        let rows = db.query_sessions(&SessionFilter::default()).unwrap();
        let isos: Vec<&str> = rows.iter().map(|(_, s)| s.start_iso.as_str()).collect();
        assert_eq!(
            isos,
            vec!["2026-04-27T10:00:00Z", "2026-04-26T10:00:00Z", "2026-04-25T10:00:00Z"],
            "rows must be ordered start_iso DESC",
        );
    }

    #[test]
    fn query_sessions_empty_db_returns_empty_vec() {
        // No rows — not an error, just an empty Vec.
        let db = Database::open_in_memory().unwrap();
        let rows = db.query_sessions(&SessionFilter::default()).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn query_sessions_limit_caps_result_count() {
        // limit=N returns at most N rows; the cap applies AFTER ordering,
        // so the newest N are returned.
        let db = Database::open_in_memory().unwrap();
        for d in 20..28 {
            db.insert_session(&Session {
                start_iso: format!("2026-04-{d:02}T10:00:00Z"),
                duration_secs: 600,
                label_id: None,
                notes: None,
                mode: SessionMode::Timer,
                uuid: String::new(),
                guided_file_uuid: None,
            }).unwrap();
        }
        let rows = db.query_sessions(&SessionFilter {
            limit: Some(3), ..Default::default()
        }).unwrap();
        let isos: Vec<&str> = rows.iter().map(|(_, s)| s.start_iso.as_str()).collect();
        assert_eq!(
            isos,
            vec!["2026-04-27T10:00:00Z", "2026-04-26T10:00:00Z", "2026-04-25T10:00:00Z"],
            "limit=3 must return the newest 3",
        );
    }

    #[test]
    fn query_sessions_offset_skips_initial_rows() {
        // offset=N skips the first N (in DESC order). Combined with
        // limit, this is the pagination contract: "give me page p of size s"
        // is offset = (p-1)*s, limit = s.
        let db = Database::open_in_memory().unwrap();
        for d in 20..28 {
            db.insert_session(&Session {
                start_iso: format!("2026-04-{d:02}T10:00:00Z"),
                duration_secs: 600,
                label_id: None,
                notes: None,
                mode: SessionMode::Timer,
                uuid: String::new(),
                guided_file_uuid: None,
            }).unwrap();
        }
        // Page 2 of size 3: skip 3, take 3.
        let rows = db.query_sessions(&SessionFilter {
            limit: Some(3),
            offset: Some(3),
            ..Default::default()
        }).unwrap();
        let isos: Vec<&str> = rows.iter().map(|(_, s)| s.start_iso.as_str()).collect();
        assert_eq!(
            isos,
            vec!["2026-04-24T10:00:00Z", "2026-04-23T10:00:00Z", "2026-04-22T10:00:00Z"],
            "page 2 of size 3 must be rows 4-6 in DESC order",
        );
    }

    #[test]
    fn query_sessions_offset_past_total_returns_empty() {
        // Asking for a page past the end is not an error.
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let rows = db.query_sessions(&SessionFilter {
            offset: Some(100),
            ..Default::default()
        }).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn query_sessions_label_id_filters_by_label() {
        // label_id=Some(id) keeps only sessions referencing that label.
        let db = Database::open_in_memory().unwrap();
        let morning = db.insert_label("Morning").unwrap();
        let evening = db.insert_label("Evening").unwrap();
        // 2 Morning, 1 Evening, 1 unlabeled.
        db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600, label_id: Some(morning),
            notes: None, mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T11:00:00Z".to_string(),
            duration_secs: 600, label_id: Some(morning),
            notes: None, mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T19:00:00Z".to_string(),
            duration_secs: 600, label_id: Some(evening),
            notes: None, mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T20:00:00Z".to_string(),
            duration_secs: 600, label_id: None,
            notes: None, mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();

        let rows = db.query_sessions(&SessionFilter {
            label_id: Some(morning), ..Default::default()
        }).unwrap();
        assert_eq!(rows.len(), 2);
        for (_, s) in &rows {
            assert_eq!(s.label_id, Some(morning));
        }
    }

    #[test]
    fn query_sessions_only_with_notes_excludes_empty_and_null() {
        // only_with_notes=true matches when notes IS NOT NULL AND notes != ''.
        // Both None (NULL in DB) and Some("") must be excluded.
        let db = Database::open_in_memory().unwrap();
        // With note.
        db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600, label_id: None,
            notes: Some("kept focus".to_string()),
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        // Without note (None).
        db.insert_session(&Session {
            start_iso: "2026-04-27T11:00:00Z".to_string(),
            duration_secs: 600, label_id: None,
            notes: None, mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        // Empty-string note — also excluded.
        db.insert_session(&Session {
            start_iso: "2026-04-27T12:00:00Z".to_string(),
            duration_secs: 600, label_id: None,
            notes: Some("".to_string()),
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();

        let rows = db.query_sessions(&SessionFilter {
            only_with_notes: true, ..Default::default()
        }).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1.notes, Some("kept focus".to_string()));
    }

    #[test]
    fn query_sessions_combines_label_filter_and_notes_filter() {
        // Compound filter: label_id AND only_with_notes both apply.
        let db = Database::open_in_memory().unwrap();
        let morning = db.insert_label("Morning").unwrap();
        // Morning + note → kept.
        db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600, label_id: Some(morning),
            notes: Some("yes".to_string()),
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        // Morning, no note → dropped (notes filter).
        db.insert_session(&Session {
            start_iso: "2026-04-27T11:00:00Z".to_string(),
            duration_secs: 600, label_id: Some(morning),
            notes: None, mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        // No label, with note → dropped (label filter).
        db.insert_session(&Session {
            start_iso: "2026-04-27T12:00:00Z".to_string(),
            duration_secs: 600, label_id: None,
            notes: Some("orphan".to_string()),
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();

        let rows = db.query_sessions(&SessionFilter {
            label_id: Some(morning),
            only_with_notes: true,
            ..Default::default()
        }).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1.notes, Some("yes".to_string()));
    }

    #[test]
    fn query_sessions_pagination_walks_all_rows_without_overlap() {
        // Walking pages of size N covers every row exactly once.
        let db = Database::open_in_memory().unwrap();
        for d in 1..=10 {
            db.insert_session(&Session {
                start_iso: format!("2026-04-{d:02}T10:00:00Z"),
                duration_secs: 600, label_id: None,
                notes: None, mode: SessionMode::Timer,
                uuid: String::new(),
                guided_file_uuid: None,
            }).unwrap();
        }
        let mut seen: Vec<i64> = Vec::new();
        let mut offset = 0u32;
        loop {
            let page = db.query_sessions(&SessionFilter {
                limit: Some(3),
                offset: Some(offset),
                ..Default::default()
            }).unwrap();
            if page.is_empty() { break; }
            for (id, _) in &page { seen.push(*id); }
            offset += page.len() as u32;
        }
        assert_eq!(seen.len(), 10);
        // No duplicates.
        let mut sorted = seen.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), 10);
    }

    #[test]
    fn get_setting_returns_default_when_key_missing() {
        // Reads of unset keys fall back to the caller-provided default
        // (no INSERT, no error).
        let db = Database::open_in_memory().unwrap();
        assert_eq!(
            db.get_setting("timer_presets", "5,10,15,20,30").unwrap(),
            "5,10,15,20,30",
        );
        // The key remained absent — getting it again returns the same default.
        assert_eq!(
            db.get_setting("timer_presets", "5,10,15,20,30").unwrap(),
            "5,10,15,20,30",
        );
    }

    #[test]
    fn set_setting_then_get_setting_round_trip() {
        // Setting a key persists the value; subsequent gets ignore the
        // default and return the stored value verbatim.
        let db = Database::open_in_memory().unwrap();
        db.set_setting("timer_presets", "3,7,12").unwrap();
        assert_eq!(
            db.get_setting("timer_presets", "5,10,15,20,30").unwrap(),
            "3,7,12",
        );
    }

    #[test]
    fn set_setting_overwrites_existing_value() {
        // Repeat sets overwrite (UPSERT semantics). The second value
        // wins; the row count stays at 1 per key.
        let db = Database::open_in_memory().unwrap();
        db.set_setting("daily_goal_mins", "20").unwrap();
        db.set_setting("daily_goal_mins", "25").unwrap();
        assert_eq!(db.get_setting("daily_goal_mins", "0").unwrap(), "25");
    }

    #[test]
    fn settings_keys_are_independent() {
        // Setting key A does not affect key B's value or default.
        let db = Database::open_in_memory().unwrap();
        db.set_setting("daily_goal_mins", "20").unwrap();
        // Other keys still return their defaults.
        assert_eq!(db.get_setting("weekly_goal_mins", "150").unwrap(), "150");
        // The set key is unaffected.
        assert_eq!(db.get_setting("daily_goal_mins", "0").unwrap(), "20");
    }

    #[test]
    fn set_setting_accepts_empty_string_and_unicode() {
        // Values are opaque to the DB layer — UTF-8 string in, UTF-8 string out.
        let db = Database::open_in_memory().unwrap();
        db.set_setting("note_template", "").unwrap();
        assert_eq!(db.get_setting("note_template", "fallback").unwrap(), "");
        db.set_setting("greeting", "こんにちは ☀️").unwrap();
        assert_eq!(db.get_setting("greeting", "").unwrap(), "こんにちは ☀️");
    }

    #[test]
    fn is_label_name_taken_false_for_empty_db() {
        // Nothing exists ⇒ no name is taken.
        let db = Database::open_in_memory().unwrap();
        assert!(!db.is_label_name_taken("Morning", 0).unwrap());
    }

    #[test]
    fn is_label_name_taken_true_for_existing_other_label() {
        // Another row holds this name. Exclude id is something else.
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let evening = db.insert_label("Evening").unwrap();
        // Asking "is 'Morning' taken by anyone other than `evening`?"
        // returns true because Morning is held by a different row.
        assert!(db.is_label_name_taken("Morning", evening).unwrap());
    }

    #[test]
    fn is_label_name_taken_false_when_only_owner_is_excluded() {
        // The single row holding this name is the one being excluded —
        // typical pre-rename validation: 'is this name taken by anyone
        // OTHER THAN the row I'm about to update?'
        let db = Database::open_in_memory().unwrap();
        let morning = db.insert_label("Morning").unwrap();
        assert!(!db.is_label_name_taken("Morning", morning).unwrap());
    }

    #[test]
    fn is_label_name_taken_is_case_insensitive() {
        // The column is COLLATE NOCASE — name comparison must follow.
        let db = Database::open_in_memory().unwrap();
        let morning = db.insert_label("Morning").unwrap();
        // Different casing of an existing name is still 'taken'.
        assert!(db.is_label_name_taken("morning", 0).unwrap());
        assert!(db.is_label_name_taken("MORNING", 0).unwrap());
        // …unless the holder is the excluded row.
        assert!(!db.is_label_name_taken("morning", morning).unwrap());
    }

    #[test]
    fn label_session_count_zero_for_unreferenced_label() {
        // A freshly-created label has no sessions referencing it.
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_label("Morning").unwrap();
        assert_eq!(db.label_session_count(id).unwrap(), 0);
    }

    #[test]
    fn label_session_count_counts_referencing_sessions() {
        // Counts only sessions whose label_id matches this label's id.
        // Sessions without labels and sessions with OTHER labels are not
        // counted.
        let db = Database::open_in_memory().unwrap();
        let morning = db.insert_label("Morning").unwrap();
        let evening = db.insert_label("Evening").unwrap();

        // Three Morning sessions.
        for i in 0..3 {
            db.insert_session(&Session {
                start_iso: format!("2026-04-2{i}T10:00:00Z"),
                duration_secs: 600,
                label_id: Some(morning),
                notes: None,
                mode: SessionMode::Timer,
                uuid: String::new(),
                guided_file_uuid: None,
            }).unwrap();
        }
        // One Evening session — must not contribute to Morning's count.
        db.insert_session(&Session {
            start_iso: "2026-04-27T19:00:00Z".to_string(),
            duration_secs: 600,
            label_id: Some(evening),
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        // Two unlabeled sessions — must not contribute either.
        for i in 0..2 {
            db.insert_session(&Session {
                start_iso: format!("2026-04-2{i}T20:00:00Z"),
                duration_secs: 300,
                label_id: None,
                notes: None,
                mode: SessionMode::Timer,
                uuid: String::new(),
                guided_file_uuid: None,
            }).unwrap();
        }

        assert_eq!(db.label_session_count(morning).unwrap(), 3);
        assert_eq!(db.label_session_count(evening).unwrap(), 1);
    }

    #[test]
    fn label_session_count_unknown_id_is_zero() {
        // No row ⇒ no references; not an error.
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.label_session_count(9999).unwrap(), 0);
    }

    #[test]
    fn delete_label_removes_only_that_row() {
        // Delete addresses one row by id; siblings survive.
        let db = Database::open_in_memory().unwrap();
        let morning = db.insert_label("Morning").unwrap();
        let evening = db.insert_label("Evening").unwrap();

        db.delete_label(morning).unwrap();

        // Morning is gone, Evening remains.
        assert_eq!(db.find_label_by_name("Morning").unwrap(), None);
        assert_eq!(db.find_label_by_name("Evening").unwrap(), Some(evening));
        let names: Vec<String> =
            db.list_labels().unwrap().into_iter().map(|l| l.name).collect();
        assert_eq!(names, vec!["Evening"]);
        assert_eq!(db.count_labels().unwrap(), 1);
    }

    #[test]
    fn delete_label_unknown_id_is_noop() {
        // Matches SQLite DELETE semantics.
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_label("Morning").unwrap();
        db.delete_label(id + 999).unwrap();
        assert_eq!(db.count_labels().unwrap(), 1);
        assert_eq!(db.find_label_by_name("Morning").unwrap(), Some(id));
    }

    #[test]
    fn delete_label_unlinks_sessions_via_set_null() {
        // Deleting a label must NOT destroy historical sessions — the
        // FK is ON DELETE SET NULL on the sessions side, so referenced
        // sessions survive with label_id = None.
        let db = Database::open_in_memory().unwrap();
        let morning = db.insert_label("Morning").unwrap();

        let labeled_id = db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: Some(morning),
            notes: Some("first sit".to_string()),
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        // A second labeled session — proves the unlink happens for ALL
        // referencing rows, not just the first.
        let labeled_id2 = db.insert_session(&Session {
            start_iso: "2026-04-27T11:00:00Z".to_string(),
            duration_secs: 1200,
            label_id: Some(morning),
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        // An unlabeled control — must remain unlabeled (was None, stays None).
        let unlabeled_id = db.insert_session(&Session {
            start_iso: "2026-04-27T12:00:00Z".to_string(),
            duration_secs: 300,
            label_id: None,
            notes: None,
            mode: SessionMode::BoxBreath,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();

        db.delete_label(morning).unwrap();

        // Both formerly-labeled sessions survive but have lost their label.
        let rows = db.list_sessions().unwrap();
        assert_eq!(rows.len(), 3, "all sessions must survive label deletion");
        let by_id: std::collections::HashMap<i64, &Session> =
            rows.iter().map(|(i, s)| (*i, s)).collect();
        assert_eq!(by_id[&labeled_id].label_id, None);
        assert_eq!(by_id[&labeled_id2].label_id, None);
        assert_eq!(by_id[&unlabeled_id].label_id, None);

        // The label row is gone.
        assert_eq!(db.count_labels().unwrap(), 0);
    }

    #[test]
    fn delete_label_does_not_affect_unrelated_sessions() {
        // Sessions referencing OTHER labels are untouched when one
        // label is deleted.
        let db = Database::open_in_memory().unwrap();
        let morning = db.insert_label("Morning").unwrap();
        let evening = db.insert_label("Evening").unwrap();

        let evening_id = db.insert_session(&Session {
            start_iso: "2026-04-27T19:00:00Z".to_string(),
            duration_secs: 600,
            label_id: Some(evening),
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();

        db.delete_label(morning).unwrap();

        // Evening session still points at Evening label.
        let row = &db.list_sessions().unwrap()[0];
        assert_eq!(row.0, evening_id);
        assert_eq!(row.1.label_id, Some(evening));
    }

    #[test]
    fn update_label_renames_row() {
        // Rename takes id + new name. The row keeps its id but the
        // name changes; sibling labels are untouched.
        let db = Database::open_in_memory().unwrap();
        let morning = db.insert_label("Morning").unwrap();
        let evening = db.insert_label("Evening").unwrap();

        db.update_label(morning, "Pre-coffee").unwrap();

        // Morning row now reports the new name.
        assert_eq!(db.find_label_by_name("Pre-coffee").unwrap(), Some(morning));
        // Old name is gone.
        assert_eq!(db.find_label_by_name("Morning").unwrap(), None);
        // Sibling untouched.
        assert_eq!(db.find_label_by_name("Evening").unwrap(), Some(evening));
        // Count unchanged.
        assert_eq!(db.count_labels().unwrap(), 2);
    }

    #[test]
    fn update_label_to_same_name_is_idempotent() {
        // Renaming to the current name is a no-op, not a UNIQUE violation.
        // The row updates "to itself" — SQLite UPDATE allows this.
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_label("Morning").unwrap();
        db.update_label(id, "Morning").unwrap();
        // Still one row, still the same id.
        assert_eq!(db.count_labels().unwrap(), 1);
        assert_eq!(db.find_label_by_name("Morning").unwrap(), Some(id));
    }

    #[test]
    fn list_labels_returns_label_per_row_alphabetic_by_name() {
        // Each retrieved Label carries its rowid so callers can address it
        // for update/delete. Order is alphabetic-by-name (case-insensitive)
        // for stable UI rendering.
        let db = Database::open_in_memory().unwrap();
        let evening = db.insert_label("Evening").unwrap();
        let morning = db.insert_label("Morning").unwrap();
        let afternoon = db.insert_label("Afternoon").unwrap();

        let rows = db.list_labels().unwrap();
        assert_eq!(rows.len(), 3);
        // Every label must carry a v4 uuid, and uuids must be pairwise
        // distinct (UNIQUE constraint at the column level guarantees that;
        // assert it here to document the contract from the read side).
        let uuids: std::collections::HashSet<_> =
            rows.iter().map(|l| l.uuid.clone()).collect();
        assert_eq!(uuids.len(), 3, "labels must have distinct uuids");
        for label in &rows {
            assert!(looks_like_uuid_v4(&label.uuid),
                "label {label:?} missing v4 uuid");
        }
        // Compare full structs — bind each label's uuid into the expected
        // value so id, name AND uuid all participate in the assertion.
        let by_name: std::collections::HashMap<_, _> =
            rows.iter().map(|l| (l.name.clone(), l.uuid.clone())).collect();
        assert_eq!(rows, vec![
            Label { id: afternoon, name: "Afternoon".to_string(),
                uuid: by_name["Afternoon"].clone() },
            Label { id: evening,   name: "Evening".to_string(),
                uuid: by_name["Evening"].clone() },
            Label { id: morning,   name: "Morning".to_string(),
                uuid: by_name["Morning"].clone() },
        ]);
    }

    #[test]
    fn list_labels_returns_label_per_row_case_insensitive_sort() {
        // The column is COLLATE NOCASE — sort must follow, so 'apple',
        // 'Banana', 'cherry' come back in that order even with mixed case.
        let db = Database::open_in_memory().unwrap();
        let banana = db.insert_label("Banana").unwrap();
        let cherry = db.insert_label("cherry").unwrap();
        let apple = db.insert_label("apple").unwrap();
        let rows = db.list_labels().unwrap();
        let names: Vec<&str> = rows.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, vec!["apple", "Banana", "cherry"]);
        // Each row carries the original casing (no normalisation on read).
        assert_eq!(rows[0].id, apple);
        assert_eq!(rows[1].id, banana);
        assert_eq!(rows[2].id, cherry);
    }

    #[test]
    fn update_label_to_case_variant_of_own_name_succeeds() {
        // Capitalising "morning" → "Morning" is a legitimate rename of
        // the same row. Because of COLLATE NOCASE on UNIQUE, SQLite
        // does NOT see this as a collision against itself.
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_label("morning").unwrap();
        db.update_label(id, "Morning").unwrap();
        // Lookup by either case still finds the row (NOCASE column).
        assert_eq!(db.find_label_by_name("morning").unwrap(), Some(id));
        assert_eq!(db.find_label_by_name("Morning").unwrap(), Some(id));
        // The actual stored value is the new casing.
        let names: Vec<String> =
            db.list_labels().unwrap().into_iter().map(|l| l.name).collect();
        assert_eq!(names, vec!["Morning"]);
    }

    #[test]
    fn update_label_to_existing_other_name_returns_duplicate_error() {
        // Renaming to a name another row already has must fail with
        // DuplicateLabel. The DB stays unchanged.
        let db = Database::open_in_memory().unwrap();
        let morning = db.insert_label("Morning").unwrap();
        let _evening = db.insert_label("Evening").unwrap();

        let result = db.update_label(morning, "Evening");
        assert!(
            matches!(result, Err(DbError::DuplicateLabel(ref n)) if n == "Evening"),
            "expected DuplicateLabel(\"Evening\"), got {result:?}"
        );
        // Both rows survive with their original names.
        assert_eq!(db.find_label_by_name("Morning").unwrap(), Some(morning));
        let names: Vec<String> =
            db.list_labels().unwrap().into_iter().map(|l| l.name).collect();
        assert_eq!(names, vec!["Evening", "Morning"]);
    }

    #[test]
    fn update_label_to_case_variant_of_other_name_returns_duplicate_error() {
        // Case-insensitive collision: renaming "Morning" to "evening"
        // collides with existing "Evening" because labels.name is
        // COLLATE NOCASE.
        let db = Database::open_in_memory().unwrap();
        let morning = db.insert_label("Morning").unwrap();
        db.insert_label("Evening").unwrap();

        let result = db.update_label(morning, "evening");
        assert!(
            matches!(result, Err(DbError::DuplicateLabel(ref n)) if n == "evening"),
            "expected DuplicateLabel(\"evening\"), got {result:?}"
        );
    }

    #[test]
    fn update_label_unknown_id_is_noop() {
        // Matches the SQLite UPDATE-zero-rows convention shared by
        // update_session: missing id is silent.
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_label("Morning").unwrap();
        db.update_label(id + 999, "Phantom").unwrap();
        // Original row untouched; phantom name not present.
        assert_eq!(db.find_label_by_name("Morning").unwrap(), Some(id));
        assert_eq!(db.find_label_by_name("Phantom").unwrap(), None);
        assert_eq!(db.count_labels().unwrap(), 1);
    }

    #[test]
    fn insert_label_returns_new_rowid() {
        // insert_label returns the AUTOINCREMENT id of the new row,
        // matching insert_session's contract. AUTOINCREMENT starts at 1.
        let db = Database::open_in_memory().unwrap();
        let id1 = db.insert_label("Morning").unwrap();
        let id2 = db.insert_label("Evening").unwrap();
        let id3 = db.insert_label("Afternoon").unwrap();
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
        assert_eq!(id3, 3);
        // The returned id matches what find_label_by_name reports.
        assert_eq!(db.find_label_by_name("Morning").unwrap(), Some(id1));
        assert_eq!(db.find_label_by_name("Evening").unwrap(), Some(id2));
    }

    #[test]
    fn find_or_create_label_creates_when_missing() {
        // First call to a fresh DB inserts the label and returns its new id.
        let db = Database::open_in_memory().unwrap();
        let id = db.find_or_create_label("Morning").unwrap();
        assert_eq!(db.count_labels().unwrap(), 1);
        // The returned id matches what find_label_by_name reports.
        assert_eq!(db.find_label_by_name("Morning").unwrap(), Some(id));
    }

    #[test]
    fn find_or_create_label_returns_existing_id() {
        // If the label already exists, the existing id is returned and
        // no new row is created.
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let existing = db.find_label_by_name("Morning").unwrap().unwrap();
        let got = db.find_or_create_label("Morning").unwrap();
        assert_eq!(got, existing);
        assert_eq!(db.count_labels().unwrap(), 1);
    }

    #[test]
    fn find_or_create_label_is_case_insensitive() {
        // CSV import frequently differs in case from what the user
        // already has; we must reuse the existing row, not duplicate.
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let existing = db.find_label_by_name("Morning").unwrap().unwrap();
        // Lookup with different casings — same id, no new rows.
        assert_eq!(db.find_or_create_label("morning").unwrap(), existing);
        assert_eq!(db.find_or_create_label("MORNING").unwrap(), existing);
        assert_eq!(db.count_labels().unwrap(), 1);
    }

    #[test]
    fn find_or_create_label_idempotent_across_calls() {
        // Calling repeatedly never inflates the row count.
        let db = Database::open_in_memory().unwrap();
        let id1 = db.find_or_create_label("Evening").unwrap();
        let id2 = db.find_or_create_label("Evening").unwrap();
        let id3 = db.find_or_create_label("evening").unwrap(); // case variant
        assert_eq!(id1, id2);
        assert_eq!(id1, id3);
        assert_eq!(db.count_labels().unwrap(), 1);
    }

    #[test]
    fn label_uniqueness_is_case_insensitive() {
        // Avoid "Morning" / "morning" as separate rows. The DB enforces
        // case-insensitive uniqueness so UI bugs that skip pre-validation
        // (is_label_name_taken) still get caught at the DB layer.
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let result = db.insert_label("morning");
        assert!(
            matches!(result, Err(DbError::DuplicateLabel(ref name)) if name == "morning"),
            "expected DuplicateLabel for 'morning', got {result:?}"
        );
        // Different mixed-case is also a duplicate.
        assert!(matches!(db.insert_label("MORNING"), Err(DbError::DuplicateLabel(_))));
        assert!(matches!(db.insert_label("MoRnInG"), Err(DbError::DuplicateLabel(_))));
        // Only the original survives.
        assert_eq!(db.count_labels().unwrap(), 1);
    }

    #[test]
    fn find_label_by_name_is_case_insensitive() {
        // Lookups follow the column's NOCASE collation so a case-different
        // search still finds the existing row — same id, same row.
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let canonical_id = db.find_label_by_name("Morning").unwrap();
        assert!(canonical_id.is_some());
        // All these case variants must return the SAME id.
        assert_eq!(db.find_label_by_name("morning").unwrap(), canonical_id);
        assert_eq!(db.find_label_by_name("MORNING").unwrap(), canonical_id);
        assert_eq!(db.find_label_by_name("MoRnInG").unwrap(), canonical_id);
    }

    #[test]
    fn duplicate_label_error_identifies_offending_name() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let err = db.insert_label("Morning").unwrap_err();
        assert!(
            matches!(err, DbError::DuplicateLabel(ref name) if name == "Morning"),
            "expected DuplicateLabel(\"Morning\"), got {err:?}"
        );
    }

    #[test]
    fn list_labels_returns_inserted_names_alphabetically() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        db.insert_label("Afternoon").unwrap();
        db.insert_label("Evening").unwrap();
        let names: Vec<String> =
            db.list_labels().unwrap().into_iter().map(|l| l.name).collect();
        assert_eq!(names, vec!["Afternoon", "Evening", "Morning"]);
    }

    #[test]
    fn find_label_by_name_returns_some_id_when_present() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let id = db.find_label_by_name("Morning").unwrap();
        assert!(id.is_some());
    }

    #[test]
    fn find_label_by_name_returns_none_when_absent() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.find_label_by_name("Morning").unwrap(), None);
    }

    #[test]
    fn empty_database_has_zero_sessions() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.count_sessions().unwrap(), 0);
    }

    #[test]
    fn insert_session_increases_count() {
        let db = Database::open_in_memory().unwrap();
        let session = Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        };
        db.insert_session(&session).unwrap();
        assert_eq!(db.count_sessions().unwrap(), 1);
    }

    #[test]
    fn insert_session_with_mode_guided_is_accepted_by_check_constraint() {
        // Sessions saved at the end of a guided meditation carry
        // mode='guided'. The schema's CHECK clause must accept it
        // alongside 'timer' and 'box_breath' or insert fails.
        let db = Database::open_in_memory().unwrap();
        let session = Session {
            start_iso: "2026-05-05T20:30:00Z".to_string(),
            duration_secs: 1200,
            label_id: None,
            notes: None,
            mode: SessionMode::Guided,
            uuid: String::new(),
            guided_file_uuid: None,
        };
        db.insert_session(&session).unwrap();
        assert_eq!(db.count_sessions().unwrap(), 1);
    }

    #[test]
    fn insert_session_with_guided_file_uuid_round_trips() {
        // A guided session that played a starred imported file carries
        // the file's uuid so the log / stats can show per-file aggregates
        // later. Verifies the column is actually persisted + read back.
        let db = Database::open_in_memory().unwrap();
        let file_uuid = "deadbeef-1234-5678-9abc-def012345678";
        let session = Session {
            start_iso: "2026-05-05T20:30:00Z".to_string(),
            duration_secs: 1200,
            label_id: None,
            notes: None,
            mode: SessionMode::Guided,
            uuid: String::new(),
            guided_file_uuid: Some(file_uuid.to_string()),
        };
        db.insert_session(&session).unwrap();
        let rows = db.query_sessions(&SessionFilter::default()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1.guided_file_uuid.as_deref(), Some(file_uuid));
    }

    #[test]
    fn insert_session_without_guided_file_uuid_round_trips_as_none() {
        // Transient one-off guided sessions don't reference a
        // library-stored file; the column must accept NULL.
        let db = Database::open_in_memory().unwrap();
        let session = Session {
            start_iso: "2026-05-05T21:00:00Z".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Guided,
            uuid: String::new(),
            guided_file_uuid: None,
        };
        db.insert_session(&session).unwrap();
        let rows = db.query_sessions(&SessionFilter::default()).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].1.guided_file_uuid.is_none());
    }

    #[test]
    fn list_sessions_for_label_filters_by_label_id() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let morning = db.find_label_by_name("Morning").unwrap().unwrap();
        let mut labeled = Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: Some(morning),
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        };
        let unlabeled = Session {
            start_iso: "2026-04-27T19:00:00Z".to_string(),
            duration_secs: 300,
            label_id: None,
            notes: None,
            mode: SessionMode::BoxBreath,
            uuid: String::new(),
            guided_file_uuid: None,
        };
        let labeled_id = db.insert_session(&labeled).unwrap();
        db.insert_session(&unlabeled).unwrap();
        let rows = db.list_sessions_for_label(morning).unwrap();
        assert_eq!(rows.len(), 1, "only the labeled session must be returned");
        assert!(looks_like_uuid_v4(&rows[0].1.uuid));
        labeled.uuid = rows[0].1.uuid.clone();
        assert_eq!(rows, vec![(labeled_id, labeled)]);
    }

    #[test]
    fn list_sessions_round_trips_inserted_session() {
        let db = Database::open_in_memory().unwrap();
        let mut session = Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: Some("felt clear today".to_string()),
            mode: SessionMode::BoxBreath,
            uuid: String::new(),
            guided_file_uuid: None,
        };
        let id = db.insert_session(&session).unwrap();
        let rows = db.list_sessions().unwrap();
        assert_eq!(rows.len(), 1);
        assert!(looks_like_uuid_v4(&rows[0].1.uuid),
            "round-tripped session must carry a v4 uuid");
        // Adopt the DB-assigned uuid into the expected value so the full
        // struct comparison below covers every other field exactly.
        session.uuid = rows[0].1.uuid.clone();
        assert_eq!(rows, vec![(id, session)]);
    }

    #[test]
    fn list_sessions_returns_id_per_row_in_insert_order() {
        // Each retrieved row carries its DB rowid so callers can address it
        // for update / delete. Ids are SQLite AUTOINCREMENT, so they
        // increase strictly and start at 1 on a fresh DB.
        let db = Database::open_in_memory().unwrap();
        let make = |start: &str| Session {
            start_iso: start.to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        };
        let id1 = db.insert_session(&make("2026-04-27T10:00:00Z")).unwrap();
        let id2 = db.insert_session(&make("2026-04-27T11:00:00Z")).unwrap();
        let id3 = db.insert_session(&make("2026-04-27T12:00:00Z")).unwrap();
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
        assert_eq!(id3, 3);
        let rows = db.list_sessions().unwrap();
        let got_ids: Vec<i64> = rows.iter().map(|(id, _)| *id).collect();
        assert_eq!(got_ids, vec![id1, id2, id3]);
    }

    #[test]
    fn update_session_replaces_all_fields() {
        // Update is destructive: every field of the new Session value
        // overwrites the row, identified by id. The other rows stay
        // untouched.
        let db = Database::open_in_memory().unwrap();
        let original = Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: Some("first take".to_string()),
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        };
        let id = db.insert_session(&original).unwrap();

        // Insert a sibling that must remain untouched.
        let other_id = db.insert_session(&Session {
            start_iso: "2026-04-27T11:00:00Z".to_string(),
            duration_secs: 300,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();

        db.insert_label("Evening").unwrap();
        let evening = db.find_label_by_name("Evening").unwrap().unwrap();
        let mut updated = Session {
            start_iso: "2026-04-28T19:00:00Z".to_string(),
            duration_secs: 1500,
            label_id: Some(evening),
            notes: Some("after dinner".to_string()),
            mode: SessionMode::BoxBreath,
            uuid: String::new(),
            guided_file_uuid: None,
        };
        db.update_session(id, &updated).unwrap();

        let rows = db.list_sessions().unwrap();
        assert_eq!(rows.len(), 2);
        // Updated row reflects every new field. Its uuid is whatever the
        // DB assigned at insert time and must survive an update unchanged
        // — bind it into `updated.uuid` for the full struct comparison.
        let updated_row = rows.iter().find(|(rid, _)| *rid == id).unwrap();
        assert!(looks_like_uuid_v4(&updated_row.1.uuid));
        updated.uuid = updated_row.1.uuid.clone();
        assert_eq!(updated_row.1, updated);
        // Sibling row untouched.
        let other_row = rows.iter().find(|(rid, _)| *rid == other_id).unwrap();
        assert_eq!(other_row.1.start_iso, "2026-04-27T11:00:00Z");
        assert_eq!(other_row.1.duration_secs, 300);
        assert_eq!(other_row.1.mode, SessionMode::Timer);
        // Each row must carry its own distinct uuid.
        assert!(looks_like_uuid_v4(&other_row.1.uuid));
        assert_ne!(updated_row.1.uuid, other_row.1.uuid);
    }

    #[test]
    fn update_session_can_clear_label_and_notes() {
        // Optional fields go round-trip in both directions: a session
        // with a label/note can have them cleared by update.
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let morning = db.find_label_by_name("Morning").unwrap().unwrap();
        let id = db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: Some(morning),
            notes: Some("had a label".to_string()),
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        db.update_session(id, &Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let row = &db.list_sessions().unwrap()[0].1;
        assert_eq!(row.label_id, None);
        assert_eq!(row.notes, None);
    }

    #[test]
    fn update_session_unknown_id_is_noop() {
        // Updating a non-existent row is silent — matches SQLite's
        // UPDATE-by-id behaviour. The DB stays unchanged.
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        db.update_session(id + 999, &Session {
            start_iso: "2099-01-01T00:00:00Z".to_string(),
            duration_secs: 9999,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        // Original row is intact.
        let rows = db.list_sessions().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1.duration_secs, 600);
        assert_eq!(rows[0].1.start_iso, "2026-04-27T10:00:00Z");
    }

    #[test]
    fn delete_session_removes_only_the_addressed_row() {
        // Delete addresses one row by id; siblings are untouched.
        let db = Database::open_in_memory().unwrap();
        let make = |start: &str| Session {
            start_iso: start.to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        };
        let id1 = db.insert_session(&make("2026-04-27T10:00:00Z")).unwrap();
        let id2 = db.insert_session(&make("2026-04-27T11:00:00Z")).unwrap();
        let id3 = db.insert_session(&make("2026-04-27T12:00:00Z")).unwrap();

        db.delete_session(id2).unwrap();

        let surviving_ids: Vec<i64> =
            db.list_sessions().unwrap().into_iter().map(|(i, _)| i).collect();
        assert_eq!(surviving_ids, vec![id1, id3]);
        assert_eq!(db.count_sessions().unwrap(), 2);
    }

    #[test]
    fn delete_session_unknown_id_is_noop() {
        // Matches SQLite DELETE semantics: missing id is silent.
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        db.delete_session(id + 999).unwrap();
        // Original row still there.
        assert_eq!(db.count_sessions().unwrap(), 1);
    }

    #[test]
    fn delete_session_does_not_remove_referenced_label() {
        // Labels survive their sessions — the FK is set-null on the
        // sessions side, not cascade-delete on the labels side.
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let morning = db.find_label_by_name("Morning").unwrap().unwrap();
        let id = db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: Some(morning),
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();

        db.delete_session(id).unwrap();

        // Label outlives the session.
        let names: Vec<String> =
            db.list_labels().unwrap().into_iter().map(|l| l.name).collect();
        assert_eq!(names, vec!["Morning"]);
        assert_eq!(db.count_labels().unwrap(), 1);
    }

    #[test]
    fn insert_session_with_unknown_label_id_is_rejected_by_fk() {
        // The labels.id ↔ sessions.label_id link is an enforced foreign key,
        // not just documentation. Inserting a session that points at a
        // non-existent label fails — the DB is the last line of defense
        // against UI bugs that pass through bad ids.
        let db = Database::open_in_memory().unwrap();
        // Sanity: the PRAGMA must be on for the FK clause to actually fire.
        let pragma: i64 = db.conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
        assert_eq!(pragma, 1, "PRAGMA foreign_keys must be ON");

        let bad_id = 9999i64;
        let result = db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: Some(bad_id),
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        });
        assert!(result.is_err(), "expected FK violation, got {result:?}");
        // No row landed.
        assert_eq!(db.count_sessions().unwrap(), 0);
    }

    #[test]
    fn bulk_insert_sessions_inserts_every_row_and_returns_count() {
        // Bulk insert is the import-CSV path's transactional API: every
        // row in the slice goes in (or none on error — see rollback test).
        // Returns the count for "imported N sessions" toasts.
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let morning = db.find_label_by_name("Morning").unwrap().unwrap();

        let to_insert = vec![
            Session {
                start_iso: "2026-04-27T10:00:00Z".to_string(),
                duration_secs: 600,
                label_id: Some(morning),
                notes: Some("first".to_string()),
                mode: SessionMode::Timer,
                uuid: String::new(),
                guided_file_uuid: None,
            },
            Session {
                start_iso: "2026-04-27T11:00:00Z".to_string(),
                duration_secs: 1200,
                label_id: None,
                notes: None,
                mode: SessionMode::Timer,
                uuid: String::new(),
                guided_file_uuid: None,
            },
            Session {
                start_iso: "2026-04-27T12:00:00Z".to_string(),
                duration_secs: 300,
                label_id: Some(morning),
                notes: None,
                mode: SessionMode::BoxBreath,
                uuid: String::new(),
                guided_file_uuid: None,
            },
        ];

        let n = db.bulk_insert_sessions(&to_insert).unwrap();
        assert_eq!(n, 3);
        assert_eq!(db.count_sessions().unwrap(), 3);

        // Every row round-trips through the DB unchanged. The DB assigns
        // each row a fresh v4 uuid that the input doesn't carry — verify
        // each is well-formed, then graft it onto the expected value
        // before comparing the rest of the fields.
        let mut stored: Vec<Session> = db.list_sessions()
            .unwrap()
            .into_iter()
            .map(|(_, s)| s)
            .collect();
        let mut expected = to_insert.clone();
        for (got, want) in stored.iter().zip(expected.iter_mut()) {
            assert!(looks_like_uuid_v4(&got.uuid),
                "bulk-inserted row missing v4 uuid: {got:?}");
            want.uuid = got.uuid.clone();
        }
        // All uuids must also be distinct.
        let unique: std::collections::HashSet<_> =
            stored.iter().map(|s| s.uuid.clone()).collect();
        assert_eq!(unique.len(), stored.len(), "bulk insert must give unique uuids");
        // Strip nothing here: we've populated `expected.uuid` to match.
        let _ = stored.iter_mut(); // silence "doesn't need mut" if linter trips
        assert_eq!(stored, expected);
    }

    #[test]
    fn bulk_insert_sessions_empty_slice_is_zero_and_no_op() {
        // Empty input is not an error; the DB is unchanged.
        let db = Database::open_in_memory().unwrap();
        let n = db.bulk_insert_sessions(&[]).unwrap();
        assert_eq!(n, 0);
        assert_eq!(db.count_sessions().unwrap(), 0);
    }

    #[test]
    fn bulk_insert_sessions_rolls_back_on_constraint_violation() {
        // If any row in the batch violates a constraint (here: a foreign-key
        // pointing at a non-existent label), the WHOLE batch is reverted —
        // the caller never gets a half-imported DB.
        let db = Database::open_in_memory().unwrap();
        let pre_id = db.insert_session(&Session {
            start_iso: "2026-04-27T09:00:00Z".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        assert_eq!(db.count_sessions().unwrap(), 1);

        let bad_label = 9999i64; // No label has this id.
        let batch = vec![
            Session {
                start_iso: "2026-04-27T10:00:00Z".to_string(),
                duration_secs: 600,
                label_id: None, // OK
                notes: None,
                mode: SessionMode::Timer,
                uuid: String::new(),
                guided_file_uuid: None,
            },
            Session {
                start_iso: "2026-04-27T11:00:00Z".to_string(),
                duration_secs: 600,
                label_id: Some(bad_label), // FK violation
                notes: None,
                mode: SessionMode::Timer,
                uuid: String::new(),
                guided_file_uuid: None,
            },
        ];
        let result = db.bulk_insert_sessions(&batch);
        assert!(result.is_err(), "expected FK violation, got {result:?}");

        // No rows from the failed batch landed; the pre-existing row is intact.
        assert_eq!(db.count_sessions().unwrap(), 1);
        let rows = db.list_sessions().unwrap();
        assert_eq!(rows[0].0, pre_id);
    }

    #[test]
    fn bulk_insert_sessions_is_atomic_with_no_partial_state_visible() {
        // Atomic-on-error: even after a failed bulk insert, count_sessions
        // and list_sessions agree on the pre-batch state. (This pins the
        // contract: "rolled back" means no observable side effect, not
        // just "rows aren't there".)
        let db = Database::open_in_memory().unwrap();
        let bad_label = 9999i64;
        let batch = vec![
            Session {
                start_iso: "2026-04-27T10:00:00Z".to_string(),
                duration_secs: 600,
                label_id: Some(bad_label), // fails immediately
                notes: None,
                mode: SessionMode::Timer,
                uuid: String::new(),
                guided_file_uuid: None,
            },
        ];
        let _ = db.bulk_insert_sessions(&batch);
        assert_eq!(db.count_sessions().unwrap(), 0);
        assert!(db.list_sessions().unwrap().is_empty());
    }

    #[test]
    fn delete_all_sessions_returns_count_and_clears_table() {
        // Wipe-all returns the row count so the caller can show "deleted N
        // sessions" toasts. Labels survive (this is a sessions-only nuke).
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let morning = db.find_label_by_name("Morning").unwrap().unwrap();
        for i in 0..3 {
            db.insert_session(&Session {
                start_iso: format!("2026-04-2{i}T10:00:00Z"),
                duration_secs: 600,
                label_id: Some(morning),
                notes: None,
                mode: SessionMode::Timer,
                uuid: String::new(),
                guided_file_uuid: None,
            }).unwrap();
        }
        assert_eq!(db.count_sessions().unwrap(), 3);

        let removed = db.delete_all_sessions().unwrap();
        assert_eq!(removed, 3);
        assert_eq!(db.count_sessions().unwrap(), 0);
        assert!(db.list_sessions().unwrap().is_empty());

        // Labels untouched.
        let names: Vec<String> =
            db.list_labels().unwrap().into_iter().map(|l| l.name).collect();
        assert_eq!(names, vec!["Morning"]);
    }

    #[test]
    fn delete_all_sessions_on_empty_db_returns_zero() {
        // Idempotent: nothing to delete is not an error.
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.delete_all_sessions().unwrap(), 0);
        assert_eq!(db.count_sessions().unwrap(), 0);
    }

    #[test]
    fn list_sessions_for_label_returns_id_per_row() {
        // Filtered list must also carry ids — same contract.
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let morning = db.find_label_by_name("Morning").unwrap().unwrap();
        let mut labeled = Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: Some(morning),
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        };
        let id = db.insert_session(&labeled).unwrap();
        // Insert a second, unlabeled session — must not appear.
        db.insert_session(&Session {
            start_iso: "2026-04-27T11:00:00Z".to_string(),
            duration_secs: 300,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let rows = db.list_sessions_for_label(morning).unwrap();
        assert_eq!(rows.len(), 1, "only the labeled session must be returned");
        assert!(looks_like_uuid_v4(&rows[0].1.uuid));
        labeled.uuid = rows[0].1.uuid.clone();
        assert_eq!(rows, vec![(id, labeled)]);
    }

    #[test]
    fn total_minutes_sums_durations_across_sessions() {
        let db = Database::open_in_memory().unwrap();
        let session_with_dur = |dur_secs| Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: dur_secs,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        };
        db.insert_session(&session_with_dur(600)).unwrap(); // 10 min
        db.insert_session(&session_with_dur(900)).unwrap(); // 15 min
        assert_eq!(db.total_minutes().unwrap(), 25);
    }

    #[test]
    fn total_minutes_is_zero_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.total_minutes().unwrap(), 0);
    }

    #[test]
    fn total_minutes_by_label_groups_per_label() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Evening").unwrap();
        db.insert_label("Morning").unwrap();
        let evening = db.find_label_by_name("Evening").unwrap();
        let morning = db.find_label_by_name("Morning").unwrap();
        // Morning: 600 + 1200 = 1800s = 30m
        db.insert_session(&Session {
            duration_secs: 600,
            label_id: morning,
            ..session_on("2026-04-27")
        })
        .unwrap();
        db.insert_session(&Session {
            duration_secs: 1200,
            label_id: morning,
            ..session_on("2026-04-26")
        })
        .unwrap();
        // Evening: 300s = 5m
        db.insert_session(&Session {
            duration_secs: 300,
            label_id: evening,
            ..session_on("2026-04-27")
        })
        .unwrap();
        // SQLite default ORDER BY name puts ASCII "Evening" before "Morning".
        assert_eq!(
            db.total_minutes_by_label().unwrap(),
            vec![
                (Some("Evening".to_string()), 5),
                (Some("Morning".to_string()), 30),
            ]
        );
    }

    #[test]
    fn total_minutes_by_label_includes_unlabeled_as_none() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let morning = db.find_label_by_name("Morning").unwrap();
        db.insert_session(&Session {
            duration_secs: 600,
            label_id: morning,
            ..session_on("2026-04-27")
        })
        .unwrap();
        db.insert_session(&Session {
            duration_secs: 300,
            label_id: None,
            ..session_on("2026-04-27")
        })
        .unwrap();
        // SQLite ORDER BY ASC sorts NULL first.
        assert_eq!(
            db.total_minutes_by_label().unwrap(),
            vec![(None, 5), (Some("Morning".to_string()), 10)]
        );
    }

    #[test]
    fn total_minutes_by_label_is_empty_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.total_minutes_by_label().unwrap(), vec![]);
    }

    #[test]
    fn count_sessions_by_label_groups_per_label() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let morning = db.find_label_by_name("Morning").unwrap();
        db.insert_session(&Session {
            label_id: morning,
            ..session_on("2026-04-27")
        })
        .unwrap();
        db.insert_session(&Session {
            label_id: morning,
            ..session_on("2026-04-26")
        })
        .unwrap();
        db.insert_session(&Session {
            label_id: None,
            ..session_on("2026-04-25")
        })
        .unwrap();
        assert_eq!(
            db.count_sessions_by_label().unwrap(),
            vec![(None, 1), (Some("Morning".to_string()), 2)]
        );
    }

    fn date(y: i32, m: u32, d: u32) -> chrono::NaiveDate {
        chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn streak_is_zero_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.get_streak(date(2026, 4, 27)).unwrap(), 0);
    }

    fn session_on(day: &str) -> Session {
        Session {
            start_iso: format!("{day}T10:00:00Z"),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }
    }

    #[test]
    fn streak_is_one_with_single_session_today() {
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&session_on("2026-04-27")).unwrap();
        assert_eq!(db.get_streak(date(2026, 4, 27)).unwrap(), 1);
    }

    #[test]
    fn streak_counts_consecutive_days_back_from_today() {
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&session_on("2026-04-27")).unwrap();
        db.insert_session(&session_on("2026-04-26")).unwrap();
        db.insert_session(&session_on("2026-04-25")).unwrap();
        assert_eq!(db.get_streak(date(2026, 4, 27)).unwrap(), 3);
    }

    #[test]
    fn streak_breaks_at_first_gap() {
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&session_on("2026-04-27")).unwrap();
        // gap on 2026-04-26
        db.insert_session(&session_on("2026-04-25")).unwrap();
        db.insert_session(&session_on("2026-04-24")).unwrap();
        assert_eq!(db.get_streak(date(2026, 4, 27)).unwrap(), 1);
    }

    #[test]
    fn streak_includes_yesterday_when_no_session_today() {
        // Forgiving variant: streak still alive if you meditated yesterday.
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&session_on("2026-04-26")).unwrap();
        db.insert_session(&session_on("2026-04-25")).unwrap();
        assert_eq!(db.get_streak(date(2026, 4, 27)).unwrap(), 2);
    }

    #[test]
    fn streak_is_zero_when_most_recent_session_is_older_than_yesterday() {
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&session_on("2026-04-24")).unwrap();
        assert_eq!(db.get_streak(date(2026, 4, 27)).unwrap(), 0);
    }

    #[test]
    fn streak_counts_each_day_once_even_with_multiple_sessions() {
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T08:00:00Z".to_string(),
            ..session_on("2026-04-27")
        })
        .unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T19:00:00Z".to_string(),
            ..session_on("2026-04-27")
        })
        .unwrap();
        assert_eq!(db.get_streak(date(2026, 4, 27)).unwrap(), 1);
    }

    #[test]
    fn best_streak_is_zero_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.get_best_streak().unwrap(), 0);
    }

    #[test]
    fn streak_for_label_only_counts_sessions_with_that_label() {
        let db = Database::open_in_memory().unwrap();
        let today = date(2026, 4, 27);
        db.insert_label("Morning").unwrap();
        db.insert_label("Evening").unwrap();
        let morning = db.find_label_by_name("Morning").unwrap().unwrap();
        let evening = db.find_label_by_name("Evening").unwrap().unwrap();
        // Today: Morning + Evening sessions.
        db.insert_session(&Session {
            label_id: Some(morning),
            ..session_on("2026-04-27")
        })
        .unwrap();
        db.insert_session(&Session {
            label_id: Some(evening),
            ..session_on("2026-04-27")
        })
        .unwrap();
        // Yesterday: Morning only.
        db.insert_session(&Session {
            label_id: Some(morning),
            ..session_on("2026-04-26")
        })
        .unwrap();
        // 2 days ago: Evening only.
        db.insert_session(&Session {
            label_id: Some(evening),
            ..session_on("2026-04-25")
        })
        .unwrap();
        // Morning streak: today + yesterday = 2 (gap on day-2).
        assert_eq!(db.get_streak_for_label(today, morning).unwrap(), 2);
        // Evening streak: today only (gap on yesterday).
        assert_eq!(db.get_streak_for_label(today, evening).unwrap(), 1);
        // Overall streak (no filter): today + yesterday + day-2 = 3.
        assert_eq!(db.get_streak(today).unwrap(), 3);
    }

    #[test]
    fn streak_and_best_streak_diverge_when_current_run_is_shorter() {
        // Mirrors `streak_gap_separates_current_from_best` from the existing app:
        // an old 6-day run, a gap, then a recent 3-day run ending today.
        let db = Database::open_in_memory().unwrap();
        let today = date(2026, 4, 27);
        // Old run: 30..25 days ago (6 days).
        for offset in 25..=30 {
            let day = today - chrono::Duration::days(offset);
            db.insert_session(&session_on(&day.format("%Y-%m-%d").to_string()))
                .unwrap();
        }
        // Current run: 0..2 days ago (3 days).
        for offset in 0..=2 {
            let day = today - chrono::Duration::days(offset);
            db.insert_session(&session_on(&day.format("%Y-%m-%d").to_string()))
                .unwrap();
        }
        assert_eq!(db.get_streak(today).unwrap(), 3, "current streak");
        assert_eq!(db.get_best_streak().unwrap(), 6, "best historical streak");
    }

    #[test]
    fn best_streak_for_label_only_counts_sessions_with_that_label() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        db.insert_label("Evening").unwrap();
        let morning = db.find_label_by_name("Morning").unwrap().unwrap();
        let evening = db.find_label_by_name("Evening").unwrap().unwrap();
        // Morning has a 3-day run.
        for d in ["2026-04-25", "2026-04-26", "2026-04-27"] {
            db.insert_session(&Session {
                label_id: Some(morning),
                ..session_on(d)
            })
            .unwrap();
        }
        // Evening has a 5-day run (longer overall, but for Morning it's irrelevant).
        for d in [
            "2026-04-01", "2026-04-02", "2026-04-03", "2026-04-04", "2026-04-05",
        ] {
            db.insert_session(&Session {
                label_id: Some(evening),
                ..session_on(d)
            })
            .unwrap();
        }
        assert_eq!(db.get_best_streak_for_label(morning).unwrap(), 3);
        assert_eq!(db.get_best_streak_for_label(evening).unwrap(), 5);
        // Overall best ignores label and finds the longest run anywhere.
        assert_eq!(db.get_best_streak().unwrap(), 5);
    }

    #[test]
    fn best_streak_finds_longest_run_across_history() {
        let db = Database::open_in_memory().unwrap();
        // Run of 2: Apr 1-2
        db.insert_session(&session_on("2026-04-01")).unwrap();
        db.insert_session(&session_on("2026-04-02")).unwrap();
        // Run of 4: Apr 10-13 (the best)
        db.insert_session(&session_on("2026-04-10")).unwrap();
        db.insert_session(&session_on("2026-04-11")).unwrap();
        db.insert_session(&session_on("2026-04-12")).unwrap();
        db.insert_session(&session_on("2026-04-13")).unwrap();
        // Run of 1: Apr 20
        db.insert_session(&session_on("2026-04-20")).unwrap();
        assert_eq!(db.get_best_streak().unwrap(), 4);
    }

    #[test]
    fn daily_totals_groups_durations_by_day() {
        let db = Database::open_in_memory().unwrap();
        // Two sessions same day → summed.
        db.insert_session(&Session {
            duration_secs: 600,
            ..session_on("2026-04-26")
        })
        .unwrap();
        db.insert_session(&Session {
            duration_secs: 300,
            ..session_on("2026-04-26")
        })
        .unwrap();
        // Different day, distinct entry.
        db.insert_session(&Session {
            duration_secs: 1200,
            ..session_on("2026-04-27")
        })
        .unwrap();
        assert_eq!(
            db.get_daily_totals().unwrap(),
            vec![(date(2026, 4, 26), 900), (date(2026, 4, 27), 1200)]
        );
    }

    #[test]
    fn daily_totals_is_empty_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.get_daily_totals().unwrap(), vec![]);
    }

    #[test]
    fn daily_totals_for_label_filters_per_day() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let morning = db.find_label_by_name("Morning").unwrap().unwrap();
        // Morning on Apr 26 (600s) and Apr 27 (1200s).
        db.insert_session(&Session {
            duration_secs: 600,
            label_id: Some(morning),
            ..session_on("2026-04-26")
        })
        .unwrap();
        db.insert_session(&Session {
            duration_secs: 1200,
            label_id: Some(morning),
            ..session_on("2026-04-27")
        })
        .unwrap();
        // Unlabeled on Apr 27 — must NOT show up in Morning's totals.
        db.insert_session(&Session {
            duration_secs: 9999,
            label_id: None,
            ..session_on("2026-04-27")
        })
        .unwrap();
        assert_eq!(
            db.get_daily_totals_for_label(morning).unwrap(),
            vec![(date(2026, 4, 26), 600), (date(2026, 4, 27), 1200)]
        );
    }

    #[test]
    fn open_creates_database_at_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let db = Database::open(&path).unwrap();
        db.insert_label("Morning").unwrap();
        assert_eq!(db.count_labels().unwrap(), 1);
    }

    #[test]
    fn inserting_session_with_unknown_label_id_is_rejected() {
        let db = Database::open_in_memory().unwrap();
        let result = db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: Some(999), // does not exist
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        });
        assert!(result.is_err(), "FK constraint should reject unknown label");
    }

    #[test]
    fn data_persists_across_reopens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        {
            let db = Database::open(&path).unwrap();
            db.insert_label("Morning").unwrap();
            db.insert_session(&session_on("2026-04-27")).unwrap();
        }
        let db = Database::open(&path).unwrap();
        let names: Vec<String> =
            db.list_labels().unwrap().into_iter().map(|l| l.name).collect();
        assert_eq!(names, vec!["Morning"]);
        assert_eq!(db.count_sessions().unwrap(), 1);
    }

    #[test]
    fn running_average_is_zero_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(
            db.get_running_average_secs(date(2026, 4, 27), 7).unwrap(),
            0.0
        );
    }

    #[test]
    fn running_average_handles_zero_days_without_divide_by_zero() {
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&session_on("2026-04-27")).unwrap();
        assert_eq!(
            db.get_running_average_secs(date(2026, 4, 27), 0).unwrap(),
            0.0
        );
    }

    #[test]
    fn running_average_divides_total_by_window_days() {
        let db = Database::open_in_memory().unwrap();
        // 600s today, window of 1 day → average = 600.
        db.insert_session(&Session {
            duration_secs: 600,
            ..session_on("2026-04-27")
        })
        .unwrap();
        assert_eq!(
            db.get_running_average_secs(date(2026, 4, 27), 1).unwrap(),
            600.0
        );
        // Same data, window of 2 days → average = 300.
        assert_eq!(
            db.get_running_average_secs(date(2026, 4, 27), 2).unwrap(),
            300.0
        );
    }

    #[test]
    fn running_average_excludes_sessions_outside_window() {
        let db = Database::open_in_memory().unwrap();
        // Today: 600s — inside any window.
        db.insert_session(&Session {
            duration_secs: 600,
            ..session_on("2026-04-27")
        })
        .unwrap();
        // 10 days ago: 1200s — outside a 7-day window.
        db.insert_session(&Session {
            duration_secs: 1200,
            ..session_on("2026-04-17")
        })
        .unwrap();
        // Window of 7 days = today and 6 prior days; only today's 600s counts.
        let avg = db.get_running_average_secs(date(2026, 4, 27), 7).unwrap();
        assert!((avg - (600.0 / 7.0)).abs() < 1e-9, "got {avg}");
    }

    #[test]
    fn median_duration_is_none_for_empty_db() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.get_median_duration_secs().unwrap(), None);
    }

    #[test]
    fn median_duration_returns_middle_for_odd_count() {
        let db = Database::open_in_memory().unwrap();
        for d in [300u32, 600, 900, 1200, 1500] {
            db.insert_session(&Session {
                duration_secs: d,
                ..session_on("2026-04-27")
            })
            .unwrap();
        }
        assert_eq!(db.get_median_duration_secs().unwrap(), Some(900));
    }

    #[test]
    fn median_duration_uses_lower_median_for_even_count() {
        let db = Database::open_in_memory().unwrap();
        // Sorted: [300, 600, 900, 1200]. Lower median = 600.
        for d in [600u32, 1200, 300, 900] {
            db.insert_session(&Session {
                duration_secs: d,
                ..session_on("2026-04-27")
            })
            .unwrap();
        }
        assert_eq!(db.get_median_duration_secs().unwrap(), Some(600));
    }

    #[test]
    fn csv_round_trips_sessions_with_labels() {
        let src = Database::open_in_memory().unwrap();
        src.insert_label("Morning").unwrap();
        let morning_id = src.find_label_by_name("Morning").unwrap();
        src.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id: morning_id,
            notes: Some("clear, focused".to_string()), // comma forces CSV quoting
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        })
        .unwrap();
        src.insert_session(&Session {
            start_iso: "2026-04-27T19:00:00Z".to_string(),
            duration_secs: 1200,
            label_id: None,
            notes: None,
            mode: SessionMode::BoxBreath,
            uuid: String::new(),
            guided_file_uuid: None,
        })
        .unwrap();

        let mut buf = Vec::new();
        src.export_sessions_csv(&mut buf).unwrap();

        let dst = Database::open_in_memory().unwrap();
        let imported = dst.import_sessions_csv(&buf[..]).unwrap();
        assert_eq!(imported, 2);

        // Label was created on import.
        let dst_names: Vec<String> =
            dst.list_labels().unwrap().into_iter().map(|l| l.name).collect();
        assert_eq!(dst_names, vec!["Morning"]);
        let dst_morning_id = dst.find_label_by_name("Morning").unwrap();

        // CSV import generates fresh v4 uuids on the destination DB
        // (uuids aren't part of the CSV format). Verify each row carries
        // one, then bind it into the expected struct so the full
        // comparison below also covers the rest of the fields.
        let sessions = dst.list_sessions().unwrap();
        assert_eq!(sessions.len(), 2);
        assert!(looks_like_uuid_v4(&sessions[0].1.uuid));
        assert!(looks_like_uuid_v4(&sessions[1].1.uuid));
        assert_ne!(sessions[0].1.uuid, sessions[1].1.uuid);
        assert_eq!(
            sessions[0].1,
            Session {
                start_iso: "2026-04-27T10:00:00Z".to_string(),
                duration_secs: 600,
                label_id: dst_morning_id,
                notes: Some("clear, focused".to_string()),
                mode: SessionMode::Timer,
                uuid: sessions[0].1.uuid.clone(),
                guided_file_uuid: None,
            }
        );
        assert_eq!(
            sessions[1].1,
            Session {
                start_iso: "2026-04-27T19:00:00Z".to_string(),
                duration_secs: 1200,
                label_id: None,
                notes: None,
                mode: SessionMode::BoxBreath,
                uuid: sessions[1].1.uuid.clone(),
                guided_file_uuid: None,
            }
        );
    }

    #[test]
    fn export_csv_writes_header_and_session_with_label_name() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let label_id = db.find_label_by_name("Morning").unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00Z".to_string(),
            duration_secs: 600,
            label_id,
            notes: Some("clear mind".to_string()),
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        })
        .unwrap();

        let mut buf = Vec::new();
        db.export_sessions_csv(&mut buf).unwrap();
        let csv = String::from_utf8(buf).unwrap();

        assert!(
            csv.contains("start_iso,duration_secs,label,notes,mode"),
            "missing header in:\n{csv}"
        );
        assert!(csv.contains("2026-04-27T10:00:00Z"));
        assert!(csv.contains("Morning"));
        assert!(csv.contains("clear mind"));
        assert!(csv.contains("timer"));
    }

    // ── UUIDs on sessions and labels (Nextcloud-Sync phase A1) ───────────────
    //
    // Every session and label row must carry a stable cross-device UUID.
    // The DB generates it at insert time — the value the caller puts in
    // the struct's `uuid` field is ignored. Reads round-trip the stored
    // UUID into the returned struct so the rest of the app (including
    // the future event log) can address rows by it.

    fn looks_like_uuid_v4(s: &str) -> bool {
        // 8-4-4-4-12 hex with v4 marker and RFC 4122 variant. Cheap shape
        // check — we don't need a full parser, just confidence that
        // generation actually used `Uuid::new_v4()` rather than (say) a
        // timestamp string or a counter.
        if s.len() != 36 { return false; }
        let bytes = s.as_bytes();
        if bytes[8] != b'-' || bytes[13] != b'-' || bytes[18] != b'-' || bytes[23] != b'-' {
            return false;
        }
        if bytes[14] != b'4' { return false; }                 // version
        if !matches!(bytes[19], b'8' | b'9' | b'a' | b'b') {  // variant
            return false;
        }
        bytes.iter().enumerate().all(|(i, c)| {
            matches!(i, 8 | 13 | 18 | 23) || c.is_ascii_hexdigit()
        })
    }

    #[test]
    fn inserted_session_has_a_uuid_in_query_results() {
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
                        uuid: String::new(),  // ignored — DB assigns
                        guided_file_uuid: None,
        })
        .unwrap();
        let rows = db.list_sessions().unwrap();
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].1.uuid.is_empty(), "uuid must be populated on read");
    }

    #[test]
    fn two_inserted_sessions_get_distinct_uuids() {
        let db = Database::open_in_memory().unwrap();
        for i in 0..2 {
            db.insert_session(&Session {
                start_iso: format!("2026-04-2{}T10:00:00", 7 + i),
                duration_secs: 600,
                label_id: None,
                notes: None,
                mode: SessionMode::Timer,
                                uuid: String::new(),
                                guided_file_uuid: None,
            })
            .unwrap();
        }
        let rows = db.list_sessions().unwrap();
        assert_eq!(rows.len(), 2);
        assert_ne!(rows[0].1.uuid, rows[1].1.uuid,
            "two inserts must produce distinct uuids");
    }

    #[test]
    fn inserted_session_uuid_is_v4_shaped() {
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-27T10:00:00".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
                        uuid: String::new(),
                        guided_file_uuid: None,
        })
        .unwrap();
        let uuid = &db.list_sessions().unwrap()[0].1.uuid;
        assert!(looks_like_uuid_v4(uuid),
            "session uuid `{uuid}` doesn't match v4 shape");
    }

    #[test]
    fn caller_supplied_session_uuid_is_ignored_in_favour_of_a_fresh_one() {
        // Documents that uuid is DB-assigned, not caller-controlled.
        // Belt-and-braces: if a caller accidentally reuses a uuid string
        // the DB still produces fresh, unique values — no collision risk.
        let db = Database::open_in_memory().unwrap();
        let bogus = "00000000-0000-4000-8000-000000000000".to_string();
        for i in 0..2 {
            db.insert_session(&Session {
                start_iso: format!("2026-04-2{}T10:00:00", 7 + i),
                duration_secs: 600,
                label_id: None,
                notes: None,
                mode: SessionMode::Timer,
                                uuid: bogus.clone(),
                                guided_file_uuid: None,
            })
            .unwrap();
        }
        let rows = db.list_sessions().unwrap();
        assert_ne!(rows[0].1.uuid, bogus, "DB must override caller's uuid");
        assert_ne!(rows[1].1.uuid, bogus, "DB must override caller's uuid");
        assert_ne!(rows[0].1.uuid, rows[1].1.uuid);
    }

    #[test]
    fn inserted_label_has_a_uuid() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let labels = db.list_labels().unwrap();
        assert_eq!(labels.len(), 1);
        assert!(!labels[0].uuid.is_empty(), "uuid must be populated on read");
    }

    #[test]
    fn two_inserted_labels_get_distinct_uuids() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        db.insert_label("Evening").unwrap();
        let labels = db.list_labels().unwrap();
        assert_eq!(labels.len(), 2);
        assert_ne!(labels[0].uuid, labels[1].uuid);
    }

    #[test]
    fn inserted_label_uuid_is_v4_shaped() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let uuid = &db.list_labels().unwrap()[0].uuid;
        assert!(looks_like_uuid_v4(uuid),
            "label uuid `{uuid}` doesn't match v4 shape");
    }

    // ── Device identity (Nextcloud-Sync phase A2.1) ──────────────────────────
    //
    // Each device gets a stable UUID that survives across app restarts and
    // tags every locally-authored event. Generated lazily on first call to
    // `device_id()` so a fresh in-memory test DB doesn't pay the cost
    // unless something asks for it.

    #[test]
    fn device_id_is_a_v4_uuid() {
        let db = Database::open_in_memory().unwrap();
        let id = db.device_id().unwrap();
        assert!(looks_like_uuid_v4(&id),
            "device_id `{id}` doesn't match v4 shape");
    }

    #[test]
    fn device_id_is_stable_across_calls_within_one_process() {
        // Two calls in succession must agree — the second call must not
        // re-generate. Otherwise every event we author would be tagged
        // with a different device, defeating the conflict-resolution
        // rule that ties-break by device_id.
        let db = Database::open_in_memory().unwrap();
        let a = db.device_id().unwrap();
        let b = db.device_id().unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn device_id_is_stable_across_database_reopens() {
        // Persistence: closing the DB and reopening the same file must
        // yield the same device_id. This is the actual cross-restart
        // contract; the in-memory variant above only proves "same call,
        // same answer".
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("device_id.db");

        let id_first = {
            let db = Database::open(&path).unwrap();
            db.device_id().unwrap()
        };
        let id_second = {
            let db = Database::open(&path).unwrap();
            db.device_id().unwrap()
        };
        assert_eq!(id_first, id_second,
            "device_id must persist across DB reopens");
    }

    #[test]
    fn two_separate_databases_get_different_device_ids() {
        // Two fresh DBs simulate two devices on the same network. Their
        // device_ids must differ so events authored on each can be
        // distinguished by `device_id` in the conflict-resolution rules.
        let db_a = Database::open_in_memory().unwrap();
        let db_b = Database::open_in_memory().unwrap();
        assert_ne!(db_a.device_id().unwrap(), db_b.device_id().unwrap());
    }

    // ── Lamport clock (Nextcloud-Sync phase A2.2) ────────────────────────────
    //
    // Logical clock for event ordering: bumped on local writes, max-merged
    // with observed remote timestamps. Persisted in the single `device`
    // row so it survives restarts. Conflict resolution depends on a
    // monotonic counter — subtle bugs here cause silent data divergence.

    #[test]
    fn lamport_clock_is_zero_on_a_fresh_database() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.lamport_clock().unwrap(), 0);
    }

    #[test]
    fn lamport_clock_starts_at_zero_even_before_device_id_is_minted() {
        // Reading the clock must not implicitly require device_id() to
        // have been called. The single-row `device` table is shared
        // state — a query on an empty table returns the column default
        // (0), not an error.
        let db = Database::open_in_memory().unwrap();
        let _ = db.lamport_clock().unwrap();          // no panic
        assert_eq!(db.lamport_clock().unwrap(), 0);   // and idempotent
    }

    #[test]
    fn bump_lamport_clock_returns_post_increment_value() {
        // Caller-friendly contract: the returned value is the timestamp
        // to attach to the event we're about to author. So bump() yields
        // 1 on a fresh DB, then 2, then 3 — never 0.
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.bump_lamport_clock().unwrap(), 1);
        assert_eq!(db.bump_lamport_clock().unwrap(), 2);
        assert_eq!(db.bump_lamport_clock().unwrap(), 3);
    }

    #[test]
    fn bump_lamport_clock_persists_the_increment() {
        // After a bump, the *plain* read must reflect it — otherwise
        // `lamport_clock()` and `bump_lamport_clock()` disagree, and
        // observe_remote_lamport's max-merge logic breaks.
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.bump_lamport_clock().unwrap(), 1);
        assert_eq!(db.lamport_clock().unwrap(), 1);
    }

    #[test]
    fn bump_lamport_clock_persists_across_database_reopens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lamport.db");

        let mid = {
            let db = Database::open(&path).unwrap();
            db.bump_lamport_clock().unwrap();
            db.bump_lamport_clock().unwrap()
        };
        assert_eq!(mid, 2);
        let after_reopen = {
            let db = Database::open(&path).unwrap();
            db.lamport_clock().unwrap()
        };
        assert_eq!(after_reopen, 2,
            "lamport_clock must survive process restart unchanged");
        let bumped = {
            let db = Database::open(&path).unwrap();
            db.bump_lamport_clock().unwrap()
        };
        assert_eq!(bumped, 3,
            "the next bump after restart continues from the persisted value");
    }

    #[test]
    fn observe_remote_lamport_advances_when_remote_is_ahead() {
        // The Lamport rule: on observing a remote ts, set local =
        // max(local, remote) + 1. When remote > local, this jumps the
        // clock forward — necessary so any event we author after
        // observing the remote will sort *after* it.
        let db = Database::open_in_memory().unwrap();
        let new_local = db.observe_remote_lamport(42).unwrap();
        assert_eq!(new_local, 43);
        assert_eq!(db.lamport_clock().unwrap(), 43);
    }

    #[test]
    fn observe_remote_lamport_keeps_advancing_when_local_is_already_ahead() {
        // Conversely, when local > remote, we use local+1 — local was
        // already ahead, so we must still produce a strictly larger
        // value to satisfy "every observation advances the clock".
        let db = Database::open_in_memory().unwrap();
        // Get local to 100.
        for _ in 0..100 { db.bump_lamport_clock().unwrap(); }
        let new_local = db.observe_remote_lamport(7).unwrap();
        assert_eq!(new_local, 101);
        assert_eq!(db.lamport_clock().unwrap(), 101);
    }

    #[test]
    fn observe_remote_lamport_treats_equal_as_max_plus_one() {
        // Tie case: max(5, 5) + 1 = 6. Documents that "max" really is
        // max, not "strictly greater" — this is what guarantees a total
        // order even when two devices independently land on the same ts.
        let db = Database::open_in_memory().unwrap();
        for _ in 0..5 { db.bump_lamport_clock().unwrap(); }
        let new_local = db.observe_remote_lamport(5).unwrap();
        assert_eq!(new_local, 6);
    }

    #[test]
    fn observe_remote_lamport_handles_zero() {
        // The very first observation of a fresh remote (ts=0) must still
        // bump the local clock past it — otherwise an event tagged 0
        // would be indistinguishable from never-set state.
        let db = Database::open_in_memory().unwrap();
        let new_local = db.observe_remote_lamport(0).unwrap();
        assert_eq!(new_local, 1);
    }

    // ── Event log: append + pending + mark_synced (A2.3) ─────────────────────
    //
    // The append-only event log is the single source of truth for all
    // mutations. `append_event` is idempotent on `event_uuid`: receiving
    // the same event twice (e.g. on retry, or from a peer that already
    // forwarded it) is a no-op rather than a constraint error escalated
    // to the caller. `pending_events` is the push-queue contract — sorted
    // by `lamport_ts` so peers see events in causal order.

    fn sample_event(seed: i64) -> Event {
        let session_uuid = format!("00000000-0000-4000-9000-{:012x}", seed);
        Event {
            event_uuid: format!("00000000-0000-4000-8000-{:012x}", seed),
            lamport_ts: seed,
            device_id: "00000000-0000-4000-8000-aaaaaaaaaaaa".to_string(),
            kind: "session_insert".to_string(),
            target_id: session_uuid.clone(),
            payload: format!("{{\"uuid\":\"{session_uuid}\",\"seed\":{seed}}}"),
        }
    }

    #[test]
    fn pending_events_is_empty_on_a_fresh_database() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.pending_events().unwrap().is_empty());
    }

    #[test]
    fn append_event_then_read_back_via_pending_events() {
        let db = Database::open_in_memory().unwrap();
        let event = sample_event(7);
        db.append_event(&event).unwrap();
        let rows = db.pending_events().unwrap();
        assert_eq!(rows.len(), 1);
        let (_, got) = &rows[0];
        assert_eq!(got, &event,
            "appended event must round-trip every field unchanged");
    }

    #[test]
    fn append_event_returns_a_distinct_local_rowid_per_call() {
        // The local rowid is the cache key inside this device — distinct
        // from `event_uuid` (the cross-device identity). Two appends must
        // get two different rowids so callers can address them locally.
        let db = Database::open_in_memory().unwrap();
        let id_a = db.append_event(&sample_event(1)).unwrap();
        let id_b = db.append_event(&sample_event(2)).unwrap();
        assert_ne!(id_a, id_b);
    }

    #[test]
    fn append_event_with_duplicate_uuid_is_idempotent_no_op() {
        // `event_uuid` is UNIQUE — a second insert of the same uuid must
        // succeed silently and NOT create a second row. This makes
        // event delivery at-most-once on the local cache regardless of
        // how often the caller (or a sync retry) submits it.
        let db = Database::open_in_memory().unwrap();
        let event = sample_event(1);
        db.append_event(&event).unwrap();
        let res = db.append_event(&event);
        assert!(res.is_ok(),
            "duplicate-event_uuid append must be a silent no-op, got: {res:?}");
        assert_eq!(db.pending_events().unwrap().len(), 1,
            "duplicate append must not create a second row");
    }

    #[test]
    fn pending_events_orders_by_lamport_ts_ascending() {
        // Peers replay in lamport order to converge on a consistent
        // state. The push queue must hand events out in that same order
        // so a peer with a slow-then-fast connection still gets them
        // monotonically.
        let db = Database::open_in_memory().unwrap();
        // Insert out of order — ts 5, then 1, then 3.
        db.append_event(&sample_event(5)).unwrap();
        db.append_event(&sample_event(1)).unwrap();
        db.append_event(&sample_event(3)).unwrap();
        let timestamps: Vec<i64> = db.pending_events().unwrap()
            .iter().map(|(_, e)| e.lamport_ts).collect();
        assert_eq!(timestamps, vec![1, 3, 5]);
    }

    #[test]
    fn mark_event_synced_removes_it_from_pending_events() {
        let db = Database::open_in_memory().unwrap();
        let id_a = db.append_event(&sample_event(1)).unwrap();
        let _id_b = db.append_event(&sample_event(2)).unwrap();
        db.mark_event_synced(id_a).unwrap();
        let pending: Vec<i64> = db.pending_events().unwrap()
            .iter().map(|(_, e)| e.lamport_ts).collect();
        assert_eq!(pending, vec![2],
            "synced event must drop out of the pending list");
    }

    #[test]
    fn mark_event_synced_unknown_id_is_a_silent_no_op() {
        // Defensive: a stale id from a partial sync attempt must not
        // panic or surface an error. SQLite UPDATE on no-match is
        // already a no-op; the wrapper preserves that.
        let db = Database::open_in_memory().unwrap();
        db.append_event(&sample_event(1)).unwrap();
        let res = db.mark_event_synced(999);
        assert!(res.is_ok());
        assert_eq!(db.pending_events().unwrap().len(), 1,
            "the existing event must still be pending — nothing was marked");
    }

    #[test]
    fn mark_events_synced_batch_marks_every_provided_id() {
        // The batch variant must produce the same end state as N calls
        // to `mark_event_synced`. Used by the bulk-push path to flip
        // every event in a successful batch in a single transaction.
        let db = Database::open_in_memory().unwrap();
        let id_a = db.append_event(&sample_event(1)).unwrap();
        let id_b = db.append_event(&sample_event(2)).unwrap();
        let id_c = db.append_event(&sample_event(3)).unwrap();
        db.mark_events_synced(&[id_a, id_c]).unwrap();
        let pending = db.pending_events().unwrap();
        assert_eq!(pending.len(), 1, "only the un-marked event remains pending");
        assert_eq!(pending[0].0, id_b,
            "the un-marked event is the one whose id wasn't in the batch");
    }

    #[test]
    fn mark_events_synced_empty_slice_is_a_silent_no_op() {
        // Don't crash on the no-work path. The bulk push only calls
        // this when at least one event was pushed, but defending
        // against the empty input is cheap and removes a footgun.
        let db = Database::open_in_memory().unwrap();
        db.append_event(&sample_event(1)).unwrap();
        db.mark_events_synced(&[]).unwrap();
        assert_eq!(db.pending_events().unwrap().len(), 1,
            "the existing event must remain pending — nothing was asked of us");
    }

    #[test]
    fn mark_events_synced_is_atomic_across_the_batch() {
        // The batch runs inside one transaction. Verifies that the
        // mid-batch state isn't visible to a concurrent reader: either
        // all rows are marked or none. Hard to test fully without a
        // second connection — we check the post-condition.
        let db = Database::open_in_memory().unwrap();
        let ids: Vec<i64> = (1..=10)
            .map(|i| db.append_event(&sample_event(i)).unwrap())
            .collect();
        db.mark_events_synced(&ids).unwrap();
        assert!(db.pending_events().unwrap().is_empty(),
            "every event in the batch must be marked synced");
    }

    #[test]
    fn mark_events_synced_ignores_unknown_ids_among_known_ones() {
        // Same defensive shape as the single-id variant: a stale id
        // mixed in with valid ones doesn't poison the batch.
        let db = Database::open_in_memory().unwrap();
        let id_real = db.append_event(&sample_event(1)).unwrap();
        let result = db.mark_events_synced(&[id_real, 99_999]);
        assert!(result.is_ok());
        assert!(db.pending_events().unwrap().is_empty(),
            "the real event must still be marked synced");
    }

    // ── flag_all_events_unsynced — "push local" recovery primitive ─────

    #[test]
    fn flag_all_events_unsynced_marks_every_synced_event_pending() {
        // The "push local up" recovery path needs every authored
        // event to be re-pushed as a single fresh batch. Flipping
        // synced=0 across the table puts them all back into
        // pending_events.
        let db = Database::open_in_memory().unwrap();
        let id_a = db.append_event(&sample_event(1)).unwrap();
        let id_b = db.append_event(&sample_event(2)).unwrap();
        db.mark_events_synced(&[id_a, id_b]).unwrap();
        assert!(db.pending_events().unwrap().is_empty());

        db.flag_all_events_unsynced().unwrap();
        let pending = db.pending_events().unwrap();
        assert_eq!(pending.len(), 2,
            "every authored event must be back in pending");
    }

    #[test]
    fn flag_all_events_unsynced_is_a_no_op_on_already_pending_events() {
        // Already-pending rows must stay pending — the operation is
        // idempotent. (SQLite UPDATE WHERE matches no rows is fine,
        // but we shouldn't accidentally clobber other state.)
        let db = Database::open_in_memory().unwrap();
        let _ = db.append_event(&sample_event(1)).unwrap();
        let _ = db.append_event(&sample_event(2)).unwrap();
        let count_before = db.pending_events().unwrap().len();
        db.flag_all_events_unsynced().unwrap();
        assert_eq!(db.pending_events().unwrap().len(), count_before);
    }

    #[test]
    fn flag_all_events_unsynced_on_an_empty_log_is_a_silent_no_op() {
        // Defensive: never-synced device, empty events table. Don't
        // crash; subsequent assertions about pending_events stay valid.
        let db = Database::open_in_memory().unwrap();
        db.flag_all_events_unsynced().unwrap();
        assert!(db.pending_events().unwrap().is_empty());
    }

    #[test]
    fn flag_all_events_unsynced_does_not_touch_other_tables() {
        // Defensive: the operation is scoped to the synced flag.
        // Sessions, labels, settings, and known_remote_files must
        // survive untouched — only the events table changes.
        let db = Database::open_in_memory().unwrap();
        db.append_event(&sample_event(1)).unwrap();
        let label_id = db.insert_label("focus").unwrap();
        db.set_setting("k", "v").unwrap();
        db.record_known_remote_file("a").unwrap();
        let labels_before = db.list_labels().unwrap().len();

        db.flag_all_events_unsynced().unwrap();

        assert_eq!(db.list_labels().unwrap().len(), labels_before);
        assert!(db.list_labels().unwrap().iter().any(|l| l.id == label_id));
        assert_eq!(db.get_setting("k", "default").unwrap(), "v");
        assert!(db.known_remote_file_uuids().unwrap().contains("a"),
            "known_remote_files must be left alone — the caller wipes it \
             explicitly when needed");
    }

    // ── wipe_local_event_log — "wipe local" recovery primitive ─────────

    #[test]
    fn wipe_local_event_log_clears_every_event_sourced_table() {
        // The "wipe local to match remote" recovery deletes every
        // user-content table whose source-of-truth is the event log,
        // plus both dedup trackers. After the wipe, the local DB
        // looks like a freshly-initialised one minus settings/device.
        let db = Database::open_in_memory().unwrap();
        db.append_event(&sample_event(1)).unwrap();
        db.insert_label("focus").unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-30T10:00:00".into(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        db.insert_interval_bell(IntervalBellKind::Interval, 5, 0, "bowl", BUNDLED_PATTERN_PULSE_UUID, SignalMode::Sound).unwrap();
        db.insert_bell_sound("Custom", "/p/c.wav", false, "audio/wav", BellSoundCategory::General).unwrap();
        db.insert_preset("Sitting", SessionMode::Timer, true, r#"{}"#).unwrap();
        db.insert_guided_file_with_uuid("gf-1", "Track", "/p/t.ogg", 300, false).unwrap();
        db.insert_vibration_pattern("Custom Pulse", 200, &[1.0, 0.0], ChartKind::Bar, false).unwrap();
        db.set_box_breath_phase(BoxBreathPhaseId::In, false, SignalMode::Sound, "x", "y").unwrap();
        db.record_known_remote_file("a").unwrap();
        db.record_known_remote_sound("bs-1").unwrap();
        // Sanity: rows present before wipe.
        assert!(!db.pending_events().unwrap().is_empty());
        assert!(!db.list_labels().unwrap().is_empty());
        assert!(!db.list_sessions().unwrap().is_empty());
        assert!(!db.list_interval_bells().unwrap().is_empty());
        assert!(!db.list_bell_sounds().unwrap().is_empty());
        assert!(!db.list_presets().unwrap().is_empty());
        assert!(!db.list_guided_files().unwrap().is_empty());
        assert!(!db.list_vibration_patterns().unwrap().is_empty());
        assert!(!db.known_remote_file_uuids().unwrap().is_empty());
        assert!(!db.known_remote_sound_uuids().unwrap().is_empty());

        db.wipe_local_event_log().unwrap();

        assert!(db.pending_events().unwrap().is_empty(),
            "events table must be empty");
        assert!(db.list_labels().unwrap().is_empty(),
            "labels table must be empty");
        assert!(db.list_sessions().unwrap().is_empty(),
            "sessions table must be empty");
        assert!(db.list_interval_bells().unwrap().is_empty(),
            "interval_bells table must be empty");
        assert!(db.list_bell_sounds().unwrap().is_empty(),
            "bell_sounds table must be empty");
        assert!(db.list_presets().unwrap().is_empty(),
            "presets table must be empty");
        assert!(db.list_guided_files().unwrap().is_empty(),
            "guided_files table must be empty");
        assert!(db.list_vibration_patterns().unwrap().is_empty(),
            "vibration_patterns table must be empty");
        assert!(db.known_remote_file_uuids().unwrap().is_empty(),
            "file dedup tracker must be empty");
        assert!(db.known_remote_sound_uuids().unwrap().is_empty(),
            "sound dedup tracker must be empty");
    }

    #[test]
    fn wipe_local_event_log_keeps_box_breath_phases_seeded_at_defaults() {
        // Box-Breath mode requires the 4 phase rows to render. Wipe
        // clears any user-customised rows but the seed re-runs inline
        // so the mode stays usable post-wipe even before sync replay.
        let db = Database::open_in_memory().unwrap();
        db.seed_box_breath_phases().unwrap();
        db.set_box_breath_phase(
            BoxBreathPhaseId::In, true, SignalMode::Vibration, "u-x", "u-y",
        ).unwrap();

        db.wipe_local_event_log().unwrap();

        let phases = db.list_box_breath_phases().unwrap();
        assert_eq!(phases.len(), 4,
            "all four phases re-seeded with defaults after wipe");
        let in_phase = phases.iter()
            .find(|p| p.phase == BoxBreathPhaseId::In).unwrap();
        assert!(!in_phase.enabled,
            "default enabled=false overwrote user's customised enabled=true");
        assert_eq!(in_phase.signal_mode, SignalMode::Sound,
            "default signal_mode overwrote user's customisation");
    }

    #[test]
    fn wipe_local_event_log_preserves_settings() {
        // User preferences (end_sound, weekly_goal, vibrate, etc.) are
        // independent of the event log we're discarding. The user
        // explicitly chose "wipe content"; their UI prefs should not
        // surprise-reset.
        let db = Database::open_in_memory().unwrap();
        db.set_setting("end_sound", "bowl").unwrap();
        db.set_setting("weekly_goal_mins", "150").unwrap();

        db.wipe_local_event_log().unwrap();

        assert_eq!(db.get_setting("end_sound", "fallback").unwrap(), "bowl");
        assert_eq!(db.get_setting("weekly_goal_mins", "0").unwrap(), "150");
    }

    #[test]
    fn wipe_local_event_log_preserves_sync_state() {
        // The configured Nextcloud account (URL, username) must
        // survive — the user is wiping local state to converge with
        // the same remote. Re-entering the URL would be a friction
        // surprise.
        let db = Database::open_in_memory().unwrap();
        db.set_sync_state("nextcloud_url", "https://nc.example/").unwrap();
        db.set_sync_state("nextcloud_username", "alice").unwrap();

        db.wipe_local_event_log().unwrap();

        assert_eq!(
            db.get_sync_state("nextcloud_url", "").unwrap(),
            "https://nc.example/");
        assert_eq!(
            db.get_sync_state("nextcloud_username", "").unwrap(),
            "alice");
    }

    #[test]
    fn wipe_local_event_log_preserves_device_id_and_lamport() {
        // Device identity persists across wipes. Resetting device_id
        // would create a new identity for the same physical device,
        // confusing peers' replay; resetting lamport could in theory
        // produce duplicate (lamport, device_id) tuples, though
        // monotonicity of the next emit_event would still prevent
        // collisions. Conservative: leave the device row alone.
        let db = Database::open_in_memory().unwrap();
        let device_before = db.device_id().unwrap();
        for _ in 0..5 { db.bump_lamport_clock().unwrap(); }
        let lamport_before = db.lamport_clock().unwrap();

        db.wipe_local_event_log().unwrap();

        assert_eq!(db.device_id().unwrap(), device_before,
            "device_id must survive wipe — it's this device's identity");
        assert_eq!(db.lamport_clock().unwrap(), lamport_before,
            "lamport_clock must survive wipe — keeps causal correctness");
    }

    #[test]
    fn wipe_local_event_log_is_idempotent_on_an_empty_database() {
        // Defensive: never-authored device, fresh DB. Don't crash.
        let db = Database::open_in_memory().unwrap();
        db.wipe_local_event_log().unwrap();
        db.wipe_local_event_log().unwrap();
        assert!(db.pending_events().unwrap().is_empty());
    }

    #[test]
    fn wipe_local_event_log_followed_by_authoring_creates_a_fresh_event() {
        // After wipe, normal authoring must work. The empty events
        // table accepts new inserts; pending_events sees the new row.
        let db = Database::open_in_memory().unwrap();
        db.append_event(&sample_event(1)).unwrap();
        db.wipe_local_event_log().unwrap();

        db.insert_session(&Session {
            start_iso: "2026-04-30T11:00:00".into(),
            duration_secs: 300,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        assert_eq!(db.list_sessions().unwrap().len(), 1);
        assert!(!db.pending_events().unwrap().is_empty(),
            "the new authoring must produce a pending event");
    }

    #[test]
    fn pending_events_excludes_synced_rows() {
        // After every event has been synced, pending_events is empty
        // again. Documents the boundary case of "fully caught up".
        let db = Database::open_in_memory().unwrap();
        let id_a = db.append_event(&sample_event(1)).unwrap();
        let id_b = db.append_event(&sample_event(2)).unwrap();
        db.mark_event_synced(id_a).unwrap();
        db.mark_event_synced(id_b).unwrap();
        assert!(db.pending_events().unwrap().is_empty());
    }

    // ── known_remote_files: bulk-file dedup tracker ─────────────────────

    #[test]
    fn known_remote_file_uuids_starts_empty() {
        // Fresh database: no batch_uuids ingested. The puller takes
        // this empty set and GETs every file it sees.
        let db = Database::open_in_memory().unwrap();
        assert!(db.known_remote_file_uuids().unwrap().is_empty());
    }

    #[test]
    fn record_known_remote_file_then_known_remote_file_uuids_returns_it() {
        // Round-trip: a recorded uuid shows up in the next read.
        let db = Database::open_in_memory().unwrap();
        db.record_known_remote_file("aaa-batch-uuid").unwrap();
        let known = db.known_remote_file_uuids().unwrap();
        assert!(known.contains("aaa-batch-uuid"),
            "recorded uuid must appear in the known set");
    }

    #[test]
    fn record_known_remote_file_is_idempotent() {
        // Inserting the same uuid twice must not error — INSERT OR
        // IGNORE protects against the "we re-pulled the same file"
        // case where the puller calls record() unconditionally.
        let db = Database::open_in_memory().unwrap();
        db.record_known_remote_file("xyz").unwrap();
        db.record_known_remote_file("xyz").unwrap();
        assert_eq!(db.known_remote_file_uuids().unwrap().len(), 1);
    }

    #[test]
    fn known_remote_files_persist_across_database_reopens() {
        // The dedup tracker MUST survive process restart — otherwise a
        // user who closes the app between sync attempts re-GETs every
        // remote file on the next pull, defeating the optimisation.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        {
            let db = Database::open(&path).unwrap();
            db.record_known_remote_file("persistent-batch").unwrap();
        }
        let db2 = Database::open(&path).unwrap();
        assert!(db2.known_remote_file_uuids().unwrap().contains("persistent-batch"));
    }

    #[test]
    fn wipe_known_remote_files_clears_every_recorded_uuid() {
        // The "remote data lost" fail-safe re-anchors the dedup tracker
        // when (a) the account changes, or (b) the user decides to push
        // their local state back up after a wipe. Both flows call
        // `wipe_known_remote_files` to flush the table cleanly.
        let db = Database::open_in_memory().unwrap();
        db.record_known_remote_file("a").unwrap();
        db.record_known_remote_file("b").unwrap();
        db.record_known_remote_file("c").unwrap();
        assert_eq!(db.known_remote_file_uuids().unwrap().len(), 3);
        db.wipe_known_remote_files().unwrap();
        assert!(db.known_remote_file_uuids().unwrap().is_empty(),
            "wipe must remove every recorded file_uuid");
    }

    #[test]
    fn wipe_known_remote_files_on_an_empty_table_is_a_silent_no_op() {
        // First-time account setup: the table is already empty, but the
        // wipe path runs unconditionally on account change. Don't crash.
        let db = Database::open_in_memory().unwrap();
        db.wipe_known_remote_files().unwrap();
        assert!(db.known_remote_file_uuids().unwrap().is_empty());
    }

    #[test]
    fn wipe_known_remote_files_does_not_touch_other_tables() {
        // Defensive: the wipe is scoped to the dedup tracker. Sessions,
        // labels, events, and settings must all survive untouched —
        // otherwise an account swap would silently destroy local state.
        let db = Database::open_in_memory().unwrap();
        let _ = db.append_event(&sample_event(1)).unwrap();
        let label_id = db.insert_label("focus").unwrap();
        db.record_known_remote_file("a").unwrap();
        db.set_setting("k", "v").unwrap();
        let labels_before = db.list_labels().unwrap().len();
        let events_before = db.pending_events().unwrap().len();

        db.wipe_known_remote_files().unwrap();

        assert_eq!(db.list_labels().unwrap().len(), labels_before,
            "labels must not be wiped");
        assert!(db.list_labels().unwrap().iter().any(|l| l.id == label_id));
        assert_eq!(db.pending_events().unwrap().len(), events_before,
            "events must not be wiped");
        assert_eq!(db.get_setting("k", "default").unwrap(), "v",
            "settings must not be wiped");
    }

    #[test]
    fn append_event_persists_across_database_reopens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.db");
        let event = sample_event(42);
        {
            let db = Database::open(&path).unwrap();
            db.append_event(&event).unwrap();
        }
        let db = Database::open(&path).unwrap();
        let rows = db.pending_events().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(&rows[0].1, &event);
    }

    // ── sync_state KV (A2.4) ─────────────────────────────────────────────────
    //
    // Holds sync-loop bookkeeping — server URL, last-pull cursor, last
    // successful sync timestamp, etc. Sensitive values (app password)
    // live in libsecret/Keystore, not here. Mirrors the existing
    // `settings` table in shape but is a separate namespace so a UI
    // export of sync diagnostics doesn't have to filter prefs out, and
    // vice-versa.

    #[test]
    fn get_sync_state_returns_default_on_a_fresh_database() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.get_sync_state("server_url", "fallback").unwrap(),
                   "fallback");
    }

    #[test]
    fn get_sync_state_returns_default_for_unknown_key_after_other_keys_set() {
        // Defensive: setting key A must not affect get on key B.
        let db = Database::open_in_memory().unwrap();
        db.set_sync_state("server_url", "https://nc.example").unwrap();
        assert_eq!(db.get_sync_state("missing", "fallback").unwrap(),
                   "fallback");
    }

    #[test]
    fn set_then_get_sync_state_round_trips_the_value() {
        let db = Database::open_in_memory().unwrap();
        db.set_sync_state("server_url", "https://nc.example").unwrap();
        assert_eq!(db.get_sync_state("server_url", "fallback").unwrap(),
                   "https://nc.example");
    }

    #[test]
    fn set_sync_state_overwrites_an_existing_value() {
        // Upsert semantics — same as `set_setting`. A second `set` must
        // replace, not silently no-op.
        let db = Database::open_in_memory().unwrap();
        db.set_sync_state("interval_seconds", "1800").unwrap();
        db.set_sync_state("interval_seconds", "300").unwrap();
        assert_eq!(db.get_sync_state("interval_seconds", "0").unwrap(),
                   "300");
    }

    #[test]
    fn sync_state_persists_across_database_reopens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sync_state.db");
        {
            let db = Database::open(&path).unwrap();
            db.set_sync_state("server_url", "https://nc.example").unwrap();
        }
        let db = Database::open(&path).unwrap();
        assert_eq!(db.get_sync_state("server_url", "x").unwrap(),
                   "https://nc.example");
    }

    #[test]
    fn sync_state_and_settings_are_separate_namespaces() {
        // Same key in both tables must NOT collide — they're conceptually
        // independent stores. Pinning this makes future "let's just merge
        // them" refactors visible in CI.
        let db = Database::open_in_memory().unwrap();
        db.set_setting("foo", "from-settings").unwrap();
        db.set_sync_state("foo", "from-sync-state").unwrap();
        assert_eq!(db.get_setting("foo", "x").unwrap(), "from-settings");
        assert_eq!(db.get_sync_state("foo", "x").unwrap(), "from-sync-state");
    }

    // ── Event emission on mutations (A3) ─────────────────────────────────────
    //
    // Every state-changing operation appends a self-contained event to
    // `events` so peers can replay it. The local DB (`sessions`,
    // `labels`, `settings`) is the materialized cache derived from
    // those events; if the cache and the log disagree, the log wins on
    // every other device.

    /// Parse the JSON payload of an event into a generic `serde_json::Value`
    /// for assertions. Avoids hardcoding a Rust struct per event kind in
    /// the test surface — the payload contract IS the JSON shape.
    fn event_payload(event: &Event) -> serde_json::Value {
        serde_json::from_str(&event.payload)
            .unwrap_or_else(|e| panic!("payload `{}` is not valid JSON: {e}",
                event.payload))
    }

    // ── A3.1: insert_session emits a session_insert event ────────────────────

    #[test]
    fn insert_session_appends_exactly_one_session_insert_event() {
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-30T10:00:00".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let events = db.pending_events().unwrap();
        assert_eq!(events.len(), 1, "one insert must produce exactly one event");
        assert_eq!(events[0].1.kind, "session_insert");
    }

    #[test]
    fn session_insert_event_payload_contains_the_rows_uuid() {
        // The event's session uuid must match the row's uuid — that's
        // how peers cross-reference events to materialized rows.
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-30T10:00:00".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let row_uuid = db.list_sessions().unwrap()[0].1.uuid.clone();
        let events = db.pending_events().unwrap();
        let payload = event_payload(&events[0].1);
        assert_eq!(payload["uuid"], serde_json::Value::String(row_uuid));
    }

    #[test]
    fn session_insert_event_payload_carries_every_relevant_field() {
        // Every column that a peer needs to reconstruct the row must be
        // present in the payload — start_iso, duration_secs, notes, mode.
        // label_uuid is null here (label_id is None); covered separately
        // when the session does have a label.
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-30T10:00:00".to_string(),
            duration_secs: 1234,
            label_id: None,
            notes: Some("note text".to_string()),
            mode: SessionMode::BoxBreath,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let payload = event_payload(&db.pending_events().unwrap()[0].1);
        assert_eq!(payload["start_iso"], "2026-04-30T10:00:00");
        assert_eq!(payload["duration_secs"], 1234);
        assert_eq!(payload["notes"], "note text");
        assert_eq!(payload["mode"], "box_breath");
        assert_eq!(payload["label_uuid"], serde_json::Value::Null);
    }

    #[test]
    fn session_insert_event_payload_label_uuid_resolves_from_label_id() {
        // sessions reference labels by rowid locally, but the event must
        // carry the label's UUID — the cross-device identity. The
        // resolution `label_id → label_uuid` happens at event-emission
        // time so a peer can apply the event without needing this
        // device's rowid space.
        let db = Database::open_in_memory().unwrap();
        let label_id = db.insert_label("Morning").unwrap();
        let label_uuid = db.list_labels().unwrap()[0].uuid.clone();
        // insert_label also emits an event — drain it before the session
        // insert so we can assert on a single event below.
        for (id, _) in db.pending_events().unwrap() {
            db.mark_event_synced(id).unwrap();
        }
        db.insert_session(&Session {
            start_iso: "2026-04-30T10:00:00".to_string(),
            duration_secs: 600,
            label_id: Some(label_id),
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let payload = event_payload(&db.pending_events().unwrap()[0].1);
        assert_eq!(payload["label_uuid"], serde_json::Value::String(label_uuid));
    }

    #[test]
    fn session_insert_event_payload_serializes_notes_null_when_absent() {
        // `notes: None` round-trips through the payload as JSON null —
        // not an empty string, which would lose the "no notes" vs "empty
        // notes" distinction on a peer that re-applies the event.
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-30T10:00:00".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let payload = event_payload(&db.pending_events().unwrap()[0].1);
        assert_eq!(payload["notes"], serde_json::Value::Null);
    }

    #[test]
    fn session_insert_event_carries_this_devices_id() {
        let db = Database::open_in_memory().unwrap();
        let device_id = db.device_id().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-30T10:00:00".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let events = db.pending_events().unwrap();
        assert_eq!(events[0].1.device_id, device_id,
            "event must be attributed to the authoring device");
    }

    #[test]
    fn session_insert_event_advances_the_lamport_clock() {
        // Bumping the clock on every authored event is what gives the
        // log a total order. After one insert, lamport must be ≥ 1; the
        // event's own ts must equal that bumped value.
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.lamport_clock().unwrap(), 0);
        db.insert_session(&Session {
            start_iso: "2026-04-30T10:00:00".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let lamport = db.lamport_clock().unwrap();
        assert!(lamport >= 1, "lamport must advance past zero");
        let events = db.pending_events().unwrap();
        assert_eq!(events[0].1.lamport_ts, lamport,
            "event ts must equal the post-bump clock value");
    }

    #[test]
    fn two_inserts_produce_two_distinct_events_in_lamport_order() {
        let db = Database::open_in_memory().unwrap();
        for i in 0..2 {
            db.insert_session(&Session {
                start_iso: format!("2026-04-3{}T10:00:00", i),
                duration_secs: 600,
                label_id: None,
                notes: None,
                mode: SessionMode::Timer,
                uuid: String::new(),
                guided_file_uuid: None,
            }).unwrap();
        }
        let events = db.pending_events().unwrap();
        assert_eq!(events.len(), 2);
        assert!(events[0].1.lamport_ts < events[1].1.lamport_ts,
            "events must be sorted ASC by lamport_ts");
        assert_ne!(events[0].1.event_uuid, events[1].1.event_uuid);
    }

    /// Drain every currently-pending event (mark them all synced) so
    /// follow-up assertions can focus on the events produced by a
    /// specific subsequent mutation. Returns nothing — callers don't
    /// care about the drained content, only that what comes next is
    /// observable in isolation.
    fn drain_events(db: &Database) {
        for (id, _) in db.pending_events().unwrap() {
            db.mark_event_synced(id).unwrap();
        }
    }

    // ── A3.2: update_session and delete_session emit events ──────────────────

    #[test]
    fn update_session_appends_a_session_update_event() {
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_session(&Session {
            start_iso: "2026-04-30T10:00:00".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        drain_events(&db);

        db.update_session(id, &Session {
            start_iso: "2026-05-01T11:00:00".to_string(),
            duration_secs: 1800,
            label_id: None,
            notes: Some("revised".to_string()),
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let events = db.pending_events().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1.kind, "session_update");
    }

    #[test]
    fn session_update_event_payload_carries_the_rows_uuid_unchanged() {
        // The session's uuid is stable — update changes every other field
        // but the cross-device identity of the session is fixed at insert
        // time. The event must reference that same uuid so peers can
        // locate the row to update.
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_session(&Session {
            start_iso: "2026-04-30T10:00:00".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let original_uuid = db.list_sessions().unwrap()[0].1.uuid.clone();
        drain_events(&db);

        db.update_session(id, &Session {
            start_iso: "2026-05-01T11:00:00".to_string(),
            duration_secs: 1800,
            label_id: None,
            notes: Some("revised".to_string()),
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let payload = event_payload(&db.pending_events().unwrap()[0].1);
        assert_eq!(payload["uuid"], serde_json::Value::String(original_uuid));
    }

    #[test]
    fn session_update_event_payload_reflects_the_new_field_values() {
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_session(&Session {
            start_iso: "2026-04-30T10:00:00".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        drain_events(&db);

        db.update_session(id, &Session {
            start_iso: "2026-05-01T11:00:00".to_string(),
            duration_secs: 1800,
            label_id: None,
            notes: Some("revised".to_string()),
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let payload = event_payload(&db.pending_events().unwrap()[0].1);
        assert_eq!(payload["start_iso"], "2026-05-01T11:00:00");
        assert_eq!(payload["duration_secs"], 1800);
        assert_eq!(payload["notes"], "revised");
        assert_eq!(payload["mode"], "timer");
    }

    #[test]
    fn session_update_event_payload_label_uuid_resolves_from_new_label() {
        // Updates can change the label — the event payload must reflect
        // the *new* label's uuid, not the old one or the rowid.
        let db = Database::open_in_memory().unwrap();
        let label_id = db.insert_label("Evening").unwrap();
        let label_uuid = db.list_labels().unwrap()[0].uuid.clone();
        let id = db.insert_session(&Session {
            start_iso: "2026-04-30T10:00:00".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        drain_events(&db);

        db.update_session(id, &Session {
            start_iso: "2026-04-30T10:00:00".to_string(),
            duration_secs: 600,
            label_id: Some(label_id),
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let payload = event_payload(&db.pending_events().unwrap()[0].1);
        assert_eq!(payload["label_uuid"], serde_json::Value::String(label_uuid));
    }

    #[test]
    fn update_session_unknown_id_emits_no_event() {
        // Defensive: an UPDATE that affects zero rows must NOT log a
        // ghost event referencing a uuid we don't know. Otherwise peers
        // would receive an update for a session they've never seen.
        let db = Database::open_in_memory().unwrap();
        drain_events(&db);
        db.update_session(9999, &Session {
            start_iso: "2026-05-01T11:00:00".to_string(),
            duration_secs: 1800,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        assert!(db.pending_events().unwrap().is_empty(),
            "no-match update must produce no event");
    }

    #[test]
    fn delete_session_appends_a_session_delete_event() {
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_session(&Session {
            start_iso: "2026-04-30T10:00:00".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let row_uuid = db.list_sessions().unwrap()[0].1.uuid.clone();
        drain_events(&db);

        db.delete_session(id).unwrap();
        let events = db.pending_events().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1.kind, "session_delete");

        // Payload is just the uuid — peers don't need any other field
        // since the tombstone semantics is "drop the row by this id".
        let payload = event_payload(&events[0].1);
        assert_eq!(payload["uuid"], serde_json::Value::String(row_uuid));
    }

    #[test]
    fn delete_session_unknown_id_emits_no_event() {
        let db = Database::open_in_memory().unwrap();
        drain_events(&db);
        db.delete_session(9999).unwrap();
        assert!(db.pending_events().unwrap().is_empty(),
            "no-match delete must produce no event");
    }

    // ── A3.3: bulk operations emit one event per row ─────────────────────────

    #[test]
    fn bulk_insert_sessions_emits_one_event_per_row() {
        // Each row crosses the network as its own SessionInserted event —
        // the cross-device replay model has no concept of "bulk insert",
        // every row is independent. So N inputs must yield N events.
        let db = Database::open_in_memory().unwrap();
        let to_insert: Vec<Session> = (0..3).map(|i| Session {
            start_iso: format!("2026-04-3{i}T10:00:00"),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).collect();
        db.bulk_insert_sessions(&to_insert).unwrap();
        let events = db.pending_events().unwrap();
        assert_eq!(events.len(), 3,
            "three input rows must yield three events");
        for (_, e) in &events {
            assert_eq!(e.kind, "session_insert");
        }
    }

    #[test]
    fn bulk_insert_sessions_event_uuids_match_inserted_rows() {
        // Each event's session uuid must correspond to a stored row's
        // uuid — the set must be equal. Otherwise a peer would receive
        // events for rows we don't have, or skip rows we do.
        let db = Database::open_in_memory().unwrap();
        let to_insert: Vec<Session> = (0..3).map(|i| Session {
            start_iso: format!("2026-04-3{i}T10:00:00"),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).collect();
        db.bulk_insert_sessions(&to_insert).unwrap();
        let row_uuids: std::collections::HashSet<String> = db.list_sessions()
            .unwrap()
            .iter().map(|(_, s)| s.uuid.clone()).collect();
        let event_uuids: std::collections::HashSet<String> = db
            .pending_events()
            .unwrap()
            .iter()
            .map(|(_, e)| event_payload(e)["uuid"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(row_uuids, event_uuids,
            "every stored row must have a matching event, and vice versa");
    }

    #[test]
    fn bulk_insert_sessions_with_empty_slice_emits_no_events() {
        let db = Database::open_in_memory().unwrap();
        drain_events(&db);
        db.bulk_insert_sessions(&[]).unwrap();
        assert!(db.pending_events().unwrap().is_empty());
    }

    #[test]
    fn bulk_insert_session_events_have_strictly_increasing_lamport_ts() {
        // Replay order is determined by lamport_ts. Even within a bulk
        // op, each row gets its own ts so peers can apply them in a
        // consistent order across devices.
        let db = Database::open_in_memory().unwrap();
        let to_insert: Vec<Session> = (0..3).map(|i| Session {
            start_iso: format!("2026-04-3{i}T10:00:00"),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).collect();
        db.bulk_insert_sessions(&to_insert).unwrap();
        let timestamps: Vec<i64> = db.pending_events().unwrap()
            .iter().map(|(_, e)| e.lamport_ts).collect();
        let mut sorted = timestamps.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(timestamps.len(), sorted.len(),
            "every bulk-inserted event must have a unique lamport_ts: {timestamps:?}");
        assert_eq!(timestamps, sorted,
            "events must be returned in ascending lamport_ts order");
    }

    #[test]
    fn delete_all_sessions_emits_one_delete_event_per_existing_row() {
        let db = Database::open_in_memory().unwrap();
        for i in 0..3 {
            db.insert_session(&Session {
                start_iso: format!("2026-04-3{i}T10:00:00"),
                duration_secs: 600,
                label_id: None,
                notes: None,
                mode: SessionMode::Timer,
                uuid: String::new(),
                guided_file_uuid: None,
            }).unwrap();
        }
        let row_uuids: std::collections::HashSet<String> = db.list_sessions()
            .unwrap()
            .iter().map(|(_, s)| s.uuid.clone()).collect();
        drain_events(&db);

        let removed = db.delete_all_sessions().unwrap();
        assert_eq!(removed, 3);

        let events = db.pending_events().unwrap();
        assert_eq!(events.len(), 3,
            "delete_all must emit one delete event per row that was present");
        for (_, e) in &events {
            assert_eq!(e.kind, "session_delete");
        }
        let event_uuids: std::collections::HashSet<String> = events.iter()
            .map(|(_, e)| event_payload(e)["uuid"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(row_uuids, event_uuids,
            "every previously-present row must show up in a tombstone event");
    }

    #[test]
    fn delete_all_sessions_on_empty_database_emits_no_events() {
        let db = Database::open_in_memory().unwrap();
        drain_events(&db);
        let removed = db.delete_all_sessions().unwrap();
        assert_eq!(removed, 0);
        assert!(db.pending_events().unwrap().is_empty());
    }

    // ── A3.4: label mutations emit events ────────────────────────────────────

    #[test]
    fn insert_label_appends_a_label_insert_event() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let events = db.pending_events().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1.kind, "label_insert");
    }

    #[test]
    fn label_insert_event_payload_carries_uuid_and_name() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let row_uuid = db.list_labels().unwrap()[0].uuid.clone();
        let payload = event_payload(&db.pending_events().unwrap()[0].1);
        assert_eq!(payload["uuid"], serde_json::Value::String(row_uuid));
        assert_eq!(payload["name"], "Morning");
    }

    #[test]
    fn duplicate_insert_label_emits_no_event() {
        // The second insert errors with DuplicateLabel — no row was
        // created, so no event must be emitted (would leak a phantom
        // label_insert to peers).
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        drain_events(&db);
        let result = db.insert_label("Morning");
        assert!(result.is_err());
        assert!(db.pending_events().unwrap().is_empty(),
            "rejected duplicate insert must produce no event");
    }

    #[test]
    fn update_label_appends_a_label_rename_event() {
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_label("Morning").unwrap();
        let row_uuid = db.list_labels().unwrap()[0].uuid.clone();
        drain_events(&db);

        db.update_label(id, "Sunrise").unwrap();
        let events = db.pending_events().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1.kind, "label_rename");

        let payload = event_payload(&events[0].1);
        assert_eq!(payload["uuid"], serde_json::Value::String(row_uuid),
            "rename event uuid must match the row's stable uuid");
        assert_eq!(payload["name"], "Sunrise",
            "rename event must carry the NEW name, not the old");
    }

    #[test]
    fn update_label_unknown_id_emits_no_event() {
        let db = Database::open_in_memory().unwrap();
        drain_events(&db);
        db.update_label(9999, "Whatever").unwrap();
        assert!(db.pending_events().unwrap().is_empty(),
            "no-match rename must produce no event");
    }

    #[test]
    fn update_label_to_duplicate_emits_no_event() {
        // The UPDATE fails with DuplicateLabel; the transaction rolls
        // back. No name actually changed, so no rename event must reach
        // peers (otherwise they'd update to a name we never committed).
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let evening_id = db.insert_label("Evening").unwrap();
        drain_events(&db);
        let result = db.update_label(evening_id, "Morning");
        assert!(result.is_err());
        assert!(db.pending_events().unwrap().is_empty(),
            "rejected rename must produce no event");
    }

    #[test]
    fn delete_label_appends_a_label_delete_event() {
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_label("Morning").unwrap();
        let row_uuid = db.list_labels().unwrap()[0].uuid.clone();
        drain_events(&db);

        db.delete_label(id).unwrap();
        let events = db.pending_events().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1.kind, "label_delete");

        let payload = event_payload(&events[0].1);
        assert_eq!(payload["uuid"], serde_json::Value::String(row_uuid));
    }

    #[test]
    fn delete_label_unknown_id_emits_no_event() {
        let db = Database::open_in_memory().unwrap();
        drain_events(&db);
        db.delete_label(9999).unwrap();
        assert!(db.pending_events().unwrap().is_empty(),
            "no-match delete must produce no event");
    }

    // ── A3.5: set_setting emits a setting_changed event ──────────────────────

    #[test]
    fn set_setting_appends_a_setting_changed_event() {
        let db = Database::open_in_memory().unwrap();
        db.set_setting("daily_goal_minutes", "20").unwrap();
        let events = db.pending_events().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1.kind, "setting_changed");

        let payload = event_payload(&events[0].1);
        assert_eq!(payload["key"], "daily_goal_minutes");
        assert_eq!(payload["value"], "20");
    }

    #[test]
    fn set_setting_overwrite_emits_a_second_event_with_the_new_value() {
        // Settings are last-write-wins by lamport_ts on the receiving
        // peer, so every overwrite must produce its own event — silently
        // collapsing to one would lose the intermediate state's lamport
        // ordering and tie-breaks.
        let db = Database::open_in_memory().unwrap();
        db.set_setting("daily_goal_minutes", "20").unwrap();
        db.set_setting("daily_goal_minutes", "30").unwrap();

        let events = db.pending_events().unwrap();
        assert_eq!(events.len(), 2,
            "two `set_setting` calls must emit two events");
        let last_payload = event_payload(&events[1].1);
        assert_eq!(last_payload["value"], "30",
            "the later event must carry the latest value");
    }

    #[test]
    fn set_setting_with_unicode_value_round_trips_through_payload() {
        // Defensive: JSON-encoding emoji / non-ASCII must not corrupt
        // the value. serde_json handles this, but pinning it makes
        // future swaps to other JSON libs visible.
        let db = Database::open_in_memory().unwrap();
        db.set_setting("greeting", "🧘 こんにちは").unwrap();
        let payload = event_payload(&db.pending_events().unwrap()[0].1);
        assert_eq!(payload["value"], "🧘 こんにちは");
    }

    // ── B1.0: events carry target_id for fast lookup ─────────────────────────
    //
    // Replay queries need to find "all events affecting target X" cheaply.
    // Parsing the JSON payload in SQL is awkward, so each event also
    // stores the affected row's identity in a denormalised `target_id`
    // column — for sessions/labels the cross-device uuid, for settings
    // the key.

    #[test]
    fn session_insert_event_target_id_is_the_session_uuid() {
        let db = Database::open_in_memory().unwrap();
        db.insert_session(&Session {
            start_iso: "2026-04-30T10:00:00".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let row_uuid = db.list_sessions().unwrap()[0].1.uuid.clone();
        let events = db.pending_events().unwrap();
        assert_eq!(events[0].1.target_id, row_uuid);
    }

    #[test]
    fn session_delete_event_target_id_is_the_session_uuid() {
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_session(&Session {
            start_iso: "2026-04-30T10:00:00".to_string(),
            duration_secs: 600,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let row_uuid = db.list_sessions().unwrap()[0].1.uuid.clone();
        drain_events(&db);
        db.delete_session(id).unwrap();
        let events = db.pending_events().unwrap();
        assert_eq!(events[0].1.target_id, row_uuid);
    }

    #[test]
    fn label_insert_event_target_id_is_the_label_uuid() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let row_uuid = db.list_labels().unwrap()[0].uuid.clone();
        let events = db.pending_events().unwrap();
        assert_eq!(events[0].1.target_id, row_uuid);
    }

    #[test]
    fn setting_changed_event_target_id_is_the_setting_key() {
        let db = Database::open_in_memory().unwrap();
        db.set_setting("daily_goal_minutes", "20").unwrap();
        let events = db.pending_events().unwrap();
        assert_eq!(events[0].1.target_id, "daily_goal_minutes",
            "settings have no uuid; the key acts as cross-device identity");
    }

    // ── B1.1: apply_event for session events ─────────────────────────────────
    //
    // apply_event consumes a remote-authored event and updates the local
    // materialized cache. The model: record the event in `events`, then
    // recompute the cache row for its target_id from the events table —
    // tombstone wins on tie/precedence, otherwise the highest-lamport
    // mutate event drives the row's values. This makes apply_event
    // idempotent (re-applying same event_uuid is a no-op via INSERT OR
    // IGNORE) and order-independent (out-of-order delivery converges).

    /// Hand-construct an event without going through a Database. Lets
    /// tests pin specific lamport_ts / device_id values for tie-break
    /// and out-of-order scenarios.
    fn synth_event(
        kind: &str,
        target_id: &str,
        lamport_ts: i64,
        device_id: &str,
        payload: serde_json::Value,
    ) -> Event {
        Event {
            event_uuid: uuid::Uuid::new_v4().to_string(),
            lamport_ts,
            device_id: device_id.to_string(),
            kind: kind.to_string(),
            target_id: target_id.to_string(),
            payload: payload.to_string(),
        }
    }

    fn synth_session_insert(
        session_uuid: &str,
        lamport_ts: i64,
        device_id: &str,
        start_iso: &str,
        duration_secs: u32,
        label_uuid: Option<&str>,
        notes: Option<&str>,
        mode: SessionMode,
    ) -> Event {
        synth_event(
            "session_insert",
            session_uuid,
            lamport_ts,
            device_id,
            serde_json::json!({
                "uuid": session_uuid,
                "start_iso": start_iso,
                "duration_secs": duration_secs,
                "label_uuid": label_uuid,
                "notes": notes,
                "mode": mode.as_db_str(),
            }),
        )
    }

    fn synth_session_update(
        session_uuid: &str,
        lamport_ts: i64,
        device_id: &str,
        start_iso: &str,
        duration_secs: u32,
        label_uuid: Option<&str>,
        notes: Option<&str>,
        mode: SessionMode,
    ) -> Event {
        synth_event(
            "session_update",
            session_uuid,
            lamport_ts,
            device_id,
            serde_json::json!({
                "uuid": session_uuid,
                "start_iso": start_iso,
                "duration_secs": duration_secs,
                "label_uuid": label_uuid,
                "notes": notes,
                "mode": mode.as_db_str(),
            }),
        )
    }

    fn synth_session_delete(
        session_uuid: &str,
        lamport_ts: i64,
        device_id: &str,
    ) -> Event {
        synth_event(
            "session_delete",
            session_uuid,
            lamport_ts,
            device_id,
            serde_json::json!({ "uuid": session_uuid }),
        )
    }

    const DEVICE_A: &str = "00000000-0000-4000-8000-aaaaaaaaaaaa";
    const DEVICE_B: &str = "00000000-0000-4000-8000-bbbbbbbbbbbb";
    const SESSION_X: &str = "11111111-1111-4111-8111-111111111111";
    /// Mirror of the shell-side BUNDLED_PATTERN_PULSE_UUID const.
    /// Kept literal here so the core tests don't need to plumb in the
    /// shell module — every interval-bell insert in the test suite
    /// passes this as the default pattern uuid.
    const BUNDLED_PATTERN_PULSE_UUID: &str = "7e9c4d2f-5a8b-4f1d-9e3c-2d6f7a8b0001";

    #[test]
    fn apply_event_session_insert_creates_the_row() {
        // Apply a single insert event from a peer; the cache row appears
        // with all the event's values.
        let db = Database::open_in_memory().unwrap();
        let event = synth_session_insert(
            SESSION_X, 5, DEVICE_A,
            "2026-04-30T10:00:00", 600,
            None, Some("from peer"), SessionMode::BoxBreath,
        );
        db.apply_event(&event).unwrap();
        let rows = db.list_sessions().unwrap();
        assert_eq!(rows.len(), 1);
        let s = &rows[0].1;
        assert_eq!(s.uuid, SESSION_X);
        assert_eq!(s.start_iso, "2026-04-30T10:00:00");
        assert_eq!(s.duration_secs, 600);
        assert_eq!(s.notes.as_deref(), Some("from peer"));
        assert_eq!(s.mode, SessionMode::BoxBreath);
    }

    #[test]
    fn apply_event_session_insert_with_guided_file_uuid_round_trips() {
        // A guided session synced from a peer carries the file's uuid
        // in the event payload so per-file stats stay consistent across
        // devices. recompute_session must lift `guided_file_uuid` out
        // of the JSON payload and write it to the column.
        let db = Database::open_in_memory().unwrap();
        let file_uuid = "fffffff0-0000-4000-8000-cccccccccccc";
        let event = synth_event(
            "session_insert",
            SESSION_X,
            7,
            DEVICE_A,
            serde_json::json!({
                "uuid": SESSION_X,
                "start_iso": "2026-05-05T20:30:00",
                "duration_secs": 1200,
                "label_uuid": serde_json::Value::Null,
                "notes": serde_json::Value::Null,
                "mode": "guided",
                "guided_file_uuid": file_uuid,
            }),
        );
        db.apply_event(&event).unwrap();
        let rows = db.list_sessions().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1.guided_file_uuid.as_deref(), Some(file_uuid));
    }

    #[test]
    fn apply_event_session_insert_without_guided_file_uuid_leaves_column_null() {
        // Old-shape event payloads (no guided_file_uuid key) must
        // continue to work — recompute_session reads the field as
        // optional and writes NULL when missing.
        let db = Database::open_in_memory().unwrap();
        let event = synth_session_insert(
            SESSION_X, 5, DEVICE_A,
            "2026-04-30T10:00:00", 600,
            None, None, SessionMode::Timer,
        );
        db.apply_event(&event).unwrap();
        let rows = db.list_sessions().unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].1.guided_file_uuid.is_none());
    }

    #[test]
    fn apply_event_is_idempotent_on_event_uuid() {
        // Applying the exact same Event twice must not double-insert
        // and must not error. The events table's UNIQUE(event_uuid)
        // is the dedup key.
        let db = Database::open_in_memory().unwrap();
        let event = synth_session_insert(
            SESSION_X, 5, DEVICE_A,
            "2026-04-30T10:00:00", 600,
            None, None, SessionMode::Timer,
        );
        db.apply_event(&event).unwrap();
        db.apply_event(&event).unwrap();
        assert_eq!(db.list_sessions().unwrap().len(), 1,
            "duplicate event_uuid must not create a second row");
    }

    #[test]
    fn apply_event_session_update_after_insert_updates_the_row() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_session_insert(
            SESSION_X, 5, DEVICE_A,
            "2026-04-30T10:00:00", 600,
            None, None, SessionMode::Timer,
        )).unwrap();
        db.apply_event(&synth_session_update(
            SESSION_X, 10, DEVICE_A,
            "2026-05-01T11:00:00", 1200,
            None, Some("revised"), SessionMode::Timer,
        )).unwrap();
        let s = &db.list_sessions().unwrap()[0].1;
        assert_eq!(s.start_iso, "2026-05-01T11:00:00");
        assert_eq!(s.duration_secs, 1200);
        assert_eq!(s.notes.as_deref(), Some("revised"));
        assert_eq!(s.mode, SessionMode::Timer);
    }

    #[test]
    fn apply_event_session_delete_removes_the_row() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_session_insert(
            SESSION_X, 5, DEVICE_A,
            "2026-04-30T10:00:00", 600,
            None, None, SessionMode::Timer,
        )).unwrap();
        db.apply_event(&synth_session_delete(SESSION_X, 10, DEVICE_A)).unwrap();
        assert!(db.list_sessions().unwrap().is_empty());
    }

    #[test]
    fn apply_event_tombstone_resists_later_applied_lower_lamport_insert() {
        // Out-of-order delivery: peer's delete arrives first (lamport=10),
        // then their insert at lamport=5 lands. The row must stay gone —
        // delete tombstones beat earlier inserts.
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_session_delete(SESSION_X, 10, DEVICE_A)).unwrap();
        db.apply_event(&synth_session_insert(
            SESSION_X, 5, DEVICE_A,
            "2026-04-30T10:00:00", 600,
            None, None, SessionMode::Timer,
        )).unwrap();
        assert!(db.list_sessions().unwrap().is_empty(),
            "tombstone with lamport 10 must beat insert at lamport 5");
    }

    #[test]
    fn apply_event_higher_lamport_update_supersedes_lower_one() {
        // Two updates from different devices on the same uuid; whichever
        // has the higher lamport_ts wins, regardless of arrival order.
        let db = Database::open_in_memory().unwrap();
        // Device A's update at lamport 10, Device B's at lamport 7 —
        // A wins. Apply B first (out of order), then A.
        db.apply_event(&synth_session_insert(
            SESSION_X, 1, DEVICE_A,
            "initial", 100, None, None, SessionMode::Timer,
        )).unwrap();
        db.apply_event(&synth_session_update(
            SESSION_X, 7, DEVICE_B,
            "B's edit", 700, None, Some("from B"), SessionMode::Timer,
        )).unwrap();
        db.apply_event(&synth_session_update(
            SESSION_X, 10, DEVICE_A,
            "A's edit", 1000, None, Some("from A"), SessionMode::BoxBreath,
        )).unwrap();
        let s = &db.list_sessions().unwrap()[0].1;
        assert_eq!(s.notes.as_deref(), Some("from A"),
            "A's lamport-10 update must win over B's lamport-7");
        assert_eq!(s.duration_secs, 1000);
    }

    #[test]
    fn apply_event_concurrent_updates_break_ties_on_device_id() {
        // Two updates with the SAME lamport_ts but different device_ids.
        // Lex-larger device_id wins (consistent across all peers per the
        // plan's tie-break rule).
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_session_insert(
            SESSION_X, 1, DEVICE_A,
            "initial", 100, None, None, SessionMode::Timer,
        )).unwrap();
        db.apply_event(&synth_session_update(
            SESSION_X, 5, DEVICE_A,
            "A wrote this", 500, None, Some("from A"), SessionMode::Timer,
        )).unwrap();
        db.apply_event(&synth_session_update(
            SESSION_X, 5, DEVICE_B,
            "B wrote this", 500, None, Some("from B"), SessionMode::Timer,
        )).unwrap();
        let s = &db.list_sessions().unwrap()[0].1;
        assert_eq!(s.notes.as_deref(), Some("from B"),
            "DEVICE_B is lex-larger than DEVICE_A; B's update wins on tie");
    }

    #[test]
    fn apply_event_records_the_event_in_the_log() {
        // After apply_event, the event must be in the events table so
        // future recomputes see it. Sync's push phase will pick it up
        // via pending_events (since `synced=0` by default).
        let db = Database::open_in_memory().unwrap();
        let event = synth_session_insert(
            SESSION_X, 5, DEVICE_A,
            "2026-04-30T10:00:00", 600,
            None, None, SessionMode::Timer,
        );
        let event_uuid = event.event_uuid.clone();
        db.apply_event(&event).unwrap();
        let pending = db.pending_events().unwrap();
        assert!(pending.iter().any(|(_, e)| e.event_uuid == event_uuid),
            "applied event must appear in events table");
    }

    #[test]
    fn apply_event_with_unknown_kind_is_a_silent_record_only() {
        // Forwards-compat: a future event kind we don't understand must
        // not panic or error. Record it — a future build can replay —
        // but don't try to mutate the cache from it.
        let db = Database::open_in_memory().unwrap();
        let weird = synth_event(
            "future_kind_not_yet_invented",
            SESSION_X, 5, DEVICE_A,
            serde_json::json!({"some": "future-data"}),
        );
        db.apply_event(&weird).unwrap();
        // Cache is empty (the event affected nothing it understood),
        // but the event was recorded.
        assert!(db.list_sessions().unwrap().is_empty());
        assert_eq!(db.pending_events().unwrap().len(), 1);
    }

    #[test]
    fn apply_event_session_insert_resolves_label_uuid_to_local_label_id() {
        // The peer's event references a label by label_uuid. If we have
        // a local label with that uuid, the materialized session must
        // link to it via local label_id. (Ensures cross-device
        // referential integrity survives the rowid-to-uuid translation.)
        let db = Database::open_in_memory().unwrap();
        let local_label_id = db.insert_label("Morning").unwrap();
        let label_uuid = db.list_labels().unwrap()[0].uuid.clone();
        drain_events(&db);

        db.apply_event(&synth_session_insert(
            SESSION_X, 5, DEVICE_A,
            "2026-04-30T10:00:00", 600,
            Some(&label_uuid), None, SessionMode::Timer,
        )).unwrap();
        let s = &db.list_sessions().unwrap()[0].1;
        assert_eq!(s.label_id, Some(local_label_id),
            "label_uuid must round-trip back to the local label_id");
    }

    // ── B1.2: apply_event for label events ───────────────────────────────────
    //
    // Same recompute pattern as sessions. label_delete cascades through
    // the FK (`ON DELETE SET NULL`) to clear `label_id` on any cached
    // sessions that referenced it.

    const LABEL_X: &str = "22222222-2222-4222-8222-222222222222";

    fn synth_label_insert(label_uuid: &str, lamport_ts: i64, device: &str, name: &str) -> Event {
        synth_event(
            "label_insert",
            label_uuid, lamport_ts, device,
            serde_json::json!({ "uuid": label_uuid, "name": name }),
        )
    }
    fn synth_label_rename(label_uuid: &str, lamport_ts: i64, device: &str, name: &str) -> Event {
        synth_event(
            "label_rename",
            label_uuid, lamport_ts, device,
            serde_json::json!({ "uuid": label_uuid, "name": name }),
        )
    }
    fn synth_label_delete(label_uuid: &str, lamport_ts: i64, device: &str) -> Event {
        synth_event(
            "label_delete",
            label_uuid, lamport_ts, device,
            serde_json::json!({ "uuid": label_uuid }),
        )
    }

    #[test]
    fn apply_event_label_insert_creates_the_label() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_label_insert(LABEL_X, 5, DEVICE_A, "Morning")).unwrap();
        let labels = db.list_labels().unwrap();
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].name, "Morning");
        assert_eq!(labels[0].uuid, LABEL_X);
    }

    #[test]
    fn apply_event_label_rename_updates_the_name() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_label_insert(LABEL_X, 5, DEVICE_A, "Morning")).unwrap();
        db.apply_event(&synth_label_rename(LABEL_X, 10, DEVICE_A, "Sunrise")).unwrap();
        let labels = db.list_labels().unwrap();
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].name, "Sunrise");
        assert_eq!(labels[0].uuid, LABEL_X,
            "rename must NOT change the cross-device uuid");
    }

    #[test]
    fn apply_event_label_delete_removes_the_label() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_label_insert(LABEL_X, 5, DEVICE_A, "Morning")).unwrap();
        db.apply_event(&synth_label_delete(LABEL_X, 10, DEVICE_A)).unwrap();
        assert!(db.list_labels().unwrap().is_empty());
    }

    #[test]
    fn apply_event_label_tombstone_resists_lower_lamport_insert() {
        let db = Database::open_in_memory().unwrap();
        // Delete arrives first at lamport 10.
        db.apply_event(&synth_label_delete(LABEL_X, 10, DEVICE_A)).unwrap();
        // Insert at lamport 5 arrives later — tombstone wins.
        db.apply_event(&synth_label_insert(LABEL_X, 5, DEVICE_A, "Morning")).unwrap();
        assert!(db.list_labels().unwrap().is_empty(),
            "tombstone with higher lamport must beat earlier insert");
    }

    #[test]
    fn apply_event_label_concurrent_renames_break_ties_on_device_id() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_label_insert(LABEL_X, 1, DEVICE_A, "Morning")).unwrap();
        db.apply_event(&synth_label_rename(LABEL_X, 5, DEVICE_A, "From A")).unwrap();
        db.apply_event(&synth_label_rename(LABEL_X, 5, DEVICE_B, "From B")).unwrap();
        let labels = db.list_labels().unwrap();
        assert_eq!(labels[0].name, "From B",
            "lex-larger device_id wins on lamport tie");
    }

    #[test]
    fn apply_event_label_delete_clears_label_id_on_cached_sessions() {
        // FK is `ON DELETE SET NULL` — when the labels row goes, sessions
        // that referenced it lose the link locally. Their session events
        // still carry the label_uuid; if the label later resurrects via
        // a higher-lamport insert, future recompute_session runs would
        // re-link.
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_label_insert(LABEL_X, 1, DEVICE_A, "Morning")).unwrap();
        db.apply_event(&synth_session_insert(
            SESSION_X, 2, DEVICE_A,
            "2026-04-30T10:00:00", 600,
            Some(LABEL_X), None, SessionMode::Timer,
        )).unwrap();
        // Sanity: the link is set.
        assert!(db.list_sessions().unwrap()[0].1.label_id.is_some());

        db.apply_event(&synth_label_delete(LABEL_X, 10, DEVICE_A)).unwrap();
        assert!(db.list_labels().unwrap().is_empty());
        let s = &db.list_sessions().unwrap()[0].1;
        assert_eq!(s.label_id, None,
            "session's label_id must clear when the label is deleted");
    }

    #[test]
    fn apply_event_label_is_idempotent() {
        let db = Database::open_in_memory().unwrap();
        let event = synth_label_insert(LABEL_X, 5, DEVICE_A, "Morning");
        db.apply_event(&event).unwrap();
        db.apply_event(&event).unwrap();
        assert_eq!(db.list_labels().unwrap().len(), 1);
    }

    // ── B1.3: apply_event for setting_changed ────────────────────────────────
    //
    // Settings have no tombstone (no `setting_delete` kind) — every
    // setting_changed event is a write. Conflict resolution: highest
    // (lamport_ts, device_id) wins per key. Out-of-order delivery is
    // handled by the same recompute-from-events approach.

    fn synth_setting_changed(key: &str, value: &str, lamport_ts: i64, device: &str) -> Event {
        synth_event(
            "setting_changed",
            key, lamport_ts, device,
            serde_json::json!({ "key": key, "value": value }),
        )
    }

    #[test]
    fn apply_event_setting_changed_writes_value_into_settings() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_setting_changed("daily_goal", "20", 5, DEVICE_A)).unwrap();
        assert_eq!(db.get_setting("daily_goal", "fallback").unwrap(), "20");
    }

    #[test]
    fn apply_event_higher_lamport_setting_overwrites_lower() {
        // First device A writes "20" at lamport 5; later device A writes
        // "30" at lamport 10. The newer value wins.
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_setting_changed("daily_goal", "20", 5, DEVICE_A)).unwrap();
        db.apply_event(&synth_setting_changed("daily_goal", "30", 10, DEVICE_A)).unwrap();
        assert_eq!(db.get_setting("daily_goal", "x").unwrap(), "30");
    }

    #[test]
    fn apply_event_out_of_order_settings_converge_correctly() {
        // The newer write (lamport=10) arrives BEFORE the older one
        // (lamport=5). The newer must still win after both are applied.
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_setting_changed("daily_goal", "30", 10, DEVICE_A)).unwrap();
        db.apply_event(&synth_setting_changed("daily_goal", "20", 5, DEVICE_A)).unwrap();
        assert_eq!(db.get_setting("daily_goal", "x").unwrap(), "30");
    }

    #[test]
    fn apply_event_setting_concurrent_writes_break_ties_on_device_id() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_setting_changed("daily_goal", "from A", 5, DEVICE_A)).unwrap();
        db.apply_event(&synth_setting_changed("daily_goal", "from B", 5, DEVICE_B)).unwrap();
        assert_eq!(db.get_setting("daily_goal", "x").unwrap(), "from B",
            "lex-larger device_id wins on lamport tie");
    }

    #[test]
    fn apply_event_settings_for_different_keys_do_not_collide() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_setting_changed("a", "alpha", 5, DEVICE_A)).unwrap();
        db.apply_event(&synth_setting_changed("b", "beta",  6, DEVICE_A)).unwrap();
        assert_eq!(db.get_setting("a", "x").unwrap(), "alpha");
        assert_eq!(db.get_setting("b", "x").unwrap(), "beta");
    }

    #[test]
    fn apply_event_setting_is_idempotent() {
        let db = Database::open_in_memory().unwrap();
        let event = synth_setting_changed("daily_goal", "20", 5, DEVICE_A);
        db.apply_event(&event).unwrap();
        db.apply_event(&event).unwrap();
        assert_eq!(db.get_setting("daily_goal", "x").unwrap(), "20");
    }

    // ── B2: replay_events ─────────────────────────────────────────────────────
    //
    // Bulk applier for incoming sync batches. Sorts the slice by
    // (lamport_ts ASC, device_id ASC, event_uuid ASC) for a stable
    // deterministic order, then dispatches each through apply_event's
    // recompute path. Idempotent on event_uuid, order-independent
    // because apply_event itself is.

    #[test]
    fn replay_events_with_empty_slice_is_a_noop() {
        let db = Database::open_in_memory().unwrap();
        db.replay_events(&[]).unwrap();
        assert!(db.list_sessions().unwrap().is_empty());
        assert!(db.list_labels().unwrap().is_empty());
        assert!(db.pending_events().unwrap().is_empty());
    }

    #[test]
    fn replay_events_with_one_event_matches_apply_event_alone() {
        let db = Database::open_in_memory().unwrap();
        let event = synth_session_insert(
            SESSION_X, 5, DEVICE_A,
            "2026-04-30T10:00:00", 600,
            None, None, SessionMode::Timer,
        );
        db.replay_events(std::slice::from_ref(&event)).unwrap();
        let rows = db.list_sessions().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1.uuid, SESSION_X);
    }

    #[test]
    fn replay_events_converges_regardless_of_input_order() {
        // The same event set in two different orders must produce the
        // same final cache state. This is the core convergence property.
        let session_b = "33333333-3333-4333-8333-333333333333";
        let events = vec![
            synth_session_insert(SESSION_X, 1, DEVICE_A,
                "S-X", 100, None, None, SessionMode::Timer),
            synth_session_insert(session_b, 2, DEVICE_A,
                "S-B", 200, None, None, SessionMode::Timer),
            synth_session_update(SESSION_X, 5, DEVICE_A,
                "S-X-edited", 150, None, Some("edit"), SessionMode::Timer),
            synth_session_delete(session_b, 6, DEVICE_A),
        ];

        let db_in_order = Database::open_in_memory().unwrap();
        db_in_order.replay_events(&events).unwrap();

        let mut shuffled = events.clone();
        shuffled.reverse();
        let db_reversed = Database::open_in_memory().unwrap();
        db_reversed.replay_events(&shuffled).unwrap();

        let in_order = db_in_order.list_sessions().unwrap();
        let reversed = db_reversed.list_sessions().unwrap();
        assert_eq!(in_order.len(), 1, "session_b must be tombstoned away");
        assert_eq!(in_order.len(), reversed.len(),
            "convergence: same event set yields same row count regardless of order");
        assert_eq!(in_order[0].1.uuid, reversed[0].1.uuid);
        assert_eq!(in_order[0].1.start_iso, reversed[0].1.start_iso);
        assert_eq!(in_order[0].1.duration_secs, reversed[0].1.duration_secs);
        assert_eq!(in_order[0].1.notes, reversed[0].1.notes);
    }

    #[test]
    fn replay_events_dedups_duplicate_event_uuids() {
        // Same Event present twice in the input slice must be applied
        // only once — no double row, no error. Real-world cause:
        // overlapping pull windows or peer-forwarded duplicates.
        let db = Database::open_in_memory().unwrap();
        let event = synth_session_insert(
            SESSION_X, 5, DEVICE_A,
            "2026-04-30T10:00:00", 600,
            None, None, SessionMode::Timer,
        );
        db.replay_events(&[event.clone(), event]).unwrap();
        assert_eq!(db.list_sessions().unwrap().len(), 1);
    }

    #[test]
    fn replay_events_two_devices_authoring_independently_merges_both() {
        // Realistic scenario: two devices author concurrently, then each
        // pulls the other's events. After cross-replay both DBs have the
        // union of both devices' inserts.
        let device_a = Database::open_in_memory().unwrap();
        let device_b = Database::open_in_memory().unwrap();

        device_a.insert_session(&Session {
            start_iso: "2026-04-30T10:00:00".to_string(),
            duration_secs: 600, label_id: None, notes: Some("from A".to_string()),
            mode: SessionMode::Timer, uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        device_b.insert_session(&Session {
            start_iso: "2026-04-30T18:00:00".to_string(),
            duration_secs: 1200, label_id: None, notes: Some("from B".to_string()),
            mode: SessionMode::Timer, uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();

        let events_a: Vec<Event> = device_a.pending_events().unwrap()
            .into_iter().map(|(_, e)| e).collect();
        let events_b: Vec<Event> = device_b.pending_events().unwrap()
            .into_iter().map(|(_, e)| e).collect();

        device_a.replay_events(&events_b).unwrap();
        device_b.replay_events(&events_a).unwrap();

        let sessions_a = device_a.list_sessions().unwrap();
        let sessions_b = device_b.list_sessions().unwrap();
        assert_eq!(sessions_a.len(), 2);
        assert_eq!(sessions_b.len(), 2);

        let notes_a: std::collections::HashSet<_> = sessions_a.iter()
            .filter_map(|(_, s)| s.notes.clone()).collect();
        let notes_b: std::collections::HashSet<_> = sessions_b.iter()
            .filter_map(|(_, s)| s.notes.clone()).collect();
        let expected: std::collections::HashSet<_> = ["from A", "from B"]
            .iter().map(|s| s.to_string()).collect();
        assert_eq!(notes_a, expected);
        assert_eq!(notes_b, expected,
            "after cross-replay, both devices must hold the same union of events");
    }

    #[test]
    fn replay_events_idempotent_under_repeat_application() {
        // Replaying the same batch twice produces the same state as
        // replaying it once. Important for sync reliability — a partial
        // sync that retries the whole batch must not corrupt state.
        let device_a = Database::open_in_memory().unwrap();
        let device_b = Database::open_in_memory().unwrap();
        for i in 0..3 {
            device_a.insert_session(&Session {
                start_iso: format!("2026-04-3{i}T10:00:00"),
                duration_secs: 600, label_id: None, notes: None,
                mode: SessionMode::Timer, uuid: String::new(),
                guided_file_uuid: None,
            }).unwrap();
        }
        let events: Vec<Event> = device_a.pending_events().unwrap()
            .into_iter().map(|(_, e)| e).collect();
        device_b.replay_events(&events).unwrap();
        let after_first = device_b.list_sessions().unwrap();
        device_b.replay_events(&events).unwrap();
        let after_second = device_b.list_sessions().unwrap();
        assert_eq!(after_first.len(), after_second.len());
        assert_eq!(after_first, after_second,
            "second replay of the same batch must be a no-op on the cache");
    }

    // ── Lamport observation rule on apply_event (regression) ────────────────
    //
    // Per Nextcloud-Sync.md: "on remote event observation: lamport =
    // max(lamport, remote.lamport) + 1". apply_event must advance the
    // local clock for fresh remote events so a follow-up local write
    // strictly orders after what we just observed. Skipped for our own
    // device's events (idempotency) and for duplicates (only first
    // observation counts).

    #[test]
    fn apply_event_advances_local_lamport_when_observing_a_higher_remote_event() {
        // Local clock starts at 0. We see a remote event tagged
        // lamport=10 from a different device. After applying, our
        // local clock must be max(0,10)+1 = 11 — so any event we
        // author next will sort strictly after the observed one.
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.lamport_clock().unwrap(), 0);
        db.apply_event(&synth_session_insert(
            SESSION_X, 10, DEVICE_A,
            "2026-04-30T10:00:00", 600,
            None, None, SessionMode::Timer,
        )).unwrap();
        assert_eq!(db.lamport_clock().unwrap(), 11,
            "observation rule: local must jump to max(local, remote)+1");
    }

    #[test]
    fn apply_event_advances_local_lamport_even_when_local_is_already_ahead() {
        // Local has done lots of work (clock at 50). Remote observation
        // at lamport=10 must still advance to max(50,10)+1=51 — every
        // observation strictly increases the clock so no two events
        // ever share a (lamport, device_id) pair on the same device.
        let db = Database::open_in_memory().unwrap();
        for _ in 0..50 { db.bump_lamport_clock().unwrap(); }
        assert_eq!(db.lamport_clock().unwrap(), 50);
        db.apply_event(&synth_session_insert(
            SESSION_X, 10, DEVICE_A,
            "_", 1, None, None, SessionMode::Timer,
        )).unwrap();
        assert_eq!(db.lamport_clock().unwrap(), 51);
    }

    #[test]
    fn apply_event_does_not_advance_local_lamport_for_our_own_device_events() {
        // Re-applying an event we authored locally (idempotency retry,
        // or pulling our own event back from remote storage) must not
        // shift the clock. Otherwise a "harmless retry" would silently
        // mutate clock state and break ordering invariants.
        let db = Database::open_in_memory().unwrap();
        let our_device_id = db.device_id().unwrap();
        db.bump_lamport_clock().unwrap();
        db.bump_lamport_clock().unwrap();
        let before = db.lamport_clock().unwrap();
        // Author an event "from us" with a very high lamport value.
        let our_event = synth_session_insert(
            SESSION_X, 999, &our_device_id,
            "_", 1, None, None, SessionMode::Timer,
        );
        db.apply_event(&our_event).unwrap();
        assert_eq!(db.lamport_clock().unwrap(), before,
            "apply_event with our own device_id must not bump the clock");
    }

    #[test]
    fn apply_event_does_not_advance_local_lamport_on_duplicate_remote_observation() {
        // Receiving the same event twice — e.g. overlapping pull
        // windows or peer-forwarded duplicates — must only bump the
        // clock once. The bump is per *new observation*, not per call.
        let db = Database::open_in_memory().unwrap();
        let event = synth_session_insert(
            SESSION_X, 10, DEVICE_A,
            "_", 1, None, None, SessionMode::Timer,
        );
        db.apply_event(&event).unwrap();
        let after_first = db.lamport_clock().unwrap();
        db.apply_event(&event).unwrap();
        let after_second = db.lamport_clock().unwrap();
        assert_eq!(after_first, after_second,
            "second observation of the same event_uuid must not bump");
    }

    #[test]
    fn local_writes_after_observing_a_remote_event_strictly_order_after_it() {
        // The end-to-end correctness property: a write authored after
        // observing a remote event must have a strictly larger
        // lamport_ts than the remote event. Without the observation
        // rule, a slow local clock would author "in the past" and
        // peers would resolve it as the older write — wrong.
        let db = Database::open_in_memory().unwrap();
        // Remote event at lamport=20 lands on a fresh local DB.
        db.apply_event(&synth_session_insert(
            SESSION_X, 20, DEVICE_A,
            "remote", 100, None, None, SessionMode::Timer,
        )).unwrap();
        // Now author a local session. Its event must have lamport > 20.
        db.insert_session(&Session {
            start_iso: "local".into(),
            duration_secs: 200,
            label_id: None,
            notes: None,
            mode: SessionMode::Timer,
            uuid: String::new(),
            guided_file_uuid: None,
        }).unwrap();
        let local_event = db.pending_events().unwrap()
            .into_iter()
            .find(|(_, e)| e.kind == "session_insert" && e.device_id == db.device_id().unwrap())
            .map(|(_, e)| e)
            .expect("local session_insert must be in pending events");
        assert!(local_event.lamport_ts > 20,
            "local event at lamport {} must order strictly after observed remote at 20",
            local_event.lamport_ts);
    }

    #[test]
    fn replay_events_advances_lamport_through_the_observation_rule() {
        // replay_events processes a batch via apply_event_inner, which
        // includes the observation step. After replaying a batch from
        // a peer whose highest lamport was N, our local clock must be
        // ≥ N+1 so subsequent local writes order after the batch.
        let db = Database::open_in_memory().unwrap();
        let batch = vec![
            synth_session_insert(SESSION_X, 5, DEVICE_A,
                "_", 1, None, None, SessionMode::Timer),
            synth_session_update(SESSION_X, 12, DEVICE_A,
                "_", 1, None, None, SessionMode::Timer),
        ];
        db.replay_events(&batch).unwrap();
        assert!(db.lamport_clock().unwrap() >= 13,
            "after replaying a batch up to lamport 12, local clock must be >= 13, got {}",
            db.lamport_clock().unwrap());
    }

    #[test]
    fn replay_events_handles_mixed_kinds_in_one_batch() {
        // A realistic batch: an insert label, an insert session that
        // references the label, an update session, a delete label, and
        // a settings change. Apply all together and the final cache
        // reflects every conflict-resolution rule.
        let db = Database::open_in_memory().unwrap();
        let events = vec![
            synth_label_insert(LABEL_X, 1, DEVICE_A, "Morning"),
            synth_session_insert(
                SESSION_X, 2, DEVICE_A,
                "10:00", 600, Some(LABEL_X), None, SessionMode::Timer,
            ),
            synth_session_update(
                SESSION_X, 3, DEVICE_A,
                "10:00", 900, Some(LABEL_X), Some("longer"), SessionMode::Timer,
            ),
            synth_label_delete(LABEL_X, 4, DEVICE_A),
            synth_setting_changed("daily_goal", "20", 5, DEVICE_A),
        ];
        db.replay_events(&events).unwrap();

        // Label is gone (deleted at lamport 4 after insert at 1).
        assert!(db.list_labels().unwrap().is_empty());
        // Session is present with the lamport-3 update's values, but
        // its label_id is NULL because the label has been deleted.
        let s = &db.list_sessions().unwrap()[0].1;
        assert_eq!(s.duration_secs, 900);
        assert_eq!(s.notes.as_deref(), Some("longer"));
        assert_eq!(s.label_id, None,
            "session keeps its data but loses the label link when the label tombstones");
        assert_eq!(db.get_setting("daily_goal", "x").unwrap(), "20");
    }

    // ── Interval-bell library ────────────────────────────────────────
    // Tests pin: enum string round-trip, schema bring-up, and the
    // base CRUD that the UI list page in B.3.3 will call.

    #[test]
    fn interval_bell_kind_round_trips_through_db_strings() {
        assert_eq!(IntervalBellKind::Interval.as_db_str(), "interval");
        assert_eq!(IntervalBellKind::FixedFromStart.as_db_str(), "fixed_from_start");
        assert_eq!(IntervalBellKind::FixedFromEnd.as_db_str(), "fixed_from_end");
        assert_eq!(
            IntervalBellKind::from_db_str("interval"),
            Some(IntervalBellKind::Interval)
        );
        assert_eq!(
            IntervalBellKind::from_db_str("fixed_from_start"),
            Some(IntervalBellKind::FixedFromStart)
        );
        assert_eq!(
            IntervalBellKind::from_db_str("fixed_from_end"),
            Some(IntervalBellKind::FixedFromEnd)
        );
    }

    #[test]
    fn interval_bell_kind_from_db_str_rejects_unknown() {
        assert_eq!(IntervalBellKind::from_db_str(""), None);
        assert_eq!(IntervalBellKind::from_db_str("INTERVAL"), None);
        assert_eq!(IntervalBellKind::from_db_str("from_start"), None);
        assert_eq!(IntervalBellKind::from_db_str("garbage"), None);
    }

    #[test]
    fn insert_interval_bell_inserts_a_row_with_uuid_and_returns_rowid() {
        let db = Database::open_in_memory().unwrap();
        let rowid = db
            .insert_interval_bell(
                IntervalBellKind::Interval, 9, 30, "bowl",
                BUNDLED_PATTERN_PULSE_UUID, SignalMode::Sound,
            )
            .unwrap();
        assert!(rowid > 0);
        let bells = db.list_interval_bells().unwrap();
        assert_eq!(bells.len(), 1);
        let b = &bells[0];
        assert_eq!(b.id, rowid);
        assert!(!b.uuid.is_empty(), "uuid is minted at insert");
        assert_eq!(b.kind, IntervalBellKind::Interval);
        assert_eq!(b.minutes, 9);
        assert_eq!(b.jitter_pct, 30);
        assert_eq!(b.sound, "bowl");
        assert!(b.enabled, "new bells default to enabled");
        assert!(!b.created_iso.is_empty());
    }

    #[test]
    fn insert_interval_bell_emits_an_interval_bell_insert_event() {
        let db = Database::open_in_memory().unwrap();
        db.insert_interval_bell(
            IntervalBellKind::FixedFromStart, 10, 0, "bell",
            BUNDLED_PATTERN_PULSE_UUID, SignalMode::Sound,
        ).unwrap();
        let events = db.pending_events().unwrap();
        // Schema-init may pre-record device-init events; filter to our kind.
        let mine: Vec<_> = events
            .iter()
            .filter(|(_, e)| e.kind == "interval_bell_insert")
            .collect();
        assert_eq!(mine.len(), 1);
        let payload: serde_json::Value =
            serde_json::from_str(&mine[0].1.payload).unwrap();
        assert_eq!(payload["kind"], "fixed_from_start");
        assert_eq!(payload["minutes"], 10);
        assert_eq!(payload["jitter_pct"], 0);
        assert_eq!(payload["sound"], "bell");
        assert_eq!(payload["enabled"], true);
        assert!(payload["uuid"].is_string());
        assert!(payload["created_iso"].is_string());
    }

    #[test]
    fn list_interval_bells_returns_rows_in_insert_order() {
        let db = Database::open_in_memory().unwrap();
        db.insert_interval_bell(IntervalBellKind::Interval, 5, 0, "bowl", BUNDLED_PATTERN_PULSE_UUID, SignalMode::Sound).unwrap();
        db.insert_interval_bell(IntervalBellKind::FixedFromStart, 10, 0, "bell", BUNDLED_PATTERN_PULSE_UUID, SignalMode::Sound).unwrap();
        db.insert_interval_bell(IntervalBellKind::FixedFromEnd, 5, 0, "gong", BUNDLED_PATTERN_PULSE_UUID, SignalMode::Sound).unwrap();
        let bells = db.list_interval_bells().unwrap();
        assert_eq!(bells.len(), 3);
        // Insert order — rowid ASC, deterministic.
        assert_eq!(bells[0].kind, IntervalBellKind::Interval);
        assert_eq!(bells[1].kind, IntervalBellKind::FixedFromStart);
        assert_eq!(bells[2].kind, IntervalBellKind::FixedFromEnd);
    }

    #[test]
    fn list_interval_bells_returns_empty_when_none_inserted() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.list_interval_bells().unwrap().is_empty());
    }

    #[test]
    fn update_interval_bell_overwrites_every_mutable_field() {
        let db = Database::open_in_memory().unwrap();
        db.insert_interval_bell(IntervalBellKind::Interval, 5, 0, "bowl", BUNDLED_PATTERN_PULSE_UUID, SignalMode::Sound).unwrap();
        let uuid = db.list_interval_bells().unwrap()[0].uuid.clone();

        db.update_interval_bell(
            &uuid,
            IntervalBellKind::FixedFromStart,
            12,
            25,
            "bell",
            BUNDLED_PATTERN_PULSE_UUID,
            SignalMode::Sound,
            false,
        ).unwrap();

        let b = &db.list_interval_bells().unwrap()[0];
        assert_eq!(b.kind, IntervalBellKind::FixedFromStart);
        assert_eq!(b.minutes, 12);
        assert_eq!(b.jitter_pct, 25);
        assert_eq!(b.sound, "bell");
        assert!(!b.enabled);
        // uuid + created_iso must be untouched by an update.
        assert_eq!(b.uuid, uuid);
    }

    #[test]
    fn update_interval_bell_emits_an_interval_bell_update_event() {
        let db = Database::open_in_memory().unwrap();
        db.insert_interval_bell(IntervalBellKind::Interval, 5, 0, "bowl", BUNDLED_PATTERN_PULSE_UUID, SignalMode::Sound).unwrap();
        let uuid = db.list_interval_bells().unwrap()[0].uuid.clone();

        db.update_interval_bell(
            &uuid,
            IntervalBellKind::Interval,
            9,
            30,
            "gong",
            BUNDLED_PATTERN_PULSE_UUID,
            SignalMode::Sound,
            true,
        ).unwrap();

        let events = db.pending_events().unwrap();
        let updates: Vec<_> = events
            .iter()
            .filter(|(_, e)| e.kind == "interval_bell_update")
            .collect();
        assert_eq!(updates.len(), 1);
        let payload: serde_json::Value =
            serde_json::from_str(&updates[0].1.payload).unwrap();
        assert_eq!(payload["uuid"], uuid);
        assert_eq!(payload["kind"], "interval");
        assert_eq!(payload["minutes"], 9);
        assert_eq!(payload["jitter_pct"], 30);
        assert_eq!(payload["sound"], "gong");
        assert_eq!(payload["enabled"], true);
    }

    #[test]
    fn update_interval_bell_unknown_uuid_is_silent_noop() {
        // Same shape as update_label — peers receiving an update for a
        // tombstoned-locally row would otherwise loop. No event emitted.
        let db = Database::open_in_memory().unwrap();
        db.update_interval_bell(
            "non-existent-uuid",
            IntervalBellKind::Interval,
            5,
            0,
            "bowl",
            BUNDLED_PATTERN_PULSE_UUID,
            SignalMode::Sound,
            true,
        ).unwrap();
        let updates: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "interval_bell_update")
            .collect();
        assert!(updates.is_empty());
    }

    #[test]
    fn delete_interval_bell_removes_the_row() {
        let db = Database::open_in_memory().unwrap();
        db.insert_interval_bell(IntervalBellKind::Interval, 5, 0, "bowl", BUNDLED_PATTERN_PULSE_UUID, SignalMode::Sound).unwrap();
        let uuid = db.list_interval_bells().unwrap()[0].uuid.clone();
        db.delete_interval_bell(&uuid).unwrap();
        assert!(db.list_interval_bells().unwrap().is_empty());
    }

    #[test]
    fn delete_interval_bell_emits_a_delete_event_with_uuid_target() {
        let db = Database::open_in_memory().unwrap();
        db.insert_interval_bell(IntervalBellKind::Interval, 5, 0, "bowl", BUNDLED_PATTERN_PULSE_UUID, SignalMode::Sound).unwrap();
        let uuid = db.list_interval_bells().unwrap()[0].uuid.clone();
        db.delete_interval_bell(&uuid).unwrap();
        let deletes: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "interval_bell_delete")
            .collect();
        assert_eq!(deletes.len(), 1);
        assert_eq!(deletes[0].1.target_id, uuid);
        let payload: serde_json::Value =
            serde_json::from_str(&deletes[0].1.payload).unwrap();
        assert_eq!(payload["uuid"], uuid);
    }

    #[test]
    fn delete_interval_bell_unknown_uuid_is_silent_noop() {
        let db = Database::open_in_memory().unwrap();
        db.delete_interval_bell("non-existent-uuid").unwrap();
        let deletes: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "interval_bell_delete")
            .collect();
        assert!(deletes.is_empty());
    }

    #[test]
    fn set_interval_bell_enabled_toggles_the_flag_only() {
        // Convenience helper for the common path: SwitchRow toggle flips
        // enabled without the UI having to round-trip the other fields.
        let db = Database::open_in_memory().unwrap();
        db.insert_interval_bell(IntervalBellKind::Interval, 9, 30, "bell", BUNDLED_PATTERN_PULSE_UUID, SignalMode::Sound).unwrap();
        let uuid = db.list_interval_bells().unwrap()[0].uuid.clone();

        db.set_interval_bell_enabled(&uuid, false).unwrap();
        let b = &db.list_interval_bells().unwrap()[0];
        assert!(!b.enabled);
        // Other fields must be preserved verbatim — this is just a flag flip.
        assert_eq!(b.kind, IntervalBellKind::Interval);
        assert_eq!(b.minutes, 9);
        assert_eq!(b.jitter_pct, 30);
        assert_eq!(b.sound, "bell");

        db.set_interval_bell_enabled(&uuid, true).unwrap();
        assert!(db.list_interval_bells().unwrap()[0].enabled);
    }

    #[test]
    fn set_interval_bell_enabled_emits_an_update_event_with_new_state() {
        // Toggle goes through the same "update" event channel as a full
        // update — peers reconstruct state by replaying events, so a
        // discrete "enabled flipped" event would just complicate apply.
        let db = Database::open_in_memory().unwrap();
        db.insert_interval_bell(IntervalBellKind::Interval, 9, 30, "bell", BUNDLED_PATTERN_PULSE_UUID, SignalMode::Sound).unwrap();
        let uuid = db.list_interval_bells().unwrap()[0].uuid.clone();
        db.set_interval_bell_enabled(&uuid, false).unwrap();

        let updates: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "interval_bell_update")
            .collect();
        assert_eq!(updates.len(), 1);
        let payload: serde_json::Value =
            serde_json::from_str(&updates[0].1.payload).unwrap();
        assert_eq!(payload["enabled"], false);
        assert_eq!(payload["minutes"], 9);
    }

    // ── Sync round-trip + tombstone precedence ────────────────────
    // Mirrors the same shape as the existing apply_event tests for
    // labels — a peer replays events and ends up at the same row state.

    fn synth_interval_bell_insert(
        bell_uuid: &str,
        lamport_ts: i64,
        device: &str,
        kind: IntervalBellKind,
        minutes: u32,
        jitter_pct: u32,
        sound: &str,
    ) -> Event {
        Event {
            event_uuid: format!("ev-insert-{bell_uuid}-{lamport_ts}-{device}"),
            lamport_ts,
            device_id: device.to_string(),
            kind: "interval_bell_insert".to_string(),
            target_id: bell_uuid.to_string(),
            payload: serde_json::json!({
                "uuid": bell_uuid,
                "kind": kind.as_db_str(),
                "minutes": minutes,
                "jitter_pct": jitter_pct,
                "sound": sound,
                "enabled": true,
                "created_iso": "2026-05-03T12:00:00Z",
            }).to_string(),
        }
    }

    fn synth_interval_bell_update(
        bell_uuid: &str,
        lamport_ts: i64,
        device: &str,
        minutes: u32,
        enabled: bool,
    ) -> Event {
        Event {
            event_uuid: format!("ev-update-{bell_uuid}-{lamport_ts}-{device}"),
            lamport_ts,
            device_id: device.to_string(),
            kind: "interval_bell_update".to_string(),
            target_id: bell_uuid.to_string(),
            payload: serde_json::json!({
                "uuid": bell_uuid,
                "kind": "interval",
                "minutes": minutes,
                "jitter_pct": 0,
                "sound": "bowl",
                "enabled": enabled,
                "created_iso": "2026-05-03T12:00:00Z",
            }).to_string(),
        }
    }

    fn synth_interval_bell_delete(
        bell_uuid: &str,
        lamport_ts: i64,
        device: &str,
    ) -> Event {
        Event {
            event_uuid: format!("ev-delete-{bell_uuid}-{lamport_ts}-{device}"),
            lamport_ts,
            device_id: device.to_string(),
            kind: "interval_bell_delete".to_string(),
            target_id: bell_uuid.to_string(),
            payload: serde_json::json!({ "uuid": bell_uuid }).to_string(),
        }
    }

    #[test]
    fn apply_event_interval_bell_insert_creates_the_row() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_interval_bell_insert(
            "bell-1", 5, "dev-A",
            IntervalBellKind::Interval, 9, 30, "bell",
        )).unwrap();
        let bells = db.list_interval_bells().unwrap();
        assert_eq!(bells.len(), 1);
        assert_eq!(bells[0].uuid, "bell-1");
        assert_eq!(bells[0].kind, IntervalBellKind::Interval);
        assert_eq!(bells[0].minutes, 9);
        assert_eq!(bells[0].jitter_pct, 30);
        assert_eq!(bells[0].sound, "bell");
        assert!(bells[0].enabled);
    }

    #[test]
    fn apply_event_interval_bell_update_applies_after_insert() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_interval_bell_insert(
            "bell-1", 5, "dev-A",
            IntervalBellKind::Interval, 9, 30, "bell",
        )).unwrap();
        db.apply_event(&synth_interval_bell_update(
            "bell-1", 7, "dev-A", 12, false,
        )).unwrap();
        let b = &db.list_interval_bells().unwrap()[0];
        assert_eq!(b.minutes, 12);
        assert!(!b.enabled);
    }

    #[test]
    fn apply_event_interval_bell_delete_removes_the_row() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_interval_bell_insert(
            "bell-1", 5, "dev-A",
            IntervalBellKind::Interval, 9, 30, "bell",
        )).unwrap();
        db.apply_event(&synth_interval_bell_delete("bell-1", 6, "dev-A")).unwrap();
        assert!(db.list_interval_bells().unwrap().is_empty());
    }

    #[test]
    fn apply_event_interval_bell_tombstone_resists_lower_lamport_insert() {
        // Out-of-order arrival: the delete from device A (lamport 10) lands
        // on device B before A's earlier insert (lamport 5). Tombstone wins.
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_interval_bell_delete("bell-1", 10, "dev-A")).unwrap();
        db.apply_event(&synth_interval_bell_insert(
            "bell-1", 5, "dev-A",
            IntervalBellKind::Interval, 9, 30, "bell",
        )).unwrap();
        assert!(db.list_interval_bells().unwrap().is_empty(),
            "delete at lamport 10 must outrank insert at lamport 5");
    }

    #[test]
    fn apply_event_interval_bell_higher_lamport_update_supersedes_lower_one() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_interval_bell_insert(
            "bell-1", 5, "dev-A",
            IntervalBellKind::Interval, 9, 30, "bell",
        )).unwrap();
        // Two competing updates from different devices.
        db.apply_event(&synth_interval_bell_update("bell-1", 7, "dev-A", 12, true)).unwrap();
        db.apply_event(&synth_interval_bell_update("bell-1", 8, "dev-B", 18, true)).unwrap();
        let b = &db.list_interval_bells().unwrap()[0];
        assert_eq!(b.minutes, 18, "higher lamport (8 from dev-B) wins over (7 from dev-A)");
    }

    // ── Bell-sound library (B.4.1) ───────────────────────────────
    // Audio-file rows the bell sites reference by uuid.

    #[test]
    fn insert_bell_sound_inserts_a_row_with_uuid_and_returns_rowid() {
        let db = Database::open_in_memory().unwrap();
        let rowid = db
            .insert_bell_sound(
                "Tibetan Bowl",
                "/io/github/janekbt/Meditate/sounds/bowl.wav",
                true,
                "audio/wav",
                BellSoundCategory::General,
            )
            .unwrap();
        assert!(rowid > 0);
        let sounds = db.list_bell_sounds().unwrap();
        assert_eq!(sounds.len(), 1);
        let s = &sounds[0];
        assert_eq!(s.id, rowid);
        assert!(!s.uuid.is_empty(), "uuid is minted at insert");
        assert_eq!(s.name, "Tibetan Bowl");
        assert_eq!(s.file_path, "/io/github/janekbt/Meditate/sounds/bowl.wav");
        assert!(s.is_bundled);
        assert_eq!(s.mime_type, "audio/wav");
        assert_eq!(s.category, BellSoundCategory::General);
        assert!(!s.created_iso.is_empty());
    }

    #[test]
    fn insert_bell_sound_with_explicit_uuid_uses_it() {
        // Stable bundled UUIDs must be reusable across devices — the
        // seed path passes them in rather than letting the DB mint a
        // fresh one each time.
        let db = Database::open_in_memory().unwrap();
        let fixed = "11111111-2222-3333-4444-555555555555";
        let rowid = db
            .insert_bell_sound_with_uuid(
                fixed,
                "Bundled bowl",
                "/io/github/janekbt/Meditate/sounds/bowl.wav",
                true,
                "audio/wav",
                BellSoundCategory::General,
            )
            .unwrap();
        assert!(rowid > 0);
        let s = &db.list_bell_sounds().unwrap()[0];
        assert_eq!(s.uuid, fixed);
    }

    #[test]
    fn insert_bell_sound_with_existing_uuid_is_silent_noop() {
        // Idempotent seed: a re-run with the same uuid skips the
        // insert AND emits no event (no peer needs to learn we
        // re-tried what they already have).
        let db = Database::open_in_memory().unwrap();
        let fixed = "22222222-2222-3333-4444-555555555555";
        let r1 = db.insert_bell_sound_with_uuid(
            fixed, "Bowl", "/path/bowl.wav", true, "audio/wav",
            BellSoundCategory::General,
        ).unwrap();
        let r2 = db.insert_bell_sound_with_uuid(
            fixed, "Bowl", "/path/bowl.wav", true, "audio/wav",
            BellSoundCategory::General,
        ).unwrap();
        assert_eq!(r1, r2, "second call returns the existing rowid");
        assert_eq!(db.list_bell_sounds().unwrap().len(), 1);
        // Only one bell_sound_insert event in pending.
        let inserts: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "bell_sound_insert")
            .collect();
        assert_eq!(inserts.len(), 1);
    }

    #[test]
    fn bell_sound_category_round_trips_through_db_str() {
        for c in [BellSoundCategory::General, BellSoundCategory::BoxBreath] {
            assert_eq!(BellSoundCategory::from_db_str(c.as_db_str()), Some(c));
        }
        assert_eq!(BellSoundCategory::from_db_str("typo"), None);
    }

    #[test]
    fn bell_sounds_category_check_constraint_rejects_unknown_value() {
        let db = Database::open_in_memory().unwrap();
        let bad = db.conn.execute(
            "INSERT INTO bell_sounds
                (uuid, name, file_path, is_bundled, mime_type, category, created_iso)
             VALUES ('u', 'n', '/p', 0, 'audio/wav', 'spiral', 'now')",
            [],
        );
        assert!(bad.is_err(),
            "category = 'spiral' must be rejected by the CHECK constraint");
    }

    #[test]
    fn list_bell_sounds_for_category_filters_by_category() {
        let db = Database::open_in_memory().unwrap();
        db.insert_bell_sound(
            "Bowl", "/p/bowl.wav", true, "audio/wav",
            BellSoundCategory::General,
        ).unwrap();
        db.insert_bell_sound(
            "Inhale", "/p/inhale.ogg", false, "audio/ogg",
            BellSoundCategory::BoxBreath,
        ).unwrap();
        db.insert_bell_sound(
            "Hold", "/p/hold.ogg", false, "audio/ogg",
            BellSoundCategory::BoxBreath,
        ).unwrap();

        let general = db.list_bell_sounds_for_category(BellSoundCategory::General).unwrap();
        assert_eq!(general.len(), 1);
        assert_eq!(general[0].name, "Bowl");

        let box_breath = db
            .list_bell_sounds_for_category(BellSoundCategory::BoxBreath)
            .unwrap();
        assert_eq!(box_breath.len(), 2);
        let names: Vec<&str> = box_breath.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Inhale"));
        assert!(names.contains(&"Hold"));
    }

    #[test]
    fn list_bell_sounds_unfiltered_returns_every_category() {
        let db = Database::open_in_memory().unwrap();
        db.insert_bell_sound(
            "Bowl", "/p/bowl.wav", true, "audio/wav",
            BellSoundCategory::General,
        ).unwrap();
        db.insert_bell_sound(
            "Inhale", "/p/inhale.ogg", false, "audio/ogg",
            BellSoundCategory::BoxBreath,
        ).unwrap();
        let all = db.list_bell_sounds().unwrap();
        assert_eq!(all.len(), 2,
            "unfiltered list spans every category — used by Manage Sounds in Preferences");
    }

    #[test]
    fn insert_bell_sound_emits_a_bell_sound_insert_event() {
        let db = Database::open_in_memory().unwrap();
        db.insert_bell_sound("Zen Bell", "/path/zen.wav", true, "audio/wav", BellSoundCategory::General).unwrap();
        let events = db.pending_events().unwrap();
        let inserts: Vec<_> = events
            .iter()
            .filter(|(_, e)| e.kind == "bell_sound_insert")
            .collect();
        assert_eq!(inserts.len(), 1);
        let payload: serde_json::Value =
            serde_json::from_str(&inserts[0].1.payload).unwrap();
        assert_eq!(payload["name"], "Zen Bell");
        assert_eq!(payload["file_path"], "/path/zen.wav");
        assert_eq!(payload["is_bundled"], true);
        assert_eq!(payload["mime_type"], "audio/wav");
        assert!(payload["uuid"].is_string());
        assert!(payload["created_iso"].is_string());
    }

    #[test]
    fn list_bell_sounds_returns_custom_rows_before_bundled() {
        // Chooser UX: a freshly imported sound lives at the top of the
        // list, directly under the synthetic "Choose your own…" entry,
        // so the user doesn't have to scroll past the bundled set.
        // Within each group, insertion order is preserved.
        let db = Database::open_in_memory().unwrap();
        db.insert_bell_sound("A", "/p/a.wav", true, "audio/wav", BellSoundCategory::General).unwrap();
        db.insert_bell_sound("B", "/p/b.wav", false, "audio/wav", BellSoundCategory::General).unwrap();
        db.insert_bell_sound("C", "/p/c.wav", true, "audio/wav", BellSoundCategory::General).unwrap();
        db.insert_bell_sound("D", "/p/d.wav", false, "audio/wav", BellSoundCategory::General).unwrap();
        let s = db.list_bell_sounds().unwrap();
        let names: Vec<_> = s.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["B", "D", "A", "C"]);
    }

    #[test]
    fn list_bell_sounds_returns_empty_when_none_inserted() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.list_bell_sounds().unwrap().is_empty());
    }

    #[test]
    fn rename_bell_sound_changes_name_and_emits_update_event() {
        // The only mutable property is `name`. file_path / is_bundled /
        // mime_type are immutable for the row's lifetime — bundled is
        // determined at seed time, custom file_path at import time.
        let db = Database::open_in_memory().unwrap();
        db.insert_bell_sound("Bowl", "/p/bowl.wav", true, "audio/wav", BellSoundCategory::General).unwrap();
        let uuid = db.list_bell_sounds().unwrap()[0].uuid.clone();
        db.rename_bell_sound(&uuid, "Singing Bowl").unwrap();
        assert_eq!(db.list_bell_sounds().unwrap()[0].name, "Singing Bowl");

        let updates: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "bell_sound_update")
            .collect();
        assert_eq!(updates.len(), 1);
        let payload: serde_json::Value =
            serde_json::from_str(&updates[0].1.payload).unwrap();
        assert_eq!(payload["name"], "Singing Bowl");
        assert_eq!(payload["uuid"], uuid);
        // file_path + is_bundled + mime_type ride along so a peer that
        // has the rename event but missed the insert can still
        // materialise the row.
        assert_eq!(payload["file_path"], "/p/bowl.wav");
        assert_eq!(payload["is_bundled"], true);
        assert_eq!(payload["mime_type"], "audio/wav");
    }

    #[test]
    fn rename_bell_sound_unknown_uuid_is_silent_noop() {
        let db = Database::open_in_memory().unwrap();
        db.rename_bell_sound("non-existent", "Bowl").unwrap();
        let updates: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "bell_sound_update")
            .collect();
        assert!(updates.is_empty());
    }

    #[test]
    fn delete_bell_sound_removes_the_row_and_emits_tombstone() {
        let db = Database::open_in_memory().unwrap();
        db.insert_bell_sound("Bowl", "/p/bowl.wav", false, "audio/wav", BellSoundCategory::General).unwrap();
        let uuid = db.list_bell_sounds().unwrap()[0].uuid.clone();
        db.delete_bell_sound(&uuid).unwrap();
        assert!(db.list_bell_sounds().unwrap().is_empty());

        let deletes: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "bell_sound_delete")
            .collect();
        assert_eq!(deletes.len(), 1);
        assert_eq!(deletes[0].1.target_id, uuid);
    }

    #[test]
    fn delete_bell_sound_unknown_uuid_is_silent_noop() {
        let db = Database::open_in_memory().unwrap();
        db.delete_bell_sound("non-existent").unwrap();
        let deletes: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "bell_sound_delete")
            .collect();
        assert!(deletes.is_empty());
    }

    fn synth_bell_sound_insert(uuid: &str, lamport_ts: i64, device: &str, name: &str) -> Event {
        Event {
            event_uuid: format!("bs-insert-{uuid}-{lamport_ts}-{device}"),
            lamport_ts,
            device_id: device.to_string(),
            kind: "bell_sound_insert".to_string(),
            target_id: uuid.to_string(),
            payload: serde_json::json!({
                "uuid": uuid,
                "name": name,
                "file_path": format!("/path/{name}.wav"),
                "is_bundled": false,
                "mime_type": "audio/wav",
                "created_iso": "2026-05-03T00:00:00Z",
            }).to_string(),
        }
    }

    fn synth_bell_sound_delete(uuid: &str, lamport_ts: i64, device: &str) -> Event {
        Event {
            event_uuid: format!("bs-del-{uuid}-{lamport_ts}-{device}"),
            lamport_ts,
            device_id: device.to_string(),
            kind: "bell_sound_delete".to_string(),
            target_id: uuid.to_string(),
            payload: serde_json::json!({ "uuid": uuid }).to_string(),
        }
    }

    #[test]
    fn apply_event_bell_sound_insert_creates_the_row() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_bell_sound_insert("bs-1", 5, "dev-A", "Bowl")).unwrap();
        let s = &db.list_bell_sounds().unwrap()[0];
        assert_eq!(s.uuid, "bs-1");
        assert_eq!(s.name, "Bowl");
        assert_eq!(s.file_path, "/path/Bowl.wav");
    }

    #[test]
    fn apply_event_bell_sound_delete_removes_the_row() {
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_bell_sound_insert("bs-1", 5, "dev-A", "Bowl")).unwrap();
        db.apply_event(&synth_bell_sound_delete("bs-1", 6, "dev-A")).unwrap();
        assert!(db.list_bell_sounds().unwrap().is_empty());
    }

    #[test]
    fn apply_event_bell_sound_tombstone_resists_lower_lamport_insert() {
        // Out-of-order arrival on a peer: delete (lamport 10) lands
        // first, then the earlier insert (lamport 5). Tombstone wins.
        let db = Database::open_in_memory().unwrap();
        db.apply_event(&synth_bell_sound_delete("bs-1", 10, "dev-A")).unwrap();
        db.apply_event(&synth_bell_sound_insert("bs-1", 5, "dev-A", "Bowl")).unwrap();
        assert!(db.list_bell_sounds().unwrap().is_empty());
    }

    // ── known_remote_sounds (B.6.1) ──────────────────────────────
    // Per-bell tracker mirroring known_remote_files. The push side
    // marks each bell uuid after a successful WebDAV PUT; the pull
    // side checks membership before issuing GETs.

    #[test]
    fn known_remote_sound_uuids_starts_empty() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.known_remote_sound_uuids().unwrap().is_empty());
    }

    #[test]
    fn record_known_remote_sound_adds_to_membership_set() {
        let db = Database::open_in_memory().unwrap();
        db.record_known_remote_sound("bs-1").unwrap();
        db.record_known_remote_sound("bs-2").unwrap();
        let known = db.known_remote_sound_uuids().unwrap();
        assert_eq!(known.len(), 2);
        assert!(known.contains("bs-1"));
        assert!(known.contains("bs-2"));
    }

    #[test]
    fn record_known_remote_sound_is_idempotent_on_repeat() {
        // INSERT OR IGNORE — calling twice with the same uuid keeps
        // the set at one entry, no error. Push retries can re-call
        // safely.
        let db = Database::open_in_memory().unwrap();
        db.record_known_remote_sound("bs-1").unwrap();
        db.record_known_remote_sound("bs-1").unwrap();
        assert_eq!(db.known_remote_sound_uuids().unwrap().len(), 1);
    }

    #[test]
    fn wipe_known_remote_sounds_clears_the_set() {
        let db = Database::open_in_memory().unwrap();
        db.record_known_remote_sound("bs-1").unwrap();
        db.record_known_remote_sound("bs-2").unwrap();
        db.wipe_known_remote_sounds().unwrap();
        assert!(db.known_remote_sound_uuids().unwrap().is_empty());
    }

    #[test]
    fn apply_event_bell_sound_replay_round_trip_across_peers() {
        let dev_a = Database::open_in_memory().unwrap();
        dev_a.insert_bell_sound("Bowl", "/p/bowl.wav", true, "audio/wav", BellSoundCategory::General).unwrap();
        let uuid = dev_a.list_bell_sounds().unwrap()[0].uuid.clone();
        dev_a.rename_bell_sound(&uuid, "Singing Bowl").unwrap();

        let events: Vec<Event> = dev_a.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind.starts_with("bell_sound_"))
            .map(|(_, e)| e)
            .collect();

        let dev_b = Database::open_in_memory().unwrap();
        dev_b.replay_events(&events).unwrap();
        let sounds = dev_b.list_bell_sounds().unwrap();
        assert_eq!(sounds.len(), 1);
        assert_eq!(sounds[0].uuid, uuid);
        assert_eq!(sounds[0].name, "Singing Bowl");
        assert!(sounds[0].is_bundled);
        assert_eq!(sounds[0].file_path, "/p/bowl.wav");
    }

    #[test]
    fn apply_event_interval_bell_replay_round_trip_across_peers() {
        // Device A creates + updates; device B replays the event log and
        // arrives at exactly the same row state.
        let dev_a = Database::open_in_memory().unwrap();
        dev_a.insert_interval_bell(IntervalBellKind::Interval, 9, 30, "bell", BUNDLED_PATTERN_PULSE_UUID, SignalMode::Sound).unwrap();
        let uuid = dev_a.list_interval_bells().unwrap()[0].uuid.clone();
        dev_a.update_interval_bell(
            &uuid, IntervalBellKind::FixedFromStart, 10, 0, "gong",
            BUNDLED_PATTERN_PULSE_UUID, SignalMode::Sound, true,
        ).unwrap();

        let events: Vec<Event> = dev_a.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind.starts_with("interval_bell_"))
            .map(|(_, e)| e)
            .collect();

        let dev_b = Database::open_in_memory().unwrap();
        dev_b.replay_events(&events).unwrap();
        let bells_b = dev_b.list_interval_bells().unwrap();
        assert_eq!(bells_b.len(), 1);
        let b = &bells_b[0];
        assert_eq!(b.uuid, uuid);
        assert_eq!(b.kind, IntervalBellKind::FixedFromStart);
        assert_eq!(b.minutes, 10);
        assert_eq!(b.sound, "gong");
    }

    // ── Presets — schema, CRUD, events ────────────────────────────────

    fn insert_basic_preset(db: &Database, name: &str, mode: SessionMode) -> i64 {
        db.insert_preset(
            name,
            mode,
            false,
            r#"{"placeholder":true}"#,
        ).unwrap()
    }

    #[test]
    fn insert_preset_round_trips_through_list() {
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_preset(
            "Sitting",
            SessionMode::Timer,
            true,
            r#"{"duration_secs":900}"#,
        ).unwrap();

        let presets = db.list_presets().unwrap();
        assert_eq!(presets.len(), 1);
        let p = &presets[0];
        assert_eq!(p.id, id);
        assert!(!p.uuid.is_empty(), "fresh insert mints a uuid");
        assert_eq!(p.name, "Sitting");
        assert_eq!(p.mode, SessionMode::Timer);
        assert!(p.is_starred);
        assert_eq!(p.config_json, r#"{"duration_secs":900}"#);
        assert!(!p.created_iso.is_empty());
        assert_eq!(p.created_iso, p.updated_iso,
            "fresh insert sets updated_iso = created_iso");
    }

    #[test]
    fn insert_preset_guided_mode_round_trips() {
        let db = Database::open_in_memory().unwrap();
        let id = db.insert_preset(
            "Headspace Track 1",
            SessionMode::Guided,
            true,
            r#"{"guided_file_uuid":"x"}"#,
        ).unwrap();
        let presets = db.list_presets().unwrap();
        assert_eq!(presets.len(), 1);
        assert_eq!(presets[0].id, id);
        assert_eq!(presets[0].mode, SessionMode::Guided);
    }

    #[test]
    fn insert_preset_with_uuid_uses_supplied_uuid() {
        let db = Database::open_in_memory().unwrap();
        let _id = db.insert_preset_with_uuid(
            "abc-123",
            "Sitting",
            SessionMode::Timer,
            false,
            r#"{}"#,
        ).unwrap();
        let presets = db.list_presets().unwrap();
        assert_eq!(presets[0].uuid, "abc-123");
    }

    #[test]
    fn insert_preset_with_existing_uuid_is_silent_noop() {
        let db = Database::open_in_memory().unwrap();
        let r1 = db.insert_preset_with_uuid(
            "u-1", "Sitting", SessionMode::Timer, false, r#"{}"#,
        ).unwrap();
        let r2 = db.insert_preset_with_uuid(
            "u-1", "Different Name", SessionMode::BoxBreath, true, r#"{"x":1}"#,
        ).unwrap();
        assert_eq!(r1, r2, "second insert returns existing rowid");
        assert_eq!(db.count_presets().unwrap(), 1);
        // Original values stand — second insert is a pure no-op.
        let p = &db.list_presets().unwrap()[0];
        assert_eq!(p.name, "Sitting");
        assert_eq!(p.mode, SessionMode::Timer);
        assert!(!p.is_starred);
    }

    #[test]
    fn insert_preset_duplicate_name_returns_duplicate_preset() {
        let db = Database::open_in_memory().unwrap();
        db.insert_preset("Sitting", SessionMode::Timer, false, r#"{}"#).unwrap();
        let r = db.insert_preset(
            "sitting",  // case-insensitive collision
            SessionMode::BoxBreath,
            false,
            r#"{}"#,
        );
        assert!(
            matches!(r, Err(DbError::DuplicatePreset(ref n)) if n == "sitting"),
            "expected DuplicatePreset, got {r:?}",
        );
        assert_eq!(db.count_presets().unwrap(), 1, "row count unchanged");
    }

    #[test]
    fn insert_preset_emits_a_preset_insert_event() {
        let db = Database::open_in_memory().unwrap();
        db.insert_preset("Sitting", SessionMode::Timer, true, r#"{"dur":900}"#).unwrap();
        let events = db.pending_events().unwrap();
        let insert: Vec<_> = events.iter()
            .filter(|(_, e)| e.kind == "preset_insert")
            .collect();
        assert_eq!(insert.len(), 1);
        let payload: serde_json::Value =
            serde_json::from_str(&insert[0].1.payload).unwrap();
        assert_eq!(payload["name"], "Sitting");
        assert_eq!(payload["mode"], "timer");
        assert_eq!(payload["is_starred"], true);
        assert_eq!(payload["config_json"], r#"{"dur":900}"#);
    }

    #[test]
    fn list_presets_orders_by_mode_then_created_iso() {
        let db = Database::open_in_memory().unwrap();
        // Interleave insert order and modes; verify the SQL ORDER BY
        // groups by mode and orders by created_iso within each group.
        insert_basic_preset(&db, "BB-1", SessionMode::BoxBreath);
        std::thread::sleep(std::time::Duration::from_millis(2));
        insert_basic_preset(&db, "T-1", SessionMode::Timer);
        std::thread::sleep(std::time::Duration::from_millis(2));
        insert_basic_preset(&db, "BB-2", SessionMode::BoxBreath);
        std::thread::sleep(std::time::Duration::from_millis(2));
        insert_basic_preset(&db, "T-2", SessionMode::Timer);

        let names: Vec<String> = db.list_presets().unwrap()
            .into_iter().map(|p| p.name).collect();
        // Mode ordering (timer < box_breath alphabetically as DB string —
        // 'box_breath' < 'timer' actually). Verify whichever way SQL sees it.
        // What matters is that within each mode block, created_iso is ASC.
        let bb_pos: Vec<usize> = names.iter().enumerate()
            .filter(|(_, n)| n.starts_with("BB-"))
            .map(|(i, _)| i).collect();
        let t_pos: Vec<usize> = names.iter().enumerate()
            .filter(|(_, n)| n.starts_with("T-"))
            .map(|(i, _)| i).collect();
        assert_eq!(bb_pos.len(), 2);
        assert_eq!(t_pos.len(), 2);
        // Within each mode, ordering follows insert order.
        assert!(bb_pos[0] < bb_pos[1]);
        assert!(t_pos[0] < t_pos[1]);
        assert_eq!(names[bb_pos[0]], "BB-1");
        assert_eq!(names[bb_pos[1]], "BB-2");
        assert_eq!(names[t_pos[0]], "T-1");
        assert_eq!(names[t_pos[1]], "T-2");
    }

    #[test]
    fn list_presets_for_mode_filters_correctly() {
        let db = Database::open_in_memory().unwrap();
        insert_basic_preset(&db, "T-1", SessionMode::Timer);
        insert_basic_preset(&db, "BB-1", SessionMode::BoxBreath);
        insert_basic_preset(&db, "T-2", SessionMode::Timer);

        let timers: Vec<String> = db.list_presets_for_mode(SessionMode::Timer)
            .unwrap().into_iter().map(|p| p.name).collect();
        assert_eq!(timers, vec!["T-1", "T-2"]);

        let breaths: Vec<String> = db.list_presets_for_mode(SessionMode::BoxBreath)
            .unwrap().into_iter().map(|p| p.name).collect();
        assert_eq!(breaths, vec!["BB-1"]);
    }

    #[test]
    fn list_starred_presets_for_mode_returns_only_starred_in_mode() {
        let db = Database::open_in_memory().unwrap();
        // Mix: Timer starred + unstarred; BoxBreath starred + unstarred.
        db.insert_preset("T-star", SessionMode::Timer, true, r#"{}"#).unwrap();
        db.insert_preset("T-no", SessionMode::Timer, false, r#"{}"#).unwrap();
        db.insert_preset("BB-star", SessionMode::BoxBreath, true, r#"{}"#).unwrap();
        db.insert_preset("BB-no", SessionMode::BoxBreath, false, r#"{}"#).unwrap();

        let timer_starred: Vec<String> =
            db.list_starred_presets_for_mode(SessionMode::Timer).unwrap()
                .into_iter().map(|p| p.name).collect();
        assert_eq!(timer_starred, vec!["T-star"]);

        let breath_starred: Vec<String> =
            db.list_starred_presets_for_mode(SessionMode::BoxBreath).unwrap()
                .into_iter().map(|p| p.name).collect();
        assert_eq!(breath_starred, vec!["BB-star"]);
    }

    #[test]
    fn is_preset_name_taken_excludes_self_uuid() {
        let db = Database::open_in_memory().unwrap();
        db.insert_preset_with_uuid(
            "u-1", "Sitting", SessionMode::Timer, false, r#"{}"#,
        ).unwrap();
        db.insert_preset_with_uuid(
            "u-2", "Walking", SessionMode::Timer, false, r#"{}"#,
        ).unwrap();
        // Collision with another preset's name.
        assert!(db.is_preset_name_taken("Walking", "u-1").unwrap());
        // Renaming to own current name — case-insensitive — is allowed.
        assert!(!db.is_preset_name_taken("sitting", "u-1").unwrap());
        // Brand-new name is fine.
        assert!(!db.is_preset_name_taken("Sleeping", "u-1").unwrap());
    }

    #[test]
    fn find_preset_by_uuid_returns_some_when_present() {
        let db = Database::open_in_memory().unwrap();
        db.insert_preset_with_uuid(
            "u-1", "Sitting", SessionMode::Timer, true, r#"{"x":1}"#,
        ).unwrap();
        let p = db.find_preset_by_uuid("u-1").unwrap().unwrap();
        assert_eq!(p.name, "Sitting");
        assert!(p.is_starred);
        assert_eq!(p.config_json, r#"{"x":1}"#);
    }

    #[test]
    fn find_preset_by_uuid_returns_none_when_absent() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.find_preset_by_uuid("missing").unwrap().is_none());
    }

    #[test]
    fn update_preset_name_changes_the_name_and_bumps_updated_iso() {
        let db = Database::open_in_memory().unwrap();
        db.insert_preset_with_uuid(
            "u-1", "Sitting", SessionMode::Timer, false, r#"{}"#,
        ).unwrap();
        let before = db.find_preset_by_uuid("u-1").unwrap().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        db.update_preset_name("u-1", "Morning Sit").unwrap();
        let after = db.find_preset_by_uuid("u-1").unwrap().unwrap();
        assert_eq!(after.name, "Morning Sit");
        assert_eq!(after.created_iso, before.created_iso);
        assert!(after.updated_iso > before.updated_iso);
    }

    #[test]
    fn update_preset_name_duplicate_returns_error_and_rolls_back() {
        let db = Database::open_in_memory().unwrap();
        db.insert_preset_with_uuid(
            "u-1", "Sitting", SessionMode::Timer, false, r#"{}"#,
        ).unwrap();
        db.insert_preset_with_uuid(
            "u-2", "Walking", SessionMode::Timer, false, r#"{}"#,
        ).unwrap();
        let result = db.update_preset_name("u-1", "Walking");
        assert!(matches!(result, Err(DbError::DuplicatePreset(_))));
        // u-1 still has its original name.
        let p = db.find_preset_by_uuid("u-1").unwrap().unwrap();
        assert_eq!(p.name, "Sitting");
    }

    #[test]
    fn update_preset_name_unknown_uuid_is_silent_noop() {
        let db = Database::open_in_memory().unwrap();
        db.update_preset_name("missing", "Foo").unwrap();
        let events: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "preset_update")
            .collect();
        assert!(events.is_empty(), "unknown uuid emits no event");
    }

    #[test]
    fn update_preset_config_replaces_the_config_blob() {
        let db = Database::open_in_memory().unwrap();
        db.insert_preset_with_uuid(
            "u-1", "Sitting", SessionMode::Timer, false, r#"{"old":1}"#,
        ).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        db.update_preset_config("u-1", r#"{"new":2}"#).unwrap();
        let p = db.find_preset_by_uuid("u-1").unwrap().unwrap();
        assert_eq!(p.config_json, r#"{"new":2}"#);
    }

    #[test]
    fn update_preset_config_emits_one_preset_update_event() {
        let db = Database::open_in_memory().unwrap();
        db.insert_preset_with_uuid(
            "u-1", "Sitting", SessionMode::Timer, false, r#"{}"#,
        ).unwrap();
        db.update_preset_config("u-1", r#"{"x":1}"#).unwrap();
        let updates: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "preset_update")
            .collect();
        assert_eq!(updates.len(), 1);
    }

    #[test]
    fn update_preset_starred_toggles_the_flag() {
        let db = Database::open_in_memory().unwrap();
        db.insert_preset_with_uuid(
            "u-1", "Sitting", SessionMode::Timer, false, r#"{}"#,
        ).unwrap();
        db.update_preset_starred("u-1", true).unwrap();
        assert!(db.find_preset_by_uuid("u-1").unwrap().unwrap().is_starred);
        db.update_preset_starred("u-1", false).unwrap();
        assert!(!db.find_preset_by_uuid("u-1").unwrap().unwrap().is_starred);
    }

    #[test]
    fn delete_preset_removes_the_row_and_emits_tombstone() {
        let db = Database::open_in_memory().unwrap();
        db.insert_preset_with_uuid(
            "u-1", "Sitting", SessionMode::Timer, false, r#"{}"#,
        ).unwrap();
        db.delete_preset("u-1").unwrap();
        assert!(db.find_preset_by_uuid("u-1").unwrap().is_none());
        let tombstones: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "preset_delete")
            .collect();
        assert_eq!(tombstones.len(), 1);
    }

    #[test]
    fn delete_preset_unknown_uuid_emits_no_event() {
        let db = Database::open_in_memory().unwrap();
        db.delete_preset("never-existed").unwrap();
        let tombstones: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "preset_delete")
            .collect();
        assert!(tombstones.is_empty());
    }

    // ── Presets — sync replay (apply_event / replay_events) ───────────

    #[test]
    fn apply_event_preset_insert_creates_the_row_on_a_fresh_peer() {
        let peer = Database::open_in_memory().unwrap();
        let payload = serde_json::json!({
            "uuid": "u-1",
            "name": "Sitting",
            "mode": "timer",
            "is_starred": true,
            "config_json": "{\"dur\":900}",
            "created_iso": "2026-05-04T10:00:00Z",
            "updated_iso": "2026-05-04T10:00:00Z",
        });
        peer.apply_event(&synth_event("preset_insert", "u-1", 1, "dev-a", payload))
            .unwrap();
        let p = peer.find_preset_by_uuid("u-1").unwrap()
            .expect("row materialised on peer");
        assert_eq!(p.name, "Sitting");
        assert_eq!(p.mode, SessionMode::Timer);
        assert!(p.is_starred);
        assert_eq!(p.config_json, r#"{"dur":900}"#);
    }

    #[test]
    fn apply_event_preset_update_overwrites_fields_after_insert() {
        let peer = Database::open_in_memory().unwrap();
        peer.apply_event(&synth_event("preset_insert", "u-1", 1, "dev-a",
            serde_json::json!({
                "uuid": "u-1", "name": "Sitting", "mode": "timer",
                "is_starred": false, "config_json": "{\"v\":1}",
                "created_iso": "2026-05-04T10:00:00Z",
                "updated_iso": "2026-05-04T10:00:00Z",
            }))).unwrap();
        peer.apply_event(&synth_event("preset_update", "u-1", 5, "dev-a",
            serde_json::json!({
                "uuid": "u-1", "name": "Morning Sit", "mode": "timer",
                "is_starred": true, "config_json": "{\"v\":2}",
                "created_iso": "2026-05-04T10:00:00Z",
                "updated_iso": "2026-05-04T10:05:00Z",
            }))).unwrap();
        let p = peer.find_preset_by_uuid("u-1").unwrap().unwrap();
        assert_eq!(p.name, "Morning Sit");
        assert!(p.is_starred);
        assert_eq!(p.config_json, r#"{"v":2}"#);
    }

    #[test]
    fn apply_event_preset_delete_removes_the_row() {
        let peer = Database::open_in_memory().unwrap();
        peer.apply_event(&synth_event("preset_insert", "u-1", 1, "dev-a",
            serde_json::json!({
                "uuid": "u-1", "name": "Sitting", "mode": "timer",
                "is_starred": false, "config_json": "{}",
                "created_iso": "x", "updated_iso": "x",
            }))).unwrap();
        peer.apply_event(&synth_event("preset_delete", "u-1", 5, "dev-a",
            serde_json::json!({"uuid": "u-1"}))).unwrap();
        assert!(peer.find_preset_by_uuid("u-1").unwrap().is_none());
    }

    #[test]
    fn apply_event_preset_tombstone_resists_lower_lamport_insert() {
        // Out-of-order delivery: delete arrives first (ts=10), then a
        // late insert event with ts=5. Tombstone wins, row stays absent.
        let peer = Database::open_in_memory().unwrap();
        peer.apply_event(&synth_event("preset_delete", "u-1", 10, "dev-a",
            serde_json::json!({"uuid": "u-1"}))).unwrap();
        peer.apply_event(&synth_event("preset_insert", "u-1", 5, "dev-a",
            serde_json::json!({
                "uuid": "u-1", "name": "Sitting", "mode": "timer",
                "is_starred": false, "config_json": "{}",
                "created_iso": "x", "updated_iso": "x",
            }))).unwrap();
        assert!(peer.find_preset_by_uuid("u-1").unwrap().is_none(),
            "tombstone with higher ts wins over later-applied lower-ts insert");
    }

    #[test]
    fn replay_events_round_trips_a_preset_through_create_rename_delete() {
        // Full lifecycle on device A, replayed on device B from the
        // event log alone.
        let dev_a = Database::open_in_memory().unwrap();
        dev_a.insert_preset_with_uuid(
            "u-1", "Sitting", SessionMode::Timer, true, r#"{"dur":900}"#,
        ).unwrap();
        dev_a.update_preset_name("u-1", "Morning Sit").unwrap();
        dev_a.update_preset_starred("u-1", false).unwrap();

        let events: Vec<Event> = dev_a.pending_events().unwrap()
            .into_iter().map(|(_, e)| e).collect();

        let dev_b = Database::open_in_memory().unwrap();
        dev_b.replay_events(&events).unwrap();
        let p = dev_b.find_preset_by_uuid("u-1").unwrap()
            .expect("device B materialised the preset from events alone");
        assert_eq!(p.name, "Morning Sit");
        assert!(!p.is_starred);
        assert_eq!(p.config_json, r#"{"dur":900}"#);
    }

    #[test]
    fn replay_events_with_delete_at_the_end_leaves_the_row_absent() {
        let dev_a = Database::open_in_memory().unwrap();
        dev_a.insert_preset_with_uuid(
            "u-1", "Sitting", SessionMode::Timer, false, r#"{}"#,
        ).unwrap();
        dev_a.delete_preset("u-1").unwrap();
        let events: Vec<Event> = dev_a.pending_events().unwrap()
            .into_iter().map(|(_, e)| e).collect();

        let dev_b = Database::open_in_memory().unwrap();
        dev_b.replay_events(&events).unwrap();
        assert!(dev_b.find_preset_by_uuid("u-1").unwrap().is_none());
    }

    // ── GuidedFiles — basic CRUD ─────────────────────────────────────

    #[test]
    fn list_guided_files_is_empty_on_a_fresh_database() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.list_guided_files().unwrap().is_empty());
    }

    #[test]
    fn insert_guided_file_with_uuid_round_trips_through_list() {
        let db = Database::open_in_memory().unwrap();
        db.insert_guided_file_with_uuid(
            "gf-1", "Body Scan", "guided/gf-1.ogg", 1200, true,
        ).unwrap();
        let rows = db.list_guided_files().unwrap();
        assert_eq!(rows.len(), 1);
        let r = &rows[0];
        assert_eq!(r.uuid, "gf-1");
        assert_eq!(r.name, "Body Scan");
        assert_eq!(r.file_path, "guided/gf-1.ogg");
        assert_eq!(r.duration_secs, 1200);
        assert!(r.is_starred);
        assert!(!r.created_iso.is_empty());
        assert_eq!(r.created_iso, r.updated_iso,
            "fresh insert: updated_iso == created_iso");
    }

    #[test]
    fn insert_guided_file_with_existing_uuid_is_silent_noop() {
        // Sync replay can land the same insert twice; the second call
        // returns the existing rowid without changing the row or
        // emitting a second event.
        let db = Database::open_in_memory().unwrap();
        let id1 = db.insert_guided_file_with_uuid(
            "gf-1", "Body Scan", "guided/gf-1.ogg", 1200, false,
        ).unwrap();
        let id2 = db.insert_guided_file_with_uuid(
            "gf-1", "Different Name", "guided/different.ogg", 999, true,
        ).unwrap();
        assert_eq!(id1, id2);
        let rows = db.list_guided_files().unwrap();
        assert_eq!(rows.len(), 1);
        // Original values preserved — no overwrite.
        assert_eq!(rows[0].name, "Body Scan");
        assert_eq!(rows[0].duration_secs, 1200);
    }

    #[test]
    fn insert_guided_file_with_duplicate_name_returns_duplicate_error() {
        // The schema's UNIQUE NOCASE on `name` blocks two rows with
        // the same display name even under different uuids — so the
        // chooser can't end up showing two visually identical entries.
        let db = Database::open_in_memory().unwrap();
        db.insert_guided_file_with_uuid(
            "gf-1", "Body Scan", "guided/gf-1.ogg", 1200, false,
        ).unwrap();
        match db.insert_guided_file_with_uuid(
            "gf-2", "BODY SCAN", "guided/gf-2.ogg", 800, false,
        ) {
            Err(DbError::DuplicateGuidedFile(name)) => assert_eq!(name, "BODY SCAN"),
            other => panic!("expected DuplicateGuidedFile, got {other:?}"),
        }
    }

    #[test]
    fn list_guided_files_orders_by_created_iso() {
        // Stable creation-order makes the home-screen list show files
        // in the order the user imported them (no surprise reshuffles
        // when toggling stars).
        let db = Database::open_in_memory().unwrap();
        db.insert_guided_file_with_uuid(
            "gf-1", "First", "guided/1.ogg", 600, true,
        ).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        db.insert_guided_file_with_uuid(
            "gf-2", "Second", "guided/2.ogg", 1200, true,
        ).unwrap();
        let rows = db.list_guided_files().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, "First");
        assert_eq!(rows[1].name, "Second");
    }

    #[test]
    fn delete_guided_file_removes_the_row() {
        let db = Database::open_in_memory().unwrap();
        db.insert_guided_file_with_uuid(
            "gf-1", "Body Scan", "guided/gf-1.ogg", 1200, true,
        ).unwrap();
        db.delete_guided_file("gf-1").unwrap();
        assert!(db.list_guided_files().unwrap().is_empty());
    }

    #[test]
    fn delete_guided_file_unknown_uuid_is_silent_noop() {
        let db = Database::open_in_memory().unwrap();
        db.delete_guided_file("never-existed").unwrap();
        assert!(db.list_guided_files().unwrap().is_empty());
    }

    // ── GuidedFiles — updates (rename / star toggle) ─────────────────

    #[test]
    fn rename_guided_file_changes_name_and_bumps_updated_iso() {
        let db = Database::open_in_memory().unwrap();
        db.insert_guided_file_with_uuid(
            "gf-1", "Old Name", "guided/gf-1.ogg", 600, false,
        ).unwrap();
        let before = db.list_guided_files().unwrap()[0].clone();
        // Sleep so the iso timestamp moves forward measurably.
        std::thread::sleep(std::time::Duration::from_millis(10));
        db.rename_guided_file("gf-1", "New Name").unwrap();
        let after = db.list_guided_files().unwrap()[0].clone();
        assert_eq!(after.name, "New Name");
        assert_eq!(after.created_iso, before.created_iso,
            "created_iso must not move on rename");
        assert!(after.updated_iso > before.updated_iso,
            "updated_iso must bump forward on rename");
    }

    #[test]
    fn rename_guided_file_unknown_uuid_is_silent_noop() {
        let db = Database::open_in_memory().unwrap();
        db.rename_guided_file("never-existed", "anything").unwrap();
        let renames: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "guided_file_update")
            .collect();
        assert!(renames.is_empty(),
            "no event for an unknown uuid (peers would otherwise update a row they don't have)");
    }

    #[test]
    fn rename_guided_file_to_existing_name_returns_duplicate_error() {
        let db = Database::open_in_memory().unwrap();
        db.insert_guided_file_with_uuid(
            "gf-1", "Body Scan", "guided/gf-1.ogg", 600, false,
        ).unwrap();
        db.insert_guided_file_with_uuid(
            "gf-2", "Loving Kindness", "guided/gf-2.ogg", 900, false,
        ).unwrap();
        match db.rename_guided_file("gf-2", "Body Scan") {
            Err(DbError::DuplicateGuidedFile(name)) => assert_eq!(name, "Body Scan"),
            other => panic!("expected DuplicateGuidedFile, got {other:?}"),
        }
    }

    #[test]
    fn rename_guided_file_to_same_name_with_different_case_is_accepted() {
        // The UNIQUE NOCASE check excludes the row being renamed
        // (matches the preset rename semantics).
        let db = Database::open_in_memory().unwrap();
        db.insert_guided_file_with_uuid(
            "gf-1", "body scan", "guided/gf-1.ogg", 600, false,
        ).unwrap();
        db.rename_guided_file("gf-1", "Body Scan").unwrap();
        assert_eq!(db.list_guided_files().unwrap()[0].name, "Body Scan");
    }

    #[test]
    fn set_guided_file_starred_toggles_and_emits_event() {
        let db = Database::open_in_memory().unwrap();
        db.insert_guided_file_with_uuid(
            "gf-1", "Body Scan", "guided/gf-1.ogg", 600, false,
        ).unwrap();
        db.set_guided_file_starred("gf-1", true).unwrap();
        assert!(db.list_guided_files().unwrap()[0].is_starred);
        db.set_guided_file_starred("gf-1", false).unwrap();
        assert!(!db.list_guided_files().unwrap()[0].is_starred);
        let updates: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "guided_file_update")
            .collect();
        assert_eq!(updates.len(), 2,
            "one event per star flip");
    }

    #[test]
    fn set_guided_file_starred_unknown_uuid_is_silent_noop() {
        let db = Database::open_in_memory().unwrap();
        db.set_guided_file_starred("never-existed", true).unwrap();
        let events: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind.starts_with("guided_file"))
            .collect();
        assert!(events.is_empty());
    }

    // ── GuidedFiles — lookups ────────────────────────────────────────

    #[test]
    fn find_guided_file_by_uuid_returns_some_when_present() {
        let db = Database::open_in_memory().unwrap();
        db.insert_guided_file_with_uuid(
            "gf-1", "Body Scan", "guided/gf-1.ogg", 600, true,
        ).unwrap();
        let got = db.find_guided_file_by_uuid("gf-1").unwrap();
        assert!(got.is_some());
        let r = got.unwrap();
        assert_eq!(r.name, "Body Scan");
        assert_eq!(r.duration_secs, 600);
        assert!(r.is_starred);
    }

    #[test]
    fn find_guided_file_by_uuid_returns_none_when_missing() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.find_guided_file_by_uuid("never-existed").unwrap().is_none());
    }

    #[test]
    fn is_guided_file_name_taken_matches_case_insensitively() {
        let db = Database::open_in_memory().unwrap();
        db.insert_guided_file_with_uuid(
            "gf-1", "Body Scan", "guided/gf-1.ogg", 600, false,
        ).unwrap();
        assert!(db.is_guided_file_name_taken("Body Scan", "").unwrap());
        assert!(db.is_guided_file_name_taken("body scan", "").unwrap());
        assert!(db.is_guided_file_name_taken("BODY SCAN", "").unwrap());
        assert!(!db.is_guided_file_name_taken("Different Name", "").unwrap());
    }

    #[test]
    fn is_guided_file_name_taken_excludes_the_row_being_renamed() {
        // The rename dialog uses this to live-validate input. The
        // user's own current name must not register as "taken" against
        // their own uuid, otherwise renaming "Body Scan" → "body scan"
        // would falsely report a collision.
        let db = Database::open_in_memory().unwrap();
        db.insert_guided_file_with_uuid(
            "gf-1", "Body Scan", "guided/gf-1.ogg", 600, false,
        ).unwrap();
        assert!(!db.is_guided_file_name_taken("Body Scan", "gf-1").unwrap());
        assert!(!db.is_guided_file_name_taken("body scan", "gf-1").unwrap());
        // But same name under a different uuid still flags as taken.
        assert!(db.is_guided_file_name_taken("Body Scan", "gf-2").unwrap());
    }

    // ── GuidedFiles — sync replay (apply_event / replay_events) ──────

    #[test]
    fn apply_event_guided_file_insert_creates_the_row_on_a_fresh_peer() {
        let peer = Database::open_in_memory().unwrap();
        let payload = serde_json::json!({
            "uuid": "gf-1",
            "name": "Body Scan",
            "file_path": "guided/gf-1.ogg",
            "duration_secs": 1200,
            "is_starred": true,
            "created_iso": "2026-05-05T20:00:00Z",
            "updated_iso": "2026-05-05T20:00:00Z",
        });
        let event = synth_event(
            "guided_file_insert", "gf-1", 5, DEVICE_A, payload,
        );
        peer.apply_event(&event).unwrap();
        let rows = peer.list_guided_files().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Body Scan");
        assert_eq!(rows[0].duration_secs, 1200);
        assert!(rows[0].is_starred);
    }

    #[test]
    fn apply_event_guided_file_update_overwrites_earlier_state() {
        let peer = Database::open_in_memory().unwrap();
        let insert_payload = serde_json::json!({
            "uuid": "gf-1",
            "name": "Old Name",
            "file_path": "guided/gf-1.ogg",
            "duration_secs": 1200,
            "is_starred": false,
            "created_iso": "2026-05-05T20:00:00Z",
            "updated_iso": "2026-05-05T20:00:00Z",
        });
        peer.apply_event(&synth_event(
            "guided_file_insert", "gf-1", 5, DEVICE_A, insert_payload,
        )).unwrap();
        let update_payload = serde_json::json!({
            "uuid": "gf-1",
            "name": "New Name",
            "file_path": "guided/gf-1.ogg",
            "duration_secs": 1200,
            "is_starred": true,
            "created_iso": "2026-05-05T20:00:00Z",
            "updated_iso": "2026-05-05T20:05:00Z",
        });
        peer.apply_event(&synth_event(
            "guided_file_update", "gf-1", 7, DEVICE_A, update_payload,
        )).unwrap();
        let rows = peer.list_guided_files().unwrap();
        assert_eq!(rows[0].name, "New Name");
        assert!(rows[0].is_starred);
    }

    #[test]
    fn apply_event_guided_file_delete_removes_the_row() {
        let peer = Database::open_in_memory().unwrap();
        let insert_payload = serde_json::json!({
            "uuid": "gf-1",
            "name": "Body Scan",
            "file_path": "guided/gf-1.ogg",
            "duration_secs": 1200,
            "is_starred": false,
            "created_iso": "2026-05-05T20:00:00Z",
            "updated_iso": "2026-05-05T20:00:00Z",
        });
        peer.apply_event(&synth_event(
            "guided_file_insert", "gf-1", 5, DEVICE_A, insert_payload,
        )).unwrap();
        peer.apply_event(&synth_event(
            "guided_file_delete", "gf-1", 7, DEVICE_A,
            serde_json::json!({ "uuid": "gf-1" }),
        )).unwrap();
        assert!(peer.list_guided_files().unwrap().is_empty());
    }

    #[test]
    fn apply_event_guided_file_tombstone_resists_lower_lamport_insert() {
        // Out-of-order arrival: delete event lands first (lamport 10),
        // then a stale insert (lamport 5). Tombstone wins.
        let peer = Database::open_in_memory().unwrap();
        peer.apply_event(&synth_event(
            "guided_file_delete", "gf-1", 10, DEVICE_A,
            serde_json::json!({ "uuid": "gf-1" }),
        )).unwrap();
        let insert_payload = serde_json::json!({
            "uuid": "gf-1",
            "name": "Body Scan",
            "file_path": "guided/gf-1.ogg",
            "duration_secs": 1200,
            "is_starred": false,
            "created_iso": "2026-05-05T20:00:00Z",
            "updated_iso": "2026-05-05T20:00:00Z",
        });
        peer.apply_event(&synth_event(
            "guided_file_insert", "gf-1", 5, DEVICE_A, insert_payload,
        )).unwrap();
        assert!(peer.list_guided_files().unwrap().is_empty(),
            "tombstone with lamport 10 must beat insert with lamport 5");
    }

    #[test]
    fn replay_guided_file_events_round_trips_to_a_fresh_peer() {
        let dev_a = Database::open_in_memory().unwrap();
        dev_a.insert_guided_file_with_uuid(
            "gf-1", "Body Scan", "guided/gf-1.ogg", 1200, true,
        ).unwrap();
        dev_a.set_guided_file_starred("gf-1", false).unwrap();
        let events: Vec<Event> = dev_a.pending_events().unwrap()
            .into_iter().map(|(_, e)| e).collect();

        let dev_b = Database::open_in_memory().unwrap();
        dev_b.replay_events(&events).unwrap();
        let rows = dev_b.list_guided_files().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Body Scan");
        assert!(!rows[0].is_starred,
            "the more recent star=false update must win on the replay");
    }

    // ── VibrationPatterns ─────────────────────────────────────────────

    #[test]
    fn chart_kind_round_trips_through_db_str() {
        for k in [ChartKind::Line, ChartKind::Bar] {
            assert_eq!(ChartKind::from_db_str(k.as_db_str()), Some(k));
        }
        assert_eq!(ChartKind::from_db_str("typo"), None);
    }

    #[test]
    fn list_vibration_patterns_is_empty_on_a_fresh_database() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.list_vibration_patterns().unwrap().is_empty());
    }

    #[test]
    fn list_vibration_patterns_returns_custom_rows_before_bundled() {
        // Chooser UX: a freshly-saved custom pattern lives at the top
        // of the list, so the user can see + pick it without scrolling
        // past the curated bundled set every time. Mirrors the
        // bell_sounds ordering rule.
        let db = Database::open_in_memory().unwrap();
        // Seed-style insert (is_bundled = true) lands first…
        db.insert_vibration_pattern_with_uuid(
            "vp-bundled", "Pulse", 400, &[0.0, 1.0, 0.0],
            ChartKind::Line, true,
        ).unwrap();
        // …but a later custom row should sort above it.
        db.insert_vibration_pattern_with_uuid(
            "vp-custom", "My Wave", 1000, &[0.0, 0.5, 0.0],
            ChartKind::Line, false,
        ).unwrap();
        let rows = db.list_vibration_patterns().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].uuid, "vp-custom",
            "custom rows must precede bundled in the chooser order");
        assert_eq!(rows[1].uuid, "vp-bundled");
    }

    #[test]
    fn insert_vibration_pattern_with_uuid_round_trips_through_list() {
        let db = Database::open_in_memory().unwrap();
        db.insert_vibration_pattern_with_uuid(
            "vp-1", "Heartbeat", 1500, &[0.0, 0.6, 0.0, 0.0, 1.0, 0.0],
            ChartKind::Line, false,
        ).unwrap();
        let rows = db.list_vibration_patterns().unwrap();
        assert_eq!(rows.len(), 1);
        let r = &rows[0];
        assert_eq!(r.uuid, "vp-1");
        assert_eq!(r.name, "Heartbeat");
        assert_eq!(r.duration_ms, 1500);
        assert_eq!(r.intensities, vec![0.0, 0.6, 0.0, 0.0, 1.0, 0.0]);
        assert_eq!(r.chart_kind, ChartKind::Line);
        assert!(!r.is_bundled);
        assert!(!r.created_iso.is_empty());
        assert_eq!(r.created_iso, r.updated_iso,
            "fresh insert: updated_iso == created_iso");
    }

    #[test]
    fn insert_vibration_pattern_with_existing_uuid_is_silent_noop() {
        // Sync replay can land the same insert twice; the second call
        // returns the existing rowid without changing the row or
        // emitting a second event. Mirrors guided_files / bell_sounds.
        let db = Database::open_in_memory().unwrap();
        let id1 = db.insert_vibration_pattern_with_uuid(
            "vp-1", "Pulse", 400, &[0.0, 1.0, 0.0],
            ChartKind::Line, true,
        ).unwrap();
        let id2 = db.insert_vibration_pattern_with_uuid(
            "vp-1", "Different Name", 999, &[0.5],
            ChartKind::Bar, false,
        ).unwrap();
        assert_eq!(id1, id2);
        let rows = db.list_vibration_patterns().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Pulse");
        assert_eq!(rows[0].duration_ms, 400);
    }

    #[test]
    fn insert_vibration_pattern_with_duplicate_name_returns_duplicate_error() {
        // UNIQUE NOCASE on `name` prevents two rows that look the same
        // in the chooser even under different uuids.
        let db = Database::open_in_memory().unwrap();
        db.insert_vibration_pattern_with_uuid(
            "vp-1", "Pulse", 400, &[0.0, 1.0, 0.0],
            ChartKind::Line, false,
        ).unwrap();
        match db.insert_vibration_pattern_with_uuid(
            "vp-2", "PULSE", 400, &[0.0, 1.0, 0.0],
            ChartKind::Line, false,
        ) {
            Err(DbError::DuplicateVibrationPattern(name)) => assert_eq!(name, "PULSE"),
            other => panic!("expected DuplicateVibrationPattern, got {other:?}"),
        }
    }

    #[test]
    fn insert_vibration_pattern_returns_a_fresh_uuid() {
        let db = Database::open_in_memory().unwrap();
        let uuid_str = db.insert_vibration_pattern(
            "Custom Wave", 1000, &[0.0, 0.5, 1.0, 0.5, 0.0],
            ChartKind::Line, false,
        ).unwrap();
        // Returned string parses as a real UUID, not just any string.
        assert!(uuid::Uuid::parse_str(&uuid_str).is_ok(),
            "returned uuid should be parseable: {uuid_str}");
        // And points at the freshly-inserted row.
        let row = db.find_vibration_pattern_by_uuid(&uuid_str).unwrap()
            .expect("row should exist under the returned uuid");
        assert_eq!(row.name, "Custom Wave");
    }

    #[test]
    fn find_vibration_pattern_by_uuid_returns_none_for_unknown_uuid() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.find_vibration_pattern_by_uuid("nope").unwrap().is_none());
    }

    #[test]
    fn update_vibration_pattern_round_trips_changes() {
        let db = Database::open_in_memory().unwrap();
        let uuid_str = db.insert_vibration_pattern(
            "Wave", 2000, &[0.0, 0.5, 1.0, 0.5, 0.0],
            ChartKind::Line, false,
        ).unwrap();
        db.update_vibration_pattern(
            &uuid_str, "Slow Wave", 3000, &[0.0, 0.3, 0.6, 0.3, 0.0, 0.0, 0.0],
            ChartKind::Bar,
        ).unwrap();
        let row = db.find_vibration_pattern_by_uuid(&uuid_str).unwrap().unwrap();
        assert_eq!(row.name, "Slow Wave");
        assert_eq!(row.duration_ms, 3000);
        assert_eq!(row.intensities, vec![0.0, 0.3, 0.6, 0.3, 0.0, 0.0, 0.0]);
        assert_eq!(row.chart_kind, ChartKind::Bar);
    }

    #[test]
    fn update_vibration_pattern_bumps_updated_iso() {
        let db = Database::open_in_memory().unwrap();
        let uuid_str = db.insert_vibration_pattern(
            "Wave", 2000, &[0.0, 1.0, 0.0], ChartKind::Line, false,
        ).unwrap();
        let before = db.find_vibration_pattern_by_uuid(&uuid_str).unwrap().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        db.update_vibration_pattern(
            &uuid_str, "Wave", 2500, &[0.0, 1.0, 0.0], ChartKind::Line,
        ).unwrap();
        let after = db.find_vibration_pattern_by_uuid(&uuid_str).unwrap().unwrap();
        assert_eq!(after.created_iso, before.created_iso,
            "created_iso must not move on update");
        assert!(after.updated_iso > before.updated_iso,
            "updated_iso must bump forward on update");
    }

    #[test]
    fn update_vibration_pattern_unknown_uuid_is_silent_noop() {
        let db = Database::open_in_memory().unwrap();
        db.update_vibration_pattern(
            "never-existed", "anything", 1000, &[0.0, 1.0], ChartKind::Line,
        ).unwrap();
        let updates: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "vibration_pattern_update")
            .collect();
        assert!(updates.is_empty(),
            "no event for unknown uuid — peers would otherwise update a row they don't have");
    }

    #[test]
    fn update_vibration_pattern_to_existing_name_returns_duplicate_error() {
        let db = Database::open_in_memory().unwrap();
        db.insert_vibration_pattern_with_uuid(
            "vp-1", "Pulse", 400, &[0.0, 1.0, 0.0],
            ChartKind::Line, false,
        ).unwrap();
        let other = db.insert_vibration_pattern(
            "Wave", 1000, &[0.0, 1.0, 0.0], ChartKind::Line, false,
        ).unwrap();
        match db.update_vibration_pattern(
            &other, "PULSE", 1000, &[0.0, 1.0, 0.0], ChartKind::Line,
        ) {
            Err(DbError::DuplicateVibrationPattern(name)) => assert_eq!(name, "PULSE"),
            other => panic!("expected DuplicateVibrationPattern, got {other:?}"),
        }
    }

    #[test]
    fn delete_vibration_pattern_removes_row_and_emits_event() {
        let db = Database::open_in_memory().unwrap();
        let uuid_str = db.insert_vibration_pattern(
            "Wave", 2000, &[0.0, 1.0, 0.0], ChartKind::Line, false,
        ).unwrap();
        db.delete_vibration_pattern(&uuid_str).unwrap();
        assert!(db.list_vibration_patterns().unwrap().is_empty());
        assert!(db.find_vibration_pattern_by_uuid(&uuid_str).unwrap().is_none());
        let deletes: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "vibration_pattern_delete")
            .collect();
        assert_eq!(deletes.len(), 1);
    }

    #[test]
    fn delete_vibration_pattern_unknown_uuid_is_silent_noop() {
        let db = Database::open_in_memory().unwrap();
        db.delete_vibration_pattern("never-existed").unwrap();
        let deletes: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "vibration_pattern_delete")
            .collect();
        assert!(deletes.is_empty(),
            "no tombstone for a row peers never knew about");
    }

    #[test]
    fn is_vibration_pattern_name_taken_returns_true_for_existing_other_row() {
        let db = Database::open_in_memory().unwrap();
        db.insert_vibration_pattern_with_uuid(
            "vp-1", "Pulse", 400, &[0.0, 1.0, 0.0],
            ChartKind::Line, false,
        ).unwrap();
        assert!(db.is_vibration_pattern_name_taken("Pulse", "").unwrap());
        // Case-insensitive match.
        assert!(db.is_vibration_pattern_name_taken("PULSE", "").unwrap());
        assert!(db.is_vibration_pattern_name_taken("pulse", "").unwrap());
    }

    #[test]
    fn is_vibration_pattern_name_taken_returns_false_for_missing_name() {
        let db = Database::open_in_memory().unwrap();
        assert!(!db.is_vibration_pattern_name_taken("Anything", "").unwrap());
    }

    #[test]
    fn is_vibration_pattern_name_taken_excludes_self_via_except_uuid() {
        // Rename-no-op (same name as before): the row being renamed
        // should not collide with itself.
        let db = Database::open_in_memory().unwrap();
        db.insert_vibration_pattern_with_uuid(
            "vp-1", "Pulse", 400, &[0.0, 1.0, 0.0],
            ChartKind::Line, false,
        ).unwrap();
        assert!(!db.is_vibration_pattern_name_taken("Pulse", "vp-1").unwrap());
        // Other UUIDs still collide.
        assert!(db.is_vibration_pattern_name_taken("Pulse", "vp-2").unwrap());
    }

    #[test]
    fn apply_event_vibration_pattern_insert_creates_the_row_on_a_fresh_peer() {
        let peer = Database::open_in_memory().unwrap();
        let payload = serde_json::json!({
            "uuid": "vp-1",
            "name": "Wave",
            "duration_ms": 2000,
            "intensities_json": "[0.0,0.5,1.0,0.5,0.0]",
            "chart_kind": "line",
            "is_bundled": false,
            "created_iso": "2026-05-06T20:00:00Z",
            "updated_iso": "2026-05-06T20:00:00Z",
        });
        peer.apply_event(&synth_event(
            "vibration_pattern_insert", "vp-1", 5, DEVICE_A, payload,
        )).unwrap();
        let rows = peer.list_vibration_patterns().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Wave");
        assert_eq!(rows[0].duration_ms, 2000);
        assert_eq!(rows[0].intensities, vec![0.0, 0.5, 1.0, 0.5, 0.0]);
        assert_eq!(rows[0].chart_kind, ChartKind::Line);
    }

    #[test]
    fn apply_event_vibration_pattern_update_overwrites_earlier_state() {
        let peer = Database::open_in_memory().unwrap();
        peer.apply_event(&synth_event(
            "vibration_pattern_insert", "vp-1", 5, DEVICE_A,
            serde_json::json!({
                "uuid": "vp-1", "name": "Wave",
                "duration_ms": 2000,
                "intensities_json": "[0.0,1.0,0.0]",
                "chart_kind": "line", "is_bundled": false,
                "created_iso": "2026-05-06T20:00:00Z",
                "updated_iso": "2026-05-06T20:00:00Z",
            }),
        )).unwrap();
        peer.apply_event(&synth_event(
            "vibration_pattern_update", "vp-1", 7, DEVICE_A,
            serde_json::json!({
                "uuid": "vp-1", "name": "Slow Wave",
                "duration_ms": 3000,
                "intensities_json": "[0.0,0.5,0.0]",
                "chart_kind": "bar", "is_bundled": false,
                "created_iso": "2026-05-06T20:00:00Z",
                "updated_iso": "2026-05-06T20:05:00Z",
            }),
        )).unwrap();
        let rows = peer.list_vibration_patterns().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Slow Wave");
        assert_eq!(rows[0].duration_ms, 3000);
        assert_eq!(rows[0].chart_kind, ChartKind::Bar);
    }

    #[test]
    fn apply_event_vibration_pattern_delete_removes_the_row() {
        let peer = Database::open_in_memory().unwrap();
        peer.apply_event(&synth_event(
            "vibration_pattern_insert", "vp-1", 5, DEVICE_A,
            serde_json::json!({
                "uuid": "vp-1", "name": "Wave",
                "duration_ms": 2000,
                "intensities_json": "[0.0,1.0,0.0]",
                "chart_kind": "line", "is_bundled": false,
                "created_iso": "2026-05-06T20:00:00Z",
                "updated_iso": "2026-05-06T20:00:00Z",
            }),
        )).unwrap();
        peer.apply_event(&synth_event(
            "vibration_pattern_delete", "vp-1", 7, DEVICE_A,
            serde_json::json!({ "uuid": "vp-1" }),
        )).unwrap();
        assert!(peer.list_vibration_patterns().unwrap().is_empty());
    }

    #[test]
    fn apply_event_vibration_pattern_tombstone_resists_lower_lamport_insert() {
        // Out-of-order arrival: delete event lands first (lamport 10),
        // a stale insert (lamport 5) follows. Tombstone must win.
        let peer = Database::open_in_memory().unwrap();
        peer.apply_event(&synth_event(
            "vibration_pattern_delete", "vp-1", 10, DEVICE_A,
            serde_json::json!({ "uuid": "vp-1" }),
        )).unwrap();
        peer.apply_event(&synth_event(
            "vibration_pattern_insert", "vp-1", 5, DEVICE_A,
            serde_json::json!({
                "uuid": "vp-1", "name": "Wave",
                "duration_ms": 2000,
                "intensities_json": "[0.0,1.0,0.0]",
                "chart_kind": "line", "is_bundled": false,
                "created_iso": "2026-05-06T20:00:00Z",
                "updated_iso": "2026-05-06T20:00:00Z",
            }),
        )).unwrap();
        assert!(peer.list_vibration_patterns().unwrap().is_empty(),
            "tombstone with lamport 10 must beat insert with lamport 5");
    }

    #[test]
    fn replay_vibration_pattern_events_round_trips_to_a_fresh_peer() {
        let dev_a = Database::open_in_memory().unwrap();
        dev_a.insert_vibration_pattern_with_uuid(
            "vp-1", "Wave", 2000, &[0.0, 0.5, 1.0, 0.5, 0.0],
            ChartKind::Line, false,
        ).unwrap();
        dev_a.update_vibration_pattern(
            "vp-1", "Slow Wave", 3000, &[0.0, 0.3, 0.6, 0.3, 0.0],
            ChartKind::Bar,
        ).unwrap();
        let events: Vec<Event> = dev_a.pending_events().unwrap()
            .into_iter().map(|(_, e)| e).collect();

        let dev_b = Database::open_in_memory().unwrap();
        dev_b.replay_events(&events).unwrap();
        let rows = dev_b.list_vibration_patterns().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Slow Wave");
        assert_eq!(rows[0].chart_kind, ChartKind::Bar,
            "the more recent update event must drive the chart_kind");
    }

    #[test]
    fn vibration_patterns_table_is_created_on_fresh_open() {
        // Schema is wired into SCHEMA — opening a fresh DB lands the
        // table without any explicit setup. The CHECK constraint is
        // exercised by inserting a bad chart_kind directly.
        let db = Database::open_in_memory().unwrap();
        let count: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master \
             WHERE type = 'table' AND name = 'vibration_patterns'",
            [], |r| r.get(0),
        ).unwrap();
        assert_eq!(count, 1);

        // CHECK (chart_kind IN ('line','bar')) rejects everything else.
        let bad = db.conn.execute(
            "INSERT INTO vibration_patterns
                (uuid, name, duration_ms, intensities_json, chart_kind,
                 is_bundled, created_iso, updated_iso)
             VALUES ('u', 'n', 100, '[]', 'spiral', 0, 'now', 'now')",
            [],
        );
        assert!(bad.is_err(),
            "chart_kind = 'spiral' must be rejected by the CHECK constraint");
    }

    // ── BoxBreathPhases ───────────────────────────────────────────────

    #[test]
    fn box_breath_phase_id_round_trips_through_db_str() {
        for p in BoxBreathPhaseId::all() {
            assert_eq!(BoxBreathPhaseId::from_db_str(p.as_db_str()), Some(*p));
        }
        assert_eq!(BoxBreathPhaseId::from_db_str("typo"), None);
    }

    #[test]
    fn box_breath_phases_table_check_constraint_rejects_unknown_phase() {
        let db = Database::open_in_memory().unwrap();
        let bad = db.conn.execute(
            "INSERT INTO box_breath_phases (phase) VALUES ('paused')",
            [],
        );
        assert!(bad.is_err(),
            "phase = 'paused' must be rejected by the CHECK constraint");
    }

    #[test]
    fn list_box_breath_phases_is_empty_before_seeding() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.list_box_breath_phases().unwrap().is_empty());
    }

    #[test]
    fn seed_box_breath_phases_creates_four_rows_with_defaults() {
        let db = Database::open_in_memory().unwrap();
        db.seed_box_breath_phases().unwrap();
        let rows = db.list_box_breath_phases().unwrap();
        assert_eq!(rows.len(), 4);
        // Order matches BoxBreathPhaseId::all().
        let expected_order = [
            BoxBreathPhaseId::In,
            BoxBreathPhaseId::HoldIn,
            BoxBreathPhaseId::Out,
            BoxBreathPhaseId::HoldOut,
        ];
        for (got, want) in rows.iter().zip(expected_order.iter()) {
            assert_eq!(got.phase, *want);
            assert!(!got.enabled, "fresh seed: enabled defaults to false");
            assert_eq!(got.signal_mode, SignalMode::Sound);
            assert!(!got.sound_uuid.is_empty());
            assert!(!got.pattern_uuid.is_empty());
        }
    }

    #[test]
    fn seed_box_breath_phases_is_idempotent() {
        let db = Database::open_in_memory().unwrap();
        db.seed_box_breath_phases().unwrap();
        db.seed_box_breath_phases().unwrap();
        assert_eq!(db.list_box_breath_phases().unwrap().len(), 4);
    }

    #[test]
    fn set_box_breath_phase_round_trips_through_get() {
        let db = Database::open_in_memory().unwrap();
        db.seed_box_breath_phases().unwrap();
        db.set_box_breath_phase(
            BoxBreathPhaseId::In,
            true,
            SignalMode::Both,
            "sound-uuid-x",
            "pattern-uuid-y",
        ).unwrap();
        let row = db.get_box_breath_phase(BoxBreathPhaseId::In).unwrap().unwrap();
        assert!(row.enabled);
        assert_eq!(row.signal_mode, SignalMode::Both);
        assert_eq!(row.sound_uuid, "sound-uuid-x");
        assert_eq!(row.pattern_uuid, "pattern-uuid-y");
        // Other phases stay at their defaults.
        let other = db.get_box_breath_phase(BoxBreathPhaseId::HoldIn).unwrap().unwrap();
        assert!(!other.enabled);
    }

    #[test]
    fn replay_box_breath_phase_event_round_trips_to_a_fresh_peer() {
        let dev_a = Database::open_in_memory().unwrap();
        dev_a.seed_box_breath_phases().unwrap();
        dev_a.set_box_breath_phase(
            BoxBreathPhaseId::Out,
            true, SignalMode::Both, "s-uuid", "p-uuid",
        ).unwrap();
        let events: Vec<Event> = dev_a.pending_events().unwrap()
            .into_iter().map(|(_, e)| e).collect();

        let dev_b = Database::open_in_memory().unwrap();
        dev_b.seed_box_breath_phases().unwrap();
        dev_b.replay_events(&events).unwrap();

        let row = dev_b.get_box_breath_phase(BoxBreathPhaseId::Out)
            .unwrap().unwrap();
        assert!(row.enabled);
        assert_eq!(row.signal_mode, SignalMode::Both);
        assert_eq!(row.sound_uuid, "s-uuid");
        assert_eq!(row.pattern_uuid, "p-uuid");
    }

    #[test]
    fn apply_event_box_breath_phase_creates_row_on_unseeded_peer() {
        // Edge case: a peer receives the update event before seeding.
        // The recompute path INSERT-OR-UPDATEs so the row materialises.
        let peer = Database::open_in_memory().unwrap();
        peer.apply_event(&synth_event(
            "box_breath_phase_update", "in", 5, DEVICE_A,
            serde_json::json!({
                "phase": "in",
                "enabled": true,
                "signal_mode": "vibration",
                "sound_uuid": "x",
                "pattern_uuid": "y",
            }),
        )).unwrap();
        let row = peer.get_box_breath_phase(BoxBreathPhaseId::In)
            .unwrap().unwrap();
        assert!(row.enabled);
        assert_eq!(row.signal_mode, SignalMode::Vibration);
    }

    #[test]
    fn set_box_breath_phase_emits_a_box_breath_phase_update_event() {
        let db = Database::open_in_memory().unwrap();
        db.seed_box_breath_phases().unwrap();
        db.set_box_breath_phase(
            BoxBreathPhaseId::Out,
            true, SignalMode::Vibration, "s-uuid", "p-uuid",
        ).unwrap();
        let updates: Vec<_> = db.pending_events().unwrap()
            .into_iter()
            .filter(|(_, e)| e.kind == "box_breath_phase_update")
            .collect();
        assert_eq!(updates.len(), 1);
        let payload: serde_json::Value =
            serde_json::from_str(&updates[0].1.payload).unwrap();
        assert_eq!(payload["phase"], "out");
        assert_eq!(payload["enabled"], true);
        assert_eq!(payload["signal_mode"], "vibration");
        assert_eq!(payload["sound_uuid"], "s-uuid");
        assert_eq!(payload["pattern_uuid"], "p-uuid");
    }
}
