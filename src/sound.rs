use std::cell::RefCell;
use gtk::prelude::*;

const RESOURCE_BASE: &str = "/io/github/janekbt/Meditate/sounds";

thread_local! {
    /// Keeps the currently-playing (or pre-warmed) MediaFile alive.
    static CURRENT_MEDIA: RefCell<Option<gtk::MediaFile>> = const { RefCell::new(None) };
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
