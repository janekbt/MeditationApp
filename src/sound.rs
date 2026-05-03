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
    /// Vec of in-flight interval-bell MediaFiles. Two bells whose
    /// boundaries land on the same second both play to completion —
    /// a single slot would clip the first, which would silently hide
    /// from the user that two cues collided. Each MediaFile removes
    /// itself from this vec via notify::ended once playback finishes.
    /// stop_all drains the vec on Save/Discard. Kept separate from
    /// CURRENT_MEDIA and STARTING_MEDIA so in-session bells don't
    /// clobber the completion-sound preload or the starting bell.
    static INTERVAL_MEDIA: RefCell<Vec<gtk::MediaFile>> = const { RefCell::new(Vec::new()) };
    /// Single slot for the bell-sound chooser's per-row preview.
    /// Mono playback (one preview at a time) — tapping a different
    /// row's play button stops the previous. Kept separate from the
    /// in-session slots so a preview in Settings can't fight with a
    /// running session's audio.
    static PREVIEW_MEDIA: RefCell<Option<gtk::MediaFile>> = const { RefCell::new(None) };
}

/// Stop whatever is currently playing in CURRENT_MEDIA (no-op if
/// nothing is). Used by preferences-page sound previews to stop the
/// previous preview before playing the next.
pub fn stop_current() {
    CURRENT_MEDIA.with(|cell| {
        if let Some(m) = cell.replace(None) {
            m.set_playing(false);
        }
    });
}

/// Stop every session-related sound — the end-sound slot
/// (CURRENT_MEDIA), the starting-bell slot (STARTING_MEDIA), and the
/// interval-bell slot (INTERVAL_MEDIA). Called from Save / Discard
/// on the Done page so a bell still playing through doesn't outlast
/// the user's choice to leave.
pub fn stop_all() {
    stop_current();
    STARTING_MEDIA.with(|cell| {
        if let Some(m) = cell.replace(None) {
            m.set_playing(false);
        }
    });
    INTERVAL_MEDIA.with(|cell| {
        for m in cell.borrow_mut().drain(..) {
            m.set_playing(false);
        }
    });
    PREVIEW_MEDIA.with(|cell| {
        if let Some(m) = cell.replace(None) {
            m.set_playing(false);
        }
    });
}

/// Play one of the bundled or custom sounds as a preview. Used by
/// the bell-sound chooser's per-row Play button. Mono — a new
/// preview stops the previous, so users can scrub through a list
/// without stacking sounds. `is_bundled=true` treats `path` as a
/// GResource path; `false` treats it as a filesystem path.
pub fn play_preview(path: &str, is_bundled: bool) {
    let media = if is_bundled {
        gtk::MediaFile::for_resource(path)
    } else {
        gtk::MediaFile::for_file(&gtk::gio::File::for_path(path))
    };
    PREVIEW_MEDIA.with(|cell| {
        if let Some(old) = cell.replace(Some(media.clone())) {
            old.set_playing(false);
        }
    });
    media.set_playing(true);
}

/// Stop the active preview, if any. Called by the chooser page
/// when it pops so a preview doesn't outlast the user's choice.
pub fn stop_preview() {
    PREVIEW_MEDIA.with(|cell| {
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

/// Play one of the bundled bells by direct sound key
/// ("bowl" / "bell" / "gong"). Used by the running tick to fire
/// interval / fixed-time bells. Each call appends to the
/// INTERVAL_MEDIA vec so two bells whose boundaries land on the
/// same second both play through — clipping one would silently
/// hide the collision from the user. The MediaFile removes itself
/// from the vec via notify::ended once playback finishes.
/// No-op for unrecognised keys.
pub fn play_interval_sound(sound: &str) {
    let Some(resource) = bundled_bell_resource_path(sound) else {
        return;
    };
    let media = gtk::MediaFile::for_resource(&resource);

    // Self-removal on completion so the vec doesn't accumulate over a
    // long session. PartialEq on glib::Object compares by pointer
    // identity, which is exactly what we need to find the entry.
    media.connect_ended_notify(|m| {
        if m.is_ended() {
            INTERVAL_MEDIA.with(|cell| {
                cell.borrow_mut().retain(|x| x != m);
            });
        }
    });

    INTERVAL_MEDIA.with(|cell| cell.borrow_mut().push(media.clone()));
    media.set_playing(true);
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
