//! Pure validation helpers for the sound-import flow shared between
//! shells. The actual file-chooser dialog, transcoding pipeline, and
//! `gtk::MediaFile` playback stay in the shell — this module covers
//! the gating predicates and the small bit of metadata derivation
//! that any shell's import path needs.

use crate::db::BellSound;

/// Shared preview-toggle state machine. The bell-sound chooser's
/// current shell-side `play_preview` / `stop_preview` pair (mono
/// `gtk::MediaFile` slot) is the thin equivalent; promoting it to
/// `PreviewToggle` gives the typed toggle / supersede / auto-revert
/// protocol the vibration chooser already uses, and one shape both
/// the gtk and Android shells can share.
pub use crate::preview::{PreviewAction, PreviewToggle};

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
    "wav", "ogg", "mp3", "opus", "flac", "m4a", "aac", "mp4",
];

/// MIME filter for the Android SAF picker (`EXTRA_MIME_TYPES`, with
/// the intent's own type set to `*/*`). `audio/*` alone greys out
/// files Android labels otherwise: an mp4 with a video track is
/// `video/mp4` (MediaStore sniffs the content), and API 26–28 label
/// `.ogg` as `application/ogg`. Every extension in
/// `IMPORTABLE_EXTENSIONS` must be admitted under every label
/// Android gives it — pinned by the tests.
pub const ANDROID_PICKER_MIME_TYPES: &[&str] = &[
    "audio/*", "video/mp4", "application/ogg",
];

/// True when `mime` matches `pattern`, where `pattern` is a concrete
/// type (`video/mp4`) or a wildcard-subtype one (`audio/*`, `*/*`).
/// Case-insensitive, surrounding whitespace ignored; anything that
/// isn't a non-empty `type/subtype` never matches.
pub fn mime_matches(pattern: &str, mime: &str) -> bool {
    fn split(s: &str) -> Option<(String, String)> {
        let (ty, sub) = s.trim().split_once('/')?;
        if ty.is_empty() || sub.is_empty() {
            return None;
        }
        Some((ty.to_ascii_lowercase(), sub.to_ascii_lowercase()))
    }
    let (Some((pty, psub)), Some((mty, msub))) = (split(pattern), split(mime)) else {
        return false;
    };
    (pty == "*" || pty == mty) && (psub == "*" || psub == msub)
}

/// Why a picked or imported audio file can't be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioFileError {
    /// The file opened fine but carries no audio stream (e.g. a
    /// video-only mp4). Each shell maps this to its own translated
    /// message.
    NoAudioTrack,
    /// Anything else; the message is for Diagnostics, not the user.
    Other(String),
}

/// Any plain error message is an `Other` — lets shell code that
/// builds a pipeline with `String` errors use `?` directly.
impl From<String> for AudioFileError {
    fn from(msg: String) -> Self {
        Self::Other(msg)
    }
}

/// Stable wire code for `AudioFileError::NoAudioTrack` in the
/// Android drop-files (`guided_import_result` as `err:<code>`, and
/// the 4th line of `guided_pick` / `sound_pick`).
pub const NO_AUDIO_TRACK_CODE: &str = "no-audio-track";

/// Parse the Android import-worker drop-file: `ok` or `err:<msg>`.
pub fn parse_import_result(raw: &str) -> Result<(), AudioFileError> {
    let trimmed = raw.trim();
    if trimmed == "ok" {
        return Ok(());
    }
    let msg = trimmed.strip_prefix("err:").unwrap_or(trimmed);
    if msg == NO_AUDIO_TRACK_CODE {
        Err(AudioFileError::NoAudioTrack)
    } else {
        Err(AudioFileError::Other(msg.to_string()))
    }
}

/// A file the Android picker copied into app storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickedFile {
    pub path: String,
    pub display_name: String,
    pub duration_secs: u32,
}

/// Parse the Android picker drop-file: path / display name /
/// duration secs, plus an optional 4th line that is
/// `NO_AUDIO_TRACK_CODE` when the probe opened the file and found no
/// audio stream. `None` when the path line is missing or blank. Only
/// the exact marker rejects — a failed probe writes nothing there and
/// must not block a possibly playable file.
pub fn parse_pick(raw: &str) -> Option<Result<PickedFile, AudioFileError>> {
    let mut lines = raw.lines().map(str::trim);
    let path = lines.next().filter(|p| !p.is_empty())?.to_string();
    let display_name = lines.next().unwrap_or("").to_string();
    let duration_secs = lines.next().and_then(|d| d.parse().ok()).unwrap_or(0);
    if lines.next() == Some(NO_AUDIO_TRACK_CODE) {
        return Some(Err(AudioFileError::NoAudioTrack));
    }
    Some(Ok(PickedFile { path, display_name, duration_secs }))
}

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

    // ── mp4 import (issue #2) ────────────────────────────────────────

    #[test]
    fn importable_extensions_include_mp4() {
        assert!(IMPORTABLE_EXTENSIONS.contains(&"mp4"));
    }

    #[test]
    fn mp4_is_transcoded_to_ogg_never_copied() {
        assert!(!is_passthrough_ext("mp4"));
        assert!(!is_passthrough_ext("MP4"));
        assert_eq!(target_extension_and_mime("mp4"), ("ogg", "audio/ogg"));
        assert_eq!(target_extension_and_mime("Mp4"), ("ogg", "audio/ogg"));
    }

    #[test]
    fn importable_extensions_are_lowercase_without_dot_or_duplicates() {
        for (i, ext) in IMPORTABLE_EXTENSIONS.iter().enumerate() {
            assert_eq!(*ext, ext.to_ascii_lowercase(), "{ext} not lowercase");
            assert!(!ext.starts_with('.'), "{ext} has a leading dot");
            assert!(!ext.is_empty());
            assert!(
                !IMPORTABLE_EXTENSIONS[..i].contains(ext),
                "{ext} listed twice",
            );
        }
    }

    #[test]
    fn mime_matches_wildcard_subtype() {
        assert!(mime_matches("audio/*", "audio/mp4"));
        assert!(mime_matches("audio/*", "audio/mpeg"));
        assert!(!mime_matches("audio/*", "video/mp4"));
        assert!(!mime_matches("audio/*", "audiox/mp4"));
    }

    #[test]
    fn mime_matches_exact_type() {
        assert!(mime_matches("video/mp4", "video/mp4"));
        assert!(!mime_matches("video/mp4", "video/ogg"));
        assert!(!mime_matches("video/mp4", "video/mp4x"));
    }

    #[test]
    fn mime_matches_is_case_insensitive_and_trims() {
        assert!(mime_matches("audio/*", "AUDIO/MP4"));
        assert!(mime_matches("Video/MP4", "video/mp4"));
        assert!(mime_matches("audio/*", " audio/ogg "));
    }

    #[test]
    fn mime_matches_rejects_malformed_input() {
        assert!(!mime_matches("audio/*", ""));
        assert!(!mime_matches("audio/*", "audio"));
        assert!(!mime_matches("audio/*", "audio/"));
        assert!(!mime_matches("", "audio/mp4"));
        assert!(!mime_matches("*/*", "garbage"));
    }

    #[test]
    fn mime_matches_full_wildcard() {
        assert!(mime_matches("*/*", "video/mp4"));
        assert!(mime_matches("*/*", "application/ogg"));
    }

    #[test]
    fn android_picker_mime_types_are_well_formed() {
        assert!(!ANDROID_PICKER_MIME_TYPES.is_empty());
        for (i, m) in ANDROID_PICKER_MIME_TYPES.iter().enumerate() {
            let (ty, sub) = m.split_once('/').expect("type/subtype");
            assert!(!ty.is_empty() && !sub.is_empty(), "{m}");
            assert_ne!(ty, "*", "{m}: a */* entry would admit every file");
            assert!(
                !ANDROID_PICKER_MIME_TYPES[..i].contains(m),
                "{m} listed twice",
            );
        }
    }

    /// How Android labels each importable extension, across the
    /// API levels we support (26–35). Sources: AOSP libcore
    /// `MimeUtils.java` (oreo-release), `android.mime.types`
    /// (android10-release) and `external/mime-support/mime.types`,
    /// plus on-device MediaStore readings on the FP5 (Android 15,
    /// 2026-09-27), which sniffs content: an mp4 WITH a video track
    /// is `video/mp4` even though it carries importable audio.
    const ANDROID_LABELS: &[(&str, &[&str])] = &[
        ("wav", &["audio/x-wav", "audio/wav"]),
        ("ogg", &["audio/ogg", "application/ogg"]),
        ("mp3", &["audio/mpeg"]),
        ("opus", &["audio/ogg", "audio/opus"]),
        ("flac", &["audio/flac"]),
        ("m4a", &["audio/mp4", "audio/mpeg"]),
        ("aac", &["audio/aac", "audio/aac-adts"]),
        ("mp4", &["video/mp4", "audio/mp4"]),
    ];

    #[test]
    fn android_labels_table_covers_every_importable_extension() {
        for ext in IMPORTABLE_EXTENSIONS {
            assert!(
                ANDROID_LABELS.iter().any(|(e, _)| e == ext),
                "no Android label table entry for {ext}",
            );
        }
    }

    #[test]
    fn android_picker_admits_every_label_of_every_importable_extension() {
        for (ext, labels) in ANDROID_LABELS {
            for label in *labels {
                assert!(
                    ANDROID_PICKER_MIME_TYPES
                        .iter()
                        .any(|p| mime_matches(p, label)),
                    "{ext} labelled {label} would be greyed out in the picker",
                );
            }
        }
    }

    #[test]
    fn android_picker_does_not_admit_unrelated_types() {
        for other in ["video/x-matroska", "image/png", "text/csv", "application/pdf"] {
            assert!(
                !ANDROID_PICKER_MIME_TYPES.iter().any(|p| mime_matches(p, other)),
                "{other} must stay greyed out",
            );
        }
    }

    #[test]
    fn plain_error_message_converts_to_other() {
        let e: AudioFileError = String::from("create vorbisenc: missing").into();
        assert_eq!(e, AudioFileError::Other("create vorbisenc: missing".into()));
    }

    // ── import result drop-file ──────────────────────────────────────

    #[test]
    fn import_result_ok() {
        assert_eq!(parse_import_result("ok"), Ok(()));
        assert_eq!(parse_import_result("ok\n"), Ok(()));
        assert_eq!(parse_import_result("  ok  "), Ok(()));
    }

    #[test]
    fn import_result_no_audio_track_is_typed() {
        assert_eq!(
            parse_import_result(&format!("err:{NO_AUDIO_TRACK_CODE}")),
            Err(AudioFileError::NoAudioTrack),
        );
        assert_eq!(
            parse_import_result(&format!("err:{NO_AUDIO_TRACK_CODE}\n")),
            Err(AudioFileError::NoAudioTrack),
        );
    }

    #[test]
    fn import_result_other_error_keeps_message() {
        assert_eq!(
            parse_import_result("err:Import needs Android 10+"),
            Err(AudioFileError::Other("Import needs Android 10+".into())),
        );
    }

    #[test]
    fn import_result_without_prefix_is_other_error() {
        assert_eq!(
            parse_import_result("something odd"),
            Err(AudioFileError::Other("something odd".into())),
        );
    }

    #[test]
    fn import_result_empty_is_other_error() {
        assert_eq!(
            parse_import_result(""),
            Err(AudioFileError::Other(String::new())),
        );
    }

    // ── picker drop-file ─────────────────────────────────────────────

    #[test]
    fn pick_three_lines_parses_as_audio() {
        assert_eq!(
            parse_pick("/data/guided/transient.mp4\nTalk.mp4\n185"),
            Some(Ok(PickedFile {
                path: "/data/guided/transient.mp4".into(),
                display_name: "Talk.mp4".into(),
                duration_secs: 185,
            })),
        );
    }

    #[test]
    fn pick_with_audio_marker_parses() {
        let got = parse_pick("/p/t.mp4\nTalk.mp4\n185\naudio\n");
        assert_eq!(got.unwrap().unwrap().duration_secs, 185);
    }

    #[test]
    fn pick_with_no_audio_marker_is_no_audio_track() {
        assert_eq!(
            parse_pick(&format!("/p/t.mp4\nClip.mp4\n0\n{NO_AUDIO_TRACK_CODE}")),
            Some(Err(AudioFileError::NoAudioTrack)),
        );
    }

    #[test]
    fn pick_unknown_fourth_line_is_treated_as_audio() {
        // Only the exact marker rejects: a failed or unfamiliar probe
        // must never block a file that may well be playable.
        assert!(matches!(parse_pick("/p/t.mp3\nA\n3\nwhatever"), Some(Ok(_))));
    }

    #[test]
    fn pick_trims_whitespace() {
        let got = parse_pick("  /p/t.mp3 \n  Name \n 7 \n").unwrap().unwrap();
        assert_eq!(got.path, "/p/t.mp3");
        assert_eq!(got.display_name, "Name");
        assert_eq!(got.duration_secs, 7);
    }

    #[test]
    fn pick_bad_or_missing_duration_is_zero() {
        assert_eq!(parse_pick("/p/t.mp3\nA\nabc").unwrap().unwrap().duration_secs, 0);
        assert_eq!(parse_pick("/p/t.mp3\nA").unwrap().unwrap().duration_secs, 0);
        assert_eq!(parse_pick("/p/t.mp3").unwrap().unwrap().display_name, "");
    }

    #[test]
    fn pick_empty_or_blank_path_is_none() {
        assert_eq!(parse_pick(""), None);
        assert_eq!(parse_pick("\nName\n3"), None);
        assert_eq!(parse_pick("   \nName\n3"), None);
    }
}
