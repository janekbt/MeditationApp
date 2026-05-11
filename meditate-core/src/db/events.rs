//! `events` table — append-only sync event log. Holds the Event
//! struct, the log CRUD (append, pending, known_uuids, mark_synced,
//! flag_unsynced), the local-author emit_event helper, the recovery
//! `wipe_local_event_log`, and the apply/replay dispatch that
//! materialises events into the per-entity cache rows.

use rusqlite::params;

use super::{
    target_id_is_well_formed_for, BoxBreathPhaseId, Database, Result, CACHE_SCHEMA_VERSION,
    CACHE_SCHEMA_VERSION_KEY,
};

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

impl Database {
    /// If the stored cache-schema version is below `CACHE_SCHEMA_VERSION`,
    /// replay every event so any kind that this build understands but
    /// the previous build skipped (apply_event_inner's "unknown kind"
    /// branch) gets materialised into the cache. Idempotent: re-applying
    /// understood kinds is a no-op (their dispatchers are UPSERT-shaped),
    /// and lamport_clock isn't bumped because the events are not "fresh
    /// from a peer" (was_new=false on re-record).
    pub(super) fn maybe_walk_events_for_cache_upgrade(&self) -> Result<()> {
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
    /// All DELETEs run inside one transaction so the wipe is atomic.
    /// box_breath_phases re-seeds inline to its 4 default rows —
    /// Box-Breath mode requires those rows to render, and the seed
    /// is gate-less so it's safe to call directly.
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
    pub(super) fn emit_event(
        &self,
        kind: &str,
        target_id: &str,
        payload: String,
    ) -> Result<()> {
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
}
