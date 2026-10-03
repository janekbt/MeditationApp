//! Bell volume — each bell's own level: the starting bell, every
//! interval bell, each mode's end bell and each Box Breath cue carry
//! one, it syncs with the bell and presets save it.
//!
//! A level is a slider position from `MIN` to `MAX` in `STEP`s.
//! Positions are spaced evenly in decibels, so equal slider moves
//! sound like equal loudness changes. What the ends mean depends on
//! what the platform can do:
//!
//! - Android plays bells on the alarm stream and can set that
//!   stream's volume, so a level is absolute: `MAX` is the loudest the
//!   device plays an alarm, `MIN` the quietest (its lowest alarm
//!   step), whatever the user's alarm volume is. The shell raises the
//!   stream to its top step while a bell plays and scales the bell by
//!   [`BellVolume::absolute_gain`] over the device's [`DeviceRange`].
//! - The desktop can't set an absolute level without changing the
//!   output volume of every app, so a level is relative: `MAX` is the
//!   system volume and the bell is scaled by
//!   [`BellVolume::relative_gain`].

use crate::settings_keys::{end_bell_volume_key_for_mode, STARTING_BELL_VOLUME_KEY};
use serde::{Deserialize, Serialize};

pub const MIN: u8 = 0;
pub const MAX: u8 = 100;
pub const STEP: u8 = 5;
/// The middle: every bell's level until the user moves its slider.
pub const DEFAULT: u8 = 50;

/// Span of the desktop slider below the system volume. Perceived
/// loudness halves about every 10 dB, so the middle is half as loud as
/// the system volume and `MIN` about a quarter — a 40 dB span was
/// inaudible below the middle on laptop speakers.
const RELATIVE_RANGE_DB: f64 = 20.0;

/// A valid bell volume: a multiple of `STEP` within `MIN..=MAX`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(from = "f64", into = "u8")]
pub struct BellVolume(u8);

impl Default for BellVolume {
    fn default() -> Self {
        Self(DEFAULT)
    }
}

impl From<f64> for BellVolume {
    fn from(percent: f64) -> Self {
        Self::from_percent(percent)
    }
}

impl From<BellVolume> for u8 {
    fn from(volume: BellVolume) -> Self {
        volume.0
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

    /// A stored value as text (a setting or an event field): a number
    /// in percent, else the default.
    pub fn parse(text: &str) -> Self {
        text.trim().parse::<f64>().map_or_else(|_| Self::default(), Self::from_percent)
    }

    pub fn percent(self) -> u8 {
        self.0
    }

    /// Desktop: linear amplitude factor relative to the system volume
    /// (1.0 at `MAX`).
    pub fn relative_gain(self) -> f64 {
        self.gain_over(RELATIVE_RANGE_DB)
    }

    /// Android: linear amplitude factor for a bell played with the
    /// alarm stream at its top step, so the bell lands at this level's
    /// share of the device's range: 1.0 at `MAX`, the quietest alarm
    /// step's loudness at `MIN`.
    pub fn absolute_gain(self, range: DeviceRange) -> f64 {
        self.gain_over(range.span_db)
    }

    fn gain_over(self, span_db: f64) -> f64 {
        let below_top_db = span_db * f64::from(MAX - self.0) / f64::from(MAX);
        10f64.powf(-below_top_db / 20.0)
    }
}

/// How far the device's quietest alarm step sits below its loudest,
/// in dB. Measured per device by the Android shell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeviceRange {
    span_db: f64,
}

impl DeviceRange {
    /// Assumed span when the device can't report its steps (Android
    /// before 9). The Fairphone 5's alarm stream spans 40 dB on its
    /// speaker.
    pub const FALLBACK_SPAN_DB: f64 = 40.0;

    /// From the reported loudness of the quietest and loudest alarm
    /// steps, in dB. A report that isn't a usable span (not finite,
    /// not quieter at the bottom) falls back.
    pub fn from_steps_db(quietest_db: f64, loudest_db: f64) -> Self {
        let span_db = loudest_db - quietest_db;
        if span_db.is_finite() && span_db > 0.0 {
            Self { span_db }
        } else {
            Self::fallback()
        }
    }

    pub fn fallback() -> Self {
        Self { span_db: Self::FALLBACK_SPAN_DB }
    }

    pub fn span_db(self) -> f64 {
        self.span_db
    }
}

/// A bell whose volume the Setup view edits in place. Interval bells
/// aren't here: their volume is a field of the row the bell editor
/// saves (`IntervalBell::volume`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BellSlot {
    Starting,
    End(crate::db::SessionMode),
    BoxBreathCue(crate::db::BoxBreathPhaseId),
}

/// The slot's stored volume; the middle when unset.
pub fn read(db: &crate::db::Database, slot: BellSlot) -> BellVolume {
    let setting = |key: &str| BellVolume::parse(&db.get_setting(key, "").unwrap_or_default());
    match slot {
        BellSlot::Starting => setting(STARTING_BELL_VOLUME_KEY),
        BellSlot::End(mode) => setting(end_bell_volume_key_for_mode(mode)),
        BellSlot::BoxBreathCue(phase) => db
            .get_box_breath_phase(phase)
            .ok()
            .flatten()
            .map_or_else(BellVolume::default, |p| p.volume),
    }
}

/// Store the slot's volume (synced like the bell's other settings).
pub fn write(db: &crate::db::Database, slot: BellSlot, volume: BellVolume) -> crate::db::Result<()> {
    let percent = volume.percent().to_string();
    match slot {
        BellSlot::Starting => db.set_setting(STARTING_BELL_VOLUME_KEY, &percent),
        BellSlot::End(mode) => db.set_setting(end_bell_volume_key_for_mode(mode), &percent),
        BellSlot::BoxBreathCue(phase) => {
            let Some(p) = db.get_box_breath_phase(phase)? else { return Ok(()) };
            db.set_box_breath_phase(
                phase, p.enabled, p.signal_mode, p.sound_uuid.as_str(), p.pattern_uuid.as_str(),
                volume,
            )
        }
    }
}

/// The sound the slot's bell plays, for the preview after the slider
/// is released.
pub fn sound_uuid(db: &crate::db::Database, slot: BellSlot) -> String {
    use crate::settings_keys::{end_bell_sound_key_for_mode, read_str};
    let bowl = crate::seeds::BUNDLED_BOWL_UUID;
    match slot {
        BellSlot::Starting => read_str(db, "starting_bell_sound", bowl),
        BellSlot::End(mode) => read_str(db, end_bell_sound_key_for_mode(mode), bowl),
        BellSlot::BoxBreathCue(phase) => db
            .get_box_breath_phase(phase)
            .ok()
            .flatten()
            .map_or_else(|| bowl.to_string(), |p| p.sound_uuid.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_steps() -> impl Iterator<Item = BellVolume> {
        (MIN..=MAX).step_by(STEP as usize).map(|p| BellVolume::from_percent(p.into()))
    }

    fn db(gain: f64) -> f64 {
        20.0 * gain.log10()
    }

    #[test]
    fn default_is_the_middle() {
        assert_eq!(BellVolume::default().percent(), 50);
    }

    #[test]
    fn from_percent_keeps_valid_steps() {
        for p in [0, 5, 10, 50, 85, 95, 100] {
            assert_eq!(BellVolume::from_percent(p.into()).percent(), p);
        }
    }

    #[test]
    fn from_percent_rounds_to_the_nearest_step() {
        assert_eq!(BellVolume::from_percent(2.4).percent(), 0);
        assert_eq!(BellVolume::from_percent(2.5).percent(), 5);
        assert_eq!(BellVolume::from_percent(52.0).percent(), 50);
        assert_eq!(BellVolume::from_percent(57.9).percent(), 60);
        assert_eq!(BellVolume::from_percent(97.4).percent(), 95);
    }

    #[test]
    fn from_percent_clamps_into_range() {
        assert_eq!(BellVolume::from_percent(-30.0).percent(), MIN);
        assert_eq!(BellVolume::from_percent(101.0).percent(), MAX);
        assert_eq!(BellVolume::from_percent(f64::INFINITY).percent(), MAX);
        assert_eq!(BellVolume::from_percent(f64::NEG_INFINITY).percent(), MIN);
        assert_eq!(BellVolume::from_percent(f64::NAN), BellVolume::default());
    }

    #[test]
    fn every_step_is_a_multiple_of_step_within_range() {
        let steps: Vec<u8> = all_steps().map(BellVolume::percent).collect();
        assert_eq!(steps.len(), 21);
        assert!(steps.iter().all(|p| p % STEP == 0 && (MIN..=MAX).contains(p)));
    }

    #[test]
    fn parse_reads_stored_text_and_repairs_bad_values() {
        for (stored, want) in [
            ("40", 40),
            (" 40 ", 40),
            ("42", 40),
            ("62.5", 65),
            ("-5", MIN),
            ("250", MAX),
            ("", DEFAULT),
            ("loud", DEFAULT),
            ("NaN", DEFAULT),
        ] {
            assert_eq!(BellVolume::parse(stored).percent(), want, "{stored:?}");
        }
    }

    #[test]
    fn relative_gain_spans_twenty_db_below_the_system_volume() {
        assert_eq!(BellVolume::from_percent(100.0).relative_gain(), 1.0);
        assert!((db(BellVolume::from_percent(50.0).relative_gain()) - -10.0).abs() < 1e-9);
        assert!((db(BellVolume::from_percent(0.0).relative_gain()) - -20.0).abs() < 1e-9);
    }

    #[test]
    fn absolute_gain_spans_the_whole_device_range() {
        // FP5 speaker: quietest alarm step -40 dB, loudest 0 dB.
        let range = DeviceRange::from_steps_db(-40.0, 0.0);
        assert_eq!(BellVolume::from_percent(100.0).absolute_gain(range), 1.0);
        // MIN is exactly the device's quietest alarm step, never louder.
        assert!((db(BellVolume::from_percent(0.0).absolute_gain(range)) - -40.0).abs() < 1e-9);
        assert!((db(BellVolume::from_percent(50.0).absolute_gain(range)) - -20.0).abs() < 1e-9);
    }

    #[test]
    fn absolute_gain_follows_each_devices_range() {
        let wide = DeviceRange::from_steps_db(-58.0, -4.0);
        assert!((wide.span_db() - 54.0).abs() < 1e-9);
        assert!((db(BellVolume::from_percent(0.0).absolute_gain(wide)) - -54.0).abs() < 1e-9);
    }

    #[test]
    fn gains_are_evenly_spaced_in_decibels() {
        for gain in [
            |v: BellVolume| v.relative_gain(),
            |v: BellVolume| v.absolute_gain(DeviceRange::from_steps_db(-40.0, 0.0)),
        ] {
            let steps: Vec<f64> = all_steps().map(|v| db(gain(v))).collect();
            let first = steps[1] - steps[0];
            assert!(first > 0.0);
            for pair in steps.windows(2) {
                assert!((pair[1] - pair[0] - first).abs() < 1e-9, "{pair:?}");
            }
        }
    }

    #[test]
    fn gains_rise_with_every_step_and_stay_in_unit_range() {
        let range = DeviceRange::fallback();
        for gains in [
            all_steps().map(BellVolume::relative_gain).collect::<Vec<_>>(),
            all_steps().map(|v| v.absolute_gain(range)).collect::<Vec<_>>(),
        ] {
            assert!(gains.windows(2).all(|w| w[0] < w[1]));
            assert!(gains.iter().all(|g| *g > 0.0 && *g <= 1.0));
        }
    }

    #[test]
    fn an_unusable_device_report_falls_back() {
        for (quietest, loudest) in [
            (f64::NEG_INFINITY, 0.0),
            (f64::NAN, 0.0),
            (0.0, 0.0),
            (0.0, -40.0),
        ] {
            assert_eq!(
                DeviceRange::from_steps_db(quietest, loudest),
                DeviceRange::fallback(),
                "{quietest} {loudest}",
            );
        }
        assert_eq!(DeviceRange::fallback().span_db(), 40.0);
    }

    #[test]
    fn json_round_trips_and_repairs() {
        let v = BellVolume::from_percent(35.0);
        assert_eq!(serde_json::to_string(&v).unwrap(), "35");
        assert_eq!(serde_json::from_str::<BellVolume>("35").unwrap(), v);
        assert_eq!(serde_json::from_str::<BellVolume>("37").unwrap().percent(), 35);
        assert_eq!(serde_json::from_str::<BellVolume>("400").unwrap().percent(), MAX);
    }

    // ── Slots ──────────────────────────────────────────────────────

    use crate::db::{BoxBreathPhaseId, Database, SessionMode};

    fn every_slot() -> Vec<BellSlot> {
        let mut slots = vec![BellSlot::Starting];
        slots.extend([SessionMode::Timer, SessionMode::Guided, SessionMode::BoxBreath].map(BellSlot::End));
        slots.extend(BoxBreathPhaseId::all().iter().map(|p| BellSlot::BoxBreathCue(*p)));
        slots
    }

    fn seeded() -> Database {
        let db = Database::open_in_memory().unwrap();
        db.seed_box_breath_phases().unwrap();
        db
    }

    #[test]
    fn every_slot_starts_at_the_middle() {
        let db = seeded();
        for slot in every_slot() {
            assert_eq!(read(&db, slot), BellVolume::default(), "{slot:?}");
        }
    }

    #[test]
    fn each_slot_keeps_its_own_volume() {
        let db = seeded();
        let slots = every_slot();
        for (i, slot) in slots.iter().enumerate() {
            write(&db, *slot, BellVolume::from_percent((i * 10) as f64)).unwrap();
        }
        for (i, slot) in slots.iter().enumerate() {
            assert_eq!(read(&db, *slot).percent() as usize, i * 10, "{slot:?}");
        }
    }

    #[test]
    fn a_slots_volume_is_what_its_bell_rings_at() {
        use crate::bells::{box_breath_cues_from_db, end_bell_cue_from_db, starting_bell_cue_from_db, DisplayMode};
        let db = seeded();
        db.set_setting("starting_bell_active", "true").unwrap();
        write(&db, BellSlot::Starting, BellVolume::from_percent(20.0)).unwrap();
        write(&db, BellSlot::End(SessionMode::Guided), BellVolume::from_percent(70.0)).unwrap();
        db.set_box_breath_phase(
            BoxBreathPhaseId::HoldOut, true, crate::bells::SignalMode::Sound,
            crate::seeds::BUNDLED_BOWL_UUID, crate::seeds::BUNDLED_PATTERN_PULSE_UUID,
            BellVolume::default(),
        )
        .unwrap();
        write(&db, BellSlot::BoxBreathCue(BoxBreathPhaseId::HoldOut), BellVolume::from_percent(90.0)).unwrap();
        assert_eq!(starting_bell_cue_from_db(&db, SessionMode::Timer).unwrap().volume.percent(), 20);
        assert_eq!(
            end_bell_cue_from_db(&db, DisplayMode::Countdown, SessionMode::Guided).unwrap().volume.percent(),
            70,
        );
        assert_eq!(box_breath_cues_from_db(&db).hold_out.unwrap().volume.percent(), 90);
    }

    #[test]
    fn writing_a_cues_volume_keeps_its_other_settings() {
        let db = seeded();
        db.set_box_breath_phase(
            BoxBreathPhaseId::In, true, crate::bells::SignalMode::Both, "sound-x", "pattern-y",
            BellVolume::default(),
        )
        .unwrap();
        write(&db, BellSlot::BoxBreathCue(BoxBreathPhaseId::In), BellVolume::from_percent(35.0)).unwrap();
        let p = db.get_box_breath_phase(BoxBreathPhaseId::In).unwrap().unwrap();
        assert!(p.enabled);
        assert_eq!(p.signal_mode, crate::bells::SignalMode::Both);
        assert_eq!(p.sound_uuid, "sound-x");
        assert_eq!(p.pattern_uuid, "pattern-y");
        assert_eq!(p.volume.percent(), 35);
    }

    #[test]
    fn the_preview_plays_the_slots_own_sound() {
        let db = seeded();
        assert_eq!(sound_uuid(&db, BellSlot::Starting), crate::seeds::BUNDLED_BOWL_UUID);
        db.set_setting("starting_bell_sound", "start-sound").unwrap();
        db.set_setting(
            crate::settings_keys::end_bell_sound_key_for_mode(SessionMode::BoxBreath),
            "end-sound",
        )
        .unwrap();
        db.set_box_breath_phase(
            BoxBreathPhaseId::Out, true, crate::bells::SignalMode::Sound, "cue-sound",
            crate::seeds::BUNDLED_PATTERN_PULSE_UUID, BellVolume::default(),
        )
        .unwrap();
        assert_eq!(sound_uuid(&db, BellSlot::Starting), "start-sound");
        assert_eq!(sound_uuid(&db, BellSlot::End(SessionMode::BoxBreath)), "end-sound");
        assert_eq!(sound_uuid(&db, BellSlot::End(SessionMode::Timer)), crate::seeds::BUNDLED_BOWL_UUID);
        assert_eq!(sound_uuid(&db, BellSlot::BoxBreathCue(BoxBreathPhaseId::Out)), "cue-sound");
    }
}
