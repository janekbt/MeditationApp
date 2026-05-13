//! `session_in_progress` table — singleton row holding the running
//! state of the current in-flight meditation. Crash-recovery primitive:
//! a battery-death / OOM / panic mid-session leaves the row behind, and
//! the next launch finalises it (commits one `session_insert` event for
//! the captured `accumulated_secs`, clears the row) so the user's work
//! is preserved.
//!
//! Lives OUTSIDE the event log: writes here do NOT emit_event, so sync
//! sees nothing while a session is in progress. The trade-off is
//! device-local in-progress state (we don't sync "I am meditating
//! right now" across devices) — acceptable since the recovery
//! scenario is local-process death anyway.

use rusqlite::{params, OptionalExtension};

use super::{Database, DbError, Result, SessionMode};

/// Single-row snapshot of the in-flight meditation. The shell writes
/// this on session start + on a ~60s tick cadence + on state
/// transitions (pause/resume/mode-specific changes), then clears it
/// on normal completion inside the same transaction that records
/// the session. A crash between two writes preserves the latest
/// snapshot; the next launch finalises from that.
///
/// `start_iso` is when the session began (already-formatted local
/// ISO 8601). `accumulated_secs` is the elapsed-time counter that
/// freezes on pause and grows on resume — the only timing fact the
/// recovery flow needs. `mode_payload` is opaque JSON the shell
/// defines (mirrors the PresetConfig convention); core stores +
/// round-trips it without parsing. `label_id` is the local rowid
/// (device-local table, no cross-device translation needed at
/// snapshot time); finalize re-resolves it through `insert_session`
/// which emits a session_insert event with the corresponding
/// label_uuid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInProgress {
    pub start_iso: String,
    pub accumulated_secs: u32,
    pub mode: SessionMode,
    pub mode_payload: String,
    pub label_id: Option<i64>,
    pub guided_file_uuid: Option<String>,
}

impl Database {
    /// Upsert the single in-flight session row. Overwrites the prior
    /// row if any. Does NOT emit a sync event — the in-progress state
    /// is device-local and the only event sync ever sees for this
    /// session is the one `session_insert` that `finalize_session_in_progress`
    /// emits.
    pub fn set_session_in_progress(&self, snapshot: &SessionInProgress) -> Result<()> {
        self.conn.execute(
            "INSERT INTO session_in_progress
                (id, start_iso, accumulated_secs, mode, mode_payload, label_id, guided_file_uuid)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                start_iso        = excluded.start_iso,
                accumulated_secs = excluded.accumulated_secs,
                mode             = excluded.mode,
                mode_payload     = excluded.mode_payload,
                label_id         = excluded.label_id,
                guided_file_uuid = excluded.guided_file_uuid",
            params![
                snapshot.start_iso,
                snapshot.accumulated_secs as i64,
                snapshot.mode.as_db_str(),
                snapshot.mode_payload,
                snapshot.label_id,
                snapshot.guided_file_uuid,
            ],
        )?;
        Ok(())
    }

    /// Read the single in-flight session row, or `None` when no
    /// session is in progress (the common case on a clean shutdown).
    pub fn get_session_in_progress(&self) -> Result<Option<SessionInProgress>> {
        self.conn.query_row(
            "SELECT start_iso, accumulated_secs, mode, mode_payload, label_id, guided_file_uuid
             FROM session_in_progress
             WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            },
        )
        .optional()?
        .map(|(start_iso, accumulated_secs, mode_str, mode_payload, label_id, guided_file_uuid)| {
            let mode = SessionMode::from_db_str(&mode_str).ok_or_else(|| {
                DbError::Csv(format!(
                    "session_in_progress.mode has invalid value: {mode_str:?}"
                ))
            })?;
            Ok(SessionInProgress {
                start_iso,
                accumulated_secs: accumulated_secs as u32,
                mode,
                mode_payload,
                label_id,
                guided_file_uuid,
            })
        })
        .transpose()
    }

    /// Drop the in-flight session row. Idempotent — a no-op when the
    /// row is absent. Called on normal completion (clearing the
    /// snapshot in the same transaction that records the session)
    /// and as the second step of `finalize_session_in_progress`.
    pub fn clear_session_in_progress(&self) -> Result<()> {
        self.conn.execute(
            "DELETE FROM session_in_progress WHERE id = 1",
            [],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(secs: u32) -> SessionInProgress {
        SessionInProgress {
            start_iso: "2026-05-13T10:00:00".into(),
            accumulated_secs: secs,
            mode: SessionMode::Timer,
            mode_payload: r#"{"target_secs":600}"#.into(),
            label_id: None,
            guided_file_uuid: None,
        }
    }

    #[test]
    fn get_on_a_fresh_db_returns_none() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.get_session_in_progress().unwrap(), None);
    }

    #[test]
    fn set_then_get_round_trips_every_field() {
        let db = Database::open_in_memory().unwrap();
        let snapshot = SessionInProgress {
            start_iso: "2026-05-13T10:00:00".into(),
            accumulated_secs: 420,
            mode: SessionMode::BoxBreath,
            mode_payload: r#"{"pattern":[4,7,8,0]}"#.into(),
            label_id: None,
            guided_file_uuid: None,
        };
        db.set_session_in_progress(&snapshot).unwrap();
        let got = db.get_session_in_progress().unwrap().unwrap();
        assert_eq!(got, snapshot);
    }

    #[test]
    fn set_twice_upserts_overwriting_the_first_row() {
        // The CHECK (id = 1) keeps the row singleton; the UPSERT
        // overwrites rather than failing. Critical because the
        // shell calls set every ~60s + on state transitions.
        let db = Database::open_in_memory().unwrap();
        db.set_session_in_progress(&sample(60)).unwrap();
        db.set_session_in_progress(&sample(120)).unwrap();
        let got = db.get_session_in_progress().unwrap().unwrap();
        assert_eq!(got.accumulated_secs, 120);
        let count: i64 = db.conn
            .query_row("SELECT COUNT(*) FROM session_in_progress", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "upsert keeps the row singleton");
    }

    #[test]
    fn clear_drops_the_row() {
        let db = Database::open_in_memory().unwrap();
        db.set_session_in_progress(&sample(60)).unwrap();
        db.clear_session_in_progress().unwrap();
        assert_eq!(db.get_session_in_progress().unwrap(), None);
    }

    #[test]
    fn clear_on_an_already_empty_table_is_a_silent_noop() {
        // Called by the shell unconditionally on save/discard, so it
        // must not panic / error when the row is already absent.
        let db = Database::open_in_memory().unwrap();
        db.clear_session_in_progress()
            .expect("clear on empty is a no-op");
    }

    // ── Critical invariant: writes here do NOT emit events ──────────────────

    #[test]
    fn set_does_not_emit_a_sync_event() {
        // The whole point of the in-progress table living outside
        // the event log is that the ~60s tick cadence does NOT pump
        // sync events. A regression here would push ~one event per
        // minute per active session into pending_events, which the
        // sync layer would then upload — undermining both the
        // privacy posture (peers see "I am currently meditating")
        // and the data-shape contract (one event per completed
        // session, not many).
        let db = Database::open_in_memory().unwrap();
        let before = db.pending_events().unwrap().len();
        db.set_session_in_progress(&sample(60)).unwrap();
        let after = db.pending_events().unwrap().len();
        assert_eq!(before, after,
            "set_session_in_progress must not append to events");
    }

    #[test]
    fn get_does_not_emit_a_sync_event() {
        // Reads are intrinsically side-effect-free, but the test is
        // here so a future refactor that adds bookkeeping (e.g.
        // "track last-seen-at") cannot accidentally start emitting.
        let db = Database::open_in_memory().unwrap();
        db.set_session_in_progress(&sample(60)).unwrap();
        let before = db.pending_events().unwrap().len();
        let _ = db.get_session_in_progress().unwrap();
        let after = db.pending_events().unwrap().len();
        assert_eq!(before, after,
            "get_session_in_progress must not append to events");
    }

    #[test]
    fn clear_does_not_emit_a_sync_event() {
        let db = Database::open_in_memory().unwrap();
        db.set_session_in_progress(&sample(60)).unwrap();
        let before = db.pending_events().unwrap().len();
        db.clear_session_in_progress().unwrap();
        let after = db.pending_events().unwrap().len();
        assert_eq!(before, after,
            "clear_session_in_progress must not append to events");
    }

    #[test]
    fn full_set_get_clear_cycle_emits_zero_events() {
        // The composite invariant: nothing in the in-progress
        // lifecycle pumps the event log. Belt-and-braces with the
        // three per-op tests above so a future caller that bundles
        // operations under a wrapper still can't sneak emissions in.
        let db = Database::open_in_memory().unwrap();
        assert!(db.pending_events().unwrap().is_empty());
        db.set_session_in_progress(&sample(60)).unwrap();
        let _ = db.get_session_in_progress().unwrap();
        db.set_session_in_progress(&sample(120)).unwrap();
        let _ = db.get_session_in_progress().unwrap();
        db.clear_session_in_progress().unwrap();
        assert!(db.pending_events().unwrap().is_empty(),
            "no event must be emitted across the full lifecycle");
    }

    #[test]
    fn label_id_set_null_when_label_is_deleted() {
        // The schema declares REFERENCES labels(id) ON DELETE SET
        // NULL — a label tombstone arriving via sync while a session
        // is in progress mustn't leave the in-progress row pointing
        // at a dead rowid. PRAGMA foreign_keys=ON is set in
        // Database::open so this fires.
        let db = Database::open_in_memory().unwrap();
        // Insert a label directly (bypassing the public API which
        // would emit_event) so we have a rowid to reference.
        db.conn.execute(
            "INSERT INTO labels (name, uuid) VALUES (?1, ?2)",
            params!["Morning", "22222222-2222-4222-8222-222222222222"],
        ).unwrap();
        let label_id: i64 = db.conn
            .query_row(
                "SELECT id FROM labels WHERE uuid = ?1",
                params!["22222222-2222-4222-8222-222222222222"],
                |r| r.get(0),
            )
            .unwrap();
        let snapshot = SessionInProgress {
            label_id: Some(label_id),
            ..sample(60)
        };
        db.set_session_in_progress(&snapshot).unwrap();

        // Delete the label row directly; FK with SET NULL clears
        // the snapshot's label_id without touching anything else.
        db.conn.execute("DELETE FROM labels WHERE id = ?1", params![label_id]).unwrap();
        let got = db.get_session_in_progress().unwrap().unwrap();
        assert_eq!(got.label_id, None,
            "ON DELETE SET NULL must blank the dangling reference");
        assert_eq!(got.accumulated_secs, 60,
            "the rest of the row stays intact");
    }
}
