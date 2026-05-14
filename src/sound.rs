use std::cell::{Cell, RefCell};
use gtk::prelude::*;

use crate::application::MeditateApplication;
use crate::db::BellSound;
use crate::i18n::gettext;

thread_local! {
    /// Keeps the currently-playing (or pre-warmed) end-sound MediaFile alive.
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
    /// Process-lifetime latch: once an audio error has been surfaced
    /// to the user via toast, subsequent errors only hit the
    /// diagnostics log. Headphones unplugging then re-plugging
    /// shouldn't spam toasts; one notification per process tells the
    /// user "something's wrong with audio" without repeated yells.
    static AUDIO_ERROR_TOASTED: Cell<bool> = const { Cell::new(false) };
}

/// Stop whatever is currently playing in CURRENT_MEDIA (no-op if
/// nothing is). Kept for the existing preferences-page interactions
/// that haven't migrated to PREVIEW_MEDIA yet.
pub fn stop_current() {
    CURRENT_MEDIA.with(|cell| {
        if let Some(m) = cell.replace(None) {
            m.set_playing(false);
        }
    });
}

/// Stop every session-related sound — the end-sound slot
/// (CURRENT_MEDIA), the starting-bell slot (STARTING_MEDIA), the
/// interval-bell vec (INTERVAL_MEDIA), and the chooser preview
/// (PREVIEW_MEDIA). Called from Save / Discard on the Done page so
/// a bell still playing through doesn't outlast the user's choice
/// to leave.
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

/// Play a bell-sound row as a preview. Used by the bell-sound
/// chooser's per-row Play/Stop button. Mono — a new preview stops
/// the previous, so users can scrub through a list without stacking
/// sounds. Uses `media_for_bell_sound` so custom rows resolve to
/// the canonical local path derived from uuid+mime, not the stored
/// `file_path` (which is the importing device's absolute path and
/// doesn't resolve on a peer that synced the row).
///
/// Returns the MediaFile so the caller can connect a notify::playing
/// listener and revert its button icon when playback ends (whether
/// via user stop, end of file, or a different row's Play taking
/// over the slot).
pub fn play_preview(sound: &BellSound) -> gtk::MediaFile {
    let media = media_for_bell_sound(sound);
    PREVIEW_MEDIA.with(|cell| {
        if let Some(old) = cell.replace(Some(media.clone())) {
            old.set_playing(false);
        }
    });
    media.set_playing(true);
    media
}

/// Play a guided-meditation file as a preview. Used by the guided-
/// files chooser's per-row Play/Stop button. Shares the chooser
/// preview slot with bell-sound previews (the user only has one
/// chooser open at a time; if both somehow overlapped the supersede
/// semantics still hold).
///
/// Derives the local path from `uuid` rather than trusting the
/// stored `file_path` — the import path writes to
/// `$XDG_DATA_HOME/meditate/guided/<uuid>.ogg`, but the row's
/// `file_path` column is the *importing* device's absolute path at
/// that moment, which doesn't resolve cleanly after a sync from a
/// peer or after an XDG-dir relocation. Same trick as
/// `media_for_bell_sound`.
///
/// Returns the MediaFile so the caller can listen for notify::playing
/// transitions through the shared `PreviewToggle` and revert the
/// row's icon.
pub fn play_preview_for_guided_file(file: &crate::db::GuidedFile) -> gtk::MediaFile {
    let local_path = gtk::glib::user_data_dir()
        .join("meditate")
        .join("guided")
        .join(format!("{}.ogg", file.uuid));
    let media = gtk::MediaFile::for_file(&gtk::gio::File::for_path(&local_path));
    wire_audio_error_handler(&media, &file.name);
    PREVIEW_MEDIA.with(|cell| {
        if let Some(old) = cell.replace(Some(media.clone())) {
            old.set_playing(false);
        }
    });
    media.set_playing(true);
    media
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

/// Resolve a bell-sound UUID through the bell_sounds library.
/// Returns `None` if the uuid is empty or no row has it (e.g.,
/// post-wipe a stale "bowl" string would miss every UUID, and
/// playback silently no-ops — the user re-picks via the chooser).
fn lookup_bell_sound_by_uuid(app: &MeditateApplication, uuid: &str) -> Option<BellSound> {
    if uuid.is_empty() {
        return None;
    }
    app.with_db(super::db::Database::list_bell_sounds)
        .and_then(std::result::Result::ok)
        .unwrap_or_default()
        .into_iter()
        .find(|s| s.uuid == uuid)
}

/// Build a MediaFile from a BellSound row.
///
/// Bundled rows: `file_path` is a GResource path baked into every
/// device's binary, so we use it directly.
///
/// Custom rows: the stored `file_path` is the *importing* device's
/// absolute path, which doesn't resolve on a peer that synced the
/// row. We ignore it and derive the canonical local path from
/// `uuid + mime_type`. Every device that has the actual file (B.6
/// makes sure peers do, by pulling from WebDAV) finds it at the
/// same relative location.
fn media_for_bell_sound(sound: &BellSound) -> gtk::MediaFile {
    let media = if sound.is_bundled {
        gtk::MediaFile::for_resource(&sound.file_path)
    } else {
        let local_path = gtk::glib::user_data_dir()
            .join("meditate")
            .join("sounds")
            .join(format!("{}.{}", sound.uuid, sound.extension()));
        gtk::MediaFile::for_file(&gtk::gio::File::for_path(&local_path))
    };
    wire_audio_error_handler(&media, &sound.name);
    media
}

/// Hook a notify::error listener on the MediaFile so a broken audio
/// pipeline (output device disappeared mid-session, file format the
/// pipeline can't decode, etc.) reaches the diagnostics log and —
/// once per process — surfaces a toast to the user. Without this,
/// gtk::MediaFile errors are completely silent: the bell just doesn't
/// ring and the session continues as if nothing happened.
fn wire_audio_error_handler(media: &gtk::MediaFile, sound_name: &str) {
    let sound_name = sound_name.to_owned();
    media.connect_error_notify(move |m| {
        let Some(err) = m.error() else { return; };
        meditate_core::log(
            "sound.playback",
            &format!("error for '{sound_name}': {err}"),
        );
        AUDIO_ERROR_TOASTED.with(|c| {
            if c.get() {
                return;
            }
            c.set(true);
            surface_audio_error_toast();
        });
    });
}

fn surface_audio_error_toast() {
    let Some(app) = gtk::gio::Application::default() else { return; };
    let Ok(app) = app.downcast::<MeditateApplication>() else { return; };
    let Some(win) = app
        .active_window()
        .and_then(|w| w.downcast::<crate::window::MeditateWindow>().ok())
    else {
        return;
    };
    win.add_toast(
        adw::Toast::builder()
            .title(gettext("Couldn't play bell sound"))
            .timeout(6)
            .build(),
    );
}

/// Construct the MediaFile for the configured end bell and cache
/// it. Called at startup and whenever the sound setting changes.
/// We deliberately do NOT start playback here — earlier attempts to
/// "pre-warm" the pipeline with `set_playing(true)` + idle-pause
/// could produce an audible click on some PipeWire configurations
/// because playback starts before the idle pause fires. The first
/// `play_end_bell()` call pays a small cold-start delay (~200 ms),
/// which is imperceptible at the end of a meditation session.
pub fn preload_end_bell(app: &MeditateApplication) {
    let uuid = app
        .with_db(|db| db.get_setting("end_bell_sound", crate::db::BUNDLED_BOWL_UUID))
        .and_then(std::result::Result::ok)
        .unwrap_or_else(|| crate::db::BUNDLED_BOWL_UUID.to_string());
    let media_opt = lookup_bell_sound_by_uuid(app, &uuid).map(|s| media_for_bell_sound(&s));
    CURRENT_MEDIA.with(|cell| {
        if let Some(old) = cell.replace(media_opt) {
            old.set_playing(false);
        }
    });
}

/// Play the configured end bell at the end of a session. Gated on
/// end_bell_active (default true). Reuses the pre-warmed pipeline
/// from `preload_end_bell()` if available, falling back to a cold
/// lookup. No-op when the master toggle is off or the configured
/// uuid doesn't resolve.
pub fn play_end_bell(app: &MeditateApplication) {
    let active = app
        .with_db(|db| meditate_core::read_bool(db.core(), "end_bell_active", true))
        .unwrap_or(true);
    if !active {
        return;
    }
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
    if reused {
        return;
    }

    // No pre-loaded pipeline — load on the spot.
    let uuid = app
        .with_db(|db| db.get_setting("end_bell_sound", crate::db::BUNDLED_BOWL_UUID))
        .and_then(std::result::Result::ok)
        .unwrap_or_default();
    let Some(sound) = lookup_bell_sound_by_uuid(app, &uuid) else {
        return;
    };
    let media = media_for_bell_sound(&sound);
    swap_and_play(media);
}

/// Play the starting bell at session start. No-op if the master
/// toggle is off or the configured uuid doesn't resolve. Uses
/// STARTING_MEDIA so the existing end-sound preload in CURRENT_MEDIA
/// isn't disturbed.
pub fn play_starting_sound(app: &MeditateApplication) {
    let active = app
        .with_db(|db| meditate_core::read_bool(db.core(), "starting_bell_active", false))
        .unwrap_or(false);
    if !active {
        return;
    }
    let uuid = app
        .with_db(|db| db.get_setting("starting_bell_sound", crate::db::BUNDLED_BOWL_UUID))
        .and_then(std::result::Result::ok)
        .unwrap_or_default();
    let Some(sound) = lookup_bell_sound_by_uuid(app, &uuid) else {
        return;
    };
    let media = media_for_bell_sound(&sound);
    STARTING_MEDIA.with(|cell| {
        if let Some(old) = cell.replace(Some(media.clone())) {
            old.set_playing(false);
        }
    });
    media.set_playing(true);
}

/// Play the end bell by an explicit `uuid` — the post-migration
/// path where the Session's `FireEndBell` effect carries the
/// resolved sound uuid. Loads on the spot (no pre-warm reuse;
/// that's reserved for the `play_end_bell` entry the gtk app
/// preloads against the user's currently-configured uuid).
pub fn play_end_bell_uuid(uuid: &str, app: &MeditateApplication) {
    let Some(sound) = lookup_bell_sound_by_uuid(app, uuid) else {
        return;
    };
    let media = media_for_bell_sound(&sound);
    swap_and_play(media);
}

/// Play the starting bell by an explicit `uuid`. Mirror of
/// `play_end_bell_uuid` but routed through `STARTING_MEDIA` so the
/// pre-warmed end-bell pipeline in `CURRENT_MEDIA` isn't disturbed.
pub fn play_starting_uuid(uuid: &str, app: &MeditateApplication) {
    let Some(sound) = lookup_bell_sound_by_uuid(app, uuid) else {
        return;
    };
    let media = media_for_bell_sound(&sound);
    STARTING_MEDIA.with(|cell| {
        if let Some(old) = cell.replace(Some(media.clone())) {
            old.set_playing(false);
        }
    });
    media.set_playing(true);
}

/// Play a bell during the running session — interval/fixed bells
/// fired by the tick. Each call appends to INTERVAL_MEDIA so two
/// bells whose boundaries coincide both ring through (clipping one
/// would silently hide the collision). The MediaFile removes itself
/// via notify::ended once playback finishes. No-op if the uuid
/// doesn't resolve (e.g., a deleted custom that's still referenced
/// by a stale active_bells snapshot).
pub fn play_interval_sound(uuid: &str, app: &MeditateApplication) {
    let Some(sound) = lookup_bell_sound_by_uuid(app, uuid) else {
        return;
    };
    let media = media_for_bell_sound(&sound);

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
