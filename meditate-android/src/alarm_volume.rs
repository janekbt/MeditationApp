// Absolute bell volume on Android. Each bell carries its own level
// (`meditate_core::bell_volume`); the core picks the alarm step and
// the player gains for it on this device's alarm curve.
// `MeditateAudio.kt` sets the alarm stream to the step the loudest
// ringing bell needs and trims each bell by its gain there, so the
// user's alarm volume doesn't change how loud a bell rings. The curve
// is read once at startup (`audio::alarm_steps`) and kept here so
// every bell play site can read it without a JNI round trip.

use meditate_core::bell_volume::{AlarmCurve, AlarmLevel, BellVolume};
use std::sync::Mutex;

static CURVE: Mutex<Option<AlarmCurve>> = Mutex::new(None);

/// Remember the reported steps: `[min, max, dB of each step...]`;
/// `None` or a short report falls back to an even curve over 1..=7.
pub fn set_curve(report: Option<Vec<f32>>) {
    let curve = match report.as_deref() {
        Some([min, max, db @ ..]) => AlarmCurve::from_report(*min as i32, *max as i32, db),
        _ => AlarmCurve::from_report(1, FALLBACK_MAX_STEP, &[]),
    };
    *CURVE.lock().unwrap_or_else(|e| e.into_inner()) = Some(curve);
}

/// AOSP's default top alarm step, for a device that reported nothing.
const FALLBACK_MAX_STEP: i32 = 7;

/// Alarm step and player gains for a bell at `volume`.
pub fn level(volume: BellVolume) -> AlarmLevel {
    let curve = CURVE.lock().unwrap_or_else(|e| e.into_inner()).clone();
    volume.alarm_level(&curve.unwrap_or_else(|| AlarmCurve::from_report(1, FALLBACK_MAX_STEP, &[])))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(rel: &str) -> String {
        std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel))
            .unwrap()
    }

    // One test for the global: parallel tests would race on it.
    #[test]
    fn the_level_follows_the_reported_steps() {
        let loudest = BellVolume::from_percent(100.0);
        // Before reading the steps: the fallback 1..=7.
        assert_eq!(level(loudest).step, 7);
        set_curve(Some(vec![1.0, 15.0]));
        assert_eq!(level(loudest).step, 15);
        assert_eq!(level(BellVolume::from_percent(0.0)).step, 1);
        // A failed read falls back instead of keeping the stale steps.
        set_curve(None);
        assert_eq!(level(loudest).step, 7);
        set_curve(Some(vec![1.0]));
        assert_eq!(level(loudest).step, 7);
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
            assert!(call.contains("alarm_volume::level("), "{call}");
        }
        assert!(!lib.contains("bell_gain"), "the global bell gain is gone");
        assert!(!lib.contains("meditate_core::bell_volume::read"), "no global level");
    }

    #[test]
    fn startup_reads_the_steps_and_recovers_the_alarm_volume() {
        let lib = source("src/ui.rs");
        assert!(lib.contains("alarm_volume::set_curve(audio::alarm_steps(app))"));
        assert!(lib.contains("audio::recover_alarm_volume(app)"));
    }

    #[test]
    fn the_level_reaches_the_media_player() {
        let audio = source("src/audio.rs");
        assert!(audio.contains("pub fn play(app: &AndroidApp, path: &str, level: &AlarmLevel)"));
        assert!(audio.contains("\"(Landroid/content/Context;Ljava/lang/String;I[F)J\""));
        let kotlin = source("kotlin/MeditateAudio.kt");
        assert!(kotlin.contains("fun play(context: Context, path: String, step: Int, gains: FloatArray): Long"));
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
            assert!(call.contains("AudioManager.STREAM_ALARM"), "{call}");
            assert!(call.trim_end().ends_with(", 0"), "no flags: {call}");
        }
        assert!(!kotlin.contains("FLAG_SHOW_UI") && !kotlin.contains("FLAG_PLAY_SOUND"));
        // The stream takes the loudest live bell's step, and the step
        // is saved before it is set, so a crash in between is put back.
        let apply = &kotlin[kotlin.find("private fun applyLocked(").unwrap()..];
        let apply = &apply[..apply.find("\n    }\n").unwrap()];
        assert!(apply.contains("levels.values.maxOf { it.step }"));
        assert!(apply.find("putInt(KEY_SET, want)").unwrap() < apply.find("setStreamVolume(").unwrap());
        // Raising trims the players first, so a quiet bell never peaks.
        assert!(apply.find("trimLocked(want)").unwrap() < apply.find("setStreamVolume(").unwrap());
        // A live slider drag moves the stream too.
        let drag = &kotlin[kotlin.find("fun setVolume(context: Context, step: Int, gains: FloatArray)").unwrap()..];
        let drag = &drag[..drag.find("\n    }\n").unwrap()];
        assert!(drag.contains("levels[mp] = Level(step, gains)") && drag.contains("applyLocked(context.applicationContext)"));
        // Set right before playing; followed or restored, once nothing
        // else rings, when a bell ends, fails or won't start, when
        // everything is stopped, and on the next start after a crash.
        let start = &kotlin[kotlin.find("private fun startLocked(").unwrap()..];
        assert!(start.find("applyLocked(app)").unwrap() < start.find("mp.start()").unwrap());
        assert!(kotlin.contains("fun recoverAlarmVolume(context: Context)"));
        assert_eq!(kotlin.matches("synchronized(lock) { finishedLocked(app, mp) }").count(), 2);
        let finished = &kotlin[kotlin.find("private fun finishedLocked(").unwrap()..];
        assert!(finished[..finished.find("\n    }\n").unwrap()].contains("settleLocked(app)"));
        let stop = &kotlin[kotlin.find("fun stop(context: Context)").unwrap()..];
        assert!(stop[..stop.find("\n    }\n").unwrap()].contains("restoreLocked(context.applicationContext)"));
        // Restored only while the stream is still at the step the app set.
        let restore = &kotlin[kotlin.find("private fun restoreSaved(").unwrap()..];
        assert!(restore.contains("== set)"));
    }
}
