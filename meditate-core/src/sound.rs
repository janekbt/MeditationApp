//! Pure validation helpers for the sound-import flow shared between
//! shells. The actual file-chooser dialog, transcoding pipeline, and
//! `gtk::MediaFile` playback stay in the shell — this module covers
//! the gating predicates and the small bit of metadata derivation
//! that any shell's import path needs.

use crate::db::BellSound;

/// Cap on imported custom-bell file size. 10 MB is comfortably
/// larger than any reasonable transient bell sound and keeps the
/// data directory from growing without bound.
pub const MAX_CUSTOM_BELL_BYTES: u64 = 10 * 1024 * 1024;

/// True iff the given byte count fits under the custom-sound cap.
/// Sole gate at file-pick time before triggering the import dialog.
pub fn is_within_size_limit(bytes: u64) -> bool {
    bytes <= MAX_CUSTOM_BELL_BYTES
}

/// Pick a destination extension + MIME type for an incoming source
/// extension (case-insensitive). `wav` and `ogg` pass through
/// unchanged because `gtk::MediaFile` plays both natively on every
/// runtime we ship to; everything else converts to `ogg/vorbis` on
/// import.
pub fn target_extension_and_mime(source_ext: &str) -> (&'static str, &'static str) {
    match source_ext.to_ascii_lowercase().as_str() {
        "wav" => ("wav", "audio/wav"),
        "ogg" => ("ogg", "audio/ogg"),
        _ => ("ogg", "audio/ogg"),
    }
}

/// Case-insensitive name-collision check against an existing
/// bell-sound library. Trims `candidate` before comparing so a name
/// that's just `existing + " "` still counts as a collision.
pub fn name_collides(candidate: &str, existing: &[BellSound]) -> bool {
    let trimmed = candidate.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_lowercase();
    existing.iter().any(|s| s.name.to_lowercase() == lower)
}

/// Same as `name_collides` but skips a row by uuid — used by the
/// Rename flow so renaming a sound to the same name it already has
/// isn't a collision against itself.
pub fn name_collides_excluding(
    candidate: &str,
    existing: &[BellSound],
    exclude_uuid: &str,
) -> bool {
    let trimmed = candidate.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_lowercase();
    existing
        .iter()
        .any(|s| s.uuid != exclude_uuid && s.name.to_lowercase() == lower)
}

/// Pull a display name from an imported file's path. Uses the file
/// stem when it parses as UTF-8; falls back to a generic
/// "Custom sound" otherwise.
pub fn display_name_from_path(source_path: &std::path::Path) -> String {
    source_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Custom sound")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::BellSoundCategory;

    fn sound(uuid: &str, name: &str) -> BellSound {
        BellSound {
            id: 0,
            uuid: uuid.into(),
            name: name.into(),
            file_path: format!("sounds/{uuid}.ogg"),
            is_bundled: false,
            mime_type: "audio/ogg".into(),
            category: BellSoundCategory::General,
            created_iso: "1970-01-01T00:00:00".into(),
        }
    }

    #[test]
    fn size_limit_is_inclusive() {
        assert!(is_within_size_limit(MAX_CUSTOM_BELL_BYTES));
        assert!(is_within_size_limit(0));
        assert!(!is_within_size_limit(MAX_CUSTOM_BELL_BYTES + 1));
    }

    #[test]
    fn wav_and_ogg_pass_through_other_formats_become_ogg() {
        assert_eq!(target_extension_and_mime("wav"), ("wav", "audio/wav"));
        assert_eq!(target_extension_and_mime("WAV"), ("wav", "audio/wav"));
        assert_eq!(target_extension_and_mime("ogg"), ("ogg", "audio/ogg"));
        assert_eq!(target_extension_and_mime("mp3"), ("ogg", "audio/ogg"));
        assert_eq!(target_extension_and_mime("flac"), ("ogg", "audio/ogg"));
        assert_eq!(target_extension_and_mime("opus"), ("ogg", "audio/ogg"));
        assert_eq!(target_extension_and_mime("m4a"), ("ogg", "audio/ogg"));
    }

    #[test]
    fn name_collision_is_case_insensitive() {
        let lib = vec![sound("u1", "Tibetan Bowl"), sound("u2", "Chime")];
        assert!(name_collides("Tibetan Bowl", &lib));
        assert!(name_collides("tibetan bowl", &lib));
        assert!(name_collides("TIBETAN BOWL", &lib));
        assert!(name_collides("  Chime ", &lib), "whitespace-trim happens");
        assert!(!name_collides("Other", &lib));
    }

    #[test]
    fn empty_or_whitespace_candidate_never_collides() {
        let lib = vec![sound("u1", "Chime")];
        assert!(!name_collides("", &lib));
        assert!(!name_collides("   ", &lib));
        assert!(!name_collides("\t", &lib));
    }

    #[test]
    fn rename_can_keep_its_own_name() {
        let lib = vec![sound("u1", "Chime"), sound("u2", "Other")];
        // Renaming u1 to "Chime" must NOT collide against itself.
        assert!(!name_collides_excluding("Chime", &lib, "u1"));
        // But renaming u1 to "Other" still collides with u2.
        assert!(name_collides_excluding("Other", &lib, "u1"));
        // Renaming u3 (not in lib) to "Chime" does collide.
        assert!(name_collides_excluding("Chime", &lib, "u3"));
    }

    #[test]
    fn display_name_from_path_extracts_stem() {
        assert_eq!(
            display_name_from_path(std::path::Path::new("/tmp/bell-chime.wav")),
            "bell-chime"
        );
        assert_eq!(
            display_name_from_path(std::path::Path::new("/tmp/no-ext")),
            "no-ext"
        );
    }

    #[test]
    fn display_name_from_path_falls_back_for_pathless_input() {
        // An empty path has no file stem.
        assert_eq!(
            display_name_from_path(std::path::Path::new("")),
            "Custom sound"
        );
    }
}
