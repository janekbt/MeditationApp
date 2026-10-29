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
//!   step), whatever the user's alarm volume is. While a bell plays
//!   the shell sets the stream to the step [`BellVolume::alarm_level`]
//!   picks on the device's [`AlarmCurve`] and trims the bell by that
//!   level's gain. With bells played as media, the same happens on the
//!   media stream: on the speaker's media curve, or in headphones on
//!   the media steps up to the hearing-safety step
//!   ([`AlarmCurve::headphones`]).
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

/// Slack for the device's dB report, which arrives as f32.
const DB_TOLERANCE: f64 = 1e-3;

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

    /// Android: the alarm step and player gains for this level on the
    /// device's curve. The step goes by the slider's position, so the
    /// slider works even where the reported curve is wrong or the
    /// player gain is ignored; the curve can only raise it, when that
    /// step plays quieter than the level. The gain trims the rest.
    pub fn alarm_level(self, curve: &AlarmCurve) -> AlarmLevel {
        let steps = (curve.db.len() - 1) as i32;
        let top = curve.db[steps as usize];
        let target = top - (top - curve.db[0]) * f64::from(MAX - self.0) / f64::from(MAX);
        let by_position = (i32::from(self.0) * steps + i32::from(MAX) - 1) / i32::from(MAX);
        let loud_enough = curve.db.iter().position(|&d| d >= target - DB_TOLERANCE);
        let index = by_position.max(loud_enough.map_or(steps, |i| i as i32));
        let gains = (0..=curve.first + steps)
            .map(|s| match usize::try_from(s - curve.first) {
                Ok(i) => 10f64.powf(((target - curve.db[i]) / 20.0).min(0.0)) as f32,
                Err(_) => 1.0,
            })
            .collect();
        AlarmLevel { step: curve.first + index, gains }
    }

    fn gain_over(self, span_db: f64) -> f64 {
        let below_top_db = span_db * f64::from(MAX - self.0) / f64::from(MAX);
        10f64.powf(-below_top_db / 20.0)
    }
}

/// The device's alarm steps and how loud each plays on the speaker,
/// as the Android shell reports them. Step 0 mutes, so the curve
/// starts at step 1 at the lowest.
#[derive(Debug, Clone, PartialEq)]
pub struct AlarmCurve {
    first: i32,
    /// dB of `first..=first + db.len() - 1`, never falling.
    db: Vec<f64>,
}

/// Android's hearing-safety limit for media on headphones: index 10
/// (AOSP's `config_safe_media_volume_index`), which AudioService keeps
/// whatever the device's step count. No public API reads the device's
/// own value.
const SAFE_MEDIA_STEP: i32 = 10;

/// Where a bell puts the alarm stream and how much its player trims.
#[derive(Debug, Clone, PartialEq)]
pub struct AlarmLevel {
    /// The alarm step this bell needs.
    pub step: i32,
    /// The player's gain with the stream at step `s`, at index `s`:
    /// other bells may hold the stream higher.
    pub gains: Vec<f32>,
}

impl AlarmCurve {
    /// Span of the even curve used when the device can't report its
    /// steps (Android before 9) or reports something that isn't a
    /// curve. The Fairphone 5's alarm stream spans 40 dB on its
    /// speaker.
    pub const FALLBACK_SPAN_DB: f64 = 40.0;

    /// From the stream's `min..=max` steps and the dB of each of them
    /// (empty when the device can't report).
    pub fn from_report(min: i32, max: i32, steps_db: &[f32]) -> Self {
        let first = min.max(1);
        let count = (max.max(first) - first + 1) as usize;
        let reported: Vec<f64> = steps_db
            .iter()
            .skip((first - min).max(0) as usize)
            .map(|&d| f64::from(d))
            .collect();
        let is_curve = reported.len() == count
            && reported.iter().all(|d| d.is_finite())
            && reported.windows(2).all(|w| w[1] >= w[0] - DB_TOLERANCE)
            && reported[count - 1] - reported[0] > DB_TOLERANCE;
        let db = if is_curve {
            reported
        } else {
            let step_db = Self::FALLBACK_SPAN_DB / (count.max(2) - 1) as f64;
            (0..count).map(|i| (i as f64 - (count - 1) as f64) * step_db).collect()
        };
        Self { first, db }
    }

    /// The media steps for headphones from the same kind of report, cut
    /// at the hearing-safety step so a bell never asks for a step Android
    /// would hold back behind its warning.
    pub fn headphones(min: i32, max: i32, steps_db: &[f32]) -> Self {
        let safe = SAFE_MEDIA_STEP.min(max);
        let kept = usize::try_from(safe - min + 1).unwrap_or(0);
        Self::from_report(min, safe, steps_db.get(..kept).unwrap_or(steps_db))
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

    /// How loud a bell plays alone, in dB below the loudest alarm step:
    /// its step's loudness plus its gain there.
    fn alarm_db(v: BellVolume, curve: &AlarmCurve) -> f64 {
        let level = v.alarm_level(curve);
        let s = level.step as usize;
        curve_db(curve, level.step) + db(f64::from(level.gains[s]))
    }

    fn curve_db(curve: &AlarmCurve, step: i32) -> f64 {
        curve.db[(step - curve.first) as usize] - curve.db.last().unwrap()
    }

    fn pct(p: f64) -> BellVolume {
        BellVolume::from_percent(p)
    }

    fn gain_db(level: &AlarmLevel, step: usize) -> f64 {
        db(f64::from(level.gains[step]))
    }

    #[test]
    fn the_slider_picks_the_alarm_step_by_its_position() {
        // Reports only 1.2 dB from the quietest to the loudest step, and
        // almost all of it below step 2: by the curve alone 50 % would be
        // step 2.
        let flat = AlarmCurve::from_report(1, 7, &[-1.2, -0.1, -0.08, -0.06, -0.04, -0.02, 0.0]);
        let steps: Vec<i32> = [0.0, 50.0, 100.0].map(|p| pct(p).alarm_level(&flat).step).to_vec();
        assert_eq!(steps, [1, 4, 7]);
    }

    #[test]
    fn a_volume_between_two_steps_is_trimmed_by_the_gain() {
        let curve = AlarmCurve::from_report(1, 5, &[-40.0, -30.0, -20.0, -10.0, 0.0]);
        let level = pct(40.0).alarm_level(&curve);
        assert_eq!(level.step, 3);
        assert!((gain_db(&level, 3) - -4.0).abs() < 1e-4, "{level:?}");
        assert_eq!(level.gains.len(), 6, "one gain per stream step 0..=5");
    }

    #[test]
    fn the_curve_can_only_raise_the_step() {
        let steep = AlarmCurve::from_report(1, 5, &[-60.0, -55.0, -50.0, -20.0, 0.0]);
        let level = pct(25.0).alarm_level(&steep);
        assert_eq!(level.step, 4);
        assert!((gain_db(&level, 4) - -25.0).abs() < 1e-4, "{level:?}");
    }

    #[test]
    fn a_quieter_bell_keeps_its_level_under_a_louder_one() {
        let curve = AlarmCurve::from_report(1, 5, &[-40.0, -30.0, -20.0, -10.0, 0.0]);
        let quiet = pct(40.0).alarm_level(&curve);
        let loud = pct(100.0).alarm_level(&curve);
        assert_eq!(loud.step, 5);
        assert!((gain_db(&quiet, 5) - -24.0).abs() < 1e-4, "{quiet:?}");
        // Below its own step the gain can't make up the difference.
        assert_eq!(quiet.gains[2], 1.0);
    }

    #[test]
    fn a_report_that_is_not_a_curve_falls_back_to_even_steps() {
        let even = AlarmCurve::from_report(1, 5, &[-40.0, -30.0, -20.0, -10.0, 0.0]);
        for (min, report) in [
            (1, vec![]),
            (1, vec![-40.0, 0.0]),
            (1, vec![-40.0, f32::NAN, -20.0, -10.0, 0.0]),
            (1, vec![-40.0, -30.0, -35.0, -10.0, 0.0]),
            (1, vec![0.0; 5]),
        ] {
            assert_eq!(AlarmCurve::from_report(min, 5, &report), even, "{report:?}");
        }
        // Step 0 mutes: a curve reported from 0 starts at step 1.
        let from_zero = AlarmCurve::from_report(0, 5, &[f32::NEG_INFINITY, -40.0, -30.0, -20.0, -10.0, 0.0]);
        assert_eq!(from_zero, even);
        assert_eq!(pct(0.0).alarm_level(&from_zero).step, 1);
    }

    #[test]
    fn the_headphone_slider_stops_at_the_hearing_safety_step() {
        // Media steps 0..=15, -60 dB to 0 dB evenly.
        let report: Vec<f32> = (0..=15).map(|s| -60.0 + 4.0 * s as f32).collect();
        let curve = AlarmCurve::headphones(0, 15, &report);
        assert_eq!(pct(0.0).alarm_level(&curve).step, 1);
        assert_eq!(pct(50.0).alarm_level(&curve).step, 6);
        assert_eq!(pct(100.0).alarm_level(&curve).step, 10);
        assert!(all_steps().all(|v| v.alarm_level(&curve).step <= 10));
        // Android's limit is index 10 whatever the step count: a device
        // with 30 media steps still warns above 10.
        let fine = AlarmCurve::headphones(0, 30, &[]);
        assert_eq!(pct(100.0).alarm_level(&fine).step, 10);
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
    fn gains_are_evenly_spaced_in_decibels() {
        for gain in [
            |v: BellVolume| db(v.relative_gain()),
            |v: BellVolume| alarm_db(v, &AlarmCurve::from_report(1, 7, &[])),
        ] {
            let steps: Vec<f64> = all_steps().map(gain).collect();
            let first = steps[1] - steps[0];
            assert!(first > 0.0);
            for pair in steps.windows(2) {
                assert!((pair[1] - pair[0] - first).abs() < 1e-4, "{pair:?}"); // gains are f32
            }
        }
    }

    #[test]
    fn gains_rise_with_every_step_and_stay_in_unit_range() {
        let curve = AlarmCurve::from_report(1, 7, &[]);
        for gains in [
            all_steps().map(BellVolume::relative_gain).collect::<Vec<_>>(),
            all_steps().map(|v| 10f64.powf(alarm_db(v, &curve) / 20.0)).collect::<Vec<_>>(),
        ] {
            assert!(gains.windows(2).all(|w| w[0] < w[1]));
            assert!(gains.iter().all(|g| *g > 0.0 && *g <= 1.0));
        }
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
