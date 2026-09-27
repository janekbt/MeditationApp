//! Bell volume — one app-wide level for every bell and bell preview
//! (starting, interval, end, Box Breath phase cues). Guided voice
//! audio is not a bell and is never scaled by it.
//!
//! The level is a percentage of the system volume (an app can't play
//! louder than the OS allows), stored per device: laptop speakers and
//! a phone need different levels, so it lives in the device-local
//! `sync_state` store rather than the synced `settings` table.
//!
//! Shells show a slider over `MIN..=MAX` in `STEP`s, persist with
//! [`write`], and multiply every bell player's volume by
//! [`BellVolume::gain`].

use crate::db::Database;

/// Device-local store key. Wire format: never rename without a
/// migration.
pub const KEY: &str = "bell_volume_pct";

/// Lowest slider position. Not 0: silencing bells is what each bell's
/// signal mode is for, and a 0 % bell that silently doesn't ring would
/// look like a bug.
pub const MIN: u8 = 5;
pub const MAX: u8 = 100;
pub const STEP: u8 = 5;
/// Full volume — what every bell played at before the setting existed.
pub const DEFAULT: u8 = MAX;

/// Loudness span of the slider: 0 % would sit this far below `MAX`.
/// Spacing positions evenly in decibels makes equal slider moves sound
/// like equal loudness changes. Perceived loudness halves about every
/// 10 dB, so 20 dB puts 50 % at half as loud and `MIN` at about a
/// quarter — a 40 dB span was inaudible below the middle on laptop
/// speakers.
const RANGE_DB: f64 = 20.0;

/// A valid bell volume: a multiple of `STEP` within `MIN..=MAX`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BellVolume(u8);

impl Default for BellVolume {
    fn default() -> Self {
        Self(DEFAULT)
    }
}

impl BellVolume {
    /// From a slider position or stored value in percent: rounds to the
    /// nearest `STEP` and clamps into `MIN..=MAX`. NaN yields the
    /// default.
    pub fn from_percent(percent: f64) -> Self {
        if percent.is_nan() {
            return Self::default();
        }
        let clamped = percent.clamp(MIN.into(), MAX.into());
        let step = f64::from(STEP);
        let snapped = ((clamped / step).round() * step).clamp(MIN.into(), MAX.into());
        // In range and a whole multiple of STEP, so the cast is exact.
        Self(snapped as u8)
    }

    pub fn percent(self) -> u8 {
        self.0
    }

    /// Linear amplitude factor for a player's volume (1.0 = unchanged).
    pub fn gain(self) -> f64 {
        let below_full_db = RANGE_DB * f64::from(MAX - self.0) / f64::from(MAX);
        10f64.powf(-below_full_db / 20.0)
    }
}

/// The stored level, or the default when it's missing or unreadable.
pub fn read(db: &Database) -> BellVolume {
    db.get_sync_state(KEY, "")
        .ok()
        .and_then(|s| s.trim().parse::<f64>().ok())
        .map_or_else(BellVolume::default, BellVolume::from_percent)
}

pub fn write(db: &Database, volume: BellVolume) -> crate::db::Result<()> {
    db.set_sync_state(KEY, &volume.percent().to_string())
}

/// The sound played once after the slider is released, so the user
/// hears the new level: the Timer end bell they chose, else the
/// bundled bowl.
pub fn preview_sound_uuid(db: &Database) -> String {
    let key = crate::settings_keys::end_bell_sound_key_for_mode(crate::db::SessionMode::Timer);
    db.get_setting(key, "")
        .ok()
        .filter(|uuid| !uuid.is_empty())
        .unwrap_or_else(|| crate::seeds::BUNDLED_BOWL_UUID.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_steps() -> impl Iterator<Item = BellVolume> {
        (MIN..=MAX).step_by(STEP as usize).map(|p| BellVolume::from_percent(p.into()))
    }

    #[test]
    fn default_is_full_volume_so_existing_users_hear_no_change() {
        assert_eq!(BellVolume::default().percent(), 100);
        assert_eq!(BellVolume::default().gain(), 1.0);
    }

    #[test]
    fn from_percent_keeps_valid_steps() {
        for p in [5, 10, 50, 85, 95, 100] {
            assert_eq!(BellVolume::from_percent(p.into()).percent(), p);
        }
    }

    #[test]
    fn from_percent_rounds_to_the_nearest_step() {
        assert_eq!(BellVolume::from_percent(52.0).percent(), 50);
        assert_eq!(BellVolume::from_percent(52.5).percent(), 55);
        assert_eq!(BellVolume::from_percent(57.9).percent(), 60);
        assert_eq!(BellVolume::from_percent(97.4).percent(), 95);
    }

    #[test]
    fn from_percent_clamps_into_range() {
        assert_eq!(BellVolume::from_percent(0.0).percent(), MIN);
        assert_eq!(BellVolume::from_percent(2.4).percent(), MIN);
        assert_eq!(BellVolume::from_percent(-30.0).percent(), MIN);
        assert_eq!(BellVolume::from_percent(101.0).percent(), MAX);
        assert_eq!(BellVolume::from_percent(1e9).percent(), MAX);
        assert_eq!(BellVolume::from_percent(f64::INFINITY).percent(), MAX);
        assert_eq!(BellVolume::from_percent(f64::NEG_INFINITY).percent(), MIN);
    }

    #[test]
    fn from_percent_nan_is_the_default() {
        assert_eq!(BellVolume::from_percent(f64::NAN), BellVolume::default());
    }

    #[test]
    fn every_step_is_a_multiple_of_step_within_range() {
        let steps: Vec<u8> = all_steps().map(BellVolume::percent).collect();
        assert_eq!(steps.len(), 20);
        assert!(steps.iter().all(|p| p % STEP == 0 && (MIN..=MAX).contains(p)));
    }

    #[test]
    fn gain_endpoints() {
        assert_eq!(BellVolume::from_percent(100.0).gain(), 1.0);
        // MIN sits 19 dB below full: about a quarter as loud, quiet but
        // clearly audible on laptop speakers (40 dB was inaudible there).
        let min = BellVolume::from_percent(MIN.into()).gain();
        assert!((min - 10f64.powf(-19.0 / 20.0)).abs() < 1e-12, "{min}");
    }

    #[test]
    fn gain_is_evenly_spaced_in_decibels() {
        let db = |v: BellVolume| 20.0 * v.gain().log10();
        // -10 dB sounds about half as loud: the slider's middle.
        assert!((db(BellVolume::from_percent(50.0)) - -10.0).abs() < 1e-9);
        let steps: Vec<f64> = all_steps().map(db).collect();
        for pair in steps.windows(2) {
            assert!((pair[1] - pair[0] - 1.0).abs() < 1e-9, "{pair:?}");
        }
    }

    #[test]
    fn gain_rises_with_every_step_and_stays_in_unit_range() {
        let gains: Vec<f64> = all_steps().map(BellVolume::gain).collect();
        assert!(gains.windows(2).all(|w| w[0] < w[1]));
        assert!(gains.iter().all(|g| *g > 0.0 && *g <= 1.0));
    }

    #[test]
    fn read_defaults_on_a_fresh_database() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(read(&db), BellVolume::default());
    }

    #[test]
    fn write_then_read_round_trips() {
        let db = Database::open_in_memory().unwrap();
        for v in all_steps() {
            write(&db, v).unwrap();
            assert_eq!(read(&db), v);
        }
    }

    #[test]
    fn read_repairs_bad_stored_values() {
        let db = Database::open_in_memory().unwrap();
        for (stored, want) in [
            ("", DEFAULT),
            ("loud", DEFAULT),
            ("NaN", DEFAULT),
            (" 40 ", 40),
            ("42", 40),
            ("0", MIN),
            ("-5", MIN),
            ("250", MAX),
            ("62.5", 65),
        ] {
            db.set_sync_state(KEY, stored).unwrap();
            assert_eq!(read(&db).percent(), want, "stored {stored:?}");
        }
    }

    #[test]
    fn the_level_stays_on_this_device() {
        let db = Database::open_in_memory().unwrap();
        let events_before = db.all_events().unwrap().len();
        write(&db, BellVolume::from_percent(30.0)).unwrap();
        assert_eq!(db.all_events().unwrap().len(), events_before, "no sync event");
        assert_eq!(db.get_setting(KEY, "absent").unwrap(), "absent", "not a synced setting");
        assert_eq!(db.get_sync_state(KEY, "").unwrap(), "30");
    }

    #[test]
    fn preview_plays_the_timer_end_bell() {
        let db = Database::open_in_memory().unwrap();
        let key = crate::settings_keys::end_bell_sound_key_for_mode(crate::db::SessionMode::Timer);
        db.set_setting(key, "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0003").unwrap();
        assert_eq!(preview_sound_uuid(&db), "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0003");
    }

    #[test]
    fn preview_falls_back_to_the_bundled_bowl() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(preview_sound_uuid(&db), crate::seeds::BUNDLED_BOWL_UUID);
        let key = crate::settings_keys::end_bell_sound_key_for_mode(crate::db::SessionMode::Timer);
        db.set_setting(key, "").unwrap();
        assert_eq!(preview_sound_uuid(&db), crate::seeds::BUNDLED_BOWL_UUID);
    }
}
