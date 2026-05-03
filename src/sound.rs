use std::cell::RefCell;
use gtk::prelude::*;

const RESOURCE_BASE: &str = "/io/github/janekbt/Meditate/sounds";

thread_local! {
    /// Keeps the currently-playing (or pre-warmed) MediaFile alive.
    static CURRENT_MEDIA: RefCell<Option<gtk::MediaFile>> = const { RefCell::new(None) };
    /// Holds the active starting-bell MediaFile so the playback isn't
    /// dropped before the bell finishes. Kept separate from
    /// CURRENT_MEDIA so playing the starting bell doesn't clobber the
    /// pre-warmed end-sound MediaFile that lives there.
    static STARTING_MEDIA: RefCell<Option<gtk::MediaFile>> = const { RefCell::new(None) };
}

/// Stop whatever is currently playing (no-op if nothing is).
pub fn stop_current() {
    CURRENT_MEDIA.with(|cell| {
        if let Some(m) = cell.replace(None) {
            m.set_playing(false);
        }
    });
}

/// Play a `file://` URI. Stops any previous sound first.
pub fn play_uri(uri: &str) -> gtk::MediaFile {
    let media = gtk::MediaFile::for_file(&gtk::gio::File::for_uri(uri));
    swap_and_play(media.clone());
    media
}

/// Play a bundled GResource path. Stops any previous sound first.
pub fn play_resource(path: &str) -> gtk::MediaFile {
    let media = gtk::MediaFile::for_resource(path);
    swap_and_play(media.clone());
    media
}

/// Convenience wrapper: play one of the bundled sounds by name.
pub fn play_bundled(name: &str) -> gtk::MediaFile {
    play_resource(&format!("{RESOURCE_BASE}/{name}.wav"))
}

/// Construct the MediaFile for the configured end sound and cache it.
///
/// Called at startup and whenever the sound setting changes. We deliberately
/// do NOT start playback here — earlier attempts to "pre-warm" the pipeline
/// with `set_playing(true)` + idle-pause could produce an audible click on
/// some PipeWire configurations because playback starts before the idle pause
/// fires. The first `play_end_sound()` call pays a small cold-start delay
/// (~200 ms), which is imperceptible at the end of a meditation session.
pub fn preload_end_sound(app: &crate::application::MeditateApplication) {
    let sound = app
        .with_db(|db| db.get_setting("end_sound", "bowl"))
        .and_then(|r| r.ok())
        .unwrap_or_else(|| "bowl".to_string());

    let media_opt: Option<gtk::MediaFile> = match sound.as_str() {
        "bowl" | "bell" | "gong" => Some(
            gtk::MediaFile::for_resource(&format!("{RESOURCE_BASE}/{sound}.wav"))
        ),
        "custom" => app
            .with_db(|db| db.get_setting("end_sound_path", ""))
            .and_then(|r| r.ok())
            .filter(|p| !p.is_empty())
            .map(|p| gtk::MediaFile::for_file(
                &gtk::gio::File::for_uri(&format!("file://{p}"))
            )),
        _ => None,
    };

    // Replace any previously cached media; stop the old one if it was playing.
    CURRENT_MEDIA.with(|cell| {
        if let Some(old) = cell.replace(media_opt) {
            old.set_playing(false);
        }
    });
}

/// Play the configured end sound. Reuses the pre-warmed pipeline from
/// `preload_end_sound()` if available, falling back to a cold start.
pub fn play_end_sound(app: &crate::application::MeditateApplication) {
    // Try to resume the pre-warmed pipeline — no cold-start delay.
    let reused = CURRENT_MEDIA.with(|cell| {
        if let Some(m) = cell.borrow().as_ref() {
            m.seek(0);
            m.set_playing(true);
            true
        } else {
            false
        }
    });
    if reused { return; }

    // No pre-loaded pipeline (sound is "none", or preload hasn't run yet).
    let sound = app
        .with_db(|db| db.get_setting("end_sound", "bowl"))
        .and_then(|r| r.ok())
        .unwrap_or_else(|| "bowl".to_string());
    match sound.as_str() {
        "bowl" | "bell" | "gong" => { play_bundled(&sound); }
        "custom" => {
            if let Some(path) = app
                .with_db(|db| db.get_setting("end_sound_path", ""))
                .and_then(|r| r.ok())
                .filter(|p| !p.is_empty())
            {
                play_uri(&format!("file://{path}"));
            }
        }
        _ => {}
    }
}

/// Map a bundled bell key ("bowl" / "bell" / "gong") to its full GResource
/// path. Returns `None` for any unrecognised key — callers can treat that
/// as "no bell" and skip playback. Pure fn so it's covered in unit tests.
///
/// B.4 broadens this to UUIDs plus custom files; for now, the same three
/// names the existing Completion-Sound combo uses.
pub fn bundled_bell_resource_path(sound: &str) -> Option<String> {
    match sound {
        "bowl" | "bell" | "gong" => Some(format!("{RESOURCE_BASE}/{sound}.wav")),
        _ => None,
    }
}

/// Play the starting bell at session start. No-op if the user has the
/// Starting Bell switch off, or if the configured sound key isn't a
/// recognised bundled bell. Uses STARTING_MEDIA so the existing
/// end-sound preload in CURRENT_MEDIA isn't disturbed.
pub fn play_starting_sound(app: &crate::application::MeditateApplication) {
    let active = app
        .with_db(|db| db.get_setting("starting_bell_active", "false"))
        .and_then(|r| r.ok())
        .map(|v| v == "true")
        .unwrap_or(false);
    if !active {
        return;
    }
    let sound = app
        .with_db(|db| db.get_setting("starting_bell_sound", "bowl"))
        .and_then(|r| r.ok())
        .unwrap_or_else(|| "bowl".to_string());
    let Some(resource) = bundled_bell_resource_path(&sound) else {
        return;
    };
    let media = gtk::MediaFile::for_resource(&resource);
    STARTING_MEDIA.with(|cell| {
        if let Some(old) = cell.replace(Some(media.clone())) {
            old.set_playing(false);
        }
    });
    media.set_playing(true);
}

// ── Internal ──────────────────────────────────────────────────────────────────

fn swap_and_play(media: gtk::MediaFile) {
    CURRENT_MEDIA.with(|cell| {
        let old = cell.replace(Some(media.clone()));
        if let Some(m) = old {
            m.set_playing(false);
        }
    });
    media.set_playing(true);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_bell_path_resolves_for_each_known_key() {
        assert_eq!(
            bundled_bell_resource_path("bowl"),
            Some("/io/github/janekbt/Meditate/sounds/bowl.wav".to_string())
        );
        assert_eq!(
            bundled_bell_resource_path("bell"),
            Some("/io/github/janekbt/Meditate/sounds/bell.wav".to_string())
        );
        assert_eq!(
            bundled_bell_resource_path("gong"),
            Some("/io/github/janekbt/Meditate/sounds/gong.wav".to_string())
        );
    }

    #[test]
    fn bundled_bell_path_is_none_for_unknown_keys() {
        assert_eq!(bundled_bell_resource_path("none"), None);
        assert_eq!(bundled_bell_resource_path(""), None);
        assert_eq!(bundled_bell_resource_path("custom"), None);
        assert_eq!(bundled_bell_resource_path("BOWL"), None);
    }
}
