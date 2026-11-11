//! Names of presets, labels, guided files, bell sounds and vibration
//! patterns: the rule a stored name follows (`clean`, used inside
//! every user-facing write) and the name field both apps' dialogs
//! run on (`NameField`).

/// A user-typed name as it is stored: trimmed, and refused when
/// that leaves nothing.
pub(crate) fn clean(name: &str) -> crate::db::Result<&str> {
    match name.trim() {
        "" => Err(crate::db::DbError::EmptyName),
        trimmed => Ok(trimmed),
    }
}

/// What a name field says under itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameNote {
    None,
    /// "This name is already in use".
    Taken,
    /// "Save failed": the write failed for another reason.
    SaveFailed,
}

/// The typed name of a name dialog (or the pattern editor's Name
/// field): whether Save is on, and what shows under the field.
/// Whether the name is free is only a hint while typing; the write
/// decides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameField {
    pub text: String,
    /// The name it opened with: a rename back to it is never a clash,
    /// even while another item shares that name (bell sounds can).
    initial: String,
    pub note: NameNote,
}

impl NameField {
    /// A create or import, opened with `text` (empty, or the picked
    /// file's name): every name is checked, the opening one too.
    pub fn new(text: &str, taken: impl FnOnce(&str) -> bool) -> Self {
        let mut field = Self { text: String::new(), initial: String::new(), note: NameNote::None };
        field.changed(text, taken);
        field
    }

    /// A rename, opened with the item's own name, which is never a
    /// clash with itself.
    pub fn renaming(old: &str, taken: impl FnOnce(&str) -> bool) -> Self {
        let mut field = Self::new("", |_| false);
        field.initial = old.trim().to_string();
        field.changed(old, taken);
        field
    }

    /// The user typed: `taken` tells whether another item holds the
    /// trimmed name (a failed read answers false).
    pub fn changed(&mut self, text: &str, taken: impl FnOnce(&str) -> bool) {
        self.text = text.to_string();
        let name = self.name();
        self.note = if !name.is_empty() && name != self.initial && taken(name) { NameNote::Taken } else { NameNote::None };
    }

    /// The name as it will be stored.
    pub fn name(&self) -> &str {
        self.text.trim()
    }

    /// Save is on unless the name is empty or taken.
    pub fn can_save(&self) -> bool {
        !self.name().is_empty() && self.note != NameNote::Taken
    }

    /// The write's result. True when the dialog closes; otherwise it
    /// says why and keeps the name. A name taken meanwhile turns Save
    /// off; any other failure, or no database (`None`), leaves it on
    /// for a retry. A failure goes to the diagnostics log under `tag`.
    pub fn confirm(&mut self, tag: &str, result: Option<Result<(), crate::db::DbError>>) -> bool {
        match &result {
            Some(Err(e)) => crate::log(tag, &format!("failed: {e:?}")),
            None => crate::log(tag, "failed: no database"),
            Some(Ok(())) => {}
        }
        match result {
            Some(Ok(())) => true,
            Some(Err(e)) if e.is_name_taken() => {
                self.note = NameNote::Taken;
                false
            }
            Some(Err(_)) | None => {
                self.note = NameNote::SaveFailed;
                false
            }
        }
    }
}

#[cfg(test)]
mod name_tests {
    use super::{NameField, NameNote};
    use crate::db::DbError;

    #[test]
    fn a_taken_name_turns_save_off_and_says_why() {
        let taken = |n: &str| n.eq_ignore_ascii_case("Pulse");
        // A rename opens with its own name: Save on, nothing said.
        let mut field = NameField::renaming("Gong", taken);
        assert!(field.can_save());
        assert_eq!(field.note, NameNote::None);
        field.changed("pulse ", taken);
        assert!(!field.can_save());
        assert_eq!(field.note, NameNote::Taken);
        field.changed("Heartbeat", taken);
        assert!(field.can_save());
        assert_eq!(field.note, NameNote::None);
        // Empty: Save off, no hint.
        field.changed("   ", taken);
        assert!(!field.can_save());
        assert_eq!(field.note, NameNote::None);
        // A create opens empty.
        assert!(!NameField::new("", taken).can_save());
        // An import opens with the file's name, and it is checked.
        let import = NameField::new("Pulse", taken);
        assert!(!import.can_save());
        assert_eq!(import.note, NameNote::Taken);
        // A rename of one of two sounds called "Pulse": its own name is
        // no clash, the same name in other case is.
        let mut twin = NameField::renaming("Pulse", taken);
        assert!(twin.can_save());
        assert_eq!(twin.note, NameNote::None);
        twin.changed("pulse", taken);
        assert_eq!(twin.note, NameNote::Taken);
    }

    #[test]
    fn a_failed_save_keeps_the_name_dialog_open() {
        let mut field = NameField::new("", |_| false);
        field.changed("Walk", |_| false);
        // The database can't be written: open, the name kept, Save on.
        assert!(!field.confirm("test", Some(Err(DbError::Decode("database is locked".into())))));
        assert_eq!(field.text, "Walk");
        assert_eq!(field.note, NameNote::SaveFailed);
        assert!(field.can_save());
        // Taken meanwhile (a sync): the hint, Save off.
        assert!(!field.confirm("test", Some(Err(DbError::DuplicateLabel("Walk".into())))));
        assert_eq!(field.text, "Walk");
        assert_eq!(field.note, NameNote::Taken);
        assert!(!field.can_save());
        // No database at all fails like any write.
        field.changed("Stroll", |_| false);
        assert!(!field.confirm("test", None));
        assert_eq!(field.note, NameNote::SaveFailed);
        // Another name, and the retry works.
        field.changed("Stroll", |_| false);
        assert!(field.confirm("test", Some(Ok(()))));
    }

    #[test]
    fn the_pattern_editor_says_a_taken_name() {
        let taken = |n: &str| n.eq_ignore_ascii_case("Pulse");
        let name = NameField::new("Pulse copy", taken);
        assert!(name.can_save());
        let mut name = NameField::new("PULSE", taken);
        assert!(!name.can_save());
        assert_eq!(name.note, NameNote::Taken);
        // Checked afresh, a typed name is never its own exemption.
        assert_eq!(NameField::new("Pulse", taken).note, NameNote::Taken);
        name.changed("Wave", taken);
        assert!(!name.confirm("test", Some(Err(DbError::DuplicateVibrationPattern("Wave".into())))));
        assert_eq!(name.note, NameNote::Taken);
    }
}

#[cfg(test)]
mod tests {
    use crate::db::{BellSoundCategory, ChartKind, Database, DbError, SessionMode};

    /// The stored name of every kind, read back by its id.
    fn names(db: &Database, label: i64, preset: &str, guided: &str, bell: &str, pattern: &str) -> [String; 5] {
        [
            crate::db::list_labels_from_db(db).unwrap().into_iter().find(|l| l.id == label).unwrap().name,
            crate::db::find_preset_by_uuid_from_db(db, preset).unwrap().unwrap().name,
            crate::db::find_guided_file_by_uuid_from_db(db, guided).unwrap().unwrap().name,
            db.list_bell_sounds().unwrap().into_iter().find(|b| b.uuid == bell).unwrap().name,
            crate::db::find_vibration_pattern_by_uuid_from_db(db, pattern).unwrap().unwrap().name,
        ]
    }

    #[test]
    fn every_kind_stores_a_name_without_outer_spaces() {
        let db = Database::open_in_memory().unwrap();
        let label = db.insert_label("  Walk  ").unwrap().id;
        db.insert_preset("  Walk  ", SessionMode::Timer, "{}").unwrap();
        let preset = crate::db::list_presets_from_db(&db).unwrap().into_iter().find(|p| p.name.contains("Walk")).unwrap().uuid;
        db.import_guided_file("g-1", "  Walk  ", "g.ogg", 60).unwrap();
        db.import_bell_sound("b-1", "  Walk  ", "/x/b.wav", "audio/wav", BellSoundCategory::General).unwrap();
        let pattern = db.insert_vibration_pattern("  Walk  ", 1000, &[0.5], ChartKind::Line, false).unwrap();
        assert_eq!(names(&db, label, preset.as_str(), "g-1", "b-1", &pattern), ["Walk", "Walk", "Walk", "Walk", "Walk"].map(String::from));

        // Each renamed to its own name in other case: not a clash with itself.
        db.update_label(label, "  walk  ").unwrap();
        db.update_preset_name(preset.as_str(), "  walk  ").unwrap();
        db.rename_guided_file("g-1", "  walk  ").unwrap();
        db.rename_bell_sound("b-1", "  walk  ").unwrap();
        db.update_vibration_pattern(&pattern, "  walk  ", 1000, &[0.5], ChartKind::Line).unwrap();
        assert_eq!(names(&db, label, preset.as_str(), "g-1", "b-1", &pattern), ["walk", "walk", "walk", "walk", "walk"].map(String::from));

        db.update_vibration_pattern(&pattern, "  Stride  ", 1000, &[0.5], ChartKind::Line).unwrap();
        assert_eq!(names(&db, label, preset.as_str(), "g-1", "b-1", &pattern)[4], "Stride");
    }

    #[test]
    fn every_kind_refuses_an_empty_name() {
        let db = Database::open_in_memory().unwrap();
        let empty = |r: crate::db::Result<()>| assert!(matches!(r, Err(DbError::EmptyName)), "{r:?}");
        empty(db.insert_label("   ").map(drop));
        empty(db.insert_preset("   ", SessionMode::Timer, "{}").map(drop));
        empty(db.import_guided_file("g-0", "   ", "g.ogg", 60).map(drop));
        empty(db.import_bell_sound("b-0", "   ", "/x/b.wav", "audio/wav", BellSoundCategory::General).map(drop));
        empty(db.insert_vibration_pattern("   ", 1000, &[0.5], ChartKind::Line, false).map(drop));
        assert!(crate::db::list_labels_from_db(&db).unwrap().is_empty());
        assert!(crate::db::list_presets_from_db(&db).unwrap().is_empty());
        assert!(crate::db::find_guided_file_by_uuid_from_db(&db, "g-0").unwrap().is_none());
        assert!(db.list_bell_sounds().unwrap().iter().all(|b| b.uuid != "b-0"));
        assert!(crate::db::list_vibration_patterns_from_db(&db).unwrap().iter().all(|p| p.is_bundled));

        let label = db.insert_label("Walk").unwrap().id;
        db.insert_preset("Walk", SessionMode::Timer, "{}").unwrap();
        let preset = crate::db::list_presets_from_db(&db).unwrap()[0].uuid.clone();
        db.import_guided_file("g-1", "Walk", "g.ogg", 60).unwrap();
        db.import_bell_sound("b-1", "Walk", "/x/b.wav", "audio/wav", BellSoundCategory::General).unwrap();
        let pattern = db.insert_vibration_pattern("Walk", 1000, &[0.5], ChartKind::Line, false).unwrap();
        empty(db.update_label(label, "  "));
        empty(db.update_preset_name(preset.as_str(), "  "));
        empty(db.rename_guided_file("g-1", "  "));
        empty(db.rename_bell_sound("b-1", "  "));
        empty(db.update_vibration_pattern(&pattern, "", 1000, &[0.5], ChartKind::Line).map(drop));
        assert_eq!(names(&db, label, preset.as_str(), "g-1", "b-1", &pattern), ["Walk", "Walk", "Walk", "Walk", "Walk"].map(String::from));
    }

}
