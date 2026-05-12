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

/// Audio extensions the importer accepts at the file-picker filter
/// level. Pinned across shells so the gtk file dialog and an
/// eventual Android SAF MIME filter agree on the same allow-list,
/// and the `do_import_io` transcode branch (passthrough vs.
/// transcode-to-ogg) can be derived from the same source via
/// `is_passthrough_ext`. Lowercase, no leading dot.
pub const IMPORTABLE_EXTENSIONS: &[&str] = &[
    "wav", "ogg", "mp3", "opus", "flac", "m4a", "aac",
];

/// True when the importer should copy the source file as-is rather
/// than transcoding to ogg/vorbis. `gtk::MediaFile` plays both
/// `wav` and `ogg` natively on every runtime we ship to; everything
/// else routes through the gstreamer pipeline. Case-insensitive on
/// the source extension.
pub fn is_passthrough_ext(ext: &str) -> bool {
    let lower = ext.to_ascii_lowercase();
    matches!(lower.as_str(), "wav" | "ogg")
}

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

/// Copy a source file to a destination that **must not exist yet**.
/// `std::fs::copy` follows symlinks on both ends — if an attacker
/// pre-plants a symlink at `dest` pointing to a sensitive file
/// (e.g. `~/.bashrc`), `fs::copy` overwrites the link's target with
/// the source bytes. The threat is small in practice because the
/// destination uuid is freshly minted v4 — but flatpak's
/// `--filesystem=home` puts shared user dirs in scope, and the
/// fix is free.
///
/// `O_CREAT | O_EXCL` (Rust's `create_new(true)`) guarantees the
/// open fails if the path already exists, defending against the
/// pre-planted-symlink case. `O_NOFOLLOW` adds defense against a
/// TOCTOU window where the destination doesn't exist at check time
/// but appears as a symlink before the open. Belt and braces.
///
/// Returns the number of bytes copied on success.
#[cfg(unix)]
pub fn safe_copy_no_follow(
    source: &std::path::Path,
    dest: &std::path::Path,
) -> std::io::Result<u64> {
    use std::fs::OpenOptions;
    use std::io::{Read, Write};
    use std::os::unix::fs::OpenOptionsExt;

    let mut src = std::fs::File::open(source)?;
    let mut dst = OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(dest)?;
    let mut buf = [0u8; 64 * 1024];
    let mut total: u64 = 0;
    loop {
        let n = src.read(&mut buf)?;
        if n == 0 { break; }
        dst.write_all(&buf[..n])?;
        total += n as u64;
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::BellSoundCategory;

    #[test]
    fn is_passthrough_ext_accepts_wav_ogg_only() {
        assert!(is_passthrough_ext("wav"));
        assert!(is_passthrough_ext("ogg"));
        assert!(is_passthrough_ext("WAV"), "case-insensitive");
        assert!(is_passthrough_ext("Ogg"), "case-insensitive");
        assert!(!is_passthrough_ext("mp3"));
        assert!(!is_passthrough_ext("flac"));
        assert!(!is_passthrough_ext("m4a"));
        assert!(!is_passthrough_ext(""));
    }

    #[test]
    fn importable_extensions_includes_all_known_audio_formats() {
        for ext in ["wav", "ogg", "mp3", "opus", "flac", "m4a", "aac"] {
            assert!(IMPORTABLE_EXTENSIONS.contains(&ext), "missing {ext}");
        }
    }

    #[test]
    fn target_extension_passthrough_matches_is_passthrough_ext() {
        for &ext in IMPORTABLE_EXTENSIONS {
            let (out_ext, _) = target_extension_and_mime(ext);
            assert_eq!(
                is_passthrough_ext(ext),
                out_ext == ext,
                "{ext}: passthrough predicate must agree with target_extension",
            );
        }
    }

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

    #[cfg(unix)]
    #[test]
    fn safe_copy_no_follow_writes_source_bytes_to_fresh_destination() {
        // Happy path: dest doesn't exist, copy goes through, bytes match.
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("in.wav");
        let dest = dir.path().join("out.wav");
        std::fs::write(&source, b"hello world").unwrap();

        let n = safe_copy_no_follow(&source, &dest).unwrap();

        assert_eq!(n, 11);
        assert_eq!(std::fs::read(&dest).unwrap(), b"hello world");
    }

    #[cfg(unix)]
    #[test]
    fn safe_copy_no_follow_refuses_when_destination_is_a_symlink() {
        // Attack scenario: an attacker (or a confused other process)
        // pre-plants a symlink at the destination uuid path pointing
        // at a sensitive file. std::fs::copy would follow the link
        // and overwrite the target. safe_copy_no_follow must refuse.
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("payload.wav");
        let victim = dir.path().join("victim.txt");
        let dest = dir.path().join("dest.wav");
        std::fs::write(&source, b"attacker bytes").unwrap();
        std::fs::write(&victim, b"sensitive original contents").unwrap();
        std::os::unix::fs::symlink(&victim, &dest).unwrap();

        let result = safe_copy_no_follow(&source, &dest);

        assert!(result.is_err(), "copy through a symlink at dest must fail");
        // Victim's contents are unchanged.
        assert_eq!(
            std::fs::read(&victim).unwrap(),
            b"sensitive original contents",
            "victim file pointed to by the dest symlink must be untouched",
        );
    }

    #[cfg(unix)]
    #[test]
    fn safe_copy_no_follow_refuses_when_destination_is_a_regular_file() {
        // Defensive: even a non-symlink destination must not be
        // clobbered. The importer mints a fresh uuid so dest should
        // never exist; if it does, that's a bug somewhere and we'd
        // rather error than overwrite.
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("in.wav");
        let dest = dir.path().join("dest.wav");
        std::fs::write(&source, b"new").unwrap();
        std::fs::write(&dest, b"existing").unwrap();

        let result = safe_copy_no_follow(&source, &dest);

        assert!(result.is_err(), "existing destination must not be clobbered");
        assert_eq!(std::fs::read(&dest).unwrap(), b"existing");
    }
}
