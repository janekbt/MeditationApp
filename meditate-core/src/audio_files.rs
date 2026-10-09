//! Where a sound file lives on THIS device.
//!
//! A bell-sound or guided-file row stores the `file_path` of the device
//! that created it, and sync copies that path to every peer — so on any
//! other device it points nowhere, and for the bundled bells it ends up
//! as whichever platform's path won the last sync. Playing, previewing
//! or deleting a file therefore never uses the stored path: custom
//! files live in this device's sounds / guided folder under their
//! uuid (the layout imports and the sync pull both write), and bundled
//! bells come from the shell's own built-in table.

use std::path::{Path, PathBuf};

use crate::db::BellSound;

/// Where a bell sound's audio is on this device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BellSoundFile<B> {
    /// A bundled bell, at the location the shell's built-in table
    /// gives (a GResource path on the desktop, an extracted file on
    /// Android).
    Bundled(B),
    /// A custom bell, imported here or pulled by sync.
    Local(PathBuf),
}

/// Resolve a bell sound to its file on this device. `bundled` maps a
/// bundled uuid to the shell's built-in location; `None` from it (a
/// bundled bell this build doesn't ship) means there is no file.
pub fn bell_sound_file<B>(
    sound: &BellSound,
    sounds_dir: &Path,
    bundled: impl FnOnce(&str) -> Option<B>,
) -> Option<BellSoundFile<B>> {
    if sound.is_bundled {
        bundled(sound.uuid.as_str()).map(BellSoundFile::Bundled)
    } else {
        Some(BellSoundFile::Local(custom_sound_path(
            sounds_dir,
            sound.uuid.as_str(),
            sound.extension(),
        )))
    }
}

/// `<sounds_dir>/<uuid>.<ext>` — where a custom bell's audio lives.
pub fn custom_sound_path(sounds_dir: &Path, uuid: &str, ext: &str) -> PathBuf {
    sounds_dir.join(format!("{uuid}.{ext}"))
}

/// `<guided_dir>/<uuid>.ogg` — where a guided file's audio lives.
pub fn guided_file_path(guided_dir: &Path, uuid: &str) -> PathBuf {
    guided_dir.join(format!("{uuid}.ogg"))
}

/// Remove a custom bell's audio, `<sounds_dir>/<uuid>.*`, after its
/// row is gone. Matched by uuid: the stored `file_path` may be another
/// device's. Best effort, a missing file or folder is fine.
pub fn remove_sound_files(sounds_dir: &Path, uuid: &str) {
    let Ok(entries) = std::fs::read_dir(sounds_dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.file_stem().and_then(|s| s.to_str()) == Some(uuid) {
            let _ = std::fs::remove_file(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_sound_files_takes_only_that_uuids_files() {
        let dir = tempfile::tempdir().unwrap();
        for name in [format!("{CUSTOM_UUID}.wav"), format!("{CUSTOM_UUID}.ogg"), format!("{CUSTOM_UUID}x.wav"), "other.wav".into()] {
            std::fs::write(dir.path().join(name), b"A").unwrap();
        }
        remove_sound_files(dir.path(), CUSTOM_UUID);
        let mut left: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(left, vec![format!("{CUSTOM_UUID}x.wav"), "other.wav".to_string()]);
        remove_sound_files(&dir.path().join("missing"), CUSTOM_UUID);
    }
    use crate::db::{BellSoundCategory, Database, Event};
    use crate::seeds::{BUNDLED_BELL_UUID, BUNDLED_BOWL_UUID};
    use std::path::{Path, PathBuf};

    const CUSTOM_UUID: &str = "abcdabcd-1111-4222-8333-444455556666";
    const GUIDED_UUID: &str = "cdefcdef-1111-4222-8333-444455556666";

    fn custom_sound(file_path: &str, mime: &str) -> crate::db::BellSound {
        let db = Database::open_in_memory().unwrap();
        db.insert_bell_sound_with_uuid(
            CUSTOM_UUID, "Mine", file_path, false, mime, BellSoundCategory::General,
        )
        .unwrap();
        db.list_bell_sounds().unwrap().into_iter().find(|b| b.uuid == CUSTOM_UUID).unwrap()
    }

    fn bundled_sound(file_path: &str) -> crate::db::BellSound {
        let db = Database::open_in_memory().unwrap();
        db.seed_bell_sounds_with_paths(&[(BUNDLED_BOWL_UUID, "Bowl", file_path, "audio/ogg")])
            .unwrap();
        db.list_bell_sounds().unwrap().into_iter().find(|b| b.uuid == BUNDLED_BOWL_UUID).unwrap()
    }

    /// A shell's bundled table: uuid → its own built-in location.
    fn phone_bundled(uuid: &str) -> Option<PathBuf> {
        let file = match uuid {
            BUNDLED_BOWL_UUID => "bowl.ogg",
            BUNDLED_BELL_UUID => "bell.ogg",
            _ => return None,
        };
        Some(Path::new("/phone/sounds").join(file))
    }

    fn desktop_bundled(uuid: &str) -> Option<String> {
        match uuid {
            BUNDLED_BOWL_UUID => Some("/io/github/janekbt/Meditate/sounds/bowl.ogg".into()),
            BUNDLED_BELL_UUID => Some("/io/github/janekbt/Meditate/sounds/bell.ogg".into()),
            _ => None,
        }
    }

    // ── custom bell sounds ──────────────────────────────────────────

    #[test]
    fn custom_sound_lives_in_this_devices_sounds_dir_by_uuid() {
        let sound = custom_sound("/home/someone/.local/share/meditate/sounds/x.ogg", "audio/ogg");
        assert_eq!(
            bell_sound_file(&sound, Path::new("/phone/sounds"), phone_bundled),
            Some(BellSoundFile::Local(PathBuf::from(format!("/phone/sounds/{CUSTOM_UUID}.ogg")))),
        );
    }

    #[test]
    fn custom_sound_ignores_an_empty_stored_path() {
        let sound = custom_sound("", "audio/ogg");
        assert_eq!(
            bell_sound_file(&sound, Path::new("/d"), phone_bundled),
            Some(BellSoundFile::Local(PathBuf::from(format!("/d/{CUSTOM_UUID}.ogg")))),
        );
    }

    #[test]
    fn custom_sound_keeps_its_own_extension() {
        let mp3 = custom_sound("/elsewhere/a.mp3", "audio/mpeg");
        assert_eq!(
            bell_sound_file(&mp3, Path::new("/d"), phone_bundled),
            Some(BellSoundFile::Local(PathBuf::from(format!("/d/{CUSTOM_UUID}.mp3")))),
        );
    }

    #[test]
    fn custom_sound_path_matches_the_sync_layout() {
        assert_eq!(
            custom_sound_path(Path::new("/d"), CUSTOM_UUID, "wav"),
            PathBuf::from(format!("/d/{CUSTOM_UUID}.wav")),
        );
    }

    #[test]
    fn custom_sound_never_asks_the_bundled_table() {
        let sound = custom_sound("/x.ogg", "audio/ogg");
        let got = bell_sound_file(&sound, Path::new("/d"), |_| -> Option<PathBuf> {
            panic!("a custom sound must not be looked up as bundled")
        });
        assert!(matches!(got, Some(BellSoundFile::Local(_))));
    }

    // ── bundled bell sounds ─────────────────────────────────────────

    #[test]
    fn bundled_sound_comes_from_this_shells_table_not_the_stored_path() {
        // The stored path is the OTHER platform's (what a sync can
        // leave behind); the resolver must not care.
        let sound = bundled_sound("/io/github/janekbt/Meditate/sounds/bowl.ogg");
        assert_eq!(
            bell_sound_file(&sound, Path::new("/phone/sounds"), phone_bundled),
            Some(BellSoundFile::Bundled(PathBuf::from("/phone/sounds/bowl.ogg"))),
        );
        let sound = bundled_sound("/data/user/0/io.github.janekbt.Meditate/files/meditate/sounds/bowl.ogg");
        assert_eq!(
            bell_sound_file(&sound, Path::new("/unused"), desktop_bundled),
            Some(BellSoundFile::Bundled("/io/github/janekbt/Meditate/sounds/bowl.ogg".to_string())),
        );
    }

    #[test]
    fn bundled_sound_is_looked_up_by_its_uuid() {
        let sound = bundled_sound("whatever");
        let mut asked = None;
        let _ = bell_sound_file(&sound, Path::new("/d"), |uuid| {
            asked = Some(uuid.to_string());
            Some(())
        });
        assert_eq!(asked.as_deref(), Some(BUNDLED_BOWL_UUID));
    }

    #[test]
    fn bundled_sound_this_shell_does_not_ship_has_no_file() {
        let sound = bundled_sound("/somewhere/bowl.ogg");
        assert_eq!(bell_sound_file(&sound, Path::new("/d"), |_| None::<PathBuf>), None);
    }

    // ── guided files ────────────────────────────────────────────────

    #[test]
    fn guided_file_lives_in_this_devices_guided_dir_by_uuid() {
        assert_eq!(
            guided_file_path(Path::new("/phone/guided"), GUIDED_UUID),
            PathBuf::from(format!("/phone/guided/{GUIDED_UUID}.ogg")),
        );
    }

    // ── the sync case end to end ────────────────────────────────────

    fn events(db: &Database) -> Vec<Event> {
        db.pending_events().unwrap().into_iter().map(|(_, e)| e).collect()
    }

    fn bowl(db: &Database) -> crate::db::BellSound {
        db.list_bell_sounds().unwrap().into_iter().find(|b| b.uuid == BUNDLED_BOWL_UUID).unwrap()
    }

    #[test]
    fn bundled_sounds_resolve_locally_on_both_devices_after_a_sync() {
        // Each device seeds the bundled rows with its own path; after
        // they exchange events one path wins on BOTH rows, so one
        // device's stored path points at the other platform. Whichever
        // wins, each device must still find its own built-in file.
        let desktop = Database::open_in_memory().unwrap();
        desktop
            .seed_bell_sounds_with_paths(&[(
                BUNDLED_BOWL_UUID, "Bowl", "/io/github/janekbt/Meditate/sounds/bowl.ogg", "audio/ogg",
            )])
            .unwrap();
        let phone = Database::open_in_memory().unwrap();
        phone
            .seed_bell_sounds_with_paths(&[(
                BUNDLED_BOWL_UUID, "Bowl", "/phone/sounds/bowl.ogg", "audio/ogg",
            )])
            .unwrap();
        let (from_desktop, from_phone) = (events(&desktop), events(&phone));
        desktop.replay_events(&from_phone).unwrap();
        phone.replay_events(&from_desktop).unwrap();
        assert_eq!(bowl(&desktop).file_path, bowl(&phone).file_path, "sync converged on one path");

        assert_eq!(
            bell_sound_file(&bowl(&phone), Path::new("/phone/sounds"), phone_bundled),
            Some(BellSoundFile::Bundled(PathBuf::from("/phone/sounds/bowl.ogg"))),
        );
        assert_eq!(
            bell_sound_file(&bowl(&desktop), Path::new("/unused"), desktop_bundled),
            Some(BellSoundFile::Bundled("/io/github/janekbt/Meditate/sounds/bowl.ogg".to_string())),
        );
    }

    #[test]
    fn a_custom_sound_from_a_peer_resolves_into_this_devices_sounds_dir() {
        let laptop = Database::open_in_memory().unwrap();
        laptop
            .insert_bell_sound_with_uuid(
                CUSTOM_UUID,
                "Mine",
                "/home/someone/.local/share/meditate/sounds/mine.ogg",
                false,
                "audio/ogg",
                BellSoundCategory::General,
            )
            .unwrap();
        let phone = Database::open_in_memory().unwrap();
        phone.replay_events(&events(&laptop)).unwrap();
        let synced = phone
            .list_bell_sounds()
            .unwrap()
            .into_iter()
            .find(|b| b.uuid == CUSTOM_UUID)
            .unwrap();
        assert_eq!(
            bell_sound_file(&synced, Path::new("/phone/sounds"), phone_bundled),
            Some(BellSoundFile::Local(PathBuf::from(format!("/phone/sounds/{CUSTOM_UUID}.ogg")))),
        );
    }
}
