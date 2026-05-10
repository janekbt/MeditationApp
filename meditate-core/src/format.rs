use std::time::Duration;

pub fn parse_hms_duration(s: &str) -> Option<Duration> {
    let parts: Vec<&str> = s.split(':').collect();
    // Last component may be fractional ("30.5"); leading components must be integers.
    match parts.as_slice() {
        [m, sec] => {
            let m: u64 = m.parse().ok()?;
            let sec: f64 = sec.parse().ok()?;
            Some(Duration::from_secs(m * 60 + sec.round() as u64))
        }
        [h, m, sec] => {
            let h: u64 = h.parse().ok()?;
            let m: u64 = m.parse().ok()?;
            let sec: f64 = sec.parse().ok()?;
            Some(Duration::from_secs(h * 3600 + m * 60 + sec.round() as u64))
        }
        _ => None,
    }
}

pub fn parse_insighttimer_datetime(s: &str) -> Option<chrono::NaiveDateTime> {
    // InsightTimer export has shipped both shapes across versions/locales:
    //   "10/15/2024 6:30:00 AM"   (12-hour with AM/PM)
    //   "04/20/2026 08:21:14"     (24-hour)
    // Try both so a mixed import works either way.
    chrono::NaiveDateTime::parse_from_str(s, "%m/%d/%Y %l:%M:%S %p")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(s, "%m/%d/%Y %H:%M:%S"))
        .ok()
}

const SESSION_MILESTONES: &[i64] = &[10, 25, 50, 100, 250, 500, 1000, 2500, 5000];

/// Returns `(target, distance_to_target)` for the next session-count milestone,
/// or `None` past the highest milestone.
pub fn next_session_milestone(count: i64) -> Option<(i64, i64)> {
    SESSION_MILESTONES
        .iter()
        .copied()
        .find(|&t| t > count)
        .map(|t| (t, t - count))
}

/// Heatmap level (0–4) for a day's meditated minutes against a daily goal.
/// Bands are percentages of the goal: 0 / 1–32 / 33–79 / 80–119 / 120+.
/// `mins <= 0` → 0; `goal_mins <= 0` (no goal set) → max level on any activity.
pub fn minutes_to_level(mins: i64, goal_mins: i64) -> u8 {
    if mins <= 0 {
        return 0;
    }
    if goal_mins <= 0 {
        return 4;
    }
    let pct = mins.saturating_mul(100) / goal_mins;
    match pct {
        0..=32 => 1,
        33..=79 => 2,
        80..=119 => 3,
        _ => 4,
    }
}

pub fn format_hm_compact(d: Duration) -> String {
    let total_mins = d.as_secs() / 60;
    if total_mins == 0 {
        return "–".to_string();
    }
    let h = total_mins / 60;
    let m = total_mins % 60;
    if h >= 100 {
        return format!("{h}h");
    }
    match (h, m) {
        (0, _) => format!("{m}m"),
        (_, 0) => format!("{h}h"),
        _ => format!("{h}h {m}m"),
    }
}

pub fn format_hm_mins(d: Duration) -> String {
    let total_mins = d.as_secs() / 60;
    let h = total_mins / 60;
    let m = total_mins % 60;
    match (h, m) {
        (0, _) => format!("{m}m"),
        (_, 0) => format!("{h}h"),
        _ => format!("{h}h {m}m"),
    }
}

/// "h/m output from a seconds-precision input." Despite the name (which
/// reflects the input precision), this drops sub-minute remainder for
/// stats display where seconds are noise. Use `format_time` for live
/// session display where seconds matter.
pub fn format_hm_secs(d: Duration) -> String {
    let total = d.as_secs();
    if total == 0 {
        return "–".to_string();
    }
    let h = total / 3600;
    let m = (total % 3600) / 60;
    match (h, m) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

pub fn format_time(d: Duration) -> String {
    let total = d.as_secs();
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h:02}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

/// How much past the planned countdown the user has gone.
/// `target.saturating_sub(elapsed)` semantics: 0 before they reach the
/// target, the difference once they've crossed it. Used by the
/// running-page "Add MM:SS ?" button to show the bonus duration that
/// would be committed if the user taps it.
pub fn overtime(target: Duration, elapsed: Duration) -> Duration {
    elapsed.saturating_sub(target)
}

/// Build the dynamic label for the running-page "Add" button. Caller
/// supplies the localized prefix word ("Add" in English, "Hinzufügen"
/// in German, etc. — gettext-translated on the gtk side); the format
/// itself is `"<prefix> <MM:SS> ?"`. Trailing space + question mark
/// match the existing GTK shell rendering.
pub fn overtime_button_label(prefix: &str, overtime: Duration) -> String {
    format!("{prefix} {} ?", format_time(overtime))
}

// ── Translatable subtitle helpers ────────────────────────────────────
//
// Pattern: core returns a typed key/struct capturing the structural
// decision; the shell maps each variant to a localized string via its
// own i18n stack. See `feedback_meditate_i18n_typed_keys` memory.

/// Decision key for the "interval-bells: how many enabled?" subtitle.
/// The shell renders each variant via its translator (e.g.
/// `gettext("None enabled")`, `gettext("{n} enabled").replace(...)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntervalsCountKey {
    None,
    One,
    Many(usize),
}

pub fn intervals_count_key(enabled_count: usize) -> IntervalsCountKey {
    match enabled_count {
        0 => IntervalsCountKey::None,
        1 => IntervalsCountKey::One,
        n => IntervalsCountKey::Many(n),
    }
}

/// Decision key for the bell-count chip in the preset-row subtitle.
/// Variants match `IntervalsCountKey` minus the `None` arm — the
/// caller of `preset_subtitle_parts` only sees this when at least
/// one bell is configured AND interval-bells are enabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BellsPart {
    One,
    Many(usize),
}

/// What the box-breath running session ends on after one cycle wrap:
/// keep going as a stopwatch, or stop when a target duration is hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxBreathAfter {
    Stopwatch,
    Duration { mins: u32 },
}

/// Timing-part decision for the preset-row subtitle. The shell renders
/// each variant via its translator + the parameters carried alongside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimingPart {
    Stopwatch,
    Duration {
        mins: u32,
    },
    BoxBreath {
        inhale_secs: u32,
        hold_full_secs: u32,
        exhale_secs: u32,
        hold_empty_secs: u32,
        after: BoxBreathAfter,
    },
}

/// Structural decomposition of a preset-row subtitle. Caller stitches
/// the localized timing chip + the resolved label name (looked up by
/// `label_uuid` against the caller's name table) + the localized
/// bells chip with its own separator (the GTK shell uses
/// `" · "`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetSubtitleParts {
    pub timing: TimingPart,
    /// `Some(uuid)` when the preset has the label expander enabled
    /// AND a label UUID pinned. The shell looks up the name from its
    /// own label table; resolution failures (uuid missing from the
    /// table, e.g. label was deleted) collapse to no label part in
    /// the rendered subtitle.
    pub label_uuid: Option<String>,
    pub bells: Option<BellsPart>,
}

/// Decompose the preset's `config_json` blob into the structural
/// decisions the shell needs to render the row's subtitle. Returns
/// `None` on JSON parse failure (the row is corrupt, the GTK shell
/// renders an empty subtitle in that case).
pub fn preset_subtitle_parts(config_json: &str) -> Option<PresetSubtitleParts> {
    use crate::preset_config::{PresetConfig, PresetTiming};
    let cfg = PresetConfig::from_json(config_json).ok()?;

    let timing = match cfg.timing {
        PresetTiming::Timer { stopwatch: true, .. } => TimingPart::Stopwatch,
        PresetTiming::Timer { stopwatch: false, duration_secs } => TimingPart::Duration {
            mins: duration_secs / 60,
        },
        PresetTiming::BoxBreath {
            stopwatch,
            inhale_secs,
            hold_full_secs,
            exhale_secs,
            hold_empty_secs,
            duration_secs,
        } => TimingPart::BoxBreath {
            inhale_secs,
            hold_full_secs,
            exhale_secs,
            hold_empty_secs,
            after: if stopwatch {
                BoxBreathAfter::Stopwatch
            } else {
                BoxBreathAfter::Duration { mins: duration_secs / 60 }
            },
        },
    };

    let label_uuid = if cfg.label.enabled {
        cfg.label.uuid.clone()
    } else {
        None
    };

    let bells = if cfg.interval_bells.enabled && !cfg.interval_bells.bells.is_empty() {
        let n = cfg.interval_bells.bells.len();
        Some(if n == 1 { BellsPart::One } else { BellsPart::Many(n) })
    } else {
        None
    };

    Some(PresetSubtitleParts {
        timing,
        label_uuid,
        bells,
    })
}

/// Bounds + default for the preparation-time silence in seconds.
///
/// Min 5 s — anything shorter feels accidental. Max 5 min — keeps the
/// SpinRow tractable and avoids a "the app froze" reading. Default 30 s
/// is long enough to settle, short enough to feel snappy.
pub const PREP_SECS_MIN: u32 = 5;
pub const PREP_SECS_MAX: u32 = 300;
pub const PREP_SECS_DEFAULT: u32 = 30;

/// Compute the elapsed-secs boundary when the next ring of an
/// interval bell should fire.
///
/// `last_ring_secs` is the elapsed-secs boundary of the previous ring
/// (use 0 for the first ring of a session). `base_min` and `jitter_pct`
/// come from the bell row. `random_unit` is a caller-supplied uniform
/// in `[0, 1)`; mapping it inside the helper keeps the helper pure and
/// testable while letting the shell choose its RNG (xorshift, system
/// time, whatever).
///
/// With `jitter_pct == 0` the offset is exactly `base_min * 60` and
/// `random_unit` is ignored. With non-zero jitter the offset is in
/// `[base * (1 - j/100), base * (1 + j/100)]`, picked linearly from
/// `random_unit`.
pub fn next_interval_ring_secs(
    last_ring_secs: u64,
    base_min: u32,
    jitter_pct: u32,
    random_unit: f64,
) -> u64 {
    let base_secs = (base_min as u64).saturating_mul(60).max(1);
    if jitter_pct == 0 {
        return last_ring_secs + base_secs;
    }
    let span = base_secs as f64 * (jitter_pct as f64) / 100.0;
    // [0, 1) → [-span, +span). Centre (0.5) lands on zero offset.
    let offset = (random_unit - 0.5) * 2.0 * span;
    let next_secs = ((base_secs as f64) + offset).round().max(1.0) as u64;
    last_ring_secs + next_secs
}

/// Compute the elapsed-secs boundary for a "T minutes from session
/// start" bell, or `None` if the bell would overlap the starting bell
/// (offset==0) or the completion sound (offset>=target).
///
/// In stopwatch mode, `total_target_secs` is `None` — only the
/// zero-offset overlap rule applies.
pub fn fixed_from_start_target_secs(
    offset_min: u32,
    total_target_secs: Option<u64>,
) -> Option<u64> {
    let offset_secs = (offset_min as u64) * 60;
    if offset_secs == 0 {
        return None;
    }
    match total_target_secs {
        Some(t) if offset_secs >= t => None,
        _ => Some(offset_secs),
    }
}

/// Compute the elapsed-secs boundary for a "T minutes before session
/// end" bell. Only meaningful in countdown mode — stopwatch mode has
/// no end so the shell skips this kind altogether. Returns `None` if
/// the bell would overlap the completion sound (offset==0) or land
/// at/before session start (offset>=total).
pub fn fixed_from_end_target_secs(
    offset_min: u32,
    total_target_secs: u64,
) -> Option<u64> {
    let offset_secs = (offset_min as u64) * 60;
    if offset_secs == 0 || offset_secs >= total_target_secs {
        return None;
    }
    Some(total_target_secs - offset_secs)
}

/// Decide whether to enter the Preparing state at session start.
///
/// `Some(d)` means schedule a prep tick of `d` and play the starting
/// bell at the end of it; `None` means skip prep and go straight to
/// Running. A 0-second prep is treated as "no prep" — bouncing through
/// Preparing for an instant would just create a flicker.
pub fn prep_target_duration(prep_active: bool, prep_secs: u32) -> Option<Duration> {
    if prep_active && prep_secs > 0 {
        Some(Duration::from_secs(prep_secs as u64))
    } else {
        None
    }
}

/// Parse a settings-table preparation-time value into a clamped u32.
///
/// Returns `PREP_SECS_DEFAULT` for empty / non-numeric / negative input
/// (anything `u32::from_str` rejects), and clamps in-range integers to
/// `[PREP_SECS_MIN, PREP_SECS_MAX]`. The shell never has to think about
/// sanitising a raw string read from the DB.
pub fn parse_prep_secs(s: &str) -> u32 {
    s.parse::<u32>()
        .map(|n| n.clamp(PREP_SECS_MIN, PREP_SECS_MAX))
        .unwrap_or(PREP_SECS_DEFAULT)
}

/// Hero-label text for a running Timer-mode session.
///
/// `target = Some(d)` means the user picked a duration; the label counts
/// down (`format_time(target - elapsed)`). `target = None` means the
/// stopwatch toggle is on; the label counts up (`format_time(elapsed)`).
/// Saturating subtraction: if a tick lands a beat past `target`, the
/// caller gets `00:00` instead of an underflow panic.
pub fn running_text(target: Option<Duration>, elapsed: Duration) -> String {
    match target {
        Some(t) => format_time(t.saturating_sub(elapsed)),
        None => format_time(elapsed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_time_zero_shows_double_zero() {
        assert_eq!(format_time(Duration::ZERO), "00:00");
    }

    // ── overtime ──────────────────────────────────────────────────────

    #[test]
    fn overtime_is_zero_before_target() {
        assert_eq!(
            overtime(Duration::from_secs(600), Duration::from_secs(0)),
            Duration::ZERO
        );
        assert_eq!(
            overtime(Duration::from_secs(600), Duration::from_secs(599)),
            Duration::ZERO
        );
    }

    #[test]
    fn overtime_is_zero_at_exact_target() {
        // Hitting the target is "session complete", not "1 second over".
        // The transition into Overtime state happens here too.
        assert_eq!(
            overtime(Duration::from_secs(600), Duration::from_secs(600)),
            Duration::ZERO
        );
    }

    #[test]
    fn overtime_is_difference_past_target() {
        assert_eq!(
            overtime(Duration::from_secs(600), Duration::from_secs(630)),
            Duration::from_secs(30)
        );
    }

    #[test]
    fn overtime_button_label_uses_supplied_prefix() {
        // Prefix passed in as a translated word; format itself is fixed.
        assert_eq!(
            overtime_button_label("Add", Duration::from_secs(45)),
            "Add 00:45 ?"
        );
        assert_eq!(
            overtime_button_label("Hinzufügen", Duration::from_secs(45)),
            "Hinzufügen 00:45 ?"
        );
    }

    #[test]
    fn overtime_button_label_at_zero_overtime_reads_double_zero() {
        // The label shows the moment-of-transition: just hit the
        // target, no bonus accumulated yet, button is "Add 00:00 ?".
        assert_eq!(
            overtime_button_label("Add", Duration::ZERO),
            "Add 00:00 ?"
        );
    }

    #[test]
    fn overtime_button_label_carries_through_to_hours() {
        // Sub-hour vs hour-or-more — same formatter as format_time
        // which uses two-digit hours per item 4's consolidation.
        assert_eq!(
            overtime_button_label("Add", Duration::from_secs(3661)),
            "Add 01:01:01 ?"
        );
    }

    // ── intervals_count_key ───────────────────────────────────────────

    #[test]
    fn intervals_count_key_zero_is_none() {
        assert_eq!(intervals_count_key(0), IntervalsCountKey::None);
    }

    #[test]
    fn intervals_count_key_one_is_one() {
        assert_eq!(intervals_count_key(1), IntervalsCountKey::One);
    }

    #[test]
    fn intervals_count_key_many_carries_count() {
        assert_eq!(intervals_count_key(5), IntervalsCountKey::Many(5));
        assert_eq!(intervals_count_key(99), IntervalsCountKey::Many(99));
    }

    // ── preset_subtitle_parts ─────────────────────────────────────────

    use crate::preset_config::{
        PresetBoxBreathCues, PresetConfig, PresetEndBell, PresetIntervalBell,
        PresetIntervalBells, PresetLabel, PresetStartingBell, PresetTiming,
    };

    fn cfg_to_json(cfg: &PresetConfig) -> String {
        serde_json::to_string(cfg).unwrap()
    }

    fn timer_cfg(stopwatch: bool, duration_secs: u32) -> PresetConfig {
        PresetConfig {
            label: PresetLabel::default(),
            starting_bell: PresetStartingBell::default(),
            interval_bells: PresetIntervalBells::default(),
            end_bell: PresetEndBell::default(),
            timing: PresetTiming::Timer { stopwatch, duration_secs },
            cues_signal_mode: "both".into(),
            keep_screen_awake: false,
            box_breath_cues: PresetBoxBreathCues::default(),
        }
    }

    fn box_breath_cfg(
        stopwatch: bool,
        duration_secs: u32,
        in_secs: u32,
        hold_in: u32,
        out_secs: u32,
        hold_out: u32,
    ) -> PresetConfig {
        PresetConfig {
            label: PresetLabel::default(),
            starting_bell: PresetStartingBell::default(),
            interval_bells: PresetIntervalBells::default(),
            end_bell: PresetEndBell::default(),
            timing: PresetTiming::BoxBreath {
                stopwatch,
                inhale_secs: in_secs,
                hold_full_secs: hold_in,
                exhale_secs: out_secs,
                hold_empty_secs: hold_out,
                duration_secs,
            },
            cues_signal_mode: "both".into(),
            keep_screen_awake: false,
            box_breath_cues: PresetBoxBreathCues::default(),
        }
    }

    #[test]
    fn preset_subtitle_parts_returns_none_for_corrupt_json() {
        assert!(preset_subtitle_parts("not json").is_none());
        assert!(preset_subtitle_parts("").is_none());
    }

    #[test]
    fn preset_subtitle_parts_timer_stopwatch() {
        let json = cfg_to_json(&timer_cfg(true, 600));
        let parts = preset_subtitle_parts(&json).unwrap();
        assert_eq!(parts.timing, TimingPart::Stopwatch);
        assert_eq!(parts.label_uuid, None);
        assert_eq!(parts.bells, None);
    }

    #[test]
    fn preset_subtitle_parts_timer_duration_in_minutes() {
        // 600 s = 10 min — duration arrives in minutes.
        let json = cfg_to_json(&timer_cfg(false, 600));
        let parts = preset_subtitle_parts(&json).unwrap();
        assert_eq!(parts.timing, TimingPart::Duration { mins: 10 });
    }

    #[test]
    fn preset_subtitle_parts_box_breath_stopwatch_carries_phase_durations() {
        let json = cfg_to_json(&box_breath_cfg(true, 0, 4, 4, 4, 4));
        let parts = preset_subtitle_parts(&json).unwrap();
        assert_eq!(
            parts.timing,
            TimingPart::BoxBreath {
                inhale_secs: 4,
                hold_full_secs: 4,
                exhale_secs: 4,
                hold_empty_secs: 4,
                after: BoxBreathAfter::Stopwatch,
            }
        );
    }

    #[test]
    fn preset_subtitle_parts_box_breath_duration_carries_minutes() {
        // 4-7-8-0 with a 10-min cap.
        let json = cfg_to_json(&box_breath_cfg(false, 600, 4, 7, 8, 0));
        let parts = preset_subtitle_parts(&json).unwrap();
        assert_eq!(
            parts.timing,
            TimingPart::BoxBreath {
                inhale_secs: 4,
                hold_full_secs: 7,
                exhale_secs: 8,
                hold_empty_secs: 0,
                after: BoxBreathAfter::Duration { mins: 10 },
            }
        );
    }

    #[test]
    fn preset_subtitle_parts_label_uuid_set_only_when_enabled_and_present() {
        let mut cfg = timer_cfg(false, 600);
        // Disabled → None even if uuid is filled in.
        cfg.label = PresetLabel { enabled: false, uuid: Some("u-1".into()) };
        let json = cfg_to_json(&cfg);
        assert_eq!(preset_subtitle_parts(&json).unwrap().label_uuid, None);

        // Enabled + uuid → Some.
        cfg.label = PresetLabel { enabled: true, uuid: Some("u-1".into()) };
        let json = cfg_to_json(&cfg);
        assert_eq!(
            preset_subtitle_parts(&json).unwrap().label_uuid,
            Some("u-1".to_string())
        );

        // Enabled but no uuid → None (mode-default fallback is the
        // shell's job; subtitle just shows nothing).
        cfg.label = PresetLabel { enabled: true, uuid: None };
        let json = cfg_to_json(&cfg);
        assert_eq!(preset_subtitle_parts(&json).unwrap().label_uuid, None);
    }

    #[test]
    fn preset_subtitle_parts_bells_uses_one_or_many() {
        let one_bell = PresetIntervalBell {
            kind: "interval".into(),
            minutes: 5,
            jitter_pct: 0,
            sound_uuid: "s".into(),
            enabled: true,
            signal_mode: "sound".into(),
            vibration_pattern_uuid: String::new(),
        };

        let mut cfg = timer_cfg(false, 600);
        // Disabled → None.
        cfg.interval_bells = PresetIntervalBells { enabled: false, bells: vec![one_bell.clone()] };
        let json = cfg_to_json(&cfg);
        assert_eq!(preset_subtitle_parts(&json).unwrap().bells, None);

        // Enabled but empty → None.
        cfg.interval_bells = PresetIntervalBells { enabled: true, bells: vec![] };
        let json = cfg_to_json(&cfg);
        assert_eq!(preset_subtitle_parts(&json).unwrap().bells, None);

        // Enabled with one → BellsPart::One.
        cfg.interval_bells = PresetIntervalBells { enabled: true, bells: vec![one_bell.clone()] };
        let json = cfg_to_json(&cfg);
        assert_eq!(
            preset_subtitle_parts(&json).unwrap().bells,
            Some(BellsPart::One)
        );

        // Enabled with three → BellsPart::Many(3).
        cfg.interval_bells = PresetIntervalBells {
            enabled: true,
            bells: vec![one_bell.clone(), one_bell.clone(), one_bell.clone()],
        };
        let json = cfg_to_json(&cfg);
        assert_eq!(
            preset_subtitle_parts(&json).unwrap().bells,
            Some(BellsPart::Many(3))
        );
    }

    // ── running_text ──────────────────────────────────────────────────
    // The hero label on the running timer page. Two regimes folded into
    // one helper so the merged Timer mode (M.2 onwards) can branch on a
    // single Option<Duration> rather than a TimerMode variant.

    #[test]
    fn running_text_targeted_shows_remaining() {
        assert_eq!(
            running_text(Some(Duration::from_secs(60)), Duration::from_secs(10)),
            "00:50"
        );
    }

    #[test]
    fn running_text_open_ended_shows_elapsed() {
        assert_eq!(
            running_text(None, Duration::from_secs(10)),
            "00:10"
        );
    }

    #[test]
    fn running_text_targeted_clamps_to_zero_when_elapsed_overshoots() {
        // Tick scheduling sometimes lands one tick after target; saturating_sub
        // gives "00:00" instead of underflowing.
        assert_eq!(
            running_text(Some(Duration::from_secs(60)), Duration::from_secs(75)),
            "00:00"
        );
    }

    #[test]
    fn running_text_targeted_at_start_shows_full_target() {
        assert_eq!(
            running_text(Some(Duration::from_secs(600)), Duration::ZERO),
            "10:00"
        );
    }

    #[test]
    fn running_text_open_ended_at_start_shows_zero() {
        assert_eq!(running_text(None, Duration::ZERO), "00:00");
    }

    #[test]
    fn format_time_pads_under_minute() {
        assert_eq!(format_time(Duration::from_secs(5)), "00:05");
    }

    #[test]
    fn format_time_under_hour_shows_minutes_seconds() {
        assert_eq!(format_time(Duration::from_secs(65)), "01:05");
    }

    #[test]
    fn format_time_at_hour_adds_hours_segment() {
        assert_eq!(format_time(Duration::from_secs(3661)), "01:01:01");
    }

    #[test]
    fn parse_hms_duration_accepts_minutes_seconds() {
        assert_eq!(parse_hms_duration("1:30"), Some(Duration::from_secs(90)));
    }

    #[test]
    fn parse_hms_duration_accepts_hours_minutes_seconds() {
        assert_eq!(
            parse_hms_duration("1:30:45"),
            Some(Duration::from_secs(5445))
        );
    }

    #[test]
    fn parse_hms_duration_rejects_garbage() {
        assert_eq!(parse_hms_duration("garbage"), None);
        assert_eq!(parse_hms_duration(""), None);
        assert_eq!(parse_hms_duration("60"), None); // single component is ambiguous
        assert_eq!(parse_hms_duration("1:30:45:00"), None);
        assert_eq!(parse_hms_duration(":30"), None);
    }

    #[test]
    fn parse_hms_duration_rounds_fractional_seconds() {
        // 1:30.5 = 1m 30.5s → rounds to 91s
        assert_eq!(parse_hms_duration("1:30.5"), Some(Duration::from_secs(91)));
        // 1:30.4 → rounds down to 90s
        assert_eq!(parse_hms_duration("1:30.4"), Some(Duration::from_secs(90)));
        // Three-part with fractional last component.
        assert_eq!(
            parse_hms_duration("1:00:30.5"),
            Some(Duration::from_secs(3631))
        );
    }

    #[test]
    fn format_hm_secs_drops_sub_minute_and_uses_em_dash_for_zero() {
        // Stats display: seconds are noise; show "–" for empty.
        assert_eq!(format_hm_secs(Duration::ZERO), "–");
        assert_eq!(format_hm_secs(Duration::from_secs(30)), "0m");
        assert_eq!(format_hm_secs(Duration::from_secs(90)), "1m");
        assert_eq!(format_hm_secs(Duration::from_secs(3600)), "1h");
        assert_eq!(format_hm_secs(Duration::from_secs(3665)), "1h 1m");
    }

    #[test]
    fn format_hm_mins_drops_seconds_and_unused_units() {
        assert_eq!(format_hm_mins(Duration::ZERO), "0m");
        assert_eq!(format_hm_mins(Duration::from_secs(30)), "0m");
        assert_eq!(format_hm_mins(Duration::from_secs(90)), "1m");
        assert_eq!(format_hm_mins(Duration::from_secs(3600)), "1h");
        assert_eq!(format_hm_mins(Duration::from_secs(3661)), "1h 1m");
    }

    #[test]
    fn format_hm_compact_uses_em_dash_for_empty() {
        // Zero is empty, not "0m" — heatmap cells with no data render "–".
        assert_eq!(format_hm_compact(Duration::ZERO), "–");
    }

    #[test]
    fn format_hm_compact_clips_at_100h() {
        assert_eq!(format_hm_compact(Duration::from_secs(90)), "1m");
        assert_eq!(format_hm_compact(Duration::from_secs(3600)), "1h");
        assert_eq!(format_hm_compact(Duration::from_secs(3661)), "1h 1m");
        // h >= 100 clips minutes — keeps the cell narrow in the heatmap.
        assert_eq!(
            format_hm_compact(Duration::from_secs(100 * 3600)),
            "100h"
        );
        assert_eq!(
            format_hm_compact(Duration::from_secs(100 * 3600 + 60)),
            "100h"
        );
    }

    #[test]
    fn minutes_to_level_buckets_at_thresholds_0_33_80_120_percent_of_goal() {
        // Bands are percentages of the daily goal, not absolute minutes.
        // With goal=100, the percentage and the minutes happen to match.
        assert_eq!(minutes_to_level(0, 100), 0);
        assert_eq!(minutes_to_level(1, 100), 1);
        assert_eq!(minutes_to_level(32, 100), 1);
        assert_eq!(minutes_to_level(33, 100), 2);
        assert_eq!(minutes_to_level(79, 100), 2);
        assert_eq!(minutes_to_level(80, 100), 3);
        assert_eq!(minutes_to_level(119, 100), 3);
        assert_eq!(minutes_to_level(120, 100), 4);
        assert_eq!(minutes_to_level(1000, 100), 4);
    }

    #[test]
    fn minutes_to_level_scales_with_goal() {
        // 18 mins against a 15-min goal = 120% → level 4 (high achievement).
        assert_eq!(minutes_to_level(18, 15), 4);
        // Same 18 mins against a 100-min goal = 18% → level 1.
        assert_eq!(minutes_to_level(18, 100), 1);
    }

    #[test]
    fn minutes_to_level_handles_no_goal_and_negative() {
        // No goal set → any positive activity clips to max level.
        assert_eq!(minutes_to_level(60, 0), 4);
        // Negative goal also treated as no goal.
        assert_eq!(minutes_to_level(60, -1), 4);
        // Negative minutes → no activity.
        assert_eq!(minutes_to_level(-5, 100), 0);
    }

    #[test]
    fn next_session_milestone_returns_target_and_distance() {
        // (target, distance_to_target).
        assert_eq!(next_session_milestone(0), Some((10, 10)));
        assert_eq!(next_session_milestone(9), Some((10, 1)));
        assert_eq!(next_session_milestone(10), Some((25, 15)));
        assert_eq!(next_session_milestone(24), Some((25, 1)));
        assert_eq!(next_session_milestone(499), Some((500, 1)));
        assert_eq!(next_session_milestone(2499), Some((2500, 1)));
        assert_eq!(next_session_milestone(4999), Some((5000, 1)));
    }

    #[test]
    fn next_session_milestone_returns_none_past_ceiling() {
        assert_eq!(next_session_milestone(5000), None);
        assert_eq!(next_session_milestone(5001), None);
        assert_eq!(next_session_milestone(10_000), None);
    }

    #[test]
    fn parse_insighttimer_datetime_handles_am_and_pm() {
        let am = parse_insighttimer_datetime("10/15/2024 6:30:00 AM").unwrap();
        assert_eq!(am.to_string(), "2024-10-15 06:30:00");
        let pm = parse_insighttimer_datetime("10/15/2024 6:30:00 PM").unwrap();
        assert_eq!(pm.to_string(), "2024-10-15 18:30:00");
    }

    #[test]
    fn parse_insighttimer_datetime_handles_24_hour() {
        // Some InsightTimer exports are 24-hour without AM/PM.
        let dt = parse_insighttimer_datetime("04/20/2026 08:21:14").unwrap();
        assert_eq!(dt.to_string(), "2026-04-20 08:21:14");
        let evening = parse_insighttimer_datetime("04/20/2026 20:00:00").unwrap();
        assert_eq!(evening.to_string(), "2026-04-20 20:00:00");
    }

    #[test]
    fn parse_insighttimer_datetime_rejects_garbage() {
        assert_eq!(parse_insighttimer_datetime(""), None);
        assert_eq!(parse_insighttimer_datetime("not a date"), None);
        // ISO format is rejected — this parser is for InsightTimer's specific shape.
        assert_eq!(parse_insighttimer_datetime("2024-10-15T06:30:00"), None);
        // Month 13 is invalid in either format.
        assert_eq!(parse_insighttimer_datetime("13/01/2024 08:30:00"), None);
    }

    // ── parse_prep_secs ──────────────────────────────────────────────
    // Settings-table values for the Preparation-Time SpinRow round-trip
    // through this helper so the shell never has to think about garbage,
    // empty strings, or out-of-range values from a future hand-edit.

    #[test]
    fn parse_prep_secs_constants_have_expected_shape() {
        // Min / max bound a "settle in" silence — long enough to feel
        // intentional, short enough not to feel like a frozen UI.
        assert_eq!(PREP_SECS_MIN, 5);
        assert_eq!(PREP_SECS_MAX, 300);
        assert_eq!(PREP_SECS_DEFAULT, 30);
        // Default must lie in the allowed range.
        assert!(PREP_SECS_MIN <= PREP_SECS_DEFAULT && PREP_SECS_DEFAULT <= PREP_SECS_MAX);
    }

    #[test]
    fn parse_prep_secs_passes_through_in_range() {
        assert_eq!(parse_prep_secs("5"), 5);
        assert_eq!(parse_prep_secs("30"), 30);
        assert_eq!(parse_prep_secs("60"), 60);
        assert_eq!(parse_prep_secs("300"), 300);
    }

    #[test]
    fn parse_prep_secs_clamps_below_min() {
        assert_eq!(parse_prep_secs("0"), PREP_SECS_MIN);
        assert_eq!(parse_prep_secs("4"), PREP_SECS_MIN);
    }

    #[test]
    fn parse_prep_secs_clamps_above_max() {
        assert_eq!(parse_prep_secs("301"), PREP_SECS_MAX);
        assert_eq!(parse_prep_secs("100000"), PREP_SECS_MAX);
    }

    #[test]
    fn parse_prep_secs_falls_back_to_default_on_garbage() {
        assert_eq!(parse_prep_secs(""), PREP_SECS_DEFAULT);
        assert_eq!(parse_prep_secs("garbage"), PREP_SECS_DEFAULT);
        // Negative — u32 parse fails, default kicks in.
        assert_eq!(parse_prep_secs("-5"), PREP_SECS_DEFAULT);
        // Stray decimals — u32 parse fails, default kicks in.
        assert_eq!(parse_prep_secs("30.0"), PREP_SECS_DEFAULT);
    }

    // ── prep_target_duration ─────────────────────────────────────────
    // Decides whether the timer should enter the Preparing state. The
    // shell calls this once at on_start; Some(d) → schedule prep,
    // None → skip prep and go straight to Running.

    #[test]
    fn prep_target_duration_returns_some_only_when_active_and_positive() {
        assert_eq!(
            prep_target_duration(true, 30),
            Some(Duration::from_secs(30))
        );
        assert_eq!(
            prep_target_duration(true, PREP_SECS_MIN),
            Some(Duration::from_secs(PREP_SECS_MIN as u64))
        );
        assert_eq!(
            prep_target_duration(true, PREP_SECS_MAX),
            Some(Duration::from_secs(PREP_SECS_MAX as u64))
        );
    }

    #[test]
    fn prep_target_duration_is_none_when_inactive() {
        // Switch off → no prep, regardless of seconds value.
        assert_eq!(prep_target_duration(false, 30), None);
        assert_eq!(prep_target_duration(false, 0), None);
    }

    #[test]
    fn prep_target_duration_is_none_when_zero_seconds() {
        // A 0-second prep is just "no prep" — don't bounce through
        // the Preparing state for an instant.
        assert_eq!(prep_target_duration(true, 0), None);
    }

    // ── Interval-bell scheduling helpers (B.3.2) ─────────────────────
    // Pure functions the running tick uses to decide when each
    // configured bell should fire. random_unit is a caller-supplied
    // [0, 1) random for the jittered intervals — so the helper itself
    // stays deterministic and trivially testable.

    #[test]
    fn next_interval_ring_with_zero_jitter_is_exactly_base_minutes() {
        // No jitter → next ring is last_ring + base_min*60 regardless
        // of random_unit. random_unit gets ignored entirely.
        assert_eq!(next_interval_ring_secs(0, 5, 0, 0.0), 300);
        assert_eq!(next_interval_ring_secs(0, 5, 0, 0.5), 300);
        assert_eq!(next_interval_ring_secs(0, 5, 0, 0.999), 300);
        assert_eq!(next_interval_ring_secs(300, 5, 0, 0.5), 600);
    }

    #[test]
    fn next_interval_ring_with_random_unit_at_centre_is_exactly_base() {
        // random_unit = 0.5 maps to the zero offset (the centre of the
        // jitter window), so the next ring is exactly base.
        assert_eq!(next_interval_ring_secs(0, 9, 30, 0.5), 540);
        assert_eq!(next_interval_ring_secs(540, 9, 30, 0.5), 1080);
    }

    #[test]
    fn next_interval_ring_random_unit_at_zero_lands_at_lower_bound() {
        // random_unit=0.0 → -span offset from base. For base=9 ±30%,
        // span = 540 * 0.30 = 162, so 540 - 162 = 378.
        assert_eq!(next_interval_ring_secs(0, 9, 30, 0.0), 378);
    }

    #[test]
    fn next_interval_ring_random_unit_just_below_one_lands_near_upper_bound() {
        // random_unit just below 1.0 → just below +span. For base=9 ±30%,
        // upper bound is 540 + 162 = 702.
        let v = next_interval_ring_secs(0, 9, 30, 0.9999);
        assert!(v <= 702 && v >= 700, "got {}", v);
    }

    #[test]
    fn next_interval_ring_stays_within_jitter_window_for_every_unit() {
        // Spot-check a fan of random_units to confirm the window holds.
        let base = 9 * 60u64;
        let jitter_pct = 30u32;
        let span = base as f64 * jitter_pct as f64 / 100.0;
        let lo = (base as f64 - span).round() as u64;
        let hi = (base as f64 + span).round() as u64;
        for i in 0..=10 {
            let u = (i as f64) / 10.0;
            let v = next_interval_ring_secs(0, 9, jitter_pct, u);
            assert!(v >= lo && v <= hi,
                "u={} produced {} outside [{}, {}]", u, v, lo, hi);
        }
    }

    #[test]
    fn next_interval_ring_zero_minutes_clamps_to_one_second() {
        // base=0 doesn't make sense (UI prevents it via SpinRow min),
        // but the helper still has to return a usable u64 — clamping
        // to 1 second is the harmless choice.
        assert_eq!(next_interval_ring_secs(0, 0, 0, 0.5), 1);
    }

    // ── fixed_from_start_target_secs ──────────────────────────────
    // Maps an "T minutes from session start" config to the elapsed-
    // seconds boundary the tick checks. None means "the bell would
    // overlap the start or end sound — drop it".

    #[test]
    fn fixed_from_start_returns_offset_when_inside_target() {
        // 10 minutes from start in a 30-minute countdown → fire at 600s.
        assert_eq!(
            fixed_from_start_target_secs(10, Some(1800)),
            Some(600)
        );
    }

    #[test]
    fn fixed_from_start_returns_offset_in_stopwatch_mode() {
        // No target (stopwatch) → as long as offset > 0, the bell fires.
        assert_eq!(
            fixed_from_start_target_secs(10, None),
            Some(600)
        );
    }

    #[test]
    fn fixed_from_start_is_none_at_zero_offset() {
        // 0-min from start would land at session start — overlaps the
        // starting bell, drop it.
        assert_eq!(fixed_from_start_target_secs(0, Some(1800)), None);
        assert_eq!(fixed_from_start_target_secs(0, None), None);
    }

    #[test]
    fn fixed_from_start_is_none_at_or_beyond_target() {
        // Bell would land at or past the completion sound — drop it.
        assert_eq!(fixed_from_start_target_secs(30, Some(1800)), None);
        assert_eq!(fixed_from_start_target_secs(45, Some(1800)), None);
    }

    // ── fixed_from_end_target_secs ────────────────────────────────
    // Maps a "T minutes before session end" config to the elapsed-
    // seconds boundary. Only meaningful in countdown mode; the shell
    // skips this kind entirely in stopwatch mode.

    #[test]
    fn fixed_from_end_returns_target_minus_offset() {
        // 5 min before the end of a 30-min session → fire at elapsed=1500.
        assert_eq!(fixed_from_end_target_secs(5, 1800), Some(1500));
    }

    #[test]
    fn fixed_from_end_is_none_at_zero_offset() {
        // 0-min before the end is exactly the end — overlaps completion.
        assert_eq!(fixed_from_end_target_secs(0, 1800), None);
    }

    #[test]
    fn fixed_from_end_is_none_at_or_beyond_total() {
        // Bell would land at or before session start — drop it.
        assert_eq!(fixed_from_end_target_secs(30, 1800), None);
        assert_eq!(fixed_from_end_target_secs(45, 1800), None);
    }
}
