use std::cell::{Cell, RefCell};
use std::time::Duration;
use gtk::{glib, prelude::*};

use crate::application::MeditateApplication;
use crate::db::BellSound;
use meditate_core::bell_volume::{BellSlot, BellVolume};
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
    /// The Bluetooth wake-up lead-in playing now, with the bells waiting
    /// for its end.
    static LEAD: RefCell<Option<Lead>> = const { RefCell::new(None) };
}

/// A lead-in noise and the bells that start when it has played out.
struct Lead {
    noise: gtk::MediaFile,
    waiting: Vec<gtk::MediaFile>,
    /// When its bells are due (`glib::monotonic_time`, µs).
    until: i64,
}

/// Play the bell `uuid` once at `volume` in the preview slot — what a
/// bell's Volume row plays when its slider comes to rest. Returns the
/// player so the caller can show a Stop button while it rings.
pub fn play_bell_preview(
    app: &MeditateApplication,
    uuid: &str,
    volume: BellVolume,
) -> Option<gtk::MediaFile> {
    lookup_bell_sound_by_uuid(app, uuid).map(|sound| play_preview(&sound, volume))
}

/// Play `media` again from the top at `volume`, if it is still the
/// preview and can play. Returns whether it did.
pub fn restart_preview(media: &gtk::MediaFile, volume: BellVolume) -> bool {
    let current = PREVIEW_MEDIA.with(|cell| cell.borrow().as_ref() == Some(media));
    if !current || media.error().is_some() {
        return false;
    }
    media.set_volume(volume.relative_gain());
    media.seek(0);
    start(media);
    true
}

/// Change the volume of the bell previewing right now, if any — a
/// Volume row's slider being dragged.
pub fn set_preview_volume(volume: BellVolume) {
    PREVIEW_MEDIA.with(|cell| {
        if let Some(m) = cell.borrow().as_ref() {
            m.set_volume(volume.relative_gain());
        }
    });
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
    LEAD.with(|cell| {
        if let Some(lead) = cell.take() {
            lead.noise.set_playing(false);
        }
    });
    stop_preview();
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
pub fn play_preview(sound: &BellSound, volume: BellVolume) -> gtk::MediaFile {
    let media = media_for_bell_sound(sound, volume);
    PREVIEW_MEDIA.with(|cell| {
        if let Some(old) = cell.replace(Some(media.clone())) {
            old.set_playing(false);
            // One still waiting for its lead-in never played; its row hears it.
            old.notify("playing");
        }
    });
    start(&media);
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
    let local_path = meditate_core::audio_files::guided_file_path(
        &crate::sync_runner::local_guided_dir(),
        file.uuid.as_str(),
    );
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
    let Some(m) = PREVIEW_MEDIA.with(|cell| cell.replace(None)) else { return };
    m.set_playing(false);
    // A preview stopped while it waits never played; its button still hears it.
    m.notify("playing");
    // A session bell waiting on the same noise keeps it.
    LEAD.with(|cell| {
        let mut cell = cell.borrow_mut();
        let idle = cell.as_mut().is_some_and(|lead| {
            lead.waiting.retain(|w| w != &m);
            lead.waiting.is_empty()
        });
        if let Some(lead) = idle.then(|| cell.take()).flatten() {
            lead.noise.set_playing(false);
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
/// The file comes from `meditate_core::audio_files::bell_sound_file`,
/// never the row's stored `file_path`: that is the creating device's
/// path, and sync can leave another platform's path even on a bundled
/// row. Bundled bells play from this build's GResource; custom bells
/// from the local sounds folder by uuid, where import and the sync
/// pull (B.6) both write them.
///
/// `volume` is the bell's own level, relative to the system volume
/// (`BellVolume::relative_gain`).
fn media_for_bell_sound(sound: &BellSound, volume: BellVolume) -> gtk::MediaFile {
    use meditate_core::audio_files::{bell_sound_file, BellSoundFile};
    let media = match bell_sound_file(
        sound,
        &crate::sync_runner::local_sounds_dir(),
        crate::db::bundled_bell_resource,
    ) {
        Some(BellSoundFile::Bundled(resource)) => gtk::MediaFile::for_resource(resource),
        Some(BellSoundFile::Local(path)) => {
            gtk::MediaFile::for_file(&gtk::gio::File::for_path(&path))
        }
        // A bundled bell this build doesn't ship: nothing to play.
        None => gtk::MediaFile::new(),
    };
    media.set_volume(volume.relative_gain());
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
    let Some(app) = app() else { return; };
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
pub fn preload_end_bell(app: &MeditateApplication, mode: meditate_core::SessionMode) {
    let key = meditate_core::settings_keys::end_bell_sound_key_for_mode(mode);
    let uuid = app
        .with_db(|db| db.get_setting(key, crate::db::BUNDLED_BOWL_UUID))
        .and_then(std::result::Result::ok)
        .unwrap_or_else(|| crate::db::BUNDLED_BOWL_UUID.to_string());
    let volume = app
        .with_db(|db| meditate_core::bell_volume::read(db.core(), BellSlot::End(mode)))
        .unwrap_or_default();
    let media_opt = lookup_bell_sound_by_uuid(app, &uuid).map(|s| media_for_bell_sound(&s, volume));
    CURRENT_MEDIA.with(|cell| {
        if let Some(old) = cell.replace(media_opt) {
            old.set_playing(false);
        }
    });
}

/// Play the end bell by an explicit `uuid` — the post-migration
/// path where the Session's `FireEndBell` effect carries the
/// resolved sound uuid. Loads on the spot (no pre-warm reuse;
/// that's reserved for the `play_end_bell` entry the gtk app
/// preloads against the user's currently-configured uuid).
pub fn play_end_bell_uuid(uuid: &str, volume: BellVolume, app: &MeditateApplication) -> Duration {
    let Some(sound) = lookup_bell_sound_by_uuid(app, uuid) else {
        return Duration::ZERO;
    };
    let media = media_for_bell_sound(&sound, volume);
    swap_and_play(media)
}

/// Play the starting bell by an explicit `uuid`. Mirror of
/// `play_end_bell_uuid` but routed through `STARTING_MEDIA` so the
/// pre-warmed end-bell pipeline in `CURRENT_MEDIA` isn't disturbed.
pub fn play_starting_uuid(uuid: &str, volume: BellVolume, app: &MeditateApplication) -> Duration {
    let Some(sound) = lookup_bell_sound_by_uuid(app, uuid) else {
        return Duration::ZERO;
    };
    let media = media_for_bell_sound(&sound, volume);
    STARTING_MEDIA.with(|cell| {
        if let Some(old) = cell.replace(Some(media.clone())) {
            old.set_playing(false);
        }
    });
    start(&media)
}

/// Play a bell during the running session — interval/fixed bells
/// fired by the tick. Each call appends to INTERVAL_MEDIA so two
/// bells whose boundaries coincide both ring through (clipping one
/// would silently hide the collision). The MediaFile removes itself
/// via notify::ended once playback finishes. No-op if the uuid
/// doesn't resolve (e.g., a deleted custom that's still referenced
/// by a stale active_bells snapshot).
pub fn play_interval_sound(uuid: &str, volume: BellVolume, app: &MeditateApplication) -> Duration {
    let Some(sound) = lookup_bell_sound_by_uuid(app, uuid) else {
        return Duration::ZERO;
    };
    let media = media_for_bell_sound(&sound, volume);

    media.connect_ended_notify(|m| {
        if m.is_ended() {
            INTERVAL_MEDIA.with(|cell| {
                cell.borrow_mut().retain(|x| x != m);
            });
        }
    });
    INTERVAL_MEDIA.with(|cell| cell.borrow_mut().push(media.clone()));
    start(&media)
}

// ── Internal ──────────────────────────────────────────────────────────────────

fn swap_and_play(media: gtk::MediaFile) -> Duration {
    CURRENT_MEDIA.with(|cell| {
        let old = cell.replace(Some(media.clone()));
        if let Some(m) = old {
            m.set_playing(false);
        }
    });
    start(&media)
}

/// Start `media`, after the wake-up lead-in when one is due. Returns
/// how long it waits, for its vibration.
fn start(media: &gtk::MediaFile) -> Duration {
    // A preview still ringing (restarted by its slider) waits silent too.
    media.set_playing(false);
    lead_in(media).unwrap_or_else(|| {
        media.set_playing(true);
        Duration::ZERO
    })
}

fn app() -> Option<MeditateApplication> {
    gtk::gio::Application::default().and_then(|a| a.downcast::<MeditateApplication>().ok())
}

/// Hold `media` back for the lead-in when it is on and the default
/// output is Bluetooth: join the one running, or play core's noise.
/// The wait; none when it starts at once (off, another output, or the
/// noise can't play: only logged, the bell rings without it).
fn lead_in(media: &gtk::MediaFile) -> Option<Duration> {
    let app = app()?;
    let ms = app.with_db(|db| meditate_core::settings_keys::bluetooth_wake_lead_in(db.core())).flatten()?;
    if !default_output_is_bluetooth() {
        return None;
    }
    let now = glib::monotonic_time();
    let left = |until: i64| Duration::from_micros(u64::try_from(until - now).unwrap_or(0));
    let joined = LEAD.with(|cell| {
        cell.borrow_mut().as_mut().map(|lead| {
            lead.waiting.push(media.clone());
            left(lead.until)
        })
    });
    if joined.is_some() {
        return joined;
    }
    // GTK's player takes files, not memory: one file per length; the
    // version changes with `lead_in::samples`.
    let path = glib::user_cache_dir().join("meditate").join(format!("lead-in-v4-{ms}.wav"));
    if !path.exists() {
        // Written aside and moved in, so a cut-off write is never reused.
        let part = path.with_extension("part");
        let written = std::fs::create_dir_all(path.parent().unwrap_or(&path))
            .and_then(|()| std::fs::write(&part, meditate_core::lead_in::wav(&meditate_core::lead_in::samples(ms, meditate_core::lead_in::RATE), meditate_core::lead_in::RATE)))
            .and_then(|()| std::fs::rename(&part, &path));
        if let Err(e) = written {
            meditate_core::log("sound.lead_in", &format!("writing {} failed: {e}", path.display()));
            return None;
        }
        forget_other_lead_ins(&path);
    }
    let noise = gtk::MediaFile::for_file(&gtk::gio::File::for_path(&path));
    noise.connect_error_notify(|m| {
        if let Some(err) = m.error() {
            meditate_core::log("sound.lead_in", &format!("noise failed: {err}"));
            wake(m);
        }
    });
    // The bell starts where the noise and its silence ended, or late.
    noise.connect_ended_notify(|m| if m.is_ended() { wake(m); });
    let wait = Duration::from_millis(u64::from(ms + meditate_core::lead_in::GAP_MS));
    let late = noise.clone();
    glib::timeout_add_local_once(wait + Duration::from_millis(u64::from(meditate_core::lead_in::LATE_MS)), move || wake(&late));
    noise.set_playing(true);
    let until = now + i64::try_from(wait.as_micros()).unwrap_or(i64::MAX);
    LEAD.with(|cell| cell.replace(Some(Lead { noise, waiting: vec![media.clone()], until })));
    Some(wait)
}

/// Delete the cached noise files of other lengths and older versions:
/// each length tried while tuning would otherwise stay, up to 1 MB each.
fn forget_other_lead_ins(keep: &std::path::Path) {
    let Some(dir) = keep.parent().and_then(|d| std::fs::read_dir(d).ok()) else { return };
    for entry in dir.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if path != keep && name.to_string_lossy().starts_with("lead-in-") {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// The lead-in `noise` played out (or ran late, or failed): its bells
/// start. A noise stopped or replaced meanwhile wakes nothing.
fn wake(noise: &gtk::MediaFile) {
    let Some(lead) = LEAD.with(|cell| {
        let mut cell = cell.borrow_mut();
        if cell.as_ref().is_some_and(|lead| &lead.noise == noise) { cell.take() } else { None }
    }) else {
        return;
    };
    lead.noise.set_playing(false);
    for m in &lead.waiting {
        // Stopped or replaced meanwhile: stays silent.
        if held(m) {
            m.set_playing(true);
        }
    }
}

/// Whether `media` is still in one of the slots.
fn held(media: &gtk::MediaFile) -> bool {
    let is = |cell: &RefCell<Option<gtk::MediaFile>>| cell.borrow().as_ref() == Some(media);
    PREVIEW_MEDIA.with(is)
        || CURRENT_MEDIA.with(is)
        || STARTING_MEDIA.with(is)
        || INTERVAL_MEDIA.with(|cell| cell.borrow().contains(media))
}

/// Whether the default sound output is Bluetooth, asked from GStreamer
/// each time so a changed output counts at once.
fn default_output_is_bluetooth() -> bool {
    use gstreamer::prelude::{DeviceExt, DeviceMonitorExt, DeviceMonitorExtManual};
    if let Err(e) = gstreamer::init() {
        meditate_core::log("sound.lead_in", &format!("gstreamer init failed: {e}"));
        return false;
    }
    let monitor = gstreamer::DeviceMonitor::new();
    monitor.add_filter(Some("Audio/Sink"), None);
    if let Err(e) = monitor.start() {
        meditate_core::log("sound.lead_in", &format!("device monitor failed: {e}"));
        return false;
    }
    let default = monitor
        .devices()
        .iter()
        .filter_map(|d| d.properties())
        .find(|p| p.get::<bool>("is-default").unwrap_or(false));
    monitor.stop();
    let Some(p) = default else {
        meditate_core::log("sound.lead_in", "no default sink found");
        return false;
    };
    meditate_core::lead_in::is_bluetooth_sink(p.get::<&str>("device.api").ok(), p.get::<&str>("node.name").ok())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    fn source(rel: &str) -> String {
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)).unwrap()
    }

    /// The body of `fn name` in `src`, up to the next top-level item.
    fn fn_body<'a>(src: &'a str, name: &str) -> &'a str {
        let start = src.find(&format!("fn {name}(")).unwrap_or_else(|| panic!("no fn {name}"));
        let end = src[start..].find("\n}\n").map_or(src.len(), |e| start + e);
        &src[start..end]
    }

    /// A player in the error state never plays again, so restarting it
    /// would leave the bell silent; one another preview took over is
    /// no longer this row's to restart.
    #[test]
    fn only_a_healthy_player_still_in_the_preview_slot_restarts() {
        let src = source("src/sound.rs");
        let body = fn_body(&src, "restart_preview");
        assert!(body.contains(".error().is_some()"), "a failed player is rebuilt");
        assert!(body.find("return false").unwrap() < body.find("seek(0)").unwrap());
        assert!(body.contains("PREVIEW_MEDIA"), "only the player still previewing");
        assert!(body.find("seek(0)").unwrap() < body.find("start(media)").unwrap());
    }

    /// With the wake-up lead-in on and a Bluetooth default output, a
    /// bell waits for core's noise to reach its end of stream (its
    /// silence included); a bell due meanwhile joins it; Stop ends both;
    /// the vibration waits as long as the sound.
    #[test]
    fn the_lead_in_plays_before_a_bluetooth_bell() {
        let src = source("src/sound.rs");
        let start = fn_body(&src, "start");
        assert!(start.contains("lead_in(media).unwrap_or_else(|| {\n        media.set_playing(true);"));
        // A preview still ringing (restarted by its slider) waits silent too.
        assert!(start.find("media.set_playing(false);").unwrap() < start.find("lead_in(media)").unwrap());
        let lead = fn_body(&src, "lead_in");
        assert!(lead.contains("let ms = app.with_db(|db| meditate_core::settings_keys::bluetooth_wake_lead_in(db.core())).flatten()?;"));
        assert!(lead.contains("if !default_output_is_bluetooth() {"));
        // Only the current length's file stays in the cache.
        assert!(lead.contains("forget_other_lead_ins(&path);"));
        assert!(lead.contains("lead.waiting.push(media.clone());"), "a running lead-in is joined");
        assert!(lead.contains("meditate_core::lead_in::wav(&meditate_core::lead_in::samples(ms, meditate_core::lead_in::RATE), meditate_core::lead_in::RATE)"));
        assert!(lead.contains("format!(\"lead-in-v4-{ms}.wav\")"));
        // The bell starts where the noise and its silence ended, or late.
        assert!(lead.contains("noise.connect_ended_notify(|m| if m.is_ended() { wake(m); });"));
        assert!(lead.contains("glib::timeout_add_local_once(wait + Duration::from_millis(u64::from(meditate_core::lead_in::LATE_MS)), move || wake(&late));"));
        // A late timer of an old noise never wakes a newer one.
        assert!(fn_body(&src, "wake").contains("if cell.as_ref().is_some_and(|lead| &lead.noise == noise) { cell.take() } else { None }"));
        assert!(fn_body(&src, "wake").contains("if held(m) {\n            m.set_playing(true);"));
        // Stop ends the noise; closing a chooser drops only its preview.
        assert!(fn_body(&src, "stop_all").contains("if let Some(lead) = cell.take() {"));
        let stop = fn_body(&src, "stop_preview");
        assert!(stop.contains("lead.waiting.retain(|w| w != &m);"));
        // A preview stopped while it waits never played; its button still hears it.
        assert!(stop.contains("m.notify(\"playing\");"));
        assert!(fn_body(&src, "play_preview").contains("old.notify(\"playing\");"), "a replaced waiting preview too");
        let bt = fn_body(&src, "default_output_is_bluetooth");
        assert!(bt.contains(".find(|p| p.get::<bool>(\"is-default\").unwrap_or(false))"));
        assert!(bt.contains("meditate_core::lead_in::is_bluetooth_sink("));
        // Not finding the output is logged, never silent.
        assert!(bt.contains("meditate_core::log(\"sound.lead_in\", \"no default sink found\");"));
        // Every bell returns its wait, and the vibration waits for it.
        for name in ["play_starting_uuid", "play_end_bell_uuid", "play_interval_sound"] {
            assert!(fn_body(&src, name).contains("-> Duration {"), "{name}");
        }
        let fire = source("src/timer/imp.rs");
        let fire = &fire[fire.find("fn dispatch_fire_route(").unwrap()..];
        let fire = &fire[..fire.find("\n    }\n").unwrap()];
        assert!(fire.contains("let wait = if route.signal_mode.includes_sound() {"));
        assert!(fire.contains("glib::timeout_add_local_once(wait, move || {"));
        // A Stop meanwhile drops the waiting buzz.
        assert!(fire.contains("if this.imp().signal_gen.get() != generation {"));
        let stop = source("src/timer/imp.rs");
        let stop = &stop[stop.find("fn stop_active_signals(&self) {").unwrap()..];
        assert!(stop[..stop.find("\n    }\n").unwrap()].contains("self.signal_gen.set(self.signal_gen.get().wrapping_add(1));"));
    }

    /// Every bundled bell this build seeds has a built-in file, found by
    /// uuid; anything else has none.
    #[test]
    fn every_bundled_bell_has_a_built_in_resource() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::Database::open(&dir.path().join("t.db")).unwrap();
        let bundled: Vec<_> = db
            .list_bell_sounds()
            .unwrap()
            .into_iter()
            .filter(|b| b.is_bundled)
            .collect();
        assert_eq!(bundled.len(), 11);
        for bell in bundled {
            let resource = crate::db::bundled_bell_resource(bell.uuid.as_str())
                .unwrap_or_else(|| panic!("{} has no resource", bell.name));
            assert!(resource.starts_with("/io/github/janekbt/Meditate/sounds/"), "{resource}");
        }
        assert_eq!(crate::db::bundled_bell_resource("not-a-bundled-uuid"), None);
    }

    /// A row's stored `file_path` is the path on the device that created
    /// it; sync copies it everywhere, so outside the database layer no
    /// code may read it. Files are found through
    /// `meditate_core::audio_files` instead.
    #[test]
    fn no_shell_code_reads_the_stored_file_path() {
        fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    if !path.ends_with("db") {
                        walk(&path, out);
                    }
                } else if path.extension().is_some_and(|e| e == "rs") {
                    out.push(path);
                }
            }
        }
        let mut files = Vec::new();
        walk(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);
        assert!(files.len() > 10, "walked the shell sources");
        for file in files {
            let src = std::fs::read_to_string(&file).unwrap();
            let code = src.split("#[cfg(test)]\nmod tests").next().unwrap();
            for line in code.lines().filter(|l| !l.trim_start().starts_with("//")) {
                assert!(
                    !line.contains(".file_path"),
                    "{} reads the stored file_path: {}",
                    file.display(),
                    line.trim(),
                );
            }
        }
    }

    /// Every bell player comes from `media_for_bell_sound`, which applies
    /// the bell volume, so no bell path can skip it. The only other
    /// player is the guided-file preview: voice audio, not a bell.
    #[test]
    fn every_bell_player_gets_the_bell_volume() {
        let src = source("src/sound.rs");
        let code = &src[..src.find("#[cfg(test)]\nmod tests").unwrap()];
        let creators: Vec<&str> = code
            .match_indices("MediaFile::for_")
            .map(|(at, _)| {
                let head = &code[..at];
                let fn_at = head.rfind("\nfn ").max(head.rfind("\npub fn ")).unwrap();
                let name = &code[fn_at..].split_once("fn ").unwrap().1;
                &name[..name.find('(').unwrap()]
            })
            .collect();
        assert!(
            // The lead-in noise plays at its own fixed level, not a bell's.
            creators.iter().all(|f| ["media_for_bell_sound", "play_preview_for_guided_file", "lead_in"].contains(f)),
            "MediaFile created outside media_for_bell_sound: {creators:?}",
        );
        assert!(
            fn_body(code, "media_for_bell_sound").contains("set_volume(volume.relative_gain())"),
            "media_for_bell_sound must apply the bell's own volume",
        );
    }

    /// Each session bell plays at the volume its effect carries.
    #[test]
    fn every_session_bell_plays_at_its_own_volume() {
        let imp = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("src/timer/imp.rs"),
        )
        .unwrap();
        let route = fn_body(&imp, "dispatch_fire_route");
        for play in ["play_interval_sound(", "play_starting_uuid(", "play_end_bell_uuid("] {
            let at = route.find(play).unwrap_or_else(|| panic!("{play} missing"));
            let call = route[at..].split_once(';').unwrap().0;
            assert!(call.contains("route.volume"), "{call}");
        }
    }

    #[test]
    fn no_other_module_creates_a_media_player() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut offenders = Vec::new();
        let mut stack = vec![dir];
        while let Some(d) = stack.pop() {
            for entry in std::fs::read_dir(d).unwrap() {
                let p = entry.unwrap().path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|e| e == "rs")
                    && !p.ends_with("sound.rs")
                    && std::fs::read_to_string(&p).unwrap().contains("MediaFile::for_")
                {
                    offenders.push(p.display().to_string());
                }
            }
        }
        assert!(offenders.is_empty(), "play bells through crate::sound: {offenders:?}");
    }
}
