// Absolute bell volume on Android. Each bell carries its own level
// (`meditate_core::bell_volume`); the core maps it onto this device's
// alarm range. `MeditateAudio.kt` raises the alarm stream to its top
// step while a bell plays and scales the bell by this gain, so the
// user's alarm volume doesn't change how loud a bell rings. The range
// is measured once at startup (`audio::alarm_range_db`) and cached
// here so every bell play site can read it without a JNI round trip.

use meditate_core::bell_volume::{BellVolume, DeviceRange};
use std::sync::atomic::{AtomicU64, Ordering};

// The span in dB as f64 bits. 0.0 (not measured yet) is no usable
// span, so it reads as the fallback.
static SPAN_DB_BITS: AtomicU64 = AtomicU64::new(0);

/// Remember the measured range; `None` (Android before 9, or a failed
/// measurement) falls back.
pub fn set_range(steps_db: Option<(f64, f64)>) {
    let range = steps_db.map_or_else(DeviceRange::fallback, |(quietest, loudest)| {
        DeviceRange::from_steps_db(quietest, loudest)
    });
    SPAN_DB_BITS.store(range.span_db().to_bits(), Ordering::Relaxed);
}

fn range() -> DeviceRange {
    DeviceRange::from_steps_db(-f64::from_bits(SPAN_DB_BITS.load(Ordering::Relaxed)), 0.0)
}

/// MediaPlayer volume for a bell at `volume`, with the alarm stream at
/// its top step.
pub fn gain(volume: BellVolume) -> f32 {
    volume.absolute_gain(range()) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(rel: &str) -> String {
        std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel))
            .unwrap()
    }

    fn db(gain: f32) -> f64 {
        20.0 * f64::from(gain).log10()
    }

    // One test for the global: parallel tests would race on it.
    #[test]
    fn the_gain_spans_the_measured_range() {
        let quietest = BellVolume::from_percent(0.0);
        let loudest = BellVolume::from_percent(100.0);
        // Before measuring: the fallback span.
        assert!((db(gain(quietest)) - -DeviceRange::FALLBACK_SPAN_DB).abs() < 1e-3);
        set_range(Some((-58.0, -4.0)));
        assert!((db(gain(quietest)) - -54.0).abs() < 1e-3);
        assert_eq!(gain(loudest), 1.0);
        assert!((db(gain(BellVolume::default())) - -27.0).abs() < 1e-3);
        // A failed measurement falls back instead of keeping stale values.
        set_range(None);
        assert!((db(gain(quietest)) - -DeviceRange::FALLBACK_SPAN_DB).abs() < 1e-3);
        set_range(Some((f64::NEG_INFINITY, 0.0)));
        assert!((db(gain(quietest)) - -DeviceRange::FALLBACK_SPAN_DB).abs() < 1e-3);
    }

    /// Every bell reaches MediaPlayer through `audio::play`, and every
    /// call passes the gain for that bell's own volume.
    #[test]
    fn every_bell_play_passes_its_own_volume() {
        let lib = source("src/lib.rs");
        let calls: Vec<&str> = lib
            .match_indices("audio::play(")
            .map(|(at, _)| lib[at..].split_once(';').unwrap().0)
            .collect();
        assert!(!calls.is_empty());
        for call in calls {
            assert!(call.contains("alarm_volume::gain("), "{call}");
        }
        assert!(!lib.contains("bell_gain"), "the global bell gain is gone");
        assert!(!lib.contains("meditate_core::bell_volume::read"), "no global level");
    }

    #[test]
    fn startup_measures_the_range_and_recovers_the_alarm_volume() {
        let lib = source("src/lib.rs");
        assert!(lib.contains("alarm_volume::set_range(audio::alarm_range_db(app))"));
        assert!(lib.contains("audio::recover_alarm_volume(app)"));
    }

    #[test]
    fn the_gain_reaches_the_media_player() {
        let audio = source("src/audio.rs");
        assert!(audio.contains("pub fn play(app: &AndroidApp, path: &str, gain: f32)"));
        assert!(audio.contains("\"(Landroid/content/Context;Ljava/lang/String;F)J\""));
        let kotlin = source("kotlin/MeditateAudio.kt");
        assert!(kotlin.contains("fun play(context: Context, path: String, gain: Float): Long"));
        assert!(kotlin.contains("mp.setVolume(gain, gain)"));
    }

    /// The alarm stream is raised around each bell and put back after,
    /// without ever showing the system volume panel or playing a click.
    #[test]
    fn the_alarm_stream_is_raised_quietly_and_restored() {
        let kotlin = source("kotlin/MeditateAudio.kt");
        let sets: Vec<&str> = kotlin
            .match_indices("setStreamVolume(")
            .map(|(at, _)| kotlin[at..].split_once(')').unwrap().0)
            .collect();
        assert_eq!(sets.len(), 2, "raise + restore");
        for call in sets {
            assert!(call.contains("AudioManager.STREAM_ALARM"), "{call}");
            assert!(call.trim_end().ends_with(", 0"), "no flags: {call}");
        }
        assert!(!kotlin.contains("FLAG_SHOW_UI") && !kotlin.contains("FLAG_PLAY_SOUND"));
        // Raised right before playing; restored — once nothing else
        // rings — when a bell ends, fails or won't start, when
        // everything is stopped, and on the next start after a crash.
        let start = &kotlin[kotlin.find("private fun startLocked(").unwrap()..];
        assert!(start.find("raiseLocked(app)").unwrap() < start.find("mp.start()").unwrap());
        assert!(kotlin.contains("fun recoverAlarmVolume(context: Context)"));
        assert_eq!(kotlin.matches("synchronized(lock) { finishedLocked(app, mp) }").count(), 2);
        let finished = &kotlin[kotlin.find("private fun finishedLocked(").unwrap()..];
        assert!(finished[..finished.find("\n    }\n").unwrap()].contains("restoreIfIdleLocked(app)"));
        let stop = &kotlin[kotlin.find("fun stop(context: Context)").unwrap()..];
        assert!(stop[..stop.find("\n    }\n").unwrap()].contains("restoreLocked(context.applicationContext)"));
    }
}
