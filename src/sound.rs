use std::cell::RefCell;
use gtk::prelude::*;
use gtk::glib;

const RESOURCE_BASE: &str = "/io/github/janekbt/Meditate/sounds";

thread_local! {
    /// Keeps the currently-playing (or pre-warmed) MediaFile alive.
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

/// Pre-warm the GStreamer pipeline and audio-server (PipeWire/PulseAudio)
/// connection for the configured end sound.
///
/// Call this at startup and whenever the sound setting changes.  The
/// pipeline is left in PAUSED state with the audio server connected, so
/// the actual end-of-session playback starts without a cold-start delay.
pub fn preload_end_sound(app: &crate::application::MeditateApplication) {
    let sound = app
        .with_db(|db| db.get_setting("end_sound", "none"))
        .and_then(|r| r.ok())
        .unwrap_or_else(|| "none".to_string());

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

    // Replace any previously pre-loaded media.
    CURRENT_MEDIA.with(|cell| {
        if let Some(old) = cell.replace(media_opt.clone()) {
            old.set_playing(false);
        }
    });

    if let Some(media) = media_opt {
        // Kick the GStreamer pipeline into life (NULL → READY → PAUSED/PLAYING).
        // The READY transition opens the audio-server connection. We queue an
        // immediate pause via an idle callback so GStreamer settles in PAUSED
        // with the connection open — no audio ever reaches the speaker, but
        // subsequent set_playing(true) calls are instant.
        media.set_playing(true);
        let m = media.clone();
        glib::idle_add_local_once(move || {
            m.set_playing(false);
        });
    }
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
