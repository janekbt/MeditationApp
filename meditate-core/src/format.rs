//! User-facing duration formatting + the typed translatable-key
//! enums the shells consume.
//!
//! Two halves:
//!   - **Plain formatters** (`format_time`, `format_hhmm`,
//!     `format_hm_compact`, etc.) — pure number-to-string. The
//!     translatable typed-key helpers below are the i18n-clean way
//!     to render copy; the plain formatters are fine for non-
//!     translatable digit-only output.
//!   - **Typed translatable keys** (`StreakKey`, `SyncedAgoKey`,
//!     `BellTitleKey`, `BellsCountKey`, `TimingKey`, `DateGroupKey`,
//!     …) — every helper that produces user-visible text returns
//!     a typed enum capturing every choice the shell needs to
//!     render; the shell maps each variant to its gettext template.
//!     Tests in core assert on the typed value, never on rendered
//!     strings.

use std::time::Duration;

pub fn parse_hms_duration(s: &str) -> Option<Duration> {
    let parts: Vec<&str> = s.split(':').collect();
    // Last component may be fractional ("30.5"); leading components must be integers.
    // Use checked_* throughout so a hostile import row ("99999999:59:59")
    // returns None rather than panicking in debug builds or silently
    // wrapping in release.
    match parts.as_slice() {
        [m, sec] => {
            let m: u64 = m.parse().ok()?;
            let sec: f64 = sec.parse().ok()?;
            let sec = sec.round();
            if !(0.0..=u64::MAX as f64).contains(&sec) {
                return None;
            }
            let total = m.checked_mul(60)?.checked_add(sec as u64)?;
            Some(Duration::from_secs(total))
        }
        [h, m, sec] => {
            let h: u64 = h.parse().ok()?;
            let m: u64 = m.parse().ok()?;
            let sec: f64 = sec.parse().ok()?;
            let sec = sec.round();
            if !(0.0..=u64::MAX as f64).contains(&sec) {
                return None;
            }
            let total = h
                .checked_mul(3600)?
                .checked_add(m.checked_mul(60)?)?
                .checked_add(sec as u64)?;
            Some(Duration::from_secs(total))
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

/// Audio-duration display: same as `format_time` but without
/// zero-padding the leading component. "2:30" / "1:00:00" instead of
/// "02:30" / "01:00:00". Used for compact metadata (Guided file
/// duration row, log card chip). The distinction matters: a stable-
/// width clock display picks `format_time`; a free-flowing inline
/// label picks `format_duration_brief`.
pub fn format_duration_brief(secs: u32) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Unicode-safe truncate-with-ellipsis. `max_chars` counts
/// `char`s, not bytes — Latin-1 accented forms and CJK input all
/// behave consistently. Returns `s` unchanged when it fits.
pub fn ellipsize(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

/// Pre-rendered hero label for the Setup view's idle state. When
/// stopwatch mode is on the hero shows "00:00" (the upcoming
/// count-up baseline); otherwise it shows the configured target
/// duration in `HH:MM`. The shell resolves the right `target_secs`
/// per mode (Timer's countdown / Box-Breath's session length /
/// Guided's probed file length) and passes it in.
pub fn idle_hero_label(stopwatch_on: bool, target_secs: u32) -> String {
    if stopwatch_on {
        "00:00".to_string()
    } else {
        format_hhmm(target_secs)
    }
}

/// Counter-strip label on the Box-Breath running page. Stopwatch
/// sessions show only the elapsed (no slash); fixed-duration
/// sessions show `elapsed / target`. Re-uses `format_time` so the
/// label format matches the rest of the running view.
pub fn box_breath_counter_label(elapsed: Duration, target: Option<Duration>) -> String {
    match target {
        Some(t) => format!("{} / {}", format_time(elapsed), format_time(t)),
        None => format_time(elapsed),
    }
}

/// Minute-precision "HH:MM" render of a session-length seconds value.
/// Used by the Setup view's hero label and the Duration row's value
/// suffix where seconds aren't shown (minute-aligned by the spinner).
/// Hours zero-pad to two digits to keep the label width stable.
pub fn format_hhmm(secs: u32) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    format!("{h:02}:{m:02}")
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

/// Classifies a `Database::open` failure. Shell maps each variant to
/// translatable AdwStatusPage copy in the recovery error window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DbOpenFailureKey {
    /// On-disk DB was written by a newer build than this one.
    /// Downgrade would silently corrupt; user must install a matching
    /// version or move the file aside.
    SchemaTooNew { db: u32, build: u32 },
    /// Any other failure (IO, permission denied, locked, on-disk
    /// corruption). The diag log carries the specific cause.
    Other,
}

pub fn db_open_failure_key(err: &crate::db::DbError) -> DbOpenFailureKey {
    match err {
        crate::db::DbError::SchemaVersionTooNew { db, build } => {
            DbOpenFailureKey::SchemaTooNew { db: *db, build: *build }
        }
        _ => DbOpenFailureKey::Other,
    }
}

/// Reason a session-save attempt failed. Shell maps each variant
/// to a translatable toast.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionSaveFailureKind {
    DbUnopened,
    StorageError,
}

/// Diag-log line for a session-save failure. The `session_save_failed:`
/// prefix is greppable across versions.
pub fn session_save_failure_log_message(
    kind: SessionSaveFailureKind,
    detail: &str,
) -> String {
    match kind {
        SessionSaveFailureKind::DbUnopened => {
            format!("session_save_failed: db unopened (detail: {detail})")
        }
        SessionSaveFailureKind::StorageError => {
            format!("session_save_failed: storage error: {detail}")
        }
    }
}

/// Decision key for the Setup-view streak chip. The shell translates
/// each variant — "Start your streak today" / "1 day streak" / "{n}
/// days streak" in the gtk shell today; Android picks its own
/// phrasing for the same three cases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreakKey {
    /// User has no current streak — invitation copy.
    Zero,
    /// Exactly one day, singular phrasing.
    One,
    /// Two or more days, plural phrasing with the count carried.
    Many(u32),
}

pub fn streak_key(streak: u32) -> StreakKey {
    match streak {
        0 => StreakKey::Zero,
        1 => StreakKey::One,
        n => StreakKey::Many(n),
    }
}

/// Decision key for any "N session(s)" copy — the log's section
/// caption, the delete-toast title, the "Imported / exported {n}
/// sessions" data toasts in preferences. The shell picks its own
/// singular vs. plural phrasing per call site; this key just owns
/// the partition rule so every shell agrees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionCountKey {
    /// Exactly one session — singular phrasing.
    One,
    /// Zero or two or more sessions — plural phrasing with the count
    /// carried (zero is part of the plural arm by convention).
    Many(usize),
}

pub fn session_count_key(n: usize) -> SessionCountKey {
    match n {
        1 => SessionCountKey::One,
        n => SessionCountKey::Many(n),
    }
}

/// Render a "{n} thing(s)" string from a singular form and a
/// `{n}`-templated plural. Dispatches on `session_count_key`. The
/// caller supplies the already-localized strings; this helper just
/// owns the match + substitution so every shell stops re-deriving
/// it. Singular case ignores `n` (the singular form is "1 thing" or
/// "Session deleted", with no placeholder).
pub fn format_count(singular: &str, plural_template: &str, n: usize) -> String {
    match session_count_key(n) {
        SessionCountKey::One => singular.to_string(),
        SessionCountKey::Many(n) => plural_template.replace("{n}", &n.to_string()),
    }
}

/// Log-card hero "minutes" display from a session's duration_secs.
/// Negatives clamp to zero, rounds to the nearest minute, then
/// floors at 1 so a Log row always shows a non-zero hero number.
/// (A 0-minute session lands on the Log because save still ran;
/// "0" would look broken, so we surface as "1 min" instead.)
pub fn log_card_minutes(duration_secs: i64) -> u64 {
    let mins = (duration_secs.max(0) as u64 + 30) / 60;
    mins.max(1)
}

/// Mini-stat value display: render zero as an en-dash (typographic
/// "no data" marker), otherwise the integer as a string. Used by
/// the Stats view's mini-stat tiles where an empty week shouldn't
/// read as a flat "0".
pub fn mini_stat_or_dash(value: i64) -> String {
    if value == 0 {
        "–".to_string()
    } else {
        value.to_string()
    }
}

/// "Synced N ago" granularity bucket. Step boundaries: under a
/// minute → JustNow; under an hour → Minutes; under a day → Hours;
/// else → Days. Shell maps each variant to its gettext-translated
/// template ("Synced {n} minutes ago" etc.). The decision is
/// portable; the strings are not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncedAgoKey {
    JustNow,
    Minutes(u64),
    Hours(u64),
    Days(u64),
}

/// Bucket `secs_ago` into a `SyncedAgoKey` granularity. Negative
/// inputs (clock jump after a sync, peer's `last_sync_unix_ts` in
/// the future) clamp to `JustNow` rather than "synced -3 minutes
/// ago", which would look like a bug.
pub fn synced_ago_key(secs_ago: i64) -> SyncedAgoKey {
    let s = secs_ago.max(0) as u64;
    if s < 60 {
        SyncedAgoKey::JustNow
    } else if s < 3600 {
        SyncedAgoKey::Minutes(s / 60)
    } else if s < 86_400 {
        SyncedAgoKey::Hours(s / 3600)
    } else {
        SyncedAgoKey::Days(s / 86_400)
    }
}

/// Decision key for the bell-count chip in the preset-row subtitle.
/// Variants match `IntervalsCountKey` minus the `None` arm — the
/// caller of `preset_subtitle_parts` only sees this when at least
/// one bell is configured AND interval-bells are enabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BellsCountKey {
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
pub enum TimingKey {
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
    pub timing: TimingKey,
    /// `Some(uuid)` when the preset has the label expander enabled
    /// AND a label UUID pinned. The shell looks up the name from its
    /// own label table; resolution failures (uuid missing from the
    /// table, e.g. label was deleted) collapse to no label part in
    /// the rendered subtitle.
    pub label_uuid: Option<String>,
    pub bells: Option<BellsCountKey>,
}

/// Decompose the preset's `config_json` blob into the structural
/// decisions the shell needs to render the row's subtitle. Returns
/// `None` on JSON parse failure (the row is corrupt, the GTK shell
/// renders an empty subtitle in that case).
pub fn preset_subtitle_parts(config_json: &str) -> Option<PresetSubtitleParts> {
    use crate::preset_config::{PresetConfig, PresetTiming};
    let cfg = PresetConfig::from_json(config_json).ok()?;

    let timing = match cfg.timing {
        PresetTiming::Timer { stopwatch: true, .. } => TimingKey::Stopwatch,
        PresetTiming::Timer { stopwatch: false, duration_secs } => TimingKey::Duration {
            mins: duration_secs / 60,
        },
        PresetTiming::BoxBreath {
            stopwatch,
            inhale_secs,
            hold_full_secs,
            exhale_secs,
            hold_empty_secs,
            duration_secs,
        } => TimingKey::BoxBreath {
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
        Some(if n == 1 { BellsCountKey::One } else { BellsCountKey::Many(n) })
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
/// Resolve the prep silence the shell should hand to
/// `Session::start_prep` from persisted state. Reads three settings
/// — `preparation_time_active`, `starting_bell_active`,
/// `preparation_time_secs` — and AND-gates them through
/// `prep_target_duration`. Returns `None` when prep is off, when the
/// starting bell is off (silence with no bell is just waiting for
/// nothing), or when the configured secs degenerate. Replaces the
/// inline three-setting closure in the gtk shell's `on_start`.
pub fn prep_plan_from_db(db: &crate::db::Database) -> Option<Duration> {
    use crate::settings_keys::read_bool;
    let active = read_bool(db, "preparation_time_active", false);
    let starting = read_bool(db, "starting_bell_active", false);
    let secs = db
        .get_setting("preparation_time_secs", &PREP_SECS_DEFAULT.to_string())
        .map(|s| parse_prep_secs(&s))
        .unwrap_or(PREP_SECS_DEFAULT);
    prep_target_duration(active && starting, secs)
}

pub fn parse_prep_secs(s: &str) -> u32 {
    s.parse::<u32>()
        .map(|n| n.clamp(PREP_SECS_MIN, PREP_SECS_MAX))
        .unwrap_or(PREP_SECS_DEFAULT)
}

/// `YYYY-MM-DD` of the local-time day the unix timestamp falls on.
/// Used as a HashMap grouping key for log sessions, not shown to
/// the user. Empty string on the rare arithmetic edge case where
/// chrono can't represent the timestamp locally.
pub fn date_group_key(unix_secs: i64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_opt(unix_secs, 0)
        .single()
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

/// Local `HH:MM` of a unix timestamp. 24-hour, locale-independent
/// (no AM/PM); shells that want 12-hour rendering should derive it
/// from their native datetime formatter.
pub fn format_time_of_day(unix_secs: i64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_opt(unix_secs, 0)
        .single()
        .map(|dt| dt.format("%H:%M").to_string())
        .unwrap_or_default()
}

/// Section-header classification for a log row's local date. The
/// shell maps `Today` / `Yesterday` to translated strings, and
/// renders `SameYearOther` / `EarlierYearOther` via its locale-
/// aware datetime formatter (gtk uses `glib::DateTime::format`,
/// Android uses its native one).
///
/// `EarlierYearOther` is `SameYearOther`'s mirror with the year
/// emitted alongside the date, so callers don't have to re-check
/// the year branch themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateGroupKey {
    Today,
    Yesterday,
    SameYearOther,
    EarlierYearOther,
}

/// Classify a unix timestamp into its section header kind relative
/// to `now_unix`. Local-timezone-aware (uses chrono::Local for both
/// timestamps). Caller supplies `now_unix` so tests can pin a
/// reference moment without messing with the system clock.
pub fn date_group_kind(unix_secs: i64, now_unix: i64) -> DateGroupKey {
    use chrono::{Datelike, TimeZone};
    let Some(then) = chrono::Local.timestamp_opt(unix_secs, 0).single() else {
        return DateGroupKey::EarlierYearOther;
    };
    let Some(now) = chrono::Local.timestamp_opt(now_unix, 0).single() else {
        return DateGroupKey::EarlierYearOther;
    };
    if then.year() == now.year() && then.ordinal() == now.ordinal() {
        return DateGroupKey::Today;
    }
    let yesterday = now.date_naive().pred_opt();
    if let Some(yest) = yesterday {
        if then.date_naive() == yest {
            return DateGroupKey::Yesterday;
        }
    }
    if then.year() == now.year() {
        DateGroupKey::SameYearOther
    } else {
        DateGroupKey::EarlierYearOther
    }
}

/// Stable-per-name 0..8 colour-class index for a label name. The
/// shell maps the index to its native palette (gtk's log view uses
/// the `log-c0`..`log-c7` CSS classes). DJB-ish string hash so the
/// mapping survives restarts without a per-label column.
pub fn label_color_class_index(name: &str) -> usize {
    let mut h: u32 = 5381;
    for b in name.bytes() {
        h = h.wrapping_mul(33).wrapping_add(b as u32);
    }
    (h as usize) % 8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_time_zero_shows_double_zero() {
        assert_eq!(format_time(Duration::ZERO), "00:00");
    }

    #[test]
    fn format_duration_brief_under_one_hour_is_m_ss() {
        assert_eq!(format_duration_brief(0), "0:00");
        assert_eq!(format_duration_brief(7), "0:07");
        assert_eq!(format_duration_brief(60), "1:00");
        assert_eq!(format_duration_brief(150), "2:30");
        assert_eq!(format_duration_brief(59 * 60 + 59), "59:59");
    }

    #[test]
    fn format_duration_brief_over_one_hour_is_h_mm_ss() {
        assert_eq!(format_duration_brief(3600), "1:00:00");
        assert_eq!(format_duration_brief(3661), "1:01:01");
        assert_eq!(format_duration_brief(2 * 3600 + 5 * 60 + 9), "2:05:09");
    }

    #[test]
    fn ellipsize_passes_through_when_within_limit() {
        assert_eq!(ellipsize("hi", 28), "hi");
        assert_eq!(ellipsize("exactly-five", 12), "exactly-five");
    }

    #[test]
    fn ellipsize_truncates_with_ellipsis_at_max_char_boundary() {
        assert_eq!(ellipsize("héllo world", 6), "héllo…");
    }

    #[test]
    fn ellipsize_counts_chars_not_bytes() {
        assert_eq!(ellipsize("éééééé", 4), "ééé…");
    }

    #[test]
    fn idle_hero_label_stopwatch_renders_double_zero() {
        assert_eq!(idle_hero_label(true, 600), "00:00");
        assert_eq!(idle_hero_label(true, 0), "00:00");
    }

    #[test]
    fn idle_hero_label_no_stopwatch_renders_target_as_hhmm() {
        assert_eq!(idle_hero_label(false, 10 * 60), "00:10");
        assert_eq!(idle_hero_label(false, 3600), "01:00");
    }

    #[test]
    fn box_breath_counter_label_stopwatch_shows_elapsed_only() {
        assert_eq!(
            box_breath_counter_label(Duration::from_secs(75), None),
            "01:15",
        );
    }

    #[test]
    fn box_breath_counter_label_fixed_shows_elapsed_over_target() {
        assert_eq!(
            box_breath_counter_label(Duration::from_secs(0), Some(Duration::from_secs(300))),
            "00:00 / 05:00",
        );
        assert_eq!(
            box_breath_counter_label(Duration::from_secs(90), Some(Duration::from_secs(300))),
            "01:30 / 05:00",
        );
    }

    #[test]
    fn format_hhmm_renders_zero_padded_hours_and_minutes() {
        assert_eq!(format_hhmm(0), "00:00");
        assert_eq!(format_hhmm(60), "00:01");
        assert_eq!(format_hhmm(60 * 60), "01:00");
        assert_eq!(format_hhmm(60 * 60 + 7 * 60), "01:07");
        // Seconds are truncated to the minute.
        assert_eq!(format_hhmm(59), "00:00");
        assert_eq!(format_hhmm(60 + 59), "00:01");
    }

    // ── overtime_button_label ─────────────────────────────────────────

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
    fn streak_key_partitions_into_zero_one_many() {
        assert_eq!(streak_key(0), StreakKey::Zero);
        assert_eq!(streak_key(1), StreakKey::One);
        assert_eq!(streak_key(2), StreakKey::Many(2));
        assert_eq!(streak_key(365), StreakKey::Many(365));
    }

    #[test]
    fn session_count_key_one_vs_many() {
        assert_eq!(session_count_key(0), SessionCountKey::Many(0));
        assert_eq!(session_count_key(1), SessionCountKey::One);
        assert_eq!(session_count_key(2), SessionCountKey::Many(2));
        assert_eq!(session_count_key(42), SessionCountKey::Many(42));
    }

    #[test]
    fn log_card_minutes_rounds_to_nearest_and_floors_at_one() {
        assert_eq!(log_card_minutes(0), 1);
        assert_eq!(log_card_minutes(29), 1, "29s rounds to 0 min, floor to 1");
        assert_eq!(log_card_minutes(30), 1);
        assert_eq!(log_card_minutes(60), 1);
        assert_eq!(log_card_minutes(89), 1, "89s rounds to 1.48 → 1 min");
        assert_eq!(log_card_minutes(90), 2);
        assert_eq!(log_card_minutes(60 * 15), 15);
        assert_eq!(log_card_minutes(-100), 1, "negative clamps to 0 then floors");
    }

    #[test]
    fn mini_stat_or_dash_renders_zero_as_endash() {
        assert_eq!(mini_stat_or_dash(0), "–");
        assert_eq!(mini_stat_or_dash(1), "1");
        assert_eq!(mini_stat_or_dash(42), "42");
    }

    #[test]
    fn synced_ago_key_partitions_at_minute_hour_day() {
        assert_eq!(synced_ago_key(0), SyncedAgoKey::JustNow);
        assert_eq!(synced_ago_key(59), SyncedAgoKey::JustNow);
        assert_eq!(synced_ago_key(60), SyncedAgoKey::Minutes(1));
        assert_eq!(synced_ago_key(3599), SyncedAgoKey::Minutes(59));
        assert_eq!(synced_ago_key(3600), SyncedAgoKey::Hours(1));
        assert_eq!(synced_ago_key(86_399), SyncedAgoKey::Hours(23));
        assert_eq!(synced_ago_key(86_400), SyncedAgoKey::Days(1));
        assert_eq!(synced_ago_key(7 * 86_400), SyncedAgoKey::Days(7));
    }

    #[test]
    fn synced_ago_key_clamps_negative_to_just_now() {
        assert_eq!(synced_ago_key(-30), SyncedAgoKey::JustNow);
        assert_eq!(synced_ago_key(i64::MIN), SyncedAgoKey::JustNow);
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

    // ── db_open_failure_key ─────────────────────────────────────────

    #[test]
    fn db_open_failure_key_schema_too_new_carries_versions() {
        let err = crate::db::DbError::SchemaVersionTooNew { db: 5, build: 2 };
        match db_open_failure_key(&err) {
            DbOpenFailureKey::SchemaTooNew { db, build } => {
                assert_eq!(db, 5);
                assert_eq!(build, 2);
            }
            other => panic!("expected SchemaTooNew, got {other:?}"),
        }
    }

    #[test]
    fn db_open_failure_key_other_variants_collapse_to_other() {
        let csv_err = crate::db::DbError::Csv("bad".to_string());
        assert_eq!(db_open_failure_key(&csv_err), DbOpenFailureKey::Other);
        let dup_err = crate::db::DbError::DuplicateLabel("focus".to_string());
        assert_eq!(db_open_failure_key(&dup_err), DbOpenFailureKey::Other);
    }

    // ── session_save_failure_log_message ────────────────────────────

    #[test]
    fn session_save_failure_db_unopened_log_message_format() {
        let msg = session_save_failure_log_message(
            SessionSaveFailureKind::DbUnopened,
            "with_db_blocking_mut returned None",
        );
        assert!(
            msg.starts_with("session_save_failed:"),
            "log line must use greppable prefix; got {msg:?}"
        );
        assert!(msg.contains("db unopened"));
        assert!(msg.contains("with_db_blocking_mut returned None"));
    }

    #[test]
    fn session_save_failure_storage_log_message_format() {
        let msg = session_save_failure_log_message(
            SessionSaveFailureKind::StorageError,
            "SqliteFailure(SQLITE_FULL): disk full",
        );
        assert!(msg.starts_with("session_save_failed:"));
        assert!(msg.contains("storage error"));
        assert!(msg.contains("SQLITE_FULL"));
        assert!(msg.contains("disk full"));
    }

    #[test]
    fn session_save_failure_kinds_distinguishable_in_log() {
        let a = session_save_failure_log_message(SessionSaveFailureKind::DbUnopened, "x");
        let b = session_save_failure_log_message(SessionSaveFailureKind::StorageError, "x");
        assert_ne!(a, b, "kinds must produce different log lines");
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
        assert_eq!(parts.timing, TimingKey::Stopwatch);
        assert_eq!(parts.label_uuid, None);
        assert_eq!(parts.bells, None);
    }

    #[test]
    fn preset_subtitle_parts_timer_duration_in_minutes() {
        // 600 s = 10 min — duration arrives in minutes.
        let json = cfg_to_json(&timer_cfg(false, 600));
        let parts = preset_subtitle_parts(&json).unwrap();
        assert_eq!(parts.timing, TimingKey::Duration { mins: 10 });
    }

    #[test]
    fn preset_subtitle_parts_box_breath_stopwatch_carries_phase_durations() {
        let json = cfg_to_json(&box_breath_cfg(true, 0, 4, 4, 4, 4));
        let parts = preset_subtitle_parts(&json).unwrap();
        assert_eq!(
            parts.timing,
            TimingKey::BoxBreath {
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
            TimingKey::BoxBreath {
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

        // Enabled with one → BellsCountKey::One.
        cfg.interval_bells = PresetIntervalBells { enabled: true, bells: vec![one_bell.clone()] };
        let json = cfg_to_json(&cfg);
        assert_eq!(
            preset_subtitle_parts(&json).unwrap().bells,
            Some(BellsCountKey::One)
        );

        // Enabled with three → BellsCountKey::Many(3).
        cfg.interval_bells = PresetIntervalBells {
            enabled: true,
            bells: vec![one_bell.clone(), one_bell.clone(), one_bell.clone()],
        };
        let json = cfg_to_json(&cfg);
        assert_eq!(
            preset_subtitle_parts(&json).unwrap().bells,
            Some(BellsCountKey::Many(3))
        );
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
    fn parse_hms_duration_rejects_overflowing_input() {
        // Pasted Insight Timer row with a u64-overflowing minutes
        // field used to panic in debug builds (overflow-checks on)
        // and wrap silently in release. Now: None.
        assert_eq!(parse_hms_duration("99999999999999999999:59:59"), None);
        assert_eq!(parse_hms_duration("18446744073709551615:1"), None);
        // h * 3600 overflows even when h itself fits in u64 — needs
        // the intermediate checked_mul to catch it. u64::MAX / 3600
        // ≈ 5.12e15; any h above that wraps without checked_mul.
        assert_eq!(parse_hms_duration("9999999999999999:0:0"), None);
        // Negative seconds via "-1.5" parses as a valid f64 but
        // shouldn't survive the as-u64 conversion either.
        assert_eq!(parse_hms_duration("1:-1.5"), None);
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

    #[test]
    fn date_group_key_renders_local_yyyy_mm_dd() {
        // 2025-04-17 12:00 UTC will be 2025-04-17 in any tz between
        // -12 and +12 hours away. The test isn't strict on the
        // exact local day; it just checks the format.
        let key = date_group_key(1_744_891_200);
        // YYYY-MM-DD is 10 chars with dashes at fixed positions.
        assert_eq!(key.len(), 10);
        assert_eq!(&key[4..5], "-");
        assert_eq!(&key[7..8], "-");
    }

    #[test]
    fn format_time_of_day_renders_local_hh_mm() {
        let s = format_time_of_day(1_744_891_200);
        assert_eq!(s.len(), 5);
        assert_eq!(&s[2..3], ":");
        // 24-hour HH range.
        let hh: u32 = s[..2].parse().unwrap();
        let mm: u32 = s[3..].parse().unwrap();
        assert!(hh < 24);
        assert!(mm < 60);
    }

    #[test]
    fn date_group_kind_classifies_today_yesterday_and_other() {
        // Pin a reference moment: 2025-04-17 12:00 UTC. Build the
        // expected unix offsets by working in seconds.
        let now_unix = 1_744_891_200_i64;
        let day = 86_400_i64;
        // Same instant → Today.
        assert_eq!(date_group_kind(now_unix, now_unix), DateGroupKey::Today);
        // ~12 hours ago might be the same local day or yesterday
        // depending on test machine TZ; either is fine.
        let result_12h = date_group_kind(now_unix - 12 * 3600, now_unix);
        assert!(matches!(result_12h, DateGroupKey::Today | DateGroupKey::Yesterday));
        // Two days ago → SameYearOther (April still).
        assert_eq!(
            date_group_kind(now_unix - 2 * day, now_unix),
            DateGroupKey::SameYearOther,
        );
        // ~400 days ago → EarlierYearOther (crosses calendar year).
        assert_eq!(
            date_group_kind(now_unix - 400 * day, now_unix),
            DateGroupKey::EarlierYearOther,
        );
    }

    #[test]
    fn label_color_index_stays_in_zero_to_seven_inclusive() {
        for name in &["", "a", "Meditation", "Box Breath", "🍵", "x".repeat(200).as_str()] {
            let idx = label_color_class_index(name);
            assert!(idx < 8, "label_color_class_index out of range for {name:?}: {idx}");
        }
    }

    #[test]
    fn label_color_index_is_deterministic_per_name() {
        assert_eq!(
            label_color_class_index("Meditation"),
            label_color_class_index("Meditation"),
        );
        assert_eq!(
            label_color_class_index("Box Breath"),
            label_color_class_index("Box Breath"),
        );
    }

    #[test]
    fn label_color_index_differs_across_some_names() {
        // Not a strict requirement (hash collisions in 8 slots are
        // expected), but a spot-check that the function isn't a
        // constant.
        use std::collections::HashSet;
        let names = ["Meditation", "Box Breath", "Guided", "Sit", "Pranayama"];
        let indices: HashSet<usize> = names
            .iter()
            .map(|n| label_color_class_index(n))
            .collect();
        assert!(indices.len() >= 2, "expected at least 2 distinct indices for {names:?}");
    }
}
