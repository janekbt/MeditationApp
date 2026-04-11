use std::cell::RefCell;
use gtk::prelude::*;

thread_local! {
    /// Keeps the currently-playing MediaFile alive until the next sound starts.
    static CURRENT_MEDIA: RefCell<Option<gtk::MediaFile>> = RefCell::new(None);
}

/// Play an arbitrary URI (file:// or resource://) via GTK4 MediaFile / GStreamer.
pub fn play_uri(uri: &str) {
    let file = gtk::gio::File::for_uri(uri);
    let media = gtk::MediaFile::for_file(&file);
    media.set_playing(true);
    CURRENT_MEDIA.with(|m| *m.borrow_mut() = Some(media));
}

/// Build the file:// URI for a bundled sound (bowl / bell / gong).
pub fn bundled_uri(name: &str) -> String {
    format!("file://{}/sounds/{}.wav", crate::config::PKGDATADIR, name)
}

/// Read the end-sound preference from the DB and play it.
/// Called automatically when a countdown reaches zero.
pub fn play_end_sound(app: &crate::application::MeditateApplication) {
    let sound = app
        .with_db(|db| db.get_setting("end_sound", "none"))
        .and_then(|r| r.ok())
        .unwrap_or_else(|| "none".to_string());

    match sound.as_str() {
        "bowl" | "bell" | "gong" => play_uri(&bundled_uri(&sound)),
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
