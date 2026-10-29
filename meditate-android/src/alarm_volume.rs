// Absolute bell volume on Android. Each bell carries its own level
// (`meditate_core::bell_volume`); the core picks the stream step and
// the player gains for it on this device's curve: the alarm curve, or
// with bells played as media (Preferences, #10) the speaker's media
// curve and the headphone curve. `MeditateAudio.kt` sets the stream to
// the step the loudest ringing bell needs and trims each bell by its
// gain there, so the user's volume doesn't change how loud a bell
// rings. The curves are read once at startup (`audio::alarm_steps`,
// `audio::media_steps`, `audio::headphone_steps`) and kept here so
// every bell play site can read them without a JNI round trip.

use meditate_core::bell_volume::{AlarmCurve, AlarmLevel, BellVolume};
use std::sync::Mutex;

/// AOSP's default top alarm and media steps, for a device that reported nothing.
const FALLBACK_MAX_STEP: i32 = 7;
const FALLBACK_MAX_MEDIA_STEP: i32 = 15;

/// One stream's curve as the device reported it, with how to read the
/// report and the even curve used before it arrives or when it fails.
struct Slot {
    curve: Mutex<Option<AlarmCurve>>,
    make: fn(i32, i32, &[f32]) -> AlarmCurve,
    fallback_max: i32,
}

impl Slot {
    const fn new(make: fn(i32, i32, &[f32]) -> AlarmCurve, fallback_max: i32) -> Self {
        Slot { curve: Mutex::new(None), make, fallback_max }
    }

    /// Remember a report `[min, max, dB of each step...]`; `None` or a
    /// short one falls back to the even curve.
    fn set(&self, report: Option<Vec<f32>>) {
        let curve = match report.as_deref() {
            Some([min, max, db @ ..]) => (self.make)(*min as i32, *max as i32, db),
            _ => self.fallback(),
        };
        *self.curve.lock().unwrap_or_else(|e| e.into_inner()) = Some(curve);
    }

    fn get(&self) -> AlarmCurve {
        self.curve.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_else(|| self.fallback())
    }

    fn fallback(&self) -> AlarmCurve {
        (self.make)(1, self.fallback_max, &[])
    }
}

static ALARM: Slot = Slot::new(AlarmCurve::from_report, FALLBACK_MAX_STEP);
static MEDIA: Slot = Slot::new(AlarmCurve::from_report, FALLBACK_MAX_MEDIA_STEP);
/// Media in headphones, cut at the safety step.
static HEADPHONES: Slot = Slot::new(AlarmCurve::headphones, FALLBACK_MAX_MEDIA_STEP);

pub fn set_curve(report: Option<Vec<f32>>) {
    ALARM.set(report);
}

pub fn set_media_curve(report: Option<Vec<f32>>) {
    MEDIA.set(report);
}

pub fn set_headphone_curve(report: Option<Vec<f32>>) {
    HEADPHONES.set(report);
}

/// A bell's level for the player: `main` on the stream it plays on
/// unless headphones take it (the alarm, or as media the speaker's
/// media steps), and `headphones` for media in headphones. Kotlin picks
/// by the route when the bell starts.
pub struct Levels {
    pub as_media: bool,
    pub main: AlarmLevel,
    pub headphones: AlarmLevel,
}

impl Levels {
    pub fn new(
        volume: BellVolume,
        as_media: bool,
        alarm: &AlarmCurve,
        speaker_media: &AlarmCurve,
        headphones: &AlarmCurve,
    ) -> Self {
        Levels {
            as_media,
            main: volume.alarm_level(if as_media { speaker_media } else { alarm }),
            headphones: volume.alarm_level(headphones),
        }
    }
}

/// The levels for a bell at `volume`, as an alarm or as media.
pub fn levels(volume: BellVolume, as_media: bool) -> Levels {
    Levels::new(volume, as_media, &ALARM.get(), &MEDIA.get(), &HEADPHONES.get())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(rel: &str) -> String {
        std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel))
            .unwrap()
    }

    #[test]
    fn a_bell_takes_the_level_of_where_it_plays() {
        // Alarm steps 1..=7, media 1..=15 on the speaker, and the same
        // in headphones cut at the safety step 10: all evenly.
        let alarm = AlarmCurve::from_report(1, 7, &[]);
        let speaker = AlarmCurve::from_report(1, 15, &[]);
        let head = AlarmCurve::headphones(1, 15, &[]);
        let half = BellVolume::from_percent(50.0);
        let as_alarm = Levels::new(half, false, &alarm, &speaker, &head);
        assert!(!as_alarm.as_media);
        assert_eq!(as_alarm.main.step, 4);
        let as_media = Levels::new(half, true, &alarm, &speaker, &head);
        assert!(as_media.as_media);
        assert_eq!(as_media.main.step, 8);
        assert_eq!(as_media.headphones.step, 6);
        let full = Levels::new(BellVolume::from_percent(100.0), true, &alarm, &speaker, &head);
        assert_eq!((full.main.step, full.headphones.step), (15, 10));
    }

    // One test for the globals: parallel tests would race on them.
    #[test]
    fn the_level_follows_the_reported_steps() {
        let loudest = BellVolume::from_percent(100.0);
        // Before reading the steps: alarm 1..=7, media 1..=15, and in
        // headphones 1..=10 of 15.
        assert_eq!(levels(loudest, false).main.step, 7);
        assert_eq!(levels(loudest, true).main.step, 15);
        assert_eq!(levels(loudest, true).headphones.step, 10);
        set_curve(Some(vec![1.0, 15.0]));
        set_media_curve(Some(vec![0.0, 25.0]));
        set_headphone_curve(Some(vec![0.0, 30.0]));
        assert_eq!(levels(loudest, false).main.step, 15);
        assert_eq!(levels(loudest, true).main.step, 25);
        assert_eq!(levels(loudest, true).headphones.step, 10, "Android's safety index, not scaled");
        assert_eq!(levels(BellVolume::from_percent(0.0), false).main.step, 1);
        // A failed read falls back instead of keeping the stale steps.
        set_curve(None);
        set_media_curve(Some(vec![1.0]));
        set_headphone_curve(None);
        assert_eq!(levels(loudest, false).main.step, 7);
        assert_eq!(levels(loudest, true).main.step, 15);
        assert_eq!(levels(loudest, true).headphones.step, 10);
    }

    /// Every bell reaches MediaPlayer through `audio::play`, and every
    /// call passes the level for that bell's own volume.
    #[test]
    fn every_bell_play_passes_its_own_volume() {
        let lib = source("src/ui.rs");
        let calls: Vec<&str> = lib
            .match_indices("audio::play(")
            .map(|(at, _)| lib[at..].split_once(';').unwrap().0)
            .collect();
        assert!(!calls.is_empty());
        for call in calls {
            assert!(call.contains("&bell_levels("), "{call}");
        }
        // Every bell and preview reads the switch as it starts, through
        // one helper: play_bell, play, and the live slider drag.
        assert!(lib.contains("fn bell_levels(volume: meditate_core::bell_volume::BellVolume) -> alarm_volume::Levels {"));
        assert!(lib.contains("alarm_volume::levels(volume, !bells_as_alarm())"));
        assert!(lib.contains("lock_db().is_some_and(|db| meditate_core::settings_keys::bells_as_alarm_from_db(&db))"));
        assert!(lib.contains("audio::play_bell(app, app::bell_channel_slot(route.channel), &path, &bell_levels(route.volume));"));
        assert!(lib.contains("audio::set_volume(app, &bell_levels(volume));"));
        assert!(!lib.contains("alarm_volume::level("), "no play site skips the switch");
        assert!(!lib.contains("bell_gain"), "the global bell gain is gone");
        assert!(!lib.contains("meditate_core::bell_volume::read"), "no global level");
    }

    #[test]
    fn startup_reads_the_steps_and_recovers_the_alarm_volume() {
        let lib = source("src/ui.rs");
        assert!(lib.contains("alarm_volume::set_curve(audio::alarm_steps(app))"));
        assert!(lib.contains("alarm_volume::set_media_curve(audio::media_steps(app))"));
        assert!(lib.contains("alarm_volume::set_headphone_curve(audio::headphone_steps(app))"));
        assert!(lib.contains("audio::recover_alarm_volume(app)"));
    }

    #[test]
    fn the_level_reaches_the_media_player() {
        let audio = source("src/audio.rs");
        assert!(audio.contains("pub fn play(app: &AndroidApp, path: &str, level: &Levels) -> i64"));
        assert!(audio.contains("\"(Landroid/content/Context;Ljava/lang/String;ZI[FI[F)J\""));
        assert!(audio.contains("\"(Landroid/content/Context;ILjava/lang/String;ZI[FI[F)V\""));
        assert!(audio.contains("\"(Landroid/content/Context;ZI[FI[F)V\""));
        let kotlin = source("kotlin/MeditateAudio.kt");
        assert!(kotlin.contains(
            "fun play(context: Context, path: String, asMedia: Boolean, step: Int, gains: FloatArray, headStep: Int, headGains: FloatArray): Long"
        ));
        assert!(kotlin.contains("mp.setVolume(gain, gain)"));
    }

    /// The alarm stream follows the bells and is put back after,
    /// without ever showing the system volume panel or playing a click.
    #[test]
    fn the_alarm_stream_follows_the_bells_quietly_and_is_restored() {
        let kotlin = source("kotlin/MeditateAudio.kt");
        let sets: Vec<&str> = kotlin
            .match_indices("setStreamVolume(")
            .map(|(at, _)| kotlin[at..].split_once(')').unwrap().0)
            .collect();
        assert_eq!(sets.len(), 2, "apply + restore");
        for call in sets {
            assert!(call.contains("hold.stream"), "{call}");
            assert!(call.trim_end().ends_with(", 0"), "no flags: {call}");
        }
        assert!(kotlin.contains("Hold(AudioManager.STREAM_ALARM, attrs, \"alarm_restore_index\", \"alarm_set_index\") { \"\" }"),
            "the alarm keys stay, so an older version's crash is still put back");
        assert!(!kotlin.contains("FLAG_SHOW_UI") && !kotlin.contains("FLAG_PLAY_SOUND"));
        // The stream takes the loudest live bell's step, and the step
        // is saved before it is set, so a crash in between is put back.
        let apply = &kotlin[kotlin.find("private fun applyLocked(").unwrap()..];
        let apply = &apply[..apply.find("\n    }\n").unwrap()];
        assert!(apply.contains("levels.values.filter { it.hold === hold }") && apply.contains("live.maxOf { it.step }"));
        assert!(apply.find("putInt(keySet, want)").unwrap() < apply.find("setStreamVolume(").unwrap());
        // Raising trims the players first, so a quiet bell never peaks.
        assert!(apply.find("trimLocked(hold, want)").unwrap() < apply.find("setStreamVolume(").unwrap());
        // A live slider drag moves the stream too.
        let drag = &kotlin[kotlin.find("fun setVolume(context: Context, asMedia: Boolean, step: Int, gains: FloatArray, headStep: Int, headGains: FloatArray)").unwrap()..];
        let drag = &drag[..drag.find("\n    }\n").unwrap()];
        assert!(drag.contains("levels[mp] = ") && drag.contains("applyLocked(context.applicationContext, old.hold)"));
        // Set right before playing; followed or restored, once nothing
        // else rings, when a bell ends, fails or won't start, when
        // everything is stopped, and on the next start after a crash.
        let start = &kotlin[kotlin.find("private fun startLocked(").unwrap()..];
        assert!(start.find("applyLocked(app, hold)").unwrap() < start.find("mp.start()").unwrap());
        assert!(kotlin.contains("fun recoverAlarmVolume(context: Context)"));
        assert_eq!(kotlin.matches("synchronized(lock) { finishedLocked(app, mp) }").count(), 2);
        let finished = &kotlin[kotlin.find("private fun finishedLocked(").unwrap()..];
        assert!(finished[..finished.find("\n    }\n").unwrap()].contains("settleLocked(app)"));
        let stop = &kotlin[kotlin.find("fun stop(context: Context)").unwrap()..];
        // Nothing left in `levels`: settle puts both streams back.
        assert!(stop[..stop.find("\n    }\n").unwrap()].contains("releaseAllLocked()\n            settleLocked(context.applicationContext)"));
        // Restored only while the stream is still at the step the app set.
        let restore = &kotlin[kotlin.find("private fun restoreSaved(").unwrap()..];
        assert!(restore.contains("!= set) return"));
    }

    /// As media a bell plays where music would: Android routes it (to
    /// headphones when connected); the app only reads the route to pick
    /// the curve, holds the media step like the alarm step, puts it back
    /// on the output it was set on, and stops the bell when the output
    /// changes under it.
    #[test]
    fn a_media_bell_holds_the_media_step_and_puts_it_back() {
        let kotlin = source("kotlin/MeditateAudio.kt");
        assert!(kotlin.contains(".setUsage(AudioAttributes.USAGE_MEDIA)"));
        assert!(kotlin.contains("Hold(AudioManager.STREAM_MUSIC, mediaAttrs, \"media_restore_index\", \"media_set_index\", ::route)"));
        let start = &kotlin[kotlin.find("private fun startLocked(").unwrap()..];
        let start = &start[..start.find("\n    }\n").unwrap()];
        // As media the headphone curve when media goes to headphones now,
        // else the stream's own; the output is read once per bell.
        assert!(start.contains("val hold = if (asMedia) media else alarm\n        val phones = headphones(app)\n        val out = outOf(phones)"));
        assert!(start.contains("levelFor(hold, out, step, gains, headStep, headGains)"));
        assert!(kotlin.contains("if (hold === media && out != SPEAKER) Level(hold, out, headStep, headGains) else Level(hold, out, step, gains)"));
        assert!(start.contains("mp.setAudioAttributes(hold.attrs)"));
        assert!(start.contains("if (!hold.raised) hold.output = if (hold === media) out else \"\""));
        // Each output keeps its own saved step: a bell on the speaker
        // never overwrites the step kept for headphones that went away.
        assert!(kotlin.contains("fun keyRestore(out: String) = if (out.isEmpty()) restoreKey else \"$restoreKey/$out\""));
        assert!(kotlin.contains("fun keySet(out: String) = if (out.isEmpty()) setKey else \"$setKey/$out\""));
        assert!(!kotlin.contains("KEY_MEDIA_OUTPUT") && !kotlin.contains("stale"));
        // Android routes a media bell itself; no focus, so music neither pauses nor ducks.
        assert!(!kotlin.contains("requestAudioFocus"));
        // The output it was set on: only there the step is put back.
        let apply = &kotlin[kotlin.find("private fun applyLocked(").unwrap()..];
        let apply = &apply[..apply.find("\n    }\n").unwrap()];
        assert!(apply.contains("if (hold.route(context) != hold.output) hold.takenOver = true"));
        assert!(apply.contains("hold.keyRestore(hold.output)") && apply.contains("hold.keySet(hold.output)"));
        // Put back on the output media plays on now, from that output's keys.
        let restore = &kotlin[kotlin.find("private fun restoreSaved(").unwrap()..];
        let restore = &restore[..restore.find("\n    }\n").unwrap()];
        assert!(restore.contains("val output = hold.route(context)") && restore.contains("hold.keyRestore(output)"));
        // The keys go only once the step is back: a stream that isn't at
        // the app's step (media still on another output for a moment, or
        // the user moved it) keeps them, and the next raise overwrites them.
        assert!(restore.contains("if (am.getStreamVolume(hold.stream) != set) return"));
        assert!(restore.find("am.setStreamVolume(hold.stream, saved, 0)").unwrap() < restore.find(".remove(keyRestore).remove(keySet)").unwrap());
        assert!(!kotlin.contains("edit().clear()"), "never every key at once");
        // Headphones pulled or plugged in: the media bells stop.
        assert!(kotlin.contains("override fun onAudioDevicesAdded(") && kotlin.contains("override fun onAudioDevicesRemoved("));
        assert!(kotlin.contains("levels.filterValues { it.out != now }.keys.toList().forEach { mp -> finishedLocked(app, mp) }"));
        // An output coming back gets its kept step back at once, also
        // after a restart (the watch starts with the recovery).
        assert!(kotlin.contains("if (!media.raised) restoreSaved(app, media)"));
        let recover = &kotlin[kotlin.find("fun recoverAlarmVolume(").unwrap()..];
        assert!(recover[..recover.find("\n    }\n").unwrap()].contains("watchLocked(context.applicationContext)"));
        // A drag only moves a preview still on the stream the switch picks now.
        let drag = &kotlin[kotlin.find("fun setVolume(").unwrap()..];
        let drag = &drag[..drag.find("\n    }\n").unwrap()];
        assert!(drag.contains("if ((old.hold === media) != asMedia) return"));
        assert!(drag.contains("levelFor(old.hold, old.out, step, gains, headStep, headGains)"));
        // The speaker as an output has its own key, so a step set there is put back there.
        assert!(kotlin.contains("private fun route(context: Context): String = outOf(headphones(context))"));
        assert!(kotlin.contains("private fun outOf(phones: AudioDeviceInfo?): String = phones?.let { key(it) } ?: SPEAKER"));
        // Both curves read at startup, both streams recovered after a crash.
        assert!(kotlin.contains("fun mediaSteps(context: Context): FloatArray"));
        assert!(kotlin.contains("steps(context, AudioManager.STREAM_MUSIC, AudioDeviceInfo.TYPE_BUILTIN_SPEAKER, \"mediaSteps\")"));
        assert!(kotlin.contains("steps(context, AudioManager.STREAM_MUSIC, AudioDeviceInfo.TYPE_WIRED_HEADPHONES, \"headphoneSteps\")"));
        let recover = &kotlin[kotlin.find("fun recoverAlarmVolume(").unwrap()..];
        assert!(recover[..recover.find("\n    }\n").unwrap()].contains("holds.forEach { restoreSaved(context.applicationContext, it) }"));
        // No lead-in noise in this change.
        assert!(!kotlin.contains("AudioTrack") && !kotlin.contains("lead"));
    }

    /// With "Play Bells as Alarm" a bell still plays as an alarm, aimed
    /// at the headphones media plays on, so it rings only there; it stops
    /// when its output changes, like a media bell.
    #[test]
    fn an_alarm_bell_plays_only_in_the_headphones() {
        let kotlin = source("kotlin/MeditateAudio.kt");
        let start = &kotlin[kotlin.find("private fun startLocked(").unwrap()..];
        let start = &start[..start.find("\n    }\n").unwrap()];
        // Only the alarm is aimed, from Android 9 (API 28 has the call);
        // media keeps Android's own route. A refused aim leaves the alarm
        // route and says so in the log.
        assert!(start.contains("if (hold === alarm && phones != null && Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {\n                if (!mp.setPreferredDevice(phones)) Log.w(TAG, "));
        assert_eq!(kotlin.matches("setPreferredDevice").count(), 1, "nothing else is aimed");
        // Android drops the aim on a player without its file (device test 2026-10-29).
        let aim = start.find("mp.setPreferredDevice(phones)").unwrap();
        assert!(start.find("mp.setDataSource(path)").unwrap() < aim && aim < start.find("mp.prepare()").unwrap());
        // The alarm on the speaker's curve and its one pair of keys, as before.
        assert!(kotlin.contains("Hold(AudioManager.STREAM_ALARM, attrs, \"alarm_restore_index\", \"alarm_set_index\") { \"\" }"));
        // Every bell is watched and stops when its own output goes.
        assert!(start.contains("            watchLocked(app)\n"));
        assert!(!start.contains("if (hold === media) watchLocked(app)"));
        let changed = &kotlin[kotlin.find("private fun outputChanged(").unwrap()..];
        let changed = &changed[..changed.find("\n    }\n").unwrap()];
        assert!(changed.contains("val now = route(app)"));
    }
}
