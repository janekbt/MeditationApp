// Process-wide bell volume for the Android shell. The level itself
// (range, steps, curve, storage) is decided in
// `meditate_core::bell_volume`; this only caches the resulting player
// gain so every bell play site can read it without taking the DB
// lock. Guided voice audio never reads it — it is not a bell.

use meditate_core::bell_volume::BellVolume;
use std::sync::atomic::{AtomicU32, Ordering};

static GAIN_BITS: AtomicU32 = AtomicU32::new(0x3f80_0000); // 1.0_f32

/// Remember `volume` as the gain for every bell played from now on.
pub fn set(volume: BellVolume) {
    GAIN_BITS.store((volume.gain() as f32).to_bits(), Ordering::Relaxed);
}

/// MediaPlayer volume for a bell (1.0 = the system alarm volume).
#[cfg(any(target_os = "android", test))]
pub fn get() -> f32 {
    f32::from_bits(GAIN_BITS.load(Ordering::Relaxed))
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
    fn gain_starts_full_and_follows_the_level() {
        assert_eq!(get(), 1.0);
        for percent in [5.0, 50.0, 100.0] {
            let volume = BellVolume::from_percent(percent);
            set(volume);
            assert_eq!(get(), volume.gain() as f32);
        }
    }

    /// Every bell reaches MediaPlayer through `audio::play`, and every
    /// call hands it this gain.
    #[test]
    fn every_bell_play_passes_the_gain() {
        let lib = source("src/lib.rs");
        let calls: Vec<&str> = lib
            .match_indices("audio::play(")
            .map(|(at, _)| lib[at..].split_once(';').unwrap().0)
            .collect();
        assert!(!calls.is_empty());
        for call in calls {
            assert!(call.contains("bell_gain::get()"), "{call}");
        }
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
}
