//! `events` table — append-only sync event log. Holds the Event
//! struct, the log CRUD (append, pending, known_uuids, mark_synced,
//! flag_unsynced), the local-author emit_event helper, the recovery
//! `wipe_local_event_log`, and the apply/replay dispatch that
//! materialises events into the per-entity cache rows.

use rusqlite::{params, OptionalExtension};

use super::{
    target_id_is_well_formed_for, BoxBreathPhaseId, Database, DbError, Result,
    CACHE_SCHEMA_VERSION, CACHE_SCHEMA_VERSION_KEY,
};

/// Typed variant of `Event.kind`. Replaces the bare string literals
/// that emit_event / apply_event_inner previously passed — a typo
/// on either side is now a compile error rather than a silent sync
/// data-loss bug.
///
/// `as_db_str` produces the canonical wire string stored in
/// `events.kind`; `from_db_str` parses one back. Unknown strings
/// (e.g. an event kind a future build understands but this one
/// doesn't) return `None` and propagate to apply_event_inner's
/// forward-compat path: record the event, skip dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EventKind {
    SessionInsert,
    SessionUpdate,
    SessionDelete,
    LabelInsert,
    LabelRename,
    LabelDelete,
    IntervalBellInsert,
    IntervalBellUpdate,
    IntervalBellDelete,
    BellSoundInsert,
    BellSoundUpdate,
    BellSoundDelete,
    PresetInsert,
    PresetUpdate,
    PresetDelete,
    GuidedFileInsert,
    GuidedFileUpdate,
    GuidedFileDelete,
    VibrationPatternInsert,
    VibrationPatternUpdate,
    VibrationPatternDelete,
    BoxBreathPhaseUpdate,
    SettingChanged,
}

impl EventKind {
    pub(crate) fn as_db_str(self) -> &'static str {
        match self {
            EventKind::SessionInsert => "session_insert",
            EventKind::SessionUpdate => "session_update",
            EventKind::SessionDelete => "session_delete",
            EventKind::LabelInsert => "label_insert",
            EventKind::LabelRename => "label_rename",
            EventKind::LabelDelete => "label_delete",
            EventKind::IntervalBellInsert => "interval_bell_insert",
            EventKind::IntervalBellUpdate => "interval_bell_update",
            EventKind::IntervalBellDelete => "interval_bell_delete",
            EventKind::BellSoundInsert => "bell_sound_insert",
            EventKind::BellSoundUpdate => "bell_sound_update",
            EventKind::BellSoundDelete => "bell_sound_delete",
            EventKind::PresetInsert => "preset_insert",
            EventKind::PresetUpdate => "preset_update",
            EventKind::PresetDelete => "preset_delete",
            EventKind::GuidedFileInsert => "guided_file_insert",
            EventKind::GuidedFileUpdate => "guided_file_update",
            EventKind::GuidedFileDelete => "guided_file_delete",
            EventKind::VibrationPatternInsert => "vibration_pattern_insert",
            EventKind::VibrationPatternUpdate => "vibration_pattern_update",
            EventKind::VibrationPatternDelete => "vibration_pattern_delete",
            EventKind::BoxBreathPhaseUpdate => "box_breath_phase_update",
            EventKind::SettingChanged => "setting_changed",
        }
    }

    pub(crate) fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "session_insert" => Some(EventKind::SessionInsert),
            "session_update" => Some(EventKind::SessionUpdate),
            "session_delete" => Some(EventKind::SessionDelete),
            "label_insert" => Some(EventKind::LabelInsert),
            "label_rename" => Some(EventKind::LabelRename),
            "label_delete" => Some(EventKind::LabelDelete),
            "interval_bell_insert" => Some(EventKind::IntervalBellInsert),
            "interval_bell_update" => Some(EventKind::IntervalBellUpdate),
            "interval_bell_delete" => Some(EventKind::IntervalBellDelete),
            "bell_sound_insert" => Some(EventKind::BellSoundInsert),
            "bell_sound_update" => Some(EventKind::BellSoundUpdate),
            "bell_sound_delete" => Some(EventKind::BellSoundDelete),
            "preset_insert" => Some(EventKind::PresetInsert),
            "preset_update" => Some(EventKind::PresetUpdate),
            "preset_delete" => Some(EventKind::PresetDelete),
            "guided_file_insert" => Some(EventKind::GuidedFileInsert),
            "guided_file_update" => Some(EventKind::GuidedFileUpdate),
            "guided_file_delete" => Some(EventKind::GuidedFileDelete),
            "vibration_pattern_insert" => Some(EventKind::VibrationPatternInsert),
            "vibration_pattern_update" => Some(EventKind::VibrationPatternUpdate),
            "vibration_pattern_delete" => Some(EventKind::VibrationPatternDelete),
            "box_breath_phase_update" => Some(EventKind::BoxBreathPhaseUpdate),
            "setting_changed" => Some(EventKind::SettingChanged),
            _ => None,
        }
    }

    /// Which materialised cache table this event kind mutates. Used by
    /// `replay_events` to dedup recomputes — a session edited five
    /// times in the same batch recomputes once — and to order Session
    /// recomputes after Label recomputes (Session's dereferences
    /// labels.uuid → labels.id during materialise).
    pub(crate) fn entity(self) -> EntityKind {
        match self {
            EventKind::SessionInsert
            | EventKind::SessionUpdate
            | EventKind::SessionDelete => EntityKind::Session,
            EventKind::LabelInsert
            | EventKind::LabelRename
            | EventKind::LabelDelete => EntityKind::Label,
            EventKind::IntervalBellInsert
            | EventKind::IntervalBellUpdate
            | EventKind::IntervalBellDelete => EntityKind::IntervalBell,
            EventKind::BellSoundInsert
            | EventKind::BellSoundUpdate
            | EventKind::BellSoundDelete => EntityKind::BellSound,
            EventKind::PresetInsert
            | EventKind::PresetUpdate
            | EventKind::PresetDelete => EntityKind::Preset,
            EventKind::GuidedFileInsert
            | EventKind::GuidedFileUpdate
            | EventKind::GuidedFileDelete => EntityKind::GuidedFile,
            EventKind::VibrationPatternInsert
            | EventKind::VibrationPatternUpdate
            | EventKind::VibrationPatternDelete => EntityKind::VibrationPattern,
            EventKind::BoxBreathPhaseUpdate => EntityKind::BoxBreathPhase,
            EventKind::SettingChanged => EntityKind::Setting,
        }
    }
}

/// Which materialised cache table an event mutates. Sibling of
/// `EventKind` collapsed across the insert/update/delete dimension —
/// the recompute is the same regardless of which CRUD variant fired.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum EntityKind {
    Session,
    Label,
    IntervalBell,
    BellSound,
    Preset,
    GuidedFile,
    VibrationPattern,
    BoxBreathPhase,
    Setting,
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
    pub(crate) fn append_event(&self, event: &Event) -> Result<i64> {
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
        // On the new-row path `last_insert_rowid()` already knows the
        // id — saves a SELECT round-trip on the common path. Only the
        // dup-ignore path needs to look up the existing rowid.
        let rowid = if was_new {
            self.conn.last_insert_rowid()
        } else {
            self.conn.query_row(
                "SELECT id FROM events WHERE event_uuid = ?1",
                params![event.event_uuid],
                |row| row.get::<_, i64>(0),
            )?
        };
        Ok((rowid, was_new))
    }

    /// All events not yet pushed to remote, ordered by `lamport_ts` ASC
    /// (then by local `id` as a stable tie-break). Sync's push phase
    /// drains this list in order; mark each entry with `mark_events_synced`
    /// once the WebDAV PUT succeeds.
    pub fn pending_events(&self) -> Result<Vec<(i64, Event)>> {
        let mut stmt = self.conn.prepare_cached(
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
    pub(crate) fn known_event_uuids(&self) -> Result<std::collections::HashSet<String>> {
        let mut stmt = self.conn.prepare_cached("SELECT event_uuid FROM events")?;
        let ids = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<std::collections::HashSet<_>>>()?;
        Ok(ids)
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
    pub(crate) fn flag_all_events_unsynced(&self) -> Result<()> {
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
    pub(crate) fn wipe_local_event_log(&self) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM known_remote_files", [])?;
        tx.execute("DELETE FROM known_remote_sounds", [])?;
        // The in-flight snapshot is device-local state derived from
        // local activity. If we kept it across a "throw away local,
        // pull from remote" recovery, the next launch would auto-
        // finalise it into a phantom session no peer authored.
        tx.execute("DELETE FROM session_in_progress", [])?;
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
        // Reset the Lamport clock. A value of N means "observed N
        // prior events"; with the log gone, that justification is
        // gone too. A subsequent pull from a peer advances the clock
        // past the peer's max via observe_remote_lamport, so the
        // reset is invisible in the typical recovery flow.
        tx.execute("UPDATE device SET lamport_clock = 0", [])?;
        tx.commit()?;
        Ok(())
    }

    /// Flip the `synced` flag on every event with one of the given local
    /// rowids so they drop out of `pending_events`. Used by the bulk-
    /// push path: after one PUT covers all pending events, a single
    /// transaction flips the synced flag on every contained event_id.
    /// Marking N rows one-at-a-time would fire N autocommit fsyncs;
    /// batching them folds into the WAL's usual one-fsync-per-commit.
    /// Unknown ids are silently no-ops (SQLite's UPDATE-on-no-match
    /// behaviour) so a stale id from a partial sync doesn't escalate.
    /// Empty input is a no-op.
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
    ///
    /// # Precondition
    ///
    /// **Every caller must already be inside an `unchecked_transaction`
    /// on `self.conn`.** Three steps run in sequence on `self.conn`
    /// (Lamport bump, event append, the caller's own data write), and
    /// a partial failure between any pair is corrupting:
    ///
    /// - A bumped Lamport without an appended event leaves the clock
    ///   one step ahead of any actual event — fine in isolation, but
    ///   the same clock value can never be reused, so a future event
    ///   that happens to land on it would diverge from a peer's view.
    /// - An appended event without the matching cache-row write means
    ///   the next `recompute_*` reads stale cache state but sees the
    ///   new event — the row in the cache table no longer matches the
    ///   event log.
    ///
    /// The precondition is honored at every call site today, but the
    /// signature doesn't enforce it. See backlog entry "Make
    /// `emit_event` take `&Transaction`" for the structural fix.
    pub(super) fn emit_event(
        &self,
        kind: EventKind,
        target_id: &str,
        payload: String,
    ) -> Result<()> {
        let device_id = self.device_id()?;
        let lamport_ts = self.bump_lamport_clock()?;
        let event = Event {
            event_uuid: uuid::Uuid::new_v4().to_string(),
            lamport_ts,
            device_id,
            kind: kind.as_db_str().to_string(),
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
    ///
    /// Production replay goes through `apply_event_inner` directly via
    /// `replay_events`; this single-event wrapper is gated to test
    /// builds because it has no production callers — tests that need
    /// the simpler "one event in, one row out" entry point use it.
    #[cfg(test)]
    pub(crate) fn apply_event(&self, event: &Event) -> Result<()> {
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
        if let Some(entity) = self.apply_event_record_only(event)? {
            self.recompute_for(entity, &event.target_id)?;
        }
        Ok(())
    }

    /// Record + validate the event without firing its recompute_*.
    /// Returns `Some(entity)` if the caller should follow up with a
    /// `recompute_for(entity, target_id)`. Returns `None` when the
    /// event is rejected (invalid target_id) or unknown (kind a
    /// future build understands but this one doesn't) — both are
    /// soft-fail / record-but-skip-dispatch paths.
    ///
    /// Split out from `apply_event_inner` so `replay_events` can
    /// record N events first, then dedup their recomputes per
    /// `(EntityKind, target_id)` — a session edited five times in
    /// the same batch recomputes once.
    fn apply_event_record_only(&self, event: &Event) -> Result<Option<EntityKind>> {
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
            return Ok(None);
        }

        // Unknown kind → record but skip dispatch (forward-compat).
        Ok(EventKind::from_db_str(&event.kind).map(|k| k.entity()))
    }

    /// Materialise the named entity's cache row from the events table.
    /// Each `recompute_X` is a pure function of `events.*` filtered
    /// by `target_id`, so it can run at any point after the events
    /// are recorded — `replay_events` batches them at the end of
    /// the apply loop.
    fn recompute_for(&self, entity: EntityKind, target_id: &str) -> Result<()> {
        match entity {
            EntityKind::Session => self.recompute_session(target_id),
            EntityKind::Label => self.recompute_label(target_id),
            EntityKind::IntervalBell => self.recompute_interval_bell(target_id),
            EntityKind::BellSound => self.recompute_bell_sound(target_id),
            EntityKind::Preset => self.recompute_preset(target_id),
            EntityKind::GuidedFile => self.recompute_guided_file(target_id),
            EntityKind::VibrationPattern => self.recompute_vibration_pattern(target_id),
            EntityKind::BoxBreathPhase => self.recompute_box_breath_phase(target_id),
            EntityKind::Setting => self.recompute_setting(target_id),
        }
    }

    /// Resolve which mutate event (if any) currently drives the cache
    /// row for `target_id`, returning its JSON payload. `None` means
    /// the row should be absent — either no mutate events exist for
    /// the target, or a delete event with `lamport_ts >= mutate_ts`
    /// has tombstoned it.
    ///
    /// Centralises the LWW + tombstone-tiebreak rule that every
    /// `recompute_X` family member relies on:
    ///
    ///   - highest (lamport_ts, device_id) mutate wins
    ///   - a delete with lamport_ts >= the winning mutate's lamport_ts
    ///     wins over the mutate (tombstones break ties in the delete's
    ///     favour, so insert+delete arriving out of order still
    ///     converges to deleted)
    ///
    /// Returns `Some(payload)` when the row should exist; the caller
    /// destructures the JSON into its entity-specific column set and
    /// UPSERTs. Returns `Ok(None)` when the row should be absent;
    /// the caller issues a DELETE.
    pub(super) fn winning_mutate(
        &self,
        target_id: &str,
        mutate_kinds: [EventKind; 2],
        delete_kind: EventKind,
    ) -> Result<Option<serde_json::Value>> {
        // `prepare_cached`: replay_events fires this helper once per
        // event during recovery (~30k events on a wipe-and-pull),
        // each call running the same two SQL strings. Caching the
        // prepared statements turns 60k parse-and-plan cycles into
        // 2 — dominates the eMMC budget on the Librem 5.
        let mut delete_stmt = self.conn.prepare_cached(
            "SELECT MAX(lamport_ts) FROM events
             WHERE target_id = ?1 AND kind = ?2",
        )?;
        let delete_ts: Option<i64> = delete_stmt.query_row(
            params![target_id, delete_kind.as_db_str()],
            |row| row.get::<_, Option<i64>>(0),
        )?;
        let mut mutate_stmt = self.conn.prepare_cached(
            "SELECT lamport_ts, payload FROM events
             WHERE target_id = ?1
               AND kind IN (?2, ?3)
             ORDER BY lamport_ts DESC, device_id DESC
             LIMIT 1",
        )?;
        let mutate: Option<(i64, String)> = mutate_stmt
            .query_row(
                params![
                    target_id,
                    mutate_kinds[0].as_db_str(),
                    mutate_kinds[1].as_db_str(),
                ],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        let row_should_exist = match (mutate.as_ref(), delete_ts) {
            (Some(_), None) => true,
            (None, _) => false,
            (Some((m_ts, _)), Some(d_ts)) => *m_ts > d_ts,
        };
        if let Some((_, payload)) = mutate.filter(|_| row_should_exist) {
            let v: serde_json::Value = serde_json::from_str(&payload)
                .map_err(|e| DbError::Decode(
                    format!("event payload not valid JSON: {e}")))?;
            Ok(Some(v))
        } else {
            Ok(None)
        }
    }

    /// Apply a batch of events to the materialized cache. Events are
    /// sorted by `(lamport_ts, device_id, event_uuid)` for a stable
    /// deterministic order before dispatch — this matches the canonical
    /// replay order across peers (the plan's tie-break rule). The whole
    /// batch runs inside one transaction so a partial failure rolls back.
    /// Idempotent on `event_uuid`: repeat calls with the same input are
    /// no-ops on the cache.
    ///
    /// Recompute dedup: each `recompute_X(target_id)` is a pure function
    /// of the events table contents, so calling it once at the end of
    /// the batch produces the same row as calling it after every event
    /// for that target. We record every event first, collect the set
    /// of `(EntityKind, target_id)` pairs touched, then recompute each
    /// pair once. A session edited five times in one batch recomputes
    /// once; on the wipe-and-pull recovery path (~30k events across
    /// ~10k targets) this drops the 2–3× duplicate-recompute factor.
    pub fn replay_events(&self, events: &[Event]) -> Result<()> {
        if events.is_empty() { return Ok(()); }
        let tx = self.conn.unchecked_transaction()?;
        let mut sorted: Vec<&Event> = events.iter().collect();
        sorted.sort_by(|a, b| {
            a.lamport_ts.cmp(&b.lamport_ts)
                .then_with(|| a.device_id.cmp(&b.device_id))
                .then_with(|| a.event_uuid.cmp(&b.event_uuid))
        });
        let mut touched: std::collections::HashSet<(EntityKind, String)> =
            std::collections::HashSet::new();
        for event in sorted {
            if let Some(entity) = self.apply_event_record_only(event)? {
                touched.insert((entity, event.target_id.clone()));
            }
        }
        // Two-pass recompute. `recompute_session` dereferences
        // labels.uuid → labels.id during materialise, so labels must
        // be present in the cache by the time it runs. Every other
        // `recompute_X` reads only from `events` and is order-free.
        // Splitting into "non-Session first, Session second" honours
        // the one cross-entity dependency without paying for a
        // topological sort.
        for (entity, target_id) in &touched {
            if *entity != EntityKind::Session {
                self.recompute_for(*entity, target_id)?;
            }
        }
        for (entity, target_id) in &touched {
            if *entity == EntityKind::Session {
                self.recompute_for(*entity, target_id)?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{
        test_helpers::*, BellSoundCategory, BoxBreathPhaseId, ChartKind,
        IntervalBellKind, Session, SessionMode, SignalMode,
    };
    use crate::seeds::{BUNDLED_BOWL_UUID, BUNDLED_PATTERN_PULSE_UUID};

    // ── EventKind wire-format round trip ─────────────────────────────────────
    //
    // The typed EventKind enum is the in-process API; the wire format
    // is still the string stored in `events.kind`. as_db_str/from_db_str
    // must be inverses for every variant — drift between them is a
    // silent sync-data-loss bug exactly the EventKind enum was added
    // to prevent.

    #[test]
    fn event_kind_round_trips_through_db_strings() {
        let all = [
            EventKind::SessionInsert,
            EventKind::SessionUpdate,
            EventKind::SessionDelete,
            EventKind::LabelInsert,
            EventKind::LabelRename,
            EventKind::LabelDelete,
            EventKind::IntervalBellInsert,
            EventKind::IntervalBellUpdate,
            EventKind::IntervalBellDelete,
            EventKind::BellSoundInsert,
            EventKind::BellSoundUpdate,
            EventKind::BellSoundDelete,
            EventKind::PresetInsert,
            EventKind::PresetUpdate,
            EventKind::PresetDelete,
            EventKind::GuidedFileInsert,
            EventKind::GuidedFileUpdate,
            EventKind::GuidedFileDelete,
            EventKind::VibrationPatternInsert,
            EventKind::VibrationPatternUpdate,
            EventKind::VibrationPatternDelete,
            EventKind::BoxBreathPhaseUpdate,
            EventKind::SettingChanged,
        ];
        for kind in all {
            assert_eq!(
                EventKind::from_db_str(kind.as_db_str()),
                Some(kind),
                "round trip failed for {kind:?}",
            );
        }
    }

    #[test]
    fn event_kind_from_db_str_returns_none_on_unknown_string() {
        // Forward-compat: an event kind a future build understands
        // but this one doesn't must parse as None, so apply_event_inner
        // records the row and skips dispatch instead of mis-routing it.
        assert_eq!(EventKind::from_db_str("session_archive"), None);
        assert_eq!(EventKind::from_db_str(""), None);
        assert_eq!(EventKind::from_db_str("SESSION_INSERT"), None);
    }

    #[test]
    fn apply_event_with_unknown_kind_records_but_skips_dispatch() {
        // The forward-compat invariant: a kind this build doesn't
        // recognise still lands in the events table (so a later build
        // that knows it can pick it up on cache-schema replay) but
        // doesn't mutate any cache row.
        let db = Database::open_in_memory().unwrap();
        let event = Event {
            event_uuid: "00000000-0000-0000-0000-000000000099".into(),
            lamport_ts: 1,
            device_id: DEVICE_A.into(),
            kind: "session_archive".into(),
            target_id: SESSION_X.into(),
            payload: "{}".into(),
        };
        db.apply_event(&event).unwrap();
        // Row landed in events table.
        let pending = db.pending_events().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].1.kind, "session_archive");
        // sessions cache stayed empty.
        assert!(crate::db::list_sessions_from_db(&db).unwrap().is_empty());
    }

    // ── Event log: append + pending + mark_synced (A2.3) ─────────────────────
    //
    // The append-only event log is the single source of truth for all
    // mutations. `append_event` is idempotent on `event_uuid`: receiving
    // the same event twice (e.g. on retry, or from a peer that already
    // forwarded it) is a no-op rather than a constraint error escalated
    // to the caller. `pending_events` is the push-queue contract — sorted
    // by `lamport_ts` so peers see events in causal order.

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
    fn mark_events_synced_marks_a_single_id() {
        let db = Database::open_in_memory().unwrap();
        let id_a = db.append_event(&sample_event(1)).unwrap();
        let _id_b = db.append_event(&sample_event(2)).unwrap();
        db.mark_events_synced(&[id_a]).unwrap();
        let pending: Vec<i64> = db.pending_events().unwrap()
            .iter().map(|(_, e)| e.lamport_ts).collect();
        assert_eq!(pending, vec![2],
            "synced event must drop out of the pending list");
    }

    #[test]
    fn mark_events_synced_unknown_id_is_a_silent_no_op() {
        // Defensive: a stale id from a partial sync attempt must not
        // panic or surface an error. SQLite UPDATE on no-match is
        // already a no-op; the wrapper preserves that.
        let db = Database::open_in_memory().unwrap();
        db.append_event(&sample_event(1)).unwrap();
        let res = db.mark_events_synced(&[999]);
        assert!(res.is_ok());
        assert_eq!(db.pending_events().unwrap().len(), 1,
            "the existing event must still be pending — nothing was marked");
    }

    #[test]
    fn mark_events_synced_batch_marks_every_provided_id() {
        // The batch variant must produce the same end state as a
        // single-id mark — exercised by the bulk-push path to flip
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
        let labels_before = crate::db::list_labels_from_db(&db).unwrap().len();

        db.flag_all_events_unsynced().unwrap();

        assert_eq!(crate::db::list_labels_from_db(&db).unwrap().len(), labels_before);
        assert!(crate::db::list_labels_from_db(&db).unwrap().iter().any(|l| l.id == label_id));
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
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        db.insert_interval_bell(IntervalBellKind::Interval, 5, 0, BUNDLED_BOWL_UUID, BUNDLED_PATTERN_PULSE_UUID, SignalMode::Sound).unwrap();
        db.insert_bell_sound("Custom", "/p/c.wav", false, "audio/wav", BellSoundCategory::General).unwrap();
        db.insert_preset("Sitting", SessionMode::Timer, true, r#"{}"#).unwrap();
        db.insert_guided_file_with_uuid("gf-1", "Track", "/p/t.ogg", 300, false).unwrap();
        db.insert_vibration_pattern("Custom Pulse", 200, &[1.0, 0.0], ChartKind::Bar, false).unwrap();
        db.set_box_breath_phase(BoxBreathPhaseId::In, false, SignalMode::Sound, "x", "y").unwrap();
        db.record_known_remote_file("a").unwrap();
        db.record_known_remote_sound("bs-1").unwrap();
        // Sanity: rows present before wipe.
        assert!(!db.pending_events().unwrap().is_empty());
        assert!(!crate::db::list_labels_from_db(&db).unwrap().is_empty());
        assert!(!crate::db::list_sessions_from_db(&db).unwrap().is_empty());
        assert!(!db.list_interval_bells().unwrap().is_empty());
        assert!(!db.list_bell_sounds().unwrap().is_empty());
        assert!(!crate::db::list_presets_from_db(&db).unwrap().is_empty());
        assert!(!crate::db::list_guided_files_from_db(&db).unwrap().is_empty());
        assert!(!crate::db::list_vibration_patterns_from_db(&db).unwrap().is_empty());
        assert!(!db.known_remote_file_uuids().unwrap().is_empty());
        assert!(!db.known_remote_sound_uuids().unwrap().is_empty());

        db.wipe_local_event_log().unwrap();

        assert!(db.pending_events().unwrap().is_empty(),
            "events table must be empty");
        assert!(crate::db::list_labels_from_db(&db).unwrap().is_empty(),
            "labels table must be empty");
        assert!(crate::db::list_sessions_from_db(&db).unwrap().is_empty(),
            "sessions table must be empty");
        assert!(db.list_interval_bells().unwrap().is_empty(),
            "interval_bells table must be empty");
        assert!(db.list_bell_sounds().unwrap().is_empty(),
            "bell_sounds table must be empty");
        assert!(crate::db::list_presets_from_db(&db).unwrap().is_empty(),
            "presets table must be empty");
        assert!(crate::db::list_guided_files_from_db(&db).unwrap().is_empty(),
            "guided_files table must be empty");
        assert!(crate::db::list_vibration_patterns_from_db(&db).unwrap().is_empty(),
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
    fn wipe_local_event_log_preserves_device_id() {
        // Device identity persists across wipes. Resetting device_id
        // would create a new identity for the same physical device,
        // confusing peers' replay.
        let db = Database::open_in_memory().unwrap();
        let device_before = db.device_id().unwrap();

        db.wipe_local_event_log().unwrap();

        assert_eq!(db.device_id().unwrap(), device_before,
            "device_id must survive wipe — it's this device's identity");
    }

    #[test]
    fn wipe_local_event_log_resets_lamport_clock_to_zero() {
        // The wipe is the "remote data lost — restart from blank"
        // recovery primitive. Lamport-causal ordering says a value
        // of N means "this happened after observing N prior events";
        // after wipe those events are gone locally. Leaving the clock
        // at N would have the next emit author lamport N+1 with no
        // preceding events to justify the timestamp, growing drift
        // forever. A re-pull from a peer advances the clock past the
        // peer's max via observe_remote_lamport anyway, so resetting
        // costs nothing in the typical recovery flow.
        let db = Database::open_in_memory().unwrap();
        for _ in 0..5 { db.bump_lamport_clock().unwrap(); }
        assert!(db.lamport_clock().unwrap() > 0,
            "precondition: clock is non-zero before wipe");

        db.wipe_local_event_log().unwrap();

        assert_eq!(db.lamport_clock().unwrap(), 0,
            "lamport_clock must reset to 0 — events justifying its value are gone");
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
    fn wipe_local_event_log_clears_session_in_progress() {
        // Wipe is the "local copy is wrong, throw it away and pull
        // from remote" recovery. An in-progress snapshot is device-
        // local state derived from local activity; if we keep it
        // around, the next launch would auto-finalise it into a
        // phantom session that no peer ever saw, polluting the
        // post-recovery sync state. Wipe must take it.
        let db = Database::open_in_memory().unwrap();
        let snapshot = crate::db::SessionInProgress {
            start_iso: "2026-05-13T10:00:00".into(),
            accumulated_secs: 600,
            mode: SessionMode::Timer,
            mode_payload: "{}".into(),
            label_id: None,
            guided_file_uuid: None,
        };
        db.set_session_in_progress(&snapshot).unwrap();
        assert!(db.get_session_in_progress().unwrap().is_some());

        db.wipe_local_event_log().unwrap();
        assert!(db.get_session_in_progress().unwrap().is_none(),
            "wipe must drop the in-progress snapshot");
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
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        assert_eq!(crate::db::list_sessions_from_db(&db).unwrap().len(), 1);
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
        db.mark_events_synced(&[id_a]).unwrap();
        db.mark_events_synced(&[id_b]).unwrap();
        assert!(db.pending_events().unwrap().is_empty());
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
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        let row_uuid = crate::db::list_sessions_from_db(&db).unwrap()[0].1.uuid.clone();
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
            uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        let row_uuid = crate::db::list_sessions_from_db(&db).unwrap()[0].1.uuid.clone();
        drain_events(&db);
        db.delete_session(id).unwrap();
        let events = db.pending_events().unwrap();
        assert_eq!(events[0].1.target_id, row_uuid);
    }

    #[test]
    fn label_insert_event_target_id_is_the_label_uuid() {
        let db = Database::open_in_memory().unwrap();
        db.insert_label("Morning").unwrap();
        let row_uuid = crate::db::list_labels_from_db(&db).unwrap()[0].uuid.clone();
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
        assert!(crate::db::list_sessions_from_db(&db).unwrap().is_empty());
        assert!(crate::db::list_labels_from_db(&db).unwrap().is_empty());
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
        let rows = crate::db::list_sessions_from_db(&db).unwrap();
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

        let in_order = crate::db::list_sessions_from_db(&db_in_order).unwrap();
        let reversed = crate::db::list_sessions_from_db(&db_reversed).unwrap();
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
        assert_eq!(crate::db::list_sessions_from_db(&db).unwrap().len(), 1);
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
            mode: SessionMode::Timer, uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();
        device_b.insert_session(&Session {
            start_iso: "2026-04-30T18:00:00".to_string(),
            duration_secs: 1200, label_id: None, notes: Some("from B".to_string()),
            mode: SessionMode::Timer, uuid: crate::db::SessionUuid::new(""),
            guided_file_uuid: None,
        }).unwrap();

        let events_a: Vec<Event> = device_a.pending_events().unwrap()
            .into_iter().map(|(_, e)| e).collect();
        let events_b: Vec<Event> = device_b.pending_events().unwrap()
            .into_iter().map(|(_, e)| e).collect();

        device_a.replay_events(&events_b).unwrap();
        device_b.replay_events(&events_a).unwrap();

        let sessions_a = crate::db::list_sessions_from_db(&device_a).unwrap();
        let sessions_b = crate::db::list_sessions_from_db(&device_b).unwrap();
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
                mode: SessionMode::Timer, uuid: crate::db::SessionUuid::new(""),
                guided_file_uuid: None,
            }).unwrap();
        }
        let events: Vec<Event> = device_a.pending_events().unwrap()
            .into_iter().map(|(_, e)| e).collect();
        device_b.replay_events(&events).unwrap();
        let after_first = crate::db::list_sessions_from_db(&device_b).unwrap();
        device_b.replay_events(&events).unwrap();
        let after_second = crate::db::list_sessions_from_db(&device_b).unwrap();
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
            uuid: crate::db::SessionUuid::new(""),
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
        assert!(crate::db::list_labels_from_db(&db).unwrap().is_empty());
        // Session is present with the lamport-3 update's values, but
        // its label_id is NULL because the label has been deleted.
        let s = &crate::db::list_sessions_from_db(&db).unwrap()[0].1;
        assert_eq!(s.duration_secs, 900);
        assert_eq!(s.notes.as_deref(), Some("longer"));
        assert_eq!(s.label_id, None,
            "session keeps its data but loses the label link when the label tombstones");
        assert_eq!(db.get_setting("daily_goal", "x").unwrap(), "20");
    }

    #[test]
    fn replay_events_session_resolves_label_id_from_same_batch() {
        // Regression test for the recompute-dedup two-pass ordering:
        // a session_insert + a label_insert (same batch, no delete)
        // must end up with the session row's label_id pointing at
        // the label's local rowid. Because `recompute_session`
        // dereferences labels.uuid → labels.id during materialise,
        // labels must be materialised before sessions — the dedup
        // pass explicitly runs non-Session recomputes before Session
        // recomputes so this holds regardless of event order.
        let db = Database::open_in_memory().unwrap();
        let events = vec![
            // Session at higher lamport (causal-after-label), but
            // listed FIRST in the input so the sorter has work to do.
            synth_session_insert(
                SESSION_X, 10, DEVICE_A,
                "2026-05-13T10:00:00", 600,
                Some(LABEL_X), None, SessionMode::Timer,
            ),
            synth_label_insert(LABEL_X, 5, DEVICE_A, "Morning"),
        ];
        db.replay_events(&events).unwrap();

        let labels = crate::db::list_labels_from_db(&db).unwrap();
        assert_eq!(labels.len(), 1);
        let label = &labels[0];
        let s = &crate::db::list_sessions_from_db(&db).unwrap()[0].1;
        assert_eq!(s.label_id, Some(label.id),
            "session.label_id must resolve to the freshly-inserted label's rowid");
    }
}
