//! Per-mode label state: which label rolls into the next session's
//! saved row (when the master toggle is on) and the user's per-mode
//! choice of which label that is. Persisted in the `settings` table
//! under the keys from `settings_keys` so every shell agrees on
//! placement; the Guided-default rule and the resolve-via-uuid
//! fallback chain live here so the Android shell doesn't have to
//! re-implement them.

use crate::db::{Database, Label, SessionMode};
use crate::settings_keys::{
    default_label_uuid_for_mode, format_bool, label_active_key_for_mode,
    label_uuid_key_for_mode, parse_bool,
};

/// Whether the active-label master toggle is on for `mode`. Guided
/// defaults to ON when the setting is absent — a fresh user almost
/// always wants their Guided sessions tagged with the seeded "Guided
/// Meditation" label, and it's a hassle to flip the toggle every
/// visit. Timer and Box-Breath default OFF: their label rows ship
/// disabled so a fresh user isn't surprised by a default tag they
/// didn't ask for. Persisted user choice always wins on subsequent
/// visits because `get_setting` only falls back here when the key is
/// missing entirely.
pub fn label_active_from_db(db: &Database, mode: SessionMode) -> bool {
    let fallback = mode == SessionMode::Guided;
    db.get_setting(label_active_key_for_mode(mode), format_bool(fallback))
        .map(|v| parse_bool(&v))
        .unwrap_or(fallback)
}

/// Read the persisted label UUID for `mode`. `None` when the setting
/// is missing or empty — callers fall back to `default_label_uuid_
/// for_mode` (or, if even the seeded default row was deleted, treat
/// the label as un-resolvable).
pub fn label_uuid_from_db(db: &Database, mode: SessionMode) -> Option<String> {
    let val = db.get_setting(label_uuid_key_for_mode(mode), "").ok()?;
    if val.is_empty() {
        None
    } else {
        Some(val)
    }
}

/// Persist the master-toggle state for `mode`. Errors fall through —
/// callers usually `let _ =` since failure here only means the next
/// launch shows the prior value; no data loss.
pub fn persist_active_for_mode(
    db: &Database,
    mode: SessionMode,
    on: bool,
) -> crate::db::Result<()> {
    db.set_setting(label_active_key_for_mode(mode), format_bool(on))
}

/// Persist the per-mode label UUID. `set_setting` upserts.
pub fn persist_uuid_for_mode(
    db: &Database,
    mode: SessionMode,
    uuid: &str,
) -> crate::db::Result<()> {
    db.set_setting(label_uuid_key_for_mode(mode), uuid)
}

/// Resolve the label row currently associated with `mode`: the
/// user's persisted choice if any, else the mode's seeded default,
/// else `None` when even the default row has been deleted by the
/// user. `list_labels` walks happen here so callers don't repeat the
/// "fetch list + find by uuid" pattern.
pub fn resolve_label_for_mode(db: &Database, mode: SessionMode) -> Option<Label> {
    let uuid = label_uuid_from_db(db, mode)
        .unwrap_or_else(|| default_label_uuid_for_mode(mode).to_string());
    if uuid.is_empty() {
        return None;
    }
    crate::db::list_labels_from_db(db)
        .ok()?
        .into_iter()
        .find(|l| l.uuid == uuid)
}

/// What the Done-screen "user picked this label" rule resolves to,
/// given the per-session pick and the current labels list.
///
/// The shell observes one of these and applies it via the
/// `persist_*` helpers (NoOp does nothing).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PersistAction {
    /// User picked an existing label — persist its uuid as the new
    /// per-mode default and flip the master toggle on.
    SetUuidAndActivate { uuid: crate::db::LabelUuid },
    /// User cleared the per-session pick — flip the master toggle off
    /// so the next session starts un-labelled too. UUID is left as
    /// the prior value so re-enabling the toggle restores intent.
    Deactivate,
    /// User picked a label id that no longer exists in the labels
    /// list (a race against a peer's delete-sync). Shell shouldn't
    /// touch persisted state — the next visit will resolve through
    /// the existing fallback chain.
    NoOp,
}

/// Typed key for the delete-label dialog's body copy. The shell
/// maps each variant to its gettext template — "Sessions tagged
/// '{name}': {n}" for `InUse`, "'{name}' isn't used by any saved
/// session yet" for `Unused`. Owns the policy "do we surface the
/// session count?" so every shell agrees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteImpactKey {
    /// The label is currently attached to one or more saved
    /// sessions. Shell shows the count so the user knows what's
    /// about to be untagged.
    InUse(i64),
    /// No saved session references the label yet.
    Unused,
}

/// Pick the typed key the shell renders in the "Delete label?"
/// confirmation dialog. Positive count → `InUse(n)` ("12 sessions
/// will be untagged"); zero or negative → `Unused`. Stays out of
/// the shell so the count→variant boundary is unit-testable.
pub fn delete_impact_key(session_count: i64) -> DeleteImpactKey {
    if session_count > 0 {
        DeleteImpactKey::InUse(session_count)
    } else {
        DeleteImpactKey::Unused
    }
}

/// Resolve the persist action for a Done-screen save:
/// - `picked = Some(id)` + the id exists in `labels` →
///   `SetUuidAndActivate` with that label's uuid.
/// - `picked = Some(id)` + the id is missing → `NoOp`.
/// - `picked = None` (user toggled the row off on the Done screen)
///   → `Deactivate`.
pub fn resolve_persist_action(picked: Option<i64>, labels: &[Label]) -> PersistAction {
    match picked {
        Some(id) => labels
            .iter()
            .find(|l| l.id == id)
            .map(|l| PersistAction::SetUuidAndActivate { uuid: l.uuid.clone() })
            .unwrap_or(PersistAction::NoOp),
        None => PersistAction::Deactivate,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Label;

    fn label(id: i64, uuid: &str, name: &str) -> Label {
        Label {
            id,
            uuid: uuid.into(),
            name: name.into(),
        }
    }

    #[test]
    fn label_active_from_db_defaults_to_off_for_timer_and_box_breath() {
        let db = Database::open_in_memory().unwrap();
        assert!(!label_active_from_db(&db, SessionMode::Timer));
        assert!(!label_active_from_db(&db, SessionMode::BoxBreath));
    }

    #[test]
    fn label_active_from_db_defaults_to_on_for_guided() {
        let db = Database::open_in_memory().unwrap();
        assert!(label_active_from_db(&db, SessionMode::Guided));
    }

    #[test]
    fn label_active_from_db_reads_back_the_user_override() {
        let db = Database::open_in_memory().unwrap();
        persist_active_for_mode(&db, SessionMode::Timer, true).unwrap();
        assert!(label_active_from_db(&db, SessionMode::Timer));
        persist_active_for_mode(&db, SessionMode::Guided, false).unwrap();
        assert!(!label_active_from_db(&db, SessionMode::Guided));
    }

    #[test]
    fn label_uuid_from_db_is_none_when_empty_or_missing() {
        let db = Database::open_in_memory().unwrap();
        assert!(label_uuid_from_db(&db, SessionMode::Timer).is_none());
        persist_uuid_for_mode(&db, SessionMode::Timer, "").unwrap();
        assert!(label_uuid_from_db(&db, SessionMode::Timer).is_none());
    }

    #[test]
    fn label_uuid_from_db_round_trips_through_settings() {
        let db = Database::open_in_memory().unwrap();
        persist_uuid_for_mode(&db, SessionMode::Timer, "custom-uuid").unwrap();
        assert_eq!(
            label_uuid_from_db(&db, SessionMode::Timer).as_deref(),
            Some("custom-uuid"),
        );
    }

    #[test]
    fn resolve_persist_action_returns_set_for_existing_id() {
        let labels = vec![label(7, "u-7", "Sit")];
        let action = resolve_persist_action(Some(7), &labels);
        assert_eq!(
            action,
            PersistAction::SetUuidAndActivate { uuid: "u-7".into() }
        );
    }

    #[test]
    fn resolve_persist_action_returns_deactivate_for_none() {
        assert_eq!(resolve_persist_action(None, &[]), PersistAction::Deactivate);
    }

    #[test]
    fn delete_impact_key_routes_at_zero_boundary() {
        assert_eq!(delete_impact_key(0), DeleteImpactKey::Unused);
        assert_eq!(delete_impact_key(1), DeleteImpactKey::InUse(1));
        assert_eq!(delete_impact_key(42), DeleteImpactKey::InUse(42));
        assert_eq!(delete_impact_key(-1), DeleteImpactKey::Unused, "no negative counts in practice");
    }

    #[test]
    fn resolve_persist_action_returns_noop_for_missing_id() {
        let labels = vec![label(1, "u-1", "Walk")];
        assert_eq!(resolve_persist_action(Some(999), &labels), PersistAction::NoOp);
    }

}
