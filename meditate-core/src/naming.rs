//! Cross-entity name-validation predicate. Every shell-side
//! create/rename dialog (labels, presets, vibration patterns, sound
//! library, vibration editor) runs the same shape: trim the user
//! input, reject empty, reject case-insensitive collisions with the
//! existing rows of the same entity type. Per-entity collision
//! predicates already live in `db` (`is_label_name_taken`,
//! `is_preset_name_taken`, `is_vibration_pattern_name_taken`,
//! `is_guided_file_name_taken`); this module centralizes the
//! validation *shape* so every dialog dispatches off the same enum.

/// Validation outcome for a trimmed candidate name against an
/// existing entity library. The shell maps each variant to its UI:
/// disable the Save/Create button on `Empty` or `Collision`, show a
/// hint message on `Collision`, enable the button on `Ok`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameValidity {
    /// Trimmed name is empty — disable Save/Create.
    Empty,
    /// Case-insensitive duplicate of an existing entity. Disable
    /// Save/Create + surface the collision hint to the user.
    Collision,
    /// Name is acceptable — enable Save/Create.
    Ok,
}

/// Validate a (pre-trimmed) candidate against a collision check.
/// The shell passes a closure that calls the per-entity
/// `is_X_name_taken` DB predicate (most accept an `except_id` so
/// the Rename dialog doesn't flag the entity's current name as a
/// self-collision).
///
/// Returns `Empty` when `trimmed.is_empty()`; otherwise calls
/// `is_collision(trimmed)` and returns `Collision` or `Ok` from its
/// result. Callers are expected to have trimmed the input already
/// (gtk `EditableExt::text()` returns a `glib::GString`; the trim
/// happens at the call site).
pub fn validate(trimmed: &str, is_collision: impl FnOnce(&str) -> bool) -> NameValidity {
    if trimmed.is_empty() {
        return NameValidity::Empty;
    }
    if is_collision(trimmed) {
        NameValidity::Collision
    } else {
        NameValidity::Ok
    }
}

impl NameValidity {
    /// Convenience for the most common shell shape — "should the
    /// Save/Create button be enabled?". `true` only on `Ok`.
    pub fn is_savable(self) -> bool {
        matches!(self, NameValidity::Ok)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_returns_empty() {
        assert_eq!(validate("", |_| false), NameValidity::Empty);
        // The closure is short-circuited, never called on empty.
        let mut called = false;
        let _ = validate("", |_| { called = true; false });
        assert!(!called);
    }

    #[test]
    fn collision_short_circuits_when_closure_returns_true() {
        assert_eq!(validate("Sit", |_| true), NameValidity::Collision);
    }

    #[test]
    fn ok_when_non_empty_and_no_collision() {
        assert_eq!(validate("Fresh Name", |_| false), NameValidity::Ok);
    }

    #[test]
    fn is_savable_only_for_ok_variant() {
        assert!(NameValidity::Ok.is_savable());
        assert!(!NameValidity::Empty.is_savable());
        assert!(!NameValidity::Collision.is_savable());
    }

    #[test]
    fn collision_closure_receives_the_trimmed_input() {
        let mut seen = String::new();
        let _ = validate("Sitting", |s| {
            seen = s.to_string();
            false
        });
        assert_eq!(seen, "Sitting");
    }
}
