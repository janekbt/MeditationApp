use std::cell::RefCell;
use gtk::prelude::*;

const RESOURCE_BASE: &str = "/io/github/janekbt/Meditate/sounds";

thread_local! {
    /// Keeps the currently-playing MediaFile alive for the duration of playback.
    static CURRENT_MEDIA: RefCell<Option<gtk::MediaFile>> = RefCell::new(None);
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
/// Returns the MediaFile so the caller can connect to `notify::playing` etc.
pub fn play_uri(uri: &str) -> gtk::MediaFile {
    let media = gtk::MediaFile::for_file(&gtk::gio::File::for_uri(uri));
    swap_and_play(media.clone());
    media
}

/// Play a bundled GResource path (e.g. "/io/github/janekbt/Meditate/sounds/bowl.wav").
/// Stops any previous sound first. Returns the MediaFile.
pub fn play_resource(path: &str) -> gtk::MediaFile {
    let media = gtk::MediaFile::for_resource(path);
    swap_and_play(media.clone());
    media
}

/// Convenience wrapper: play one of the three bundled sounds by name.
pub fn play_bundled(name: &str) -> gtk::MediaFile {
    play_resource(&format!("{RESOURCE_BASE}/{name}.wav"))
}

/// Read the configured end-sound from the DB and play it.
/// Called when a countdown reaches zero.
pub fn play_end_sound(app: &crate::application::MeditateApplication) {
    let sound = app
        .with_db(|db| db.get_setting("end_sound", "none"))
        .and_then(|r| r.ok())
        .unwrap_or_else(|| "none".to_string());

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
